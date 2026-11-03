use super::*;
use hantong_contracts::message_body::{render_message_text, sanitize_terminal_text};
const BASE: &str = "/gmail/v1/users/me/";
const MAX_BODY_TEXT_BYTES: usize = 65536;
// The shared MIME renderer includes this synthetic, bounded header in its 1MiB budget.
const MAX_TEXT_PART_BYTES: usize = hantong_contracts::message_body::MAX_BODY_BYTES - 2048;

struct RenderedBody {
    text: String,
    truncated: bool,
}
fn bounded_body(mut text: String, inherited_truncation: bool) -> RenderedBody {
    let mut end = text.len().min(MAX_BODY_TEXT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = inherited_truncation || end < text.len();
    text.truncate(end);
    RenderedBody { text, truncated }
}
fn get(
    transport: &impl http::Transport,
    path: &str,
    access: &SecretText,
    limit: usize,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<Value, ErrorCode> {
    http::check(deadline, cancel)?;
    let r = transport.request(http::Request {
        host: http::Host::Gmail,
        path,
        post: None,
        bearer: Some(access.text()),
        limit,
        deadline,
        cancel,
    })?;
    http::success(r.status)?;
    serde_json::from_slice(r.body.as_slice()).map_err(|_| ErrorCode::InvalidInput)
}
pub(super) fn fetch(
    transport: &impl http::Transport,
    binding: &GoogleAccountBinding,
    request: &GoogleSyncRequest,
    access: &SecretText,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<GooglePage, ErrorCode> {
    request.validate()?;
    if request.account_id != binding.account_id {
        return Err(ErrorCode::ScopeDenied);
    }
    let labels = get(
        transport,
        &format!("{BASE}labels"),
        access,
        256 * 1024,
        deadline,
        cancel,
    )?;
    let labels = labels
        .get("labels")
        .and_then(Value::as_array)
        .ok_or(ErrorCode::InvalidInput)?;
    if labels.len() > 1024 {
        return Err(ErrorCode::QuotaExceeded);
    }
    let labels = labels
        .iter()
        .map(|v| {
            Ok(GoogleLabel {
                id: required(v, "id")?.into(),
                name: display(required(v, "name")?, 1024),
                kind: required(v, "type")?.into(),
            })
        })
        .collect::<Result<Vec<_>, ErrorCode>>()?;
    let mut path = format!("{BASE}messages?maxResults={}", request.limit);
    if let Some(label) = &request.label_id {
        path.push_str("&labelIds=");
        path.push_str(&oauth::encode(label));
    }
    if let Some(token) = &request.page_token {
        path.push_str("&pageToken=");
        path.push_str(&oauth::encode(token));
    }
    let list = if let Some(id) = &request.message_id {
        serde_json::json!({"messages":[{"id":id}],"resultSizeEstimate":1})
    } else {
        get(transport, &path, access, 65536, deadline, cancel)?
    };
    let empty = Vec::new();
    let rows = match list.get("messages") {
        None => &empty,
        Some(Value::Array(rows)) => rows,
        _ => return Err(ErrorCode::InvalidInput),
    };
    if rows.len() > request.limit as usize {
        return Err(ErrorCode::QuotaExceeded);
    }
    let mut messages = Vec::with_capacity(rows.len());
    for row in rows {
        let id = required(row, "id")?;
        let thread_id = if request.message_id.is_some() {
            None
        } else {
            Some(required(row, "threadId")?)
        };
        // Validate opaque IDs before constructing a path component.
        if id.is_empty()
            || id.len() > 256
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(ErrorCode::InvalidInput);
        }
        let path = format!("{BASE}messages/{}?format=full", oauth::encode(id));
        let (detail, too_large) = match get(
            transport,
            &path,
            access,
            2 * 1024 * 1024,
            deadline,
            cancel,
        ) {
            Ok(v) => (v, false),
            Err(ErrorCode::QuotaExceeded) => {
                let path = format!(
                    "{BASE}messages/{}?format=metadata&metadataHeaders=Subject&metadataHeaders=From",
                    oauth::encode(id)
                );
                (
                    get(transport, &path, access, 131072, deadline, cancel)?,
                    true,
                )
            }
            Err(e) => return Err(e),
        };
        let mut message = parse_message(&detail, too_large)?;
        if message.id != id || thread_id.is_some_and(|thread| message.thread_id != thread) {
            return Err(ErrorCode::AuthRequired);
        }
        if request.verify_attachments {
            if too_large {
                return Err(ErrorCode::QuotaExceeded);
            }
            message.attachment_checks = Some(verify_attachments(
                transport, &detail, id, access, deadline, cancel,
            )?);
        }
        if too_large {
            message.body_error = Some(ErrorCode::QuotaExceeded.as_str().into());
            message.body_text = None;
            message.body_truncated = true;
        }
        message.validate()?;
        messages.push(message);
    }
    let next_page_token = match list.get("nextPageToken") {
        None => None,
        Some(Value::String(s)) => Some(s.clone()),
        _ => return Err(ErrorCode::InvalidInput),
    };
    let page = GooglePage {
        account_id: binding.account_id.clone(),
        subject: binding.subject.clone(),
        labels,
        messages,
        next_page_token,
        result_size_estimate: match list.get("resultSizeEstimate") {
            None => 0,
            Some(value) => value.as_u64().ok_or(ErrorCode::InvalidInput)?,
        },
    };
    page.validate()?;
    Ok(page)
}
fn required<'a>(v: &'a Value, k: &str) -> Result<&'a str, ErrorCode> {
    v.get(k)
        .and_then(Value::as_str)
        .ok_or(ErrorCode::InvalidInput)
}
fn display(value: &str, max: usize) -> String {
    let mut value = sanitize_terminal_text(value);
    if value.len() > max {
        let mut end = max;
        while !value.is_char_boundary(end) {
            end -= 1
        }
        value.truncate(end)
    }
    value
}
fn header<'a>(payload: &'a Value, name: &str) -> Option<&'a str> {
    payload.get("headers")?.as_array()?.iter().find_map(|h| {
        (h.get("name")?.as_str()?.eq_ignore_ascii_case(name))
            .then(|| h.get("value")?.as_str())
            .flatten()
    })
}
pub(super) fn parse_message(v: &Value, metadata_only: bool) -> Result<GoogleMessage, ErrorCode> {
    let payload = v.get("payload").unwrap_or(&Value::Null);
    let label_ids = v
        .get("labelIds")
        .map(|value| value.as_array().ok_or(ErrorCode::InvalidInput))
        .transpose()?
        .map(|a| {
            a.iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .ok_or(ErrorCode::InvalidInput)
                })
                .collect()
        })
        .transpose()?
        .unwrap_or_default();
    let rendered = if metadata_only {
        Err(ErrorCode::QuotaExceeded)
    } else {
        let mut parts = 0;
        body(payload, 0, &mut parts).and_then(|body| body.ok_or(ErrorCode::CapabilityUnsupported))
    };
    let (body_text, body_error, body_truncated) = match rendered {
        Ok(v) => (Some(v.text), None, v.truncated),
        Err(e) => (None, Some(e.as_str().into()), e == ErrorCode::QuotaExceeded),
    };
    let subject =
        Some(display(header(payload, "Subject").unwrap_or(""), 8192)).filter(|v| !v.is_empty());
    let sender =
        Some(display(header(payload, "From").unwrap_or(""), 8192)).filter(|v| !v.is_empty());
    let to = Some(display(header(payload, "To").unwrap_or(""), 8192)).filter(|v| !v.is_empty());
    let cc = Some(display(header(payload, "Cc").unwrap_or(""), 8192)).filter(|v| !v.is_empty());
    let date = Some(display(header(payload, "Date").unwrap_or(""), 8192)).filter(|v| !v.is_empty());
    let headers = if subject.is_some()
        || sender.is_some()
        || to.is_some()
        || cc.is_some()
        || date.is_some()
    {
        Some(hantong_contracts::message_body::MessageEnvelope::new(
            subject.clone(),
            sender.clone(),
            to,
            cc,
            date,
        )?)
    } else {
        None
    };
    Ok(GoogleMessage {
        attachment_checks: None,
        id: required(v, "id")?.into(),
        thread_id: required(v, "threadId")?.into(),
        internal_date_ms: required(v, "internalDate")?
            .parse()
            .map_err(|_| ErrorCode::InvalidInput)?,
        label_ids,
        subject: subject.unwrap_or_default(),
        sender: sender.unwrap_or_default(),
        headers,
        snippet: display(v.get("snippet").and_then(Value::as_str).unwrap_or(""), 8192),
        body_text,
        body_error,
        body_truncated,
        size: v.get("sizeEstimate").and_then(Value::as_u64).unwrap_or(0),
    })
}
fn body(
    part: &Value,
    depth: usize,
    visited: &mut usize,
) -> Result<Option<RenderedBody>, ErrorCode> {
    *visited += 1;
    if depth > 8 || *visited > 128 {
        return Err(ErrorCode::QuotaExceeded);
    }
    if part
        .get("filename")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty())
        || header(part, "Content-Disposition").is_some_and(|s| {
            s.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("attachment")
        })
    {
        return Ok(None);
    }
    let mime = part.get("mimeType").and_then(Value::as_str).unwrap_or("");
    if mime.starts_with("multipart/") {
        let parts = part
            .get("parts")
            .and_then(Value::as_array)
            .ok_or(ErrorCode::CapabilityUnsupported)?;
        if parts.len() > 128 {
            return Err(ErrorCode::QuotaExceeded);
        }
        if mime == "multipart/alternative" {
            for wanted in ["text/plain", "text/html"] {
                for p in parts {
                    if p.get("mimeType").and_then(Value::as_str) == Some(wanted)
                        && let Some(text) = body(p, depth + 1, visited)?
                    {
                        return Ok(Some(text));
                    }
                }
            }
        }
        let mut out = String::new();
        let mut truncated = false;
        for p in parts {
            if let Some(rendered) = body(p, depth + 1, visited)? {
                if !out.is_empty() {
                    out.push('\n')
                }
                out.push_str(&rendered.text);
                let bounded = bounded_body(out, truncated || rendered.truncated);
                out = bounded.text;
                truncated = bounded.truncated;
            }
        }
        return Ok((!out.is_empty()).then_some(RenderedBody {
            text: out,
            truncated,
        }));
    }
    if !matches!(mime, "text/plain" | "text/html") {
        return Ok(None);
    }
    let body = part.get("body").ok_or(ErrorCode::CapabilityUnsupported)?;
    if body.get("attachmentId").is_some() {
        return Ok(None);
    }
    if body
        .get("size")
        .and_then(Value::as_u64)
        .is_some_and(|s| s > MAX_TEXT_PART_BYTES as u64)
    {
        return Err(ErrorCode::QuotaExceeded);
    }
    let data = body.get("data").and_then(Value::as_str).unwrap_or("");
    let data = crypto::decode64(data, MAX_TEXT_PART_BYTES)?;
    let content_type = header(part, "Content-Type").unwrap_or(mime);
    if content_type.len() > 1024 || content_type.chars().any(char::is_control) {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    let mut raw = format!("Content-Type: {content_type}\r\n\r\n").into_bytes();
    raw.extend_from_slice(&data);
    let rendered = render_message_text(&raw)?;
    Ok(Some(bounded_body(rendered, false)))
}

