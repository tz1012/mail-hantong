//! Explicit passphrase-backed storage for Linux systems without Secret Service.
//! The owning Core must select this backend explicitly during first setup.

#![cfg(target_os = "linux")]

use crate::unix_secret_store::{ProtectedBytes, UnixSecretError};
use age::secrecy::SecretString;
use age::{Decryptor, Encryptor};
use hantong_platform::unix_paths::UnixDirectoryGuard;
use ring::rand::SecureRandom;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const METADATA: &str = "backend-v1.json";
const CIPHERTEXT: &str = "headless-v1.age";
const MAX_CIPHERTEXT: usize = 16 * 1024 * 1024;
const MAX_PLAINTEXT: usize = 8 * 1024 * 1024;
const MAX_SELECTOR: usize = 384;
const MAX_SECRET: usize = 65_536;
const MAX_PASSPHRASE: usize = 4096;
// Keep hostile ciphertext decryption below the age default upper bound.  The
// 2^18 scrypt setting is still deliberately expensive while bounding memory
// use for files stored in this local vault.
const MAX_SCRYPT_WORK_FACTOR: u8 = 18;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    format_version: u8,
    backend: String,
    vault_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VaultFile {
    format_version: u8,
    vault_id: String,
    generation: u64,
    items: Vec<Item>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    selector: String,
    secret: String,
}

impl Drop for Item {
    fn drop(&mut self) {
        let mut selector = std::mem::take(&mut self.selector).into_bytes();
        let mut secret = std::mem::take(&mut self.secret).into_bytes();
        crate::wipe(&mut selector);
        crate::wipe(&mut secret);
    }
}

/// Explicit Linux headless vault. The caller must collect the passphrase in a
/// Secrets-owned protected foreground prompt before constructing this value.
pub struct HeadlessVault {
    root: PathBuf,
    _root_guard: UnixDirectoryGuard,
    passphrase: SecretString,
    metadata: Metadata,
    _lock: File,
    cache: Mutex<Option<VaultFile>>,
}

pub type SharedHeadlessVault = Arc<Mutex<HeadlessVault>>;

/// Selects and initializes Linux storage without exposing a passphrase to Core.
/// Existing vault metadata always selects the headless vault. An existing
/// database without that metadata stays on the explicit OS-store path.
pub fn prepare_backend(root: &Path, data_root: &Path) -> Result<(), crate::SecretError> {
    validate_root(root).map_err(|_| crate::SecretError::InvalidRoot)?;
    hantong_platform::unix_paths::UnixDirectoryGuard::open(data_root, true)
        .map_err(|_| crate::SecretError::InvalidRoot)?;
    let metadata = root.join(METADATA);
    let ciphertext = root.join(CIPHERTEXT);
    let database = data_root.join("mail-hantong.db");
    let metadata_exists = fs::symlink_metadata(&metadata).is_ok();
    let ciphertext_exists = fs::symlink_metadata(&ciphertext).is_ok();
    let database_exists = match fs::symlink_metadata(&database) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => true,
        Ok(_) => return Err(crate::SecretError::InvalidRoot),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => return Err(crate::SecretError::Protection),
    };
    if metadata_exists != ciphertext_exists {
        return Err(crate::SecretError::InvalidCiphertext);
    }
    if metadata_exists {
        let passphrase = passphrase_from_buffer(prompt_passphrase(false)?)?;
        let vault = HeadlessVault::unlock(root, passphrase)
            .map_err(|_| crate::SecretError::Protection)?
            .into_shared();
        return crate::unix_secret_store::UnixSecretStore::install_headless_backend(vault)
            .map_err(|_| crate::SecretError::Protection);
    }
    if database_exists {
        return crate::unix_secret_store::UnixSecretStore::install_platform_store()
            .map_err(|_| crate::SecretError::UnsupportedPlatform);
    }
    let choice = prompt_choice()?;
    if choice == b'2' {
        return crate::unix_secret_store::UnixSecretStore::install_platform_store()
            .map_err(|_| crate::SecretError::UnsupportedPlatform);
    }
    if choice != b'1' {
        return Err(crate::SecretError::InvalidInputWindow);
    }
    let first = prompt_passphrase(true)?;
    let second = prompt_passphrase(true)?;
    if first.as_slice() != second.as_slice() {
        return Err(crate::SecretError::InvalidCredential);
    }
    let passphrase = passphrase_from_buffer(first)?;
    let vault = HeadlessVault::initialize(root, passphrase)
        .map_err(|_| crate::SecretError::Protection)?
        .into_shared();
    crate::unix_secret_store::UnixSecretStore::install_headless_backend(vault)
        .map_err(|_| crate::SecretError::Protection)
}

