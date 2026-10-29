//! OS-facing seams. Implementations must keep secrets and authorization proof
//! behind their owning crate; these traits never return plaintext or approval.

use std::io::Read;

pub const MAX_IPC_FRAME: usize = 1_048_576;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformError {
    InvalidInput,
    Unsupported,
    Denied,
    NotFound,
    FrameTooLarge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutoStartState {
    Enabled,
    Disabled,
    OwnershipConflict,
}

#[derive(Clone, Eq, PartialEq)]
pub struct VerificationChallenge {
    pub request_id: String,
    pub account_id: String,
    pub operation_hash: [u8; 32],
    pub nonce: [u8; 32],
    pub expires_at_unix: u64,
    pub profile_epoch: u64,
}

impl std::fmt::Debug for VerificationChallenge {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerificationChallenge")
            .field("request_id", &"<redacted>")
            .field("account_id", &"<redacted>")
            .field("operation_hash", &"<redacted>")
            .field("nonce", &"<redacted>")
            .field("expires_at_unix", &self.expires_at_unix)
            .field("profile_epoch", &self.profile_epoch)
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct UnverifiedEvidence {
    pub request_id: String,
    pub account_id: String,
    pub operation_hash: [u8; 32],
    pub nonce: [u8; 32],
    pub expires_at_unix: u64,
    pub profile_epoch: u64,
    /// Opaque, untrusted platform assertion. The policy/approval owner must validate it.
    pub assertion: Vec<u8>,
}

impl std::fmt::Debug for UnverifiedEvidence {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UnverifiedEvidence")
            .field("request_id", &"<redacted>")
            .field("account_id", &"<redacted>")
            .field("operation_hash", &"<redacted>")
            .field("nonce", &"<redacted>")
            .field("expires_at_unix", &self.expires_at_unix)
            .field("profile_epoch", &self.profile_epoch)
            .field("assertion", &"<redacted>")
            .finish()
    }
}

pub trait SecretStore {
    type Handle: Clone + std::fmt::Debug + Eq;

    fn store_from_reader(
        &mut self,
        account_id: &str,
        kind: &str,
        reader: &mut dyn Read,
    ) -> Result<Self::Handle, PlatformError>;
    fn contains(&self, handle: &Self::Handle) -> bool;
    fn verify(&mut self, handle: &Self::Handle) -> Result<(), PlatformError>;
    fn delete(&mut self, handle: &Self::Handle) -> Result<(), PlatformError>;
}

pub trait IpcTransport {
    fn send_frame(&mut self, frame: &[u8]) -> Result<Vec<u8>, PlatformError>;
}

pub trait Notifier {
    fn notify(&mut self, message_key: &str, object_ref: &str) -> Result<(), PlatformError>;
}

pub trait UserVerifier {
    fn verify(
        &mut self,
        challenge: &VerificationChallenge,
    ) -> Result<UnverifiedEvidence, PlatformError>;
}

pub trait AutoStart {
    fn set_enabled(&mut self, enabled: bool) -> Result<(), PlatformError>;
    fn state(&self) -> Result<AutoStartState, PlatformError>;
}
