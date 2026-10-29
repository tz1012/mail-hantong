use crate::ReadResult;
use hantong_contracts::{
    ErrorCode,
    folders::{FolderCursor, FolderListRequest, FolderRefreshReceipt, FolderRefreshState},
};
use hantong_domain::{
    accounts::utc_millis_rfc3339,
    mailboxes::{ImapMailboxSnapshot, MailboxChildren, MailboxEncoding, SpecialUseRole},
};
use serde_json::{Value, json};

const MAX_RESULT: usize = 1_040_000;

pub fn render_folder_refresh_receipt(receipt: &FolderRefreshReceipt) -> Value {
    let (state, generation, error) = match receipt.state() {
        FolderRefreshState::Unresolved => ("unresolved", Value::Null, Value::Null),
        FolderRefreshState::Succeeded => ("succeeded", json!(receipt.generation()), Value::Null),
        FolderRefreshState::Failed => (
            "failed",
            Value::Null,
            json!(receipt.error().expect("validated receipt").as_str()),
        ),
    };
    json!({"schema_version":1,"ok":true,"account_id":receipt.account_id(),"execution_id":hantong_contracts::wire_hex::encode(receipt.execution_id()),"state":state,"generation":generation,"error_code":error})
}

pub fn render_folder_page(
    request: &FolderListRequest,
    generation: u64,
    account_version: u64,
    endpoint_version: u64,
    stored_at_utc_ms: i64,
    snapshot: &ImapMailboxSnapshot,
    binding_current: bool,
) -> Result<ReadResult<Value>, ErrorCode> {
    if generation == 0
        || generation > i64::MAX as u64
        || account_version == 0
        || account_version > i64::MAX as u64
        || endpoint_version == 0
        || endpoint_version > i64::MAX as u64
        || snapshot.account_id() != request.account_id()
        || snapshot.settings_version() != endpoint_version
    {
        return Err(ErrorCode::InvalidInput);
    }
    let all = snapshot.mailboxes();
    let start = if let Some(cursor) = request.cursor() {
        if cursor.generation() != generation {
            return Err(ErrorCode::CursorExpired);
        }
        let after = decode_hex(cursor.after_wire_hex())?;
        all.iter()
            .position(|item| item.name().wire_name() == after.as_slice())
            .ok_or(ErrorCode::CursorExpired)?
            + 1
    } else {
        0
    };
    let end = start
        .saturating_add(request.limit() as usize)
        .min(all.len());
    let rows: Vec<Value> = all[start..end].iter().map(|item| json!({
        "wire_hex": lower_hex(item.name().wire_name()), "display_name": item.name().display_name(),
        "encoding": match item.name().encoding() { MailboxEncoding::ModifiedUtf7 => "modified_utf7", MailboxEncoding::Utf8 => "utf8" },
        "delimiter": item.delimiter().map(|v| v.to_string()), "no_select": item.no_select(), "non_existent": item.non_existent(),
        "children": match item.children() { MailboxChildren::Unknown => "unknown", MailboxChildren::HasChildren => "has_children", MailboxChildren::HasNoChildren => "has_no_children" },
        "roles": item.roles().iter().map(|role| match role { SpecialUseRole::All=>"all",SpecialUseRole::Archive=>"archive",SpecialUseRole::Drafts=>"drafts",SpecialUseRole::Flagged=>"flagged",SpecialUseRole::Junk=>"junk",SpecialUseRole::Sent=>"sent",SpecialUseRole::Trash=>"trash" }).collect::<Vec<_>>()
    })).collect();
    let next_cursor = if end < all.len() {
        Some(
            FolderCursor::new(
                request.account_id(),
                generation,
                rows.last()
                    .and_then(|v| v.get("wire_hex"))
                    .and_then(Value::as_str)
                    .ok_or(ErrorCode::InternalError)?,
            )?
            .encode(),
        )
    } else {
        None
    };
    let observed_at = utc_millis_rfc3339(stored_at_utc_ms).map_err(|_| ErrorCode::InternalError)?;
    let completeness = if next_cursor.is_some() {
        "partial"
    } else {
        "complete"
    };
    let warnings = if binding_current {
        Vec::<&str>::new()
    } else {
        vec!["STALE_BINDING"]
    };
    let metadata = json!({"source":"local_store","cached":true,"account_id":request.account_id(),"generation":generation,"account_version":account_version,"endpoint_version":endpoint_version,"binding_current":binding_current,"special_use_complete":false});
    let result = json!({"schema_version":1,"ok":true,"items":rows,"next_cursor":next_cursor,"account_results":[{"account_id":request.account_id(),"status":completeness,"items":rows}],"completeness":completeness,"warnings":warnings,"observed_at":observed_at,"metadata":metadata});
    if serde_json::to_vec(&result)
        .map_err(|_| ErrorCode::InternalError)?
        .len()
        > MAX_RESULT
    {
        return Err(ErrorCode::QuotaExceeded);
    }
    ReadResult::new(result, Some((end - start) as u64)).map_err(|_| ErrorCode::InternalError)
}
fn decode_hex(value: &str) -> Result<Vec<u8>, ErrorCode> {
    if value.is_empty() || !value.len().is_multiple_of(2) {
        return Err(ErrorCode::InvalidInput);
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|p| {
            let h = hex(p[0])?;
            let l = hex(p[1])?;
            Ok((h << 4) | l)
        })
        .collect()
}
fn hex(v: u8) -> Result<u8, ErrorCode> {
    match v {
        b'0'..=b'9' => Ok(v - b'0'),
        b'a'..=b'f' => Ok(v - b'a' + 10),
        _ => Err(ErrorCode::InvalidInput),
    }
}
fn lower_hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 15) as usize] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_domain::{
        capabilities::ImapCapabilities,
        mailboxes::{ImapMailboxDescriptor, ImapMailboxName},
    };

    fn snapshot() -> ImapMailboxSnapshot {
        let names = [b"A".as_slice(), b"B".as_slice(), b"C".as_slice()];
        let items = names
            .iter()
            .map(|name| {
                ImapMailboxDescriptor::new(
                    ImapMailboxName::from_wire(name, MailboxEncoding::Utf8).unwrap(),
                    Some('/'),
                    false,
                    false,
                    MailboxChildren::HasNoChildren,
                    vec![],
                )
                .unwrap()
            })
            .collect();
        ImapMailboxSnapshot::new("acct", 4, ImapCapabilities::from_features([]), items).unwrap()
    }

    #[test]
    fn m203_folder_page_paginates_and_formats_fixed_milliseconds() {
        let first = render_folder_page(
            &FolderListRequest::new("acct", Some(2), None).unwrap(),
            1,
            2,
            4,
            1,
            &snapshot(),
            true,
        )
        .unwrap()
        .value();
        assert_eq!(first["items"].as_array().unwrap().len(), 2);
        assert_eq!(first["observed_at"], "1970-01-01T00:00:00.001Z");
        assert_eq!(first["metadata"]["special_use_complete"], false);
        let cursor = FolderCursor::parse(first["next_cursor"].as_str().unwrap()).unwrap();
        let last = render_folder_page(
            &FolderListRequest::new("acct", Some(2), Some(cursor)).unwrap(),
            1,
            2,
            4,
            1000,
            &snapshot(),
            false,
        )
        .unwrap()
        .value();
        assert_eq!(last["items"].as_array().unwrap().len(), 1);
        assert_eq!(last["next_cursor"], Value::Null);
        assert_eq!(last["warnings"], json!(["STALE_BINDING"]));
    }

    #[test]
    fn m203_folder_page_rejects_cursor_scope_generation_and_endpoint_mismatch() {
        let snap = snapshot();
        assert!(matches!(
            render_folder_page(
                &FolderListRequest::new("acct", None, None).unwrap(),
                1,
                2,
                5,
                1,
                &snap,
                true
            ),
            Err(ErrorCode::InvalidInput)
        ));
        let bad_generation = FolderCursor::new("acct", 2, "42").unwrap();
        assert!(matches!(
            render_folder_page(
                &FolderListRequest::new("acct", None, Some(bad_generation)).unwrap(),
                1,
                2,
                4,
                1,
                &snap,
                true
            ),
            Err(ErrorCode::CursorExpired)
        ));
        let bad_wire = FolderCursor::new("acct", 1, "ff").unwrap();
        assert!(matches!(
            render_folder_page(
                &FolderListRequest::new("acct", None, Some(bad_wire)).unwrap(),
                1,
                2,
                4,
                1,
                &snap,
                true
            ),
            Err(ErrorCode::CursorExpired)
        ));
        let other = FolderListRequest::new("other", None, None).unwrap();
        assert!(matches!(
            render_folder_page(&other, 1, 2, 4, 1, &snap, true),
            Err(ErrorCode::InvalidInput)
        ));
    }
}
