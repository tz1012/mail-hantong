//! CLI syntax for one bounded IMAP metadata search request.

use crate::{ErrorCode, Lang, emit_error, resource};
use hantong_contracts::message_search::{
    ImapMetadataFilter, ImapMetadataSearchCursor, ImapMetadataSearchRequest,
};
use hantong_contracts::uid_page::{UidMailboxEncoding, UidMailboxIdentity};
use hantong_ipc_client::message_search::ValidatedMessageSearchResponse;

fn number(value: &str) -> Result<u32, ErrorCode> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ErrorCode::InvalidInput);
    }
    value.parse().map_err(|_| ErrorCode::InvalidInput)
}

pub(super) fn parse(
    command: &[String],
    options: &[(String, String)],
) -> Result<ImapMetadataSearchRequest, ErrorCode> {
    if command.len() != 1 || command[0] != "search" {
        return Err(ErrorCode::InvalidInput);
    }
    let mut values = std::collections::BTreeMap::new();
    for (key, value) in options {
        if values.insert(key.as_str(), value.as_str()).is_some() {
            return Err(ErrorCode::InvalidInput);
        }
    }
    const ALLOWED: &[&str] = &[
        "--account",
        "--wire-hex",
        "--encoding",
        "--filter",
        "--limit",
        "--cursor",
    ];
    if values.keys().any(|key| !ALLOWED.contains(key)) {
        return Err(ErrorCode::InvalidInput);
    }
    let get = |key: &str| values.get(key).copied().ok_or(ErrorCode::InvalidInput);
    let account = get("--account")?;
    let mailbox = UidMailboxIdentity::new(
        get("--wire-hex")?,
        UidMailboxEncoding::parse(get("--encoding")?)?,
    )?;
    let filter = values
        .get("--filter")
        .map(|value| ImapMetadataFilter::parse(value))
        .transpose()?;
    let limit = values
        .get("--limit")
        .map(|value| number(value))
        .transpose()?;
    let cursor = values
        .get("--cursor")
        .map(|value| ImapMetadataSearchCursor::parse(value))
        .transpose()?;
    ImapMetadataSearchRequest::new(account, mailbox, filter, limit, cursor)
}

pub(super) fn run(
    command: &[String],
    options: &[(String, String)],
    lang: Lang,
    json: bool,
    resolve_lang: &mut impl FnMut() -> Lang,
) -> i32 {
    let request = match parse(command, options) {
        Ok(request) => request,
        Err(error) => return emit_error(error, lang, json),
    };
    let lang = resolve_lang();
    match hantong_ipc_client::message_search::search_default(request) {
        Ok(response) => {
            if json {
                match hantong_ipc_client::message_search::search_json(&response) {
                    Ok(value) => println!("{value}"),
                    Err(error) => return emit_error(error, lang, json),
                }
            } else {
                println!("{}", human_output(&response, lang));
            }
            0
        }
        Err(error) => emit_error(error, lang, json),
    }
}