fn verify_attachments(
    transport: &impl http::Transport,
    detail: &Value,
    message: &str,
    access: &SecretText,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<Vec<hantong_contracts::google::GoogleAttachmentCheck>, ErrorCode> {
    use hantong_contracts::{
        google::GoogleAttachmentCheck,
        google_write::{MAX_ATTACHMENT_BYTES, MAX_ATTACHMENT_COUNT},
    };
    fn collect<'a>(
        part: &'a Value,
        depth: usize,
        visited: &mut usize,
        out: &mut Vec<&'a Value>,
    ) -> Result<(), ErrorCode> {
        *visited += 1;
        if depth > 8 || *visited > 128 {
            return Err(ErrorCode::QuotaExceeded);
        }
        if part
            .get("filename")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
        {
            out.push(part);
        }
        if let Some(children) = part.get("parts") {
            for child in children.as_array().ok_or(ErrorCode::InvalidInput)? {
                collect(child, depth + 1, visited, out)?;
            }
        }
        Ok(())
    }
    let mut parts = vec![];
    collect(
        detail.get("payload").ok_or(ErrorCode::InvalidInput)?,
        0,
        &mut 0,
        &mut parts,
    )?;
    if parts.len() > MAX_ATTACHMENT_COUNT {
        return Err(ErrorCode::QuotaExceeded);
    }
    let mut total = 0usize;
    let mut checks = vec![];
    for part in parts {
        let body = part.get("body").ok_or(ErrorCode::InvalidInput)?;
        let size = body
            .get("size")
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or(ErrorCode::InvalidInput)?;
        total = total.checked_add(size).ok_or(ErrorCode::QuotaExceeded)?;
        if total > MAX_ATTACHMENT_BYTES {
            return Err(ErrorCode::QuotaExceeded);
        }
        let mut check = GoogleAttachmentCheck {
            filename: required(part, "filename")?.into(),
            media_type: required(part, "mimeType")?.into(),
            size,
            sha256: [0; 32],
        };
        diagnostics::Trace::new()
            .step(diagnostics::Stage::AttachmentMetadata, || check.validate())?;
        let bytes = if let Some(id) = body.get("attachmentId") {
            let id = id.as_str().ok_or(ErrorCode::InvalidInput)?;
            write::attachment(transport, message, id, size, access, deadline, cancel)?.into_bytes()
        } else {
            SensitiveBuffer(crypto::decode64(required(body, "data")?, size)?)
        };
        if bytes.as_slice().len() != size {
            return Err(ErrorCode::Conflict);
        }
        check.sha256 = crypto::sha256(bytes.as_slice())?;
        checks.push(check);
    }
    Ok(checks)
}