fn prompt_choice() -> Result<u8, crate::SecretError> {
    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| crate::SecretError::InvalidInputWindow)?;
    tty.write_all(b"Secret storage: 1) headless passphrase  2) OS credential store\nChoice: ")
        .map_err(|_| crate::SecretError::InvalidInputWindow)?;
    let mut bytes = Vec::new();
    let mut byte = [0_u8; 1];
    let mut read = 0;
    while read < 256 {
        let count = tty
            .read(&mut byte)
            .map_err(|_| crate::SecretError::InvalidInputWindow)?;
        if count == 0 {
            break;
        }
        if byte[0] == b'\n' || byte[0] == b'\r' {
            break;
        }
        if bytes.len() < 8 {
            bytes.push(byte[0]);
        }
        read += 1;
    }
    Ok(bytes
        .into_iter()
        .find(|b| !b.is_ascii_whitespace())
        .unwrap_or(0))
}

fn prompt_passphrase(confirm: bool) -> Result<crate::SensitiveBuffer, crate::SecretError> {
    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| crate::SecretError::InvalidInputWindow)?;
    tty.write_all(if confirm {
        b"Enter headless vault passphrase (input hidden): "
    } else {
        b"Unlock headless vault (input hidden): "
    })
    .map_err(|_| crate::SecretError::InvalidInputWindow)?;
    let value = crate::terminal_secret::read_masked_terminal_line()?;
    if value.as_slice().is_empty() || value.as_slice().len() > MAX_PASSPHRASE {
        return Err(crate::SecretError::InvalidCredential);
    }
    Ok(value)
}

fn passphrase_from_buffer(
    mut value: crate::SensitiveBuffer,
) -> Result<age::secrecy::SecretString, crate::SecretError> {
    let bytes = std::mem::take(&mut value.0);
    drop(value);
    match String::from_utf8(bytes) {
        Ok(text) => Ok(age::secrecy::SecretString::from(text)),
        Err(error) => {
            let mut bytes = error.into_bytes();
            crate::wipe(&mut bytes);
            Err(crate::SecretError::InvalidCredential)
        }
    }
}

impl HeadlessVault {
    pub fn initialize(root: &Path, passphrase: SecretString) -> Result<Self, UnixSecretError> {
        validate_root(root)?;
        let root_guard = open_root_guard(root)?;
        let metadata_path = root.join(METADATA);
        if metadata_path.exists() || root.join(CIPHERTEXT).exists() {
            return Err(UnixSecretError::AlreadyExists);
        }
        let lock = lock_file(&root_guard)?;
        let metadata = Metadata {
            format_version: 1,
            backend: "age-passphrase-v1".to_string(),
            vault_id: random_vault_id()?,
        };
        let vault = Self {
            root: root.to_path_buf(),
            _root_guard: root_guard,
            passphrase,
            metadata,
            _lock: lock,
            cache: Mutex::new(None),
        };
        write_metadata(&vault._root_guard, METADATA, &vault.metadata)?;
        if let Err(error) = vault.write_vault(&VaultFile {
            format_version: 1,
            vault_id: vault.metadata.vault_id.clone(),
            generation: 1,
            items: Vec::new(),
        }) {
            let _ = fs::remove_file(metadata_path);
            return Err(error);
        }
        Ok(vault)
    }

    pub fn unlock(root: &Path, passphrase: SecretString) -> Result<Self, UnixSecretError> {
        validate_root(root)?;
        let root_guard = open_root_guard(root)?;
        let metadata = read_metadata(&root_guard, METADATA)?;
        let lock = lock_file(&root_guard)?;
        let vault = Self {
            root: root.to_path_buf(),
            _root_guard: root_guard,
            passphrase,
            metadata,
            _lock: lock,
            cache: Mutex::new(None),
        };
        let file = vault.read_vault()?;
        if file.vault_id != vault.metadata.vault_id || file.format_version != 1 {
            return Err(UnixSecretError::InvalidSecret);
        }
        Ok(vault)
    }

