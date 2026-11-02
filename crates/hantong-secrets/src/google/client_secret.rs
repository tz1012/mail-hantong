//! Opaque client credential: only native capture can construct a production value.
#[cfg(any(windows, test))]
use super::*;
pub struct GoogleClientSecret {
    // Keep a private field on unsupported hosts so callers cannot construct
    // this opaque credential even though native capture is unavailable there.
    #[cfg_attr(
        not(any(windows, test)),
        expect(dead_code, reason = "native credential capture is Windows-only")
    )]
    client_id: String,
    #[cfg(any(windows, test))]
    secret: SecretText,
}
impl std::fmt::Debug for GoogleClientSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GoogleClientSecret(<redacted>)")
    }
}
impl GoogleClientSecret {
    #[cfg(any(windows, test))]
    pub(crate) fn from_capture(
        client_id: &str,
        secret: SensitiveBuffer,
    ) -> Result<Self, ErrorCode> {
        validate_google_client_id(client_id)?;
        if secret.0.is_empty()
            || secret.0.len() > 1024
            || !secret.0.iter().all(|b| (0x21..=0x7e).contains(b))
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            client_id: client_id.into(),
            secret: SecretText(secret),
        })
    }
    #[cfg(any(windows, test))]
    pub(super) fn matches(&self, client_id: &str) -> bool {
        self.client_id == client_id
    }
    #[cfg(any(windows, test))]
    pub(super) fn value_for(&self, client_id: &str) -> Result<&str, ErrorCode> {
        if !self.matches(client_id) {
            return Err(ErrorCode::ScopeDenied);
        }
        Ok(self.secret.text())
    }
}
#[cfg(any(windows, test))]
pub(super) fn bundle(
    binding: &GoogleAccountBinding,
    refresh: &SecretText,
    scopes: &[String],
    secret: Option<&GoogleClientSecret>,
) -> Result<SecretJson, ErrorCode> {
    binding.validate()?;
    binding.access_mode.validate_granted(scopes)?;
    let mut value = SecretJson(
        serde_json::json!({"format":if binding.access_mode != GoogleAccessMode::ReadOnly {5} else if secret.is_some() {4} else {3},"account_id":binding.account_id,"subject":binding.subject,"client_id":binding.client_id,"credential_version":binding.credential_version,"refresh_token":refresh.text(),"scopes":scopes}),
    );
    if binding.access_mode != GoogleAccessMode::ReadOnly {
        value.0["access_mode"] = binding.access_mode.as_str().into();
    }
    if let Some(secret) = secret {
        value.0["client_secret"] = secret.value_for(&binding.client_id)?.into();
    }
    Ok(value)
}
#[cfg(any(windows, test))]
pub(super) fn from_bundle(
    bundle: &mut SecretJson,
    client_id: &str,
) -> Result<Option<GoogleClientSecret>, ErrorCode> {
    match bundle.0.get("format").and_then(Value::as_u64) {
        Some(3) if bundle.0.get("client_secret").is_none() => Ok(None),
        Some(4 | 5) if bundle.0.get("client_secret").is_none() => Ok(None),
        Some(4 | 5) => {
            let secret = SecretText::from_string(bundle.take_text("client_secret")?)?;
            GoogleClientSecret::from_capture(client_id, secret.0).map(Some)
        }
        _ => Err(ErrorCode::AuthRequired),
    }
}

#[cfg(any(windows, test))]
pub(super) fn bundle_mode(bundle: &SecretJson) -> Result<GoogleAccessMode, ErrorCode> {
    match bundle.0.get("format").and_then(Value::as_u64) {
        Some(3 | 4) if bundle.0.get("access_mode").is_none() => Ok(GoogleAccessMode::ReadOnly),
        Some(5) => GoogleAccessMode::parse(
            bundle
                .0
                .get("access_mode")
                .and_then(Value::as_str)
                .ok_or(ErrorCode::AuthRequired)?,
        )
        .map_err(|_| ErrorCode::AuthRequired),
        _ => Err(ErrorCode::AuthRequired),
    }
}
