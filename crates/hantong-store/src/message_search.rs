//! Bounded local IMAP metadata search.
use crate::Store;
use hantong_contracts::{
    ErrorCode,
    message_search::{ImapMetadataSearchPage, ImapMetadataSearchRequest},
};

impl Store {
    pub fn search_imap_metadata(
        &self,
        request: &ImapMetadataSearchRequest,
    ) -> Result<ImapMetadataSearchPage, ErrorCode> {
        #[cfg(windows)]
        {
            storage::search(self, request)
        }
        #[cfg(not(windows))]
        {
            let _ = request;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}

#[cfg(windows)]
mod storage;

#[cfg(all(test, windows))]
mod tests;
