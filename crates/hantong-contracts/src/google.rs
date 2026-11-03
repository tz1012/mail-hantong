//! Bounded Google REST values. All provider text is untrusted display data.
use crate::ErrorCode;
use std::fmt;

pub const GOOGLE_READ_SCOPE: &str = "https://www.googleapis.com/auth/gmail.readonly";
pub const GOOGLE_SCOPES: [&str; 3] = ["openid", "email", GOOGLE_READ_SCOPE];

#[derive(Clone, Eq, PartialEq)]
pub struct GoogleAccountBinding {
    pub access_mode: crate::google_write::GoogleAccessMode,
    pub account_id: String,
    pub subject: String,
    pub client_id: String,
    pub credential_version: u64,
}
#[derive(Clone, Eq, PartialEq)]
pub struct GoogleIdentity {
    pub access_mode: crate::google_write::GoogleAccessMode,
    pub account_id: String,
    pub subject: String,
    pub email: String,
    pub granted_scopes: Vec<String>,
}
#[derive(Clone, Eq, PartialEq)]
pub struct GoogleLabel {
    pub id: String,
    pub name: String,
    pub kind: String,
}
#[derive(Clone, Eq, PartialEq)]
pub struct GoogleMessage {
    pub attachment_checks: Option<Vec<GoogleAttachmentCheck>>,
    pub id: String,
    pub thread_id: String,
    pub internal_date_ms: i64,
    pub label_ids: Vec<String>,
    pub subject: String,
    pub sender: String,
    pub headers: Option<crate::message_body::MessageEnvelope>,
    pub snippet: String,
    pub body_text: Option<String>,
    pub body_error: Option<String>,
    pub body_truncated: bool,
    pub size: u64,
}
#[derive(Clone, Eq, PartialEq)]
pub struct GooglePage {
    pub account_id: String,
    pub subject: String,
    pub labels: Vec<GoogleLabel>,
    pub messages: Vec<GoogleMessage>,
    pub next_page_token: Option<String>,
    pub result_size_estimate: u64,
}
#[derive(Clone, Eq, PartialEq)]
pub struct GoogleSyncRequest {
    pub message_id: Option<String>,
    pub verify_attachments: bool,
    pub account_id: String,
    pub label_id: Option<String>,
    pub page_token: Option<String>,
    pub limit: u16,
    pub expected_version: u64,
    pub operation_id: String,
    pub idempotency_key: String,
}
#[derive(Clone, Eq, PartialEq)]
pub struct GoogleSyncReceipt {
    pub account_id: String,
    pub execution_id: [u8; 32],
    pub state: String,
    pub version: u64,
    pub next_page_token: Option<String>,
    pub error_code: Option<ErrorCode>,
    pub downloaded_count: u64,
}
macro_rules! redacted {
    ($($t:ty),+) => {$(impl fmt::Debug for $t {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(concat!(stringify!($t), "(<redacted>)"))
        }
    })+};
}
redacted!(
    GoogleAccountBinding,
    GoogleIdentity,
    GoogleLabel,
    GoogleMessage,
    GooglePage,
    GoogleSyncRequest,
    GoogleSyncReceipt
);

