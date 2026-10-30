//! Strict, account-bound Gmail variants of the existing authenticated tools.
use hantong_contracts::{ErrorCode, google::*, google_mail::GoogleMailRequest, wire_hex};
use serde::{
    Deserialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value};
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
                object(
                    m,
                    &[
                        "id",
                        "thread_id",
                        "internal_date_ms",
                        "label_ids",
                        "subject",
                        "sender",
                        "snippet",
                        "size",
                    ],
                )?;
                let labels = m["label_ids"]
                    .as_array()
                    .filter(|v| v.len() <= 1024)
                    .ok_or(ErrorCode::InvalidInput)?
                    .iter()
                    .map(|l| l.as_str().map(str::to_owned).ok_or(ErrorCode::InvalidInput))
                    .collect::<Result<Vec<_>, _>>()?;
                let message = GoogleMessage {
                    id: text(m, "id", 256)?.into(),
                    thread_id: text(m, "thread_id", 256)?.into(),
                    internal_date_ms: m["internal_date_ms"]
                        .as_i64()
                        .ok_or(ErrorCode::InvalidInput)?,
                    label_ids: labels,
                    subject: text(m, "subject", 8192)?.into(),
                    sender: text(m, "sender", 8192)?.into(),
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
            object(b, &["message_id", "text", "truncated"])?;
            if b["message_id"] != *message_id || !b["truncated"].is_boolean() {
                return Err(ErrorCode::InvalidInput);
            }
            text(b, "text", *max_bytes)?;
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
    #[cfg(not(windows))]
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
