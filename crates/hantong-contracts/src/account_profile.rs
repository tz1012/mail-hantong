//! Strict public model-2 account-qualified profile codec.
//!
//! This parser is additive and intentionally does not replace the legacy
//! profile reader.  A schema-2 profile must carry the typed account scope;
//! there is no flattened account/tool fallback.

use crate::{ErrorCode, account_scope::AccountScopeV2, wire_hex};
use serde_json::Value;
use std::fmt;

pub const MAX_PROFILE_BYTES: usize = 32 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountProfileSelector {
    Default,
    Gui,
    Mcp,
    AiCodex,
    AiClaude,
}

impl AccountProfileSelector {
    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        match value {
            "default" => Ok(Self::Default),
            "gui" => Ok(Self::Gui),
            "mcp" => Ok(Self::Mcp),
            "ai-codex" => Ok(Self::AiCodex),
            "ai-claude" => Ok(Self::AiClaude),
            _ => Err(ErrorCode::InvalidInput),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Gui => "gui",
            Self::Mcp => "mcp",
            Self::AiCodex => "ai-codex",
            Self::AiClaude => "ai-claude",
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ParsedAccountClientProfile {
    profile: AccountProfileSelector,
    registration_id: [u8; 16],
    client_key_id: [u8; 32],
    client_public_key: [u8; 72],
    server_public_key: [u8; 72],
    profile_epoch: u64,
    scope: AccountScopeV2,
}

impl fmt::Debug for ParsedAccountClientProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ParsedAccountClientProfile { <redacted> }")
    }
}

