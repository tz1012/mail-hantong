//! A bounded EXAMINE result without messages or a reusable provider session.
use crate::{
    imap_sync::{ImapSyncError, ImapUidPageRequest},
    mailboxes::ImapMailboxName,
};
use std::fmt;

#[derive(Clone, Eq, PartialEq)]
pub struct ImapUidObservationRequest {
    account_id: String,
    endpoint_version: u64,
    mailbox: ImapMailboxName,
}
impl fmt::Debug for ImapUidObservationRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapUidObservationRequest(<redacted>)")
    }
}
impl ImapUidObservationRequest {
    pub fn new(
        account_id: &str,
        endpoint_version: u64,
        mailbox: ImapMailboxName,
    ) -> Result<Self, ImapSyncError> {
        let bound = ImapUidPageRequest::new(account_id, endpoint_version, mailbox, None, 0)?;
        Ok(Self {
            account_id: bound.account_id().into(),
            endpoint_version,
            mailbox: bound.mailbox().clone(),
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
}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapUidObservation {
    request: ImapUidObservationRequest,
    uid_validity: u32,
    uid_next: u32,
}
impl fmt::Debug for ImapUidObservation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapUidObservation(<redacted>)")
    }
}
impl ImapUidObservation {
    pub fn new(
        request: ImapUidObservationRequest,
        uid_validity: u32,
        uid_next: u32,
    ) -> Result<Self, ImapSyncError> {
        if uid_validity == 0 || uid_next == 0 {
            return Err(ImapSyncError::InvalidInput);
        }
        Ok(Self {
            request,
            uid_validity,
            uid_next,
        })
    }
    pub fn request(&self) -> &ImapUidObservationRequest {
        &self.request
    }
    pub fn uid_validity(&self) -> u32 {
        self.uid_validity
    }
    pub fn uid_next(&self) -> u32 {
        self.uid_next
    }
}

#[cfg(test)]
mod tests;
