#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

//! OS-backed secret storage. Windows FFI is isolated below for human review.

use std::fmt;
use std::fs;
#[cfg(windows)]
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

mod database;
mod imap_probe;
mod mail_credentials;
#[cfg(windows)]
mod mail_transport;
mod protected_input;
mod smtp_probe;
pub use database::{DatabaseError, KeyedConnection};
pub use imap_probe::{ImapCapabilities, ImapProbeReceipt, ImapUidPageError};
pub use protected_input::{ImapPasswordCapturePlan, PasswordInput, PendingImapCredential};
pub use smtp_probe::{MailProbeError, SmtpProbeReceipt};

const MAX_SECRET: usize = 65_536;
#[cfg(windows)]
const MAX_CIPHERTEXT: usize = 128 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretKind {
    OAuthAccess,
    OAuthRefresh,
    AppPassword,
    ModelApiKey,
    Mail(MailCredentialSlot),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailCredentialSlot {
    ImapAppPassword,
    ImapOAuthAccess,
    ImapOAuthRefresh,
    SmtpAppPassword,
    SmtpOAuthAccess,
    SmtpOAuthRefresh,
}
impl MailCredentialSlot {
    pub const fn name(self) -> &'static str {
        match self {
            Self::ImapAppPassword => "imap-app-password",
            Self::ImapOAuthAccess => "imap-oauth-access",
            Self::ImapOAuthRefresh => "imap-oauth-refresh",
            Self::SmtpAppPassword => "smtp-app-password",
            Self::SmtpOAuthAccess => "smtp-oauth-access",
            Self::SmtpOAuthRefresh => "smtp-oauth-refresh",
        }
    }
}
impl SecretKind {
    fn name(self) -> &'static str {
        match self {
            Self::OAuthAccess => "oauth-access",
            Self::OAuthRefresh => "oauth-refresh",
            Self::AppPassword => "app-password",
            Self::ModelApiKey => "model-api-key",
            Self::Mail(slot) => slot.name(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretError {
    InvalidAccountId,
    EmptySecret,
    SecretTooLarge,
    UnsupportedPlatform,
    InvalidRoot,
    Io,
    Protection,
    NotFound,
    InvalidCiphertext,
    InvalidCredential,
    CredentialMismatch,
    InvalidInputWindow,
    InputExpired,
}
impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidAccountId => "invalid account id",
            Self::EmptySecret => "empty secret",
            Self::SecretTooLarge => "secret too large",
            Self::UnsupportedPlatform => "secret store unsupported on this platform",
            Self::InvalidRoot => "invalid secret store root",
            Self::Io => "secret store I/O failed",
            Self::Protection => "secret protection failed",
            Self::NotFound => "secret not found",
            Self::InvalidCiphertext => "invalid protected secret",
            Self::InvalidCredential => "invalid mail credential",
            Self::CredentialMismatch => "mail credential requires matching connection settings",
            Self::InvalidInputWindow => "invalid protected input window",
            Self::InputExpired => "protected input expired",
        })
    }
}
impl std::error::Error for SecretError {}

#[derive(Clone, Eq, PartialEq)]
pub struct SecretHandle {
    path: PathBuf,
    account_id: String,
    kind: SecretKind,
}
impl fmt::Debug for SecretHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretHandle")
            .field("secret", &"<redacted>")
            .finish()
    }
}

