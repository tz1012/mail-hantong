//! Pure bounded public UID-page values; these are never authentication proof.
use crate::ErrorCode;
use std::fmt;

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum UidMailboxEncoding {
    ModifiedUtf7,
    Utf8,
}
impl UidMailboxEncoding {
    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        match value {
            "modified_utf7" => Ok(Self::ModifiedUtf7),
            "utf8" => Ok(Self::Utf8),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ModifiedUtf7 => "modified_utf7",
            Self::Utf8 => "utf8",
        }
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct UidMailboxIdentity {
    wire_hex: String,
    encoding: UidMailboxEncoding,
}
impl UidMailboxIdentity {
    pub fn new(wire_hex: &str, encoding: UidMailboxEncoding) -> Result<Self, ErrorCode> {
        if wire_hex.is_empty() {
            return Err(ErrorCode::InvalidInput);
        }
        crate::wire_hex::decode_bounded(wire_hex, 1024)?;
        Ok(Self {
            wire_hex: wire_hex.into(),
            encoding,
        })
    }
    pub fn wire_hex(&self) -> &str {
        &self.wire_hex
    }
    pub fn encoding(&self) -> UidMailboxEncoding {
        self.encoding
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct UidCursorRequest {
    account_id: String,
    mailbox: UidMailboxIdentity,
}
impl UidCursorRequest {
    pub fn new(account_id: &str, mailbox: UidMailboxIdentity) -> Result<Self, ErrorCode> {
        id(account_id)?;
        Ok(Self {
            account_id: account_id.into(),
            mailbox,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn mailbox(&self) -> &UidMailboxIdentity {
        &self.mailbox
    }
    pub fn account_ids(&self) -> &[String] {
        std::slice::from_ref(&self.account_id)
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct UidPageRequest {
    account_id: String,
    mailbox: UidMailboxIdentity,
    operation_id: String,
    idempotency_key: String,
    expected_catalog_generation: u64,
    expected_cursor_version: u64,
    intent_hash: String,
}
impl UidPageRequest {
    pub fn new(
        account_id: &str,
        mailbox: UidMailboxIdentity,
        operation_id: &str,
        idempotency_key: &str,
        expected_catalog_generation: u64,
        expected_cursor_version: u64,
        intent_hash: &str,
    ) -> Result<Self, ErrorCode> {
        id(account_id)?;
        id(operation_id)?;
        id(idempotency_key)?;
        positive(expected_catalog_generation)?;
        if expected_cursor_version >= i64::MAX as u64 {
            return Err(ErrorCode::InvalidInput);
        }
        crate::wire_hex::decode_fixed::<32>(intent_hash)?;
        Ok(Self {
            account_id: account_id.into(),
            mailbox,
            operation_id: operation_id.into(),
            idempotency_key: idempotency_key.into(),
            expected_catalog_generation,
            expected_cursor_version,
            intent_hash: intent_hash.into(),
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn mailbox(&self) -> &UidMailboxIdentity {
        &self.mailbox
    }
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }
    pub fn expected_catalog_generation(&self) -> u64 {
        self.expected_catalog_generation
    }
    pub fn expected_cursor_version(&self) -> u64 {
        self.expected_cursor_version
    }
    pub fn intent_hash(&self) -> &str {
        &self.intent_hash
    }
    pub fn account_ids(&self) -> &[String] {
        std::slice::from_ref(&self.account_id)
    }
    pub fn intent_bytes(&self) -> Vec<u8> {
        let mut out = b"mail-hantong.imap-uid-page-intent.v1\0".to_vec();
        for v in [
            self.account_id(),
            self.operation_id(),
            self.idempotency_key(),
            self.mailbox.encoding().as_str(),
            self.mailbox.wire_hex(),
        ] {
            out.extend_from_slice(&(v.len() as u32).to_be_bytes());
            out.extend_from_slice(v.as_bytes());
        }
        out.extend_from_slice(&self.expected_catalog_generation.to_be_bytes());
        out.extend_from_slice(&self.expected_cursor_version.to_be_bytes());
        out
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct UidPageStatusRequest {
    account_id: String,
    execution_id: [u8; 32],
}
impl UidPageStatusRequest {
    pub fn new(account_id: &str, execution_id: [u8; 32]) -> Result<Self, ErrorCode> {
        id(account_id)?;
        if execution_id == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account_id: account_id.into(),
            execution_id,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn execution_id(&self) -> &[u8; 32] {
        &self.execution_id
    }
    pub fn account_ids(&self) -> &[String] {
        std::slice::from_ref(&self.account_id)
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct UidCursorSnapshot {
    account_id: String,
    mailbox: UidMailboxIdentity,
    catalog_generation: u64,
    cursor_version: u64,
    covered_through_uid: u32,
    binding_current: bool,
}
impl UidCursorSnapshot {
    pub fn new(
        account_id: &str,
        mailbox: UidMailboxIdentity,
        catalog_generation: u64,
        cursor_version: u64,
        covered_through_uid: u32,
        binding_current: bool,
    ) -> Result<Self, ErrorCode> {
        id(account_id)?;
        positive(catalog_generation)?;
        if cursor_version > i64::MAX as u64
            || covered_through_uid == u32::MAX
            || (cursor_version == 0 && covered_through_uid != 0)
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account_id: account_id.into(),
            mailbox,
            catalog_generation,
            cursor_version,
            covered_through_uid,
            binding_current,
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
    pub fn cursor_version(&self) -> u64 {
        self.cursor_version
    }
    pub fn covered_through_uid(&self) -> u32 {
        self.covered_through_uid
    }
    pub fn binding_current(&self) -> bool {
        self.binding_current
    }
}
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum UidPageState {
    Unresolved,
    Succeeded,
    Failed,
}
impl UidPageState {
    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        match value {
            "unresolved" => Ok(Self::Unresolved),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unresolved => "unresolved",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct UidPageReceipt {
    account_id: String,
    execution_id: [u8; 32],
    state: UidPageState,
    cursor_version: Option<u64>,
    covered_through_uid: Option<u32>,
    inserted_count: Option<u32>,
    error: Option<ErrorCode>,
}
impl UidPageReceipt {
    pub fn new(
        account_id: &str,
        execution_id: [u8; 32],
        state: UidPageState,
        cursor_version: Option<u64>,
        covered_through_uid: Option<u32>,
        inserted_count: Option<u32>,
        error: Option<ErrorCode>,
    ) -> Result<Self, ErrorCode> {
        id(account_id)?;
        if execution_id == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        match (
            state,
            cursor_version,
            covered_through_uid,
            inserted_count,
            error,
        ) {
            (UidPageState::Unresolved, None, None, None, None)
            | (UidPageState::Failed, None, None, None, Some(_)) => {}
            (UidPageState::Succeeded, Some(version), Some(covered), Some(count), None)
                if version > 0
                    && version <= i64::MAX as u64
                    && covered < u32::MAX
                    && count <= 64
                    && count <= covered => {}
            _ => return Err(ErrorCode::InvalidInput),
        }
        Ok(Self {
            account_id: account_id.into(),
            execution_id,
            state,
            cursor_version,
            covered_through_uid,
            inserted_count,
            error,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn execution_id(&self) -> &[u8; 32] {
        &self.execution_id
    }
    pub fn state(&self) -> UidPageState {
        self.state
    }
    pub fn cursor_version(&self) -> Option<u64> {
        self.cursor_version
    }
    pub fn covered_through_uid(&self) -> Option<u32> {
        self.covered_through_uid
    }
    pub fn inserted_count(&self) -> Option<u32> {
        self.inserted_count
    }
    pub fn error(&self) -> Option<ErrorCode> {
        self.error
    }
}
fn id(v: &str) -> Result<(), ErrorCode> {
    if v.is_empty()
        || v.len() > 128
        || !v
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}
fn positive(v: u64) -> Result<(), ErrorCode> {
    if v == 0 || v > i64::MAX as u64 {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}
macro_rules! redacted {($($name:ty),+)=>{$(impl fmt::Debug for $name {fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{f.write_str(concat!(stringify!($name)," { <redacted> }"))}})+};}
redacted!(
    UidMailboxEncoding,
    UidMailboxIdentity,
    UidCursorRequest,
    UidPageRequest,
    UidPageStatusRequest,
    UidCursorSnapshot,
    UidPageState,
    UidPageReceipt
);
#[cfg(test)]
#[path = "uid_page_tests.rs"]
mod tests;
