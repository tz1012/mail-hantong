//! Bounded mutation data, never native approval or permission authority.
use crate::{
    ErrorCode, google::validate_google_account_id, google_write::MessageLabelChange,
    mail_send::SendReviewRequest, wire_hex,
};
use serde_json::{Value, json};
use std::fmt;

pub const REQUEST_TOOL: &str = "mail.google.changes.request";
pub const EXECUTE_TOOL: &str = "mail.google.changes.execute";
const GRANT_PREFIX: &str = "mail.google.change.";

pub fn account_grant(account: &str) -> Result<String, ErrorCode> {
    validate_google_account_id(account)?;
    Ok(format!("{GRANT_PREFIX}{}", &account[7..]))
}
pub fn grant_account(grant: &str) -> Option<String> {
    let account = format!("google_{}", grant.strip_prefix(GRANT_PREFIX)?);
    validate_google_account_id(&account).ok()?;
    Some(account)
}

#[derive(Clone, Eq, PartialEq)]
pub enum MutationPayload {
    SaveDraft(SendReviewRequest),
    Labels {
        change: MessageLabelChange,
        expected_version: u64,
        operation_id: String,
        idempotency_key: String,
        payload_hash: [u8; 32],
    },
}
impl fmt::Debug for MutationPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MutationPayload(<redacted>)")
    }
}
impl MutationPayload {
    pub fn from_json(value: &Value) -> Result<Self, ErrorCode> {
        let obj = value.as_object().ok_or(ErrorCode::InvalidInput)?;
        let text = |key| {
            obj.get(key)
                .and_then(Value::as_str)
                .ok_or(ErrorCode::InvalidInput)
        };
        let account = text("account_id")?;
        validate_google_account_id(account)?;
        let operation = text("operation_id")?;
        let key = text("idempotency_key")?;
        let version = obj
            .get("expected_version")
            .and_then(Value::as_u64)
            .ok_or(ErrorCode::InvalidInput)?;
        let hash = wire_hex::decode_fixed(text("payload_hash")?)?;
        match text("kind")? {
            "save_draft" if obj.len() == 7 => Ok(Self::SaveDraft(SendReviewRequest::new(
                account,
                wire_hex::decode_fixed(text("draft_id")?)?,
                version,
                operation,
                key,
                hash,
            )?)),
            "labels" if obj.len() == 9 => {
                // Reuse the bounded identifiers/version contract; this sentinel is
                // validation data only and is never persisted as a draft target.
                SendReviewRequest::new(account, [1; 32], version, operation, key, hash)?;
                let list = |name| {
                    serde_json::from_value::<Vec<String>>(
                        obj.get(name).cloned().ok_or(ErrorCode::InvalidInput)?,
                    )
                    .map_err(|_| ErrorCode::InvalidInput)
                };
                let change = MessageLabelChange::new(
                    account.into(),
                    text("message_id")?.into(),
                    list("add_labels")?,
                    list("remove_labels")?,
                )?;
                Ok(Self::Labels {
                    change,
                    expected_version: version,
                    operation_id: operation.into(),
                    idempotency_key: key.into(),
                    payload_hash: hash,
                })
            }
            _ => Err(ErrorCode::InvalidInput),
        }
    }
    pub fn to_json(&self) -> Value {
        match self {
            Self::SaveDraft(r) => json!({"kind":"save_draft","account_id":r.account_id(),
                "draft_id":wire_hex::encode(r.draft_id()),"expected_version":r.expected_version(),
                "operation_id":r.operation_id(),"idempotency_key":r.idempotency_key(),
                "payload_hash":wire_hex::encode(r.payload_hash())}),
            Self::Labels {
                change,
                expected_version,
                operation_id,
                idempotency_key,
                payload_hash,
            } => {
                json!({"kind":"labels","account_id":change.account_id(),"message_id":change.message_id(),
                    "expected_version":expected_version,"operation_id":operation_id,"idempotency_key":idempotency_key,
                    "payload_hash":wire_hex::encode(payload_hash),"add_labels":change.add(),"remove_labels":change.remove()})
            }
        }
    }
    pub fn account_id(&self) -> &str {
        match self {
            Self::SaveDraft(r) => r.account_id(),
            Self::Labels { change, .. } => change.account_id(),
        }
    }
    pub fn account_ids(&self) -> Vec<String> {
        vec![self.account_id().into()]
    }
    pub fn operation_id(&self) -> &str {
        match self {
            Self::SaveDraft(r) => r.operation_id(),
            Self::Labels { operation_id, .. } => operation_id,
        }
    }
    pub fn idempotency_key(&self) -> &str {
        match self {
            Self::SaveDraft(r) => r.idempotency_key(),
            Self::Labels {
                idempotency_key, ..
            } => idempotency_key,
        }
    }
    pub fn expected_version(&self) -> u64 {
        match self {
            Self::SaveDraft(r) => r.expected_version(),
            Self::Labels {
                expected_version, ..
            } => *expected_version,
        }
    }
    pub fn payload_hash(&self) -> &[u8; 32] {
        match self {
            Self::SaveDraft(r) => r.payload_hash(),
            Self::Labels { payload_hash, .. } => payload_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn labels() -> Value {
        json!({"kind":"labels","account_id":format!("google_{}","a".repeat(64)),
        "message_id":"abc123","expected_version":1,"operation_id":"test-op","idempotency_key":"test-key",
        "payload_hash":"01".repeat(32),"add_labels":["STARRED"],"remove_labels":["UNREAD"]})
    }
    #[test]
    fn sec03_mutation_inputs_roundtrip_and_never_accept_approval_or_delete() {
        let value = labels();
        let request = MutationPayload::from_json(&value).unwrap();
        assert_eq!(request.to_json(), value);
        assert_eq!(format!("{request:?}"), "MutationPayload(<redacted>)");
        for field in ["approved", "path", "token", "url"] {
            let mut v = value.clone();
            v[field] = json!(true);
            assert!(MutationPayload::from_json(&v).is_err());
        }
        for kind in ["delete", "trash", "batch", "send"] {
            let mut v = value.clone();
            v["kind"] = json!(kind);
            assert!(MutationPayload::from_json(&v).is_err());
        }
        for label in ["TRASH", "SPAM", "DRAFT", "SENT"] {
            let mut v = value.clone();
            v["add_labels"] = json!([label]);
            assert!(MutationPayload::from_json(&v).is_err());
        }
    }
    #[test]
    fn sec03_mutation_rejects_invalid_version_identity_and_overlap() {
        for (field, value) in [
            ("expected_version", json!(0)),
            ("expected_version", json!(u64::MAX)),
            ("account_id", json!("other")),
            ("message_id", json!("../trash")),
            ("operation_id", json!("")),
            ("remove_labels", json!(["STARRED"])),
        ] {
            let mut v = labels();
            v[field] = value;
            assert!(MutationPayload::from_json(&v).is_err());
        }
    }
}