    pub fn generation(&self) -> Result<u64, UnixSecretError> {
        Ok(self.read_vault()?.generation)
    }

    pub fn read(&self, selector: &str) -> Result<ProtectedBytes, UnixSecretError> {
        validate_selector(selector)?;
        let file = self.read_vault()?;
        let selector_bytes = selector.as_bytes();
        let item = file
            .items
            .iter()
            .find(|item| base64_decode(&item.selector).ok().as_deref() == Some(selector_bytes))
            .ok_or(UnixSecretError::NotFound)?;
        ProtectedBytes::from_backend(base64_decode(&item.secret)?)
    }

    pub fn replace(
        &self,
        selector: &str,
        secret: &[u8],
        expected_generation: u64,
    ) -> Result<u64, UnixSecretError> {
        validate_selector(selector)?;
        if secret.is_empty() {
            return Err(UnixSecretError::InvalidSecret);
        }
        if secret.len() > MAX_SECRET {
            return Err(UnixSecretError::SecretTooLarge);
        }
        let mut file = self.read_vault()?;
        if file.generation != expected_generation {
            return Err(UnixSecretError::Conflict);
        }
        let encoded = base64_encode(secret);
        let encoded_selector = base64_encode(selector.as_bytes());
        if let Some(item) = file
            .items
            .iter_mut()
            .find(|item| item.selector == encoded_selector)
        {
            item.secret = encoded;
        } else {
            file.items.push(Item {
                selector: encoded_selector,
                secret: encoded,
            });
        }
        file.generation = file
            .generation
            .checked_add(1)
            .ok_or(UnixSecretError::StoreFailure)?;
        self.write_vault(&file)?;
        Ok(file.generation)
    }

    pub fn delete(&self, selector: &str, expected_generation: u64) -> Result<u64, UnixSecretError> {
        validate_selector(selector)?;
        let mut file = self.read_vault()?;
        if file.generation != expected_generation {
            return Err(UnixSecretError::Conflict);
        }
        let encoded_selector = base64_encode(selector.as_bytes());
        let before = file.items.len();
        file.items.retain(|item| item.selector != encoded_selector);
        if file.items.len() == before {
            return Err(UnixSecretError::NotFound);
        }
        file.generation = file
            .generation
            .checked_add(1)
            .ok_or(UnixSecretError::StoreFailure)?;
        self.write_vault(&file)?;
        Ok(file.generation)
    }

    pub fn into_shared(self) -> SharedHeadlessVault {
        Arc::new(Mutex::new(self))
    }

    fn read_vault(&self) -> Result<VaultFile, UnixSecretError> {
        if let Some(file) = self
            .cache
            .lock()
            .map_err(|_| UnixSecretError::Unavailable)?
            .as_ref()
            .cloned()
        {
            return Ok(file);
        }
        let metadata = fs::symlink_metadata(self.root.join(CIPHERTEXT))
            .map_err(|_| UnixSecretError::NotFound)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(UnixSecretError::InvalidSecret);
        }
        validate_leaf_metadata(&metadata)?;
        if metadata.len() == 0 || metadata.len() > MAX_CIPHERTEXT as u64 {
            return Err(UnixSecretError::InvalidSecret);
        }
        let input = self
            ._root_guard
            .open_existing_readonly_regular(CIPHERTEXT)
            .map_err(|_| UnixSecretError::NotFound)?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        input
            .take((MAX_CIPHERTEXT + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| UnixSecretError::InvalidSecret)?;
        if bytes.len() > MAX_CIPHERTEXT {
            return Err(UnixSecretError::SecretTooLarge);
        }
        let decryptor =
            Decryptor::new(bytes.as_slice()).map_err(|_| UnixSecretError::InvalidSecret)?;
        let mut identity = age::scrypt::Identity::new(self.passphrase.clone());
        identity.set_max_work_factor(MAX_SCRYPT_WORK_FACTOR);
        let reader = decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .map_err(|_| UnixSecretError::InvalidSecret)?;
        let mut plaintext = crate::SensitiveBuffer(Vec::new());
        reader
            .take((MAX_PLAINTEXT + 1) as u64)
            .read_to_end(&mut plaintext.0)
            .map_err(|_| UnixSecretError::InvalidSecret)?;
        if plaintext.0.len() > MAX_PLAINTEXT {
            return Err(UnixSecretError::SecretTooLarge);
        }
        let result = serde_json::from_slice(&plaintext.0)
            .map_err(|_| UnixSecretError::InvalidSecret)
            .and_then(validate_vault_file);
        drop(plaintext);
        let file = result?;
        *self
            .cache
            .lock()
            .map_err(|_| UnixSecretError::Unavailable)? = Some(file.clone());
        Ok(file)
    }

