//! Private Windows boundary. Root handles prevent ancestor replacement while a
//! store is alive; plaintext never leaves the Secrets crate.
use super::*;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Prefix};
use std::sync::atomic::{AtomicU64, Ordering};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

fn acceptable(metadata: &fs::Metadata, directory: bool) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
        && if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        }
}

pub(super) fn lock_root(root: &Path) -> Result<(PathBuf, Vec<File>), SecretError> {
    let mut components = root.components();
    match components.next() {
        Some(Component::Prefix(p))
            if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) => {}
        _ => return Err(SecretError::InvalidRoot),
    }
    if !matches!(components.next(), Some(Component::RootDir))
        || components.any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(SecretError::InvalidRoot);
    }
    let mut current = PathBuf::new();
    let mut handles = Vec::new();
    for part in root.components() {
        current.push(part);
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        if matches!(part, Component::Normal(_)) {
            match fs::create_dir(&current) {
                Ok(()) => (),
                Err(e) if e.kind() == ErrorKind::AlreadyExists => (),
                Err(_) => return Err(SecretError::InvalidRoot),
            }
        }
        // OPEN_REPARSE_POINT inspects the directory itself. Omitting SHARE_DELETE
        // pins each ancestor before opening/creating its children.
        let handle = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&current)
            .map_err(|_| SecretError::InvalidRoot)?;
        if !acceptable(
            &handle.metadata().map_err(|_| SecretError::InvalidRoot)?,
            true,
        ) {
            return Err(SecretError::InvalidRoot);
        }
        handles.push(handle);
    }
    let canonical = fs::canonicalize(root).map_err(|_| SecretError::InvalidRoot)?;
    Ok((canonical, handles))
}

pub(super) fn reject_reparse(path: &Path) -> Result<(), SecretError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| SecretError::InvalidRoot)?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        Err(SecretError::InvalidRoot)
    } else {
        Ok(())
    }
}

pub(super) fn open_checked(path: &Path) -> Result<File, SecretError> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                SecretError::NotFound
            } else {
                SecretError::Io
            }
        })?;
    if !acceptable(&file.metadata().map_err(|_| SecretError::Io)?, false) {
        return Err(SecretError::InvalidRoot);
    }
    Ok(file)
}

struct OsBlob {
    blob: CRYPT_INTEGER_BLOB,
    plaintext: bool,
}
impl OsBlob {
    fn empty(plaintext: bool) -> Self {
        Self {
            blob: CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: std::ptr::null_mut(),
            },
            plaintext,
        }
    }
    fn checked(&self, maximum: usize) -> Result<&[u8], SecretError> {
        let length = self.blob.cbData as usize;
        if self.blob.pbData.is_null() || length == 0 || length > maximum {
            return Err(SecretError::InvalidCiphertext);
        }
        // SAFETY: after successful DPAPI, cbData describes the allocation owned
        // by pbData. The lifetime is bounded by this RAII owner and size checked.
        Ok(unsafe { std::slice::from_raw_parts(self.blob.pbData, length) })
    }
}
impl Drop for OsBlob {
    fn drop(&mut self) {
        if self.blob.pbData.is_null() {
            return;
        }
        // SAFETY: DPAPI owns/allocates this buffer, including its reported length;
        // LocalFree is its documented allocator pair. Nothing escapes this owner.
        unsafe {
            if self.plaintext {
                wipe(std::slice::from_raw_parts_mut(
                    self.blob.pbData,
                    self.blob.cbData as usize,
                ));
            }
            LocalFree(self.blob.pbData.cast());
        }
    }
}

fn entropy(account: &str, kind: SecretKind) -> Vec<u8> {
    format!("mail-hantong\0{account}\0{}", kind.name()).into_bytes()
}
fn input_blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
    // All call sites enforce <= MAX_CIPHERTEXT before this conversion.
    CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    }
}

pub(super) fn store(
    path: &Path,
    account: &str,
    kind: SecretKind,
    bytes: &[u8],
) -> Result<(), SecretError> {
    store_scoped(path, &entropy(account, kind), bytes)
}

pub(super) fn store_scoped(path: &Path, extra: &[u8], bytes: &[u8]) -> Result<(), SecretError> {
    if fs::symlink_metadata(path).is_ok() {
        drop(open_checked(path)?);
    }
    let mut output = OsBlob::empty(false);
    // SAFETY: input/entropy live through the call; DPAPI writes only the output
    // descriptor and allocates its buffer. CurrentUser is used (no machine flag).
    let ok = unsafe {
        CryptProtectData(
            &input_blob(bytes),
            std::ptr::null(),
            &input_blob(extra),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output.blob,
        )
    };
    if ok == 0 {
        return Err(SecretError::Protection);
    }
    atomic_write(path, output.checked(MAX_CIPHERTEXT)?)
}

