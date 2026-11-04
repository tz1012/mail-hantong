//! Strict, account-bound Gmail variants of the existing authenticated tools.
use hantong_contracts::{
    ErrorCode, google::*, google_mail::GoogleMailRequest, message_body::MessageEnvelope, wire_hex,
};
use serde::{
    Deserialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value};
#[cfg(any(windows, unix, test))]
use std::collections::BTreeSet;
use std::fmt;

// serde_json::Value alone overwrites duplicate object keys. Reject duplicates
// recursively before interpreting any envelope, cached metadata, or body.
struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Strict;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("bounded JSON with unique object keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut v = Vec::new();
                while let Some(Strict(item)) = a.next_element()? {
                    v.push(item)
                }
                Ok(Strict(Value::Array(v)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut v = Map::new();
                while let Some(key) = a.next_key::<String>()? {
                    if v.contains_key(&key) {
                        return Err(de::Error::custom("duplicate JSON field"));
                    }
                    let Strict(item) = a.next_value()?;
                    v.insert(key, item);
                }
                Ok(Strict(Value::Object(v)))
            }
        }
        d.deserialize_any(V)
    }
}
fn object<'a>(v: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>, ErrorCode> {
    let m = v.as_object().ok_or(ErrorCode::InvalidInput)?;
    if m.len() != keys.len() || keys.iter().any(|k| !m.contains_key(*k)) {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(m)
}
fn text<'a>(v: &'a Value, key: &str, max: usize) -> Result<&'a str, ErrorCode> {
    let s = v
        .get(key)
        .and_then(Value::as_str)
        .ok_or(ErrorCode::InvalidInput)?;
    if s.len() > max {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(s)
}
fn uint(v: &Value, key: &str) -> Result<u64, ErrorCode> {
    v.get(key)
        .and_then(Value::as_u64)
        .filter(|n| *n < i64::MAX as u64)
        .ok_or(ErrorCode::InvalidInput)
}
fn optional(v: &Value, key: &str, max: usize) -> Result<Option<String>, ErrorCode> {
    match v.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(s))
            if !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control) =>
        {
            Ok(Some(s.clone()))
        }
        _ => Err(ErrorCode::InvalidInput),
    }
}
fn base(v: &Value, account: &str, extra: &[&str]) -> Result<(), ErrorCode> {
    let mut keys = vec![
        "schema_version",
        "ok",
        "provider",
        "read_only",
        "account_id",
    ];
    keys.extend(extra);
    object(v, &keys)?;
    if v["schema_version"] != 1
        || v["ok"] != true
        || v["provider"] != "gmail"
        || v["read_only"] != true
        || v["account_id"] != account
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

pub fn parse_response(
    bytes: &[u8],
    id: &str,
    request: &GoogleMailRequest,
) -> Result<Value, ErrorCode> {
    if bytes.is_empty() || bytes.len() > 524288 || bytes.starts_with(&[239, 187, 191]) {
        return Err(ErrorCode::InvalidInput);
    }
    validate_google_account_id(&request.account_ids()[0])?;
    match request {
        GoogleMailRequest::Sync(r) => r.validate()?,
        GoogleMailRequest::Messages { limit, .. } if !(1..=32).contains(limit) => {
            return Err(ErrorCode::InvalidInput);
        }
        GoogleMailRequest::Body { max_bytes, .. } if !(1..=65536).contains(max_bytes) => {
            return Err(ErrorCode::InvalidInput);
        }
        _ => {}
    }
    let Strict(envelope) = serde_json::from_slice(bytes).map_err(|_| ErrorCode::InvalidInput)?;
    let success = envelope.get("result").is_some();
    object(
        &envelope,
        &["jsonrpc", "id", if success { "result" } else { "error" }],
    )?;
    if envelope["jsonrpc"] != "2.0" || envelope["id"] != id {
        return Err(ErrorCode::InvalidInput);
    }
    if !success {
        let error = &envelope["error"];
        object(error, &["code", "message"])?;
        if error["code"] != error["message"] {
            return Err(ErrorCode::InvalidInput);
        }
        return Err(ErrorCode::ALL
            .into_iter()
            .find(|e| Some(e.as_str()) == error["code"].as_str())
            .ok_or(ErrorCode::InvalidInput)?);
    }
    let v = envelope["result"].clone();
    let account = &request.account_ids()[0];
    match request {
        GoogleMailRequest::Labels { .. } => {
            base(&v, account, &["labels", "sync_version"])?;
            uint(&v, "sync_version")?;
            let labels = v["labels"]
                .as_array()
                .filter(|a| a.len() <= 1024)
                .ok_or(ErrorCode::InvalidInput)?;
            let mut seen = std::collections::BTreeSet::new();
            for l in labels {
                object(l, &["id", "name", "kind"])?;
                let label = GoogleLabel {
                    id: text(l, "id", 256)?.into(),
                    name: text(l, "name", 1024)?.into(),
                    kind: text(l, "kind", 16)?.into(),
                };
                label.validate()?;
                if !seen.insert(label.id) {
                    return Err(ErrorCode::InvalidInput);
                }
            }
        }
        GoogleMailRequest::Messages { limit, .. } => {
            base(&v, account, &["messages", "next_cursor", "sync_version"])?;
            uint(&v, "sync_version")?;
            optional(&v, "next_cursor", 256)?;
            let messages = v["messages"]
                .as_array()
                .filter(|m| m.len() <= usize::from(*limit))
                .ok_or(ErrorCode::InvalidInput)?;
            let mut seen = std::collections::BTreeSet::new();
            for m in messages {
                let mut keys = vec![
                    "id",
                    "thread_id",
                    "internal_date_ms",
                    "label_ids",
                    "subject",
                    "sender",
                    "snippet",
                    "size",
                ];
                if m.get("attachment_checks").is_some() {
                    keys.push("attachment_checks");
                }
                if m.get("headers").is_some() {
                    keys.push("headers");
                }
                object(m, &keys)?;
                let attachment_checks = match m.get("attachment_checks") {
                    None | Some(Value::Null) => None,
                    Some(value) => {
                        let checks = value
                            .as_array()
                            .filter(|v| v.len() <= 8)
                            .ok_or(ErrorCode::InvalidInput)?;
                        Some(
                            checks
                                .iter()
                                .map(GoogleAttachmentCheck::from_json)
                                .collect::<Result<Vec<_>, _>>()?,
                        )
                    }
                };
                let labels = m["label_ids"]
                    .as_array()
                    .filter(|v| v.len() <= 1024)
                    .ok_or(ErrorCode::InvalidInput)?
                    .iter()
                    .map(|l| l.as_str().map(str::to_owned).ok_or(ErrorCode::InvalidInput))
                    .collect::<Result<Vec<_>, _>>()?;
                let headers = match m.get("headers") {
                    None | Some(Value::Null) => None,
                    Some(value) => {
                        let o = value.as_object().ok_or(ErrorCode::InvalidInput)?;
                        if o.len() != 5
                            || !["subject", "from", "to", "cc", "date"]
                                .iter()
                                .all(|k| o.contains_key(*k))
                        {
                            return Err(ErrorCode::InvalidInput);
                        }
                        let text =
                            |key: &str| o.get(key).and_then(Value::as_str).map(str::to_owned);
                        Some(MessageEnvelope::new(
                            text("subject"),
                            text("from"),
                            text("to"),
                            text("cc"),
                            text("date"),
                        )?)
                    }
                };
                let message = GoogleMessage {
                    attachment_checks,
                    id: text(m, "id", 256)?.into(),
                    thread_id: text(m, "thread_id", 256)?.into(),
                    internal_date_ms: m["internal_date_ms"]
                        .as_i64()
                        .ok_or(ErrorCode::InvalidInput)?,
                    label_ids: labels,
                    subject: text(m, "subject", 8192)?.into(),
                    sender: text(m, "sender", 8192)?.into(),
                    headers,
                    snippet: text(m, "snippet", 8192)?.into(),
                    body_text: None,
                    body_error: None,
                    body_truncated: false,
                    size: m["size"].as_u64().ok_or(ErrorCode::InvalidInput)?,
                };
                message.validate()?;
                if !seen.insert(message.id) {
                    return Err(ErrorCode::InvalidInput);
                }
            }
        }
        GoogleMailRequest::Body {
            message_id,
            max_bytes,
            ..
        } => {
            base(&v, account, &["body"])?;
            let b = &v["body"];
            let mut keys = vec!["message_id", "text", "truncated"];
            if b.get("headers").is_some() {
                keys.push("headers");
            }
            object(b, &keys)?;
            if b["message_id"] != *message_id || !b["truncated"].is_boolean() {
                return Err(ErrorCode::InvalidInput);
            }
            text(b, "text", *max_bytes)?;
            if let Some(value) = b.get("headers") {
                if value.is_null() {
                    return Ok(v);
                }
                let o = value.as_object().ok_or(ErrorCode::InvalidInput)?;
                if o.len() != 5
                    || !["subject", "from", "to", "cc", "date"]
                        .iter()
                        .all(|k| o.contains_key(*k))
                {
                    return Err(ErrorCode::InvalidInput);
                }
                let text = |key: &str| o.get(key).and_then(Value::as_str).map(str::to_owned);
                MessageEnvelope::new(
                    text("subject"),
                    text("from"),
                    text("to"),
                    text("cc"),
                    text("date"),
                )?;
            }
        }
        GoogleMailRequest::Sync(_) | GoogleMailRequest::Status { .. } => {
            base(&v, account, &["sync"])?;
            let s = &v["sync"];
            object(
                s,
                &[
                    "account_id",
                    "execution_id",
                    "state",
                    "version",
                    "next_page_token",
                    "error_code",
                    "downloaded_count",
                ],
            )?;
            let execution_id = wire_hex::decode_fixed(text(s, "execution_id", 64)?)?;
            let error_code = optional(s, "error_code", 32)?
                .map(|s| {
                    ErrorCode::ALL
                        .into_iter()
                        .find(|e| e.as_str() == s)
                        .ok_or(ErrorCode::InvalidInput)
                })
                .transpose()?;
            let receipt = GoogleSyncReceipt {
                account_id: text(s, "account_id", 128)?.into(),
                execution_id,
                state: text(s, "state", 16)?.into(),
                version: uint(s, "version")?,
                next_page_token: optional(s, "next_page_token", 2048)?,
                error_code,
                downloaded_count: uint(s, "downloaded_count")?,
            };
            receipt.validate()?;
            if receipt.account_id != *account
                || matches!(request,GoogleMailRequest::Status{execution_id:id,..} if *id!=execution_id)
                || !matches!(
                    receipt.state.as_str(),
                    "pending" | "running" | "completed" | "failed" | "interrupted"
                )
                || matches!(receipt.state.as_str(), "failed" | "interrupted")
                    != receipt.error_code.is_some()
            {
                return Err(ErrorCode::InvalidInput);
            }
        }
    }
    Ok(v)
}

pub fn call_default(tool: &str, params: Value) -> Result<Value, ErrorCode> {
    let request =
        GoogleMailRequest::parse(tool, params.as_object().ok_or(ErrorCode::InvalidInput)?)?;
    #[cfg(windows)]
    {
        tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build().map_err(|_|ErrorCode::CoreUnavailable)?.block_on(async{
            let profile=crate::authenticate_default().await?;
            if !profile.scope().permits_account(&request.account_ids()[0],tool){return Err(ErrorCode::ScopeDenied)}
            let id="gmail-read-1";let envelope=serde_json::json!({"schema_version":1,"request_id":id,"tool":tool,"params":params});
            let bytes=profile.exchange(id,tool,&envelope).await?;
            parse_response(&bytes,id,&request)
        })
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let profile = hantong_platform::load_default_client_profile()?;
        let parsed = crate::profile::ParsedClientProfile::from_json(profile.bytes())?;
        if !request
            .account_ids()
            .iter()
            .all(|account| parsed.scope().permits_account(account, tool))
        {
            return Err(ErrorCode::ScopeDenied);
        }
        let id = "gmail-read-1";
        let raw = crate::unix_relay::call(parsed.profile(), id, tool, &params)?;
        parse_response(&raw, id, &request)
    }
    #[cfg(all(not(windows), not(any(target_os = "linux", target_os = "macos"))))]
    {
        let _ = (tool, params, request);
        Err(ErrorCode::CapabilityUnsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> GoogleMailRequest {
        GoogleMailRequest::Labels {
            account_id: format!("google_{}", "a".repeat(64)),
        }
    }
    fn reply() -> Value {
        serde_json::json!({"jsonrpc":"2.0","id":"r","result":{"schema_version":1,"ok":true,"provider":"gmail","read_only":true,"account_id":request().account_ids()[0],"labels":[{"id":"INBOX","name":"Inbox","kind":"system"}],"sync_version":0}})
    }
    #[test]
    fn acc03_gmail_reply_account_readonly_extra_fields_and_duplicates_fail_closed() {
        let r = request();
        let v = reply();
        assert!(parse_response(&serde_json::to_vec(&v).unwrap(), "r", &r).is_ok());
        for key in ["account_id", "read_only", "provider"] {
            let mut bad = v.clone();
            bad["result"][key] = Value::Null;
            assert!(parse_response(&serde_json::to_vec(&bad).unwrap(), "r", &r).is_err());
        }
        let mut bad = v.clone();
        bad["result"]["access_token"] = "forbidden".into();
        assert!(parse_response(&serde_json::to_vec(&bad).unwrap(), "r", &r).is_err());
        let raw = v.to_string().replace(
            "\"name\":\"Inbox\"",
            "\"name\":\"Inbox\",\"name\":\"other\"",
        );
        assert!(parse_response(raw.as_bytes(), "r", &r).is_err());
        assert!(parse_response(&vec![b' '; 524289], "r", &r).is_err());
    }
    #[test]
    fn acc03_gmail_body_binds_message_and_never_accepts_extra_approval() {
        let r = GoogleMailRequest::Body {
            account_id: request().account_ids()[0].clone(),
            message_id: "m1".into(),
            max_bytes: 5,
        };
        let mut v = reply();
        v["result"].as_object_mut().unwrap().remove("labels");
        v["result"].as_object_mut().unwrap().remove("sync_version");
        v["result"]["body"] =
            serde_json::json!({"message_id":"m1","text":"hello","truncated":false});
        assert!(parse_response(&serde_json::to_vec(&v).unwrap(), "r", &r).is_ok());
        v["result"]["body"]["text"] = "too long".into();
        assert!(parse_response(&serde_json::to_vec(&v).unwrap(), "r", &r).is_err());
    }
}

/// Probe the negotiated protected profile without reading mail or changing grants.
/// False means authenticated but missing selected read access. Authentication
/// errors, including protected-path SCOPE_DENIED, remain errors, never grants.
pub fn verify_default_readonly(account: &str) -> Result<bool, ErrorCode> {
    validate_google_account_id(account)?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let loaded = hantong_platform::load_default_client_profile()?;
        let profile = crate::profile::ParsedClientProfile::from_json(loaded.bytes())?;
        Ok(readonly_scope(profile.scope(), account))
    }
    #[cfg(all(not(windows), not(any(target_os = "linux", target_os = "macos"))))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
    #[cfg(windows)]
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .block_on(async {
            let connection = crate::authenticate_default().await?;
            Ok(readonly_scope(connection.scope(), account))
        })
}
/// Verifies the exact selected personal feature profile without reading mail.
pub fn verify_default_personal(
    account: &str,
    profile: hantong_contracts::google_setup::GoogleLocalProfile,
) -> Result<bool, ErrorCode> {
    validate_google_account_id(account)?;
    if profile.access_mode().is_none() {
        return Err(ErrorCode::InvalidInput);
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let loaded = hantong_platform::load_default_client_profile()?;
        let profile_data = crate::profile::ParsedClientProfile::from_json(loaded.bytes())?;
        Ok(personal_scope(profile_data.scope(), account, profile))
    }
    #[cfg(all(not(windows), not(any(target_os = "linux", target_os = "macos"))))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
    #[cfg(windows)]
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .block_on(async {
            let connection = crate::authenticate_default().await?;
            Ok(personal_scope(connection.scope(), account, profile))
        })
}
/// Returns the bounded union of the current default Google scope accounts and
/// the selected account. The result is only suitable for an explicit
/// terminal scope request; it never grants or enumerates ready accounts.
pub fn default_google_account_union(account: &str) -> Result<Vec<String>, ErrorCode> {
    validate_google_account_id(account)?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let loaded = hantong_platform::load_default_client_profile()?;
        let profile = crate::profile::ParsedClientProfile::from_json(loaded.bytes())?;
        account_union(profile.scope(), account)
    }
    #[cfg(all(not(windows), not(any(target_os = "linux", target_os = "macos"))))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
    #[cfg(windows)]
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .block_on(async {
            let connection = crate::authenticate_default().await?;
            account_union(connection.scope(), account)
        })
}

/// Returns the bounded union of every account in the authenticated default
/// profile and the selected Google account. The account list is explicit and
/// provider-neutral; it is only an input to the terminal manage review.
pub fn default_registered_account_union(account: &str) -> Result<Vec<String>, ErrorCode> {
    hantong_contracts::scope_refresh::ManageAccounts::parse(&[account.to_owned()])?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let loaded = hantong_platform::load_default_client_profile()?;
        let profile = crate::profile::ParsedClientProfile::from_json(loaded.bytes())?;
        generic_account_union(profile.scope(), account)
    }
    #[cfg(all(not(windows), not(any(target_os = "linux", target_os = "macos"))))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
    #[cfg(windows)]
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .block_on(async {
            let connection = crate::authenticate_default().await?;
            generic_account_union(connection.scope(), account)
        })
}