    fn write_vault(&self, file: &VaultFile) -> Result<(), UnixSecretError> {
        let mut plaintext = crate::SensitiveBuffer(
            serde_json::to_vec(file).map_err(|_| UnixSecretError::StoreFailure)?,
        );
        if plaintext.0.len() > MAX_PLAINTEXT {
            return Err(UnixSecretError::SecretTooLarge);
        }
        let mut recipient = age::scrypt::Recipient::new(self.passphrase.clone());
        recipient.set_work_factor(MAX_SCRYPT_WORK_FACTOR);
        let encryptor =
            Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
                .map_err(|_| UnixSecretError::StoreFailure)?;
        let mut ciphertext = Vec::new();
        let mut writer = encryptor
            .wrap_output(&mut ciphertext)
            .map_err(|_| UnixSecretError::StoreFailure)?;
        writer
            .write_all(&plaintext.0)
            .map_err(|_| UnixSecretError::StoreFailure)?;
        writer.finish().map_err(|_| UnixSecretError::StoreFailure)?;
        crate::wipe(&mut plaintext.0);
        atomic_write(&self._root_guard, CIPHERTEXT, &ciphertext)?;
        *self
            .cache
            .lock()
            .map_err(|_| UnixSecretError::Unavailable)? = Some(file.clone());
        Ok(())
    }
}

fn validate_root(root: &Path) -> Result<(), UnixSecretError> {
    let _ = open_root_guard(root)?;
    Ok(())
}

fn open_root_guard(root: &Path) -> Result<UnixDirectoryGuard, UnixSecretError> {
    let _guard = hantong_platform::unix_paths::UnixDirectoryGuard::open(root, true)
        .map_err(|_| UnixSecretError::Unavailable)?;
    let meta = fs::symlink_metadata(root).map_err(|_| UnixSecretError::Unavailable)?;
    let file_type = std::fs::Metadata::file_type(&meta);
    if !std::fs::FileType::is_dir(&file_type) || std::fs::FileType::is_symlink(&file_type) {
        return Err(UnixSecretError::Unavailable);
    }
    Ok(_guard)
}

fn read_metadata(guard: &UnixDirectoryGuard, name: &str) -> Result<Metadata, UnixSecretError> {
    let file = guard
        .open_existing_readonly_regular(name)
        .map_err(|_| UnixSecretError::NotFound)?;
    let file_meta = file.metadata().map_err(|_| UnixSecretError::NotFound)?;
    if !file_meta.is_file() || file_meta.file_type().is_symlink() || file_meta.len() > 4096 {
        return Err(UnixSecretError::InvalidSecret);
    }
    validate_leaf_metadata(&file_meta)?;
    let mut bytes = Vec::with_capacity(file_meta.len() as usize);
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| UnixSecretError::InvalidSecret)?;
    if bytes.len() > 4096 {
        return Err(UnixSecretError::InvalidSecret);
    }
    let metadata: Metadata =
        serde_json::from_slice(&bytes).map_err(|_| UnixSecretError::InvalidSecret)?;
    if metadata.format_version != 1
        || metadata.backend != "age-passphrase-v1"
        || metadata.vault_id.len() != 64
        || !metadata
            .vault_id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(UnixSecretError::InvalidSecret);
    }
    Ok(metadata)
}