fn unprotect(path: &Path, account: &str, kind: SecretKind) -> Result<OsBlob, SecretError> {
    unprotect_scoped(path, &entropy(account, kind))
}

fn unprotect_scoped(path: &Path, extra: &[u8]) -> Result<OsBlob, SecretError> {
    let file = open_checked(path)?;
    if file.metadata().map_err(|_| SecretError::Io)?.len() > MAX_CIPHERTEXT as u64 {
        return Err(SecretError::InvalidCiphertext);
    }
    let mut ciphertext = Vec::new();
    file.take((MAX_CIPHERTEXT + 1) as u64)
        .read_to_end(&mut ciphertext)
        .map_err(|_| SecretError::Io)?;
    if ciphertext.is_empty() || ciphertext.len() > MAX_CIPHERTEXT {
        return Err(SecretError::InvalidCiphertext);
    }
    let mut output = OsBlob::empty(true);
    // SAFETY: bounded input/entropy descriptors remain live through DPAPI. The
    // output owner wipes the OS buffer even when later validation returns error.
    let ok = unsafe {
        CryptUnprotectData(
            &input_blob(&ciphertext),
            std::ptr::null_mut(),
            &input_blob(extra),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output.blob,
        )
    };
    if ok == 0 {
        return Err(SecretError::InvalidCiphertext);
    }
    output.checked(MAX_SECRET)?;
    Ok(output)
}

// The callback stays crate-private: no caller outside Secrets sees plaintext.
pub(super) fn with_scoped_secret<T>(
    path: &Path,
    extra: &[u8],
    action: impl FnOnce(&[u8]) -> Result<T, SecretError>,
) -> Result<T, SecretError> {
    let value = unprotect_scoped(path, extra)?;
    action(value.checked(MAX_SECRET)?)
}

pub(super) fn random_bytes(bytes: &mut [u8]) -> Result<(), SecretError> {
    use windows_sys::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
    };
    let length = u32::try_from(bytes.len()).map_err(|_| SecretError::Protection)?;
    // SAFETY: the mutable slice owns exactly length writable bytes; the null
    // provider selects the documented system-preferred RNG flag.
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            length,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 {
        Err(SecretError::Protection)
    } else {
        Ok(())
    }
}

pub(super) fn pin_database_file(path: &Path, create: bool) -> Result<File, SecretError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(create)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| SecretError::Io)?;
    if !acceptable(&file.metadata().map_err(|_| SecretError::Io)?, false) {
        return Err(SecretError::InvalidRoot);
    }
    Ok(file)
}

pub(super) fn lock_database(path: &Path) -> Result<File, std::io::Error> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    if !acceptable(&file.metadata()?, false) {
        return Err(std::io::Error::other("invalid lock file"));
    }
    Ok(file)
}

pub(super) fn file_identity(file: &File) -> Result<(u64, [u8; 16]), SecretError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };
    let mut info = FILE_ID_INFO::default();
    // SAFETY: the live File owns a non-pipe handle; info has the matching
    // FileIdInfo layout and is writable for exactly the supplied size.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if ok == 0 {
        return Err(SecretError::Io);
    }
    Ok((info.VolumeSerialNumber, info.FileId.Identifier))
}

pub(super) fn verify(path: &Path, account: &str, kind: SecretKind) -> Result<(), SecretError> {
    unprotect(path, account, kind).map(drop)
}

// Only the Secrets module can access this bounded plaintext owner. No callback,
// external stream, or public SecretStore method returns it.
pub(super) fn read_mail_credential(
    path: &Path,
    account: &str,
    slot: MailCredentialSlot,
) -> Result<SensitiveBuffer, SecretError> {
    let value = unprotect(path, account, SecretKind::Mail(slot))?;
    Ok(SensitiveBuffer(value.checked(MAX_SECRET)?.to_vec()))
}

#[cfg(test)]
pub(super) fn read_for_test(
    path: &Path,
    account: &str,
    kind: SecretKind,
) -> Result<SensitiveBuffer, SecretError> {
    let value = unprotect(path, account, kind)?;
    Ok(SensitiveBuffer(value.checked(MAX_SECRET)?.to_vec()))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), SecretError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for _ in 0..32 {
        let temporary = path.with_extension(format!(
            "tmp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(SecretError::Io),
        };
        let result = file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        let result = result.and_then(|()| fs::rename(&temporary, path));
        // Windows std::fs::rename uses replace-existing semantics. Never move the
        // old file out of place first: a crash must leave old OR new ciphertext.
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return result.map_err(|_| SecretError::Io);
    }
    Err(SecretError::Io)
}
