//! Unix server identity coordination.
//!
//! The Store row is the authority for the staged selector and public key. The
//! protected signing material remains inside `hantong-secrets`; this module
//! only coordinates its opaque signer with the Store metadata.

use hantong_contracts::ErrorCode;
use hantong_platform::{random_nonce, validate_public_key};
use hantong_secrets::{unix_secret_store::UnixSecretError, unix_signing::OpaqueSigner};
use hantong_store::{ServerIdentity, Store};
use std::path::Path;

fn map_secret_error(error: UnixSecretError) -> ErrorCode {
    match error {
        UnixSecretError::NotFound => ErrorCode::NotFound,
        UnixSecretError::InvalidSelector | UnixSecretError::InvalidSecret => {
            ErrorCode::InternalError
        }
        UnixSecretError::SecretTooLarge
        | UnixSecretError::Unavailable
        | UnixSecretError::Ambiguous
        | UnixSecretError::StoreFailure
        | UnixSecretError::AlreadyExists => ErrorCode::InternalError,
    }
}

/// Initializes or recovers the Unix server identity without exposing private
/// key bytes. An active row is never regenerated or repinned.
pub fn initialize_server_identity(
    store: &Store,
    secrets_root: impl AsRef<Path>,
) -> Result<OpaqueSigner, ErrorCode> {
    let secrets_root = secrets_root.as_ref();
    match store.load_server_identity()? {
        None => {
            let key_id = random_nonce()?;
            store.stage_server_identity(key_id)?;
            let key =
                hantong_secrets::unix_signing::create_server_signing_key(secrets_root, key_id)
                    .map_err(map_secret_error)?;
            let public = key.public_key().map_err(map_secret_error)?;
            validate_public_key(&public)?;
            store.activate_server_identity(key_id, public, 1)?;
            Ok(key)
        }
        Some(ServerIdentity::Creating {
            key_id, version, ..
        }) => {
            let key = match hantong_secrets::unix_signing::load_staged_server_signing_key(
                secrets_root,
                key_id,
            ) {
                Ok(key) => key,
                Err(UnixSecretError::NotFound) => {
                    hantong_secrets::unix_signing::create_server_signing_key(secrets_root, key_id)
                        .map_err(map_secret_error)?
                }
                Err(error) => return Err(map_secret_error(error)),
            };
            let public = key.public_key().map_err(map_secret_error)?;
            validate_public_key(&public)?;
            store.activate_server_identity(key_id, public, version)?;
            Ok(key)
        }
        Some(ServerIdentity::Active {
            key_id, public_key, ..
        }) => {
            let key = hantong_secrets::unix_signing::load_server_signing_key(
                secrets_root,
                key_id,
                public_key,
            )
            .map_err(|_| ErrorCode::InternalError)?;
            let current = key.public_key().map_err(|_| ErrorCode::InternalError)?;
            if current != public_key {
                return Err(ErrorCode::InternalError);
            }
            Ok(key)
        }
    }
}