fn write_metadata(
    guard: &UnixDirectoryGuard,
    name: &str,
    metadata: &Metadata,
) -> Result<(), UnixSecretError> {
    atomic_write(
        guard,
        name,
        &serde_json::to_vec(metadata).map_err(|_| UnixSecretError::StoreFailure)?,
    )
}

fn atomic_write(
    guard: &UnixDirectoryGuard,
    name: &str,
    bytes: &[u8],
) -> Result<(), UnixSecretError> {
    let tmp = format!(".{}.tmp-{}", name, std::process::id());
    let mut file = guard
        .create_new_regular(&tmp)
        .map_err(|_| UnixSecretError::StoreFailure)?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|_| UnixSecretError::StoreFailure)?;
        file.sync_all().map_err(|_| UnixSecretError::StoreFailure)?;
        drop(file);
        guard
            .rename_under_lock(&tmp, name)
            .map_err(|_| UnixSecretError::StoreFailure)?;
        sync_directory(guard)
    })();
    if result.is_err()
        && let Ok(tmp_file) = guard.open_existing_regular(&tmp)
    {
        let _ = guard.unlink_owned(&tmp, &tmp_file);
    }
    result
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn sync_directory(guard: &UnixDirectoryGuard) -> Result<(), UnixSecretError> {
    let fd = guard
        .directory_fd()
        .map_err(|_| UnixSecretError::StoreFailure)?;
    if unsafe { libc::fsync(fd.as_raw_fd()) } != 0 {
        return Err(UnixSecretError::StoreFailure);
    }
    Ok(())
}

