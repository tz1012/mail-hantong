//! Pure, bounded request values for the IMAP folder tools.
use crate::ErrorCode;
use std::fmt;

const MAX_CURSOR: usize = 2220;

#[derive(Clone, Eq, PartialEq)]
pub struct FolderListRequest {
    account_id: String,
    limit: u32,
    cursor: Option<FolderCursor>,
}

impl fmt::Debug for FolderListRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FolderListRequest { <redacted> }")
    }
}

impl FolderListRequest {
    pub fn new(
        account_id: &str,
        limit: Option<u32>,
        cursor: Option<FolderCursor>,
    ) -> Result<Self, ErrorCode> {
        validate_account(account_id)?;
        let limit = limit.unwrap_or(32);
        if !(1..=32).contains(&limit) {
            return Err(ErrorCode::InvalidInput);
        }
        if let Some(value) = &cursor
            && value.account_id() != account_id
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account_id: account_id.to_owned(),
            limit,
            cursor,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn limit(&self) -> u32 {
        self.limit
    }
    pub fn cursor(&self) -> Option<&FolderCursor> {
        self.cursor.as_ref()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct FolderCursor {
    account_id: String,
    generation: u64,
    after_wire_hex: String,
}

impl fmt::Debug for FolderCursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FolderCursor { <redacted> }")
    }
}

impl FolderCursor {
    pub fn new(account_id: &str, generation: u64, after_wire_hex: &str) -> Result<Self, ErrorCode> {
        validate_account(account_id)?;
        if !(1..=i64::MAX as u64).contains(&generation)
            || after_wire_hex.is_empty()
            || after_wire_hex.len() > 2048
            || !after_wire_hex.len().is_multiple_of(2)
            || !after_wire_hex
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(ErrorCode::InvalidInput);
        }
        let bytes = after_wire_hex.len() / 2;
        if !(1..=1024).contains(&bytes) || account_id.len() + after_wire_hex.len() > MAX_CURSOR {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account_id: account_id.to_owned(),
            generation,
            after_wire_hex: after_wire_hex.to_owned(),
        })
    }
    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        let mut parts = value.splitn(4, ':');
        if parts.next() != Some("v1") {
            return Err(ErrorCode::InvalidInput);
        }
        let account = parts.next().ok_or(ErrorCode::InvalidInput)?;
        let generation_text = parts.next().ok_or(ErrorCode::InvalidInput)?;
        if generation_text.is_empty()
            || generation_text == "0"
            || generation_text.starts_with('0')
            || generation_text.starts_with('+')
            || !generation_text.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(ErrorCode::InvalidInput);
        }
        let generation = generation_text
            .parse::<u64>()
            .map_err(|_| ErrorCode::InvalidInput)?;
        let wire = parts.next().ok_or(ErrorCode::InvalidInput)?;
        if value.len() > MAX_CURSOR || value.contains("::") {
            return Err(ErrorCode::InvalidInput);
        }
        Self::new(account, generation, wire)
    }
    pub fn encode(&self) -> String {
        format!(
            "v1:{}:{}:{}",
            self.account_id, self.generation, self.after_wire_hex
        )
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn after_wire_hex(&self) -> &str {
        &self.after_wire_hex
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct FolderRefreshRequest {
    account_id: String,
    operation_id: String,
    idempotency_key: String,
    expected_generation: u64,
    payload_hash: String,
}
impl fmt::Debug for FolderRefreshRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FolderRefreshRequest { <redacted> }")
    }
}
impl FolderRefreshRequest {
    pub fn new(
        account_id: &str,
        operation_id: &str,
        idempotency_key: &str,
        expected_generation: u64,
        payload_hash: &str,
    ) -> Result<Self, ErrorCode> {
        validate_account(account_id)?;
        validate_id(operation_id)?;
        validate_id(idempotency_key)?;
        if expected_generation >= i64::MAX as u64
            || payload_hash.len() != 64
            || !payload_hash
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account_id: account_id.to_owned(),
            operation_id: operation_id.to_owned(),
            idempotency_key: idempotency_key.to_owned(),
            expected_generation,
            payload_hash: payload_hash.to_owned(),
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }
    pub fn expected_generation(&self) -> u64 {
        self.expected_generation
    }
    pub fn payload_hash(&self) -> &str {
        &self.payload_hash
    }
    pub fn intent_bytes(&self) -> Vec<u8> {
        let mut out = b"mail-hantong.imap-folder-refresh-intent.v1\0".to_vec();
        for value in [&self.account_id, &self.operation_id, &self.idempotency_key] {
            out.extend_from_slice(&(value.len() as u32).to_be_bytes());
            out.extend_from_slice(value.as_bytes());
        }
        out.extend_from_slice(&self.expected_generation.to_be_bytes());
        out
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct FolderRefreshStatusRequest {
    account_id: String,
    execution_id: [u8; 32],
}
impl fmt::Debug for FolderRefreshStatusRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FolderRefreshStatusRequest { <redacted> }")
    }
}
impl FolderRefreshStatusRequest {
    pub fn new(account_id: &str, execution_id: [u8; 32]) -> Result<Self, ErrorCode> {
        validate_account(account_id)?;
        if execution_id == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account_id: account_id.to_owned(),
            execution_id,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn execution_id(&self) -> &[u8; 32] {
        &self.execution_id
    }
}

fn validate_account(value: &str) -> Result<(), ErrorCode> {
    validate_id(value)
}
fn validate_id(value: &str) -> Result<(), ErrorCode> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn m203_cursor_roundtrips_and_binds_account() {
        let c = FolderCursor::new("acct", 3, "00ff").unwrap();
        assert_eq!(FolderCursor::parse(&c.encode()).unwrap(), c);
        assert!(FolderListRequest::new("other", None, Some(c)).is_err());
    }
    #[test]
    fn m203_cursor_rejects_uppercase_padding_and_bad_generation() {
        for value in [
            "v1:a:0:aa",
            "v1:a:+1:aa",
            "v1:a:1:AA",
            "v1:a:1:a",
            "v1:a:1:",
        ] {
            assert!(FolderCursor::parse(value).is_err(), "{value}");
        }
    }
    #[test]
    fn m203_refresh_intent_is_canonical_and_debug_redacts() {
        let r = FolderRefreshRequest::new("acct", "op", "key", 0, &"00".repeat(32)).unwrap();
        assert!(
            r.intent_bytes()
                .starts_with(b"mail-hantong.imap-folder-refresh-intent.v1\0")
        );
        assert!(!format!("{r:?}").contains("acct"));
        assert!(
            FolderRefreshRequest::new("acct", "op", "key", i64::MAX as u64, &"00".repeat(32))
                .is_err()
        );
    }
}
