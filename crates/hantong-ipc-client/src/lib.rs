#![forbid(unsafe_code)]

//! Bounded bootstrap, registration, and mutually authenticated read clients.

#[cfg(windows)]
mod auth;
#[cfg(windows)]
mod auth_diagnostic;
#[cfg(windows)]
pub use auth::{PendingAccountClientAuthentication, PendingClientAuthentication};
#[cfg(windows)]
pub mod authenticated_wire;
#[cfg(windows)]
mod trusted_profile;
#[cfg(windows)]
pub use trusted_profile::{
    AuthenticatedDefaultProfile, AuthenticatedGuiProfile, authenticate_ai, authenticate_default,
    authenticate_default_app_read, authenticate_gui, authenticate_mcp, verify_ai, verify_default,
};

/// Verifies the active Unix default profile through the Core-owned relay.
/// The profile loader and relay perform the fixed-path and registration
/// checks; this probe does not expose credentials or establish a second
/// client-side signing path.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn verify_default() -> Result<(), ErrorCode> {
    accounts::list_default_accounts(false).map(|_| ())
}

/// Unix relay-backed profile used by the MCP and built-in AI clients.  The
/// client loads only the fixed public profile selector and scope; Core owns
/// authentication, consent, and all private signing material.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub struct AuthenticatedDefaultProfile {
    selector: profile::ProfileSelector,
    accounts: Vec<String>,
    scope: RequestedScope,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl AuthenticatedDefaultProfile {
    pub fn accounts(&self) -> &[String] {
        &self.accounts
    }

    pub fn scope(&self) -> &RequestedScope {
        &self.scope
    }

    pub fn tools(&self) -> &[String] {
        self.scope.tools()
    }

    pub async fn exchange(
        self,
        id: &str,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<Vec<u8>, ErrorCode> {
        unix_call(self.selector, id, method, params).await
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub async fn authenticate_mcp() -> Result<AuthenticatedDefaultProfile, ErrorCode> {
    authenticate_unix_profile(profile::ProfileSelector::Mcp).await
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub async fn authenticate_ai(
    provider: AiProvider,
) -> Result<AuthenticatedDefaultProfile, ErrorCode> {
    authenticate_unix_profile(match provider {
        AiProvider::Codex => profile::ProfileSelector::AiCodex,
        AiProvider::Claude => profile::ProfileSelector::AiClaude,
    })
    .await
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn authenticate_unix_profile(
    selector: profile::ProfileSelector,
) -> Result<AuthenticatedDefaultProfile, ErrorCode> {
    let loaded = match selector {
        profile::ProfileSelector::Mcp => hantong_platform::load_mcp_client_profile()?,
        profile::ProfileSelector::AiCodex => hantong_platform::load_ai_codex_client_profile()?,
        profile::ProfileSelector::AiClaude => hantong_platform::load_ai_claude_client_profile()?,
        _ => return Err(ErrorCode::ScopeDenied),
    };
    let parsed = profile::ParsedClientProfile::from_json(loaded.bytes())?;
    if parsed.profile() != selector || parsed.scope().accounts().is_empty() {
        return Err(ErrorCode::AuthRequired);
    }
    let request_id = match selector {
        profile::ProfileSelector::Mcp => "mcp-profile-accounts-probe-1",
        profile::ProfileSelector::AiCodex => "ai-codex-accounts-probe-1",
        profile::ProfileSelector::AiClaude => "ai-claude-accounts-probe-1",
        _ => return Err(ErrorCode::ScopeDenied),
    };
    let params = serde_json::json!({
        "account_ids": parsed.scope().accounts(),
        "include_capabilities": true,
    });
    let response = unix_call(selector, request_id, "mail.accounts.list", &params).await?;
    accounts::parse_account_list_response(&response, request_id, parsed.scope().accounts(), true)?;
    Ok(AuthenticatedDefaultProfile {
        selector,
        accounts: parsed.scope().accounts().to_vec(),
        scope: parsed.scope().clone(),
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn unix_call(
    selector: profile::ProfileSelector,
    id: &str,
    method: &str,
    params: &serde_json::Value,
) -> Result<Vec<u8>, ErrorCode> {
    let id = id.to_owned();
    let method = method.to_owned();
    let params = params.clone();
    tokio::task::spawn_blocking(move || unix_relay::call(selector, &id, &method, &params))
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?
}

pub mod account_auth_wire;
pub mod account_profile;
pub mod accounts;
pub mod auth_wire;
pub mod diagnostic_snapshot;
pub mod display_settings;
pub mod folders;
pub mod google_mutation;
pub mod imap_setup;
pub mod imap_trash;
pub mod initial_sync;
pub mod mail_send;
pub mod message_body;
pub mod message_search;
pub mod profile;
pub mod registration;
pub mod uid_inspection;
pub mod uid_page;
pub mod uid_reconcile;
pub mod uid_recovery;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod unix_auth;
pub mod unix_auth_wire;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod unix_authenticated_wire;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod unix_relay;

use hantong_contracts::ErrorCode;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use hantong_contracts::{auth::RequestedScope, registration::AiProvider};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_RESPONSE: usize = 16_384;
const PROTOCOL_VERSION: u64 = 1;

/// Verifies a provider-qualified built-in AI profile through the Core relay.
/// Only the fixed public profile is loaded here; credentials and signing keys
/// remain in Core's protected path.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn verify_ai(provider: AiProvider) -> Result<(), ErrorCode> {
    let (selector, loaded) = match provider {
        AiProvider::Codex => (
            profile::ProfileSelector::AiCodex,
            hantong_platform::load_ai_codex_client_profile()?,
        ),
        AiProvider::Claude => (
            profile::ProfileSelector::AiClaude,
            hantong_platform::load_ai_claude_client_profile()?,
        ),
    };
    let profile = profile::ParsedClientProfile::from_json(loaded.bytes())?;
    if profile.profile() != selector || profile.scope().accounts().is_empty() {
        return Err(ErrorCode::AuthRequired);
    }
    let request_id = match provider {
        AiProvider::Codex => "ai-codex-accounts-probe-1",
        AiProvider::Claude => "ai-claude-accounts-probe-1",
    };
    let params = serde_json::json!({
        "account_ids": profile.scope().accounts(),
        "include_capabilities": true,
    });
    let response = unix_relay::call(selector, request_id, "mail.accounts.list", &params)?;
    let value: serde_json::Value =
        serde_json::from_slice(&response).map_err(|_| ErrorCode::InvalidInput)?;
    let object = value.as_object().ok_or(ErrorCode::InvalidInput)?;
    if object.get("jsonrpc") != Some(&serde_json::Value::String("2.0".into()))
        || object.get("id") != Some(&serde_json::Value::String(request_id.into()))
    {
        return Err(ErrorCode::InvalidInput);
    }
    if let Some(error) = object.get("error") {
        let code = error
            .get("code")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| {
                ErrorCode::ALL
                    .into_iter()
                    .find(|code| code.as_str() == value)
            })
            .ok_or(ErrorCode::InternalError)?;
        return Err(code);
    }
    if object
        .get("result")
        .and_then(serde_json::Value::as_object)
        .is_none()
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Readiness {
    pub transport_ready: bool,
    pub mail_ready: bool,
    pub authenticated: bool,
    pub protocol_version: u64,
}

impl<'de> Deserialize<'de> for Readiness {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{Error, MapAccess, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Readiness;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("minimal readiness object")
            }
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut keys = BTreeSet::new();
                let mut tr = None;
                let mut mr = None;
                let mut au = None;
                let mut pv = None;
                while let Some(key) = map.next_key::<String>()? {
                    if !keys.insert(key.clone()) {
                        return Err(A::Error::custom("duplicate readiness field"));
                    }
                    match key.as_str() {
                        "transport_ready" => tr = Some(map.next_value()?),
                        "mail_ready" => mr = Some(map.next_value()?),
                        "authenticated" => au = Some(map.next_value()?),
                        "protocol_version" => pv = Some(map.next_value()?),
                        _ => return Err(A::Error::custom("unknown readiness field")),
                    }
                }
                Ok(Readiness {
                    transport_ready: tr
                        .ok_or_else(|| A::Error::custom("missing transport_ready"))?,
                    mail_ready: mr.ok_or_else(|| A::Error::custom("missing mail_ready"))?,
                    authenticated: au.ok_or_else(|| A::Error::custom("missing authenticated"))?,
                    protocol_version: pv
                        .ok_or_else(|| A::Error::custom("missing protocol_version"))?,
                })
            }
        }
        deserializer.deserialize_map(V)
    }
}

#[derive(Debug)]
struct RpcEnvelope {
    jsonrpc: String,
    id: String,
    body: RpcBody,
}

#[derive(Debug)]
enum RpcBody {
    Result { result: Readiness },
    Error { error: RpcError },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RpcError {
    code: String,
    message: String,
}

impl<'de> Deserialize<'de> for RpcEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{Error, MapAccess, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = RpcEnvelope;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a strict JSON-RPC response")
            }
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut keys = BTreeSet::new();
                let mut jsonrpc = None;
                let mut id = None;
                let mut result: Option<Readiness> = None;
                let mut error: Option<RpcError> = None;
                while let Some(key) = map.next_key::<String>()? {
                    if !keys.insert(key.clone()) {
                        return Err(A::Error::custom("duplicate response field"));
                    }
                    match key.as_str() {
                        "jsonrpc" => jsonrpc = Some(map.next_value()?),
                        "id" => id = Some(map.next_value()?),
                        "result" => result = Some(map.next_value()?),
                        "error" => error = Some(map.next_value()?),
                        _ => return Err(A::Error::custom("unknown response field")),
                    }
                }
                let body = match (result, error) {
                    (Some(value), None) => RpcBody::Result { result: value },
                    (None, Some(value)) => RpcBody::Error { error: value },
                    _ => return Err(A::Error::custom("response must contain exactly one body")),
                };
                Ok(RpcEnvelope {
                    jsonrpc: jsonrpc.ok_or_else(|| A::Error::custom("missing jsonrpc"))?,
                    id: id.ok_or_else(|| A::Error::custom("missing id"))?,
                    body,
                })
            }
        }
        deserializer.deserialize_map(V)
    }
}

pub fn readiness_request(id: &str) -> Result<Vec<u8>, ErrorCode> {
    if id.is_empty() || id.chars().count() > 64 {
        return Err(ErrorCode::InvalidInput);
    }
    let value = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "system.readiness",
        "params": {"protocol_version": PROTOCOL_VERSION}
    });
    let body = serde_json::to_vec(&value).map_err(|_| ErrorCode::InternalError)?;
    if body.len() > MAX_RESPONSE {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(body)
    }
}

