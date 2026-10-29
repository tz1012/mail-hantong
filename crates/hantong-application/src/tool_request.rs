//! Pure validation for the common tool envelope.
//!
//! This module receives a `serde_json::Value` that has already passed the
//! bounded IPC decoder. It performs no authentication, policy, approval, or
//! provider work.

use hantong_contracts::{
    ErrorCode,
    audit::AuditTool,
    folders::{FolderCursor, FolderListRequest, FolderRefreshRequest, FolderRefreshStatusRequest},
};
use serde_json::{Map, Value};
use std::{collections::BTreeSet, fmt};

#[derive(Clone, Eq, PartialEq)]
pub enum ToolRequest {
    AccountsList(AccountsListRequest),
    FoldersList {
        request_id: String,
        request: FolderListRequest,
    },
    FoldersRefresh {
        request_id: String,
        action: FolderRefreshAction,
    },
    SendExecute(SendExecuteRequest),
}

#[derive(Clone, Eq, PartialEq)]
pub enum FolderRefreshAction {
    Request(FolderRefreshRequest),
    Status(FolderRefreshStatusRequest),
}

#[derive(Clone, Eq, PartialEq)]
pub struct AccountsListRequest {
    request_id: String,
    account_ids: Vec<String>,
    include_capabilities: bool,
}

#[derive(Clone, Eq, PartialEq)]
pub struct SendExecuteRequest {
    request_id: String,
    account_id: String,
    draft_id: String,
    operation_id: String,
    expected_version: i64,
    payload_hash: [u8; 32],
    approval_id: String,
    idempotency_key: String,
}

impl fmt::Debug for ToolRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccountsList(_) => f.write_str("ToolRequest::AccountsList(<redacted>)"),
            Self::FoldersList { .. } => f.write_str("ToolRequest::FoldersList(<redacted>)"),
            Self::FoldersRefresh { .. } => f.write_str("ToolRequest::FoldersRefresh(<redacted>)"),
            Self::SendExecute(_) => f.write_str("ToolRequest::SendExecute(<redacted>)"),
        }
    }
}
impl fmt::Debug for AccountsListRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountsListRequest(<redacted>)")
    }
}
impl fmt::Debug for SendExecuteRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SendExecuteRequest(<redacted>)")
    }
}

impl ToolRequest {
    /// Validates a tool envelope after the strict IPC decoder has bounded it.
    pub fn validate(value: &Value) -> Result<Self, ErrorCode> {
        let root = object(value)?;
        exact_keys(root, &["schema_version", "request_id", "tool", "params"])?;
        if integer(root.get("schema_version").unwrap())? != 1 {
            return Err(ErrorCode::InvalidInput);
        }
        let request_id = bounded_id(string(root.get("request_id").unwrap())?, 64)?;
        let tool = string(root.get("tool").unwrap())?;
        let params = object(root.get("params").unwrap())?;
        match tool {
            "mail.accounts.list" => Ok(Self::AccountsList(parse_accounts(request_id, params)?)),
            "mail.folders.list" => Ok(Self::FoldersList {
                request_id,
                request: parse_folder_list(params)?,
            }),
            "mail.folders.refresh" => Ok(Self::FoldersRefresh {
                request_id,
                action: parse_folder_refresh(params)?,
            }),
            "mail.send.execute" => Ok(Self::SendExecute(parse_send(request_id, params)?)),
            _ => Err(ErrorCode::InvalidInput),
        }
    }

    /// Validates the envelope and binds its tool to the outer JSON-RPC method.
    pub fn validate_method(method: &str, value: &Value) -> Result<Self, ErrorCode> {
        let request = Self::validate(value)?;
        let expected = request.tool().as_str();
        if method == expected {
            Ok(request)
        } else {
            Err(ErrorCode::InvalidInput)
        }
    }

    pub fn request_id(&self) -> &str {
        match self {
            Self::AccountsList(v) => &v.request_id,
            Self::FoldersList { request_id, .. } | Self::FoldersRefresh { request_id, .. } => {
                request_id
            }
            Self::SendExecute(v) => &v.request_id,
        }
    }