pub fn validate_google_account_id(v: &str) -> Result<(), ErrorCode> {
    let Some(suffix) = v.strip_prefix("google_") else {
        return Err(ErrorCode::InvalidInput);
    };
    ensure(
        suffix.len() == 64
            && suffix
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
    )
}
pub fn validate_google_client_id(v: &str) -> Result<(), ErrorCode> {
    let Some(prefix) = v.strip_suffix(".apps.googleusercontent.com") else {
        return Err(ErrorCode::InvalidInput);
    };
    ensure(
        !prefix.is_empty()
            && v.len() <= 256
            && prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
    )
}
fn ensure(v: bool) -> Result<(), ErrorCode> {
    if v {
        Ok(())
    } else {
        Err(ErrorCode::InvalidInput)
    }
}
fn id(v: &str) -> Result<(), ErrorCode> {
    ensure(
        !v.is_empty()
            && v.len() <= 256
            && v.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
    )
}
fn subject(v: &str) -> Result<(), ErrorCode> {
    ensure(!v.is_empty() && v.len() <= 255 && v.bytes().all(|b| b.is_ascii_graphic()))
}
fn token(v: &Option<String>) -> Result<(), ErrorCode> {
    ensure(
        v.as_ref()
            .is_none_or(|s| !s.is_empty() && s.len() <= 2048 && !s.chars().any(char::is_control)),
    )
}
fn text(v: &str, max: usize) -> Result<(), ErrorCode> {
    ensure(
        v.len() <= max
            && !v
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t')),
    )
}
impl GoogleAccountBinding {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        validate_google_account_id(&self.account_id)?;
        subject(&self.subject)?;
        validate_google_client_id(&self.client_id)?;
        ensure(self.credential_version > 0 && self.credential_version < i64::MAX as u64)
    }
}
impl GoogleIdentity {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        validate_google_account_id(&self.account_id)?;
        subject(&self.subject)?;
        ensure(
            !self.email.is_empty()
                && self.email.len() <= 320
                && self.email.contains('@')
                && !self.email.chars().any(char::is_control),
        )?;
        ensure(
            self.granted_scopes.len() <= 32
                && self
                    .access_mode
                    .validate_granted(&self.granted_scopes)
                    .is_ok()
                && self.granted_scopes.iter().all(|s| {
                    !s.is_empty() && s.len() <= 256 && s.bytes().all(|b| b.is_ascii_graphic())
                }),
        )
    }
}
impl GoogleLabel {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        id(&self.id)?;
        ensure(!self.name.is_empty())?;
        text(&self.name, 1024)?;
        ensure(matches!(self.kind.as_str(), "system" | "user"))
    }
}
impl GoogleMessage {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        if let Some(checks) = &self.attachment_checks {
            ensure(checks.len() <= crate::google_write::MAX_ATTACHMENT_COUNT)?;
            let mut total = 0usize;
            for check in checks {
                check.validate()?;
                total = total
                    .checked_add(check.size)
                    .ok_or(ErrorCode::QuotaExceeded)?;
            }
            ensure(total <= crate::google_write::MAX_ATTACHMENT_BYTES)?;
        }
        id(&self.id)?;
        id(&self.thread_id)?;
        ensure(self.label_ids.len() <= 1024 && self.size <= i64::MAX as u64)?;
        for l in &self.label_ids {
            id(l)?;
        }
        text(&self.subject, 8192)?;
        text(&self.sender, 8192)?;
        text(&self.snippet, 8192)?;
        if let Some(body) = &self.body_text {
            text(body, 65536)?;
        }
        ensure(self.body_text.is_none() || self.body_error.is_none())?;
        ensure(self.body_error.as_ref().is_none_or(|e| {
            matches!(
                e.as_str(),
                "QUOTA_EXCEEDED" | "CAPABILITY_UNSUPPORTED" | "INVALID_INPUT"
            )
        }))
    }
}
impl GooglePage {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        validate_google_account_id(&self.account_id)?;
        subject(&self.subject)?;
        token(&self.next_page_token)?;
        ensure(
            self.labels.len() <= 1024
                && self.messages.len() <= 32
                && self.result_size_estimate <= i64::MAX as u64,
        )?;
        for l in &self.labels {
            l.validate()?;
        }
        for m in &self.messages {
            m.validate()?;
        }
        let mut labels = std::collections::BTreeSet::new();
        let mut messages = std::collections::BTreeSet::new();
        ensure(
            self.labels.iter().all(|l| labels.insert(&l.id))
                && self.messages.iter().all(|m| messages.insert(&m.id)),
        )
    }
}
impl GoogleSyncRequest {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        validate_google_account_id(&self.account_id)?;
        if let Some(l) = &self.label_id {
            id(l)?;
        }
        token(&self.page_token)?;
        if let Some(message) = &self.message_id {
            id(message)?;
            ensure(
                message.len() <= 128
                    && self.limit == 1
                    && self.label_id.is_none()
                    && self.page_token.is_none(),
            )?;
        }
        id(&self.operation_id)?;
        id(&self.idempotency_key)?;
        ensure(self.operation_id.len() <= 128 && self.idempotency_key.len() <= 128)?;
        ensure(!self.verify_attachments || self.limit == 1)?;
        ensure((1..=32).contains(&self.limit) && self.expected_version < i64::MAX as u64)
    }
    pub fn account_ids(&self) -> &[String] {
        std::slice::from_ref(&self.account_id)
    }
}
impl GoogleSyncReceipt {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        validate_google_account_id(&self.account_id)?;
        token(&self.next_page_token)?;
        ensure(
            self.execution_id != [0; 32]
                && matches!(
                    self.state.as_str(),
                    "pending" | "running" | "completed" | "failed" | "interrupted" | "cancelled"
                )
                && self.version < i64::MAX as u64
                && self.downloaded_count <= 32,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acc01_google_contracts_bound_scope_page_and_redact() {
        let mut r = GoogleSyncRequest {
            message_id: None,
            verify_attachments: false,
            account_id: format!("google_{}", "a".repeat(64)),
            label_id: Some("Label_1".into()),
            page_token: Some("opaque&labelIds=other".into()),
            limit: 32,
            expected_version: 0,
            operation_id: "op".into(),
            idempotency_key: "idem".into(),
        };
        r.validate().unwrap();
        assert!(!format!("{r:?}").contains("opaque"));
        r.limit = 33;
        assert_eq!(r.validate(), Err(ErrorCode::InvalidInput));
        assert!(validate_google_account_id(&format!("google_{}", "A".repeat(64))).is_err());
        assert!(validate_google_client_id("fixture.apps.googleusercontent.com").is_ok());
        assert!(validate_google_client_id("evil/?x.apps.googleusercontent.com").is_err());
    }
}

/// Digest of downloaded, length-checked attachment bytes, never raw content.
#[derive(Clone, Eq, PartialEq)]
pub struct GoogleAttachmentCheck {
    pub filename: String,
    pub media_type: String,
    pub size: usize,
    pub sha256: [u8; 32],
}
impl GoogleAttachmentCheck {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({"filename":self.filename,"media_type":self.media_type,"size":self.size,"sha256":crate::wire_hex::encode(&self.sha256)})
    }
    pub fn from_json(v: &serde_json::Value) -> Result<Self, ErrorCode> {
        let o = v.as_object().ok_or(ErrorCode::InvalidInput)?;
        if o.len() != 4
            || !["filename", "media_type", "size", "sha256"]
                .iter()
                .all(|k| o.contains_key(*k))
        {
            return Err(ErrorCode::InvalidInput);
        }
        let text = |k| {
            o.get(k)
                .and_then(serde_json::Value::as_str)
                .ok_or(ErrorCode::InvalidInput)
        };
        let value = Self {
            filename: text("filename")?.into(),
            media_type: text("media_type")?.into(),
            size: o["size"]
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or(ErrorCode::InvalidInput)?,
            sha256: crate::wire_hex::decode_fixed(text("sha256")?)?,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), ErrorCode> {
        crate::google_write::AttachmentSnapshot::new(
            self.filename.clone(),
            self.media_type.clone(),
            vec![],
        )?;
        ensure(self.size <= crate::google_write::MAX_ATTACHMENT_BYTES)
    }
}
redacted!(GoogleAttachmentCheck);
