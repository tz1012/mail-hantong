#![deny(unsafe_code)]

#[cfg(not(windows))]
use hantong_contracts::ErrorCode;

#[allow(unsafe_code)]
mod paths;
mod profile_file;
#[cfg(windows)]
mod windows_autostart;
#[cfg(windows)]
mod windows_identity;
#[cfg(windows)]
mod windows_pipe;

pub use paths::{AppDirectories, app_directories};
pub use profile_file::{LoadedClientProfile, load_default_client_profile};

#[cfg(windows)]
pub use windows_autostart::WindowsAutoStart;
#[cfg(windows)]
pub use windows_identity::{
    ClientSigningKey, ServerSigningKey, client_key_id_for_registration, random_nonce,
    registration_id_for_request, sha256, validate_public_key, verify_signature,
};
#[cfg(windows)]
pub use windows_pipe::{LocalPipeConnection, LocalPipeServer, connect, current_peer_context};

#[cfg(not(windows))]
pub struct LocalPipeServer;

#[cfg(not(windows))]
impl LocalPipeServer {
    pub fn bind() -> Result<Self, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
}

#[cfg(not(windows))]
pub struct LocalPipeConnection;

#[cfg(test)]
mod tests {
    #[cfg(not(windows))]
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn platform_is_explicitly_unsupported_outside_windows() {
        assert!(matches!(
            LocalPipeServer::bind(),
            Err(ErrorCode::CapabilityUnsupported)
        ));
    }
}
