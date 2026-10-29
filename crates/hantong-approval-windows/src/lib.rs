#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

//! Core-owned native user verification. Never a CLI/MCP dependency.

use hantong_approval::RegistrationReview;
use hantong_contracts::ErrorCode;
use hantong_domain::config::{Language, Theme};
use std::fmt;

#[cfg(windows)]
mod binding;
#[cfg(windows)]
#[allow(unsafe_code)]
mod native;

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
