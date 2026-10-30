#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

//! Core-owned native user verification. Never a CLI/MCP dependency.

use hantong_approval::{AccountSetupReview, RegistrationReview, ScopeRefreshReview};
use hantong_contracts::ErrorCode;
use hantong_domain::config::{Language, Theme};
use std::fmt;

#[cfg(windows)]
mod binding;
#[cfg(windows)]
#[allow(unsafe_code)]
mod native;
#[cfg(windows)]
mod setup_binding;

#[cfg(windows)]
#[allow(unsafe_code)]
mod profile_publication;

/// Holds the published file and pinned directories through Store activation.
/// Publication is not an authorization or activation result.
pub struct PublishedProfile {
    #[cfg(windows)]
    _inner: profile_publication::Publication,
    #[cfg(not(windows))]
    _private: (),
}

impl fmt::Debug for PublishedProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PublishedProfile { <redacted> }")
    }
}

/// Core-only fixed-path public profile publication. The caller supplies canonical
/// validated public bytes after its approved registration has been staged.
pub fn publish_default_profile(bytes: &[u8]) -> Result<PublishedProfile, ErrorCode> {
    #[cfg(windows)]
    {
        profile_publication::publish(bytes).map(|inner| PublishedProfile { _inner: inner })
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// Core-only publication to the distinct fixed GUI profile after approved staging.
pub fn publish_gui_profile(bytes: &[u8]) -> Result<PublishedProfile, ErrorCode> {
    #[cfg(windows)]
    {
        profile_publication::publish_gui(bytes).map(|inner| PublishedProfile { _inner: inner })
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// Publishes a new default profile while retaining the exact old profile as a
/// request-bound backup until Store activation. This is not authorization.
pub fn refresh_default_profile(
    refresh_id: [u8; 32],
    old_bytes: &[u8],
    new_bytes: &[u8],
) -> Result<PublishedProfile, ErrorCode> {
    #[cfg(windows)]
    {
        profile_publication::refresh(refresh_id, old_bytes, new_bytes)
            .map(|inner| PublishedProfile { _inner: inner })
    }
    #[cfg(not(windows))]
    {
        let _ = (refresh_id, old_bytes, new_bytes);
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// Removes the inert old-profile backup after Store activation. Failure leaves
/// the active profile intact and the backup recoverable for explicit cleanup.
pub fn cleanup_profile_refresh_backup(publication: &mut PublishedProfile) -> Result<(), ErrorCode> {
    #[cfg(windows)]
    {
        profile_publication::cleanup_backup(&mut publication._inner)
    }
    #[cfg(not(windows))]
    {
        let _ = publication;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// In-process, single-use result of this crate's owned window and OS verifier.
/// No constructor, cloning, serialization or boolean conversion is exposed.
pub struct VerifiedRegistration {
    #[cfg(windows)]
    binding: binding::CapturedReview,
    #[cfg(windows)]
    verified_at_utc_ms: i64,
    #[cfg(not(windows))]
    _private: (),
}
impl fmt::Debug for VerifiedRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerifiedRegistration { <redacted> }")
    }
}
impl VerifiedRegistration {
    pub fn consume(self, expected: &RegistrationReview) -> Result<RegistrationReview, ErrorCode> {
        #[cfg(windows)]
        {
            self.binding.consume(expected, self.verified_at_utc_ms)
        }
        #[cfg(not(windows))]
        {
            let _ = expected;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}

/// Single-use OS-verified setup result owning its exact secret capture.
pub struct VerifiedImapSetup {
    #[cfg(windows)]
    binding: binding::CapturedReview<AccountSetupReview>,
    #[cfg(windows)]
    verified_at_utc_ms: i64,
    #[cfg(windows)]
    pending: hantong_secrets::PendingImapCredential,
    #[cfg(not(windows))]
    _private: (),
}

/// Single-use OS-verified default CLI scope refresh result.
pub struct VerifiedScopeRefresh {
    #[cfg(windows)]
    binding: binding::CapturedReview<ScopeRefreshReview>,
    #[cfg(windows)]
    verified_at_utc_ms: i64,
    #[cfg(not(windows))]
    _private: (),
}
impl fmt::Debug for VerifiedScopeRefresh {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerifiedScopeRefresh { <redacted> }")
    }
}
impl VerifiedScopeRefresh {
    pub fn consume(self, expected: &ScopeRefreshReview) -> Result<ScopeRefreshReview, ErrorCode> {
        #[cfg(windows)]
        {
            self.binding.consume(expected, self.verified_at_utc_ms)
        }
        #[cfg(not(windows))]
        {
            let _ = expected;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}
impl fmt::Debug for VerifiedImapSetup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerifiedImapSetup { <redacted> }")
    }
}
impl VerifiedImapSetup {
    pub fn consume(
        self,
        expected: &AccountSetupReview,
    ) -> Result<hantong_secrets::PendingImapCredential, ErrorCode> {
        #[cfg(windows)]
        {
            self.binding.consume(expected, self.verified_at_utc_ms)?;
            setup_binding::validate_pending(expected, &self.pending)?;
            Ok(self.pending)
        }
        #[cfg(not(windows))]
        {
            let _ = expected;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}

/// Opens the shared core-owned review window with a secret-owned password input.
/// Dropping the future cancels verification and discards any pending capture.
pub async fn review_imap_setup(
    review: AccountSetupReview,
    plan: hantong_secrets::ImapPasswordCapturePlan,
    language: Option<Language>,
    theme: Theme,
) -> Result<VerifiedImapSetup, ErrorCode> {
    #[cfg(windows)]
    {
        native::show_setup(review, plan, language, theme).await
    }
    #[cfg(not(windows))]
    {
        let _ = (review, plan, language, theme);
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// Opens the core-owned registration window lazily. Dropping this future cancels
/// the window/verification. The caller must revalidate Store state before commit.
pub async fn review_registration(
    review: RegistrationReview,
    language: Option<Language>,
    theme: Theme,
) -> Result<VerifiedRegistration, ErrorCode> {
    #[cfg(windows)]
    {
        native::show(review, language, theme).await
    }
    #[cfg(not(windows))]
    {
        let _ = (review, language, theme);
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// Opens the core-owned default CLI scope refresh review window.
pub async fn review_scope_refresh(
    review: ScopeRefreshReview,
    language: Option<Language>,
    theme: Theme,
) -> Result<VerifiedScopeRefresh, ErrorCode> {
    #[cfg(windows)]
    {
        native::show_scope_refresh(review, language, theme).await
    }
    #[cfg(not(windows))]
    {
        let _ = (review, language, theme);
        Err(ErrorCode::CapabilityUnsupported)
    }
}