    pub fn tool(&self) -> AuditTool {
        match self {
            Self::AccountsList(_) => AuditTool::AccountsList,
            Self::FoldersList { .. } => AuditTool::FoldersList,
            Self::FoldersRefresh { .. } => AuditTool::FoldersRefresh,
            Self::SendExecute(_) => AuditTool::SendExecute,
        }
    }

    pub fn account_ids(&self) -> &[String] {
        match self {
            Self::AccountsList(value) => value.account_ids(),
            Self::FoldersList { request, .. } => request.account_ids(),
            Self::FoldersRefresh { action, .. } => match action {
                FolderRefreshAction::Request(value) => value.account_ids(),
                FolderRefreshAction::Status(value) => value.account_ids(),
            },
            Self::SendExecute(value) => std::slice::from_ref(&value.account_id),
        }
    }
}

impl ToolRequest {
    pub fn folder_list(&self) -> Option<&FolderListRequest> {
        match self {
            Self::FoldersList { request, .. } => Some(request),
            _ => None,
        }
    }
    pub fn folder_refresh(&self) -> Option<&FolderRefreshAction> {
        match self {
            Self::FoldersRefresh { action, .. } => Some(action),
            _ => None,
        }
    }
}

impl AccountsListRequest {
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub fn account_ids(&self) -> &[String] {
        &self.account_ids
    }
    pub fn include_capabilities(&self) -> bool {
        self.include_capabilities
    }
}
impl SendExecuteRequest {
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn draft_id(&self) -> &str {
        &self.draft_id
    }
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn expected_version(&self) -> i64 {
        self.expected_version
    }
    pub fn payload_hash(&self) -> &[u8; 32] {
        &self.payload_hash
    }
    pub fn approval_id(&self) -> &str {
        &self.approval_id
    }
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }
}

fn object(value: &Value) -> Result<&Map<String, Value>, ErrorCode> {
    value.as_object().ok_or(ErrorCode::InvalidInput)
}
fn string(value: &Value) -> Result<&str, ErrorCode> {
    value.as_str().ok_or(ErrorCode::InvalidInput)
}
fn integer(value: &Value) -> Result<i64, ErrorCode> {
    value.as_i64().ok_or(ErrorCode::InvalidInput)
}

fn exact_keys(map: &Map<String, Value>, allowed: &[&str]) -> Result<(), ErrorCode> {
    if map.keys().all(|key| allowed.contains(&key.as_str())) && map.len() == allowed.len() {
        Ok(())
    } else {
        Err(ErrorCode::InvalidInput)
    }
}
fn bounded_id(value: &str, max: usize) -> Result<String, ErrorCode> {
    if value.is_empty()
        || value.len() > max
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(value.to_owned())
}
fn account_id(value: &str) -> Result<String, ErrorCode> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(value.to_owned())
}
fn parse_accounts(
    request_id: String,
    params: &Map<String, Value>,
) -> Result<AccountsListRequest, ErrorCode> {
    if params
        .keys()
        .any(|k| k != "account_id" && k != "account_ids" && k != "include_capabilities")
    {
        return Err(ErrorCode::InvalidInput);
    }
    let include = match params.get("include_capabilities") {
        None => false,
        Some(v) => v.as_bool().ok_or(ErrorCode::InvalidInput)?,
    };
    let mut ids = match (params.get("account_id"), params.get("account_ids")) {
        (Some(_), Some(_)) | (None, None) => return Err(ErrorCode::InvalidInput),
        (Some(v), None) => vec![account_id(string(v)?)?],
        (None, Some(v)) => {
            let values = v.as_array().ok_or(ErrorCode::InvalidInput)?;
            if values.is_empty() || values.len() > 128 {
                return Err(ErrorCode::InvalidInput);
            }
            values
                .iter()
                .map(|v| account_id(string(v)?))
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    let mut unique = BTreeSet::new();
    if !ids.iter().all(|id| unique.insert(id.clone())) {
        return Err(ErrorCode::InvalidInput);
    }
    ids.sort();
    Ok(AccountsListRequest {
        request_id,
        account_ids: ids,
        include_capabilities: include,
    })
}
fn parse_send(
    request_id: String,
    params: &Map<String, Value>,
) -> Result<SendExecuteRequest, ErrorCode> {
    const KEYS: [&str; 7] = [
        "account_id",
        "draft_id",
        "operation_id",
        "expected_version",
        "payload_hash",
        "approval_id",
        "idempotency_key",
    ];
    exact_keys(params, &KEYS)?;
    let id = |key: &str| {
        bounded_id(
            string(params.get(key).ok_or(ErrorCode::InvalidInput)?)?,
            128,
        )
    };
    let expected = integer(params.get("expected_version").unwrap())?;
    if expected < 1 {
        return Err(ErrorCode::InvalidInput);
    }
    let hash = string(params.get("payload_hash").unwrap())?;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ErrorCode::InvalidInput);
    }
    let mut payload_hash = [0; 32];
    for (i, chunk) in hash.as_bytes().chunks_exact(2).enumerate() {
        payload_hash[i] = (hex(chunk[0]) << 4) | hex(chunk[1]);
    }
    Ok(SendExecuteRequest {
        request_id,
        account_id: account_id(string(
            params.get("account_id").ok_or(ErrorCode::InvalidInput)?,
        )?)?,
        draft_id: id("draft_id")?,
        operation_id: id("operation_id")?,
        expected_version: expected,
        payload_hash,
        approval_id: id("approval_id")?,
        idempotency_key: id("idempotency_key")?,
    })
}