pub struct SecretStore {
    root: PathBuf,
    #[cfg(windows)]
    _root_handles: Vec<File>,
}
impl SecretStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, SecretError> {
        #[cfg(not(windows))]
        {
            let _ = root;
            Err(SecretError::UnsupportedPlatform)
        }
        #[cfg(windows)]
        {
            let (root, handles) = windows_impl::lock_root(root.as_ref())?;
            Ok(Self {
                root,
                _root_handles: handles,
            })
        }
    }
    pub fn store_from_reader<R: Read>(
        &self,
        account_id: &str,
        kind: SecretKind,
        reader: R,
    ) -> Result<SecretHandle, SecretError> {
        validate_account(account_id)?;
        let secret = SensitiveBuffer::read(reader)?;
        if secret.0.is_empty() {
            return Err(SecretError::EmptySecret);
        };
        if secret.0.len() > MAX_SECRET {
            return Err(SecretError::SecretTooLarge);
        };
        validate_path(&self.root)?;
        let h = self.handle(account_id, kind);
        self.store_bytes(&h, account_id, kind, secret.as_slice())?;
        Ok(h)
    }
    pub fn contains(&self, h: &SecretHandle) -> bool {
        #[cfg(windows)]
        {
            self.owns(h) && windows_impl::open_checked(&h.path).is_ok()
        }
        #[cfg(not(windows))]
        {
            let _ = h;
            false
        }
    }
    /// Resolves an existing mail slot without decrypting it.
    pub fn resolve_mail_handle(
        &self,
        account_id: &str,
        slot: MailCredentialSlot,
    ) -> Result<SecretHandle, SecretError> {
        validate_account(account_id)?;
        validate_path(&self.root)?;
        let handle = self.handle(account_id, SecretKind::Mail(slot));
        #[cfg(windows)]
        {
            drop(windows_impl::open_checked(&handle.path)?);
            Ok(handle)
        }
        #[cfg(not(windows))]
        {
            let _ = handle;
            Err(SecretError::UnsupportedPlatform)
        }
    }
    pub fn verify(&self, h: &SecretHandle) -> Result<(), SecretError> {
        if !self.owns(h) {
            return Err(SecretError::NotFound);
        }
        validate_path(&self.root)?;
        self.verify_inner(h)
    }
    pub fn delete(&self, h: &SecretHandle) -> Result<(), SecretError> {
        if !self.owns(h) {
            return Err(SecretError::NotFound);
        }
        validate_path(&self.root)?;
        #[cfg(windows)]
        drop(windows_impl::open_checked(&h.path)?);
        fs::remove_file(&h.path).map_err(|_| SecretError::NotFound)
    }
    fn handle(&self, a: &str, k: SecretKind) -> SecretHandle {
        SecretHandle {
            path: self.path_for(a, k),
            account_id: a.into(),
            kind: k,
        }
    }
    fn path_for(&self, account_id: &str, kind: SecretKind) -> PathBuf {
        let file = match kind {
            SecretKind::Mail(slot) => format!(
                "v2-{}--{}.secret",
                encode_base32hex(account_id.as_bytes()),
                slot.name()
            ),
            _ => format!("{account_id}--{}.secret", kind.name()),
        };
        self.root.join(file)
    }
    fn owns(&self, h: &SecretHandle) -> bool {
        validate_account(&h.account_id).is_ok()
            && h.path == self.path_for(&h.account_id, h.kind)
            && h.path.parent() == Some(self.root.as_path())
            && h.path.file_name().is_some()
    }
    fn store_bytes(
        &self,
        h: &SecretHandle,
        a: &str,
        k: SecretKind,
        b: &[u8],
    ) -> Result<(), SecretError> {
        #[cfg(windows)]
        {
            windows_impl::store(&h.path, a, k, b)
        }
        #[cfg(not(windows))]
        {
            let _ = (h, a, k, b);
            Err(SecretError::UnsupportedPlatform)
        }
    }
    fn verify_inner(&self, h: &SecretHandle) -> Result<(), SecretError> {
        #[cfg(windows)]
        {
            windows_impl::verify(&h.path, &h.account_id, h.kind)
        }
        #[cfg(not(windows))]
        {
            let _ = h;
            Err(SecretError::UnsupportedPlatform)
        }
    }
}

fn validate_account(v: &str) -> Result<(), SecretError> {
    if v.is_empty()
        || v.len() > 128
        || !v
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Err(SecretError::InvalidAccountId)
    } else {
        Ok(())
    }
}

