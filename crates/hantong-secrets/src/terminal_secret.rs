//! Same-console secret capture for the foreground Core helper.
//!
//! Plaintext is read and kept inside this crate until it is protected with
//! current-user DPAPI.  Callers receive only a ticket or an opaque staged
//! handle; neither type contains the captured bytes.

#[cfg(windows)]
use crate::windows_impl;
use crate::{
    SecretError, SecretHandle, SecretKind, SecretStore, SensitiveBuffer, validate_account,
    validate_path,
};
use std::{
    fmt, fs,
    io::{Cursor, Read},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use hantong_platform::unix_paths::UnixDirectoryGuard;
#[cfg(not(windows))]
use ring::rand::SecureRandom;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::io::Write;

const MAX_TERMINAL_SECRET: usize = 16 * 1024;
#[cfg(any(target_os = "linux", target_os = "macos"))]
const MAX_STAGE_SCRYPT_WORK_FACTOR: u8 = 18;
const OPAQUE_TICKET_BYTES: usize = 320;
#[cfg(test)]
const TICKET_TTL_MS: i64 = 60_000;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct TerminalSecretBinding {
    request_kind: [u8; 16],
    request_id: [u8; 32],
    payload_hash: [u8; 32],
    account_hash: [u8; 32],
    endpoint_hash: [u8; 32],
    expected_version: u64,
    sid_hash: [u8; 32],
    logon_hash: [u8; 32],
    server_hash: [u8; 32],
    expires_at_utc_ms: i64,
}

impl fmt::Debug for TerminalSecretBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TerminalSecretBinding { <redacted> }")
    }
}

impl TerminalSecretBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_kind: [u8; 16],
        request_id: [u8; 32],
        payload_hash: [u8; 32],
        account_hash: [u8; 32],
        endpoint_hash: [u8; 32],
        expected_version: u64,
        sid_hash: [u8; 32],
        logon_hash: [u8; 32],
        server_hash: [u8; 32],
        expires_at_utc_ms: i64,
    ) -> Result<Self, SecretError> {
        if request_kind == [0; 16]
            || request_id == [0; 32]
            || payload_hash == [0; 32]
            || account_hash == [0; 32]
            || endpoint_hash == [0; 32]
            || sid_hash == [0; 32]
            || logon_hash == [0; 32]
            || server_hash == [0; 32]
            || expected_version == 0
            || expires_at_utc_ms <= now_utc_ms()
        {
            return Err(SecretError::InvalidCredential);
        }
        Ok(Self {
            request_kind,
            request_id,
            payload_hash,
            account_hash,
            endpoint_hash,
            expected_version,
            sid_hash,
            logon_hash,
            server_hash,
            expires_at_utc_ms,
        })
    }

    pub fn is_expired(&self) -> bool {
        now_utc_ms() >= self.expires_at_utc_ms
    }

    pub fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }

    pub fn matches(&self, other: &Self) -> bool {
        self == other
    }
}

/// The ticket is intentionally opaque.  Its binding is retained in-memory by
/// the foreground Core and is checked again when the staged value is consumed.
pub struct TerminalSecretTicket {
    binding: TerminalSecretBinding,
    ticket_id: [u8; 32],
    nonce: [u8; 32],
}

impl fmt::Debug for TerminalSecretTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TerminalSecretTicket { <redacted> }")
    }
}

impl TerminalSecretTicket {
    pub fn binding(&self) -> TerminalSecretBinding {
        self.binding
    }

    /// An opaque identifier suitable for a typed IPC response.  It is not a
    /// credential and cannot be used without the live ticket and its binding.
    pub fn opaque_id(&self) -> [u8; 32] {
        self.ticket_id
    }

    /// Serializes only the typed ticket binding and random ticket material.
    /// This is opaque transport data for the foreground Core, never a CLI
    /// approval or a credential.  `from_opaque` validates it before use.
    pub fn to_opaque(&self) -> [u8; OPAQUE_TICKET_BYTES] {
        let mut out = [0; OPAQUE_TICKET_BYTES];
        let mut offset = 0;
        macro_rules! put {
            ($value:expr) => {{
                let value = $value;
                out[offset..offset + value.len()].copy_from_slice(value);
                offset += value.len();
            }};
        }
        put!(&self.binding.request_kind);
        put!(&self.binding.request_id);
        put!(&self.binding.payload_hash);
        put!(&self.binding.account_hash);
        put!(&self.binding.endpoint_hash);
        put!(&self.binding.expected_version.to_be_bytes());
        put!(&self.binding.sid_hash);
        put!(&self.binding.logon_hash);
        put!(&self.binding.server_hash);
        put!(&self.binding.expires_at_utc_ms.to_be_bytes());
        put!(&self.ticket_id);
        put!(&self.nonce[..24]);
        let checksum = opaque_checksum(&out[..offset]);
        put!(&checksum);
        debug_assert_eq!(offset, OPAQUE_TICKET_BYTES);
        out
    }

    pub fn from_opaque(bytes: &[u8]) -> Result<Self, SecretError> {
        if bytes.len() != OPAQUE_TICKET_BYTES {
            return Err(SecretError::CredentialMismatch);
        }
        let mut offset = 0;
        macro_rules! take {
            ($length:expr) => {{
                let end = offset + $length;
                let value = &bytes[offset..end];
                offset = end;
                value
            }};
        }
        let mut request_kind = [0; 16];
        request_kind.copy_from_slice(take!(16));
        let mut request_id = [0; 32];
        request_id.copy_from_slice(take!(32));
        let mut payload_hash = [0; 32];
        payload_hash.copy_from_slice(take!(32));
        let mut account_hash = [0; 32];
        account_hash.copy_from_slice(take!(32));
        let mut endpoint_hash = [0; 32];
        endpoint_hash.copy_from_slice(take!(32));
        let expected_version = u64::from_be_bytes(take!(8).try_into().unwrap());
        let mut sid_hash = [0; 32];
        sid_hash.copy_from_slice(take!(32));
        let mut logon_hash = [0; 32];
        logon_hash.copy_from_slice(take!(32));
        let mut server_hash = [0; 32];
        server_hash.copy_from_slice(take!(32));
        let expires_at_utc_ms = i64::from_be_bytes(take!(8).try_into().unwrap());
        let mut ticket_id = [0; 32];
        ticket_id.copy_from_slice(take!(32));
        let mut nonce = [0; 32];
        nonce[..24].copy_from_slice(&bytes[offset..offset + 24]);
        let expected_checksum = opaque_checksum(&bytes[..offset + 24]);
        nonce[24..].copy_from_slice(&bytes[offset + 24..offset + 32]);
        if nonce[24..] != expected_checksum {
            return Err(SecretError::CredentialMismatch);
        }
        let binding = TerminalSecretBinding::new(
            request_kind,
            request_id,
            payload_hash,
            account_hash,
            endpoint_hash,
            expected_version,
            sid_hash,
            logon_hash,
            server_hash,
            expires_at_utc_ms,
        )?;
        if ticket_id == [0; 32] || nonce == [0; 32] {
            return Err(SecretError::CredentialMismatch);
        }
        Ok(Self {
            binding,
            ticket_id,
            nonce,
        })
    }