fn human_output(response: &ValidatedMessageSearchResponse, lang: Lang) -> String {
    let mut output = String::new();
    output.push_str(resource(
        lang,
        if response.completeness() == "partial" {
            "message_search_partial"
        } else {
            "message_search_unobserved"
        },
    ));
    output.push('\n');
    if response.value()["warnings"]
        .as_array()
        .expect("validated warnings")
        .iter()
        .any(|v| v.as_str() == Some("STALE_BINDING"))
    {
        output.push_str(resource(lang, "message_search_stale"));
        output.push('\n');
    }
    if let Some(observed) = response.observed_at() {
        output.push_str(resource(lang, "message_search_observed"));
        output.push_str(": ");
        output.push_str(observed);
        output.push('\n');
    }
    for (item, header) in response.items().iter().zip(response.headers()) {
        output.push('\n');
        output.push_str(resource(lang, "message_search_id"));
        output.push_str(": ");
        output.push_str(&crate::escape_metadata_bytes(
            item.internal_message_id().as_bytes(),
        ));
        output.push('\n');
        output.push_str(resource(lang, "message_search_uid"));
        output.push_str(": ");
        output.push_str(&item.uid().to_string());
        output.push('\n');
        output.push_str(resource(lang, "message_search_date"));
        output.push_str(": ");
        output.push_str(&item.internal_date_utc_ms().to_string());
        output.push('\n');
        output.push_str(resource(lang, "message_search_size"));
        output.push_str(": ");
        output.push_str(&item.size_bytes().to_string());
        output.push('\n');
        output.push_str(resource(lang, "message_search_read"));
        output.push_str(": ");
        output.push_str(resource(
            lang,
            if item.is_read() {
                "message_search_yes"
            } else {
                "message_search_no"
            },
        ));
        output.push('\n');
        output.push_str(resource(lang, "message_search_favorite"));
        output.push_str(": ");
        output.push_str(resource(
            lang,
            if item.is_favorite() {
                "message_search_yes"
            } else {
                "message_search_no"
            },
        ));
        output.push('\n');
        output.push_str(resource(lang, "message_search_subject"));
        output.push_str(": ");
        output.push_str(&header.subject().map_or_else(
            || resource(lang, "message_search_nil").to_owned(),
            crate::escape_metadata_bytes,
        ));
        output.push('\n');
        output.push_str(resource(lang, "message_search_from"));
        output.push_str(":\n");
        if let Some(addresses) = header.from() {
            for address in addresses {
                append_address(
                    &mut output,
                    lang,
                    "message_search_address_name",
                    address.name(),
                );
                append_address(
                    &mut output,
                    lang,
                    "message_search_address_mailbox",
                    address.mailbox(),
                );
                append_address(
                    &mut output,
                    lang,
                    "message_search_address_host",
                    address.host(),
                );
                append_address(
                    &mut output,
                    lang,
                    "message_search_address_route",
                    address.route(),
                );
            }
        } else {
            output.push_str(resource(lang, "message_search_nil"));
            output.push('\n');
        }
    }
    if let Some(cursor) = response.next_cursor() {
        output.push('\n');
        output.push_str(resource(lang, "message_search_next"));
        output.push(' ');
        output.push_str(cursor);
        output.push('\n');
    }
    output
}

