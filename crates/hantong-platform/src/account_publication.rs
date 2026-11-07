//! Evidence of exact schema-2 bytes read from the protected default profile.
//! This owns the existing fixed-path loader's file/ancestor guards. It is not
//! provider consent, a grant, or proof of a human action.

use crate::LoadedClientProfile;
use hantong_contracts::{ErrorCode, account_profile::ParsedAccountClientProfile};

pub struct VerifiedAccountProfilePublication {
    _loaded: LoadedClientProfile,
    registration_id: [u8; 16],
    profile_epoch: u64,
    client_key_hash: [u8; 32],
    server_key_hash: [u8; 32],
    scope_hash: [u8; 32],
    profile_hash: [u8; 32],
}

impl std::fmt::Debug for VerifiedAccountProfilePublication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VerifiedAccountProfilePublication { <redacted> }")
    }
}

impl VerifiedAccountProfilePublication {
    /// Verifies the exact default profile through handles already pinned by a
    /// native publication. Content is read from the held object; fixed-path
    /// metadata probes independently verify its filesystem identity.
    #[cfg(windows)]
    /// # Safety
    /// This trusted capability constructor requires handles from one native
    /// publication: the leaf denies write/delete sharing and its ancestors
    /// deny namespace replacement. The original guards or their exact handle
    /// clones must remain alive through Store activation. Runtime object-ID,
    /// fixed-Default path, type and ACL checks reject non-Default publications;
    /// they do not independently establish the caller's share-mode invariant.
    #[allow(unsafe_code)] // Capability precondition enforced at the private native publisher boundary.
    pub unsafe fn from_pinned_default_handles(
        file: &std::fs::File,
        directories: &[std::fs::File],
    ) -> Result<Self, ErrorCode> {
        let loaded = crate::profile_file::load_default_from_pinned(file, directories)?;
        let profile = ParsedAccountClientProfile::from_json(loaded.bytes())?;
        crate::validate_public_key(profile.client_public_key())?;
        crate::validate_public_key(profile.server_public_key())?;
        Ok(Self {
            registration_id: *profile.registration_id(),
            profile_epoch: profile.profile_epoch(),
            client_key_hash: crate::sha256(profile.client_public_key())?,
            server_key_hash: crate::sha256(profile.server_public_key())?,
            scope_hash: crate::sha256(&profile.scope().to_json()?)?,
            profile_hash: crate::sha256(loaded.bytes())?,
            _loaded: loaded,
        })
    }

    /// Call while holding the protected publication handle. There is no path,
    /// arbitrary byte buffer, hash, or boolean accepted as publication evidence.
    pub fn load_default() -> Result<Self, ErrorCode> {
        let loaded = crate::load_default_client_profile()?;
        let profile = ParsedAccountClientProfile::from_json(loaded.bytes())?;
        crate::validate_public_key(profile.client_public_key())?;
        crate::validate_public_key(profile.server_public_key())?;
        Ok(Self {
            registration_id: *profile.registration_id(),
            profile_epoch: profile.profile_epoch(),
            client_key_hash: crate::sha256(profile.client_public_key())?,
            server_key_hash: crate::sha256(profile.server_public_key())?,
            scope_hash: crate::sha256(&profile.scope().to_json()?)?,
            profile_hash: crate::sha256(loaded.bytes())?,
            _loaded: loaded,
        })
    }

    pub fn registration_id(&self) -> &[u8; 16] {
        &self.registration_id
    }
    pub fn profile_epoch(&self) -> u64 {
        self.profile_epoch
    }
    pub fn client_key_hash(&self) -> &[u8; 32] {
        &self.client_key_hash
    }
    pub fn server_key_hash(&self) -> &[u8; 32] {
        &self.server_key_hash
    }
    pub fn scope_hash(&self) -> &[u8; 32] {
        &self.scope_hash
    }
    pub fn profile_hash(&self) -> &[u8; 32] {
        &self.profile_hash
    }
}
