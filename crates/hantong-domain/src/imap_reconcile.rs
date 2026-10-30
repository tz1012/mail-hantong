//! Fixed-range observations of previously stored IMAP UIDs.

use crate::{
    imap_sync::{
        ImapMessageMetadata, ImapSyncError, ImapUidPageRequest, StorageMessage, decode_hex, hex,
    },
    mailboxes::{ImapMailboxName, MailboxEncoding},
};
use serde::{Deserialize, Serialize};
use std::fmt;

const MAX_PAGE_JSON: usize = 8 * 1024 * 1024;

#[derive(Clone, Eq, PartialEq)]
pub struct ImapUidReconcileRequest {
    account_id: String,
    endpoint_version: u64,
    mailbox: ImapMailboxName,
    uid_validity: u32,
    after_uid: u32,
    through_uid: u32,
    minimum_uid_next: u32,
}
impl fmt::Debug for ImapUidReconcileRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapUidReconcileRequest { <redacted> }")
    }
}
impl ImapUidReconcileRequest {
    pub fn new(
        account_id: &str,
        endpoint_version: u64,
        mailbox: ImapMailboxName,
        uid_validity: u32,
        after_uid: u32,
        through_uid: u32,
        minimum_uid_next: u32,
    ) -> Result<Self, ImapSyncError> {
        if after_uid >= through_uid
            || through_uid == u32::MAX
            || through_uid - after_uid > 64
            || minimum_uid_next <= through_uid
        {
            return Err(ImapSyncError::InvalidInput);
        }
        // Reuse the existing identity/endpoint validation and canonical mailbox
        // reconstruction, without using that page's growing range calculation.
        let bound = ImapUidPageRequest::new(
            account_id,
            endpoint_version,
            mailbox,
            Some(uid_validity),
            after_uid,
        )?;
        Ok(Self {
            account_id: bound.account_id().into(),
            endpoint_version,
            mailbox: bound.mailbox().clone(),
            uid_validity,
            after_uid,
            through_uid,
            minimum_uid_next,
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
    pub fn uid_validity(&self) -> u32 {
        self.uid_validity
    }
    pub fn after_uid(&self) -> u32 {
        self.after_uid
    }
    pub fn through_uid(&self) -> u32 {
        self.through_uid
    }
    pub fn minimum_uid_next(&self) -> u32 {
        self.minimum_uid_next
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapUidReconcilePage {
    request: ImapUidReconcileRequest,
    uid_next_at_start: u32,
    messages: Vec<ImapMessageMetadata>,
}
impl fmt::Debug for ImapUidReconcilePage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapUidReconcilePage { <redacted> }")
    }
}
impl ImapUidReconcilePage {
    pub fn new(
        request: ImapUidReconcileRequest,
        uid_next_at_start: u32,
        mut messages: Vec<ImapMessageMetadata>,
    ) -> Result<Self, ImapSyncError> {
        if uid_next_at_start < request.minimum_uid_next() {
            return Err(ImapSyncError::CursorInvalid);
        }
        if messages.len() > 64 {
            return Err(ImapSyncError::TooLarge);
        }
        if messages
            .iter()
            .any(|m| m.uid() <= request.after_uid() || m.uid() > request.through_uid())
        {
            return Err(ImapSyncError::CursorInvalid);
        }
        messages.sort_by_key(ImapMessageMetadata::uid);
        if messages
            .windows(2)
            .any(|pair| pair[0].uid() == pair[1].uid())
        {
            return Err(ImapSyncError::InvalidInput);
        }
        let value = Self {
            request,
            uid_next_at_start,
            messages,
        };
        value.to_storage_json()?;
        Ok(value)
    }
    pub fn request(&self) -> &ImapUidReconcileRequest {
        &self.request
    }
    pub fn uid_next_at_start(&self) -> u32 {
        self.uid_next_at_start
    }
    pub fn messages(&self) -> &[ImapMessageMetadata] {
        &self.messages
    }
    pub fn to_storage_json(&self) -> Result<Vec<u8>, ImapSyncError> {
        let wire = StorageReconcilePage {
            schema_version: 1,
            account_id: self.request.account_id.clone(),
            endpoint_version: self.request.endpoint_version,
            wire_hex: hex(self.request.mailbox.wire_name()),
            encoding: match self.request.mailbox.encoding() {
                MailboxEncoding::ModifiedUtf7 => "modified_utf7",
                MailboxEncoding::Utf8 => "utf8",
            }
            .into(),
            uid_validity: self.request.uid_validity,
            after_uid: self.request.after_uid,
            through_uid: self.request.through_uid,
            minimum_uid_next: self.request.minimum_uid_next,
            uid_next_at_start: self.uid_next_at_start,
            messages: self
                .messages
                .iter()
                .map(StorageMessage::from_message)
                .collect::<Result<_, _>>()?,
        };
        let bytes = serde_json::to_vec(&wire).map_err(|_| ImapSyncError::InvalidStorage)?;
        if bytes.len() > MAX_PAGE_JSON {
            return Err(ImapSyncError::TooLarge);
        }
        Ok(bytes)
    }
    pub fn from_storage_json(bytes: &[u8]) -> Result<Self, ImapSyncError> {
        if bytes.len() > MAX_PAGE_JSON {
            return Err(ImapSyncError::TooLarge);
        }
        let wire: StorageReconcilePage =
            serde_json::from_slice(bytes).map_err(|_| ImapSyncError::InvalidStorage)?;
        let page = wire
            .into_page()
            .map_err(|_| ImapSyncError::InvalidStorage)?;
        if page.to_storage_json()?.as_slice() != bytes {
            return Err(ImapSyncError::InvalidStorage);
        }
        Ok(page)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageReconcilePage {
    schema_version: u8,
    account_id: String,
    endpoint_version: u64,
    wire_hex: String,
    encoding: String,
    uid_validity: u32,
    after_uid: u32,
    through_uid: u32,
    minimum_uid_next: u32,
    uid_next_at_start: u32,
    messages: Vec<StorageMessage>,
}
impl StorageReconcilePage {
    fn into_page(self) -> Result<ImapUidReconcilePage, ImapSyncError> {
        if self.schema_version != 1 || self.messages.len() > 64 || self.wire_hex.len() > 2048 {
            return Err(ImapSyncError::InvalidStorage);
        }
        let encoding = match self.encoding.as_str() {
            "modified_utf7" => MailboxEncoding::ModifiedUtf7,
            "utf8" => MailboxEncoding::Utf8,
            _ => return Err(ImapSyncError::InvalidStorage),
        };
        let mailbox = ImapMailboxName::from_wire(&decode_hex(&self.wire_hex)?, encoding)
            .map_err(|_| ImapSyncError::InvalidStorage)?;
        let request = ImapUidReconcileRequest::new(
            &self.account_id,
            self.endpoint_version,
            mailbox,
            self.uid_validity,
            self.after_uid,
            self.through_uid,
            self.minimum_uid_next,
        )?;
        ImapUidReconcilePage::new(
            request,
            self.uid_next_at_start,
            self.messages
                .into_iter()
                .map(StorageMessage::into_message)
                .collect::<Result<_, _>>()?,
        )
    }
}

#[cfg(test)]
mod tests;