#[cfg(test)]
mod attachment_checks_tests {
    use super::super::tests::Fake;
    use super::*;
    fn detail(size: u64, id: Option<&str>) -> Value {
        let mut body = serde_json::json!({"size":size,"data":crypto::base64url(b"synthetic")});
        if let Some(id) = id {
            body.as_object_mut().unwrap().remove("data");
            body["attachmentId"] = id.into();
        }
        serde_json::json!({"payload":{"parts":[{"filename":"fixture.txt","mimeType":"text/plain","body":body}]}})
    }
    #[test]
    fn gmail_write_receive_verifies_inline_and_downloaded_bytes_and_rejects_size_or_path_substitution()
     {
        let access = SecretText::from_string("synthetic-access".into()).unwrap();
        let stop = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(20);
        let fake = Fake::new(vec![]);
        let inline = verify_attachments(
            &fake,
            &detail(9, None),
            "message1",
            &access,
            deadline,
            &stop,
        )
        .unwrap();
        assert_eq!(inline.len(), 1);
        assert_eq!(inline[0].sha256, crypto::sha256(b"synthetic").unwrap());
        assert!(fake.calls.lock().unwrap().is_empty());
        let fake = Fake::new(vec![
            serde_json::json!({"size":9,"data":crypto::base64url(b"synthetic")}),
        ]);
        let downloaded = verify_attachments(
            &fake,
            &detail(9, Some("attachment1")),
            "message1",
            &access,
            deadline,
            &stop,
        )
        .unwrap();
        assert_eq!(inline, downloaded);
        assert_eq!(
            fake.calls.lock().unwrap()[0].1,
            "/gmail/v1/users/me/messages/message1/attachments/attachment1"
        );
        assert!(
            verify_attachments(
                &Fake::new(vec![]),
                &detail(8, None),
                "message1",
                &access,
                deadline,
                &stop
            )
            .is_err()
        );
        let fake = Fake::new(vec![]);
        assert!(
            verify_attachments(
                &fake,
                &detail(9, Some("../other")),
                "message1",
                &access,
                deadline,
                &stop
            )
            .is_err()
        );
        assert!(fake.calls.lock().unwrap().is_empty());
    }
}