    pub fn is_expired(&self) -> bool {
        self.binding.is_expired()
    }
}

pub struct TerminalSecretHandle {
    ticket_id: [u8; 32],
    path: PathBuf,
}

impl fmt::Debug for TerminalSecretHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TerminalSecretHandle { <redacted> }")
    }
}

impl TerminalSecretHandle {
    /// Returns only the random staged-ticket identifier.  The filesystem path
    /// and any captured bytes remain inside Secrets.
    pub fn opaque_id(&self) -> [u8; 32] {
        self.ticket_id
    }
}

impl SecretStore {
    /// Consumes a staged terminal value and writes the endpoint-bound IMAP
    /// envelope entirely inside Secrets. Plaintext never crosses this API.
    pub fn consume_terminal_imap_credential(
        &self,
        ticket: TerminalSecretTicket,
        handle: TerminalSecretHandle,
        expected: TerminalSecretBinding,
        account: &str,
        endpoint: &hantong_domain::mail_connection::ImapEndpointConfig,
    ) -> Result<SecretHandle, SecretError> {
        self.consume_terminal_bound(ticket, handle, expected, |bytes| {
            self.store_imap_credential(account, endpoint, Cursor::new(bytes))
        })
    }

    /// Consumes a staged terminal value and writes the endpoint-bound SMTP
    /// envelope entirely inside Secrets. Plaintext never crosses this API.
    pub fn consume_terminal_smtp_credential(
        &self,
        ticket: TerminalSecretTicket,
        handle: TerminalSecretHandle,
        expected: TerminalSecretBinding,
        account: &str,
        endpoint: &hantong_domain::mail_connection::SmtpEndpointConfig,
    ) -> Result<SecretHandle, SecretError> {
        self.consume_terminal_bound(ticket, handle, expected, |bytes| {
            self.store_smtp_credential(account, endpoint, Cursor::new(bytes))
        })
    }

    fn consume_terminal_bound<F>(
        &self,
        ticket: TerminalSecretTicket,
        handle: TerminalSecretHandle,
        expected: TerminalSecretBinding,
        store: F,
    ) -> Result<SecretHandle, SecretError>
    where
        F: FnOnce(&[u8]) -> Result<SecretHandle, SecretError>,
    {
        if ticket.is_expired() || !ticket.binding.matches(&expected) {
            return Err(SecretError::CredentialMismatch);
        }
        let path = self.terminal_secret_path(&ticket.ticket_id);
        if handle.ticket_id != ticket.ticket_id || handle.path != path {
            return Err(SecretError::CredentialMismatch);
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        if fs::symlink_metadata(&path).is_err() {
            return Err(SecretError::CredentialMismatch);
        }
        #[cfg(windows)]
        {
            let entropy = terminal_entropy(&ticket);
            let result = windows_impl::with_scoped_secret(&path, &entropy, |bytes| {
                if bytes.is_empty() || bytes.len() > MAX_TERMINAL_SECRET {
                    return Err(SecretError::InvalidCredential);
                }
                store(bytes)
            });
            let _ = fs::remove_file(&path);
            result
        }
        #[cfg(not(windows))]
        {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            {
                let secret = consume_linux_staged(&path, &ticket)?;
                if secret.as_slice().is_empty() || secret.as_slice().len() > MAX_TERMINAL_SECRET {
                    return Err(SecretError::InvalidCredential);
                }
                store(secret.as_slice())
            }
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            {
                let _ = (path, store);
                Err(SecretError::UnsupportedPlatform)
            }
        }
    }

    /// Reconstructs a staged handle from its opaque identifier after IPC.
    /// The caller must still provide the live ticket; the identifier alone is
    /// never sufficient to consume a staged value.
    pub fn terminal_secret_handle_from_opaque(
        &self,
        ticket: &TerminalSecretTicket,
        opaque_id: [u8; 32],
    ) -> Result<TerminalSecretHandle, SecretError> {
        if ticket.is_expired() || opaque_id == [0; 32] || opaque_id != ticket.ticket_id {
            return Err(SecretError::CredentialMismatch);
        }
        let path = self.terminal_secret_path(&opaque_id);
        validate_path(&self.root)?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if fs::symlink_metadata(&path)
            .ok()
            .filter(|meta| meta.is_file() && !meta.file_type().is_symlink())
            .is_none()
        {
            return Err(SecretError::CredentialMismatch);
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        if fs::symlink_metadata(&path).is_err() {
            return Err(SecretError::CredentialMismatch);
        }
        Ok(TerminalSecretHandle {
            ticket_id: opaque_id,
            path,
        })
    }

    /// Consume a staged Google client secret without returning plaintext to
    /// Core.  The opaque credential is constructed inside Secrets while the
    /// DPAPI scoped value is live, then the staging file is deleted.
    #[cfg(windows)]
    pub fn consume_terminal_google_client_secret(
        &self,
        ticket: TerminalSecretTicket,
        handle: TerminalSecretHandle,
        expected: TerminalSecretBinding,
        client_id: &str,
    ) -> Result<crate::google::GoogleClientSecret, SecretError> {
        if ticket.is_expired() || !ticket.binding.matches(&expected) {
            return Err(SecretError::CredentialMismatch);
        }
        let path = self.terminal_secret_path(&ticket.ticket_id);
        if handle.ticket_id != ticket.ticket_id
            || handle.path != path
            || fs::symlink_metadata(&path).is_err()
        {
            return Err(SecretError::CredentialMismatch);
        }
        let entropy = terminal_entropy(&ticket);
        let result = windows_impl::with_scoped_secret(&path, &entropy, |bytes| {
            if bytes.is_empty() || bytes.len() > MAX_TERMINAL_SECRET {
                return Err(SecretError::InvalidCredential);
            }
            crate::google::GoogleClientSecret::from_capture(
                client_id,
                SensitiveBuffer(bytes.to_vec()),
            )
            .map_err(|_| SecretError::InvalidCredential)
        });
        let _ = fs::remove_file(&path);
        result
    }

    /// Issues a one-use ticket for a same-console capture.  The random ticket
    /// and DPAPI entropy are generated only inside Secrets.
    pub fn issue_terminal_secret_ticket(
        &self,
        binding: TerminalSecretBinding,
    ) -> Result<TerminalSecretTicket, SecretError> {
        if binding.is_expired() {
            return Err(SecretError::InputExpired);
        }
        let mut ticket_id = [0; 32];
        let mut nonce = [0; 32];
        random_bytes(&mut ticket_id)?;
        random_bytes(&mut nonce)?;
        Ok(TerminalSecretTicket {
            binding,
            ticket_id,
            nonce,
        })
    }

    /// Stages a captured value as a DPAPI-protected opaque file.  The reader
    /// is consumed here, so plaintext never crosses the Secrets API boundary.
    pub fn stage_terminal_secret<R: Read>(
        &self,
        ticket: &TerminalSecretTicket,
        reader: R,
    ) -> Result<TerminalSecretHandle, SecretError> {
        if ticket.is_expired() {
            return Err(SecretError::InputExpired);
        }
        let path = self.terminal_secret_path(&ticket.ticket_id);
        validate_path(&self.root)?;
        if fs::symlink_metadata(&path).is_ok() {
            return Err(SecretError::CredentialMismatch);
        }
        let secret = SensitiveBuffer::read_limited(reader, MAX_TERMINAL_SECRET)?;
        self.stage_terminal_secret_bytes(ticket, secret)
    }

    /// Reads one masked line from the inherited console and stages it.  This
    /// is the only console-reading entry point; the echo guard restores the
    /// original mode on success, EOF, cancellation, and every error path.
    #[cfg(windows)]
    pub fn capture_terminal_secret(
        &self,
        ticket: &TerminalSecretTicket,
    ) -> Result<TerminalSecretHandle, SecretError> {
        if ticket.is_expired() {
            return Err(SecretError::InputExpired);
        }
        let secret = read_masked_console_line()?;
        self.stage_terminal_secret_bytes(ticket, secret)
    }

    #[cfg(not(windows))]
    pub fn capture_terminal_secret(
        &self,
        ticket: &TerminalSecretTicket,
    ) -> Result<TerminalSecretHandle, SecretError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            if ticket.is_expired() {
                return Err(SecretError::InputExpired);
            }
            let secret = read_masked_terminal_line()?;
            self.stage_terminal_secret_bytes(ticket, secret)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = ticket;
            Err(SecretError::UnsupportedPlatform)
        }
    }

