//! A single account-bound IMAP move intent. None of these values is approval.
use crate::{
    ErrorCode,
    uid_page::{UidCursorRequest, UidMailboxEncoding, UidMailboxIdentity},
    wire_hex,
};
use serde_json::{Value, json};

#[derive(Clone, Eq, PartialEq)]
pub struct ImapTrashRequest {
    account_id: String,
    mailbox: UidMailboxIdentity,
    catalog_generation: u64,
    uidvalidity: u32,
    uid: u32,
    operation_id: String,
    idempotency_key: String,
    payload_hash: [u8; 32],
}
impl std::fmt::Debug for ImapTrashRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ImapTrashRequest(<redacted>)")
    }
}
impl ImapTrashRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        account: &str,
        mailbox: UidMailboxIdentity,
        generation: u64,
        uidvalidity: u32,
        uid: u32,
        operation: &str,
        idempotency: &str,
        payload_hash: [u8; 32],
    ) -> Result<Self, ErrorCode> {
        UidCursorRequest::new(account, mailbox.clone())?;
        let valid_id = |value: &str| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        };
        if generation == 0
            || generation > i64::MAX as u64
            || uidvalidity == 0
            || uid == 0
            || !valid_id(operation)
            || !valid_id(idempotency)
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account_id: account.into(),
            mailbox,
            catalog_generation: generation,
            uidvalidity,
            uid,
            operation_id: operation.into(),
            idempotency_key: idempotency.into(),
            payload_hash,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn mailbox(&self) -> &UidMailboxIdentity {
        &self.mailbox
    }
    pub fn catalog_generation(&self) -> u64 {
        self.catalog_generation
    }
    pub fn uidvalidity(&self) -> u32 {
        self.uidvalidity
    }
    pub fn uid(&self) -> u32 {
        self.uid
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
    pub fn payload_bytes(&self) -> Vec<u8> {
        let mut out = b"mail-hantong.imap.trash.intent.v1\0".to_vec();
        for text in [
            &self.account_id,
            self.mailbox.wire_hex(),
            self.mailbox.encoding().as_str(),
            &self.operation_id,
            &self.idempotency_key,
        ] {
            out.extend_from_slice(&(text.len() as u32).to_be_bytes());
            out.extend_from_slice(text.as_bytes());
        }
        out.extend_from_slice(&self.catalog_generation.to_be_bytes());
        out.extend_from_slice(&self.uidvalidity.to_be_bytes());
        out.extend_from_slice(&self.uid.to_be_bytes());
        out
    }
    pub fn to_json(&self) -> Value {
        json!({"kind":"imap_trash", "account_id":self.account_id,
            "mailbox_wire_hex":self.mailbox.wire_hex(), "mailbox_encoding":self.mailbox.encoding().as_str(),
            "expected_catalog_generation":self.catalog_generation, "uidvalidity":self.uidvalidity,
            "uid":self.uid, "operation_id":self.operation_id, "idempotency_key":self.idempotency_key,
            "payload_hash":wire_hex::encode(&self.payload_hash)})
    }
    pub fn from_json(value: &Value) -> Result<Self, ErrorCode> {
        let invalid = || ErrorCode::InvalidInput;
        let object = value.as_object().ok_or_else(invalid)?;
        if object.len() != 10 || value["kind"] != "imap_trash" {
            return Err(invalid());
        }
        let text = |key: &str| value.get(key).and_then(Value::as_str).ok_or_else(invalid);
        let number = |key: &str| value.get(key).and_then(Value::as_u64).ok_or_else(invalid);
        let encoding = match text("mailbox_encoding")? {
            "utf8" => UidMailboxEncoding::Utf8,
            "modified_utf7" => UidMailboxEncoding::ModifiedUtf7,
            _ => return Err(invalid()),
        };
        Self::new(
            text("account_id")?,
            UidMailboxIdentity::new(text("mailbox_wire_hex")?, encoding)?,
            number("expected_catalog_generation")?,
            u32::try_from(number("uidvalidity")?).map_err(|_| invalid())?,
            u32::try_from(number("uid")?).map_err(|_| invalid())?,
            text("operation_id")?,
            text("idempotency_key")?,
            wire_hex::decode_fixed(text("payload_hash")?)?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(account: &str, uid: u32) -> ImapTrashRequest {
        ImapTrashRequest::new(
            account,
            UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::Utf8).unwrap(),
            3,
            7,
            uid,
            "operation",
            "idempotency",
            [1; 32],
        )
        .unwrap()
    }
    #[test]
    fn sec03_imap_trash_intent_binds_exact_account_mailbox_generation_and_uid() {
        let first = request("first", 9);
        assert_eq!(
            ImapTrashRequest::from_json(&first.to_json()).unwrap(),
            first
        );
        assert_ne!(first.payload_bytes(), request("second", 9).payload_bytes());
        assert_ne!(first.payload_bytes(), request("first", 10).payload_bytes());
        let mut value = first.to_json();
        value["expected_catalog_generation"] = json!(4);
        assert_ne!(
            first.payload_bytes(),
            ImapTrashRequest::from_json(&value).unwrap().payload_bytes()
        );
        value["approved"] = json!(true);
        assert_eq!(
            ImapTrashRequest::from_json(&value),
            Err(ErrorCode::InvalidInput)
        );
    }
    #[test]
    fn ext04_imap_trash_rejects_missing_identity_and_multiple_uids() {
        let value = request("first", 9).to_json();
        for (key, invalid) in [
            ("uid", json!(0)),
            ("uid", json!("1:*")),
            ("uidvalidity", json!(0)),
            ("expected_catalog_generation", json!(0)),
            ("operation_id", json!("bad\r\ncommand")),
        ] {
            let mut altered = value.clone();
            altered[key] = invalid;
            assert_eq!(
                ImapTrashRequest::from_json(&altered),
                Err(ErrorCode::InvalidInput)
            );
        }
    }
}
