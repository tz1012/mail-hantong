//! Typed foreground terminal review messages.
//!
//! These messages carry an opaque, one-use ticket and immutable binding data.
//! They intentionally contain no `approved`/`human` flag and are not an OS
//! authentication proof.
use crate::{ErrorCode, wire_hex};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalReviewKind {
    Registration,
    ScopeRefresh,
    GoogleSetup,
    ImapSetup,
    SmtpSetup,
    GoogleMutation,
    ProtectedSend,
    PersonalSend,
}
impl TerminalReviewKind {
    pub const ALL: [Self; 8] = [
        Self::Registration,
        Self::ScopeRefresh,
        Self::GoogleSetup,
        Self::ImapSetup,
        Self::SmtpSetup,
        Self::GoogleMutation,
        Self::ProtectedSend,
        Self::PersonalSend,
    ];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Registration => "registration",
            Self::ScopeRefresh => "scope_refresh",
            Self::GoogleSetup => "google_setup",
            Self::ImapSetup => "imap_setup",
            Self::SmtpSetup => "smtp_setup",
            Self::GoogleMutation => "google_mutation",
            Self::ProtectedSend => "protected_send",
            Self::PersonalSend => "personal_send",
        }
    }
    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        Self::ALL
            .into_iter()
            .find(|v| v.as_str() == value)
            .ok_or(ErrorCode::InvalidInput)
    }
}

fn valid_account(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[derive(Clone, Eq, PartialEq)]
pub struct TerminalReviewGet {
    pub kind: TerminalReviewKind,
    pub request_id: [u8; 32],
    pub account_id: Option<String>,
    pub payload_hash: [u8; 32],
}
impl TerminalReviewGet {
    pub fn new(
        kind: TerminalReviewKind,
        request_id: [u8; 32],
        account_id: Option<String>,
        payload_hash: [u8; 32],
    ) -> Result<Self, ErrorCode> {
        if request_id == [0; 32] || account_id.as_deref().is_some_and(|v| !valid_account(v)) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            kind,
            request_id,
            account_id,
            payload_hash,
        })
    }
}
impl fmt::Debug for TerminalReviewGet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TerminalReviewGet(<redacted>)")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct TerminalReviewConfirm {
    pub ticket: [u8; 32],
    pub snapshot_hash: [u8; 32],
}
impl TerminalReviewConfirm {
    pub fn new(ticket: [u8; 32], snapshot_hash: [u8; 32]) -> Result<Self, ErrorCode> {
        if ticket == [0; 32] || snapshot_hash == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            ticket,
            snapshot_hash,
        })
    }
}
impl fmt::Debug for TerminalReviewConfirm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TerminalReviewConfirm(<redacted>)")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct TerminalReviewCancel {
    pub get: TerminalReviewGet,
    pub ticket: [u8; 32],
}

#[derive(Clone, Eq, PartialEq)]
pub struct TerminalReviewSecretSubmit {
    pub ticket: [u8; 32],
    pub snapshot_hash: [u8; 32],
    pub secret_ticket_opaque: Vec<u8>,
    pub staged_handle: [u8; 32],
}
impl TerminalReviewSecretSubmit {
    pub fn new(
        ticket: [u8; 32],
        snapshot_hash: [u8; 32],
        secret_ticket_opaque: Vec<u8>,
        staged_handle: [u8; 32],
    ) -> Result<Self, ErrorCode> {
        if ticket == [0; 32]
            || snapshot_hash == [0; 32]
            || secret_ticket_opaque.len() != 320
            || staged_handle == [0; 32]
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            ticket,
            snapshot_hash,
            secret_ticket_opaque,
            staged_handle,
        })
    }
}
impl fmt::Debug for TerminalReviewSecretSubmit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TerminalReviewSecretSubmit(<redacted>)")
    }
}
impl TerminalReviewCancel {
    pub fn new(get: TerminalReviewGet, ticket: [u8; 32]) -> Result<Self, ErrorCode> {
        if ticket == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self { get, ticket })
    }
}
impl fmt::Debug for TerminalReviewCancel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TerminalReviewCancel(<redacted>)")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum TerminalReviewCommand {
    Get(TerminalReviewGet),
    Confirm(TerminalReviewConfirm),
    Cancel(TerminalReviewCancel),
    SecretSubmit(TerminalReviewSecretSubmit),
}
impl fmt::Debug for TerminalReviewCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TerminalReviewCommand(<redacted>)")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct TerminalReviewField {
    pub label: String,
    pub value: String,
}
impl TerminalReviewField {
    pub fn new(label: String, value: String) -> Result<Self, ErrorCode> {
        if label.is_empty()
            || label.len() > 64
            || value.len() > 1_048_576
            || label.chars().any(char::is_control)
            || value.contains('\0')
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self { label, value })
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct TerminalReviewReceipt {
    pub kind: TerminalReviewKind,
    pub request_id: [u8; 32],
    pub ticket: Option<[u8; 32]>,
    pub snapshot_hash: [u8; 32],
    pub owner_registration_id: [u8; 16],
    pub profile_epoch: u64,
    pub expires_at_utc_ms: i64,
    pub state: String,
    pub secret_ticket_opaque: Option<Vec<u8>>,
    pub fields: Vec<TerminalReviewField>,
}
impl TerminalReviewReceipt {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        if self.request_id == [0; 32]
            || self.snapshot_hash == [0; 32]
            || self.owner_registration_id == [0; 16]
            || self.expires_at_utc_ms <= 0
            || self.state.is_empty()
            || self.state.len() > 32
            || self.fields.len() > 64
            || self.fields.iter().any(|v| v.label.is_empty())
            || self
                .secret_ticket_opaque
                .as_ref()
                .is_some_and(|v| v.len() != 320)
        {
            return Err(ErrorCode::InvalidInput);
        }
        if self.state == "pending" && self.ticket.is_none() {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(())
    }
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": self.kind.as_str(), "request_id": wire_hex::encode(&self.request_id),
            "ticket_id": self.ticket.map(|v| wire_hex::encode(&v)),
            "snapshot_hash": wire_hex::encode(&self.snapshot_hash),
            "owner_registration_id": wire_hex::encode(&self.owner_registration_id),
            "profile_epoch": self.profile_epoch, "expires_at_utc_ms": self.expires_at_utc_ms,
            "state": self.state,
            "requires_secret": self.secret_ticket_opaque.is_some(),
            "secret_ticket_opaque": self.secret_ticket_opaque.as_ref().map(|v|wire_hex::encode(v)),
            "fields": self.fields.iter().map(|v| serde_json::json!({"label":v.label,"value":v.value})).collect::<Vec<_>>()
        })
    }
}
impl fmt::Debug for TerminalReviewReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TerminalReviewReceipt(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_review_rejects_forged_empty_bindings_and_unknown_kinds() {
        assert!(TerminalReviewKind::parse("personal_send").is_ok());
        assert!(TerminalReviewKind::parse("unknown").is_err());
        assert!(
            TerminalReviewGet::new(TerminalReviewKind::PersonalSend, [0; 32], None, [1; 32])
                .is_err()
        );
        assert!(
            TerminalReviewGet::new(
                TerminalReviewKind::PersonalSend,
                [1; 32],
                Some("all".into()),
                [2; 32]
            )
            .is_ok()
        );
        assert!(TerminalReviewConfirm::new([0; 32], [2; 32]).is_err());
    }
}