    /// Captures into the Secrets-owned encrypted stage without opening a
    /// keyring backend. This is used by a separate foreground review helper
    /// while the primary Core owns the headless vault lock.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn capture_terminal_secret_at(
        root: impl AsRef<std::path::Path>,
        ticket: &TerminalSecretTicket,
    ) -> Result<TerminalSecretHandle, SecretError> {
        let root = root.as_ref();
        if let Err(error) = validate_path(root) {
            report_terminal_capture_failure(
                TerminalCaptureStage::ProtectedStaging,
                terminal_stage_kind(error),
            );
            return Err(error);
        }
        capture_terminal_secret_after_ticket(ticket, read_masked_terminal_line, |secret| {
            Self::stage_terminal_secret_bytes_at(root, ticket, secret)
        })
    }

    /// Removes one staged value after a cancelled or failed review. The
    /// ticket id is the only selector accepted; no credential bytes leave
    /// Secrets.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn cancel_terminal_secret_at(
        root: impl AsRef<std::path::Path>,
        ticket: &TerminalSecretTicket,
        opaque_id: [u8; 32],
    ) -> Result<(), SecretError> {
        let root = root.as_ref();
        validate_path(root)?;
        if ticket.is_expired() || opaque_id == [0; 32] || opaque_id != ticket.ticket_id {
            return Err(SecretError::CredentialMismatch);
        }
        let path = root.join(format!(
            "terminal-v1-{}.stage",
            hantong_contracts::wire_hex::encode(&opaque_id)
        ));
        let metadata = fs::symlink_metadata(&path).map_err(|_| SecretError::NotFound)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(SecretError::CredentialMismatch);
        }
        let guard = UnixDirectoryGuard::open(root, true).map_err(|_| SecretError::Protection)?;
        let file = guard
            .open_existing_regular(&format!(
                "terminal-v1-{}.stage",
                hantong_contracts::wire_hex::encode(&opaque_id)
            ))
            .map_err(|_| SecretError::Protection)?;
        guard
            .unlink_owned(
                &format!(
                    "terminal-v1-{}.stage",
                    hantong_contracts::wire_hex::encode(&opaque_id)
                ),
                &file,
            )
            .map_err(|_| SecretError::Protection)
    }

    fn stage_terminal_secret_bytes(
        &self,
        ticket: &TerminalSecretTicket,
        secret: SensitiveBuffer,
    ) -> Result<TerminalSecretHandle, SecretError> {
        Self::stage_terminal_secret_bytes_at(&self.root, ticket, secret)
    }

    fn stage_terminal_secret_bytes_at(
        root: &std::path::Path,
        ticket: &TerminalSecretTicket,
        secret: SensitiveBuffer,
    ) -> Result<TerminalSecretHandle, SecretError> {
        // Input can block while the console user types.  Recheck immediately
        // before protection so a ticket that expired during capture is never
        // staged for later consumption.
        if ticket.is_expired() {
            return Err(SecretError::InputExpired);
        }
        validate_terminal_secret(secret.as_slice())?;
        let path = root.join(format!(
            "terminal-v1-{}.stage",
            hantong_contracts::wire_hex::encode(&ticket.ticket_id)
        ));
        validate_path(root)?;
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        if fs::symlink_metadata(&path).is_ok() {
            return Err(SecretError::CredentialMismatch);
        }
        #[cfg(windows)]
        {
            let entropy = terminal_entropy(ticket);
            windows_impl::store_scoped(&path, &entropy, secret.as_slice())?;
            Ok(TerminalSecretHandle {
                ticket_id: ticket.ticket_id,
                path,
            })
        }
        #[cfg(not(windows))]
        {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            {
                write_linux_staged(root, ticket, secret.as_slice())?;
                Ok(TerminalSecretHandle {
                    ticket_id: ticket.ticket_id,
                    path,
                })
            }
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            {
                let _ = (ticket, path, secret);
                Err(SecretError::UnsupportedPlatform)
            }
        }
    }

    /// Consumes a staged value exactly once and writes it into the ordinary
    /// DPAPI-backed secret slot.  The expected binding must equal the ticket's
    /// original binding; callers cannot retarget an already captured value.
    pub fn consume_terminal_secret(
        &self,
        ticket: TerminalSecretTicket,
        handle: TerminalSecretHandle,
        expected: TerminalSecretBinding,
        account_id: &str,
        kind: SecretKind,
    ) -> Result<SecretHandle, SecretError> {
        if ticket.is_expired() {
            return Err(SecretError::InputExpired);
        }
        if !ticket.binding.matches(&expected) || handle.ticket_id != ticket.ticket_id {
            return Err(SecretError::CredentialMismatch);
        }
        validate_account(account_id)?;
        let path = self.terminal_secret_path(&ticket.ticket_id);
        if handle.path != path {
            return Err(SecretError::CredentialMismatch);
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        if fs::symlink_metadata(&path).is_err() {
            return Err(SecretError::CredentialMismatch);
        }
        #[cfg(windows)]
        {
            let entropy = terminal_entropy(&ticket);
            let result = windows_impl::with_scoped_secret(&path, &entropy, |bytes| {
                if bytes.is_empty() || bytes.len() > MAX_TERMINAL_SECRET {
                    return Err(SecretError::InvalidCredential);
                }
                let target = self.handle(account_id, kind);
                self.store_bytes(&target, account_id, kind, bytes)?;
                Ok(target)
            });
            let _ = fs::remove_file(&path);
            result
        }
        #[cfg(not(windows))]
        {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            {
                if !ticket.binding.matches(&expected) || handle.ticket_id != ticket.ticket_id {
                    return Err(SecretError::CredentialMismatch);
                }
                let secret = consume_linux_staged(&path, &ticket)?;
                if secret.as_slice().is_empty() || secret.as_slice().len() > MAX_TERMINAL_SECRET {
                    return Err(SecretError::InvalidCredential);
                }
                let target = self.handle(account_id, kind);
                self.store_bytes(&target, account_id, kind, secret.as_slice())?;
                Ok(target)
            }
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            {
                let _ = (ticket, handle, expected, account_id, kind);
                Err(SecretError::UnsupportedPlatform)
            }
        }
    }

    fn terminal_secret_path(&self, ticket_id: &[u8; 32]) -> PathBuf {
        self.root.join(format!(
            "terminal-v1-{}.stage",
            hantong_contracts::wire_hex::encode(ticket_id)
        ))
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn read_masked_console_line() -> Result<SensitiveBuffer, SecretError> {
    use std::os::windows::io::RawHandle;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        ENABLE_ECHO_INPUT, ENABLE_PROCESSED_INPUT, GetConsoleMode, GetStdHandle, ReadConsoleW,
        STD_INPUT_HANDLE, SetConsoleMode,
    };

    struct EchoGuard {
        handle: RawHandle,
        mode: u32,
    }
    impl EchoGuard {
        fn new() -> Result<Self, SecretError> {
            // SAFETY: GetStdHandle returns the process-owned standard input
            // handle; no ownership is transferred.
            let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return Err(SecretError::InvalidInputWindow);
            }
            let mut mode = 0;
            // SAFETY: mode points to writable storage for one DWORD and the
            // handle remains valid for the guard lifetime.
            if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
                return Err(SecretError::InvalidInputWindow);
            }
            // Keep line and processed-input behavior, disabling only echo.
            // SAFETY: the same live console handle is used and the mode is a
            // value returned by GetConsoleMode.
            if unsafe {
                SetConsoleMode(handle, mode & !(ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT))
            } == 0
            {
                return Err(SecretError::InvalidInputWindow);
            }
            Ok(Self { handle, mode })
        }
    }
    impl Drop for EchoGuard {
        fn drop(&mut self) {
            // SAFETY: best-effort restoration of the mode captured from this
            // handle. Drop cannot report an error without hiding the original.
            let _ = unsafe { SetConsoleMode(self.handle, self.mode) };
        }
    }

    let guard = EchoGuard::new()?;
    let mut wide = [0u16; MAX_TERMINAL_SECRET / 2];
    struct WideWipe(*mut u16, usize);
    impl Drop for WideWipe {
        fn drop(&mut self) {
            // SAFETY: pointer and length refer to the stack buffer for the
            // entire lifetime of this guard.
            unsafe { wipe_wide(std::slice::from_raw_parts_mut(self.0, self.1)) };
        }
    }
    let _wide_wipe = WideWipe(wide.as_mut_ptr(), wide.len());
    let mut read = 0u32;
    // SAFETY: wide is writable for exactly its declared capacity; ReadConsoleW
    // is bounded by that capacity and the live guard handle.
    if unsafe {
        ReadConsoleW(
            guard.handle,
            wide.as_mut_ptr().cast(),
            wide.len() as u32,
            &mut read,
            std::ptr::null_mut(),
        )
    } == 0
        || read == 0
    {
        return Err(SecretError::InvalidCredential);
    }
    let has_line_end = wide[..read as usize]
        .iter()
        .any(|code| *code == '\r' as u16 || *code == '\n' as u16);
    if !has_line_end && read as usize == wide.len() {
        let mut drain = [0u16; 256];
        loop {
            let mut drained = 0u32;
            let ok = unsafe {
                ReadConsoleW(
                    guard.handle,
                    drain.as_mut_ptr().cast(),
                    drain.len() as u32,
                    &mut drained,
                    std::ptr::null_mut(),
                )
            } != 0;
            let ended = drain[..drained as usize]
                .iter()
                .any(|code| *code == '\r' as u16 || *code == '\n' as u16);
            unsafe { wipe_wide(&mut drain) };
            if !ok || drained == 0 {
                return Err(SecretError::InvalidCredential);
            }
            if ended {
                return Err(SecretError::SecretTooLarge);
            }
        }
    }
    let mut bytes = SensitiveBuffer(Vec::with_capacity(read as usize));
    let units = wide[..read as usize]
        .iter()
        .copied()
        .take_while(|code| *code != '\r' as u16 && *code != '\n' as u16);
    for ch in std::char::decode_utf16(units) {
        let ch = ch.map_err(|_| SecretError::InvalidCredential)?;
        let mut encoded = [0u8; 4];
        bytes
            .0
            .extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
        if bytes.0.len() > MAX_TERMINAL_SECRET {
            return Err(SecretError::SecretTooLarge);
        }
    }
    drop(guard);
    validate_terminal_secret(bytes.as_slice())?;
    Ok(bytes)
}