pub fn parse_readiness_response(bytes: &[u8], expected_id: &str) -> Result<Readiness, ErrorCode> {
    if bytes.is_empty() || bytes.len() > MAX_RESPONSE || bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(ErrorCode::CoreUnavailable);
    }
    let envelope: RpcEnvelope =
        serde_json::from_slice(bytes).map_err(|_| ErrorCode::CoreUnavailable)?;
    if envelope.jsonrpc != "2.0" || envelope.id != expected_id {
        return Err(ErrorCode::CoreUnavailable);
    }
    match envelope.body {
        RpcBody::Result { result } => {
            if result.protocol_version != PROTOCOL_VERSION
                || !result.transport_ready
                || result.mail_ready
                || result.authenticated
            {
                return Err(ErrorCode::CoreUnavailable);
            }
            Ok(result)
        }
        RpcBody::Error { error } => {
            let _ = (error.code, error.message);
            Err(ErrorCode::CoreUnavailable)
        }
    }
}

pub fn readiness_json(status: &Readiness) -> String {
    serde_json::json!({
        "schema_version": 1,
        "ok": true,
        "items": [status],
        "next_cursor": null,
        "account_results": [],
        "completeness": "complete",
        "warnings": [],
        "observed_at": null
    })
    .to_string()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn core_status() -> Result<Readiness, ErrorCode> {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .block_on(async {
            use hantong_platform::unix_transport::connect;
            tokio::time::timeout(tokio::time::Duration::from_secs(5), async {
                let connection = connect().await.map_err(|error| {
                    if error == ErrorCode::NotFound {
                        ErrorCode::CoreUnavailable
                    } else {
                        error
                    }
                })?;
                exchange_readiness(connection, "core-status-1").await
            })
            .await
            .map_err(|_| ErrorCode::CoreUnavailable)?
        })
}

/// Exchange readiness on an already connected Unix transport.  Callers that
/// are already inside a Tokio runtime must use this entry point so that a
/// second runtime is never nested around the same connection.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub async fn core_status_on_connection(
    connection: hantong_platform::unix_transport::UnixLocalConnection,
) -> Result<Readiness, ErrorCode> {
    if !connection.peer_matches_current_image()? {
        return Err(ErrorCode::ScopeDenied);
    }
    exchange_readiness(connection, "core-status-1").await
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn core_status() -> Result<Readiness, ErrorCode> {
    Err(ErrorCode::CapabilityUnsupported)
}

#[cfg(windows)]
pub fn core_status() -> Result<Readiness, ErrorCode> {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .block_on(core_status_async())
}

#[cfg(windows)]
async fn core_status_async() -> Result<Readiness, ErrorCode> {
    use hantong_platform::connect;
    use tokio::time::{Duration, timeout};

    timeout(Duration::from_secs(5), async {
        exchange_readiness(connect().await?, "core-status-1").await
    })
    .await
    .map_err(|_| ErrorCode::CoreUnavailable)?
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
async fn exchange_readiness<T>(mut pipe: T, id: &str) -> Result<Readiness, ErrorCode>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let request = readiness_request(id)?;
    let frame_len = u32::try_from(request.len()).map_err(|_| ErrorCode::InvalidInput)?;
    pipe.write_all(&frame_len.to_le_bytes())
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    pipe.write_all(&request)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    pipe.flush().await.map_err(|_| ErrorCode::CoreUnavailable)?;

    let mut header = [0u8; 4];
    pipe.read_exact(&mut header)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let length = u32::from_le_bytes(header) as usize;
    if !(1..=MAX_RESPONSE).contains(&length) {
        return Err(ErrorCode::CoreUnavailable);
    }
    let mut response = vec![0u8; length];
    pipe.read_exact(&mut response)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let status = parse_readiness_response(&response, id)?;
    // Named-pipe disconnect discards unread bytes. This receipt is only a
    // transport acknowledgement, never an approval or authenticated claim.
    pipe.write_all(&[0x06])
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    pipe.flush().await.map_err(|_| ErrorCode::CoreUnavailable)?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli06_request_is_fixed_and_bounded() {
        let request = String::from_utf8(readiness_request("r1").unwrap()).unwrap();
        assert!(request.contains("system.readiness"));
        assert!(request.contains("protocol_version"));
    }

    #[test]
    fn cli06_accepts_only_minimal_readiness() {
        let input = br#"{"jsonrpc":"2.0","id":"r1","result":{"transport_ready":true,"mail_ready":false,"authenticated":false,"protocol_version":1}}"#;
        assert!(!parse_readiness_response(input, "r1").unwrap().mail_ready);
    }

    #[test]
    fn cli06_rejects_extra_fields_duplicate_keys_and_wrong_state() {
        let extra = br#"{"jsonrpc":"2.0","id":"r1","result":{"transport_ready":true,"mail_ready":false,"authenticated":false,"protocol_version":1,"accounts":0}}"#;
        assert_eq!(
            parse_readiness_response(extra, "r1"),
            Err(ErrorCode::CoreUnavailable)
        );
        let duplicate = br#"{"jsonrpc":"2.0","id":"r1","result":{"transport_ready":true,"mail_ready":false,"authenticated":false,"protocol_version":1,"mail_ready":false}}"#;
        assert_eq!(
            parse_readiness_response(duplicate, "r1"),
            Err(ErrorCode::CoreUnavailable)
        );
        let wrong = br#"{"jsonrpc":"2.0","id":"r1","result":{"transport_ready":true,"mail_ready":true,"authenticated":false,"protocol_version":1}}"#;
        assert_eq!(
            parse_readiness_response(wrong, "r1"),
            Err(ErrorCode::CoreUnavailable)
        );
    }

    #[test]
    fn cli06_json_status_uses_the_common_success_envelope() {
        let status = Readiness {
            transport_ready: true,
            mail_ready: false,
            authenticated: false,
            protocol_version: 1,
        };
        let json = readiness_json(&status);
        assert!(json.contains("\"schema_version\":1"));
        assert!(json.contains("\"ok\":true"));
        assert!(json.contains("\"items\":[{"));
        assert!(json.contains("\"completeness\":\"complete\""));
        assert!(!json.contains("account_id"));
    }

    #[cfg(windows)]
    #[test]
    fn cli06_duplex_sends_transport_ack_after_valid_response() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (client, mut server) = tokio::io::duplex(64 * 1024);
            let task = tokio::spawn(async move {
                let mut header = [0u8; 4];
                server.read_exact(&mut header).await.unwrap();
                let length = u32::from_le_bytes(header) as usize;
                let mut request = vec![0u8; length];
                server.read_exact(&mut request).await.unwrap();
                let response = br#"{"jsonrpc":"2.0","id":"r1","result":{"transport_ready":true,"mail_ready":false,"authenticated":false,"protocol_version":1}}"#;
                server.write_all(&(response.len() as u32).to_le_bytes()).await.unwrap();
                server.write_all(response).await.unwrap();
                server.flush().await.unwrap();
                let mut ack = [0u8; 1];
                server.read_exact(&mut ack).await.unwrap();
                ack[0]
            });
            let status = exchange_readiness(client, "r1").await.unwrap();
            assert!(!status.mail_ready);
            assert_eq!(task.await.unwrap(), 0x06);
        });
    }
}

pub mod scope_refresh;

pub mod google_mail;
pub mod google_setup;
pub mod smtp_setup;
pub mod terminal_review;

#[cfg(all(test, target_os = "linux"))]
mod unix_core_status_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    #[test]
    fn cli07_missing_runtime_socket_maps_to_core_unavailable() {
        if std::env::var_os("HANTONG_CORE_STATUS_CHILD").is_some() {
            assert_eq!(core_status(), Err(ErrorCode::CoreUnavailable));
            return;
        }
        let root = std::env::temp_dir().join(format!("hantong-core-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("runtime root");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .expect("private runtime root");
        let status = Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("unix_core_status_tests::cli07_missing_runtime_socket_maps_to_core_unavailable")
            .arg("--nocapture")
            .env("HANTONG_CORE_STATUS_CHILD", "1")
            .env("XDG_RUNTIME_DIR", &root)
            .status()
            .expect("child test");
        let _ = std::fs::remove_dir_all(root);
        assert!(status.success());
    }
}
