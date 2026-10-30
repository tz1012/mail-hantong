//! An explicit first-page observation of a changed IMAP UID namespace.

use crate::{
    imap_sync::{ImapSyncError, ImapUidPage, ImapUidPageRequest},
    mailboxes::ImapMailboxName,
};
use std::fmt;

#[derive(Clone, Eq, PartialEq)]
pub struct ImapUidRecoveryRequest {
    account_id: String,
    endpoint_version: u64,
    mailbox: ImapMailboxName,
    previous_uid_validity: u32,
}
impl fmt::Debug for ImapUidRecoveryRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapUidRecoveryRequest { <redacted> }")
    }
}
impl ImapUidRecoveryRequest {
    pub fn new(
        account_id: &str,
        endpoint_version: u64,
        mailbox: ImapMailboxName,
        previous_uid_validity: u32,
    ) -> Result<Self, ImapSyncError> {
        let bound = ImapUidPageRequest::new(
            account_id,
            endpoint_version,
            mailbox,
            Some(previous_uid_validity),
            0,
        )?;
        Ok(Self {
            account_id: bound.account_id().into(),
            endpoint_version,
            mailbox: bound.mailbox().clone(),
            previous_uid_validity,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn endpoint_version(&self) -> u64 {
        self.endpoint_version
    }
    pub fn mailbox(&self) -> &ImapMailboxName {
        &self.mailbox
    }
    pub fn previous_uid_validity(&self) -> u32 {
        self.previous_uid_validity
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapUidRecoveryPage {
    request: ImapUidRecoveryRequest,
    page: ImapUidPage,
}
impl fmt::Debug for ImapUidRecoveryPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapUidRecoveryPage { <redacted> }")
    }
}
impl ImapUidRecoveryPage {
    pub fn new(request: ImapUidRecoveryRequest, page: ImapUidPage) -> Result<Self, ImapSyncError> {
        if page.uid_validity() == request.previous_uid_validity() {
            return Err(ImapSyncError::CursorInvalid);
        }
        let bound = page.request();
        if bound.account_id() != request.account_id()
            || bound.endpoint_version() != request.endpoint_version()
            || bound.mailbox() != request.mailbox()
            || bound.expected_uid_validity() != Some(page.uid_validity())
            || bound.after_uid() != 0
        {
            return Err(ImapSyncError::InvalidInput);
        }
        Ok(Self { request, page })
    }
    pub fn request(&self) -> &ImapUidRecoveryRequest {
        &self.request
    }
    pub fn page(&self) -> &ImapUidPage {
        &self.page
    }
}

#[cfg(test)]
mod tests;