/// Verifies the exact provider-neutral manage toolset over the explicit
/// current registered-account union. Account and source checks remain owned by
/// Core; this is only a post-review scope probe.
pub fn verify_default_manage_accounts(account: &str) -> Result<bool, ErrorCode> {
    hantong_contracts::scope_refresh::ManageAccounts::parse(&[account.to_owned()])?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let loaded = hantong_platform::load_default_client_profile()?;
        let profile = crate::profile::ParsedClientProfile::from_json(loaded.bytes())?;
        let accounts = generic_account_union(profile.scope(), account)?;
        Ok(manage_scope(profile.scope(), &accounts))
    }
    #[cfg(all(not(windows), not(any(target_os = "linux", target_os = "macos"))))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
    #[cfg(windows)]
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .block_on(async {
            let connection = crate::authenticate_default().await?;
            let accounts = generic_account_union(connection.scope(), account)?;
            Ok(manage_scope(connection.scope(), &accounts))
        })
}
/// Verifies the exact send toolset over the bounded current-account union.
pub fn verify_default_personal_accounts(account: &str) -> Result<bool, ErrorCode> {
    validate_google_account_id(account)?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let loaded = hantong_platform::load_default_client_profile()?;
        let profile = crate::profile::ParsedClientProfile::from_json(loaded.bytes())?;
        personal_send_eligible(profile.scope(), account)
    }
    #[cfg(all(not(windows), not(any(target_os = "linux", target_os = "macos"))))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
    #[cfg(windows)]
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .block_on(async {
            let connection = crate::authenticate_default().await?;
            // A valid authenticated session may contain non-Google accounts
            // registered in the same default profile. It cannot satisfy the
            // Google-only personal fast path, so return false and let the
            // provider-neutral protected terminal path enforce registration
            // and effective send tools. Authentication and malformed selected
            // Google identities remain hard errors above.
            personal_send_eligible(connection.scope(), account)
        })
}
#[cfg(any(windows, unix, test))]
fn personal_scope(
    scope: &hantong_contracts::auth::RequestedScope,
    account: &str,
    profile: hantong_contracts::google_setup::GoogleLocalProfile,
) -> bool {
    use hantong_contracts::{auth::*, google_setup::GoogleLocalProfile};
    let tools = match profile {
        GoogleLocalProfile::PersonalRead => default_cli_body_tools(),
        GoogleLocalProfile::PersonalReadSend => default_cli_send_tools(),
        GoogleLocalProfile::PersonalManage => default_cli_manage_tools(),
        GoogleLocalProfile::None => return false,
    };
    scope.accounts() == [account] && scope.tools() == tools
}
#[cfg(any(windows, unix, test))]
fn account_union(
    scope: &hantong_contracts::auth::RequestedScope,
    selected: &str,
) -> Result<Vec<String>, ErrorCode> {
    validate_google_account_id(selected)?;
    let mut accounts = BTreeSet::new();
    for account in scope.accounts() {
        validate_google_account_id(account)?;
        accounts.insert(account.clone());
    }
    accounts.insert(selected.to_owned());
    if accounts.len() > 128 {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(accounts.into_iter().collect())
}
#[cfg(any(windows, unix, test))]
fn generic_account_union(
    scope: &hantong_contracts::auth::RequestedScope,
    selected: &str,
) -> Result<Vec<String>, ErrorCode> {
    hantong_contracts::scope_refresh::ManageAccounts::parse(&[selected.to_owned()])?;
    let mut accounts = BTreeSet::new();
    accounts.extend(scope.accounts().iter().cloned());
    accounts.insert(selected.to_owned());
    if accounts.len() > 128 {
        return Err(ErrorCode::InvalidInput);
    }
    hantong_contracts::scope_refresh::ManageAccounts::parse(
        &accounts.into_iter().collect::<Vec<_>>(),
    )
    .map(|accounts| accounts.accounts())
}
#[cfg(any(windows, unix, test))]
fn manage_scope(scope: &hantong_contracts::auth::RequestedScope, accounts: &[String]) -> bool {
    scope.accounts() == accounts
        && hantong_contracts::auth::is_supported_default_cli_manage_toolset(scope.tools())
}
#[cfg(any(windows, unix, test))]
fn send_scope(scope: &hantong_contracts::auth::RequestedScope, accounts: &[String]) -> bool {
    scope.accounts() == accounts
        && (scope.tools() == hantong_contracts::auth::default_cli_send_tools()
            || scope.tools() == hantong_contracts::auth::default_cli_manage_tools())
}
#[cfg(any(windows, unix, test))]
fn personal_send_eligible(
    scope: &hantong_contracts::auth::RequestedScope,
    selected: &str,
) -> Result<bool, ErrorCode> {
    if scope
        .accounts()
        .iter()
        .any(|candidate| !candidate.starts_with("google_"))
    {
        return Ok(false);
    }
    let accounts = account_union(scope, selected)?;
    Ok(send_scope(scope, &accounts))
}
#[cfg(any(windows, unix, test))]
fn readonly_scope(scope: &hantong_contracts::auth::RequestedScope, account: &str) -> bool {
    scope.accounts().iter().any(|a| a == account)
        && hantong_contracts::auth::default_cli_body_tools()
            .iter()
            .all(|t| scope.tools().contains(t))
}
#[cfg(test)]
mod onboarding_tests {
    use super::*;
    use hantong_contracts::auth::*;
    #[test]
    fn acc02_personal_ready_probe_requires_exact_selected_features_and_account() {
        use hantong_contracts::google_setup::GoogleLocalProfile::*;
        let account = format!("google_{}", "a".repeat(64));
        for (tools, profile, other) in [
            (default_cli_body_tools(), PersonalRead, PersonalReadSend),
            (default_cli_send_tools(), PersonalReadSend, PersonalRead),
            (default_cli_manage_tools(), PersonalManage, PersonalReadSend),
        ] {
            let scope = RequestedScope::new(vec![account.clone()], tools).unwrap();
            assert!(personal_scope(&scope, &account, profile));
            assert!(!personal_scope(&scope, &account, other));
            assert!(!personal_scope(&scope, "different", profile));
            assert!(!personal_scope(&scope, &account, None));
        }
    }
    #[cfg(all(not(windows), not(any(target_os = "linux", target_os = "macos"))))]
    #[test]
    fn acc02_readonly_probe_without_windows_is_explicitly_unsupported() {
        let account = format!("google_{}", "a".repeat(64));
        assert_eq!(
            verify_default_readonly(&account),
            Err(ErrorCode::CapabilityUnsupported)
        );
        assert_eq!(
            verify_default_readonly("invalid"),
            Err(ErrorCode::InvalidInput)
        );
    }
    #[test]
    fn acc02_readonly_probe_requires_selected_account_and_all_read_tools() {
        let a = format!("google_{}", "a".repeat(64));
        let b = format!("google_{}", "b".repeat(64));
        let s = RequestedScope::new(vec![a.clone()], default_cli_body_tools()).unwrap();
        assert!(readonly_scope(&s, &a));
        assert!(!readonly_scope(&s, &b));
        let s = RequestedScope::new(vec![a.clone()], default_cli_folder_tools()).unwrap();
        assert!(!readonly_scope(&s, &a));
    }
    #[test]
    fn acc02_send_probe_uses_bounded_current_account_union() {
        let a = format!("google_{}", "a".repeat(64));
        let b = format!("google_{}", "b".repeat(64));
        let scope = RequestedScope::new(vec![a.clone()], default_cli_send_tools()).unwrap();
        assert_eq!(
            account_union(&scope, &b).unwrap(),
            vec![a.clone(), b.clone()]
        );
        assert!(!send_scope(&scope, &account_union(&scope, &b).unwrap()));
        let scope =
            RequestedScope::new(vec![a.clone(), b.clone()], default_cli_send_tools()).unwrap();
        let union = account_union(&scope, &b).unwrap();
        assert_eq!(union, vec![a, b]);
        assert!(send_scope(&scope, &union));

        let many = (0..128)
            .map(|value| format!("google_{value:064x}"))
            .collect::<Vec<_>>();
        let scope = RequestedScope::new(many.clone(), default_cli_send_tools()).unwrap();
        assert_eq!(account_union(&scope, &many[127]).unwrap(), many);
    }
    #[test]
    fn acc02_send_probe_rejects_more_than_128_accounts() {
        let accounts = (0..129)
            .map(|value| format!("google_{value:064x}"))
            .collect::<Vec<_>>();
        let scope =
            RequestedScope::new(accounts[..128].to_vec(), default_cli_send_tools()).unwrap();
        assert_eq!(
            account_union(&scope, &accounts[128]),
            Err(ErrorCode::InvalidInput)
        );
    }
    #[test]
    fn ai09_manage_probe_keeps_generic_registered_accounts_and_selected_google_account() {
        let google = format!("google_{}", "a".repeat(64));
        let scope = RequestedScope::new(
            vec![google.clone(), "imap_account".into()],
            default_cli_manage_tools(),
        )
        .unwrap();
        let accounts = generic_account_union(&scope, &google).unwrap();
        assert_eq!(
            accounts,
            vec![
                "google_".to_owned() + &"a".repeat(64),
                "imap_account".into()
            ]
        );
        assert!(manage_scope(&scope, &accounts));
    }
    #[test]
    fn ai09_manage_probe_requires_selected_generic_account_in_current_scope() {
        let google = format!("google_{}", "c".repeat(64));
        let selected = "imap_account".to_owned();
        let scope = RequestedScope::new(vec![google.clone()], default_cli_manage_tools()).unwrap();
        let accounts = generic_account_union(&scope, &selected).unwrap();
        assert_eq!(accounts, vec![google, selected]);
        // A proposed union does not itself grant the newly selected account.
        assert!(!manage_scope(&scope, &accounts));
        let granted = RequestedScope::new(accounts.clone(), default_cli_manage_tools()).unwrap();
        assert!(manage_scope(&granted, &accounts));
    }
    #[test]
    fn ai09_manage_probe_accepts_google_account_qualified_manage_grants() {
        let google = format!("google_{}", "b".repeat(64));
        let mut tools = default_cli_manage_tools();
        tools.push(hantong_contracts::google_mutation::account_grant(&google).unwrap());
        tools.sort();
        let scope = RequestedScope::new(vec![google.clone()], tools).unwrap();
        assert!(manage_scope(&scope, &[google]));
    }
    #[test]
    fn acc02_mixed_provider_scope_uses_generic_terminal_path() {
        let google = format!("google_{}", "a".repeat(64));
        let mixed = RequestedScope::new(
            vec![google.clone(), "smtp_account".into()],
            default_cli_send_tools(),
        )
        .unwrap();
        assert_eq!(personal_send_eligible(&mixed, &google), Ok(false));
        assert_eq!(account_union(&mixed, &google), Err(ErrorCode::InvalidInput));
    }
}
