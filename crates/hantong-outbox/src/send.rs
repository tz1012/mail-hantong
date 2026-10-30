//! Approved-send orchestration boundary.
//!
//! This module contains no CLI or network code. The Core supplies an approval
//! consumer backed by the native helper and a transport backed by Secrets. A
//! caller cannot mark a message sent without first recording the outbox entry
//! and claiming it with the exact approval receipt.

use super::{Outbox, OutboxEntry, OutboxError, SendEnvelope};

/// Opaque proof returned by the native approval helper. CLI code must forward
/// this value and never create one; `from_native` is intended for the trusted
/// Core/native bridge only.
#[derive(Clone, Eq, PartialEq)]
pub struct ApprovalToken(String);

impl ApprovalToken {
    pub fn from_native(token: impl Into<String>) -> Option<Self> {
        let token = token.into();
        (!token.is_empty() && token.len() <= 512).then_some(Self(token))
    }
}

impl std::fmt::Debug for ApprovalToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApprovalToken(<redacted>)")
    }
}

/// Native helper consumes the token only when it matches the immutable review
/// snapshot. The returned receipt is the only value persisted with Outbox.
pub trait ApprovalConsumer {
    fn consume(
        &mut self,
        token: &ApprovalToken,
        envelope: &SendEnvelope,
    ) -> Result<String, OutboxError>;
}

/// Provider result deliberately distinguishes an unknown response from an
/// explicit failure. Unknown is terminal and cannot be retried automatically.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportResult {
    Sent,
    Failed,
    Unknown,
}

/// SMTP adapter implemented by Core using a Secrets-owned credential handle.
pub trait SendTransport {
    fn send(&mut self, envelope: &SendEnvelope) -> TransportResult;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendOutcome {
    Sent,
    Failed,
    Unknown,
}

/// Coordinates one approved request. Core's persistent Store should provide
/// the same operations transactionally; this in-memory coordinator exists for
/// deterministic fake-provider tests and as the narrow integration seam.
#[derive(Default)]
pub struct SendService {
    outbox: Outbox,
}

impl SendService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn execute<C: ApprovalConsumer, T: SendTransport>(
        &mut self,
        envelope: SendEnvelope,
        token: &ApprovalToken,
        consumer: &mut C,
        transport: &mut T,
    ) -> Result<SendOutcome, OutboxError> {
        let envelope = envelope.validate()?;
        if let Some(existing) = self.outbox.get(&envelope.operation_id) {
            if existing.envelope.idempotency_key == envelope.idempotency_key
                && existing.envelope.payload_hash == envelope.payload_hash
            {
                return match existing.state {
                    super::DeliveryState::Sent => Ok(SendOutcome::Sent),
                    super::DeliveryState::Unknown => Ok(SendOutcome::Unknown),
                    super::DeliveryState::Failed => Ok(SendOutcome::Failed),
                    super::DeliveryState::Queued | super::DeliveryState::Sending => {
                        Err(OutboxError::Conflict)
                    }
                };
            }
            return Err(OutboxError::Conflict);
        }
        let receipt = consumer.consume(token, &envelope)?;
        let operation_id = envelope.operation_id.clone();
        self.outbox.enqueue(envelope, receipt.clone())?;
        let entry = self.outbox.claim(&operation_id, &receipt)?.clone();
        let outcome = transport.send(&entry.envelope);
        match outcome {
            TransportResult::Sent => {
                self.outbox.complete(&operation_id, true)?;
                Ok(SendOutcome::Sent)
            }
            TransportResult::Failed => {
                self.outbox.complete(&operation_id, false)?;
                Ok(SendOutcome::Failed)
            }
            TransportResult::Unknown => {
                self.outbox.mark_unknown(&operation_id)?;
                Ok(SendOutcome::Unknown)
            }
        }
    }

    pub fn entry(&self, operation_id: &str) -> Option<&OutboxEntry> {
        self.outbox.get(operation_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeliveryState;

    struct FakeApproval {
        consumed: bool,
    }
    impl ApprovalConsumer for FakeApproval {
        fn consume(&mut self, _: &ApprovalToken, _: &SendEnvelope) -> Result<String, OutboxError> {
            if self.consumed {
                return Err(OutboxError::ApprovalMismatch);
            }
            self.consumed = true;
            Ok("receipt-1".into())
        }
    }
    struct FakeTransport(TransportResult);
    impl SendTransport for FakeTransport {
        fn send(&mut self, _: &SendEnvelope) -> TransportResult {
            self.0
        }
    }
    fn envelope() -> SendEnvelope {
        SendEnvelope {
            account_id: "a".into(),
            draft_id: "d".into(),
            operation_id: "o".into(),
            idempotency_key: "i".into(),
            expected_version: 1,
            payload_hash: [1; 32],
            recipients: vec!["x@example.test".into()],
            subject: "s".into(),
            body: "b".into(),
        }
    }
    #[test]
    fn send01_persists_before_transport_and_records_success() {
        let token = ApprovalToken::from_native("native").unwrap();
        let mut service = SendService::new();
        let result = service
            .execute(
                envelope(),
                &token,
                &mut FakeApproval { consumed: false },
                &mut FakeTransport(TransportResult::Sent),
            )
            .unwrap();
        assert_eq!(result, SendOutcome::Sent);
        assert_eq!(service.entry("o").unwrap().state, DeliveryState::Sent);
        assert_eq!(service.entry("o").unwrap().attempts, 1);
    }
    #[test]
    fn send01_unknown_is_terminal() {
        let token = ApprovalToken::from_native("native").unwrap();
        let mut service = SendService::new();
        assert_eq!(
            service
                .execute(
                    envelope(),
                    &token,
                    &mut FakeApproval { consumed: false },
                    &mut FakeTransport(TransportResult::Unknown)
                )
                .unwrap(),
            SendOutcome::Unknown
        );
        assert_eq!(service.entry("o").unwrap().state, DeliveryState::Unknown);
        assert!(!Outbox::retry_allowed(DeliveryState::Unknown));
    }
}