impl ParsedAccountClientProfile {
    pub fn from_json(input: &[u8]) -> Result<Self, ErrorCode> {
        if input.len() > MAX_PROFILE_BYTES
            || input
                .iter()
                .find(|&&byte| !matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
                .copied()
                != Some(b'{')
        {
            return Err(ErrorCode::InvalidInput);
        }
        let wire: Value = serde_json::from_slice(input).map_err(|_| ErrorCode::InvalidInput)?;
        let object = wire.as_object().ok_or(ErrorCode::InvalidInput)?;
        if object.len() != 9
            || wire["schema_version"].as_u64() != Some(2)
            || wire["scope_model_version"].as_u64() != Some(2)
        {
            return Err(ErrorCode::InvalidInput);
        }
        let text = |name: &str| {
            object
                .get(name)
                .and_then(Value::as_str)
                .ok_or(ErrorCode::InvalidInput)
        };
        let profile = AccountProfileSelector::parse(text("profile")?)?;
        let scope_bytes =
            serde_json::to_vec(&wire["scope"]).map_err(|_| ErrorCode::InvalidInput)?;
        let scope = AccountScopeV2::from_json(&scope_bytes)?;
        let parsed = Self {
            profile,
            registration_id: wire_hex::decode_fixed(text("registration_id")?)?,
            client_key_id: wire_hex::decode_fixed(text("client_key_id")?)?,
            client_public_key: wire_hex::decode_fixed(text("client_public_key")?)?,
            server_public_key: wire_hex::decode_fixed(text("server_public_key")?)?,
            profile_epoch: wire["profile_epoch"]
                .as_u64()
                .ok_or(ErrorCode::InvalidInput)?,
            scope,
        };
        parsed.validate()?;
        if parsed.to_json()?.as_slice() != input {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(parsed)
    }

    pub fn new(
        profile: AccountProfileSelector,
        registration_id: [u8; 16],
        client_key_id: [u8; 32],
        client_public_key: [u8; 72],
        server_public_key: [u8; 72],
        profile_epoch: u64,
        scope: AccountScopeV2,
    ) -> Result<Self, ErrorCode> {
        let parsed = Self {
            profile,
            registration_id,
            client_key_id,
            client_public_key,
            server_public_key,
            profile_epoch,
            scope,
        };
        parsed.validate()?;
        Ok(parsed)
    }

    fn validate(&self) -> Result<(), ErrorCode> {
        // Other principals keep their existing profile/consent workflow until
        // their account-qualified issuer and runtime are implemented together.
        if self.profile != AccountProfileSelector::Default {
            return Err(ErrorCode::CapabilityUnsupported);
        }
        if self.registration_id == [0; 16]
            || self.client_key_id == [0; 32]
            || self.profile_epoch == 0
            || &self.client_public_key[..8] != b"ECS1\x20\0\0\0"
            || &self.server_public_key[..8] != b"ECS1\x20\0\0\0"
        {
            return Err(ErrorCode::InvalidInput);
        }
        let scope = self.scope.to_json()?;
        if scope.len() > 24 * 1024 {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(())
    }

    pub fn to_json(&self) -> Result<Vec<u8>, ErrorCode> {
        let scope = serde_json::from_slice::<Value>(&self.scope.to_json()?)
            .map_err(|_| ErrorCode::InternalError)?;
        let bytes = format!(
            "{{\"schema_version\":2,\"profile\":\"{}\",\"registration_id\":\"{}\",\"client_key_id\":\"{}\",\"client_public_key\":\"{}\",\"server_public_key\":\"{}\",\"profile_epoch\":{},\"scope_model_version\":2,\"scope\":{}}}",
            self.profile.as_str(), wire_hex::encode(&self.registration_id),
            wire_hex::encode(&self.client_key_id),wire_hex::encode(&self.client_public_key),
            wire_hex::encode(&self.server_public_key),self.profile_epoch,scope
        ).into_bytes();
        if bytes.len() > MAX_PROFILE_BYTES {
            return Err(ErrorCode::InternalError);
        }
        Ok(bytes)
    }

    pub fn profile(&self) -> AccountProfileSelector {
        self.profile
    }
    pub fn registration_id(&self) -> &[u8; 16] {
        &self.registration_id
    }
    pub fn client_key_id(&self) -> &[u8; 32] {
        &self.client_key_id
    }
    pub fn client_public_key(&self) -> &[u8; 72] {
        &self.client_public_key
    }
    pub fn server_public_key(&self) -> &[u8; 72] {
        &self.server_public_key
    }
    pub fn profile_epoch(&self) -> u64 {
        self.profile_epoch
    }
    pub fn scope(&self) -> &AccountScopeV2 {
        &self.scope
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_scope::{AccountGrantV2, AccountScopeV2};

    fn scope() -> AccountScopeV2 {
        AccountScopeV2::new(
            vec!["mail.accounts.list".into()],
            vec![
                AccountGrantV2::new(
                    "acct".into(),
                    vec!["mail.messages.get".into(), "mail.send.execute".into()],
                    false,
                )
                .unwrap(),
            ],
        )
        .unwrap()
    }

    fn profile() -> ParsedAccountClientProfile {
        let mut client = [3; 72];
        let mut server = [4; 72];
        client[..8].copy_from_slice(b"ECS1\x20\0\0\0");
        server[..8].copy_from_slice(b"ECS1\x20\0\0\0");
        ParsedAccountClientProfile::new(
            AccountProfileSelector::Default,
            [1; 16],
            [2; 32],
            client,
            server,
            1,
            scope(),
        )
        .unwrap()
    }

    #[test]
    fn sec03_model2_profile_rejects_other_principal_selectors() {
        for selector in [
            AccountProfileSelector::Gui,
            AccountProfileSelector::Mcp,
            AccountProfileSelector::AiCodex,
            AccountProfileSelector::AiClaude,
        ] {
            let mut value = profile();
            value.profile = selector;
            assert_eq!(value.validate(), Err(ErrorCode::CapabilityUnsupported));
            let encoded = String::from_utf8(profile().to_json().unwrap())
                .unwrap()
                .replacen("\"default\"", &format!("\"{}\"", selector.as_str()), 1)
                .into_bytes();
            assert_eq!(
                ParsedAccountClientProfile::from_json(&encoded),
                Err(ErrorCode::CapabilityUnsupported)
            );
        }
    }

    #[test]
    fn sec03_model2_profile_roundtrips_and_redacts() {
        let profile = profile();
        let bytes = profile.to_json().unwrap();
        assert_eq!(
            ParsedAccountClientProfile::from_json(&bytes).unwrap(),
            profile
        );
        assert_eq!(
            format!("{profile:?}"),
            "ParsedAccountClientProfile { <redacted> }"
        );
    }

    #[test]
    fn sec03_model2_profile_rejects_unknown_duplicate_and_legacy_schema() {
        let bytes = profile().to_json().unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let raw = serde_json::to_string(&value).unwrap();
        assert!(
            ParsedAccountClientProfile::from_json(
                format!("{{\"unknown\":1,{}", &raw[1..]).as_bytes()
            )
            .is_err()
        );
        assert!(
            ParsedAccountClientProfile::from_json(
                format!("{{\"profile\":\"default\",{}", &raw[1..]).as_bytes()
            )
            .is_err()
        );
        let mut legacy = value;
        legacy["schema_version"] = Value::from(1);
        assert!(
            ParsedAccountClientProfile::from_json(&serde_json::to_vec(&legacy).unwrap()).is_err()
        );
    }
}