fn parse_folder_list(params: &Map<String, Value>) -> Result<FolderListRequest, ErrorCode> {
    if params
        .keys()
        .any(|k| !matches!(k.as_str(), "account_id" | "limit" | "cursor"))
    {
        return Err(ErrorCode::InvalidInput);
    }
    let account = account_id(string(
        params.get("account_id").ok_or(ErrorCode::InvalidInput)?,
    )?)?;
    let limit = match params.get("limit") {
        Some(v) => Some(u32::try_from(integer(v)?).map_err(|_| ErrorCode::InvalidInput)?),
        None => None,
    };
    let cursor = match params.get("cursor") {
        Some(v) => Some(FolderCursor::parse(string(v)?)?),
        None => None,
    };
    FolderListRequest::new(&account, limit, cursor)
}

fn parse_folder_refresh(params: &Map<String, Value>) -> Result<FolderRefreshAction, ErrorCode> {
    let action = string(params.get("action").ok_or(ErrorCode::InvalidInput)?)?;
    match action {
        "request" => {
            const KEYS: [&str; 6] = [
                "action",
                "account_id",
                "operation_id",
                "idempotency_key",
                "expected_generation",
                "payload_hash",
            ];
            exact_keys(params, &KEYS)?;
            let account = account_id(string(params.get("account_id").unwrap())?)?;
            let operation = bounded_id(string(params.get("operation_id").unwrap())?, 128)?;
            let key = bounded_id(string(params.get("idempotency_key").unwrap())?, 128)?;
            let generation = u64::try_from(integer(params.get("expected_generation").unwrap())?)
                .map_err(|_| ErrorCode::InvalidInput)?;
            let hash = string(params.get("payload_hash").unwrap())?;
            Ok(FolderRefreshAction::Request(FolderRefreshRequest::new(
                &account, &operation, &key, generation, hash,
            )?))
        }
        "status" => {
            const KEYS: [&str; 3] = ["action", "account_id", "execution_id"];
            exact_keys(params, &KEYS)?;
            let account = account_id(string(params.get("account_id").unwrap())?)?;
            let encoded = string(params.get("execution_id").unwrap())?;
            let execution_id = hantong_contracts::wire_hex::decode_fixed::<32>(encoded)?;
            Ok(FolderRefreshAction::Status(
                FolderRefreshStatusRequest::new(&account, execution_id)?,
            ))
        }
        _ => Err(ErrorCode::InvalidInput),
    }
}
fn hex(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn envelope(tool: &str, params: Value) -> Value {
        serde_json::json!({"schema_version":1,"request_id":"r-1","tool":tool,"params":params})
    }
    fn send_params() -> Value {
        serde_json::json!({"account_id":"a","draft_id":"d","operation_id":"o","expected_version":1,"payload_hash":"aa00000000000000000000000000000000000000000000000000000000000000","approval_id":"p","idempotency_key":"i"})
    }
    #[test]
    fn ai05_accounts_are_sorted_and_bounded() {
        let r = ToolRequest::validate(&envelope(
            "mail.accounts.list",
            serde_json::json!({"account_ids":["z","a"]}),
        ))
        .unwrap();
        let ToolRequest::AccountsList(v) = r else {
            panic!()
        };
        assert_eq!(v.account_ids(), &["a", "z"]);
        assert!(
            ToolRequest::validate(&envelope(
                "mail.accounts.list",
                serde_json::json!({"account_ids":[]})
            ))
            .is_err()
        );
    }

    #[test]
    fn m203_folder_tools_parse_exact_actions_and_bind_method() {
        let list = envelope(
            "mail.folders.list",
            serde_json::json!({"account_id":"a","limit":2}),
        );
        let ToolRequest::FoldersList { request, .. } =
            ToolRequest::validate_method("mail.folders.list", &list).unwrap()
        else {
            panic!()
        };
        assert_eq!(request.account_id(), "a");
        let refresh = envelope(
            "mail.folders.refresh",
            serde_json::json!({"action":"request","account_id":"a","operation_id":"op","idempotency_key":"key","expected_generation":0,"payload_hash":"00".repeat(32)}),
        );
        assert!(matches!(
            ToolRequest::validate_method("mail.folders.refresh", &refresh).unwrap(),
            ToolRequest::FoldersRefresh {
                action: FolderRefreshAction::Request(_),
                ..
            }
        ));
        let status = envelope(
            "mail.folders.refresh",
            serde_json::json!({"action":"status","account_id":"a","execution_id":"01".repeat(32)}),
        );
        assert!(matches!(
            ToolRequest::validate(&status).unwrap(),
            ToolRequest::FoldersRefresh {
                action: FolderRefreshAction::Status(_),
                ..
            }
        ));
        let mut invalid = list;
        invalid["params"]["cursor"] = serde_json::Value::Null;
        assert_eq!(
            ToolRequest::validate(&invalid),
            Err(ErrorCode::InvalidInput)
        );
    }
    #[test]
    fn ai05_rejects_unknown_auth_fields_and_duplicates() {
        for field in ["client_id", "role", "session_id", "approved"] {
            let mut params = serde_json::json!({"account_id":"a"});
            params
                .as_object_mut()
                .unwrap()
                .insert(field.into(), Value::Bool(true));
            assert!(ToolRequest::validate(&envelope("mail.accounts.list", params)).is_err());
            let mut root = envelope("mail.accounts.list", serde_json::json!({"account_id":"a"}));
            root.as_object_mut()
                .unwrap()
                .insert(field.into(), Value::Bool(true));
            assert!(ToolRequest::validate(&root).is_err());
        }
        assert!(
            ToolRequest::validate(&envelope(
                "mail.accounts.list",
                serde_json::json!({"account_ids":["a","a"]})
            ))
            .is_err()
        );
        assert!(
            ToolRequest::validate(&envelope(
                "mail.accounts.list",
                serde_json::json!({"account_id":"a","account_ids":["b"]})
            ))
            .is_err()
        );
    }
    #[test]
    fn ai05_send_requires_all_fields_and_integer_version() {
        let base = send_params();
        let r = ToolRequest::validate(&envelope("mail.send.execute", base)).unwrap();
        let ToolRequest::SendExecute(v) = r else {
            panic!()
        };
        assert_eq!(v.payload_hash()[0], 0xaa);
        let bad = serde_json::json!({"account_id":"a","draft_id":"d","operation_id":"o","expected_version":1.2,"payload_hash":"aa00000000000000000000000000000000000000000000000000000000000000","approval_id":"p","idempotency_key":"i"});
        assert!(ToolRequest::validate(&envelope("mail.send.execute", bad)).is_err());
    }
    #[test]
    fn ai05_debug_redacts_values() {
        let v = ToolRequest::validate(&envelope(
            "mail.accounts.list",
            serde_json::json!({"account_id":"secret-account"}),
        ))
        .unwrap();
        let d = format!("{v:?}");
        assert!(!d.contains("secret-account"));
    }

    #[test]
    fn ai05_binds_outer_method_to_inner_tool_and_preserves_scope() {
        let accounts = envelope(
            "mail.accounts.list",
            serde_json::json!({"account_ids":["z","a"]}),
        );
        let parsed = ToolRequest::validate_method("mail.accounts.list", &accounts).unwrap();
        assert_eq!(parsed.tool(), AuditTool::AccountsList);
        assert_eq!(parsed.account_ids(), &["a", "z"]);
        assert!(ToolRequest::validate_method("mail.send.execute", &accounts).is_err());

        let send = envelope("mail.send.execute", send_params());
        let parsed = ToolRequest::validate_method("mail.send.execute", &send).unwrap();
        assert_eq!(parsed.tool(), AuditTool::SendExecute);
        assert_eq!(parsed.account_ids(), &["a"]);
        assert!(ToolRequest::validate_method("mail.unknown", &send).is_err());
    }

    #[test]
    fn ai05_method_binding_does_not_accept_claimed_authorization_fields() {
        let mut value = envelope("mail.accounts.list", serde_json::json!({"account_id":"a"}));
        value["params"]["approved"] = Value::Bool(true);
        assert!(ToolRequest::validate_method("mail.accounts.list", &value).is_err());
        value = envelope("mail.accounts.list", serde_json::json!({"account_id":"a"}));
        value["role"] = Value::String("admin".into());
        assert!(ToolRequest::validate_method("mail.accounts.list", &value).is_err());
    }

    #[test]
    fn ai05_enforces_request_and_identifier_lengths() {
        let mut value = envelope("mail.accounts.list", serde_json::json!({"account_id":"a"}));
        value["request_id"] = Value::String("r".repeat(65));
        assert!(ToolRequest::validate(&value).is_err());
        value["request_id"] = Value::String("r".repeat(64));
        value["params"]["account_id"] = Value::String("a".repeat(129));
        assert!(ToolRequest::validate(&value).is_err());
        value["params"]["account_id"] = Value::String("a".repeat(128));
        assert!(ToolRequest::validate(&value).is_ok());
        value["params"] = serde_json::json!({
            "account_ids": (0..129).map(|i| Value::String(format!("a{i}"))).collect::<Vec<_>>()
        });
        assert!(ToolRequest::validate(&value).is_err());
    }

    #[test]
    fn ai05_accounts_accept_exact_maximum_and_reject_bad_values() {
        let ids = (0..128)
            .map(|i| Value::String(format!("a{i}")))
            .collect::<Vec<_>>();
        assert!(
            ToolRequest::validate(&envelope(
                "mail.accounts.list",
                serde_json::json!({"account_ids":ids})
            ))
            .is_ok()
        );
        for bad in ["", "white space", "점", "a.b", "a/b"] {
            assert!(
                ToolRequest::validate(&envelope(
                    "mail.accounts.list",
                    serde_json::json!({"account_id":bad})
                ))
                .is_err()
            );
        }
        for value in [Value::Null, Value::String("true".into())] {
            let mut p = serde_json::json!({"account_id":"a","include_capabilities":false});
            p["include_capabilities"] = value;
            assert!(ToolRequest::validate(&envelope("mail.accounts.list", p)).is_err());
        }
        let ok = envelope(
            "mail.accounts.list",
            serde_json::json!({"account_id":"a","include_capabilities":true}),
        );
        assert!(ToolRequest::validate(&ok).is_ok());
    }

    #[test]
    fn ai05_rejects_scalar_unknown_tool_and_malformed_envelopes() {
        assert!(ToolRequest::validate(&Value::String("x".into())).is_err());
        assert!(ToolRequest::validate(&envelope("mail.unknown", serde_json::json!({}))).is_err());
        for version in [
            Value::from(0),
            Value::from(2),
            Value::String("1".into()),
            Value::Null,
        ] {
            let mut v = envelope("mail.accounts.list", serde_json::json!({"account_id":"a"}));
            v["schema_version"] = version;
            assert!(ToolRequest::validate(&v).is_err());
        }
        let mut v = envelope("mail.accounts.list", serde_json::json!(["a"]));
        assert!(ToolRequest::validate(&v).is_err());
        v["params"] = Value::String("x".into());
        assert!(ToolRequest::validate(&v).is_err());
    }

    #[test]
    fn ai05_send_requires_each_field_and_rejects_nulls() {
        for field in [
            "account_id",
            "draft_id",
            "operation_id",
            "expected_version",
            "payload_hash",
            "approval_id",
            "idempotency_key",
        ] {
            let mut p = send_params();
            p.as_object_mut().unwrap().remove(field);
            assert!(ToolRequest::validate(&envelope("mail.send.execute", p)).is_err());
            let mut p = send_params();
            p[field] = Value::Null;
            assert!(ToolRequest::validate(&envelope("mail.send.execute", p)).is_err());
        }
    }

    #[test]
    fn ai05_send_hash_and_max_version_validation() {
        let mut p = send_params();
        p["expected_version"] = Value::from(i64::MAX);
        assert!(ToolRequest::validate(&envelope("mail.send.execute", p)).is_ok());
        for hash in [
            "",
            "aa",
            "zz00000000000000000000000000000000000000000000000000000000000000",
            "aa0000000000000000000000000000000000000000000000000000000000000",
        ] {
            let mut p = send_params();
            p["payload_hash"] = Value::String(hash.into());
            assert!(ToolRequest::validate(&envelope("mail.send.execute", p)).is_err());
        }
        let mut p = send_params();
        p["payload_hash"] = Value::String("한".repeat(64));
        assert!(ToolRequest::validate(&envelope("mail.send.execute", p)).is_err());
    }

    #[test]
    fn ai05_rejects_negative_and_overflow_versions() {
        for version in [
            Value::from(-1),
            Value::from(0),
            Value::from(u64::MAX),
            Value::from(1.0),
        ] {
            let mut params = send_params();
            params["expected_version"] = version;
            assert!(ToolRequest::validate(&envelope("mail.send.execute", params)).is_err());
        }
    }

    #[test]
    fn ai05_envelope_requires_every_field_and_rejects_null_batch_and_bad_ids() {
        let base = envelope("mail.accounts.list", serde_json::json!({"account_id":"a"}));
        for field in ["schema_version", "request_id", "tool", "params"] {
            let mut value = base.clone();
            value.as_object_mut().unwrap().remove(field);
            assert_eq!(ToolRequest::validate(&value), Err(ErrorCode::InvalidInput));
            let mut value = base.clone();
            value[field] = Value::Null;
            assert_eq!(ToolRequest::validate(&value), Err(ErrorCode::InvalidInput));
        }
        for value in [
            Value::Null,
            Value::Bool(true),
            serde_json::json!([base.clone()]),
        ] {
            assert_eq!(ToolRequest::validate(&value), Err(ErrorCode::InvalidInput));
        }
        for id in ["", "a b", "한글", "a/b", "x\ny"] {
            let mut value = base.clone();
            value["request_id"] = Value::from(id);
            assert_eq!(ToolRequest::validate(&value), Err(ErrorCode::InvalidInput));
        }
        let mut value = base;
        value["schema_version"] = Value::from(1.0);
        assert_eq!(ToolRequest::validate(&value), Err(ErrorCode::InvalidInput));
    }

    #[test]
    fn ai05_send_ids_hash_case_and_inner_debug_follow_contract() {
        for field in ["draft_id", "operation_id", "approval_id", "idempotency_key"] {
            for invalid in [String::new(), "bad/id".into(), "한".into(), "x".repeat(129)] {
                let mut params = send_params();
                params[field] = Value::from(invalid);
                assert_eq!(
                    ToolRequest::validate(&envelope("mail.send.execute", params)),
                    Err(ErrorCode::InvalidInput)
                );
            }
            let mut params = send_params();
            params[field] = Value::from("x".repeat(128));
            assert!(ToolRequest::validate(&envelope("mail.send.execute", params)).is_ok());
        }
        let lower = ToolRequest::validate(&envelope("mail.send.execute", send_params())).unwrap();
        let mut params = send_params();
        params["payload_hash"] =
            Value::from(params["payload_hash"].as_str().unwrap().to_uppercase());
        let upper = ToolRequest::validate(&envelope("mail.send.execute", params)).unwrap();
        assert_eq!(lower, upper);
        let ToolRequest::SendExecute(value) = upper else {
            panic!("wrong tool");
        };
        assert_eq!(format!("{value:?}"), "SendExecuteRequest(<redacted>)");
        for field in ["approved", "role", "client_id", "session_id"] {
            let mut params = send_params();
            params[field] = Value::Bool(true);
            assert_eq!(
                ToolRequest::validate(&envelope("mail.send.execute", params)),
                Err(ErrorCode::InvalidInput)
            );
        }
    }
}
