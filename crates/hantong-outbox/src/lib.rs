#![forbid(unsafe_code)]

//! Durable send intent state and the no-automatic-retry boundary.
//!
//! This crate deliberately has no network or approval dependencies. The core
//! owns approval consumption and persists an [`OutboxEntry`] before calling a
//! provider. A provider response that cannot be classified is terminal
//! [`DeliveryState::Unknown`]; callers must never claim it is safe to retry.

use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SendEnvelope {
    pub account_id: String,
    pub draft_id: String,
    pub operation_id: String,
    pub idempotency_key: String,
    pub expected_version: i64,
    pub payload_hash: [u8; 32],
    pub recipients: Vec<String>,
    pub subject: String,
    pub body: String,
}

impl SendEnvelope {
    pub fn validate(self) -> Result<Self, OutboxError> {
        if self.account_id.is_empty()
            || self.draft_id.is_empty()
            || self.operation_id.is_empty()
            || self.idempotency_key.is_empty()
            || self.expected_version < 1
            || self.recipients.is_empty()
            || self.recipients.len() > 100
            || self.subject.len() > 998
            || self.body.len() > 16 * 1024 * 1024
            || self.recipients.iter().any(|value| {
                value.is_empty()
                    || value.len() > 320
                    || value.bytes().any(|b| b < 0x20 || b == 0x7f)
            })
            || self
                .subject
                .bytes()
                .any(|b| b == 0 || b == 0x0a || b == 0x0d)
            || self.body.bytes().any(|b| b == 0)
        {
            return Err(OutboxError::InvalidEnvelope);
        }
        if self.recipients.iter().any(|address| {
            !address.contains('@') || address.starts_with('@') || address.ends_with('@')
        }) {
            return Err(OutboxError::InvalidRecipient);
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryState {
    Queued,
    Sending,
    Sent,
    Unknown,
    Failed,
}

impl DeliveryState {
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Sent | Self::Unknown | Self::Failed)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboxEntry {
    pub envelope: SendEnvelope,
    pub state: DeliveryState,
    pub attempts: u32,
    pub approval_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutboxError {
    InvalidEnvelope,
    InvalidRecipient,
    Conflict,
    NotFound,
    Terminal,
    ApprovalRequired,
    ApprovalMismatch,
}

/// An in-process durable-state model used by the Core Store adapter.
///
/// The map is intentionally keyed by operation id and idempotency key is
/// indexed separately. A real Store implementation serializes this same
/// transition in one SQLCipher transaction with approval consumption and the
/// audit start. This type gives that adapter a strict, provider-independent
/// state machine and fake-provider tests without network access.
#[derive(Default)]
pub struct Outbox {
    entries: BTreeMap<String, OutboxEntry>,
    idempotency: BTreeMap<String, String>,
}

impl Outbox {
    pub fn enqueue(
        &mut self,
        envelope: SendEnvelope,
        approval_id: impl Into<String>,
    ) -> Result<&OutboxEntry, OutboxError> {
        let envelope = envelope.validate()?;
        let approval_id = approval_id.into();
        if approval_id.is_empty() {
            return Err(OutboxError::ApprovalRequired);
        }
        if let Some(operation_id) = self.idempotency.get(&envelope.idempotency_key) {
            let existing = self
                .entries
                .get(operation_id)
                .ok_or(OutboxError::Conflict)?;
            if existing.envelope.payload_hash == envelope.payload_hash
                && existing.envelope.account_id == envelope.account_id
                && existing.envelope.operation_id == envelope.operation_id
            {
                return Ok(existing);
            }
            return Err(OutboxError::Conflict);
        }
        if self.entries.contains_key(&envelope.operation_id) {
            return Err(OutboxError::Conflict);
        }
        let operation_id = envelope.operation_id.clone();
        let key = envelope.idempotency_key.clone();
        self.idempotency.insert(key, operation_id.clone());
        self.entries.insert(
            operation_id.clone(),
            OutboxEntry {
                envelope,
                state: DeliveryState::Queued,
                attempts: 0,
                approval_id,
            },
        );
        Ok(self
            .entries
            .get(&operation_id)
            .expect("inserted outbox entry"))
    }

    pub fn get(&self, operation_id: &str) -> Option<&OutboxEntry> {
        self.entries.get(operation_id)
    }

    pub fn claim(
        &mut self,
        operation_id: &str,
        approval_id: &str,
    ) -> Result<&OutboxEntry, OutboxError> {
        let entry = self
            .entries
            .get_mut(operation_id)
            .ok_or(OutboxError::NotFound)?;
        if entry.approval_id != approval_id {
            return Err(OutboxError::ApprovalMismatch);
        }
        if entry.state.terminal() || entry.state == DeliveryState::Sending {
            return Err(OutboxError::Terminal);
        }
        entry.attempts = entry.attempts.saturating_add(1);
        entry.state = DeliveryState::Sending;
        Ok(self
            .entries
            .get(operation_id)
            .expect("claimed entry exists"))
    }

    pub fn complete(
        &mut self,
        operation_id: &str,
        sent: bool,
    ) -> Result<&OutboxEntry, OutboxError> {
        let entry = self
            .entries
            .get_mut(operation_id)
            .ok_or(OutboxError::NotFound)?;
        if entry.state != DeliveryState::Sending {
            return Err(OutboxError::Conflict);
        }
        entry.state = if sent {
            DeliveryState::Sent
        } else {
            DeliveryState::Failed
        };
        Ok(self
            .entries
            .get(operation_id)
            .expect("completed entry exists"))
    }

    pub fn mark_unknown(&mut self, operation_id: &str) -> Result<&OutboxEntry, OutboxError> {
        let entry = self
            .entries
            .get_mut(operation_id)
            .ok_or(OutboxError::NotFound)?;
        if entry.state != DeliveryState::Sending {
            return Err(OutboxError::Conflict);
        }
        entry.state = DeliveryState::Unknown;
        Ok(self
            .entries
            .get(operation_id)
            .expect("unknown entry exists"))
    }

    /// Unknown delivery is deliberately not retryable. The caller must obtain
    /// a provider receipt or create a new, separately approved operation.
    pub const fn retry_allowed(state: DeliveryState) -> bool {
        matches!(state, DeliveryState::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(key: &str) -> SendEnvelope {
        SendEnvelope {
            account_id: "acct-1".into(),
            draft_id: "draft-1".into(),
            operation_id: key.into(),
            idempotency_key: key.into(),
            expected_version: 1,
            payload_hash: [7; 32],
            recipients: vec!["person@example.test".into()],
            subject: "Hello".into(),
            body: "Body".into(),
        }
    }

    #[test]
    fn outbox01_idempotent_enqueue_returns_same_entry_without_second_send() {
        let mut outbox = Outbox::default();
        let first = outbox
            .enqueue(envelope("op-1"), "approval-1")
            .unwrap()
            .clone();
        let replay = outbox.enqueue(envelope("op-1"), "approval-1").unwrap();
        assert_eq!(first, *replay);
        assert_eq!(outbox.get("op-1").unwrap().attempts, 0);
    }

    #[test]
    fn outbox01_unknown_is_terminal_and_never_retryable() {
        let mut outbox = Outbox::default();
        outbox.enqueue(envelope("op-1"), "approval-1").unwrap();
        outbox.claim("op-1", "approval-1").unwrap();
        let entry = outbox.mark_unknown("op-1").unwrap();
        assert_eq!(entry.state, DeliveryState::Unknown);
        assert!(!Outbox::retry_allowed(entry.state));
        assert_eq!(
            outbox.claim("op-1", "approval-1"),
            Err(OutboxError::Terminal)
        );
    }

    #[test]
    fn outbox01_wrong_approval_and_payload_conflicts_are_rejected() {
        let mut outbox = Outbox::default();
        outbox.enqueue(envelope("op-1"), "approval-1").unwrap();
        assert_eq!(
            outbox.claim("op-1", "other"),
            Err(OutboxError::ApprovalMismatch)
        );
        let mut changed = envelope("op-2");
        changed.idempotency_key = "op-1".into();
        changed.payload_hash = [8; 32];
        assert_eq!(
            outbox.enqueue(changed, "approval-2"),
            Err(OutboxError::Conflict)
        );
    }

    #[test]
    fn outbox01_send_result_transitions_are_strict() {
        let mut outbox = Outbox::default();
        outbox.enqueue(envelope("op-1"), "approval-1").unwrap();
        assert_eq!(outbox.complete("op-1", true), Err(OutboxError::Conflict));
        outbox.claim("op-1", "approval-1").unwrap();
        assert_eq!(
            outbox.complete("op-1", true).unwrap().state,
            DeliveryState::Sent
        );
        assert_eq!(outbox.mark_unknown("op-1"), Err(OutboxError::Conflict));
    }
}
