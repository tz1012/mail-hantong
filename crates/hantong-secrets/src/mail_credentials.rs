//! Bound credential envelopes. Plaintext is confined to this crate.
use super::*;
use hantong_domain::mail_connection::{
    Authentication, ImapEndpointConfig, SmtpEndpointConfig, TlsMode,
};

const MAGIC: &[u8] = b"mail-hantong.mail-credential.v1\0";
pub(super) struct BoundCredential {
    #[cfg(windows)]
    envelope: SensitiveBuffer,
    #[cfg(windows)]
    offset: usize,
}
impl BoundCredential {
    #[cfg(windows)]
    pub(super) fn text(&self) -> Result<&str, SecretError> {
        std::str::from_utf8(&self.envelope.as_slice()[self.offset..])
            .map_err(|_| SecretError::CredentialMismatch)
    }
}
struct Binding<'a> {
    role: u8,
    version: u64,
    host: &'a str,
    port: u16,
    tls: TlsMode,
    login: &'a str,
    auth: Authentication,
}
impl<'a> From<&'a ImapEndpointConfig> for Binding<'a> {
    fn from(e: &'a ImapEndpointConfig) -> Self {
        Self {
            role: 0,
            version: e.settings_version(),
            host: e.host(),
            port: e.port(),
            tls: e.tls(),
            login: e.login_name(),
            auth: e.authentication(),
        }
    }
}
impl<'a> From<&'a SmtpEndpointConfig> for Binding<'a> {
    fn from(e: &'a SmtpEndpointConfig) -> Self {
        Self {
            role: 1,
            version: e.settings_version(),
            host: e.host(),
            port: e.port(),
            tls: e.tls(),
            login: e.login_name(),
            auth: e.authentication(),
        }
    }
}
impl Binding<'_> {
    fn slot(&self) -> MailCredentialSlot {
        match (self.role, self.auth) {
            (0, Authentication::AppPassword) => MailCredentialSlot::ImapAppPassword,
            (0, Authentication::Oauth2) => MailCredentialSlot::ImapOAuthAccess,
            (_, Authentication::AppPassword) => MailCredentialSlot::SmtpAppPassword,
            (_, Authentication::Oauth2) => MailCredentialSlot::SmtpOAuthAccess,
        }
    }
    fn limit(&self) -> usize {
        match self.auth {
            Authentication::AppPassword => 1024,
            Authentication::Oauth2 => 16384,
        }
    }
    fn valid_credential(&self, bytes: &[u8]) -> bool {
        if bytes.is_empty() || bytes.len() > self.limit() {
            return false;
        }
        match self.auth {
            Authentication::AppPassword => {
                std::str::from_utf8(bytes).is_ok_and(|s| !s.chars().any(char::is_control))
            }
            Authentication::Oauth2 => bytes.iter().all(|b| (0x21..=0x7e).contains(b)),
        }
    }
    fn prefix(&self, account: &str) -> Result<Vec<u8>, SecretError> {
        validate_account(account)?;
        let mut bytes = MAGIC.to_vec();
        let push = |out: &mut Vec<u8>, s: &str| {
            out.extend_from_slice(&(s.len() as u32).to_be_bytes());
            out.extend_from_slice(s.as_bytes());
        };
        push(&mut bytes, account);
        bytes.push(self.role);
        bytes.extend_from_slice(&self.version.to_be_bytes());
        push(&mut bytes, self.host);
        bytes.extend_from_slice(&self.port.to_be_bytes());
        bytes.push(match self.tls {
            TlsMode::ImplicitTls => 0,
            TlsMode::StarttlsRequired => 1,
        });
        push(&mut bytes, self.login);
        bytes.push(match self.auth {
            Authentication::AppPassword => 0,
            Authentication::Oauth2 => 1,
        });
        Ok(bytes)
    }
}
impl SecretStore {
    /// Stores a credential bound to exactly this IMAP endpoint/version.
    pub fn store_imap_credential<R: Read>(
        &self,
        account: &str,
        endpoint: &ImapEndpointConfig,
        reader: R,
    ) -> Result<SecretHandle, SecretError> {
        self.store_bound_mail(account, endpoint.into(), reader)
    }
    /// Stores a credential bound to exactly this SMTP endpoint/version.
    pub fn store_smtp_credential<R: Read>(
        &self,
        account: &str,
        endpoint: &SmtpEndpointConfig,
        reader: R,
    ) -> Result<SecretHandle, SecretError> {
        self.store_bound_mail(account, endpoint.into(), reader)
    }
    /// Local integrity/binding check only; this does not authenticate a server.
    pub fn verify_imap_credential(
        &self,
        account: &str,
        endpoint: &ImapEndpointConfig,
    ) -> Result<(), SecretError> {
        self.verify_bound_mail(account, endpoint.into())
    }
    /// Local integrity/binding check only; this does not authorize sending.
    pub fn verify_smtp_credential(
        &self,
        account: &str,
        endpoint: &SmtpEndpointConfig,
    ) -> Result<(), SecretError> {
        self.verify_bound_mail(account, endpoint.into())
    }