fn lock_file(guard: &UnixDirectoryGuard) -> Result<File, UnixSecretError> {
    let file = guard
        .open_or_create_regular("headless-v1.lock")
        .map_err(|_| UnixSecretError::Unavailable)?;
    validate_leaf_metadata(&file.metadata().map_err(|_| UnixSecretError::Unavailable)?)?;
    file.try_lock().map_err(|_| UnixSecretError::Conflict)?;
    Ok(file)
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn validate_leaf_metadata(metadata: &std::fs::Metadata) -> Result<(), UnixSecretError> {
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || metadata.mode() & 0o7777 != 0o600
    {
        return Err(UnixSecretError::Unavailable);
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn validate_leaf_metadata(_metadata: &std::fs::Metadata) -> Result<(), UnixSecretError> {
    Ok(())
}

fn validate_selector(selector: &str) -> Result<(), UnixSecretError> {
    if selector.is_empty()
        || selector.len() > MAX_SELECTOR
        || !selector.is_ascii()
        || selector.bytes().any(|b| b == 0 || b == b'/' || b == b'\\')
    {
        return Err(UnixSecretError::InvalidSelector);
    }
    Ok(())
}

fn random_vault_id() -> Result<String, UnixSecretError> {
    let mut bytes = [0_u8; 32];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| UnixSecretError::StoreFailure)?;
    Ok(hex_encode(&bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn validate_vault_file(file: VaultFile) -> Result<VaultFile, UnixSecretError> {
    if file.format_version != 1 || file.vault_id.len() != 64 || file.generation == 0 {
        return Err(UnixSecretError::InvalidSecret);
    }
    let mut seen = std::collections::HashSet::new();
    for item in &file.items {
        let selector = base64_decode(&item.selector)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or(UnixSecretError::InvalidSecret)?;
        validate_selector(&selector)?;
        if !seen.insert(selector) {
            return Err(UnixSecretError::InvalidSecret);
        }
        let mut decoded = base64_decode(&item.secret)?;
        if decoded.is_empty() || decoded.len() > MAX_SECRET {
            crate::wipe(&mut decoded);
            return Err(UnixSecretError::InvalidSecret);
        }
        crate::wipe(&mut decoded);
    }
    Ok(file)
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        out.push(TABLE[(a >> 2) as usize] as char);
        out.push(TABLE[((a << 4 | b >> 4) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((b << 2 | c >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(value: &str) -> Result<Vec<u8>, UnixSecretError> {
    if value.is_empty()
        || value.len() > MAX_SECRET.div_ceil(3) * 4
        || !value.len().is_multiple_of(4)
    {
        return Err(UnixSecretError::InvalidSecret);
    }
    fn sextet(byte: u8) -> Option<u8> {
        Some(match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(value.len() / 4 * 3);
    for (index, chunk) in bytes.chunks_exact(4).enumerate() {
        let last = index + 1 == bytes.len() / 4;
        let a = sextet(chunk[0]).ok_or(UnixSecretError::InvalidSecret)? as u32;
        let b = sextet(chunk[1]).ok_or(UnixSecretError::InvalidSecret)? as u32;
        let c = if chunk[2] == b'=' {
            0
        } else {
            sextet(chunk[2]).ok_or(UnixSecretError::InvalidSecret)? as u32
        };
        let d = if chunk[3] == b'=' {
            0
        } else {
            sextet(chunk[3]).ok_or(UnixSecretError::InvalidSecret)? as u32
        };
        let invalid_two_byte_padding =
            chunk[2] == b'=' && (!last || chunk[3] != b'=' || (b & 0x0f) != 0);
        let invalid_one_byte_padding =
            chunk[3] == b'=' && chunk[2] != b'=' && (!last || (c & 0x03) != 0);
        if invalid_two_byte_padding || invalid_one_byte_padding {
            return Err(UnixSecretError::InvalidSecret);
        }
        out.push(((a << 2) | (b >> 4)) as u8);
        if chunk[2] != b'=' {
            out.push(((b << 4) | (c >> 2)) as u8);
        }
        if chunk[3] != b'=' {
            out.push(((c << 6) | d) as u8);
        }
    }
    if out.len() > MAX_SECRET {
        return Err(UnixSecretError::SecretTooLarge);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    fn fixture() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hantong-headless-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
        path
    }
    #[test]
    fn sec031_headless_round_trip_and_reopen_without_plaintext() {
        let root = fixture();
        let passphrase = SecretString::from("fixture-passphrase".to_owned());
        let vault = HeadlessVault::initialize(&root, passphrase.clone()).unwrap();
        assert_eq!(vault.generation().unwrap(), 1);
        assert_eq!(vault.replace("mail-a", b"secret-value", 1).unwrap(), 2);
        drop(vault);
        let reopened = HeadlessVault::unlock(&root, passphrase).unwrap();
        assert_eq!(
            reopened.read("mail-a").unwrap().as_bytes_in_secrets(),
            b"secret-value"
        );
        assert!(
            !fs::read(root.join(CIPHERTEXT))
                .unwrap()
                .windows(12)
                .any(|w| w == b"secret-value")
        );
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn sec031_headless_cas_and_wrong_passphrase_fail_closed() {
        let root = fixture();
        let passphrase = SecretString::from("fixture-passphrase".to_owned());
        let vault = HeadlessVault::initialize(&root, passphrase.clone()).unwrap();
        assert_eq!(
            vault.replace("mail-a", b"x", 99),
            Err(UnixSecretError::Conflict)
        );
        drop(vault);
        assert!(matches!(
            HeadlessVault::unlock(&root, SecretString::from("wrong".to_owned())),
            Err(UnixSecretError::InvalidSecret)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sec031_headless_lifetime_lock_blocks_second_unlock() {
        let root = fixture();
        let passphrase = SecretString::from("fixture-passphrase".to_owned());
        let vault = HeadlessVault::initialize(&root, passphrase.clone()).unwrap();
        assert!(matches!(
            HeadlessVault::unlock(&root, passphrase),
            Err(UnixSecretError::Conflict)
        ));
        drop(vault);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sec031_headless_lock_symlink_is_rejected() {
        let root = fixture();
        let passphrase = SecretString::from("fixture-passphrase".to_owned());
        let vault = HeadlessVault::initialize(&root, passphrase.clone()).unwrap();
        drop(vault);
        fs::remove_file(root.join("headless-v1.lock")).unwrap();
        fs::write(root.join("lock-target"), b"x").unwrap();
        std::os::unix::fs::symlink("lock-target", root.join("headless-v1.lock")).unwrap();
        assert!(matches!(
            HeadlessVault::unlock(&root, passphrase),
            Err(UnixSecretError::Unavailable) | Err(UnixSecretError::Conflict)
        ));
        let _ = fs::remove_dir_all(root);
    }
}
