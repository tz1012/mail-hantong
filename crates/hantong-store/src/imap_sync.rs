//! Atomic storage of bounded read-only IMAP UID observations.
use crate::{Store, mail_connection::MailMutation};
use hantong_contracts::ErrorCode;
use hantong_domain::{
    imap_sync::{ImapMessageMetadata, ImapUidPage},
    mail_connection::ImapEndpointConfig,
    mailboxes::ImapMailboxName,
};
use std::fmt;

#[derive(Clone, Eq, PartialEq)]
pub struct ImapUidApplyReceipt {
    cursor_version: u64,
    covered_through_uid: u32,
    inserted_count: u32,
}
impl fmt::Debug for ImapUidApplyReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapUidApplyReceipt { <redacted> }")
    }
}
impl ImapUidApplyReceipt {
    pub fn cursor_version(&self) -> u64 {
        self.cursor_version
    }
    pub fn covered_through_uid(&self) -> u32 {
        self.covered_through_uid
    }
    pub fn inserted_count(&self) -> u32 {
        self.inserted_count
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct StoredImapUidCursor {
    version: u64,
    account_version: u64,
    catalog_generation: u64,
    endpoint_version: u64,
    uid_validity: u32,
    uid_next_at_start: u32,
    covered_through_uid: u32,
    stored_at_utc_ms: i64,
    binding_current: bool,
}
impl fmt::Debug for StoredImapUidCursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StoredImapUidCursor { <redacted> }")
    }
}
impl StoredImapUidCursor {
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn account_version(&self) -> u64 {
        self.account_version
    }
    pub fn catalog_generation(&self) -> u64 {
        self.catalog_generation
    }
    pub fn endpoint_version(&self) -> u64 {
        self.endpoint_version
    }
    pub fn uid_validity(&self) -> u32 {
        self.uid_validity
    }
    pub fn uid_next_at_start(&self) -> u32 {
        self.uid_next_at_start
    }
    pub fn covered_through_uid(&self) -> u32 {
        self.covered_through_uid
    }
    pub fn stored_at_utc_ms(&self) -> i64 {
        self.stored_at_utc_ms
    }
    pub fn binding_current(&self) -> bool {
        self.binding_current
    }
}
pub fn imap_uid_page_payload_hash(
    account_version: u64,
    catalog_generation: u64,
    endpoint: &ImapEndpointConfig,
    page: &ImapUidPage,
    cursor_version: u64,
) -> Result<[u8; 32], ErrorCode> {
    #[cfg(windows)]
    {
        storage::payload_hash(
            account_version,
            catalog_generation,
            endpoint,
            page,
            cursor_version,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (
            account_version,
            catalog_generation,
            endpoint,
            page,
            cursor_version,
        );
        Err(ErrorCode::CapabilityUnsupported)
    }
}
impl Store {
    pub fn apply_imap_uid_page(
        &self,
        account_version: u64,
        catalog_generation: u64,
        endpoint: &ImapEndpointConfig,
        page: &ImapUidPage,
        mutation: &MailMutation,
    ) -> Result<ImapUidApplyReceipt, ErrorCode> {
        #[cfg(windows)]
        {
            storage::apply(
                self,
                account_version,
                catalog_generation,
                endpoint,
                page,
                mutation,
            )
        }
        #[cfg(not(windows))]
        {
            let _ = (
                account_version,
                catalog_generation,
                endpoint,
                page,
                mutation,
            );
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn load_imap_uid_cursor(
        &self,
        account: &str,
        mailbox: &ImapMailboxName,
    ) -> Result<Option<StoredImapUidCursor>, ErrorCode> {
        #[cfg(windows)]
        {
            storage::cursor(self, account, mailbox)
        }
        #[cfg(not(windows))]
        {
            let _ = (account, mailbox);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn load_imap_uid_metadata(
        &self,
        account: &str,
        mailbox: &ImapMailboxName,
        validity: u32,
        uid: u32,
    ) -> Result<Option<ImapMessageMetadata>, ErrorCode> {
        #[cfg(windows)]
        {
            storage::metadata(self, account, mailbox, validity, uid)
        }
        #[cfg(not(windows))]
        {
            let _ = (account, mailbox, validity, uid);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}
#[cfg(windows)]
pub(crate) mod storage;
#[cfg(windows)]
pub(super) use storage::validate_history;
#[cfg(all(test, windows))]
mod tests;
