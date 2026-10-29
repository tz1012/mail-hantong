#![allow(unsafe_code)]

use hantong_contracts::ErrorCode;
use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr};
use windows_sys::Win32::Security::Cryptography::*;

const MAX_TRANSCRIPT: usize = 32 * 1024;
const PUBLIC_BLOB_LEN: usize = 72;
const SIGNATURE_LEN: usize = 64;
const NTE_EXISTS_STATUS: i32 = windows_sys::Win32::Foundation::NTE_EXISTS;

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}
fn ok_hr(value: i32) -> bool {
    value == 0
}
fn ok_nt(value: i32) -> bool {
    value == 0
}
fn is_missing_key_status(value: i32) -> bool {
    value == windows_sys::Win32::Foundation::NTE_BAD_KEYSET
}

struct Provider(NCRYPT_PROV_HANDLE);
impl Drop for Provider {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe {
                NCryptFreeObject(self.0);
            }
        }
    }
}
struct Key {
    handle: NCRYPT_KEY_HANDLE,
    delete_on_drop: bool,
}
impl Drop for Key {
    fn drop(&mut self) {
        if self.handle != 0 {
            unsafe {
                if self.delete_on_drop {
                    if NCryptDeleteKey(self.handle, 0) != 0 {
                        let _ = NCryptFreeObject(self.handle);
                    }
                } else {
                    let _ = NCryptFreeObject(self.handle);
                }
            }
        }
    }
}
struct BcryptAlg(BCRYPT_ALG_HANDLE);
impl Drop for BcryptAlg {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = BCryptCloseAlgorithmProvider(self.0, 0);
            }
        }
    }
}
struct BcryptKey(BCRYPT_KEY_HANDLE);
impl Drop for BcryptKey {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = BCryptDestroyKey(self.0);
            }
        }
    }
}

struct BcryptHash(BCRYPT_HASH_HANDLE);
impl Drop for BcryptHash {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = BCryptDestroyHash(self.0);
            }
        }
    }
}

/// Current-user CNG key with private material kept inside the provider.
pub struct ClientSigningKey {
    material: SigningMaterial,
    name: String,
}
impl std::fmt::Debug for ClientSigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientSigningKey")
            .field("name", &"<redacted>")
            .finish()
    }
}

fn open_provider() -> Result<Provider, ErrorCode> {
    let mut handle = 0;
    let status = unsafe { NCryptOpenStorageProvider(&mut handle, MS_KEY_STORAGE_PROVIDER, 0) };
    if ok_hr(status) && handle != 0 {
        Ok(Provider(handle))
    } else {
        Err(ErrorCode::CapabilityUnsupported)
    }
}