fn append_address(output: &mut String, lang: Lang, key: &str, value: Option<&[u8]>) {
    output.push_str("  ");
    output.push_str(resource(lang, key));
    output.push_str(": ");
    output.push_str(&value.map_or_else(
        || resource(lang, "message_search_nil").to_owned(),
        crate::escape_metadata_bytes,
    ));
    output.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::message_search::ImapMetadataSearchRequest;
    use hantong_contracts::uid_page::{UidMailboxEncoding, UidMailboxIdentity};
    use hantong_ipc_client::message_search::parse_search_response;

    #[test]
    fn mail03_search_human_output_distinguishes_local_scope_and_exposes_next_cursor() {
        let request = parse(&["search".into()], &valid_options()).unwrap();
        let token = format!("m1:{}:1:4294967295:1767225900000", "01".repeat(32));
        let template = r#"{"jsonrpc":"2.0","id":"r","result":{"schema_version":1,"ok":true,"items":[],"next_cursor":NEXT,"account_results":[{"account_id":"primary","status":"STATUS","items":[]}],"completeness":"STATUS","warnings":WARNINGS,"observed_at":OBSERVED,"metadata":{"provider":"imap","source":"local_store","cached":true},"untrusted_content":[]}}"#;
        let observed = template
            .replace("NEXT", &format!("\"{token}\""))
            .replace("STATUS", "partial")
            .replace("WARNINGS", r#"["LOCAL_METADATA_ONLY","STALE_BINDING"]"#)
            .replace("OBSERVED", r#""2026-10-30T00:00:00.001Z""#);
        let response = parse_search_response(observed.as_bytes(), "r", &request).unwrap();
        let english = human_output(&response, Lang::En);
        assert!(english.contains(&token));
        assert!(english.contains("2026-10-30T00:00:00.001Z"));
        assert!(english.contains(resource(Lang::En, "message_search_partial")));
        assert!(english.contains(resource(Lang::En, "message_search_stale")));
        let unavailable = template
            .replace("NEXT", "null")
            .replace("STATUS", "unavailable")
            .replace("OBSERVED", "null")
            .replace("WARNINGS", r#"["LOCAL_CACHE_UNOBSERVED"]"#);
        let response = parse_search_response(unavailable.as_bytes(), "r", &request).unwrap();
        let korean = human_output(&response, Lang::Ko);
        assert!(korean.contains(resource(Lang::Ko, "message_search_unobserved")));
        assert!(!korean.contains(&token));
        assert_ne!(english, korean);
    }

    fn valid_options() -> Vec<(String, String)> {
        vec![
            ("--account".into(), "primary".into()),
            ("--wire-hex".into(), "494e424f58".into()),
            ("--encoding".into(), "utf8".into()),
        ]
    }

    #[test]
    fn search_parse_accepts_defaults_and_explicit_filter() {
        let mut options = valid_options();
        options.push(("--filter".into(), "unread".into()));
        options.push(("--limit".into(), "8".into()));
        let request = parse(&["search".into()], &options).expect("valid search");
        assert_eq!(request.account_id(), "primary");
        assert_eq!(request.filter(), ImapMetadataFilter::Unread);
        assert_eq!(request.limit(), 8);
    }

    #[test]
    fn search_parse_rejects_duplicate_unknown_and_invalid_values() {
        let mut duplicate = valid_options();
        duplicate.push(("--account".into(), "other".into()));
        assert_eq!(
            parse(&["search".into()], &duplicate),
            Err(ErrorCode::InvalidInput)
        );
        let mut unknown = valid_options();
        unknown.push(("--bogus".into(), "x".into()));
        assert_eq!(
            parse(&["search".into()], &unknown),
            Err(ErrorCode::InvalidInput)
        );
        for value in ["0", "01", "33", "+1", "x"] {
            let mut options = valid_options();
            options.push(("--limit".into(), value.into()));
            assert_eq!(
                parse(&["search".into()], &options),
                Err(ErrorCode::InvalidInput)
            );
        }
    }

    #[test]
    fn search_parse_rejects_missing_required_fields_and_bad_cursor() {
        let options = valid_options();
        assert_eq!(
            parse(&["search".into()], &options[..2]),
            Err(ErrorCode::InvalidInput)
        );
        let mut bad_cursor = options;
        bad_cursor.push(("--cursor".into(), "m1:not-a-cursor".into()));
        assert_eq!(
            parse(&["search".into()], &bad_cursor),
            Err(ErrorCode::InvalidInput)
        );
    }

    #[test]
    fn human_output_decodes_headers_and_escapes_untrusted_controls() {
        let request = ImapMetadataSearchRequest::new(
            "primary",
            UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::Utf8).unwrap(),
            None,
            None,
            None,
        )
        .unwrap();
        let raw = br#"{"jsonrpc":"2.0","id":"search-1","result":{"schema_version":1,"ok":true,"items":[{"internal_message_id":"msg_0000000000000000000000000000000000000000000000000000000000000000","account_id":"primary","uid":9,"internal_date_utc_ms":0,"size_bytes":12,"is_read":false,"is_favorite":true,"header_content_index":0}],"next_cursor":null,"account_results":[{"account_id":"primary","status":"partial","items":[{"internal_message_id":"msg_0000000000000000000000000000000000000000000000000000000000000000","account_id":"primary","uid":9,"internal_date_utc_ms":0,"size_bytes":12,"is_read":false,"is_favorite":true,"header_content_index":0}]}],"completeness":"partial","warnings":["LOCAL_METADATA_ONLY"],"observed_at":"2026-10-30T00:00:00.000Z","metadata":{"provider":"imap","source":"local_store","cached":true},"untrusted_content":[{"content_type":"header","content":"{\"from\":[{\"host_hex\":\"6578616d706c65\",\"mailbox_hex\":\"75736572\",\"name_hex\":\"611b62\",\"route_hex\":null}],\"subject_hex\":\"610a62\"}","byte_length":125,"truncated":false}]}}"#;
        let response = parse_search_response(raw, "search-1", &request).unwrap();
        let output = human_output(&response, Lang::En);
        assert!(output.contains("a\\x0Ab"));
        assert!(output.contains("a\\x1Bb"));
        assert!(!output.contains('\u{1b}'));
        let korean = human_output(&response, Lang::Ko);
        assert!(korean.contains("제목"));
    }
}
