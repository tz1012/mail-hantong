#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

//! Core-owned native user verification. Never a CLI/MCP dependency.

use hantong_approval::GoogleClientReview;
use hantong_approval::{
    AccountSetupReview, GoogleMutationReview, GoogleSetupReview, RegistrationReview,
    ScopeRefreshReview, SendReview, SmtpSetupReview,
};
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

/// Core startup guard for the fixed profile directories. This only creates
/// absent product/profile directories with the protected ACL and retains
/// handles through core lifetime; it never publishes a profile or issues an
/// approval.
pub struct PreparedProfileDirectories {
    #[cfg(windows)]
    _inner: profile_publication::DirectoryGuard,
    #[cfg(not(windows))]
    _private: (),
}

/// Core startup guard for the fixed data and secrets directories. This only
/// creates absent leaves with an explicit protected ACL and validates existing
/// leaves without repairing them.
pub struct PreparedStorageDirectories {
    #[cfg(windows)]
    _inner: profile_publication::DirectoryGuard,
    #[cfg(not(windows))]
    _private: (),
}

#[cfg(windows)]
pub fn prepare_profile_directories(
    directories: &hantong_platform::AppDirectories,
) -> Result<PreparedProfileDirectories, ErrorCode> {
    profile_publication::prepare_directories(&directories.profiles)
        .map(|inner| PreparedProfileDirectories { _inner: inner })
}

