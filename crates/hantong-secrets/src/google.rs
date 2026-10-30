//! OAuth and read-only Gmail boundary. No token-bearing value is public.
use crate::SecretStore;
#[cfg(any(windows, test))]
use crate::{SensitiveBuffer, wipe};
use hantong_contracts::{ErrorCode, google::*};
#[cfg(any(windows, test))]
use serde_json::Value;
use std::sync::atomic::AtomicBool;
#[cfg(any(windows, test))]
use std::sync::atomic::Ordering;
#[cfg(any(windows, test))]
use std::time::Duration;
#[cfg(any(windows, test))]
use std::time::Instant;

#[cfg(any(windows, test))]
mod crypto;
#[cfg(any(windows, test))]
mod gmail;
#[cfg(any(windows, test))]
mod http;
mod oauth;
pub use oauth::{GoogleAuthorization, begin_google_authorization};

#[cfg(any(windows, test))]
struct SecretText(SensitiveBuffer);
#[cfg(any(windows, test))]
impl SecretText {
    fn from_string(value: String) -> Result<Self, ErrorCode> {
        let value = Self(SensitiveBuffer(value.into_bytes()));
        if value.0.0.is_empty()
            || value.0.0.len() > 16384
            || !value.0.0.iter().all(|b| (0x21..=0x7e).contains(b))
        {
            return Err(ErrorCode::AuthRequired);
        }
        Ok(value)
    }
    fn text(&self) -> &str {
        std::str::from_utf8(self.0.as_slice()).unwrap_or("")
    }
}
#[cfg(any(windows, test))]
struct SecretJson(Value);
#[cfg(any(windows, test))]
impl SecretJson {
    fn parse(bytes: &[u8]) -> Result<Self, ErrorCode> {
        serde_json::from_slice(bytes)
            .map(Self)
            .map_err(|_| ErrorCode::AuthRequired)
    }
    fn take_text(&mut self, key: &str) -> Result<String, ErrorCode> {
        let mut value = SecretJson(
            self.0
                .as_object_mut()
                .and_then(|o| o.remove(key))
                .ok_or(ErrorCode::AuthRequired)?,
        );
        match &mut value.0 {
            Value::String(s) => Ok(std::mem::take(s)),
            _ => Err(ErrorCode::AuthRequired),
        }
    }
}
#[cfg(any(windows, test))]
impl Drop for SecretJson {
    fn drop(&mut self) {
        fn clear(v: &mut Value) {
            match v {
                Value::String(s) => {
                    let old = std::mem::take(s);
                    let mut bytes = old.into_bytes();
                    wipe(&mut bytes);
                }
                Value::Array(a) => a.iter_mut().for_each(clear),
                Value::Object(o) => o.values_mut().for_each(clear),
                _ => (),
            }
        }
        clear(&mut self.0);
    }
}

pub struct PendingGoogleCredential {
    identity: GoogleIdentity,
    client_id: String,
    #[cfg(any(windows, test))]
    refresh: SecretText,
}
impl std::fmt::Debug for PendingGoogleCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PendingGoogleCredential(<redacted>)")
    }
}
impl PendingGoogleCredential {
    pub fn identity(&self) -> &GoogleIdentity {
        &self.identity
    }
    pub fn binding(&self, credential_version: u64) -> Result<GoogleAccountBinding, ErrorCode> {
        let binding = GoogleAccountBinding {
            account_id: self.identity.account_id.clone(),
            subject: self.identity.subject.clone(),
            client_id: self.client_id.clone(),
            credential_version,
        };
        binding.validate()?;
        Ok(binding)
    }
    pub fn store(
        self,
        store: &SecretStore,
        binding: &GoogleAccountBinding,
    ) -> Result<(), ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = (self, store, binding);
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            if self.binding(binding.credential_version)? != *binding {
                return Err(ErrorCode::AuthRequired);
            }
            store.write_google_bundle(binding, &self.refresh, &self.identity.granted_scopes)
        }
    }
}

