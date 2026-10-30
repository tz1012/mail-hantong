//! Parsed public profile data, never evidence of trust or human authorization.
use hantong_contracts::{
    ErrorCode,
    auth::{RequestedScope, default_cli_legacy_tools, is_supported_default_cli_toolset},
    wire_hex,
};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const MAX_PROFILE_BYTES: usize = 32 * 1024;
const PUBLIC_KEY_HEADER: &[u8; 8] = b"ECS1\x20\0\0\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileSelector {
    Default,
}
impl ProfileSelector {
    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        match value {
            "default" => Ok(Self::Default),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
    pub fn as_str(self) -> &'static str {
        "default"
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ParsedClientProfile {
    registration_id: [u8; 16],
    client_key_id: [u8; 32],
    client_public_key: [u8; 72],
    server_public_key: [u8; 72],
    profile_epoch: u64,
    scope: RequestedScope,
}
// The wire value is deliberately private and does not derive Debug. Duplicate
// fields are rejected by struct deserialization, including escaped field names.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireProfile {
    schema_version: u32,
    profile: String,
    registration_id: String,
    client_key_id: String,
    client_public_key: String,
    server_public_key: String,
    profile_epoch: u64,
    account_ids: Vec<String>,
    tool_ids: Vec<String>,
    read_only: bool,
    requests_per_minute: u32,
}
impl fmt::Debug for ParsedClientProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ParsedClientProfile { <redacted> }")
    }
}
impl ParsedClientProfile {
    /// Validates public data only. This is not a registration or trust operation.
    pub fn new(
        registration_id: [u8; 16],
        client_key_id: [u8; 32],
        client_public_key: [u8; 72],
        server_public_key: [u8; 72],
        profile_epoch: u64,
        account_ids: Vec<String>,
    ) -> Result<Self, ErrorCode> {
        Self::new_with_scope(
            registration_id,
            client_key_id,
            client_public_key,
            server_public_key,
            profile_epoch,
            RequestedScope::new(account_ids, default_cli_legacy_tools())?,
        )
    }
    pub fn new_with_scope(
        registration_id: [u8; 16],
        client_key_id: [u8; 32],
        client_public_key: [u8; 72],
        server_public_key: [u8; 72],
        profile_epoch: u64,
        scope: RequestedScope,
    ) -> Result<Self, ErrorCode> {
        if registration_id == [0; 16]
            || client_key_id == [0; 32]
            || !(1..=i64::MAX as u64).contains(&profile_epoch)
            || &client_public_key[..8] != PUBLIC_KEY_HEADER
            || &server_public_key[..8] != PUBLIC_KEY_HEADER
        {
            return Err(ErrorCode::InvalidInput);
        }
        if !is_supported_default_cli_toolset(scope.tools()) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            registration_id,
            client_key_id,
            client_public_key,
            server_public_key,
            profile_epoch,
            scope,
        })
    }
    /// Parses bounded, untrusted bytes. CNG point validation and trusted file
    /// loading are separate requirements before attempting authentication.
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
        let value: WireProfile =
            serde_json::from_slice(input).map_err(|_| ErrorCode::InvalidInput)?;
        if value.schema_version != 1
            || ProfileSelector::parse(&value.profile).is_err()
            || !value.read_only
            || value.requests_per_minute != 60
            || !is_supported_default_cli_toolset(&value.tool_ids)
        {
            return Err(ErrorCode::InvalidInput);
        }
        Self::new_with_scope(
            wire_hex::decode_fixed(&value.registration_id)?,
            wire_hex::decode_fixed(&value.client_key_id)?,
            wire_hex::decode_fixed(&value.client_public_key)?,
            wire_hex::decode_fixed(&value.server_public_key)?,
            value.profile_epoch,
            RequestedScope::new(value.account_ids, value.tool_ids)?,
        )
    }
    /// Deterministic public-data encoding; publishing it is a separate operation.
    pub fn to_json(&self) -> Result<Vec<u8>, ErrorCode> {
        let value = WireProfile {
            schema_version: 1,
            profile: ProfileSelector::Default.as_str().to_owned(),
            registration_id: wire_hex::encode(&self.registration_id),
            client_key_id: wire_hex::encode(&self.client_key_id),
            client_public_key: wire_hex::encode(&self.client_public_key),
            server_public_key: wire_hex::encode(&self.server_public_key),
            profile_epoch: self.profile_epoch,
            account_ids: self.scope.accounts().to_vec(),
            tool_ids: self.scope.tools().to_vec(),
            read_only: true,
            requests_per_minute: 60,
        };
        let bytes = serde_json::to_vec(&value).map_err(|_| ErrorCode::InternalError)?;
        if bytes.len() > MAX_PROFILE_BYTES {
            return Err(ErrorCode::InternalError);
        }
        Ok(bytes)
    }
    pub fn profile(&self) -> ProfileSelector {
        ProfileSelector::Default
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
    pub fn scope(&self) -> &RequestedScope {
        &self.scope
    }
    pub fn read_only(&self) -> bool {
        true
    }
    pub fn requests_per_minute(&self) -> u32 {
        60
    }
    pub fn client_key_name(&self) -> String {
        format!(
            "mail-hantong-client-{}",
            wire_hex::encode(&self.client_key_id)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::wire_hex;
    use serde_json::{Value, json};

    // Synthetic public blob only; parsing does not validate an elliptic-curve point.
    fn public_key(fill: u8) -> [u8; 72] {
        let mut key = [fill; 72];
        key[..8].copy_from_slice(b"ECS1\x20\0\0\0");
        key
    }
    fn value() -> Value {
        json!({
            "schema_version":1, "profile":"default",
            "registration_id":wire_hex::encode(&[1;16]),
            "client_key_id":wire_hex::encode(&[2;32]),
            "client_public_key":wire_hex::encode(&public_key(3)),
            "server_public_key":wire_hex::encode(&public_key(4)),
            "profile_epoch":1, "account_ids":["b", "a"],
            "tool_ids":["mail.accounts.list"], "read_only":true, "requests_per_minute":60
        })
    }
    fn parse(value: &Value) -> Result<ParsedClientProfile, ErrorCode> {
        ParsedClientProfile::from_json(&serde_json::to_vec(value).unwrap())
    }
    fn invalid(value: &Value) {
        assert_eq!(parse(value), Err(ErrorCode::InvalidInput));
    }

    #[test]
    fn cli07_profile_roundtrip_preserves_identity_and_canonical_scope() {
        let profile = parse(&value()).unwrap();
        assert_eq!(profile.profile(), ProfileSelector::Default);
        assert_eq!(profile.registration_id(), &[1; 16]);
        assert_eq!(profile.client_key_id(), &[2; 32]);
        assert_eq!(profile.client_public_key(), &public_key(3));
        assert_eq!(profile.server_public_key(), &public_key(4));
        assert_eq!(profile.profile_epoch(), 1);
        assert_eq!(profile.scope().accounts(), &["a", "b"]);
        assert_eq!(profile.scope().tools(), &["mail.accounts.list"]);
        assert!(profile.read_only());
        assert_eq!(profile.requests_per_minute(), 60);
        assert_eq!(
            profile.client_key_name(),
            format!("mail-hantong-client-{}", "02".repeat(32))
        );
        let bytes = profile.to_json().unwrap();
        let again = ParsedClientProfile::from_json(&bytes).unwrap();
        assert_eq!(profile, again);
        assert_eq!(bytes, again.to_json().unwrap());
    }
    #[test]
    fn cli15_uid_scope_profile_roundtrip_keeps_legacy_and_folder_permissions_exact() {
        for tools in [
            hantong_contracts::auth::default_cli_legacy_tools(),
            hantong_contracts::auth::default_cli_folder_tools(),
        ] {
            let mut input = value();
            input["tool_ids"] = json!(tools);
            let profile = parse(&input).unwrap();
            let bytes = profile.to_json().unwrap();
            assert_eq!(
                ParsedClientProfile::from_json(&bytes)
                    .unwrap()
                    .to_json()
                    .unwrap(),
                bytes
            );
            assert!(
                !profile
                    .scope()
                    .tools()
                    .iter()
                    .any(|tool| tool == "mail.sync.page")
            );
        }
        let mut input = value();
        input["tool_ids"] = json!([
            "mail.accounts.list",
            "mail.folders.list",
            "mail.folders.refresh",
            "mail.sync.page"
        ]);
        let profile = parse(&input).unwrap();
        let bytes = profile.to_json().unwrap();
        assert_eq!(ParsedClientProfile::from_json(&bytes).unwrap(), profile);
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["schema_version"],
            1
        );
        input["tool_ids"] = json!(["mail.accounts.list", "mail.sync.page"]);
        invalid(&input);
    }

    #[test]
    fn cli15_profile_roundtrip_accepts_only_canonical_folder_scope() {
        let mut input = value();
        input["tool_ids"] = json!([
            "mail.accounts.list",
            "mail.folders.list",
            "mail.folders.refresh"
        ]);
        let profile = parse(&input).unwrap();
        assert_eq!(
            profile.scope().tools(),
            &[
                "mail.accounts.list",
                "mail.folders.list",
                "mail.folders.refresh"
            ]
        );
        let mut partial = input;
        partial["tool_ids"] = json!(["mail.accounts.list", "mail.folders.list"]);
        invalid(&partial);
    }
    #[test]
    fn acc03_profile_empty_scope_means_no_accounts() {
        let mut input = value();
        input["account_ids"] = json!([]);
        let profile = parse(&input).unwrap();
        assert!(profile.scope().accounts().is_empty());
        assert!(!profile.scope().permits_account("a", "mail.accounts.list"));
        assert_eq!(
            parse(&serde_json::from_slice(&profile.to_json().unwrap()).unwrap()).unwrap(),
            profile
        );
    }
    #[test]
    fn cli07_profile_constructor_validates_ids_epoch_header_and_scope() {
        let create = |id, key_id, client, server, epoch, accounts| {
            ParsedClientProfile::new(id, key_id, client, server, epoch, accounts)
        };
        assert!(
            create(
                [1; 16],
                [2; 32],
                public_key(3),
                public_key(4),
                i64::MAX as u64,
                vec![]
            )
            .is_ok()
        );
        for (id, key_id, epoch) in [
            ([0; 16], [2; 32], 1),
            ([1; 16], [0; 32], 1),
            ([1; 16], [2; 32], 0),
            ([1; 16], [2; 32], i64::MAX as u64 + 1),
        ] {
            assert_eq!(
                create(id, key_id, public_key(3), public_key(4), epoch, vec![]),
                Err(ErrorCode::InvalidInput)
            );
        }
        for index in 0..8 {
            let mut malformed = public_key(3);
            malformed[index] ^= 1;
            assert!(create([1; 16], [2; 32], malformed, public_key(4), 1, vec![]).is_err());
            assert!(create([1; 16], [2; 32], public_key(3), malformed, 1, vec![]).is_err());
        }
        assert!(
            create(
                [1; 16],
                [2; 32],
                public_key(3),
                public_key(4),
                1,
                vec!["a".into(), "a".into()]
            )
            .is_err()
        );
    }
    #[test]
    fn cli07_profile_all_fields_are_required_and_non_null() {
        for key in value().as_object().unwrap().keys() {
            let mut absent = value();
            absent.as_object_mut().unwrap().remove(key);
            invalid(&absent);
            let mut null = value();
            null[key] = Value::Null;
            invalid(&null);
        }
    }
    #[test]
    fn cli07_profile_fixed_values_and_numeric_types_cannot_expand_authority() {
        for (key, bad) in [
            ("schema_version", json!(2)),
            ("schema_version", json!(1.0)),
            ("schema_version", json!("1")),
            ("profile", json!("other")),
            ("profile_epoch", json!(0)),
            ("profile_epoch", json!(-1)),
            ("profile_epoch", json!(1.0)),
            ("profile_epoch", json!(u64::MAX)),
            ("read_only", json!(false)),
            ("read_only", json!(1)),
            ("requests_per_minute", json!(600)),
            ("requests_per_minute", json!(60.0)),
            ("tool_ids", json!([])),
            ("tool_ids", json!(["mail.send.execute"])),
            (
                "tool_ids",
                json!(["mail.accounts.list", "mail.accounts.list"]),
            ),
        ] {
            let mut input = value();
            input[key] = bad;
            invalid(&input);
        }
    }
    #[test]
    fn cli07_profile_rejects_unknown_duplicate_and_escaped_duplicate_fields() {
        let base = value();
        let raw = serde_json::to_string(&base).unwrap();
        for (key, original) in base.as_object().unwrap() {
            let duplicate = format!(
                "{{{}:{},{}",
                serde_json::to_string(key).unwrap(),
                original,
                &raw[1..]
            );
            assert_eq!(
                ParsedClientProfile::from_json(duplicate.as_bytes()),
                Err(ErrorCode::InvalidInput)
            );
        }
        let escaped = format!("{{\"profi\\u006ce\":\"default\",{}", &raw[1..]);
        assert!(ParsedClientProfile::from_json(escaped.as_bytes()).is_err());
        for key in ["approved", "verified", "server_pin", "secret", "path"] {
            let mut input = value();
            input[key] = json!("untrusted-marker");
            invalid(&input);
        }
    }
    #[test]
    fn acc03_profile_rejects_unbounded_or_invalid_account_scopes() {
        for accounts in [
            json!(["a", "a"]),
            json!([""]),
            json!(["a/b"]),
            json!(["한글"]),
            json!(["a\nb"]),
            json!(["x".repeat(129)]),
            json!((0..129).map(|i| format!("a{i}")).collect::<Vec<_>>()),
            json!("all"),
        ] {
            let mut input = value();
            input["account_ids"] = accounts;
            invalid(&input);
        }
        let mut max = value();
        max["account_ids"] = json!(
            (0..128)
                .map(|i| format!("{i:03}{}", "x".repeat(125)))
                .collect::<Vec<_>>()
        );
        let profile = parse(&max).unwrap();
        assert_eq!(profile.scope().accounts().len(), 128);
        assert!(profile.to_json().unwrap().len() < MAX_PROFILE_BYTES);
    }
    #[test]
    fn cli07_profile_binary_fields_require_exact_lowercase_hex_and_public_headers() {
        for key in [
            "registration_id",
            "client_key_id",
            "client_public_key",
            "server_public_key",
        ] {
            for bad in ["", "0", "gg", "AB"].map(str::to_owned) {
                let mut input = value();
                input[key] = json!(bad);
                invalid(&input);
            }
            let mut input = value();
            input[key] = json!(format!("{}00", input[key].as_str().unwrap()));
            invalid(&input);
        }
        for key in ["registration_id", "client_key_id"] {
            let mut input = value();
            input[key] = json!("0".repeat(input[key].as_str().unwrap().len()));
            invalid(&input);
        }
        let mut wrong = public_key(3);
        wrong[4] = 64;
        let mut input = value();
        input["client_public_key"] = json!(wire_hex::encode(&wrong));
        invalid(&input);
    }
    #[test]
    fn cli07_profile_rejects_bad_json_and_enforces_total_input_limit() {
        let raw = serde_json::to_vec(&value()).unwrap();
        for suffix in [b"{}".as_slice(), b"null", b"\0"] {
            let mut trailing = raw.clone();
            trailing.extend_from_slice(suffix);
            assert!(ParsedClientProfile::from_json(&trailing).is_err());
        }
        for bad in [
            b"".as_slice(),
            b"[]",
            b"null",
            b"{}",
            b"\xff",
            b"{",
            b"true",
        ] {
            assert!(ParsedClientProfile::from_json(bad).is_err());
        }
        let mut bom = b"\xef\xbb\xbf".to_vec();
        bom.extend_from_slice(&raw);
        assert!(ParsedClientProfile::from_json(&bom).is_err());
        let mut exact = raw;
        exact.resize(MAX_PROFILE_BYTES, b' ');
        assert!(ParsedClientProfile::from_json(&exact).is_ok());
        exact.push(b' ');
        assert!(ParsedClientProfile::from_json(&exact).is_err());
    }
    #[test]
    fn cli07_profile_rejects_positional_array_representation() {
        let fields = value();
        let array = json!([
            fields["schema_version"],
            fields["profile"],
            fields["registration_id"],
            fields["client_key_id"],
            fields["client_public_key"],
            fields["server_public_key"],
            fields["profile_epoch"],
            fields["account_ids"],
            fields["tool_ids"],
            fields["read_only"],
            fields["requests_per_minute"]
        ]);
        invalid(&array);
    }
    #[test]
    fn cli07_profile_selector_never_accepts_paths_or_aliases() {
        assert_eq!(
            ProfileSelector::parse("default"),
            Ok(ProfileSelector::Default)
        );
        assert_eq!(ProfileSelector::Default.as_str(), "default");
        for value in [
            "",
            "Default",
            "default.json",
            "./default",
            "../default",
            "D:/data/default.json",
            "default ",
            "default\0",
        ] {
            assert_eq!(ProfileSelector::parse(value), Err(ErrorCode::InvalidInput));
        }
    }
    #[test]
    fn sec08_profile_debug_and_errors_hide_untrusted_data() {
        let profile = parse(&value()).unwrap();
        assert_eq!(format!("{profile:?}"), "ParsedClientProfile { <redacted> }");
        let mut input = value();
        input["profile"] = json!("private-marker");
        let error = parse(&input).unwrap_err();
        assert_eq!(error, ErrorCode::InvalidInput);
        assert!(!format!("{error:?}").contains("private-marker"));
    }
}
