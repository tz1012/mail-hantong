//! Fixed Gmail mutation transport. Core must provide native-review-bound input.
//! No retry occurs here, including after an ambiguous provider response.
use super::*;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use hantong_contracts::google_write::GoogleOperation;
use hantong_contracts::google_write::{ComposeSnapshot, MAX_ATTACHMENT_BYTES, MessageLabelChange};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GmailWriteOutcome {
    Accepted,
    Rejected,
    Unknown,
}

pub struct GoogleAttachmentData(SensitiveBuffer);
impl std::fmt::Debug for GoogleAttachmentData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GoogleAttachmentData(<redacted>)")
    }
}
impl GoogleAttachmentData {
    pub(super) fn into_bytes(self) -> SensitiveBuffer {
        self.0
    }
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl SecretStore {
    /// Only the native Core send coordinator may call this after its durable claim.
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    pub fn send_google_message(
        &self,
        binding: &GoogleAccountBinding,
        sender: &str,
        compose: &ComposeSnapshot,
        review_id: &[u8; 32],
        utc_seconds: u64,
        cancel: &AtomicBool,
    ) -> Result<GmailWriteOutcome, ErrorCode> {
        if compose.draft().account_id() != binding.account_id {
            return Err(ErrorCode::ScopeDenied);
        }
        let mime = compose::encode(compose, sender, review_id, utc_seconds)?;
        self.with_google_access(
            binding,
            cancel,
            GoogleOperation::Send,
            |transport, access, verified_sender, deadline| {
                if sender != verified_sender {
                    return Err(ErrorCode::AuthRequired);
                }
                send(transport, &mime, access, deadline, cancel)
            },
        )
    }
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    pub fn create_google_draft(
        &self,
        binding: &GoogleAccountBinding,
        sender: &str,
        compose: &ComposeSnapshot,
        review_id: &[u8; 32],
        utc_seconds: u64,
        cancel: &AtomicBool,
    ) -> Result<GmailWriteOutcome, ErrorCode> {
        if compose.draft().account_id() != binding.account_id {
            return Err(ErrorCode::ScopeDenied);
        }
        let mime = compose::encode(compose, sender, review_id, utc_seconds)?;
        self.with_google_access(
            binding,
            cancel,
            GoogleOperation::SaveDraft,
            |transport, access, verified_sender, deadline| {
                if sender != verified_sender {
                    return Err(ErrorCode::AuthRequired);
                }
                post_raw(transport, &mime, access, deadline, cancel, true)
            },
        )
    }
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    pub fn change_google_labels(
        &self,
        binding: &GoogleAccountBinding,
        change: &MessageLabelChange,
        cancel: &AtomicBool,
    ) -> Result<GmailWriteOutcome, ErrorCode> {
        if change.account_id() != binding.account_id {
            return Err(ErrorCode::ScopeDenied);
        }
        self.with_google_access(
            binding,
            cancel,
            GoogleOperation::ChangeLabels,
            |transport, access, _, deadline| {
                change_labels(transport, change, access, deadline, cancel)
            },
        )
    }
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    pub fn trash_google_message(
        &self,
        binding: &GoogleAccountBinding,
        message_id: &str,
        cancel: &AtomicBool,
    ) -> Result<GmailWriteOutcome, ErrorCode> {
        if binding.account_id.is_empty() || !http::provider_id(message_id) {
            return Err(ErrorCode::InvalidInput);
        }
        self.with_google_access(
            binding,
            cancel,
            GoogleOperation::TrashMessage,
            |transport, access, _, deadline| trash(transport, message_id, access, deadline, cancel),
        )
    }
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    pub fn fetch_google_attachment(
        &self,
        binding: &GoogleAccountBinding,
        message_id: &str,
        attachment_id: &str,
        expected_size: usize,
        cancel: &AtomicBool,
    ) -> Result<GoogleAttachmentData, ErrorCode> {
        if !http::provider_id(message_id)
            || !http::attachment_id(attachment_id)
            || expected_size > MAX_ATTACHMENT_BYTES
        {
            return Err(ErrorCode::InvalidInput);
        }
        self.with_google_access(
            binding,
            cancel,
            GoogleOperation::Read,
            |transport, access, _, deadline| {
                attachment(
                    transport,
                    message_id,
                    attachment_id,
                    expected_size,
                    access,
                    deadline,
                    cancel,
                )
            },
        )
    }
}

fn json_body(value: Value) -> Result<SensitiveBuffer, ErrorCode> {
    let json = SecretJson(value);
    serde_json::to_vec(&json.0)
        .map(SensitiveBuffer)
        .map_err(|_| ErrorCode::InternalError)
}
fn request_once(
    transport: &impl http::Transport,
    path: &str,
    body: &SensitiveBuffer,
    access: &SecretText,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<http::Response, ErrorCode> {
    let request = http::Request {
        host: http::Host::Gmail,
        path,
        post: Some(body.as_slice()),
        bearer: Some(access.text()),
        limit: 65536,
        deadline,
        cancel,
    };
    http::validate(&request)?;
    transport.request(request)
}
fn outcome(
    response: Result<http::Response, ErrorCode>,
    draft: bool,
    expected_message: Option<&str>,
) -> GmailWriteOutcome {
    let Ok(response) = response else {
        return GmailWriteOutcome::Unknown;
    };
    if matches!(response.status, 400 | 401 | 403 | 404 | 413 | 429) {
        return GmailWriteOutcome::Rejected;
    }
    if !matches!(response.status, 200 | 201) {
        return GmailWriteOutcome::Unknown;
    }
    let Ok(json) = SecretJson::parse(response.body.as_slice()) else {
        return GmailWriteOutcome::Unknown;
    };
    let valid_id =
        |value: Option<&Value>| value.and_then(Value::as_str).is_some_and(http::provider_id);
    let valid = if draft {
        valid_id(json.0.get("id")) && valid_id(json.0.get("message").and_then(|m| m.get("id")))
    } else {
        valid_id(json.0.get("id"))
            && expected_message
                .is_none_or(|id| json.0.get("id").and_then(Value::as_str) == Some(id))
    };
    if valid {
        GmailWriteOutcome::Accepted
    } else {
        GmailWriteOutcome::Unknown
    }
}
fn send(
    transport: &impl http::Transport,
    mime: &compose::GoogleMimeMessage,
    access: &SecretText,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<GmailWriteOutcome, ErrorCode> {
    post_raw(transport, mime, access, deadline, cancel, false)
}
fn post_raw(
    transport: &impl http::Transport,
    mime: &compose::GoogleMimeMessage,
    access: &SecretText,
    deadline: Instant,
    cancel: &AtomicBool,
    draft: bool,
) -> Result<GmailWriteOutcome, ErrorCode> {
    let raw = crypto::base64url(mime.as_bytes());
    let value = if draft {
        serde_json::json!({"message":{"raw":raw}})
    } else {
        serde_json::json!({"raw":raw})
    };
    let body = json_body(value)?;
    let path = if draft {
        "/gmail/v1/users/me/drafts"
    } else {
        "/gmail/v1/users/me/messages/send"
    };
    Ok(outcome(
        request_once(transport, path, &body, access, deadline, cancel),
        draft,
        None,
    ))
}
fn change_labels(
    transport: &impl http::Transport,
    change: &MessageLabelChange,
    access: &SecretText,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<GmailWriteOutcome, ErrorCode> {
    let path = format!("/gmail/v1/users/me/messages/{}/modify", change.message_id());
    let body = json_body(
        serde_json::json!({"addLabelIds":change.add(), "removeLabelIds":change.remove()}),
    )?;
    Ok(outcome(
        request_once(transport, &path, &body, access, deadline, cancel),
        false,
        Some(change.message_id()),
    ))
}
fn trash(
    transport: &impl http::Transport,
    message_id: &str,
    access: &SecretText,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<GmailWriteOutcome, ErrorCode> {
    if !http::provider_id(message_id) {
        return Err(ErrorCode::InvalidInput);
    }
    let path = format!("/gmail/v1/users/me/messages/{message_id}/trash");
    let request = http::Request {
        host: http::Host::Gmail,
        path: &path,
        post: None,
        bearer: Some(access.text()),
        limit: 65536,
        deadline,
        cancel,
    };
    http::validate(&request)?;
    Ok(outcome(transport.request(request), false, Some(message_id)))
}
pub(super) fn attachment(
    transport: &impl http::Transport,
    message: &str,
    id: &str,
    expected_size: usize,
    access: &SecretText,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<GoogleAttachmentData, ErrorCode> {
    let trace = diagnostics::Trace::new();
    trace.step(diagnostics::Stage::AttachmentIdentifierLength, || {
        if id.is_empty() || id.len() > http::MAX_ATTACHMENT_ID_BYTES {
            Err(ErrorCode::InvalidInput)
        } else {
            Ok(())
        }
    })?;
    let path = format!("/gmail/v1/users/me/messages/{message}/attachments/{id}");
    let request = http::Request {
        host: http::Host::Gmail,
        path: &path,
        post: None,
        bearer: Some(access.text()),
        limit: 2 * 1024 * 1024,
        deadline,
        cancel,
    };
    trace.step(diagnostics::Stage::AttachmentRoute, || {
        if !http::provider_id(message) || !http::attachment_id(id) {
            Err(ErrorCode::InvalidInput)
        } else {
            http::validate(&request)
        }
    })?;
    let response = trace.step(diagnostics::Stage::AttachmentTransport, || {
        transport.request(request)
    })?;
    let json = trace.step(diagnostics::Stage::AttachmentResponse, || {
        http::success(response.status)?;
        SecretJson::parse(response.body.as_slice())
    })?;
    if json.0.get("size").and_then(Value::as_u64) != Some(expected_size as u64) {
        return Err(ErrorCode::Conflict);
    }
    let data = json
        .0
        .get("data")
        .and_then(Value::as_str)
        .ok_or(ErrorCode::InvalidInput)?;
    let bytes = SensitiveBuffer(trace.step(diagnostics::Stage::AttachmentDecode, || {
        crypto::decode64(data, expected_size.min(MAX_ATTACHMENT_BYTES))
    })?);
    if bytes.0.len() != expected_size {
        return Err(ErrorCode::Conflict);
    }
    Ok(GoogleAttachmentData(bytes))
}

#[cfg(test)]
#[path = "write_tests.rs"]
mod tests;
