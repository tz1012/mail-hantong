//! Secret-owned setup input boundary.
//!
//! The native EDIT implementation is intentionally kept behind this module. No
//! public API exposes a window handle or plaintext; the Windows UI integration
//! will attach the control only on the owning UI thread.
use crate::{SecretError, SecretHandle, SecretStore, SensitiveBuffer};
use hantong_domain::mail_connection::{Authentication, ImapEndpointConfig};
use std::{
    fmt,
    marker::PhantomData,
    rc::Rc,
    time::{Duration, Instant},
};

const MAX_PASSWORD_BYTES: usize = 1024;
const INPUT_TTL: Duration = Duration::from_secs(300);

pub struct ImapPasswordCapturePlan {
    account_id: String,
    endpoint: ImapEndpointConfig,
    request_id: [u8; 32],
    payload_hash: [u8; 32],
    nonce: [u8; 32],
    #[cfg(windows)]
    started: Instant,
    // The plan may cross from Core to its UI thread, but is not shared.
    _not_sync: PhantomData<CellMarker>,
}
struct CellMarker(std::cell::Cell<()>);
impl fmt::Debug for ImapPasswordCapturePlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapPasswordCapturePlan { <redacted> }")
    }
}
impl ImapPasswordCapturePlan {
    pub fn new(
        account_id: impl Into<String>,
        endpoint: &ImapEndpointConfig,
        request_id: [u8; 32],
        payload_hash: [u8; 32],
    ) -> Result<Self, SecretError> {
        let account_id = account_id.into();
        crate::validate_account(&account_id)?;
        if endpoint.authentication() != Authentication::AppPassword
            || request_id == [0; 32]
            || payload_hash == [0; 32]
        {
            return Err(SecretError::InvalidCredential);
        }
        let nonce = random_nonce()?;
        Ok(Self {
            account_id,
            endpoint: endpoint.clone(),
            request_id,
            payload_hash,
            nonce,
            #[cfg(windows)]
            started: Instant::now(),
            _not_sync: PhantomData,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn endpoint(&self) -> &ImapEndpointConfig {
        &self.endpoint
    }
    pub fn setup_request_id(&self) -> &[u8; 32] {
        &self.request_id
    }
    pub fn setup_payload_hash(&self) -> &[u8; 32] {
        &self.payload_hash
    }
    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }

    #[cfg(windows)]
    pub fn attach_to_parent(
        self,
        parent_hwnd: isize,
        control_id: u16,
    ) -> Result<PasswordInput, SecretError> {
        native::attach(self, parent_hwnd, control_id)
    }
    #[cfg(not(windows))]
    pub fn attach_to_parent(
        self,
        _parent_hwnd: isize,
        _control_id: u16,
    ) -> Result<PasswordInput, SecretError> {
        Err(SecretError::UnsupportedPlatform)
    }
}

pub struct PendingImapCredential {
    plan: ImapPasswordCapturePlan,
    secret: SensitiveBuffer,
    captured_at: Instant,
    _not_sync: PhantomData<CellMarker>,
}
impl fmt::Debug for PendingImapCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PendingImapCredential { <redacted> }")
    }
}
impl PendingImapCredential {
    pub fn account_id(&self) -> &str {
        self.plan.account_id()
    }
    pub fn endpoint(&self) -> &ImapEndpointConfig {
        self.plan.endpoint()
    }
    pub fn setup_request_id(&self) -> &[u8; 32] {
        self.plan.setup_request_id()
    }
    pub fn setup_payload_hash(&self) -> &[u8; 32] {
        self.plan.setup_payload_hash()
    }
    pub fn nonce(&self) -> &[u8; 32] {
        self.plan.nonce()
    }
}

