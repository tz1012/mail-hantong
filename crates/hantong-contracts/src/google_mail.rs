//! Gmail-specific identifiers behind the common mail tool names.
use crate::{
    ErrorCode,
    audit::AuditTool,
    google::{GoogleSyncRequest, validate_google_account_id},
    wire_hex,
};
use serde_json::{Map, Value};
#[derive(Clone, Eq, PartialEq)]
pub enum GoogleMailRequest {
    Sync(GoogleSyncRequest),
    Status {
        account_id: String,
        execution_id: [u8; 32],
    },
    Labels {
        account_id: String,
    },
    Messages {
        account_id: String,
        label_id: Option<String>,
        query: Option<String>,
        limit: u16,
        after: Option<String>,
    },
    Body {
        account_id: String,
        message_id: String,
        max_bytes: usize,
    },
}
impl std::fmt::Debug for GoogleMailRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GoogleMailRequest(<redacted>)")
    }
}
impl GoogleMailRequest {
    pub fn account_ids(&self) -> &[String] {
        let s = match self {
            Self::Sync(r) => &r.account_id,
            Self::Status { account_id, .. }
            | Self::Labels { account_id }
            | Self::Messages { account_id, .. }
            | Self::Body { account_id, .. } => account_id,
        };
        std::slice::from_ref(s)
    }
    pub fn tool(&self) -> AuditTool {
        match self {
            Self::Sync(_) | Self::Status { .. } => AuditTool::SyncPage,
            Self::Labels { .. } => AuditTool::FoldersList,
            Self::Messages { .. } => AuditTool::MessagesSearch,
            Self::Body { .. } => AuditTool::MessagesGet,
        }
    }
    pub fn parse(tool: &str, p: &Map<String, Value>) -> Result<Self, ErrorCode> {
        let text = |key| {
            p.get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(ErrorCode::InvalidInput)
        };
        let optional = |key, max: usize| match p.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s))
                if !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control) =>
            {
                Ok(Some(s.clone()))
            }
            _ => Err(ErrorCode::InvalidInput),
        };
        let integer = |key, default: u64| match p.get(key) {
            None => Ok(default),
            Some(v) => v.as_u64().ok_or(ErrorCode::InvalidInput),
        };
        let account_id = text("account_id")?;
        validate_google_account_id(&account_id)?;
        let mode = text("mode")?;
        let (request, keys) = match (tool, mode.as_str()) {
            ("mail.folders.list", "gmail_labels") => {
                (Self::Labels { account_id }, vec!["mode", "account_id"])
            }
            ("mail.messages.search", "gmail_messages") => {
                let limit =
                    u16::try_from(integer("limit", 32)?).map_err(|_| ErrorCode::InvalidInput)?;
                if !(1..=32).contains(&limit) {
                    return Err(ErrorCode::InvalidInput);
                }
                (
                    Self::Messages {
                        account_id,
                        label_id: optional("label_id", 128)?,
                        query: optional("query", 512)?,
                        limit,
                        after: optional("after", 2048)?,
                    },
                    vec!["mode", "account_id", "label_id", "query", "limit", "after"],
                )
            }
            ("mail.messages.get", "gmail_body") => {
                let message_id = text("message_id")?;
                if message_id.len() > 128
                    || message_id.is_empty()
                    || !message_id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                {
                    return Err(ErrorCode::InvalidInput);
                }
                let max_bytes = usize::try_from(integer("max_bytes", 65536)?)
                    .map_err(|_| ErrorCode::InvalidInput)?;
                if !(1..=65536).contains(&max_bytes) {
                    return Err(ErrorCode::InvalidInput);
                }
                (
                    Self::Body {
                        account_id,
                        message_id,
                        max_bytes,
                    },
                    vec!["mode", "account_id", "message_id", "max_bytes"],
                )
            }
            ("mail.sync.page", "gmail_status") => {
                let id = wire_hex::decode_fixed(&text("execution_id")?)?;
                if id == [0; 32] {
                    return Err(ErrorCode::InvalidInput);
                }
                (
                    Self::Status {
                        account_id,
                        execution_id: id,
                    },
                    vec!["mode", "account_id", "execution_id"],
                )
            }
            ("mail.sync.page", "gmail_page") => {
                let request = GoogleSyncRequest {
                    account_id,
                    label_id: optional("label_id", 128)?,
                    page_token: optional("page_token", 2048)?,
                    limit: u16::try_from(integer("limit", 32)?)
                        .map_err(|_| ErrorCode::InvalidInput)?,
                    expected_version: p
                        .get("expected_version")
                        .and_then(Value::as_u64)
                        .ok_or(ErrorCode::InvalidInput)?,
                    operation_id: text("operation_id")?,
                    idempotency_key: text("idempotency_key")?,
                };
                request.validate()?;
                (
                    Self::Sync(request),
                    vec![
                        "mode",
                        "account_id",
                        "label_id",
                        "page_token",
                        "limit",
                        "expected_version",
                        "operation_id",
                        "idempotency_key",
                    ],
                )
            }
            _ => return Err(ErrorCode::InvalidInput),
        };
        if p.keys().any(|k| !keys.contains(&k.as_str())) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(request)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acc03_gmail_modes_bind_explicit_account_and_never_take_approval_or_tokens() {
        let mut v = serde_json::json!({"mode":"gmail_labels","account_id":format!("google_{}","a".repeat(64))});
        assert!(GoogleMailRequest::parse("mail.folders.list", v.as_object().unwrap()).is_ok());
        v["approved"] = true.into();
        assert!(GoogleMailRequest::parse("mail.folders.list", v.as_object().unwrap()).is_err());
        assert!(GoogleMailRequest::parse("mail.messages.get", v.as_object().unwrap()).is_err());
    }
}