#[cfg(windows)]
#[allow(unsafe_code)]
unsafe fn wipe_wide(bytes: &mut [u16]) {
    for value in bytes {
        unsafe { std::ptr::write_volatile(value, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

fn validate_terminal_secret(bytes: &[u8]) -> Result<(), SecretError> {
    if bytes.is_empty() {
        return Err(SecretError::EmptySecret);
    }
    if bytes.len() > MAX_TERMINAL_SECRET {
        return Err(SecretError::InvalidCredential);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| SecretError::InvalidCredential)?;
    if text.chars().any(char::is_control) {
        return Err(SecretError::InvalidCredential);
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalCaptureStage {
    Input,
    ProtectedStaging,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalCaptureKind {
    EmptyInput,
    TtyUnavailable,
    Timeout,
    TimeoutAfterInput,
    InputIo,
    InvalidInput,
    ProtectedStore,
    BindingMismatch,
    MissingStage,
    StageQuota,
    Failure,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl TerminalCaptureStage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::ProtectedStaging => "protected_staging",
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl TerminalCaptureKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::EmptyInput => "empty_input",
            Self::TtyUnavailable => "tty_unavailable",
            Self::Timeout => "timeout",
            Self::TimeoutAfterInput => "timeout_after_input",
            Self::InputIo => "input_io",
            Self::InvalidInput => "invalid_input",
            Self::ProtectedStore => "protected_store",
            Self::BindingMismatch => "binding_mismatch",
            Self::MissingStage => "missing_stage",
            Self::StageQuota => "stage_quota",
            Self::Failure => "failure",
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn terminal_input_kind(error: SecretError) -> TerminalCaptureKind {
    match error {
        SecretError::EmptySecret => TerminalCaptureKind::EmptyInput,
        SecretError::InvalidInputWindow => TerminalCaptureKind::TtyUnavailable,
        SecretError::InputExpired => TerminalCaptureKind::Timeout,
        SecretError::Io => TerminalCaptureKind::InputIo,
        SecretError::SecretTooLarge | SecretError::InvalidCredential => {
            TerminalCaptureKind::InvalidInput
        }
        _ => TerminalCaptureKind::Failure,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn terminal_stage_kind(error: SecretError) -> TerminalCaptureKind {
    match error {
        SecretError::InputExpired => TerminalCaptureKind::TimeoutAfterInput,
        SecretError::Protection | SecretError::InvalidRoot | SecretError::Io => {
            TerminalCaptureKind::ProtectedStore
        }
        SecretError::CredentialMismatch | SecretError::InvalidCiphertext => {
            TerminalCaptureKind::BindingMismatch
        }
        SecretError::NotFound => TerminalCaptureKind::MissingStage,
        SecretError::SecretTooLarge => TerminalCaptureKind::StageQuota,
        _ => TerminalCaptureKind::Failure,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn report_terminal_capture_failure(stage: TerminalCaptureStage, kind: TerminalCaptureKind) {
    let _ = writeln!(
        std::io::stderr().lock(),
        "hantong_terminal_secret stage={} kind={}",
        stage.as_str(),
        kind.as_str()
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn capture_terminal_secret_after_ticket<R, S>(
    ticket: &TerminalSecretTicket,
    read: R,
    stage: S,
) -> Result<TerminalSecretHandle, SecretError>
where
    R: FnOnce() -> Result<SensitiveBuffer, SecretError>,
    S: FnOnce(SensitiveBuffer) -> Result<TerminalSecretHandle, SecretError>,
{
    if ticket.is_expired() {
        report_terminal_capture_failure(TerminalCaptureStage::Input, TerminalCaptureKind::Timeout);
        return Err(SecretError::InputExpired);
    }
    let secret = match read() {
        Ok(secret) => secret,
        Err(error) => {
            report_terminal_capture_failure(
                TerminalCaptureStage::Input,
                terminal_input_kind(error),
            );
            return Err(error);
        }
    };
    match stage(secret) {
        Ok(handle) => Ok(handle),
        Err(error) => {
            report_terminal_capture_failure(
                TerminalCaptureStage::ProtectedStaging,
                terminal_stage_kind(error),
            );
            Err(error)
        }
    }
}

/// Read one protected-input line without waiting for EOF. A terminal stays
/// open after Enter, so the general bounded reader is unsuitable here.
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
fn read_line_limited<R: Read>(mut reader: R, limit: usize) -> Result<SensitiveBuffer, SecretError> {
    if limit > crate::MAX_SECRET {
        return Err(SecretError::SecretTooLarge);
    }
    let mut output = SensitiveBuffer(vec![0; limit + 1]);
    let mut used = 0;
    loop {
        let count = match reader.read(&mut output.0[used..used + 1]) {
            Ok(0) => return Err(SecretError::InvalidCredential),
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(SecretError::Io),
        };
        if count != 1 {
            return Err(SecretError::Io);
        }
        if output.0[used] == b'\n' {
            output.0.truncate(used);
            if output.0.last() == Some(&b'\r') {
                output.0.pop();
            }
            return Ok(output);
        }
        used += 1;
        if used == limit + 1 {
            return Err(SecretError::SecretTooLarge);
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn linux_stage_passphrase(ticket: &TerminalSecretTicket) -> age::secrecy::SecretString {
    age::secrecy::SecretString::from(hantong_contracts::wire_hex::encode(&ticket.nonce))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn write_linux_staged(
    root: &std::path::Path,
    ticket: &TerminalSecretTicket,
    secret: &[u8],
) -> Result<(), SecretError> {
    let guard = UnixDirectoryGuard::open(root, true).map_err(|_| SecretError::Protection)?;
    let name = format!(
        "terminal-v1-{}.stage",
        hantong_contracts::wire_hex::encode(&ticket.ticket_id)
    );
    let mut recipient = age::scrypt::Recipient::new(linux_stage_passphrase(ticket));
    recipient.set_work_factor(MAX_STAGE_SCRYPT_WORK_FACTOR);
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
            .map_err(|_| SecretError::Protection)?;
    let mut ciphertext = Vec::new();
    let mut writer = encryptor
        .wrap_output(&mut ciphertext)
        .map_err(|_| SecretError::Protection)?;
    writer
        .write_all(secret)
        .map_err(|_| SecretError::Protection)?;
    writer.finish().map_err(|_| SecretError::Protection)?;
    if ciphertext.len() > MAX_TERMINAL_SECRET * 2 {
        return Err(SecretError::SecretTooLarge);
    }
    let tmp = format!(".{}.tmp-{}", name, std::process::id());
    let mut file = guard
        .create_new_regular(&tmp)
        .map_err(|_| SecretError::Protection)?;
    let result = (|| {
        file.write_all(&ciphertext)
            .map_err(|_| SecretError::Protection)?;
        file.sync_all().map_err(|_| SecretError::Protection)?;
        drop(file);
        guard
            .rename_under_lock(&tmp, &name)
            .map_err(|_| SecretError::Protection)?;
        sync_linux_directory(&guard)
    })();
    if result.is_err()
        && let Ok(tmp_file) = guard.open_existing_regular(&tmp)
    {
        let _ = guard.unlink_owned(&tmp, &tmp_file);
    }
    result
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn consume_linux_staged(
    path: &std::path::Path,
    ticket: &TerminalSecretTicket,
) -> Result<SensitiveBuffer, SecretError> {
    let root = path.parent().ok_or(SecretError::CredentialMismatch)?;
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or(SecretError::CredentialMismatch)?;
    let guard =
        UnixDirectoryGuard::open(root, true).map_err(|_| SecretError::CredentialMismatch)?;
    let file = guard
        .open_existing_readonly_regular(name)
        .map_err(|_| SecretError::CredentialMismatch)?;
    let meta = file
        .metadata()
        .map_err(|_| SecretError::CredentialMismatch)?;
    if meta.len() == 0 || meta.len() > (MAX_TERMINAL_SECRET * 2) as u64 {
        return Err(SecretError::CredentialMismatch);
    }
    let mut ciphertext = Vec::with_capacity(meta.len() as usize);
    (&file)
        .take((MAX_TERMINAL_SECRET * 2 + 1) as u64)
        .read_to_end(&mut ciphertext)
        .map_err(|_| SecretError::CredentialMismatch)?;
    let decryptor =
        age::Decryptor::new(ciphertext.as_slice()).map_err(|_| SecretError::CredentialMismatch)?;
    let mut identity = age::scrypt::Identity::new(linux_stage_passphrase(ticket));
    identity.set_max_work_factor(MAX_STAGE_SCRYPT_WORK_FACTOR);
    let reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|_| SecretError::CredentialMismatch)?;
    let mut plaintext = SensitiveBuffer(Vec::new());
    reader
        .take((MAX_TERMINAL_SECRET + 1) as u64)
        .read_to_end(&mut plaintext.0)
        .map_err(|_| SecretError::CredentialMismatch)?;
    if plaintext.0.len() > MAX_TERMINAL_SECRET {
        return Err(SecretError::SecretTooLarge);
    }
    guard
        .unlink_owned(name, &file)
        .map_err(|_| SecretError::Protection)?;
    sync_linux_directory(&guard)?;
    Ok(plaintext)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn sync_linux_directory(guard: &UnixDirectoryGuard) -> Result<(), SecretError> {
    let fd = guard
        .directory_fd()
        .map_err(|_| SecretError::Protection)?
        .try_clone_to_owned()
        .map_err(|_| SecretError::Protection)?;
    std::fs::File::from(fd)
        .sync_all()
        .map_err(|_| SecretError::Protection)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(unsafe_code)]
pub(crate) fn read_masked_terminal_line() -> Result<SensitiveBuffer, SecretError> {
    use std::os::fd::AsRawFd;

    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| SecretError::InvalidInputWindow)?;
    let fd = tty.as_raw_fd();
    let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
    if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } != 0 {
        return Err(SecretError::InvalidInputWindow);
    }
    let original = unsafe { original.assume_init() };
    let mut masked = original;
    masked.c_lflag &= !libc::ECHO;
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &masked) } != 0 {
        return Err(SecretError::InvalidInputWindow);
    }
    struct Restore {
        fd: libc::c_int,
        mode: libc::termios,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            // SAFETY: fd and mode were obtained from the same live /dev/tty.
            let _ = unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.mode) };
        }
    }
    let _restore = Restore { fd, mode: original };
    let secret = read_line_limited(&mut tty, MAX_TERMINAL_SECRET)?;
    validate_terminal_secret(secret.as_slice())?;
    Ok(secret)
}

fn opaque_checksum(bytes: &[u8]) -> [u8; 8] {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish().to_be_bytes()
}

fn now_utc_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(windows)]
fn random_bytes(bytes: &mut [u8]) -> Result<(), SecretError> {
    windows_impl::random_bytes(bytes)
}

#[cfg(not(windows))]
fn random_bytes(bytes: &mut [u8]) -> Result<(), SecretError> {
    ring::rand::SystemRandom::new()
        .fill(bytes)
        .map_err(|_| SecretError::Protection)
}

#[cfg(windows)]
fn terminal_entropy(ticket: &TerminalSecretTicket) -> Vec<u8> {
    let mut entropy = b"mail-hantong.terminal-secret.v1\0".to_vec();
    entropy.extend_from_slice(&ticket.ticket_id);
    entropy.extend_from_slice(&ticket.nonce);
    entropy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(expires: i64) -> TerminalSecretBinding {
        TerminalSecretBinding::new(
            [1; 16], [2; 32], [3; 32], [4; 32], [5; 32], 1, [6; 32], [7; 32], [8; 32], expires,
        )
        .unwrap()
    }

    #[test]
    fn terminal_secret_binding_rejects_zero_and_expired_values() {
        assert!(
            TerminalSecretBinding::new(
                [0; 16],
                [2; 32],
                [3; 32],
                [4; 32],
                [5; 32],
                1,
                [6; 32],
                [7; 32],
                [8; 32],
                now_utc_ms() + TICKET_TTL_MS,
            )
            .is_err()
        );
        assert!(
            TerminalSecretBinding::new(
                [1; 16],
                [2; 32],
                [3; 32],
                [4; 32],
                [5; 32],
                1,
                [6; 32],
                [7; 32],
                [8; 32],
                now_utc_ms() - 1,
            )
            .is_err()
        );
    }

    #[test]
    fn terminal_secret_input_rejects_eof_empty_controls_and_oversize() {
        assert_eq!(validate_terminal_secret(b""), Err(SecretError::EmptySecret));
        assert!(validate_terminal_secret(b"secret\n").is_err());
        assert!(validate_terminal_secret(&vec![b'x'; MAX_TERMINAL_SECRET + 1]).is_err());
        assert!(validate_terminal_secret(b" exact ").is_ok());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn sec08_terminal_secret_capture_failure_kinds_are_fixed_and_stage_specific() {
        assert_eq!(
            terminal_input_kind(SecretError::EmptySecret),
            TerminalCaptureKind::EmptyInput
        );
        assert_eq!(
            terminal_input_kind(SecretError::InvalidInputWindow),
            TerminalCaptureKind::TtyUnavailable
        );
        assert_eq!(
            terminal_input_kind(SecretError::InputExpired),
            TerminalCaptureKind::Timeout
        );
        assert_eq!(
            terminal_stage_kind(SecretError::InputExpired),
            TerminalCaptureKind::TimeoutAfterInput
        );
        assert_eq!(
            terminal_stage_kind(SecretError::Protection),
            TerminalCaptureKind::ProtectedStore
        );
        assert_eq!(
            terminal_stage_kind(SecretError::CredentialMismatch),
            TerminalCaptureKind::BindingMismatch
        );
        assert_eq!(
            terminal_stage_kind(SecretError::NotFound),
            TerminalCaptureKind::MissingStage
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn sec08_terminal_secret_capture_seam_stops_before_stage_on_input_error() {
        let ticket = TerminalSecretTicket {
            binding: binding(now_utc_ms() + TICKET_TTL_MS),
            ticket_id: [9; 32],
            nonce: [10; 32],
        };
        for error in [
            SecretError::EmptySecret,
            SecretError::InvalidInputWindow,
            SecretError::Io,
            SecretError::InputExpired,
        ] {
            let mut staged = false;
            let result = capture_terminal_secret_after_ticket(
                &ticket,
                || Err(error),
                |_secret| {
                    staged = true;
                    unreachable!("input failure must never stage or submit")
                },
            );
            assert!(matches!(result, Err(actual) if actual == error));
            assert!(!staged);
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn sec08_terminal_secret_capture_seam_reports_post_input_expiry_as_staging_failure() {
        let ticket = TerminalSecretTicket {
            binding: binding(now_utc_ms() + TICKET_TTL_MS),
            ticket_id: [9; 32],
            nonce: [10; 32],
        };
        for error in [
            SecretError::Protection,
            SecretError::InvalidRoot,
            SecretError::Io,
            SecretError::InputExpired,
        ] {
            let mut stages = 0;
            let result = capture_terminal_secret_after_ticket(
                &ticket,
                || Ok(SensitiveBuffer(b"synthetic".to_vec())),
                |_secret| {
                    stages += 1;
                    Err(error)
                },
            );
            assert!(matches!(result, Err(actual) if actual == error));
            assert_eq!(stages, 1);
        }
        assert_eq!(
            terminal_stage_kind(SecretError::InputExpired),
            TerminalCaptureKind::TimeoutAfterInput
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn sec08_terminal_secret_expired_ticket_does_not_read_or_stage() {
        let mut ticket = TerminalSecretTicket {
            binding: binding(now_utc_ms() + TICKET_TTL_MS),
            ticket_id: [9; 32],
            nonce: [10; 32],
        };
        ticket.binding.expires_at_utc_ms = 0;
        let result = capture_terminal_secret_after_ticket(
            &ticket,
            || unreachable!("expired ticket must not read input"),
            |_| unreachable!("expired ticket must not stage"),
        );
        assert!(matches!(result, Err(SecretError::InputExpired)));
    }

    #[test]
    fn terminal_secret_line_reader_returns_at_newline_without_waiting_for_eof() {
        struct StopsAfterNewline {
            bytes: &'static [u8],
            offset: usize,
        }
        impl Read for StopsAfterNewline {
            fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
                assert_eq!(output.len(), 1);
                assert!(self.offset < self.bytes.len(), "reader used after newline");
                output[0] = self.bytes[self.offset];
                self.offset += 1;
                Ok(1)
            }
        }
        let value = read_line_limited(
            StopsAfterNewline {
                bytes: b"secret\n",
                offset: 0,
            },
            MAX_TERMINAL_SECRET,
        )
        .unwrap();
        assert_eq!(value.as_slice(), b"secret");
    }

    #[test]
    fn terminal_secret_line_reader_rejects_eof_and_overflow() {
        assert!(matches!(
            read_line_limited(Cursor::new(b"secret"), MAX_TERMINAL_SECRET),
            Err(SecretError::InvalidCredential)
        ));
        assert!(matches!(
            read_line_limited(
                Cursor::new(vec![b'x'; MAX_TERMINAL_SECRET + 2]),
                MAX_TERMINAL_SECRET
            ),
            Err(SecretError::SecretTooLarge)
        ));
    }

    #[test]
    fn terminal_ticket_debug_and_binding_debug_are_redacted() {
        let b = binding(now_utc_ms() + TICKET_TTL_MS);
        assert!(!format!("{b:?}").contains("payload"));
    }

    #[test]
    fn terminal_ticket_opaque_round_trip_rejects_tampering() {
        let ticket = TerminalSecretTicket {
            binding: binding(now_utc_ms() + TICKET_TTL_MS),
            ticket_id: [9; 32],
            nonce: [10; 32],
        };
        let encoded = ticket.to_opaque();
        let decoded = TerminalSecretTicket::from_opaque(&encoded).unwrap();
        assert_eq!(decoded.opaque_id(), [9; 32]);
        let mut tampered = encoded;
        tampered[0] = 0;
        assert!(TerminalSecretTicket::from_opaque(&tampered).is_err());
        assert!(TerminalSecretTicket::from_opaque(&encoded[..319]).is_err());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn terminal_secret_linux_stage_is_encrypted_and_cross_process_readable() {
        if let Some(root) = std::env::var_os("HANTONG_STAGE_CHILD_ROOT") {
            let root = std::path::PathBuf::from(root);
            let ticket = TerminalSecretTicket {
                binding: binding(now_utc_ms() + TICKET_TTL_MS),
                ticket_id: [9; 32],
                nonce: [10; 32],
            };
            let path = root.join(format!(
                "terminal-v1-{}.stage",
                hantong_contracts::wire_hex::encode(&ticket.ticket_id)
            ));
            let recovered = consume_linux_staged(&path, &ticket).unwrap();
            assert_eq!(recovered.as_slice(), b"synthetic-cross-process-secret");
            assert!(!path.exists());
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "hantong-terminal-linux-stage-{}-{}",
            std::process::id(),
            now_utc_ms()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
        // macOS exposes the temporary directory through /var, while the
        // guarded path validator intentionally rejects symlink ancestors.
        // Pin the fixture to its canonical directory before handing it to
        // production staging code; no production path policy is relaxed.
        let root = std::fs::canonicalize(&root).unwrap();
        let ticket = TerminalSecretTicket {
            binding: binding(now_utc_ms() + TICKET_TTL_MS),
            ticket_id: [9; 32],
            nonce: [10; 32],
        };
        let handle = SecretStore::stage_terminal_secret_bytes_at(
            &root,
            &ticket,
            SensitiveBuffer(b"synthetic-cross-process-secret".to_vec()),
        )
        .unwrap();
        let bytes = std::fs::read(&handle.path).unwrap();
        assert!(
            !bytes
                .windows(b"synthetic-cross-process-secret".len())
                .any(|window| window == b"synthetic-cross-process-secret")
        );
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal_secret::tests::terminal_secret_linux_stage_is_encrypted_and_cross_process_readable",
                "--nocapture",
            ])
            .env("HANTONG_STAGE_CHILD_ROOT", &root)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(
            !handle.path.exists(),
            "the child must consume the staged file"
        );
        std::fs::remove_dir(root).unwrap();
    }

    // This end-to-end credential fixture uses the Linux-only isolated vault;
    // the cross-process encrypted stage above runs on both Unix targets.
    #[cfg(target_os = "linux")]
    #[test]
    fn terminal_secret_linux_wrong_binding_preserves_stage_and_replay_is_rejected() {
        use hantong_domain::mail_connection::MailConnectionConfig;
        use std::io::Cursor;

        let root = std::env::temp_dir().join(format!(
            "hantong-terminal-secret-linux-{}-{}",
            std::process::id(),
            now_utc_ms()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
        let store = crate::test_headless_store(&root).unwrap();
        let endpoint = MailConnectionConfig::from_json(
            &serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "account_id": "fixture",
                "imap": {
                    "settings_version": 1,
                    "host": "mail.example.test",
                    "port": 993,
                    "tls": "implicit_tls",
                    "login_name": "fixture@example.test",
                    "authentication": "app_password"
                },
                "smtp": {
                    "settings_version": 1,
                    "host": "mail.example.test",
                    "port": 465,
                    "tls": "implicit_tls",
                    "login_name": "fixture@example.test",
                    "authentication": "app_password"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let expected = binding(now_utc_ms() + TICKET_TTL_MS);
        let ticket = store.issue_terminal_secret_ticket(expected).unwrap();
        let nonce = ticket.nonce;
        let handle = store
            .stage_terminal_secret(&ticket, Cursor::new(b"synthetic-secret"))
            .unwrap();
        let opaque = handle.opaque_id();
        let mut wrong = expected;
        wrong.payload_hash = [9; 32];
        assert_eq!(
            store.consume_terminal_imap_credential(
                ticket,
                handle,
                wrong,
                "fixture",
                endpoint.imap(),
            ),
            Err(SecretError::CredentialMismatch)
        );
        let ticket = TerminalSecretTicket {
            binding: expected,
            ticket_id: opaque,
            nonce,
        };
        let handle = store
            .terminal_secret_handle_from_opaque(&ticket, opaque)
            .unwrap();
        let replay_ticket = TerminalSecretTicket {
            binding: ticket.binding,
            ticket_id: ticket.ticket_id,
            nonce: ticket.nonce,
        };
        store
            .consume_terminal_imap_credential(ticket, handle, expected, "fixture", endpoint.imap())
            .unwrap();
        assert!(
            store
                .terminal_secret_handle_from_opaque(&replay_ticket, opaque)
                .is_err()
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn terminal_mail_capture_stores_endpoint_bound_envelopes_and_consumes_once() {
        use hantong_domain::mail_connection::MailConnectionConfig;
        let endpoint = |host: &str| {
            MailConnectionConfig::from_json(&serde_json::to_vec(&serde_json::json!({
            "schema_version":1,"account_id":"fixture","imap":{"settings_version":1,"host":host,"port":993,
            "tls":"implicit_tls","login_name":"fixture@example.test","authentication":"app_password"},
            "smtp":{"settings_version":1,"host":host,"port":465,
            "tls":"implicit_tls","login_name":"fixture@example.test","authentication":"app_password"}
        })).unwrap()).unwrap()
        };
        let expected = endpoint("mail.example.test");
        let wrong = endpoint("other.example.test");
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let root = temp.join(format!(
            "hantong-terminal-bound-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        assert_eq!(root.parent(), Some(temp.as_path()));
        let store = SecretStore::open(&root).unwrap();
        for smtp in [false, true] {
            let b = binding(now_utc_ms() + TICKET_TTL_MS);
            let ticket = store.issue_terminal_secret_ticket(b).unwrap();
            let handle = store
                .stage_terminal_secret(&ticket, std::io::Cursor::new(b"synthetic-credential"))
                .unwrap();
            let replay = ticket.to_opaque();
            let handle_id = handle.opaque_id();
            if smtp {
                store
                    .consume_terminal_smtp_credential(
                        ticket,
                        handle,
                        b,
                        "fixture",
                        expected.smtp().unwrap(),
                    )
                    .unwrap();
                store
                    .verify_smtp_credential("fixture", expected.smtp().unwrap())
                    .unwrap();
                assert!(
                    store
                        .verify_smtp_credential("fixture", wrong.smtp().unwrap())
                        .is_err()
                );
            } else {
                store
                    .consume_terminal_imap_credential(ticket, handle, b, "fixture", expected.imap())
                    .unwrap();
                store
                    .verify_imap_credential("fixture", expected.imap())
                    .unwrap();
                assert!(
                    store
                        .verify_imap_credential("fixture", wrong.imap())
                        .is_err()
                );
            }
            let replay = TerminalSecretTicket::from_opaque(&replay).unwrap();
            assert!(
                store
                    .terminal_secret_handle_from_opaque(&replay, handle_id)
                    .is_err()
            );
        }
        drop(store);
        assert_eq!(root.canonicalize().unwrap().parent(), Some(temp.as_path()));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn terminal_secret_wrong_binding_preserves_stage_and_replay_is_rejected() {
        use std::io::Cursor;

        let root =
            std::env::temp_dir().join(format!("hantong-terminal-secret-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = SecretStore::open(&root).unwrap();
        let original = binding(now_utc_ms() + TICKET_TTL_MS);
        let ticket = store.issue_terminal_secret_ticket(original).unwrap();
        let handle = store
            .stage_terminal_secret(&ticket, Cursor::new(b"synthetic-secret"))
            .unwrap();
        let replay_ticket = TerminalSecretTicket {
            binding: ticket.binding,
            ticket_id: ticket.ticket_id,
            nonce: ticket.nonce,
        };
        let replay_handle = TerminalSecretHandle {
            ticket_id: handle.ticket_id,
            path: handle.path.clone(),
        };
        let second_ticket = TerminalSecretTicket {
            binding: ticket.binding,
            ticket_id: ticket.ticket_id,
            nonce: ticket.nonce,
        };
        let second_handle = TerminalSecretHandle {
            ticket_id: handle.ticket_id,
            path: handle.path.clone(),
        };
        let mut wrong = original;
        wrong.payload_hash = [9; 32];
        assert_eq!(
            store.consume_terminal_secret(
                ticket,
                handle,
                wrong,
                "fixture",
                SecretKind::AppPassword,
            ),
            Err(SecretError::CredentialMismatch)
        );
        assert!(replay_handle.path.exists());

        let stored = store
            .consume_terminal_secret(
                replay_ticket,
                replay_handle,
                original,
                "fixture",
                SecretKind::AppPassword,
            )
            .unwrap();
        assert!(store.contains(&stored));
        // The staged path was removed by the successful consume.  Reusing the
        // original opaque handle therefore cannot consume a second time.
        assert_eq!(
            store.consume_terminal_secret(
                second_ticket,
                second_handle,
                original,
                "fixture",
                SecretKind::AppPassword,
            ),
            Err(SecretError::CredentialMismatch)
        );
        drop(store);
        let _ = std::fs::remove_dir_all(&root);
    }
}
