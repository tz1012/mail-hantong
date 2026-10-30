use crate::ReadResult;
use hantong_contracts::{ErrorCode, message_search::ImapMetadataSearchPage};
use serde_json::{Value, json};

const MAX_RESULT_BYTES: usize = 524_288;

/// Renders one local cache snapshot without interpreting its header bytes.
pub fn render_imap_metadata_search(
    page: &ImapMetadataSearchPage,
) -> Result<ReadResult<Value>, ErrorCode> {
    let items: Vec<Value> = page
        .items()
        .iter()
        .map(|item| {
            json!({
                "internal_message_id": item.internal_message_id(),
                "account_id": item.account_id(),
                "uid": item.uid(),
                "internal_date_utc_ms": item.internal_date_utc_ms(),
                "size_bytes": item.size_bytes(),
                "is_read": item.is_read(),
                "is_favorite": item.is_favorite(),
                "header_content_index": item.header_content_index()
            })
        })
        .collect();
    let headers: Vec<Value> = page
        .headers()
        .iter()
        .map(|header| {
            json!({
                "content_type": "header",
                "content": header.projection_json(),
                "byte_length": header.projection_json().len(),
                "truncated": false
            })
        })
        .collect();
    let observed = page
        .observed_at_utc_ms()
        .map(|value| {
            hantong_domain::accounts::utc_millis_rfc3339(value)
                .map_err(|_| ErrorCode::InternalError)
        })
        .transpose()?;
    let (status, warning) = if observed.is_some() {
        ("partial", "LOCAL_METADATA_ONLY")
    } else {
        ("unavailable", "LOCAL_CACHE_UNOBSERVED")
    };
    let mut warnings = vec![warning];
    if !page.binding_current() {
        warnings.push("STALE_BINDING");
    }
    let mut result = json!({
        "schema_version": 1,
        "ok": true,
        "items": items,
        "next_cursor": page.next_cursor().map(|cursor| cursor.encode()),
        "account_results": [{"account_id": page.account_id(), "status": status}],
        "completeness": status,
        "warnings": warnings,
        "observed_at": observed,
        "metadata": {"provider": "imap", "source": "local_store", "cached": true},
        "untrusted_content": headers
    });
    // Only the structural array is duplicated; header content remains once.
    result["account_results"][0]["items"] = result["items"].clone();
    let encoded = serde_json::to_vec(&result).map_err(|_| ErrorCode::InternalError)?;
    if encoded.len() > MAX_RESULT_BYTES {
        return Err(ErrorCode::QuotaExceeded);
    }
    ReadResult::new(result, Some(page.items().len() as u64)).map_err(|_| ErrorCode::InternalError)
}
