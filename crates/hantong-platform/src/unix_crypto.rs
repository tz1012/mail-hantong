//! Unix implementations of the existing public crypto wire formats.
//! Persistent signing keys remain inside Secrets; this module exposes no signer.
use hantong_contracts::ErrorCode;
use ring::{digest, rand::SecureRandom};
use std::io::Read;
use std::path::Path;

pub const MAX_RELEASE_FILE: u64 = 512 * 1024 * 1024;

pub fn random_nonce() -> Result<[u8; 32], ErrorCode> {
    let mut bytes = [0; 32];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| ErrorCode::InternalError)?;
    Ok(bytes)
}
pub fn sha256(input: &[u8]) -> Result<[u8; 32], ErrorCode> {
    bounded_hash(input, 32 * 1024)
}
pub fn sha256_storage_payload(input: &[u8]) -> Result<[u8; 32], ErrorCode> {
    bounded_hash(input, 8 * 1024 * 1024 + 4096)
}
fn bounded_hash(input: &[u8], limit: usize) -> Result<[u8; 32], ErrorCode> {
    if input.len() > limit {
        return Err(ErrorCode::InvalidInput);
    }
    digest::digest(&digest::SHA256, input)
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::InternalError)
}
pub fn sha256_release_file(path: impl AsRef<Path>) -> Result<[u8; 32], ErrorCode> {
    let mut file = std::fs::File::open(path).map_err(|_| ErrorCode::CoreUnavailable)?;
    let expected = file
        .metadata()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .len();
    if expected == 0 || expected > MAX_RELEASE_FILE {
        return Err(ErrorCode::CoreUnavailable);
    }
    let mut hash = digest::Context::new(&digest::SHA256);
    let mut buffer = [0; 64 * 1024];
    let mut count = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        if read == 0 {
            break;
        }
        count = count
            .checked_add(read as u64)
            .ok_or(ErrorCode::CoreUnavailable)?;
        if count > expected {
            return Err(ErrorCode::CoreUnavailable);
        }
        hash.update(&buffer[..read]);
    }
    if count != expected
        || file
            .metadata()
            .map_err(|_| ErrorCode::CoreUnavailable)?
            .len()
            != expected
    {
        return Err(ErrorCode::CoreUnavailable);
    }
    hash.finish()
        .as_ref()
        .try_into()
        .map_err(|_| ErrorCode::InternalError)
}
pub fn registration_id_for_request(request: [u8; 32]) -> Result<[u8; 16], ErrorCode> {
    if request == [0; 32] {
        return Err(ErrorCode::InvalidInput);
    }
    let mut bytes = b"mail-hantong/registration-id/v1\0".to_vec();
    bytes.extend_from_slice(&request);
    let hash = sha256(&bytes)?;
    let mut id = [0; 16];
    id.copy_from_slice(&hash[..16]);
    if id == [0; 16] {
        return Err(ErrorCode::InternalError);
    }
    Ok(id)
}
pub fn client_key_id_for_registration(id: [u8; 16]) -> Result<[u8; 32], ErrorCode> {
    if id == [0; 16] {
        return Err(ErrorCode::InvalidInput);
    }
    let mut bytes = b"mail-hantong/client-registration-key/v1\0".to_vec();
    bytes.extend_from_slice(&id);
    sha256(&bytes)
}
fn point(blob: &[u8]) -> Result<[u8; 65], ErrorCode> {
    if blob.len() != 72 || &blob[..4] != b"ECS1" || blob[4..8] != 32_u32.to_le_bytes() {
        return Err(ErrorCode::InvalidInput);
    }
    let mut point = [0; 65];
    point[0] = 4;
    point[1..].copy_from_slice(&blob[8..]);
    Ok(point)
}
pub fn validate_public_key(blob: &[u8]) -> Result<(), ErrorCode> {
    // ring validates the curve point during agreement. This temporary opaque
    // key is only a validation primitive, never an authentication identity.
    let point = point(blob)?;
    let probe = ring::agreement::EphemeralPrivateKey::generate(
        &ring::agreement::ECDH_P256,
        &ring::rand::SystemRandom::new(),
    )
    .map_err(|_| ErrorCode::InternalError)?;
    ring::agreement::agree_ephemeral(
        probe,
        &ring::agreement::UnparsedPublicKey::new(&ring::agreement::ECDH_P256, point),
        |_| (),
    )
    .map_err(|_| ErrorCode::InvalidInput)
}
pub fn verify_signature(
    blob: &[u8],
    transcript: &[u8],
    signature: &[u8],
) -> Result<bool, ErrorCode> {
    if transcript.len() > 32 * 1024 || signature.len() != 64 {
        return Err(ErrorCode::InvalidInput);
    }
    validate_public_key(blob)?;
    Ok(ring::signature::UnparsedPublicKey::new(
        &ring::signature::ECDSA_P256_SHA256_FIXED,
        point(blob)?,
    )
    .verify(transcript, signature)
    .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sec03_unix_hash_matches_sha256_and_existing_bounds() {
        assert_eq!(
            hantong_contracts::wire_hex::encode(&sha256(b"abc").unwrap()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(sha256(&vec![0; 32769]).is_err());
        assert!(sha256_storage_payload(&vec![0; 8 * 1024 * 1024 + 4097]).is_err());
        assert!(registration_id_for_request([0; 32]).is_err());
        assert!(client_key_id_for_registration([0; 16]).is_err());
    }
    #[test]
    fn sec03_unix_public_blob_rejects_invalid_point_header_and_signature() {
        let mut blob = [0; 72];
        blob[..4].copy_from_slice(b"ECS1");
        blob[4..8].copy_from_slice(&32_u32.to_le_bytes());
        assert!(validate_public_key(&blob).is_err());
        assert!(validate_public_key(&blob[..71]).is_err());
        assert!(verify_signature(&blob, b"test", &[0; 63]).is_err());
    }
}