#[cfg(any(windows, test))]
fn normalize_scopes(scope: &str) -> Result<Vec<String>, ErrorCode> {
    if scope.len() > 8192 {
        return Err(ErrorCode::ScopeDenied);
    }
    let mut scopes = Vec::new();
    for part in scope.split_ascii_whitespace() {
        let part = if part == "https://www.googleapis.com/auth/userinfo.email" {
            "email"
        } else {
            part
        };
        if !scopes.iter().any(|s| s == part) {
            scopes.push(part.to_string());
        }
    }
    if !GOOGLE_SCOPES.iter().all(|s| scopes.iter().any(|g| g == s)) || scopes.len() > 32 {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(scopes)
}
#[cfg(any(windows, test))]
struct TokenResponse {
    access: SecretText,
    refresh: Option<SecretText>,
    scopes: Vec<String>,
}
#[cfg(any(windows, test))]
fn parse_tokens(
    response: http::Response,
    fallback_scope: Option<&[String]>,
) -> Result<TokenResponse, ErrorCode> {
    http::success(response.status)?;
    let mut json = SecretJson::parse(response.body.as_slice())?;
    if json
        .0
        .get("token_type")
        .and_then(Value::as_str)
        .is_none_or(|t| !t.eq_ignore_ascii_case("bearer"))
    {
        return Err(ErrorCode::AuthRequired);
    }
    let scopes = match json.0.get("scope").and_then(Value::as_str) {
        Some(scope) => normalize_scopes(scope)?,
        None => fallback_scope
            .map(|s| s.to_vec())
            .ok_or(ErrorCode::ScopeDenied)?,
    };
    let access = SecretText::from_string(json.take_text("access_token")?)?;
    let refresh = if json.0.get("refresh_token").is_some() {
        Some(SecretText::from_string(json.take_text("refresh_token")?)?)
    } else {
        None
    };
    Ok(TokenResponse {
        access,
        refresh,
        scopes,
    })
}
#[cfg(any(windows, test))]
fn identity(
    transport: &impl http::Transport,
    access: &SecretText,
    scopes: Vec<String>,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<GoogleIdentity, ErrorCode> {
    let response = transport.request(http::Request {
        host: http::Host::Identity,
        path: "/v1/userinfo",
        post: None,
        bearer: Some(access.text()),
        limit: 16384,
        deadline,
        cancel,
    })?;
    http::success(response.status)?;
    let json = SecretJson::parse(response.body.as_slice())?;
    let subject = json
        .0
        .get("sub")
        .and_then(Value::as_str)
        .ok_or(ErrorCode::AuthRequired)?;
    if json.0.get("email_verified").and_then(Value::as_bool) != Some(true) {
        return Err(ErrorCode::AuthRequired);
    }
    let identity = GoogleIdentity {
        account_id: account_id(subject)?,
        subject: subject.into(),
        email: json
            .0
            .get("email")
            .and_then(Value::as_str)
            .ok_or(ErrorCode::AuthRequired)?
            .into(),
        granted_scopes: scopes,
    };
    identity.validate().map_err(|_| ErrorCode::AuthRequired)?;
    Ok(identity)
}
#[cfg(any(windows, test))]
fn account_id(subject: &str) -> Result<String, ErrorCode> {
    Ok(format!(
        "google_{}",
        hantong_contracts::wire_hex::encode(&crypto::sha256(subject.as_bytes())?)
    ))
}

#[cfg(any(windows, test))]
fn refresh_verified(
    transport: &impl http::Transport,
    binding: &GoogleAccountBinding,
    refresh: &SecretText,
    scopes: &[String],
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<TokenResponse, ErrorCode> {
    let form = oauth::form(&[
        ("client_id", &binding.client_id),
        ("refresh_token", refresh.text()),
        ("grant_type", "refresh_token"),
    ]);
    let tokens = parse_tokens(
        transport.request(http::Request {
            host: http::Host::Token,
            path: "/token",
            post: Some(form.as_slice()),
            bearer: None,
            limit: 65536,
            deadline,
            cancel,
        })?,
        Some(scopes),
    )?;
    let verified = identity(
        transport,
        &tokens.access,
        tokens.scopes.clone(),
        deadline,
        cancel,
    )?;
    if verified.subject != binding.subject || verified.account_id != binding.account_id {
        return Err(ErrorCode::AuthRequired);
    }
    Ok(tokens)
}

impl SecretStore {
    #[cfg(windows)]
    fn write_google_bundle(
        &self,
        binding: &GoogleAccountBinding,
        refresh: &SecretText,
        scopes: &[String],
    ) -> Result<(), ErrorCode> {
        binding.validate()?;
        if account_id(&binding.subject)? != binding.account_id {
            return Err(ErrorCode::AuthRequired);
        }
        let json = SecretJson(
            serde_json::json!({"format":3,"account_id":binding.account_id,"subject":binding.subject,"client_id":binding.client_id,"credential_version":binding.credential_version,"refresh_token":refresh.text(),"scopes":scopes}),
        );
        let mut bytes = SensitiveBuffer(Vec::with_capacity(crate::MAX_SECRET));
        serde_json::to_writer(&mut bytes.0, &json.0).map_err(|_| ErrorCode::InternalError)?;
        if bytes.0.len() > crate::MAX_SECRET {
            return Err(ErrorCode::QuotaExceeded);
        }
        crate::validate_path(&self.root).map_err(|_| ErrorCode::InternalError)?;
        let kind = crate::SecretKind::GoogleOAuth(binding.credential_version);
        let handle = self.handle(&binding.account_id, kind);
        self.store_bytes(&handle, &binding.account_id, kind, bytes.as_slice())
            .map_err(|_| ErrorCode::InternalError)?;
        Ok(())
    }
    pub fn fetch_google_page(
        &self,
        binding: &GoogleAccountBinding,
        request: &GoogleSyncRequest,
        cancel: &AtomicBool,
    ) -> Result<GooglePage, ErrorCode> {
        binding.validate()?;
        request.validate()?;
        if request.account_id != binding.account_id {
            return Err(ErrorCode::AuthRequired);
        }
        #[cfg(windows)]
        {
            if account_id(&binding.subject)? != binding.account_id {
                return Err(ErrorCode::AuthRequired);
            }
            self.fetch_google_page_windows(binding, request, cancel)
        }
        #[cfg(not(windows))]
        {
            let _ = cancel;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    #[cfg(windows)]
    fn fetch_google_page_windows(
        &self,
        binding: &GoogleAccountBinding,
        request: &GoogleSyncRequest,
        cancel: &AtomicBool,
    ) -> Result<GooglePage, ErrorCode> {
        use std::collections::BTreeMap;
        use std::sync::{Arc, Mutex, OnceLock};
        static LOCKS: OnceLock<Mutex<BTreeMap<String, std::sync::Weak<Mutex<()>>>>> =
            OnceLock::new();
        let lock = {
            let mut locks = LOCKS
                .get_or_init(Default::default)
                .lock()
                .map_err(|_| ErrorCode::InternalError)?;
            locks.retain(|_, v| v.strong_count() > 0);
            if let Some(l) = locks
                .get(&binding.account_id)
                .and_then(std::sync::Weak::upgrade)
            {
                l
            } else {
                let l = Arc::new(Mutex::new(()));
                locks.insert(binding.account_id.clone(), Arc::downgrade(&l));
                l
            }
        };
        let deadline = Instant::now() + Duration::from_secs(90);
        let _held = loop {
            http::check(deadline, cancel)?;
            match lock.try_lock() {
                Ok(guard) => break guard,
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(_) => return Err(ErrorCode::InternalError),
            }
        };
        crate::validate_path(&self.root).map_err(|_| ErrorCode::AuthRequired)?;
        let handle = self.handle(
            &binding.account_id,
            crate::SecretKind::GoogleOAuth(binding.credential_version),
        );
        let bytes = crate::windows_impl::read_google_credential(
            &handle.path,
            &binding.account_id,
            binding.credential_version,
        )
        .map_err(|_| ErrorCode::AuthRequired)?;
        let mut bundle = SecretJson::parse(bytes.as_slice())?;
        if bundle.0.get("format").and_then(Value::as_u64) != Some(3)
            || bundle.0.get("account_id").and_then(Value::as_str) != Some(&binding.account_id)
            || bundle.0.get("subject").and_then(Value::as_str) != Some(&binding.subject)
            || bundle.0.get("client_id").and_then(Value::as_str) != Some(&binding.client_id)
            || bundle.0.get("credential_version").and_then(Value::as_u64)
                != Some(binding.credential_version)
        {
            return Err(ErrorCode::AuthRequired);
        }
        let scopes = bundle
            .0
            .get("scopes")
            .and_then(Value::as_array)
            .ok_or(ErrorCode::ScopeDenied)?
            .iter()
            .map(|v| v.as_str().ok_or(ErrorCode::ScopeDenied))
            .collect::<Result<Vec<_>, _>>()?
            .join(" ");
        let scopes = normalize_scopes(&scopes)?;
        let refresh = SecretText::from_string(bundle.take_text("refresh_token")?)?;
        let transport = http::NativeTransport;
        let tokens = refresh_verified(&transport, binding, &refresh, &scopes, deadline, cancel)?;
        if let Some(rotated) = &tokens.refresh {
            self.write_google_bundle(binding, rotated, &tokens.scopes)?;
        }
        gmail::fetch(
            &transport,
            binding,
            request,
            &tokens.access,
            deadline,
            cancel,
        )
    }
}

#[cfg(test)]
mod tests;