#[cfg(windows)]
pub fn prepare_storage_directories(
    directories: &hantong_platform::AppDirectories,
) -> Result<PreparedStorageDirectories, ErrorCode> {
    profile_publication::prepare_storage_directories(directories)
        .map(|inner| PreparedStorageDirectories { _inner: inner })
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
pub fn publish_mcp_profile(bytes: &[u8]) -> Result<PublishedProfile, ErrorCode> {
    #[cfg(windows)]
    {
        profile_publication::publish_mcp(bytes).map(|inner| PublishedProfile { _inner: inner })
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
pub fn publish_ai_profile(
    provider: hantong_contracts::registration::AiProvider,
    bytes: &[u8],
) -> Result<PublishedProfile, ErrorCode> {
    #[cfg(windows)]
    {
        profile_publication::publish_ai(provider, bytes)
            .map(|inner| PublishedProfile { _inner: inner })
    }
    #[cfg(not(windows))]
    {
        let _ = (provider, bytes);
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

/// Verifies a refreshed default profile through the native publication's
/// already-pinned handles. Content is read from the held object; separate
/// metadata-only handles verify its identity at the fixed Default path.
#[cfg(windows)]
pub fn verify_published_default_profile(
    publication: &PublishedProfile,
) -> Result<hantong_platform::account_publication::VerifiedAccountProfilePublication, ErrorCode> {
    profile_publication::verify_default_publication(&publication._inner)
}

/// Loads the exact old default profile retained for a request-bound refresh.
/// The native implementation accepts only the fixed application profile path.
pub fn load_default_profile_refresh_backup(refresh_id: [u8; 32]) -> Result<Vec<u8>, ErrorCode> {
    #[cfg(windows)]
    {
        profile_publication::load_default_refresh_backup(refresh_id)
    }
    #[cfg(not(windows))]
    {
        let _ = refresh_id;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// Removes the exact old default profile backup after validating its bytes.
/// This operation never writes or replaces the active profile.
pub fn discard_default_profile_refresh_backup(
    refresh_id: [u8; 32],
    expected_old_bytes: &[u8],
) -> Result<(), ErrorCode> {
    #[cfg(windows)]
    {
        profile_publication::discard_default_refresh_backup(refresh_id, expected_old_bytes)
    }
    #[cfg(not(windows))]
    {
        let _ = (refresh_id, expected_old_bytes);
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// Protected compare-and-replace for an explicitly native-reviewed MCP identity replacement.
pub fn refresh_mcp_profile(
    request_id: [u8; 32],
    old_bytes: &[u8],
    new_bytes: &[u8],
) -> Result<PublishedProfile, ErrorCode> {
    #[cfg(windows)]
    {
        profile_publication::refresh_mcp(request_id, old_bytes, new_bytes)
            .map(|inner| PublishedProfile { _inner: inner })
    }
    #[cfg(not(windows))]
    {
        let _ = (request_id, old_bytes, new_bytes);
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

/// Single-use native account-connection verification, never a send approval.
pub struct VerifiedGoogleSetup {
    #[cfg(windows)]
    binding: binding::CapturedReview<GoogleSetupReview>,
    #[cfg(windows)]
    verified_at_utc_ms: i64,
    #[cfg(windows)]
    pending: Option<hantong_secrets::google::PendingGoogleCredential>,
    #[cfg(not(windows))]
    _private: (),
}
impl fmt::Debug for VerifiedGoogleSetup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerifiedGoogleSetup(<redacted>)")
    }
}
impl VerifiedGoogleSetup {
    pub fn consume(
        self,
        expected: &GoogleSetupReview,
    ) -> Result<hantong_secrets::google::PendingGoogleCredential, ErrorCode> {
        #[cfg(windows)]
        {
            self.binding.consume(expected, self.verified_at_utc_ms)?;
            let pending = self.pending.ok_or(ErrorCode::InternalError)?;
            validate_google_pending(expected, &pending)?;
            Ok(pending)
        }
        #[cfg(not(windows))]
        {
            let _ = expected;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}
fn validate_google_pending(
    review: &GoogleSetupReview,
    pending: &hantong_secrets::google::PendingGoogleCredential,
) -> Result<(), ErrorCode> {
    if pending.identity() != &review.identity
        || pending.binding(review.binding.credential_version)? != review.binding
    {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(())
}
pub async fn review_google_setup(
    review: GoogleSetupReview,
    pending: hantong_secrets::google::PendingGoogleCredential,
    language: Option<Language>,
    theme: Theme,
) -> Result<VerifiedGoogleSetup, ErrorCode> {
    review.validate()?;
    validate_google_pending(&review, &pending)?;
    #[cfg(windows)]
    {
        let mut verified = native::show_google_setup(review, language, theme).await?;
        verified.pending = Some(pending);
        Ok(verified)
    }
    #[cfg(not(windows))]
    {
        let _ = (review, pending, language, theme);
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// Single-use result, constructible only by this module's owned OS verification.
pub struct VerifiedSend {
    #[cfg(windows)]
    binding: binding::CapturedReview<SendReview>,
    #[cfg(windows)]
    verified_at_utc_ms: i64,
    #[cfg(not(windows))]
    _private: (),
}

/// Single-use result for the narrowly scoped personal CLI send confirmation.
/// This is intentionally a different type from `VerifiedSend`: it carries no
/// OS identity-verification claim and cannot enter protected review branches.
pub struct ConfirmedPersonalSend {
    #[cfg(windows)]
    binding: binding::CapturedReview<SendReview>,
    #[cfg(windows)]
    confirmed_at_utc_ms: i64,
    #[cfg(not(windows))]
    _private: (),
}
impl fmt::Debug for ConfirmedPersonalSend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ConfirmedPersonalSend(<redacted>)")
    }
}
impl ConfirmedPersonalSend {
    pub fn consume(self, expected: &SendReview) -> Result<SendReview, ErrorCode> {
        #[cfg(windows)]
        {
            self.binding.consume(expected, self.confirmed_at_utc_ms)
        }
        #[cfg(not(windows))]
        {
            let _ = expected;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}
impl fmt::Debug for VerifiedSend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerifiedSend(<redacted>)")
    }
}
impl VerifiedSend {
    pub fn consume(self, expected: &SendReview) -> Result<SendReview, ErrorCode> {
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
pub async fn review_send(
    review: SendReview,
    language: Option<Language>,
    theme: Theme,
) -> Result<VerifiedSend, ErrorCode> {
    review.validate()?;
    #[cfg(windows)]
    {
        native::show_send(review, language, theme).await
    }
    #[cfg(not(windows))]
    {
        let _ = (review, language, theme);
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// Shows the personal CLI message review. The native `Send` button issues a
/// private, message-bound confirmation without invoking Windows Hello.
pub async fn confirm_personal_send(
    review: SendReview,
    language: Option<Language>,
    theme: Theme,
) -> Result<ConfirmedPersonalSend, ErrorCode> {
    review.validate()?;
    #[cfg(windows)]
    {
        native::show_personal_send(review, language, theme).await
    }
    #[cfg(not(windows))]
    {
        let _ = (review, language, theme);
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// SMTP secret capture stays opaque and inside Secrets even after verification.
pub struct VerifiedSmtpSetup {
    #[cfg(windows)]
    binding: binding::CapturedReview<SmtpSetupReview>,
    #[cfg(windows)]
    verified_at_utc_ms: i64,
    #[cfg(windows)]
    pending: hantong_secrets::PendingImapCredential,
    #[cfg(not(windows))]
    _private: (),
}
impl fmt::Debug for VerifiedSmtpSetup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerifiedSmtpSetup(<redacted>)")
    }
}
impl VerifiedSmtpSetup {
    pub fn consume(
        self,
        expected: &SmtpSetupReview,
    ) -> Result<hantong_secrets::PendingImapCredential, ErrorCode> {
        #[cfg(windows)]
        {
            self.binding.consume(expected, self.verified_at_utc_ms)?;
            setup_binding::validate_pending(expected.capture_review(), &self.pending)?;
            Ok(self.pending)
        }
        #[cfg(not(windows))]
        {
            let _ = expected;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}
pub async fn review_smtp_setup(
    review: SmtpSetupReview,
    plan: hantong_secrets::SmtpPasswordCapturePlan,
    language: Option<Language>,
    theme: Theme,
) -> Result<VerifiedSmtpSetup, ErrorCode> {
    #[cfg(windows)]
    {
        native::show_smtp_setup(review, plan, language, theme).await
    }
    #[cfg(not(windows))]
    {
        let _ = (review, plan, language, theme);
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// Only the owned native review and Windows verification can create this value.
pub struct VerifiedGoogleClient {
    #[cfg(windows)]
    binding: binding::CapturedReview<GoogleClientReview>,
    #[cfg(windows)]
    verified_at_utc_ms: i64,
    #[cfg(windows)]
    pending: hantong_secrets::PendingImapCredential,
    #[cfg(not(windows))]
    _private: (),
}
impl fmt::Debug for VerifiedGoogleClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerifiedGoogleClient(<redacted>)")
    }
}
impl VerifiedGoogleClient {
    pub fn consume(
        self,
        expected: &GoogleClientReview,
    ) -> Result<hantong_secrets::google::GoogleClientSecret, ErrorCode> {
        #[cfg(windows)]
        {
            self.binding.consume(expected, self.verified_at_utc_ms)?;
            setup_binding::validate_pending(expected.capture_review(), &self.pending)?;
            self.pending.into_google_client_secret(expected.client_id())
        }
        #[cfg(not(windows))]
        {
            let _ = expected;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}
pub async fn review_google_client(
    review: GoogleClientReview,
    plan: hantong_secrets::ImapPasswordCapturePlan,
    language: Option<Language>,
    theme: Theme,
) -> Result<VerifiedGoogleClient, ErrorCode> {
    #[cfg(windows)]
    {
        native::show_google_client(review, plan, language, theme).await
    }
    #[cfg(not(windows))]
    {
        let _ = (review, plan, language, theme);
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// Single-use result, constructible only by this module's owned OS verification.
pub struct VerifiedGoogleMutation {
    #[cfg(windows)]
    binding: binding::CapturedReview<GoogleMutationReview>,
    #[cfg(windows)]
    verified_at_utc_ms: i64,
    #[cfg(not(windows))]
    _private: (),
}
impl fmt::Debug for VerifiedGoogleMutation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerifiedGoogleMutation(<redacted>)")
    }
}
impl VerifiedGoogleMutation {
    pub fn consume(
        self,
        expected: &GoogleMutationReview,
    ) -> Result<GoogleMutationReview, ErrorCode> {
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
pub async fn review_google_mutation(
    review: GoogleMutationReview,
    language: Option<Language>,
    theme: Theme,
) -> Result<VerifiedGoogleMutation, ErrorCode> {
    review.validate()?;
    #[cfg(windows)]
    {
        native::show_google_mutation(review, language, theme).await
    }
    #[cfg(not(windows))]
    {
        let _ = (review, language, theme);
        Err(ErrorCode::CapabilityUnsupported)
    }
}