fn encode_base32hex(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";
    let mut output = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let mut buffer = 0u16;
    let mut bits = 0u8;
    for &byte in bytes {
        buffer = (buffer << 8) | u16::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            output.push(ALPHABET[((buffer >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits != 0 {
        output.push(ALPHABET[((buffer << (5 - bits)) & 0x1f) as usize] as char);
    }
    output
}
fn validate_path(p: &Path) -> Result<(), SecretError> {
    let mut c = PathBuf::new();
    for x in p.components() {
        c.push(x);
        if c.exists()
            && fs::symlink_metadata(&c)
                .map_err(|_| SecretError::InvalidRoot)?
                .file_type()
                .is_symlink()
        {
            return Err(SecretError::InvalidRoot);
        }
    }
    #[cfg(windows)]
    {
        windows_impl::reject_reparse(p)?
    }
    Ok(())
}

struct SensitiveBuffer(Vec<u8>);
impl SensitiveBuffer {
    fn read<R: Read>(r: R) -> Result<Self, SecretError> {
        Self::read_limited(r, MAX_SECRET)
    }
    fn read_limited<R: Read>(mut r: R, limit: usize) -> Result<Self, SecretError> {
        if limit > MAX_SECRET {
            return Err(SecretError::SecretTooLarge);
        }
        // A fixed allocation prevents reallocation from leaving an unwiped old
        // plaintext allocation behind. On error Drop wipes the entire buffer.
        let mut b = Self(vec![0; limit + 1]);
        let mut used = 0;
        while used < b.0.len() {
            match r.read(&mut b.0[used..]) {
                Ok(0) => break,
                Ok(count) if count <= b.0.len() - used => used += count,
                Ok(_) => return Err(SecretError::Io),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(SecretError::Io),
            }
        }
        b.0.truncate(used);
        Ok(b)
    }
    fn as_slice(&self) -> &[u8] {
        &self.0
    }
}
impl Drop for SensitiveBuffer {
    fn drop(&mut self) {
        wipe(&mut self.0)
    }
}
#[allow(unsafe_code)]
fn wipe(b: &mut [u8]) {
    for x in b {
        unsafe { std::ptr::write_volatile(x, 0) }
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows_impl;

#[cfg(test)]
mod tests {
    use super::*;
    struct TempGuard {
        root: PathBuf,
    }
    impl TempGuard {
        fn new(label: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "mail-hantong-secret-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            assert!(
                !root.exists(),
                "test directory unexpectedly exists: {root:?}"
            );
            fs::create_dir(&root).unwrap();
            Self { root }
        }
        #[cfg(windows)]
        fn store(&self) -> SecretStore {
            SecretStore::open(&self.root).unwrap()
        }
    }
    impl Drop for TempGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[cfg(windows)]
    struct FailingReader {
        bytes: Vec<u8>,
        failed: bool,
    }
    #[cfg(windows)]
    impl Read for FailingReader {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.failed {
                return Err(std::io::Error::other("fixture reader failure"));
            }
            self.failed = true;
            let count = self.bytes.len().min(out.len());
            out[..count].copy_from_slice(&self.bytes[..count]);
            Ok(count)
        }
    }

    #[cfg(windows)]
    #[test]
    fn sec01_minimum_and_maximum_secret_sizes_round_trip() {
        let t = TempGuard::new("sizes");
        let s = t.store();
        let one = s
            .store_from_reader("a", SecretKind::AppPassword, [b'x'].as_slice())
            .unwrap();
        s.verify(&one).unwrap();
        let max = s
            .store_from_reader(
                "b",
                SecretKind::AppPassword,
                vec![b'y'; MAX_SECRET].as_slice(),
            )
            .unwrap();
        s.verify(&max).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn sec01_exact_plaintext_round_trip_is_private_and_disk_ciphertext_has_no_plaintext() {
        let t = TempGuard::new("roundtrip");
        let s = t.store();
        let secret = "exact-fixture-secret-한통".as_bytes();
        let h = s
            .store_from_reader("account-a", SecretKind::OAuthAccess, secret)
            .unwrap();
        s.verify(&h).unwrap();
        assert_eq!(
            windows_impl::read_for_test(&h.path, &h.account_id, h.kind)
                .unwrap()
                .as_slice(),
            secret
        );
        let raw = fs::read(t.root.join("account-a--oauth-access.secret")).unwrap();
        assert!(!raw.windows(secret.len()).any(|window| window == secret));
    }

    #[cfg(windows)]
    #[test]
    fn sec01_overwrite_reopen_and_delete_preserve_current_secret_state() {
        let t = TempGuard::new("overwrite");
        let s = t.store();
        let h = s
            .store_from_reader("account-a", SecretKind::OAuthAccess, b"old".as_slice())
            .unwrap();
        s.store_from_reader("account-a", SecretKind::OAuthAccess, b"new".as_slice())
            .unwrap();
        let reopened = SecretStore::open(&t.root).unwrap();
        reopened.verify(&h).unwrap();
        assert_eq!(
            windows_impl::read_for_test(&h.path, &h.account_id, h.kind)
                .unwrap()
                .as_slice(),
            b"new"
        );
        assert!(reopened.contains(&h));
        reopened.delete(&h).unwrap();
        assert!(!reopened.contains(&h));
        assert_eq!(reopened.verify(&h), Err(SecretError::NotFound));
    }

    #[cfg(windows)]
    #[test]
    fn sec01_account_and_kind_entropy_mismatch_is_rejected() {
        let t = TempGuard::new("entropy");
        let s = t.store();
        let h = s
            .store_from_reader("account-a", SecretKind::OAuthAccess, b"secret".as_slice())
            .unwrap();
        let wrong_account = SecretHandle {
            path: h.path.clone(),
            account_id: "account-b".into(),
            kind: h.kind,
        };
        let wrong_kind = SecretHandle {
            path: h.path.clone(),
            account_id: "account-a".into(),
            kind: SecretKind::AppPassword,
        };
        assert_eq!(s.verify(&wrong_account), Err(SecretError::NotFound));
        assert_eq!(s.verify(&wrong_kind), Err(SecretError::NotFound));
    }

    #[cfg(windows)]
    #[test]
    fn sec01_empty_truncated_and_oversized_ciphertexts_are_rejected() {
        let t = TempGuard::new("ciphertext");
        let s = t.store();
        let h = s
            .store_from_reader("account-a", SecretKind::OAuthAccess, b"secret".as_slice())
            .unwrap();
        let p = h.path.clone();
        fs::write(&p, []).unwrap();
        assert_eq!(s.verify(&h), Err(SecretError::InvalidCiphertext));
        fs::write(&p, [1, 2, 3]).unwrap();
        assert_eq!(s.verify(&h), Err(SecretError::InvalidCiphertext));
        fs::write(&p, vec![0u8; MAX_CIPHERTEXT + 1]).unwrap();
        assert_eq!(s.verify(&h), Err(SecretError::InvalidCiphertext));
    }

    #[cfg(windows)]
    #[test]
    fn sec01_partial_reader_error_keeps_previous_ciphertext() {
        let t = TempGuard::new("reader-error");
        let s = t.store();
        let h = s
            .store_from_reader("account-a", SecretKind::OAuthAccess, b"old".as_slice())
            .unwrap();
        let result = s.store_from_reader(
            "account-a",
            SecretKind::OAuthAccess,
            FailingReader {
                bytes: b"partial".to_vec(),
                failed: false,
            },
        );
        assert_eq!(result, Err(SecretError::Io));
        s.verify(&h).unwrap();
        assert_eq!(
            windows_impl::read_for_test(&h.path, &h.account_id, h.kind)
                .unwrap()
                .as_slice(),
            b"old"
        );
    }

    #[cfg(windows)]
    #[test]
    fn sec01_preexisting_unowned_temp_file_is_retained() {
        let t = TempGuard::new("temp-retained");
        let s = t.store();
        let temp = t.root.join("account-a--oauth-access.tmp-foreign");
        fs::write(&temp, b"foreign").unwrap();
        let h = s
            .store_from_reader("account-a", SecretKind::OAuthAccess, b"secret".as_slice())
            .unwrap();
        s.verify(&h).unwrap();
        assert_eq!(fs::read(&temp).unwrap(), b"foreign");
    }

    #[cfg(windows)]
    #[test]
    fn sec01_debug_redacts_handle_and_invalid_ids_are_rejected() {
        let t = TempGuard::new("validation");
        let s = t.store();
        let h = s
            .store_from_reader("account-a", SecretKind::OAuthAccess, b"secret".as_slice())
            .unwrap();
        let debug = format!("{h:?}");
        assert!(!debug.contains("account-a"));
        assert!(!debug.contains("fixture-secret"));
        assert!(debug.contains("<redacted>"));
        for invalid in ["", "a/b", "a\\b", "a..b", &"x".repeat(129)] {
            assert_eq!(
                s.store_from_reader(invalid, SecretKind::OAuthAccess, b"x".as_slice()),
                Err(SecretError::InvalidAccountId)
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn sec01_junction_and_junction_ancestor_are_rejected_without_following() {
        let target = TempGuard::new("junction-target");
        let links = TempGuard::new("junction-links");
        let link = links.root.join("link");
        // Junction creation does not require Windows symlink privilege. Paths
        // are child-process environment data, never interpolated shell source.
        let result = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:HANTONG_TEST_LINK -Target $env:HANTONG_TEST_TARGET | Out-Null"])
            .env("HANTONG_TEST_LINK", &link).env("HANTONG_TEST_TARGET", &target.root)
            .output().expect("PowerShell junction fixture");
        assert!(result.status.success(), "junction fixture failed");
        assert!(matches!(
            SecretStore::open(&link),
            Err(SecretError::InvalidRoot)
        ));
        assert!(matches!(
            SecretStore::open(link.join("must-not-create")),
            Err(SecretError::InvalidRoot)
        ));
        assert!(!target.root.join("must-not-create").exists());
        fs::remove_dir(&link).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn sec01_store_lifetime_prevents_ancestor_replacement() {
        let fixture = TempGuard::new("pinned-root");
        let path = fixture.root.join("store");
        let store = SecretStore::open(&path).unwrap();
        let handle = store
            .store_from_reader("a", SecretKind::AppPassword, b"secret".as_slice())
            .unwrap();
        assert!(fs::rename(&path, fixture.root.join("moved")).is_err());
        store.verify(&handle).unwrap();
        drop(store);
        fs::rename(&path, fixture.root.join("moved")).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn sec01_failed_replacement_preserves_old_ciphertext() {
        let fixture = TempGuard::new("replacement");
        let store = fixture.store();
        let handle = store
            .store_from_reader("a", SecretKind::AppPassword, b"old".as_slice())
            .unwrap();
        let held = windows_impl::open_checked(&handle.path).unwrap();
        assert_eq!(
            store.store_from_reader("a", SecretKind::AppPassword, b"new".as_slice()),
            Err(SecretError::Io)
        );
        drop(held);
        assert_eq!(
            windows_impl::read_for_test(&handle.path, &handle.account_id, handle.kind)
                .unwrap()
                .as_slice(),
            b"old"
        );
    }
    #[cfg(not(windows))]
    #[test]
    fn sec01_non_windows_never_writes_plaintext() {
        let fixture = TempGuard::new("unsupported");
        let r = fixture.root.join("must-not-create");
        assert!(matches!(
            SecretStore::open(&r),
            Err(SecretError::UnsupportedPlatform)
        ));
        assert!(!r.exists());
    }
    #[cfg(windows)]
    #[test]
    fn sec01_windows_roundtrip_tamper() {
        let t = TempGuard::new("tamper");
        let s = t.store();
        let h = s
            .store_from_reader(
                "account-a",
                SecretKind::OAuthAccess,
                b"fixture-secret".as_slice(),
            )
            .unwrap();
        s.verify(&h).unwrap();
        let p = t.root.join("account-a--oauth-access.secret");
        let mut c = fs::read(&p).unwrap();
        assert!(!c.windows(14).any(|w| w == b"fixture-secret"));
        c[0] ^= 1;
        fs::write(&p, c).unwrap();
        assert_eq!(s.verify(&h), Err(SecretError::InvalidCiphertext));
    }

    #[cfg(windows)]
    #[test]
    fn sec01_m2_mail_slots_roundtrip_reopen_and_keep_imap_smtp_independent() {
        let t = TempGuard::new("mail-slots");
        let s = t.store();
        let slots = [
            MailCredentialSlot::ImapAppPassword,
            MailCredentialSlot::ImapOAuthAccess,
            MailCredentialSlot::ImapOAuthRefresh,
            MailCredentialSlot::SmtpAppPassword,
            MailCredentialSlot::SmtpOAuthAccess,
            MailCredentialSlot::SmtpOAuthRefresh,
        ];
        for (index, slot) in slots.into_iter().enumerate() {
            let handle = s
                .store_from_reader(
                    "Acct",
                    SecretKind::Mail(slot),
                    format!("secret-{index}").as_bytes(),
                )
                .unwrap();
            assert!(s.contains(&handle));
            s.resolve_mail_handle("Acct", slot).unwrap();
            s.verify(&handle).unwrap();
            assert!(
                handle
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("v2-")
            );
        }
        let reopened = SecretStore::open(&t.root).unwrap();
        for slot in slots {
            reopened
                .verify(&reopened.resolve_mail_handle("Acct", slot).unwrap())
                .unwrap();
        }
        let imap = s
            .store_from_reader(
                "Acct",
                SecretKind::Mail(MailCredentialSlot::ImapAppPassword),
                b"old".as_slice(),
            )
            .unwrap();
        let smtp = s
            .store_from_reader(
                "Acct",
                SecretKind::Mail(MailCredentialSlot::SmtpAppPassword),
                b"smtp".as_slice(),
            )
            .unwrap();
        s.store_from_reader(
            "Acct",
            SecretKind::Mail(MailCredentialSlot::ImapAppPassword),
            b"new".as_slice(),
        )
        .unwrap();
        s.verify(&smtp).unwrap();
        s.delete(&imap).unwrap();
        assert!(s.contains(&smtp));
    }

    #[cfg(windows)]
    #[test]
    fn sec01_m2_mail_slots_case_safe_entropy_and_forged_handles_are_fail_closed() {
        let t = TempGuard::new("mail-security");
        let s = t.store();
        let upper = s
            .store_from_reader(
                "Acct",
                SecretKind::Mail(MailCredentialSlot::ImapOAuthAccess),
                b"upper".as_slice(),
            )
            .unwrap();
        let lower = s
            .store_from_reader(
                "acct",
                SecretKind::Mail(MailCredentialSlot::ImapOAuthAccess),
                b"lower".as_slice(),
            )
            .unwrap();
        assert_ne!(upper.path, lower.path);
        s.verify(&upper).unwrap();
        s.verify(&lower).unwrap();
        let forged = SecretHandle {
            path: upper.path.clone(),
            account_id: "other".into(),
            kind: upper.kind,
        };
        assert!(!s.contains(&forged));
        assert_eq!(s.verify(&forged), Err(SecretError::NotFound));
        assert_eq!(s.delete(&forged), Err(SecretError::NotFound));
        let wrong_slot = SecretHandle {
            path: upper.path.clone(),
            account_id: "Acct".into(),
            kind: SecretKind::Mail(MailCredentialSlot::SmtpOAuthAccess),
        };
        assert_eq!(s.verify(&wrong_slot), Err(SecretError::NotFound));
        assert!(s.contains(&upper));
    }

    #[cfg(windows)]
    #[test]
    fn sec01_m2_mail_copy_across_account_or_slot_is_dpapi_invalid_ciphertext() {
        let t = TempGuard::new("mail-copy");
        let s = t.store();
        let source = s
            .store_from_reader(
                "source",
                SecretKind::Mail(MailCredentialSlot::ImapOAuthAccess),
                b"secret".as_slice(),
            )
            .unwrap();
        let target = s
            .store_from_reader(
                "target",
                SecretKind::Mail(MailCredentialSlot::ImapOAuthAccess),
                b"other".as_slice(),
            )
            .unwrap();
        fs::copy(&source.path, &target.path).unwrap();
        assert_eq!(s.verify(&target), Err(SecretError::InvalidCiphertext));
        let slot = s
            .store_from_reader(
                "source",
                SecretKind::Mail(MailCredentialSlot::SmtpOAuthAccess),
                b"other".as_slice(),
            )
            .unwrap();
        fs::copy(&source.path, &slot.path).unwrap();
        assert_eq!(s.verify(&slot), Err(SecretError::InvalidCiphertext));
    }

    #[cfg(windows)]
    #[test]
    fn sec01_m2_mail_resolve_is_existence_only_and_corruption_fails_verify() {
        let t = TempGuard::new("mail-resolve");
        let s = t.store();
        let slot = MailCredentialSlot::ImapOAuthRefresh;
        let handle = s
            .store_from_reader("broken", SecretKind::Mail(slot), b"secret".as_slice())
            .unwrap();
        fs::write(&handle.path, [1, 2, 3]).unwrap();
        let resolved = s.resolve_mail_handle("broken", slot).unwrap();
        assert_eq!(s.verify(&resolved), Err(SecretError::InvalidCiphertext));
        assert_eq!(
            s.resolve_mail_handle("missing", slot),
            Err(SecretError::NotFound)
        );
    }

    #[cfg(windows)]
    #[test]
    fn sec01_m2_mail_base32hex_vectors_and_maximum_account_path() {
        assert_eq!(encode_base32hex(b""), "");
        assert_eq!(encode_base32hex(&[0]), "00");
        assert_eq!(encode_base32hex(&[0xff]), "vs");
        assert_eq!(encode_base32hex(b"foo"), "cpnmu");
        let t = TempGuard::new("mail-max-id");
        let s = t.store();
        let account = "A".repeat(128);
        let slot = MailCredentialSlot::SmtpOAuthRefresh;
        let h = s
            .store_from_reader(&account, SecretKind::Mail(slot), b"max".as_slice())
            .unwrap();
        assert!(
            h.path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .encode_utf16()
                .count()
                <= 255
        );
        s.resolve_mail_handle(&account, slot).unwrap();
        s.verify(&h).unwrap();
    }
}