pub struct PasswordInput {
    #[cfg(windows)]
    native: native::NativeInput,
    plan: Option<ImapPasswordCapturePlan>,
    _thread_bound: PhantomData<Rc<()>>,
}
impl fmt::Debug for PasswordInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PasswordInput { <redacted> }")
    }
}
impl Drop for PasswordInput {
    fn drop(&mut self) {
        #[cfg(windows)]
        let _ = self.native.destroy();
    }
}
impl PasswordInput {
    pub fn set_bounds(&self, x: i32, y: i32, width: i32, height: i32) -> Result<(), SecretError> {
        #[cfg(windows)]
        {
            self.native.set_bounds(x, y, width, height)
        }
        #[cfg(not(windows))]
        {
            let _ = (x, y, width, height);
            Err(SecretError::UnsupportedPlatform)
        }
    }
    pub fn focus(&self) -> Result<(), SecretError> {
        #[cfg(windows)]
        {
            self.native.focus()?;
            Ok(())
        }
        #[cfg(not(windows))]
        {
            Err(SecretError::UnsupportedPlatform)
        }
    }
    pub fn set_enabled(&self, _enabled: bool) -> Result<(), SecretError> {
        #[cfg(windows)]
        {
            self.native.set_enabled(_enabled)
        }
        #[cfg(not(windows))]
        {
            Err(SecretError::UnsupportedPlatform)
        }
    }
    pub fn set_font(&self, font: isize) -> Result<(), SecretError> {
        #[cfg(windows)]
        {
            self.native.set_font(font)
        }
        #[cfg(not(windows))]
        {
            let _ = font;
            Err(SecretError::UnsupportedPlatform)
        }
    }
    pub fn matches_child_hwnd(&self, _candidate: isize) -> bool {
        #[cfg(windows)]
        {
            self.native.matches(_candidate)
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
    pub fn cancel(mut self) -> Result<(), SecretError> {
        self.plan.take();
        #[cfg(windows)]
        {
            self.native.destroy()
        }
        #[cfg(not(windows))]
        {
            Ok(())
        }
    }
    #[cfg(windows)]
    pub fn capture(mut self) -> Result<PendingImapCredential, SecretError> {
        let plan = self.plan.take().ok_or(SecretError::InvalidInputWindow)?;
        if plan.started.elapsed() >= INPUT_TTL {
            let _ = self.native.destroy();
            return Err(SecretError::InputExpired);
        }
        let secret = match self.native.capture() {
            Ok(value) => value,
            Err(error) => {
                let _ = self.native.destroy();
                return Err(error);
            }
        };
        validate_password(secret.as_slice())?;
        self.native.destroy()?;
        Ok(PendingImapCredential {
            plan,
            secret,
            captured_at: Instant::now(),
            _not_sync: PhantomData,
        })
    }
    #[cfg(not(windows))]
    pub fn capture(self) -> Result<PendingImapCredential, SecretError> {
        Err(SecretError::UnsupportedPlatform)
    }
}

impl SecretStore {
    pub fn store_pending_imap_credential(
        &self,
        pending: PendingImapCredential,
        expected_account: &str,
        expected_endpoint: &ImapEndpointConfig,
    ) -> Result<SecretHandle, SecretError> {
        if pending.captured_at.elapsed() >= INPUT_TTL {
            return Err(SecretError::InputExpired);
        }
        if pending.plan.account_id != expected_account
            || pending.plan.endpoint != *expected_endpoint
        {
            return Err(SecretError::CredentialMismatch);
        }
        validate_password(pending.secret.as_slice())?;
        self.store_pending_imap_bytes(
            expected_account,
            expected_endpoint,
            pending.secret.as_slice(),
        )
    }
}

fn validate_password(bytes: &[u8]) -> Result<(), SecretError> {
    if bytes.is_empty() || bytes.len() > MAX_PASSWORD_BYTES {
        return Err(SecretError::InvalidCredential);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| SecretError::InvalidCredential)?;
    if text.chars().any(char::is_control) {
        return Err(SecretError::InvalidCredential);
    }
    Ok(())
}

fn random_nonce() -> Result<[u8; 32], SecretError> {
    #[cfg(windows)]
    {
        native::random_nonce()
    }
    #[cfg(not(windows))]
    {
        Err(SecretError::UnsupportedPlatform)
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod native;

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use hantong_domain::mail_connection::MailConnectionConfig;

    #[cfg(windows)]
    fn endpoint(auth: &str) -> ImapEndpointConfig {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "account_id": "test",
            "imap": {
                "settings_version": 1,
                "host": "imap.example",
                "port": 993,
                "tls": "implicit_tls",
                "login_name": "user",
                "authentication": auth
            }
        }))
        .unwrap();
        MailConnectionConfig::from_json(&bytes)
            .unwrap()
            .imap()
            .clone()
    }

    #[cfg(windows)]
    #[test]
    fn inst05_plan_rejects_oauth_and_zero_bindings_without_window() {
        assert!(matches!(
            ImapPasswordCapturePlan::new("acct", &endpoint("oauth2"), [1; 32], [2; 32]),
            Err(SecretError::InvalidCredential)
        ));
        assert!(matches!(
            ImapPasswordCapturePlan::new("acct", &endpoint("app_password"), [0; 32], [2; 32]),
            Err(SecretError::InvalidCredential)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn inst05_debug_and_metadata_never_include_secret_material() {
        let plan =
            ImapPasswordCapturePlan::new("acct", &endpoint("app_password"), [1; 32], [2; 32])
                .unwrap();
        assert!(!format!("{plan:?}").contains("imap.example"));
    }

    #[test]
    fn inst05_password_bounds_reject_empty_controls_and_oversize_without_normalizing() {
        assert_eq!(validate_password(b""), Err(SecretError::InvalidCredential));
        assert_eq!(
            validate_password(b"a\n"),
            Err(SecretError::InvalidCredential)
        );
        assert_eq!(
            validate_password(&vec![b'a'; MAX_PASSWORD_BYTES + 1]),
            Err(SecretError::InvalidCredential)
        );
        assert_eq!(validate_password(b" exact "), Ok(()));
    }

    #[test]
    fn inst05_plan_and_pending_are_send() {
        fn assert_send<T: Send>() {}
        assert_send::<ImapPasswordCapturePlan>();
        assert_send::<PendingImapCredential>();
    }
}
