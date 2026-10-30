//! Untrusted compose/review inputs. None of these values is approval authority.
use crate::ErrorCode;
use std::fmt;

pub const MAX_COMPOSE_BYTES: usize = 1024 * 1024;

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
}

#[derive(Clone, PartialEq, Eq)]
pub struct DraftCreateRequest {
    account_id: String,
    recipients: Vec<String>,
    subject: String,
    body: String,
    operation_id: String,
    idempotency_key: String,
    payload_hash: [u8; 32],
}
impl fmt::Debug for DraftCreateRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DraftCreateRequest(<redacted>)")
    }
}
impl DraftCreateRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        account_id: &str,
        recipients: Vec<String>,
        subject: String,
        body: String,
        operation_id: &str,
        idempotency_key: &str,
        expected_version: u64,
        payload_hash: [u8; 32],
    ) -> Result<Self, ErrorCode> {
        if account_id.is_empty()
            || account_id.len() > 128
            || !account_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || !identifier(operation_id)
            || !identifier(idempotency_key)
            || expected_version != 0
            || recipients.is_empty()
            || recipients.len() > 100
            || subject.len() > 998
            || subject.chars().any(char::is_control)
            || body.len() > MAX_COMPOSE_BYTES
            || body.contains('\0')
        {
            return Err(ErrorCode::InvalidInput);
        }
        let mut seen = std::collections::BTreeSet::new();
        for address in &recipients {
            if address.is_empty()
                || address.len() > 320
                || !address.is_ascii()
                || address
                    .bytes()
                    .any(|b| b <= 32 || b >= 127 || b"<>(),;\\\"".contains(&b))
                || address.matches('@').count() != 1
                || address.starts_with('@')
                || address.ends_with('@')
                || !seen.insert(address.clone())
            {
                return Err(ErrorCode::InvalidInput);
            }
        }
        Ok(Self {
            account_id: account_id.into(),
            recipients,
            subject,
            body,
            operation_id: operation_id.into(),
            idempotency_key: idempotency_key.into(),
            payload_hash,
        })
    }
    pub fn account_ids(&self) -> &[String] {
        std::slice::from_ref(&self.account_id)
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn recipients(&self) -> &[String] {
        &self.recipients
    }
    pub fn subject(&self) -> &str {
        &self.subject
    }
    pub fn body(&self) -> &str {
        &self.body
    }
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }
    pub fn payload_hash(&self) -> &[u8; 32] {
        &self.payload_hash
    }
    pub fn canonical_payload(&self) -> Vec<u8> {
        let mut bytes = b"mail-hantong.draft.v1\0".to_vec();
        // JSON serialization of these already bounded strings cannot fail.
        bytes.extend(
            serde_json::to_vec(&(
                &self.account_id,
                &self.recipients,
                &self.subject,
                &self.body,
            ))
            .expect("bounded strings"),
        );
        bytes
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SendReviewRequest {
    account_id: String,
    draft_id: [u8; 32],
    expected_version: u64,
    operation_id: String,
    idempotency_key: String,
    payload_hash: [u8; 32],
}
impl fmt::Debug for SendReviewRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SendReviewRequest(<redacted>)")
    }
}
impl SendReviewRequest {
    pub fn new(
        account_id: &str,
        draft_id: [u8; 32],
        expected_version: u64,
        operation_id: &str,
        idempotency_key: &str,
        payload_hash: [u8; 32],
    ) -> Result<Self, ErrorCode> {
        if account_id.is_empty()
            || account_id.len() > 128
            || !account_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || draft_id == [0; 32]
            || !(1..=i64::MAX as u64).contains(&expected_version)
            || !identifier(operation_id)
            || !identifier(idempotency_key)
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account_id: account_id.into(),
            draft_id,
            expected_version,
            operation_id: operation_id.into(),
            idempotency_key: idempotency_key.into(),
            payload_hash,
        })
    }
    pub fn account_ids(&self) -> &[String] {
        std::slice::from_ref(&self.account_id)
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn draft_id(&self) -> &[u8; 32] {
        &self.draft_id
    }
    pub fn expected_version(&self) -> u64 {
        self.expected_version
    }
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }
    pub fn payload_hash(&self) -> &[u8; 32] {
        &self.payload_hash
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SendReviewStatusRequest {
    pub account_id: String,
    pub review_id: [u8; 32],
}
impl fmt::Debug for SendReviewStatusRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SendReviewStatusRequest(<redacted>)")
    }
}
