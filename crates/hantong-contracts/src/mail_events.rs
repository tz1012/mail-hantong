//! Bounded metadata-only receive events shared by Core and local clients.

use crate::ErrorCode;

pub const MAX_EVENT_KIND_BYTES: usize = 32;
pub const MAX_MAILBOX_BYTES: usize = 512;

#[derive(Clone, Eq, PartialEq)]
pub struct MailEventTarget {
    pub account_id: String,
    pub mailbox_key: String,
    pub mailbox_encoding: String,
}

#[derive(Clone, Eq, PartialEq)]
pub struct MailEvent {
    pub event_id: u64,
    pub account_id: String,
    pub mailbox_key: String,
    pub mailbox_encoding: String,
    pub change_kind: String,
    pub cursor: String,
    pub profile_epoch: u64,
}

#[derive(Clone, Eq, PartialEq)]
pub struct MailEventPage {
    pub events: Vec<MailEvent>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

impl std::fmt::Debug for MailEventTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MailEventTarget(<redacted>)")
    }
}

impl std::fmt::Debug for MailEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MailEvent")
            .field("event_id", &self.event_id)
            .field("change_kind", &self.change_kind)
            .field("profile_epoch", &self.profile_epoch)
            .finish()
    }
}

impl std::fmt::Debug for MailEventPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MailEventPage")
            .field("event_count", &self.events.len())
            .field("has_more", &self.has_more)
            .finish()
    }
}

pub fn validate_target(target: &MailEventTarget) -> Result<(), ErrorCode> {
    if target.account_id.is_empty()
        || target.account_id.len() > 128
        || !target
            .account_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        || target.mailbox_key.is_empty()
        || target.mailbox_key.len() > MAX_MAILBOX_BYTES
        || target.mailbox_encoding.is_empty()
        || target.mailbox_encoding.len() > 32
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_never_carry_content_fields() {
        let event = MailEvent {
            event_id: 1,
            account_id: "acct".into(),
            mailbox_key: "INBOX".into(),
            mailbox_encoding: "utf8".into(),
            change_kind: "new_message".into(),
            cursor: "cursor".into(),
            profile_epoch: 1,
        };
        let debug = format!("{event:?}");
        assert!(!debug.contains("subject"));
        assert!(!debug.contains("body"));
    }
}