pub fn random_nonce() -> Result<[u8; 32], ErrorCode> {
    let mut out = [0u8; 32];
    let status = unsafe {
        BCryptGenRandom(
            ptr::null_mut(),
            out.as_mut_ptr(),
            out.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if ok_nt(status) {
        Ok(out)
    } else {
        Err(ErrorCode::InternalError)
    }
}

/// SHA-256 using the Windows CNG primitive; input is bounded for IPC use.
pub fn sha256(input: &[u8]) -> Result<[u8; 32], ErrorCode> {
    if input.len() > MAX_TRANSCRIPT {
        return Err(ErrorCode::InvalidInput);
    }
    let mut alg = ptr::null_mut();
    let status =
        unsafe { BCryptOpenAlgorithmProvider(&mut alg, BCRYPT_SHA256_ALGORITHM, ptr::null(), 0) };
    if !ok_nt(status) || alg.is_null() {
        return Err(ErrorCode::InternalError);
    }
    let alg = BcryptAlg(alg);
    let mut object_len = 0u32;
    let mut result_len = 0u32;
    let status = unsafe {
        BCryptGetProperty(
            alg.0,
            BCRYPT_OBJECT_LENGTH,
            (&mut object_len as *mut u32).cast(),
            4,
            &mut result_len,
            0,
        )
    };
    if !ok_nt(status) || result_len != 4 || object_len == 0 || object_len > 1024 * 1024 {
        return Err(ErrorCode::InternalError);
    }
    let mut object = vec![0u8; object_len as usize];
    let mut hash = ptr::null_mut();
    let status = unsafe {
        BCryptCreateHash(
            alg.0,
            &mut hash,
            object.as_mut_ptr(),
            object_len,
            ptr::null(),
            0,
            0,
        )
    };
    if !ok_nt(status) || hash.is_null() {
        return Err(ErrorCode::InternalError);
    }
    let hash = BcryptHash(hash);
    let status = unsafe { BCryptHashData(hash.0, input.as_ptr(), input.len() as u32, 0) };
    if !ok_nt(status) {
        return Err(ErrorCode::InternalError);
    }
    let mut out = [0u8; 32];
    let status = unsafe { BCryptFinishHash(hash.0, out.as_mut_ptr(), 32, 0) };
    if ok_nt(status) {
        Ok(out)
    } else {
        Err(ErrorCode::InternalError)
    }
}

impl ClientSigningKey {
    pub fn create() -> Result<Self, ErrorCode> {
        let random = random_nonce()?;
        let name = format!(
            "mail-hantong-client-{}",
            random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        Ok(Self {
            material: create_named_key(&name)?,
            name,
        })
    }

    pub fn open(name: &str) -> Result<Self, ErrorCode> {
        let Some(suffix) = name.strip_prefix("mail-hantong-client-") else {
            return Err(ErrorCode::InvalidInput);
        };
        if suffix.len() != 64
            || !suffix
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            material: open_named_key(name)?,
            name: name.to_owned(),
        })
    }

    /// Deletes only this explicitly opened key. The key handle is consumed;
    /// a failed delete still closes the handle and never recreates a name.
    pub fn delete(mut self) -> Result<(), ErrorCode> {
        delete_material(&mut self.material)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn public_key(&self) -> Result<[u8; PUBLIC_BLOB_LEN], ErrorCode> {
        public_key_for(&self.material)
    }

    pub fn sign(&self, transcript: &[u8]) -> Result<[u8; SIGNATURE_LEN], ErrorCode> {
        sign_with(&self.material, transcript)
    }
}

pub fn verify_signature(
    public_blob: &[u8],
    transcript: &[u8],
    signature: &[u8],
) -> Result<bool, ErrorCode> {
    if public_blob.len() != PUBLIC_BLOB_LEN || signature.len() != SIGNATURE_LEN {
        return Err(ErrorCode::InvalidInput);
    }
    let digest = sha256(transcript)?;
    let verifier = import_public_key(public_blob)?;
    let status = unsafe {
        BCryptVerifySignature(
            verifier.key.0,
            ptr::null(),
            digest.as_ptr(),
            digest.len() as u32,
            signature.as_ptr(),
            signature.len() as u32,
            0,
        )
    };
    if ok_nt(status) { Ok(true) } else { Ok(false) }
}

struct PublicVerificationKey {
    key: BcryptKey,
    _algorithm: BcryptAlg,
}

/// Checks the header and imports the point with CNG validation enabled.
pub fn validate_public_key(public_blob: &[u8]) -> Result<(), ErrorCode> {
    import_public_key(public_blob).map(drop)
}

fn import_public_key(public_blob: &[u8]) -> Result<PublicVerificationKey, ErrorCode> {
    if !valid_public_header(public_blob) {
        return Err(ErrorCode::InvalidInput);
    }
    let mut alg = ptr::null_mut();
    let status = unsafe {
        BCryptOpenAlgorithmProvider(&mut alg, BCRYPT_ECDSA_P256_ALGORITHM, ptr::null(), 0)
    };
    if !ok_nt(status) || alg.is_null() {
        return Err(ErrorCode::InternalError);
    }
    let alg = BcryptAlg(alg);
    let mut key = ptr::null_mut();
    let status = unsafe {
        BCryptImportKeyPair(
            alg.0,
            ptr::null_mut(),
            BCRYPT_ECCPUBLIC_BLOB,
            &mut key,
            public_blob.as_ptr(),
            public_blob.len() as u32,
            0,
        )
    };
    if !ok_nt(status) || key.is_null() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(PublicVerificationKey {
        key: BcryptKey(key),
        _algorithm: alg,
    })
}

fn valid_public_header(blob: &[u8]) -> bool {
    blob.len() >= 8
        && u32::from_le_bytes(blob[0..4].try_into().unwrap()) == BCRYPT_ECDSA_PUBLIC_P256_MAGIC
        && u32::from_le_bytes(blob[4..8].try_into().unwrap()) == 32
        && blob.len() == PUBLIC_BLOB_LEN
}

fn key_properties_valid(handle: NCRYPT_KEY_HANDLE) -> Result<bool, ErrorCode> {
    let mut usage = 0u32;
    let mut written = 0u32;
    let status = unsafe {
        NCryptGetProperty(
            handle,
            NCRYPT_KEY_USAGE_PROPERTY,
            (&mut usage as *mut u32).cast(),
            4,
            &mut written,
            0,
        )
    };
    if !ok_hr(status) || written != 4 || usage != NCRYPT_ALLOW_SIGNING_FLAG {
        return Ok(false);
    }
    let mut policy = 0u32;
    let mut written = 0u32;
    let status = unsafe {
        NCryptGetProperty(
            handle,
            NCRYPT_EXPORT_POLICY_PROPERTY,
            (&mut policy as *mut u32).cast(),
            4,
            &mut written,
            0,
        )
    };
    if !ok_hr(status) || written != 4 || policy != 0 {
        return Ok(false);
    }
    let mut name = [0u16; 32];
    let mut written = 0u32;
    let status = unsafe {
        NCryptGetProperty(
            handle,
            NCRYPT_ALGORITHM_PROPERTY,
            name.as_mut_ptr().cast(),
            (name.len() * 2) as u32,
            &mut written,
            0,
        )
    };
    if !ok_hr(status)
        || written < 2
        || written as usize > name.len() * 2
        || !written.is_multiple_of(2)
    {
        return Ok(false);
    }
    let units = &name[..written as usize / 2];
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    Ok(String::from_utf16(&units[..end]).ok().as_deref() == Some("ECDSA_P256"))
}

struct SigningMaterial {
    key: Key,
    _provider: Provider,
}
fn delete_material(material: &mut SigningMaterial) -> Result<(), ErrorCode> {
    let status = unsafe { NCryptDeleteKey(material.key.handle, 0) };
    if !ok_hr(status) {
        unsafe {
            let _ = NCryptFreeObject(material.key.handle);
        }
    }
    material.key.handle = 0;
    if ok_hr(status) {
        Ok(())
    } else {
        Err(ErrorCode::InternalError)
    }
}
fn server_name(id: &[u8; 32]) -> String {
    format!(
        "mail-hantong-server-{}",
        id.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}
fn create_named_key(name: &str) -> Result<SigningMaterial, ErrorCode> {
    let provider = open_provider()?;
    let name_w = wide(name);
    let alg_w = wide("ECDSA_P256");
    let mut handle = 0;
    let status = unsafe {
        NCryptCreatePersistedKey(
            provider.0,
            &mut handle,
            alg_w.as_ptr(),
            name_w.as_ptr(),
            0,
            0,
        )
    };
    if !ok_hr(status) || handle == 0 {
        return Err(if status == NTE_EXISTS_STATUS {
            ErrorCode::Conflict
        } else {
            ErrorCode::InternalError
        });
    }
    let mut key = Key {
        handle,
        delete_on_drop: true,
    };
    for (property, value) in [
        (NCRYPT_KEY_USAGE_PROPERTY, NCRYPT_ALLOW_SIGNING_FLAG),
        (NCRYPT_EXPORT_POLICY_PROPERTY, 0),
    ] {
        if !ok_hr(unsafe {
            NCryptSetProperty(key.handle, property, (&value as *const u32).cast(), 4, 0)
        }) {
            return Err(ErrorCode::InternalError);
        }
    }
    if !ok_hr(unsafe { NCryptFinalizeKey(key.handle, NCRYPT_SILENT_FLAG) })
        || !key_properties_valid(key.handle)?
    {
        return Err(ErrorCode::InternalError);
    }
    key.delete_on_drop = false;
    Ok(SigningMaterial {
        key,
        _provider: provider,
    })
}
fn open_named_key(name: &str) -> Result<SigningMaterial, ErrorCode> {
    let provider = open_provider()?;
    let name_w = wide(name);
    let mut handle = 0;
    let status = unsafe {
        NCryptOpenKey(
            provider.0,
            &mut handle,
            name_w.as_ptr(),
            0,
            NCRYPT_SILENT_FLAG,
        )
    };
    if !ok_hr(status) {
        if handle != 0 {
            unsafe {
                let _ = NCryptFreeObject(handle);
            }
        }
        return Err(if is_missing_key_status(status) {
            ErrorCode::NotFound
        } else {
            ErrorCode::InternalError
        });
    }
    if handle == 0 {
        return Err(ErrorCode::InternalError);
    }
    let key = Key {
        handle,
        delete_on_drop: false,
    };
    if !key_properties_valid(key.handle)? {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    Ok(SigningMaterial {
        key,
        _provider: provider,
    })
}
fn public_key_for(material: &SigningMaterial) -> Result<[u8; PUBLIC_BLOB_LEN], ErrorCode> {
    let mut out = [0u8; PUBLIC_BLOB_LEN];
    let mut written = 0u32;
    let status = unsafe {
        NCryptExportKey(
            material.key.handle,
            0,
            BCRYPT_ECCPUBLIC_BLOB,
            ptr::null(),
            out.as_mut_ptr(),
            out.len() as u32,
            &mut written,
            0,
        )
    };
    if !ok_hr(status) || written as usize != PUBLIC_BLOB_LEN || !valid_public_header(&out) {
        return Err(ErrorCode::InternalError);
    }
    Ok(out)
}
fn sign_with(
    material: &SigningMaterial,
    transcript: &[u8],
) -> Result<[u8; SIGNATURE_LEN], ErrorCode> {
    let digest = sha256(transcript)?;
    let mut out = [0u8; SIGNATURE_LEN];
    let mut written = 0u32;
    let status = unsafe {
        NCryptSignHash(
            material.key.handle,
            ptr::null(),
            digest.as_ptr(),
            digest.len() as u32,
            out.as_mut_ptr(),
            out.len() as u32,
            &mut written,
            0,
        )
    };
    if ok_hr(status) && written as usize == SIGNATURE_LEN {
        Ok(out)
    } else {
        Err(ErrorCode::InternalError)
    }
}
pub struct ServerSigningKey {
    material: SigningMaterial,
    id: [u8; 32],
    name: String,
}
impl std::fmt::Debug for ServerSigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerSigningKey")
            .field("name", &"<redacted>")
            .finish()
    }
}
impl ServerSigningKey {
    pub fn create(id: [u8; 32]) -> Result<Self, ErrorCode> {
        let name = server_name(&id);
        Ok(Self {
            material: create_named_key(&name)?,
            id,
            name,
        })
    }
    pub fn open(id: [u8; 32]) -> Result<Self, ErrorCode> {
        let name = server_name(&id);
        Ok(Self {
            material: open_named_key(&name)?,
            id,
            name,
        })
    }
    pub fn id_name(&self) -> &str {
        &self.name
    }
    pub fn id(&self) -> [u8; 32] {
        self.id
    }
    pub fn public_key(&self) -> Result<[u8; PUBLIC_BLOB_LEN], ErrorCode> {
        public_key_for(&self.material)
    }
    pub fn sign(&self, transcript: &[u8]) -> Result<[u8; SIGNATURE_LEN], ErrorCode> {
        sign_with(&self.material, transcript)
    }
    pub fn delete(mut self) -> Result<(), ErrorCode> {
        delete_material(&mut self.material)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TestKey(Option<ClientSigningKey>);
    impl TestKey {
        fn new() -> Self {
            Self(Some(ClientSigningKey::create().unwrap()))
        }
        fn get(&self) -> &ClientSigningKey {
            self.0.as_ref().unwrap()
        }
    }
    impl Drop for TestKey {
        fn drop(&mut self) {
            if let Some(key) = self.0.take() {
                let _ = key.delete();
            }
        }
    }
    #[test]
    fn sec03_cng_p256_roundtrip_and_tamper() {
        let key = TestKey::new();
        let public = key.get().public_key().unwrap();
        let message = b"mail-hantong auth transcript";
        let sig = key.get().sign(message).unwrap();
        assert!(verify_signature(&public, message, &sig).unwrap());
        assert!(!verify_signature(&public, b"tampered", &sig).unwrap());
        assert!(verify_signature(&public, message, &[0u8; 63]).is_err());
        let mut unaligned = [0u8; PUBLIC_BLOB_LEN + 1];
        unaligned[1..].copy_from_slice(&public);
        assert!(verify_signature(&unaligned[1..], message, &sig).unwrap());
    }
    #[test]
    fn sec03_cng_reopen_uses_same_key_without_private_export() {
        let key = TestKey::new();
        let name = key.get().name().to_owned();
        let public = key.get().public_key().unwrap();
        let reopened = ClientSigningKey::open(&name).unwrap();
        let sig = reopened.sign(b"reopen").unwrap();
        assert!(verify_signature(&public, b"reopen", &sig).unwrap());
        assert!(format!("{:?}", key.get()).contains("redacted"));
        let mut private = [0u8; 256];
        let mut written = 0u32;
        let status = unsafe {
            NCryptExportKey(
                key.get().material.key.handle,
                0,
                BCRYPT_ECCPRIVATE_BLOB,
                ptr::null(),
                private.as_mut_ptr(),
                private.len() as u32,
                &mut written,
                0,
            )
        };
        // The software KSP on this Windows build reports NOT_SUPPORTED for
        // private export with policy=0; Microsoft does not promise one error.
        assert!(matches!(
            status,
            windows_sys::Win32::Foundation::NTE_PERM
                | windows_sys::Win32::Foundation::NTE_NOT_SUPPORTED
        ));
        assert!(private.iter().all(|byte| *byte == 0));
    }
    #[test]
    fn sec03_cng_rejects_malformed_public_blob_and_name() {
        assert!(ClientSigningKey::open("mail-hantong-client-not-hex").is_err());
        assert!(
            ClientSigningKey::open(
                "mail-hantong-client-é00000000000000000000000000000000000000000000000000000000"
            )
            .is_err()
        );
        assert!(verify_signature(&[0u8; 71], b"x", &[0u8; 64]).is_err());
        let mut invalid_point = [0u8; PUBLIC_BLOB_LEN];
        invalid_point[..4].copy_from_slice(&BCRYPT_ECDSA_PUBLIC_P256_MAGIC.to_le_bytes());
        invalid_point[4..8].copy_from_slice(&32u32.to_le_bytes());
        assert_eq!(
            validate_public_key(&invalid_point),
            Err(ErrorCode::InvalidInput)
        );
    }
    #[test]
    fn sec03_cng_wrong_key_does_not_verify() {
        let first = TestKey::new();
        let second = TestKey::new();
        let signature = first.get().sign(b"wrong-key").unwrap();
        let public = second.get().public_key().unwrap();
        assert!(!verify_signature(&public, b"wrong-key", &signature).unwrap());
    }

    #[test]
    fn sec03_ncrypt_open_key_classifies_only_missing_key_status() {
        assert!(is_missing_key_status(
            windows_sys::Win32::Foundation::NTE_BAD_KEYSET
        ));
        assert!(!is_missing_key_status(
            windows_sys::Win32::Foundation::NTE_INVALID_HANDLE
        ));
        assert!(!is_missing_key_status(
            windows_sys::Win32::Foundation::NTE_KEYSET_NOT_DEF
        ));
        assert!(!is_missing_key_status(
            windows_sys::Win32::Foundation::NTE_NO_MEMORY
        ));
    }

    #[test]
    fn sec03_cng_deleted_key_is_not_silently_recreated() {
        let mut owned = TestKey::new();
        let name = owned.get().name().to_owned();
        owned.0.take().unwrap().delete().unwrap();
        assert!(matches!(
            ClientSigningKey::open(&name),
            Err(ErrorCode::NotFound)
        ));
        assert_eq!(
            sha256(&vec![0; MAX_TRANSCRIPT + 1]),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(
            sha256(b"abc").unwrap(),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad
            ]
        );
    }

    struct ServerTestKey {
        id: [u8; 32],
        key: Option<ServerSigningKey>,
    }
    impl ServerTestKey {
        fn new() -> Self {
            let id = random_nonce().unwrap();
            Self {
                id,
                key: Some(ServerSigningKey::create(id).unwrap()),
            }
        }
        fn get(&self) -> &ServerSigningKey {
            self.key.as_ref().unwrap()
        }
    }
    impl Drop for ServerTestKey {
        fn drop(&mut self) {
            if let Some(key) = self.key.take() {
                let _ = key.delete();
            }
        }
    }

    #[test]
    fn sec03_server_key_namespace_is_separate_and_persisted() {
        let mut first = ServerTestKey::new();
        let id = first.id;
        let public = first.get().public_key().unwrap();
        let name = first.get().id_name().to_owned();
        assert!(name.starts_with("mail-hantong-server-"));
        assert!(ClientSigningKey::open(&name).is_err());
        let reopened = ServerSigningKey::open(id).unwrap();
        let sig = reopened.sign(b"server identity").unwrap();
        assert!(verify_signature(&public, b"server identity", &sig).unwrap());
        drop(reopened);
        assert!(matches!(
            ServerSigningKey::create(id),
            Err(ErrorCode::Conflict)
        ));
        let still_original = ServerSigningKey::open(id).unwrap();
        assert_eq!(still_original.public_key().unwrap(), public);
        drop(still_original);
        first.key.take().unwrap().delete().unwrap();
        assert!(matches!(
            ServerSigningKey::open(id),
            Err(ErrorCode::NotFound)
        ));
    }

    #[test]
    fn sec03_server_key_missing_open_does_not_regenerate() {
        let mut owned = ServerTestKey::new();
        let id = owned.id;
        owned.key.as_ref().unwrap().public_key().unwrap();
        let key = owned.key.take().unwrap();
        key.delete().unwrap();
        assert!(matches!(
            ServerSigningKey::open(id),
            Err(ErrorCode::NotFound)
        ));
        assert!(matches!(
            ServerSigningKey::open(id),
            Err(ErrorCode::NotFound)
        ));
    }

    #[test]
    fn sec03_server_key_cannot_open_client_namespace() {
        let client = TestKey::new();
        let suffix = client
            .get()
            .name()
            .strip_prefix("mail-hantong-client-")
            .unwrap();
        let mut id = [0u8; 32];
        for (index, pair) in suffix.as_bytes().chunks_exact(2).enumerate() {
            id[index] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
        }
        assert!(matches!(
            ServerSigningKey::open(id),
            Err(ErrorCode::NotFound)
        ));
    }
}
