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
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            storage::search(self, request)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = request;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod storage;

#[cfg(all(test, windows))]
mod tests;
