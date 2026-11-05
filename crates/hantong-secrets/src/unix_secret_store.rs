//! Unix OS credential-store boundary.
//!
//! The only persistent representation used by this module is the host OS
//! credential store.  In particular, this module deliberately has no file
//! fallback: a missing or locked Keychain/Secret Service is an error.

#[cfg(any(target_os = "linux", target_os = "macos"))]
use keyring_core::Entry;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::io::Write;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use hantong_platform::unix_paths::UnixDirectoryGuard;

#[cfg(target_os = "linux")]
const PROTECTED_LABEL: &str = "mail-hantong protected item";

#[cfg(target_os = "linux")]
static HEADLESS_BACKEND: std::sync::OnceLock<crate::headless_vault::SharedHeadlessVault> =
    std::sync::OnceLock::new();

pub const SERVICE: &str = "mail-hantong.local-security.v1";
const MAX_PKCS8: usize = 4096;
const MAX_GENERAL: usize = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnixSecretKind {
    DatabaseKey,
    ClientSigningKey,
    ServerSigningKey,
    GoogleOAuth,
    MailCredential(crate::MailCredentialSlot),
    ModelApiKey,
}

impl UnixSecretKind {
    const fn label(self) -> &'static str {
        match self {
            Self::DatabaseKey => "database-key",
            Self::ClientSigningKey => "client-signing",
            Self::ServerSigningKey => "server-signing",
            Self::GoogleOAuth => "google-oauth",
            Self::MailCredential(_) => "mail",
            Self::ModelApiKey => "model-api-key",
        }
    }

    pub(crate) const fn is_signing_key(self) -> bool {
        matches!(self, Self::ClientSigningKey | Self::ServerSigningKey)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnixSecretError {
    InvalidSelector,
    InvalidSecret,
    SecretTooLarge,
    Unavailable,
    NotFound,
    Ambiguous,
    StoreFailure,
    AlreadyExists,
    Conflict,
}

impl std::fmt::Display for UnixSecretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidSelector => "invalid Unix secret selector",
            Self::InvalidSecret => "invalid protected secret",
            Self::SecretTooLarge => "protected secret too large",
            Self::Unavailable => "host credential store unavailable",
            Self::NotFound => "protected secret not found",
            Self::Ambiguous => "protected secret selector is ambiguous",
            Self::StoreFailure => "host credential store operation failed",
            Self::AlreadyExists => "protected secret already exists",
            Self::Conflict => "protected secret store changed concurrently",
        })
    }
}
impl std::error::Error for UnixSecretError {}

/// A selector contains no secret material and is safe to use as a keyring user.
#[derive(Clone, Eq, PartialEq)]
pub struct UnixSecretSelector {
    kind: UnixSecretKind,
    value: String,
}

impl std::fmt::Debug for UnixSecretSelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("UnixSecretSelector")
            .field(&"<redacted>")
            .finish()
    }
}

impl UnixSecretSelector {
    pub fn new(
        kind: UnixSecretKind,
        account_id: Option<&str>,
        version: u64,
    ) -> Result<Self, UnixSecretError> {
        if version == 0 {
            return Err(UnixSecretError::InvalidSelector);
        }
        let value = match kind {
            UnixSecretKind::DatabaseKey if account_id.is_none() && version == 1 => {
                "database-key".to_string()
            }
            UnixSecretKind::ClientSigningKey | UnixSecretKind::ServerSigningKey => {
                let id = account_id.ok_or(UnixSecretError::InvalidSelector)?;
                if version != 1 || id.len() != 64 || !is_lower_hex(id) {
                    return Err(UnixSecretError::InvalidSelector);
                }
                format!("{}-{id}", kind.label())
            }
            UnixSecretKind::GoogleOAuth => {
                let id = account_id.ok_or(UnixSecretError::InvalidSelector)?;
                let id = encoded_account(id)?;
                if id.is_empty() || id.len() > 384 || version == 0 {
                    return Err(UnixSecretError::InvalidSelector);
                }
                format!("google-{id}-v{version}")
            }
            UnixSecretKind::ModelApiKey => {
                let id = account_id.ok_or(UnixSecretError::InvalidSelector)?;
                let id = encoded_account(id)?;
                if id.is_empty() || id.len() > 384 || version != 1 {
                    return Err(UnixSecretError::InvalidSelector);
                }
                format!("model-{id}")
            }
            UnixSecretKind::MailCredential(slot) => {
                let id = account_id.ok_or(UnixSecretError::InvalidSelector)?;
                let id = encoded_account(id)?;
                if id.is_empty() || id.len() > 384 || version != 1 {
                    return Err(UnixSecretError::InvalidSelector);
                }
                format!("mail-{id}-{}", slot.name())
            }
            _ => return Err(UnixSecretError::InvalidSelector),
        };
        if value.len() > 384 {
            return Err(UnixSecretError::InvalidSelector);
        }
        Ok(Self { kind, value })
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.value
    }

