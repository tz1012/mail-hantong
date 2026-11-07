//! Strict, non-secret account connection RPC messages.

use crate::{ErrorCode, wire_hex};
use serde_json::{Map, Value, json};
use std::fmt;

pub const MAX_FRAME_BYTES: usize = 4096;
pub const PREPARE_METHOD: &str = "app.accounts.connect.prepare";
pub const STATUS_METHOD: &str = "app.accounts.connect.status";
pub const COMPLETE_METHOD: &str = "app.accounts.connect.complete";
pub const CANCEL_METHOD: &str = "app.accounts.connect.cancel";
pub const BIND_SMTP_METHOD: &str = "app.accounts.connect.bind_smtp";
const MAX_REQUEST_ID: usize = 64;
const MAX_ACCOUNT_ID: usize = 128;
const MAX_LOGIN_URL: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountConnectProvider {
    Google,
    ImapSmtp,
}
impl AccountConnectProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::ImapSmtp => "imap_smtp",
        }
    }
    fn parse(value: &str) -> Result<Self, ErrorCode> {
        match value {
            "google" => Ok(Self::Google),
            "imap_smtp" => Ok(Self::ImapSmtp),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum AccountConnectSelection {
    New,
    Existing(String),
    ImapSetup([u8; 32]),
}
impl fmt::Debug for AccountConnectSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::New => f.write_str("New"),
            Self::Existing(_) => f.write_str("Existing(<redacted>)"),
            Self::ImapSetup(_) => f.write_str("ImapSetup(<redacted>)"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountConnectCommand {
    Prepare {
        provider: AccountConnectProvider,
        selection: AccountConnectSelection,
    },
    Status([u8; 32]),
    Complete([u8; 32]),
    Cancel([u8; 32]),
    BindSmtp {
        intent_id: [u8; 32],
        request_id: [u8; 32],
    },
}

#[derive(Clone, Eq, PartialEq)]
pub struct AccountConnectReceipt {
    pub intent_id: [u8; 32],
    pub expires_at_utc_ms: i64,
    pub state: String,
    pub provider: AccountConnectProvider,
    pub account_id: Option<String>,
    pub login_url: Option<String>,
    pub error_code: Option<ErrorCode>,
}
impl fmt::Debug for AccountConnectReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountConnectReceipt(<redacted>)")
    }
}
impl AccountConnectReceipt {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        if self.intent_id == [0; 32] || self.expires_at_utc_ms <= 0 {
            return Err(ErrorCode::InvalidInput);
        }
        if let Some(id) = &self.account_id {
            validate_account_id(id)?;
        }
        let valid = match self.state.as_str() {
            "queued" => self.error_code.is_none(),
            "awaiting_browser" => {
                self.provider == AccountConnectProvider::Google
                    && self.login_url.is_some()
                    && self.error_code.is_none()
            }
            "awaiting_setup" => {
                self.provider == AccountConnectProvider::ImapSmtp
                    && self.account_id.is_some()
                    && self.error_code.is_none()
            }
            "provider_ready" | "staged" | "ready" => {
                self.account_id.is_some() && self.error_code.is_none()
            }
            "cancelled" => self.error_code == Some(ErrorCode::ApprovalDenied),
            "expired" => self.error_code == Some(ErrorCode::ApprovalExpired),
            "failed" => self.error_code.is_some(),
            _ => false,
        };
        if !valid {
            return Err(ErrorCode::InvalidInput);
        }
        if self.login_url.as_ref().is_some_and(|url| {
            self.provider != AccountConnectProvider::Google
                || self.state != "awaiting_browser"
                || url.len() > MAX_LOGIN_URL
                || !url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?")
                || url.chars().any(char::is_control)
        }) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(())
    }
    pub fn to_json(&self) -> Result<Vec<u8>, ErrorCode> {
        self.validate()?;
        bounded_canonical(&json!({
            "protocol_version": 1, "intent_id": wire_hex::encode(&self.intent_id),
            "expires_at_utc_ms": self.expires_at_utc_ms, "state": self.state,
            "provider": self.provider.as_str(), "account_id": self.account_id,
            "login_url": self.login_url, "error_code": self.error_code.map(ErrorCode::as_str),
        }))
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self, ErrorCode> {
        let value = parse_canonical(bytes)?;
        let root = value.as_object().ok_or(ErrorCode::InvalidInput)?;
        require_keys(
            root,
            &[
                "protocol_version",
                "intent_id",
                "expires_at_utc_ms",
                "state",
                "provider",
                "account_id",
                "login_url",
                "error_code",
            ],
        )?;
        if root.get("protocol_version").and_then(Value::as_u64) != Some(1) {
            return Err(ErrorCode::InvalidInput);
        }
        let error_code = match optional_string(root, "error_code")? {
            None => None,
            Some(v) => Some(parse_error_code(v)?),
        };
        let receipt = Self {
            intent_id: wire_hex::decode_fixed::<32>(string(root, "intent_id")?)?,
            expires_at_utc_ms: root
                .get("expires_at_utc_ms")
                .and_then(Value::as_i64)
                .ok_or(ErrorCode::InvalidInput)?,
            state: string(root, "state")?.to_owned(),
            provider: AccountConnectProvider::parse(string(root, "provider")?)?,
            account_id: optional_string(root, "account_id")?.map(str::to_owned),
            login_url: optional_string(root, "login_url")?.map(str::to_owned),
            error_code,
        };
        receipt.validate()?;
        Ok(receipt)
    }
}

pub fn encode_request(
    request_id: &str,
    command: &AccountConnectCommand,
) -> Result<Vec<u8>, ErrorCode> {
    validate_request_id(request_id)?;
    let (method, params) = match command {
        AccountConnectCommand::Prepare {
            provider,
            selection,
        } => (
            PREPARE_METHOD,
            match selection {
                AccountConnectSelection::New => {
                    json!({"protocol_version": 1, "provider": provider.as_str(), "selection": {"kind": "new"}})
                }
                AccountConnectSelection::Existing(account_id) => {
                    validate_account_id(account_id)?;
                    json!({"protocol_version": 1, "provider": provider.as_str(), "selection": {"kind": "existing", "account_id": account_id}})
                }
                AccountConnectSelection::ImapSetup(request_id) => {
                    if *provider != AccountConnectProvider::ImapSmtp || *request_id == [0; 32] {
                        return Err(ErrorCode::InvalidInput);
                    }
                    json!({"protocol_version": 1, "provider": provider.as_str(), "selection": {"kind": "imap_setup", "request_id": wire_hex::encode(request_id)}})
                }
            },
        ),
        AccountConnectCommand::Status(id) => (STATUS_METHOD, intent_params(id)?),
        AccountConnectCommand::Complete(id) => (COMPLETE_METHOD, intent_params(id)?),
        AccountConnectCommand::Cancel(id) => (CANCEL_METHOD, intent_params(id)?),
        AccountConnectCommand::BindSmtp {
            intent_id,
            request_id,
        } => (BIND_SMTP_METHOD, bind_smtp_params(intent_id, request_id)?),
    };
    bounded_canonical(
        &json!({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}),
    )
}

pub fn parse_request(bytes: &[u8]) -> Result<(String, AccountConnectCommand), ErrorCode> {
    let value = parse_canonical(bytes)?;
    let root = value.as_object().ok_or(ErrorCode::InvalidInput)?;
    require_keys(root, &["jsonrpc", "id", "method", "params"])?;
    if string(root, "jsonrpc")? != "2.0" {
        return Err(ErrorCode::InvalidInput);
    }
    let request_id = string(root, "id")?.to_owned();
    validate_request_id(&request_id)?;
    let params = root
        .get("params")
        .ok_or(ErrorCode::InvalidInput)?
        .as_object()
        .ok_or(ErrorCode::InvalidInput)?;
    let command = match string(root, "method")? {
        PREPARE_METHOD => parse_prepare(params)?,
        STATUS_METHOD => AccountConnectCommand::Status(parse_intent_params(params)?),
        COMPLETE_METHOD => AccountConnectCommand::Complete(parse_intent_params(params)?),
        CANCEL_METHOD => AccountConnectCommand::Cancel(parse_intent_params(params)?),
        BIND_SMTP_METHOD => {
            let (intent_id, request_id) = parse_bind_smtp_params(params)?;
            AccountConnectCommand::BindSmtp {
                intent_id,
                request_id,
            }
        }
        _ => return Err(ErrorCode::InvalidInput),
    };
    Ok((request_id, command))
}

fn parse_prepare(params: &Map<String, Value>) -> Result<AccountConnectCommand, ErrorCode> {
    require_keys(params, &["protocol_version", "provider", "selection"])?;
    if params.get("protocol_version").and_then(Value::as_u64) != Some(1) {
        return Err(ErrorCode::InvalidInput);
    }
    let selection = params
        .get("selection")
        .ok_or(ErrorCode::InvalidInput)?
        .as_object()
        .ok_or(ErrorCode::InvalidInput)?;
    let result = match string(selection, "kind")? {
        "new" => {
            require_keys(selection, &["kind"])?;
            AccountConnectSelection::New
        }
        "existing" => {
            require_keys(selection, &["kind", "account_id"])?;
            AccountConnectSelection::Existing(string(selection, "account_id")?.to_owned())
        }
        "imap_setup" => {
            require_keys(selection, &["kind", "request_id"])?;
            if string(params, "provider")? != AccountConnectProvider::ImapSmtp.as_str() {
                return Err(ErrorCode::InvalidInput);
            }
            AccountConnectSelection::ImapSetup(wire_hex::decode_fixed::<32>(string(
                selection,
                "request_id",
            )?)?)
        }
        _ => return Err(ErrorCode::InvalidInput),
    };
    if let AccountConnectSelection::Existing(id) = &result {
        validate_account_id(id)?;
    }
    if let AccountConnectSelection::ImapSetup(id) = &result
        && *id == [0; 32]
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(AccountConnectCommand::Prepare {
        provider: AccountConnectProvider::parse(string(params, "provider")?)?,
        selection: result,
    })
}
fn intent_params(id: &[u8; 32]) -> Result<Value, ErrorCode> {
    if *id == [0; 32] {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(json!({"protocol_version": 1, "intent_id": wire_hex::encode(id)}))
}
fn bind_smtp_params(intent_id: &[u8; 32], request_id: &[u8; 32]) -> Result<Value, ErrorCode> {
    if *intent_id == [0; 32] || *request_id == [0; 32] {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(json!({
        "protocol_version": 1,
        "intent_id": wire_hex::encode(intent_id),
        "request_id": wire_hex::encode(request_id),
    }))
}
fn parse_intent_params(params: &Map<String, Value>) -> Result<[u8; 32], ErrorCode> {
    require_keys(params, &["protocol_version", "intent_id"])?;
    if params.get("protocol_version").and_then(Value::as_u64) != Some(1) {
        return Err(ErrorCode::InvalidInput);
    }
    let id = wire_hex::decode_fixed::<32>(string(params, "intent_id")?)?;
    if id == [0; 32] {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(id)
    }
}
fn parse_bind_smtp_params(params: &Map<String, Value>) -> Result<([u8; 32], [u8; 32]), ErrorCode> {
    require_keys(params, &["protocol_version", "intent_id", "request_id"])?;
    if params.get("protocol_version").and_then(Value::as_u64) != Some(1) {
        return Err(ErrorCode::InvalidInput);
    }
    let intent_id = wire_hex::decode_fixed::<32>(string(params, "intent_id")?)?;
    let request_id = wire_hex::decode_fixed::<32>(string(params, "request_id")?)?;
    if intent_id == [0; 32] || request_id == [0; 32] {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok((intent_id, request_id))
    }
}
fn parse_canonical(bytes: &[u8]) -> Result<Value, ErrorCode> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
        return Err(ErrorCode::InvalidInput);
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ErrorCode::InvalidInput)?;
    if serde_json::to_vec(&value).map_err(|_| ErrorCode::InvalidInput)? != bytes {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(value)
}
fn bounded_canonical(value: &Value) -> Result<Vec<u8>, ErrorCode> {
    let bytes = serde_json::to_vec(value).map_err(|_| ErrorCode::InvalidInput)?;
    if bytes.len() > MAX_FRAME_BYTES {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(bytes)
    }
}
fn require_keys(root: &Map<String, Value>, expected: &[&str]) -> Result<(), ErrorCode> {
    if root.len() == expected.len() && expected.iter().all(|key| root.contains_key(*key)) {
        Ok(())
    } else {
        Err(ErrorCode::InvalidInput)
    }
}
fn string<'a>(root: &'a Map<String, Value>, key: &str) -> Result<&'a str, ErrorCode> {
    root.get(key)
        .and_then(Value::as_str)
        .ok_or(ErrorCode::InvalidInput)
}
fn optional_string<'a>(
    root: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, ErrorCode> {
    match root.get(key) {
        Some(Value::String(v)) => Ok(Some(v)),
        Some(Value::Null) => Ok(None),
        _ => Err(ErrorCode::InvalidInput),
    }
}
fn validate_request_id(value: &str) -> Result<(), ErrorCode> {
    if value.is_empty()
        || value.len() > MAX_REQUEST_ID
        || !value.is_ascii()
        || value.chars().any(char::is_control)
    {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}
fn validate_account_id(value: &str) -> Result<(), ErrorCode> {
    if value.is_empty()
        || value.len() > MAX_ACCOUNT_ID
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}
fn parse_error_code(value: &str) -> Result<ErrorCode, ErrorCode> {
    ErrorCode::ALL
        .into_iter()
        .find(|code| code.as_str() == value)
        .ok_or(ErrorCode::InvalidInput)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id() -> [u8; 32] {
        [7; 32]
    }
    fn request(command: AccountConnectCommand) -> Vec<u8> {
        encode_request("req-1", &command).unwrap()
    }
    #[test]
    fn sec03_account_connect_methods_are_exact_and_roundtrip() {
        for command in [
            AccountConnectCommand::Prepare {
                provider: AccountConnectProvider::Google,
                selection: AccountConnectSelection::New,
            },
            AccountConnectCommand::Prepare {
                provider: AccountConnectProvider::ImapSmtp,
                selection: AccountConnectSelection::Existing("acct-1".into()),
            },
            AccountConnectCommand::Prepare {
                provider: AccountConnectProvider::ImapSmtp,
                selection: AccountConnectSelection::ImapSetup(id()),
            },
            AccountConnectCommand::Status(id()),
            AccountConnectCommand::Complete(id()),
            AccountConnectCommand::Cancel(id()),
            AccountConnectCommand::BindSmtp {
                intent_id: id(),
                request_id: [8; 32],
            },
        ] {
            assert_eq!(parse_request(&request(command.clone())).unwrap().1, command);
        }
        let mut value: Value =
            serde_json::from_slice(&request(AccountConnectCommand::Status(id()))).unwrap();
        value["method"] = Value::String("app.accounts.connect.prepare.extra".into());
        assert_eq!(
            parse_request(&serde_json::to_vec(&value).unwrap()),
            Err(ErrorCode::InvalidInput)
        );
    }
    #[test]
    fn sec03_prepare_rejects_authority_fields_and_noncanonical_or_duplicate_input() {
        let mut value: Value = serde_json::from_slice(&request(AccountConnectCommand::Prepare {
            provider: AccountConnectProvider::Google,
            selection: AccountConnectSelection::New,
        }))
        .unwrap();
        value["params"]["scope"] = Value::String("full".into());
        assert!(parse_request(&serde_json::to_vec(&value).unwrap()).is_err());
        let duplicate = br#"{"jsonrpc":"2.0","id":"x","method":"app.accounts.connect.status","params":{"protocol_version":1,"intent_id":"0707070707070707070707070707070707070707070707070707070707070707","intent_id":"0707070707070707070707070707070707070707070707070707070707070707"}}"#;
        assert!(parse_request(duplicate).is_err());
        assert!(parse_request(br#" {"jsonrpc":"2.0","id":"x","method":"app.accounts.connect.status","params":{"protocol_version":1,"intent_id":"0707070707070707070707070707070707070707070707070707070707070707"}}"#).is_err());
    }
    #[test]
    fn sec03_receipt_validates_provider_url_state_and_redacts() {
        let receipt = AccountConnectReceipt {
            intent_id: id(),
            expires_at_utc_ms: 1,
            state: "awaiting_browser".into(),
            provider: AccountConnectProvider::Google,
            account_id: None,
            login_url: Some("https://accounts.google.com/o/oauth2/v2/auth?x=1".into()),
            error_code: None,
        };
        let bytes = receipt.to_json().unwrap();
        assert!(!format!("{receipt:?}").contains("accounts.google.com"));
        assert!(AccountConnectReceipt::from_json(&bytes).is_ok());
        let mut invalid = receipt;
        invalid.provider = AccountConnectProvider::ImapSmtp;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn sec03_receipt_requires_account_for_provider_ready_and_browser_url_for_google() {
        let mut receipt = AccountConnectReceipt {
            intent_id: id(),
            expires_at_utc_ms: 1,
            state: "provider_ready".into(),
            provider: AccountConnectProvider::Google,
            account_id: None,
            login_url: None,
            error_code: None,
        };
        assert!(receipt.validate().is_err());
        receipt.state = "awaiting_browser".into();
        receipt.account_id = None;
        assert!(receipt.validate().is_err());
        receipt.login_url = Some("https://accounts.google.com/o/oauth2/v2/auth?x=1".into());
        assert!(receipt.validate().is_ok());
    }

    #[test]
    fn sec03_awaiting_setup_is_imap_only_and_account_bound() {
        let mut receipt = AccountConnectReceipt {
            intent_id: id(),
            expires_at_utc_ms: 1,
            state: "awaiting_setup".into(),
            provider: AccountConnectProvider::ImapSmtp,
            account_id: Some("imap-account".into()),
            login_url: None,
            error_code: None,
        };
        assert!(receipt.validate().is_ok());
        receipt.provider = AccountConnectProvider::Google;
        assert!(receipt.validate().is_err());
        receipt.provider = AccountConnectProvider::ImapSmtp;
        receipt.account_id = None;
        assert!(receipt.validate().is_err());
    }
}
