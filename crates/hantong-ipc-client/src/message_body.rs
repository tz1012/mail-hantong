//! Strict body responses and one authenticated Core exchange.
use crate::accounts::Object;
use hantong_contracts::{ErrorCode, message_body::MessageBodyRequest};
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Success {
    jsonrpc: String,
    id: String,
    result: Object<BodyResult>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    jsonrpc: String,
    id: String,
    error: Object<FailureCode>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FailureCode {
    code: String,
    message: String,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Reply {
    Success(Object<Success>),
    Failure(Object<Failure>),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyResult {
    schema_version: u32,
    ok: bool,
    state: String,
    account_id: String,
    mailbox_wire_hex: String,
    mailbox_encoding: String,
    uidvalidity: u32,
    uid: u32,
    text: serde_json::Value,
    truncated: bool,
}
pub struct BodyResponse {
    pub state: String,
    pub text: Option<String>,
    pub truncated: bool,
}
impl std::fmt::Debug for BodyResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BodyResponse(<redacted>)")
    }
}
pub fn parse_response(
    bytes: &[u8],
    id: &str,
    request: &MessageBodyRequest,
) -> Result<BodyResponse, ErrorCode> {
    if bytes.is_empty() || bytes.len() > 524288 || bytes.starts_with(&[239, 187, 191]) {
        return Err(ErrorCode::InvalidInput);
    }
    let reply: Reply = serde_json::from_slice(bytes).map_err(|_| ErrorCode::InvalidInput)?;
    let result = match reply {
        Reply::Success(Object(v)) => {
            if v.jsonrpc != "2.0" || v.id != id {
                return Err(ErrorCode::InvalidInput);
            }
            v.result.0
        }
        Reply::Failure(Object(v)) => {
            if v.jsonrpc != "2.0" || v.id != id || v.error.0.message != v.error.0.code {
                return Err(ErrorCode::InvalidInput);
            }
            return ErrorCode::ALL
                .iter()
                .find(|e| e.as_str() == v.error.0.code)
                .copied()
                .ok_or(ErrorCode::InvalidInput)
                .and_then(Err);
        }
    };
    if result.schema_version != 1
        || !result.ok
        || result.account_id != request.account_id()
        || result.mailbox_wire_hex != request.mailbox().wire_hex()
        || result.mailbox_encoding != request.mailbox().encoding().as_str()
        || result.uidvalidity != request.uidvalidity()
        || result.uid != request.uid()
    {
        return Err(ErrorCode::InvalidInput);
    }
    let text = match result.state.as_str() {
        "pending" if result.text.is_null() && !result.truncated => None,
        "ready" => {
            let text = result.text.as_str().ok_or(ErrorCode::InvalidInput)?;
            if text.len() > 65536 {
                return Err(ErrorCode::InvalidInput);
            }
            Some(hantong_contracts::message_body::render_terminal_text(
                text.as_bytes(),
            ))
        }
        _ => return Err(ErrorCode::InvalidInput),
    };
    Ok(BodyResponse {
        state: result.state,
        text,
        truncated: result.truncated,
    })
}
pub fn get_default(request: &MessageBodyRequest) -> Result<BodyResponse, ErrorCode> {
    #[cfg(windows)]
    {
        tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build().map_err(|_|ErrorCode::CoreUnavailable)?.block_on(async {
   let profile=crate::authenticate_default().await?;
   if !profile.scope().permits_account(request.account_id(),"mail.messages.get"){return Err(ErrorCode::ScopeDenied)}
   let id="body-read-1";
   let params=serde_json::json!({"schema_version":1,"request_id":id,"tool":"mail.messages.get","params":{"mode":"imap_body","account_id":request.account_id(),"mailbox_wire_hex":request.mailbox().wire_hex(),"mailbox_encoding":request.mailbox().encoding().as_str(),"uidvalidity":request.uidvalidity(),"uid":request.uid(),"max_bytes":request.max_bytes()}});
   let bytes=profile.exchange(id,"mail.messages.get",&params).await?;
   parse_response(&bytes,id,request)
  })
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::uid_page::{UidMailboxEncoding, UidMailboxIdentity};
    fn request() -> MessageBodyRequest {
        MessageBodyRequest::new(
            "acct",
            UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::Utf8).unwrap(),
            42,
            9,
            1024,
        )
        .unwrap()
    }
    fn response() -> serde_json::Value {
        serde_json::json!({"jsonrpc":"2.0","id":"body-1","result":{"schema_version":1,"ok":true,"state":"ready","account_id":"acct","mailbox_wire_hex":"494e424f58","mailbox_encoding":"utf8","uidvalidity":42,"uid":9,"text":"hello","truncated":false}})
    }
    #[test]
    fn mail03_body_reply_binds_identity_and_rejects_unexpected_fields() {
        let r = request();
        let mut v = response();
        assert_eq!(
            parse_response(&serde_json::to_vec(&v).unwrap(), "body-1", &r)
                .unwrap()
                .text
                .as_deref(),
            Some("hello")
        );
        for field in [
            "account_id",
            "mailbox_wire_hex",
            "mailbox_encoding",
            "uidvalidity",
            "uid",
        ] {
            let old = v["result"][field].clone();
            v["result"][field] = serde_json::Value::Null;
            assert!(parse_response(&serde_json::to_vec(&v).unwrap(), "body-1", &r).is_err());
            v["result"][field] = old;
        }
        v["result"]["approved"] = true.into();
        assert!(parse_response(&serde_json::to_vec(&v).unwrap(), "body-1", &r).is_err());
    }
    #[test]
    fn mail03_body_reply_rejects_duplicates_and_pending_content() {
        let r = request();
        let text = serde_json::to_string(&response()).unwrap().replace(
            "\"text\":\"hello\"",
            "\"text\":\"hello\",\"text\":\"other\"",
        );
        assert!(parse_response(text.as_bytes(), "body-1", &r).is_err());
        let mut v = response();
        v["result"]["state"] = "pending".into();
        assert!(parse_response(&serde_json::to_vec(&v).unwrap(), "body-1", &r).is_err());
        v["result"]["text"] = serde_json::Value::Null;
        assert_eq!(
            parse_response(&serde_json::to_vec(&v).unwrap(), "body-1", &r)
                .unwrap()
                .state,
            "pending"
        );
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocatorSuccess {
    jsonrpc: String,
    id: String,
    result: Object<LocatorWire>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocatorWire {
    schema_version: u32,
    ok: bool,
    account_id: String,
    internal_message_id: String,
    mailbox_wire_hex: String,
    mailbox_encoding: String,
    uidvalidity: u32,
    uid: u32,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum LocatorReply {
    Success(Object<LocatorSuccess>),
    Failure(Object<Failure>),
}
pub fn parse_locator(
    bytes: &[u8],
    id: &str,
    account: &str,
    message: &str,
    max_bytes: usize,
) -> Result<MessageBodyRequest, ErrorCode> {
    if bytes.len() > 16384 || bytes.is_empty() || bytes.starts_with(&[239, 187, 191]) {
        return Err(ErrorCode::InvalidInput);
    }
    let reply: LocatorReply = serde_json::from_slice(bytes).map_err(|_| ErrorCode::InvalidInput)?;
    let v = match reply {
        LocatorReply::Success(Object(v)) if v.jsonrpc == "2.0" && v.id == id => v.result.0,
        LocatorReply::Failure(Object(v))
            if v.jsonrpc == "2.0" && v.id == id && v.error.0.message == v.error.0.code =>
        {
            return Err(ErrorCode::ALL
                .iter()
                .find(|e| e.as_str() == v.error.0.code)
                .copied()
                .unwrap_or(ErrorCode::InvalidInput));
        }
        _ => return Err(ErrorCode::InvalidInput),
    };
    if v.schema_version != 1 || !v.ok || v.account_id != account || v.internal_message_id != message
    {
        return Err(ErrorCode::InvalidInput);
    }
    MessageBodyRequest::new(
        account,
        hantong_contracts::uid_page::UidMailboxIdentity::new(
            &v.mailbox_wire_hex,
            hantong_contracts::uid_page::UidMailboxEncoding::parse(&v.mailbox_encoding)?,
        )?,
        v.uidvalidity,
        v.uid,
        max_bytes,
    )
}
pub fn locate_default(
    account: &str,
    message: &str,
    max_bytes: usize,
) -> Result<MessageBodyRequest, ErrorCode> {
    if account.is_empty()
        || account.len() > 128
        || !account
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        || message.len() != 68
        || !message.starts_with("msg_")
        || !message[4..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || !(1..=hantong_contracts::message_body::MAX_BODY_BYTES).contains(&max_bytes)
    {
        return Err(ErrorCode::InvalidInput);
    }
    #[cfg(windows)]
    {
        tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build().map_err(|_|ErrorCode::CoreUnavailable)?.block_on(async{
  let profile=crate::authenticate_default().await?;if !profile.scope().permits_account(account,"mail.messages.get"){return Err(ErrorCode::ScopeDenied)}
  let id="body-locate-1";let params=serde_json::json!({"schema_version":1,"request_id":id,"tool":"mail.messages.get","params":{"mode":"locator","account_id":account,"internal_message_id":message,"max_bytes":max_bytes}});
  let bytes=profile.exchange(id,"mail.messages.get",&params).await?;parse_locator(&bytes,id,account,message,max_bytes)
 })
    }
    #[cfg(not(windows))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
}