    pub(crate) fn kind(&self) -> UnixSecretKind {
        self.kind
    }
}

/// Memory-only secret bytes.  It has no public byte accessor by design.
pub struct ProtectedBytes(Vec<u8>);

impl ProtectedBytes {
    pub(crate) fn from_backend(bytes: Vec<u8>) -> Result<Self, UnixSecretError> {
        if bytes.is_empty() {
            let mut bytes = bytes;
            crate::wipe(&mut bytes);
            return Err(UnixSecretError::InvalidSecret);
        }
        if bytes.len() > MAX_GENERAL {
            let mut bytes = bytes;
            crate::wipe(&mut bytes);
            return Err(UnixSecretError::SecretTooLarge);
        }
        Ok(Self(bytes))
    }

    pub(crate) fn as_bytes_in_secrets(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for ProtectedBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProtectedBytes(<redacted>)")
    }
}

impl Drop for ProtectedBytes {
    fn drop(&mut self) {
        crate::wipe(&mut self.0);
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub struct UnixSecretStore {
    #[cfg(target_os = "linux")]
    headless: Option<crate::headless_vault::SharedHeadlessVault>,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl UnixSecretStore {
    /// Emit a closed diagnostic for failures while opening the host store.
    ///
    /// Keep this boundary deliberately free of paths, selectors, provider
    /// details, and backend error text.  The Core maps the public result to a
    /// stable error, while this line identifies the internal stage that
    /// failed for local troubleshooting.
    pub(crate) fn report_open_failure(stage: &'static str, kind: &'static str, code: &'static str) {
        let _ = writeln!(
            std::io::stderr().lock(),
            "{}",
            open_failure_line(stage, kind, code)
        );
    }

    pub(crate) const fn diagnostic_kind(error: UnixSecretError) -> &'static str {
        match error {
            UnixSecretError::InvalidSelector => "invalid_selector",
            UnixSecretError::InvalidSecret => "invalid_secret",
            UnixSecretError::SecretTooLarge => "secret_too_large",
            UnixSecretError::Unavailable => "unavailable",
            UnixSecretError::NotFound => "not_found",
            UnixSecretError::Ambiguous => "ambiguous",
            UnixSecretError::StoreFailure => "store_failure",
            UnixSecretError::AlreadyExists => "already_exists",
            UnixSecretError::Conflict => "conflict",
        }
    }

    /// Installs the real host credential store for this process.
    ///
    /// Linux uses freedesktop Secret Service over zbus.  macOS uses the
    /// legacy Keychain store because the CLI is not provisioned/sandboxed;
    /// the protected-data store is intentionally not selected here.
    pub fn install_platform_store() -> Result<(), UnixSecretError> {
        #[cfg(target_os = "linux")]
        if HEADLESS_BACKEND.get().is_some() {
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        let store = zbus_secret_service_keyring_store::Store::new().map_err(map_keyring_error)?;
        #[cfg(target_os = "macos")]
        let store =
            apple_native_keyring_store::keychain::Store::new().map_err(map_keyring_error)?;
        keyring_core::set_default_store(store);
        Ok(())
    }

    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            headless: HEADLESS_BACKEND.get().cloned(),
        }
    }

    #[cfg(target_os = "linux")]
    pub fn new_headless(vault: crate::headless_vault::SharedHeadlessVault) -> Self {
        Self {
            headless: Some(vault),
        }
    }

    #[cfg(target_os = "linux")]
    pub fn install_headless_backend(
        vault: crate::headless_vault::SharedHeadlessVault,
    ) -> Result<(), UnixSecretError> {
        HEADLESS_BACKEND
            .set(vault)
            .map_err(|_| UnixSecretError::Conflict)
    }

    fn entry(selector: &UnixSecretSelector) -> Result<Entry, UnixSecretError> {
        #[cfg(target_os = "linux")]
        {
            let modifiers = std::collections::HashMap::from([("label", PROTECTED_LABEL)]);
            Entry::new_with_modifiers(SERVICE, selector.as_str(), &modifiers)
                .map_err(map_keyring_error)
        }
        #[cfg(target_os = "macos")]
        {
            // The Apple store does not accept Secret-Service's `label`
            // modifier.  Its service/user pair remains the stable protected
            // item identity; no user-derived label is ever sent to Keychain.
            Entry::new(SERVICE, selector.as_str()).map_err(map_keyring_error)
        }
    }

    pub fn read(&self, selector: &UnixSecretSelector) -> Result<ProtectedBytes, UnixSecretError> {
        #[cfg(target_os = "linux")]
        if let Some(vault) = &self.headless {
            let vault = vault.lock().map_err(|_| UnixSecretError::Unavailable)?;
            return vault.read(selector.as_str());
        }
        let entry = Self::entry(selector)?;
        let bytes = entry.get_secret().map_err(map_keyring_error)?;
        let protected = ProtectedBytes::from_backend(bytes)?;
        validate_secret(protected.as_bytes_in_secrets(), selector.kind())?;
        Ok(protected)
    }

    /// Create the exclusive, cross-process mutation lock beneath the already
    /// validated secrets directory.  Read operations remain lock-free.
    pub fn mutation_guard(
        root: &UnixDirectoryGuard,
    ) -> Result<UnixSecretMutationGuard, UnixSecretError> {
        UnixSecretMutationGuard::lock(root)
    }

    pub fn create(
        &self,
        guard: &UnixSecretMutationGuard,
        selector: &UnixSecretSelector,
        secret: &[u8],
        kind: UnixSecretKind,
    ) -> Result<(), UnixSecretError> {
        #[cfg(target_os = "linux")]
        if let Some(vault) = &self.headless {
            validate_secret(secret, kind)?;
            if selector.kind() != kind {
                return Err(UnixSecretError::InvalidSelector);
            }
            let vault = vault.lock().map_err(|_| UnixSecretError::Unavailable)?;
            let current = vault.generation()?;
            if vault.read(selector.as_str()).is_ok() {
                return Err(UnixSecretError::AlreadyExists);
            }
            vault.replace(selector.as_str(), secret, current)?;
            return Ok(());
        }
        validate_secret(secret, kind)?;
        if selector.kind() != kind {
            return Err(UnixSecretError::InvalidSelector);
        }
        let _guard = guard;
        let entry = Self::entry(selector)?;
        match entry.get_secret() {
            Ok(mut existing) => {
                crate::wipe(&mut existing);
                return Err(UnixSecretError::AlreadyExists);
            }
            Err(keyring_core::Error::NoEntry) => {}
            Err(error) => return Err(map_keyring_error(error)),
        }
        entry.set_secret(secret).map_err(map_keyring_error)
    }

    pub fn replace(
        &self,
        _guard: &UnixSecretMutationGuard,
        selector: &UnixSecretSelector,
        secret: &[u8],
        kind: UnixSecretKind,
    ) -> Result<(), UnixSecretError> {
        #[cfg(target_os = "linux")]
        if let Some(vault) = &self.headless {
            validate_secret(secret, kind)?;
            if selector.kind() != kind {
                return Err(UnixSecretError::InvalidSelector);
            }
            let vault = vault.lock().map_err(|_| UnixSecretError::Unavailable)?;
            let current = vault.generation()?;
            vault.replace(selector.as_str(), secret, current)?;
            return Ok(());
        }
        validate_secret(secret, kind)?;
        if selector.kind() != kind {
            return Err(UnixSecretError::InvalidSelector);
        }
        let entry = Self::entry(selector)?;
        let mut existing = entry.get_secret().map_err(map_keyring_error)?;
        let result = entry.set_secret(secret).map_err(map_keyring_error);
        if result.is_err() {
            // Preserve the prior credential if the backend rejects the
            // replacement.  The old bytes are wiped on every path.
            let _ = entry.set_secret(&existing);
        }
        crate::wipe(&mut existing);
        result
    }

    pub fn delete(
        &self,
        _guard: &UnixSecretMutationGuard,
        selector: &UnixSecretSelector,
    ) -> Result<(), UnixSecretError> {
        #[cfg(target_os = "linux")]
        if let Some(vault) = &self.headless {
            let vault = vault.lock().map_err(|_| UnixSecretError::Unavailable)?;
            let current = vault.generation()?;
            vault.delete(selector.as_str(), current)?;
            return Ok(());
        }
        Self::entry(selector)?
            .delete_credential()
            .map_err(map_keyring_error)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn open_failure_line(stage: &str, kind: &str, code: &str) -> String {
    format!("hantong_secret_store stage={stage} kind={kind} code={code}")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub struct UnixSecretMutationGuard {
    file: std::fs::File,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl UnixSecretMutationGuard {
    pub fn lock(root: &UnixDirectoryGuard) -> Result<Self, UnixSecretError> {
        let file = root
            .open_or_create_regular("secret-store.lock")
            .map_err(|_| UnixSecretError::Unavailable)?;
        file.try_lock().map_err(|_| UnixSecretError::StoreFailure)?;
        Ok(Self { file })
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Drop for UnixSecretMutationGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn map_keyring_error(error: keyring_core::Error) -> UnixSecretError {
    match error {
        keyring_core::Error::NoEntry => UnixSecretError::NotFound,
        keyring_core::Error::Ambiguous(_) => UnixSecretError::Ambiguous,
        keyring_core::Error::NoDefaultStore => UnixSecretError::Unavailable,
        keyring_core::Error::NoStorageAccess(_) => UnixSecretError::Unavailable,
        keyring_core::Error::NotSupportedByStore(_) => UnixSecretError::Unavailable,
        keyring_core::Error::BadEncoding(mut bytes) => {
            crate::wipe(&mut bytes);
            UnixSecretError::InvalidSecret
        }
        keyring_core::Error::BadDataFormat(mut bytes, _) => {
            crate::wipe(&mut bytes);
            UnixSecretError::InvalidSecret
        }
        _ => UnixSecretError::StoreFailure,
    }
}

fn validate_secret(secret: &[u8], kind: UnixSecretKind) -> Result<(), UnixSecretError> {
    if matches!(kind, UnixSecretKind::DatabaseKey) && secret.len() != 32 {
        return Err(UnixSecretError::InvalidSecret);
    }
    let limit = if kind.is_signing_key() {
        MAX_PKCS8
    } else {
        MAX_GENERAL
    };
    if secret.is_empty() {
        return Err(UnixSecretError::InvalidSecret);
    }
    if secret.len() > limit {
        return Err(UnixSecretError::SecretTooLarge);
    }
    if kind.is_signing_key() && !looks_like_pkcs8(secret) {
        return Err(UnixSecretError::InvalidSecret);
    }
    Ok(())
}

fn looks_like_pkcs8(bytes: &[u8]) -> bool {
    // DER PKCS#8 starts with a SEQUENCE.  Full curve/key validation is done
    // by the signer before a key can be used; this bounds accidental storage
    // of arbitrary text without attempting to parse private key material here.
    bytes.first() == Some(&0x30) && bytes.len() >= 16
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn encoded_account(account_id: &str) -> Result<String, UnixSecretError> {
    crate::validate_account(account_id).map_err(|_| UnixSecretError::InvalidSelector)?;
    Ok(crate::encode_base32hex(account_id.as_bytes()))
}

impl Default for UnixSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec08_open_diagnostics_are_closed_and_redacted() {
        let line = open_failure_line("root_validation", "guard_rejected", "SCOPE_DENIED");
        assert_eq!(
            line,
            "hantong_secret_store stage=root_validation kind=guard_rejected code=SCOPE_DENIED"
        );
        assert!(!line.contains("/"));
        assert!(!line.contains("password"));
    }

    #[test]
    fn sec08_keyring_failures_map_to_closed_diagnostic_kinds() {
        assert_eq!(
            UnixSecretStore::diagnostic_kind(map_keyring_error(
                keyring_core::Error::NoDefaultStore
            )),
            "unavailable"
        );
        assert_eq!(
            UnixSecretStore::diagnostic_kind(map_keyring_error(keyring_core::Error::NoEntry)),
            "not_found"
        );
        assert_eq!(
            UnixSecretStore::diagnostic_kind(map_keyring_error(
                keyring_core::Error::NoStorageAccess(Box::new(std::io::Error::other(
                    "sentinel secret"
                )),)
            )),
            "unavailable"
        );
        assert_eq!(
            UnixSecretStore::diagnostic_kind(map_keyring_error(
                keyring_core::Error::PlatformFailure(Box::new(std::io::Error::other(
                    "sentinel secret"
                )),)
            )),
            "store_failure"
        );
        let line = open_failure_line(
            "platform_store_init",
            "store_failure",
            "UNSUPPORTED_PLATFORM",
        );
        assert!(!line.contains("sentinel"));
    }

    #[test]
    fn sec01_selector_is_stable_and_redacted() {
        let id = "a".repeat(64);
        let selector =
            UnixSecretSelector::new(UnixSecretKind::ClientSigningKey, Some(&id), 1).unwrap();
        assert_eq!(selector.as_str(), format!("client-signing-{id}"));
        assert!(!format!("{selector:?}").contains(&id));
        assert!(selector.as_str().len() <= 384);
    }

    #[test]
    fn sec01_signing_key_requires_bounded_der_shape() {
        let kind = UnixSecretKind::ClientSigningKey;
        assert_eq!(
            validate_secret(b"plaintext", kind),
            Err(UnixSecretError::InvalidSecret)
        );
        assert!(validate_secret(&[0x30; 32], kind).is_ok());
        assert_eq!(
            validate_secret(&vec![0x30; MAX_PKCS8 + 1], kind),
            Err(UnixSecretError::SecretTooLarge)
        );
    }

    #[test]
    fn sec01_database_key_is_exactly_32_bytes() {
        assert_eq!(
            validate_secret(&[0_u8; 31], UnixSecretKind::DatabaseKey),
            Err(UnixSecretError::InvalidSecret)
        );
        assert!(validate_secret(&[0_u8; 32], UnixSecretKind::DatabaseKey).is_ok());
        assert_eq!(
            validate_secret(&[0_u8; 33], UnixSecretKind::DatabaseKey),
            Err(UnixSecretError::InvalidSecret)
        );
    }

    #[test]
    fn sec01_selector_rejects_injection_and_zero_version() {
        assert_eq!(
            UnixSecretSelector::new(UnixSecretKind::GoogleOAuth, Some("a:b"), 1),
            Err(UnixSecretError::InvalidSelector)
        );
        assert_eq!(
            UnixSecretSelector::new(UnixSecretKind::GoogleOAuth, Some("a"), 0),
            Err(UnixSecretError::InvalidSelector)
        );
        assert_eq!(
            UnixSecretSelector::new(UnixSecretKind::ClientSigningKey, Some(&"A".repeat(64)), 1),
            Err(UnixSecretError::InvalidSelector)
        );
        assert_eq!(
            UnixSecretSelector::new(UnixSecretKind::DatabaseKey, Some("unexpected"), 1),
            Err(UnixSecretError::InvalidSelector)
        );
    }
}