    fn store_bound_mail<R: Read>(
        &self,
        account: &str,
        binding: Binding<'_>,
        reader: R,
    ) -> Result<SecretHandle, SecretError> {
        // Reject an invalid target before consuming any caller-owned reader.
        binding.prefix(account)?;
        let secret = SensitiveBuffer::read_limited(reader, binding.limit())?;
        if !binding.valid_credential(secret.as_slice()) {
            return Err(SecretError::InvalidCredential);
        }
        self.store_bound_mail_bytes(account, binding, secret.as_slice())
    }

    pub(super) fn store_pending_imap_bytes(
        &self,
        account: &str,
        endpoint: &ImapEndpointConfig,
        bytes: &[u8],
    ) -> Result<SecretHandle, SecretError> {
        self.store_bound_mail_bytes(account, endpoint.into(), bytes)
    }

    fn store_bound_mail_bytes(
        &self,
        account: &str,
        binding: Binding<'_>,
        bytes: &[u8],
    ) -> Result<SecretHandle, SecretError> {
        let prefix = binding.prefix(account)?;
        if !binding.valid_credential(bytes) {
            return Err(SecretError::InvalidCredential);
        }
        // Allocate once before copying any secret; all copied plaintext is wiped on Drop.
        let mut envelope = SensitiveBuffer(Vec::with_capacity(prefix.len() + 4 + bytes.len()));
        envelope.0.extend_from_slice(&prefix);
        envelope
            .0
            .extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        envelope.0.extend_from_slice(bytes);
        validate_path(&self.root)?;
        let kind = SecretKind::Mail(binding.slot());
        let handle = self.handle(account, kind);
        self.store_bytes(&handle, account, kind, envelope.as_slice())?;
        Ok(handle)
    }
    fn verify_bound_mail(&self, account: &str, binding: Binding<'_>) -> Result<(), SecretError> {
        self.unseal_bound_mail(account, binding).map(|_| ())
    }
    #[cfg(windows)]
    pub(super) fn unseal_imap_credential(
        &self,
        account: &str,
        endpoint: &ImapEndpointConfig,
    ) -> Result<BoundCredential, SecretError> {
        self.unseal_bound_mail(account, endpoint.into())
    }
    #[cfg(windows)]
    pub(super) fn unseal_smtp_credential(
        &self,
        account: &str,
        endpoint: &SmtpEndpointConfig,
    ) -> Result<BoundCredential, SecretError> {
        self.unseal_bound_mail(account, endpoint.into())
    }
    fn unseal_bound_mail(
        &self,
        account: &str,
        binding: Binding<'_>,
    ) -> Result<BoundCredential, SecretError> {
        let prefix = binding.prefix(account)?;
        let handle = self.resolve_mail_handle(account, binding.slot())?;
        #[cfg(windows)]
        {
            let envelope =
                windows_impl::read_mail_credential(&handle.path, account, binding.slot())?;
            let rest = envelope
                .as_slice()
                .strip_prefix(prefix.as_slice())
                .ok_or(SecretError::CredentialMismatch)?;
            let length = rest.get(..4).ok_or(SecretError::CredentialMismatch)?;
            let length = u32::from_be_bytes(
                length
                    .try_into()
                    .map_err(|_| SecretError::CredentialMismatch)?,
            ) as usize;
            let credential = &rest[4..];
            if credential.len() != length || !binding.valid_credential(credential) {
                return Err(SecretError::CredentialMismatch);
            }
            Ok(BoundCredential {
                envelope,
                offset: prefix.len() + 4,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = (prefix, handle);
            Err(SecretError::UnsupportedPlatform)
        }
    }
}
#[cfg(all(test, windows))]
mod tests;
