//! Bounded one-request relay framing for Unix clients.
//!
//! This module deliberately owns no signing key and has no Secrets dependency.
//! A Core-owned helper supplies an already-authenticated connection and uses
//! this framing only to carry one fixed-profile request and its bounded Core
//! response. It is not a signing or approval API.

#![cfg(unix)]

use hantong_contracts::ErrorCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{Read, Write};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_FRAME: usize = 1_049_600;
pub const MAX_REQUEST_ID: usize = 64;
pub const MAX_METHOD: usize = 128;
const MAX_CHILD_LIFETIME: Duration = Duration::from_secs(40);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum RelayProfile {
    #[serde(rename = "default")]
    Default,
    #[serde(rename = "mcp")]
    Mcp,
    #[serde(rename = "ai-codex")]
    AiCodex,
    #[serde(rename = "ai-claude")]
    AiClaude,
}

impl RelayProfile {
    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        match value {
            "default" => Ok(Self::Default),
            "mcp" => Ok(Self::Mcp),
            "ai-codex" => Ok(Self::AiCodex),
            "ai-claude" => Ok(Self::AiClaude),
            _ => Err(ErrorCode::InvalidInput),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Mcp => "mcp",
            Self::AiCodex => "ai-codex",
            Self::AiClaude => "ai-claude",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RelayRequest {
    pub protocol_version: u32,
    pub profile: RelayProfile,
    pub request_id: String,
    pub method: String,
    pub params: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRequest {
    protocol_version: u32,
    profile: String,
    request_id: String,
    method: String,
    params: Value,
}

impl RelayRequest {
    pub fn new(
        profile: RelayProfile,
        request_id: impl Into<String>,
        method: impl Into<String>,
        params: Value,
    ) -> Result<Self, ErrorCode> {
        let request_id = request_id.into();
        let method = method.into();
        let params = normalize_application_params(&request_id, &method, params)?;
        let request = Self {
            protocol_version: 1,
            profile,
            request_id,
            method,
            params,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), ErrorCode> {
        if self.protocol_version != 1
            || self.request_id.is_empty()
            || self.request_id.len() > MAX_REQUEST_ID
            || self.method.is_empty()
            || self.method.len() > MAX_METHOD
            || !ascii_token(&self.request_id)
            || !ascii_token(&self.method)
            || (!self.method.starts_with("mail.")
                && !terminal_review_method(&self.method)
                && !bootstrap_method(&self.method))
            || !self.params.is_object()
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, ErrorCode> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| ErrorCode::InvalidInput)?;
        if bytes.is_empty() || bytes.len() > MAX_FRAME {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ErrorCode> {
        if bytes.is_empty() || bytes.len() > MAX_FRAME {
            return Err(ErrorCode::InvalidInput);
        }
        let wire: WireRequest =
            serde_json::from_slice(bytes).map_err(|_| ErrorCode::InvalidInput)?;
        let request = Self {
            protocol_version: wire.protocol_version,
            profile: RelayProfile::parse(&wire.profile)?,
            request_id: wire.request_id,
            method: wire.method,
            params: wire.params,
        };
        request.validate()?;
        if request.encode()?.as_slice() != bytes {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(request)
    }
}

/// Application execution receives a ToolRequest envelope inside JSON-RPC
/// `params`. Setup, registration, and terminal review methods deliberately
/// retain their typed raw payloads and are handled by their own dispatcher.
fn normalize_application_params(
    request_id: &str,
    method: &str,
    params: Value,
) -> Result<Value, ErrorCode> {
    if !method.starts_with("mail.") && !method.starts_with("app.") {
        return Ok(params);
    }
    let Some(object) = params.as_object() else {
        return Err(ErrorCode::InvalidInput);
    };
    let full = object.contains_key("schema_version")
        || object.contains_key("request_id")
        || object.contains_key("tool")
        || object.contains_key("params");
    if full {
        if object.len() != 4
            || object.get("schema_version") != Some(&Value::from(1))
            || object.get("request_id").and_then(Value::as_str) != Some(request_id)
            || object.get("tool").and_then(Value::as_str) != Some(method)
            || !object.get("params").is_some_and(Value::is_object)
        {
            return Err(ErrorCode::InvalidInput);
        }
        return Ok(params);
    }
    Ok(serde_json::json!({
        "schema_version": 1,
        "request_id": request_id,
        "tool": method,
        "params": params,
    }))
}

fn bootstrap_method(method: &str) -> bool {
    matches!(
        method,
        "registration.request"
            | "registration.status"
            | "registration.google_readonly.request"
            | "registration.mcp.request"
            | "registration.mcp.status"
            | "registration.mcp.v3.request"
            | "registration.mcp.v3.status"
            | "registration.gui.request"
            | "registration.gui.status"
            | "registration.ai.request"
            | "registration.ai.status"
            | "registration.scope.request"
            | "registration.scope.status"
            | "registration.scope.manage.request"
            | "registration.scope.google_send.request"
            | "registration.scope.google_changes.request"
            | "registration.scope.google_readonly.request"
            | "registration.scope.google_readonly.add"
            | "setup.google.request"
            | "setup.google.status"
            | "setup.google.client_ticket"
            | "setup.google.stage_client"
            | "setup.imap.request"
            | "setup.imap.status"
            | "setup.imap.resume"
            | "setup.smtp.request"
            | "setup.smtp.status"
            | "app.settings.display.get"
            | "app.diagnostics.snapshot"
    )
}

fn terminal_review_method(method: &str) -> bool {
    matches!(
        method,
        "terminal.review.get"
            | "terminal.review.confirm"
            | "terminal.review.cancel"
            | "terminal.review.secret_submit"
    )
}

fn ascii_token(value: &str) -> bool {
    value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Vec<u8>, ErrorCode> {
    let length = reader
        .read_u32_le()
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)? as usize;
    if !(1..=MAX_FRAME).contains(&length) {
        return Err(ErrorCode::InvalidInput);
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    Ok(body)
}

pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    body: &[u8],
) -> Result<(), ErrorCode> {
    if body.is_empty() || body.len() > MAX_FRAME {
        return Err(ErrorCode::InvalidInput);
    }
    let length = u32::try_from(body.len()).map_err(|_| ErrorCode::InvalidInput)?;
    writer
        .write_u32_le(length)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    writer
        .write_all(body)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    writer.flush().await.map_err(|_| ErrorCode::CoreUnavailable)
}

/// Performs exactly one relay request. A partial response is terminal; this
/// function never retries and never exposes an authentication signature.
pub async fn exchange<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut R,
    writer: &mut W,
    request: &RelayRequest,
) -> Result<Vec<u8>, ErrorCode> {
    let body = request.encode()?;
    write_frame(writer, &body).await?;
    read_frame(reader).await
}

/// Sends one relay request to the already-running Core broker. The broker is
/// accepted only when the kernel-observed peer is the verified Core image.
/// This path is used by the Core foreground helper so terminal Get/Confirm
/// retain one Core process identity.
pub async fn broker_exchange(request: &RelayRequest) -> Result<Vec<u8>, ErrorCode> {
    let mut connection = hantong_platform::unix_transport::connect().await?;
    if !connection.peer_matches_current_image()? {
        return Err(ErrorCode::ScopeDenied);
    }
    let params = serde_json::to_value(request).map_err(|_| ErrorCode::InvalidInput)?;
    let outer = serde_json::json!({
        "jsonrpc":"2.0",
        "id":request.request_id,
        "method":"core.relay",
        "params":params
    });
    let body = serde_json::to_vec(&outer).map_err(|_| ErrorCode::InvalidInput)?;
    if body.len() > 1_050_112 {
        return Err(ErrorCode::InvalidInput);
    }
    connection
        .write_u32_le(u32::try_from(body.len()).map_err(|_| ErrorCode::InvalidInput)?)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    connection
        .write_all(&body)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    connection
        .flush()
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let response = read_frame(&mut connection).await?;
    validate_response(&response)?;
    let value: Value = serde_json::from_slice(&response).map_err(|_| ErrorCode::CoreUnavailable)?;
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || value.get("id").and_then(Value::as_str) != Some(request.request_id.as_str())
    {
        return Err(ErrorCode::CoreUnavailable);
    }
    connection
        .write_all(&[0x06])
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    Ok(response)
}

/// Strictly validates a returned Core response without interpreting its mail
/// payload. The Core response remains opaque to the relay transport layer.
pub fn validate_response(bytes: &[u8]) -> Result<(), ErrorCode> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME || bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(ErrorCode::CoreUnavailable);
    }
    let _: Value = serde_json::from_slice(bytes).map_err(|_| ErrorCode::CoreUnavailable)?;
    Ok(())
}

/// Starts the verified sibling Core helper for exactly one request. The helper
/// forwards to the Core broker; this process receives only the bounded JSON-RPC
/// result. A failed child is terminal and is never retried.
pub fn call(
    profile: crate::profile::ProfileSelector,
    request_id: &str,
    method: &str,
    params: &Value,
) -> Result<Vec<u8>, ErrorCode> {
    let request = RelayRequest::new(
        RelayProfile::parse(profile.as_str())?,
        request_id.to_owned(),
        method.to_owned(),
        params.clone(),
    )?;
    #[cfg(target_os = "linux")]
    let caller = {
        let asset = hantong_platform::linux_identity::current_asset()?;
        asset
            .strip_prefix("bin/")
            .ok_or(ErrorCode::CoreUnavailable)?
            .to_owned()
    };
    #[cfg(not(target_os = "linux"))]
    let caller = std::env::current_exe()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ErrorCode::CoreUnavailable)?
        .to_ascii_lowercase();
    if caller == "mail-hantong-core" {
        #[cfg(target_os = "linux")]
        hantong_platform::linux_identity::ensure_entry("bin/mail-hantong-core")?;
        let _ = hantong_platform::unix_release::verified_installed_core(&caller)?;
        return tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| ErrorCode::CoreUnavailable)?
            .block_on(async {
                tokio::time::timeout(MAX_CHILD_LIFETIME, broker_exchange(&request)).await
            })
            .map_err(|_| ErrorCode::CoreUnavailable)?;
    }
    let core = hantong_platform::unix_release::verified_core_command(&caller)?;
    let frame = request.encode()?;
    let body = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .block_on(async {
            tokio::time::timeout(
                MAX_CHILD_LIFETIME,
                tokio::task::spawn_blocking(move || run_child(core, frame)),
            )
            .await
        })
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .map_err(|_| ErrorCode::CoreUnavailable)??;
    body.pipe_response(&request)
}

fn run_child(
    mut core: hantong_platform::unix_release::VerifiedCoreCommand,
    frame: Vec<u8>,
) -> Result<Vec<u8>, ErrorCode> {
    let mut child = core
        .arg("unix-relay")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let mut input = child.stdin.take().ok_or(ErrorCode::CoreUnavailable)?;
    let mut output = child.stdout.take().ok_or(ErrorCode::CoreUnavailable)?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = read_sync_frame(&mut output);
        let _ = sender.send(result);
    });
    let mut bytes = Vec::with_capacity(frame.len() + 4);
    bytes.extend_from_slice(
        &u32::try_from(frame.len())
            .map_err(|_| ErrorCode::InvalidInput)?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&frame);
    input
        .write_all(&bytes)
        .and_then(|_| input.flush())
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    drop(input);
    let deadline = Instant::now() + MAX_CHILD_LIFETIME;
    let body = loop {
        if let Ok(Some(_)) = child.try_wait() {
            break receiver
                .recv_timeout(Duration::from_millis(50))
                .map_err(|_| ErrorCode::CoreUnavailable)??;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ErrorCode::CoreUnavailable);
        }
        if let Ok(body) = receiver.try_recv() {
            break body?;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let _ = child.wait();
    Ok(body)
}

fn read_sync_frame(reader: &mut impl Read) -> Result<Vec<u8>, ErrorCode> {
    let mut header = [0u8; 4];
    reader
        .read_exact(&mut header)
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let length = u32::from_le_bytes(header) as usize;
    if !(1..=MAX_FRAME).contains(&length) {
        return Err(ErrorCode::CoreUnavailable);
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    Ok(body)
}

trait RelayResponseExt {
    fn pipe_response(self, request: &RelayRequest) -> Result<Vec<u8>, ErrorCode>;
}

impl RelayResponseExt for Vec<u8> {
    fn pipe_response(self, request: &RelayRequest) -> Result<Vec<u8>, ErrorCode> {
        validate_response(&self)?;
        let value: Value = serde_json::from_slice(&self).map_err(|_| ErrorCode::CoreUnavailable)?;
        if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || value.get("id").and_then(Value::as_str) != Some(request.request_id.as_str())
        {
            return Err(ErrorCode::CoreUnavailable);
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    fn request() -> RelayRequest {
        RelayRequest::new(
            RelayProfile::Default,
            "relay-1",
            "mail.accounts.list",
            serde_json::json!({"account_ids":["account_a"]}),
        )
        .unwrap()
    }

    #[test]
    fn cli07_relay_request_is_canonical_and_bounded() {
        let request = request();
        let bytes = request.encode().unwrap();
        assert_eq!(RelayRequest::decode(&bytes).unwrap(), request);
        assert!(RelayRequest::decode(
            br#"{"protocol_version":1,"profile":"default","request_id":"relay-1","method":"mail.accounts.list","params":{},"extra":1}"#
        )
        .is_err());
    }

    #[test]
    fn cli07_mail_relay_wraps_application_params_once_for_tool_execution() {
        let params = serde_json::json!({
            "account_ids": ["account_a"],
            "include_capabilities": true
        });
        let request = RelayRequest::new(
            RelayProfile::Default,
            "accounts-list-1",
            "mail.accounts.list",
            params.clone(),
        )
        .unwrap();
        assert_eq!(request.params.get("schema_version"), Some(&Value::from(1)));
        assert_eq!(
            request.params.get("request_id").and_then(Value::as_str),
            Some("accounts-list-1")
        );
        assert_eq!(
            request.params.get("tool").and_then(Value::as_str),
            Some("mail.accounts.list")
        );
        assert_eq!(request.params.get("params"), Some(&params));
    }

    #[test]
    fn cli07_relay_rejects_arbitrary_profile_and_unknown_method() {
        assert!(RelayProfile::parse("path:/tmp/profile.json").is_err());
        assert!(
            RelayRequest::new(
                RelayProfile::Default,
                "relay-1",
                "admin.secret.export",
                serde_json::json!({}),
            )
            .is_err()
        );
    }

    #[test]
    fn cli07_relay_accepts_typed_bootstrap_and_display_methods() {
        for method in [
            "registration.request",
            "setup.google.request",
            "terminal.review.get",
            "terminal.review.confirm",
            "terminal.review.cancel",
            "terminal.review.secret_submit",
            "app.settings.display.get",
        ] {
            assert!(
                RelayRequest::new(
                    RelayProfile::Default,
                    "relay-1",
                    method,
                    serde_json::json!({"protocol_version":1}),
                )
                .is_ok()
            );
        }
        assert!(
            RelayRequest::new(
                RelayProfile::Default,
                "relay-1",
                "terminal.review.unknown",
                serde_json::json!({}),
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn cli07_relay_is_one_request_without_retry() {
        let (client, mut helper) = duplex(4096);
        let (mut client_reader, mut client_writer) = tokio::io::split(client);
        let task = tokio::spawn(async move {
            let request = RelayRequest::decode(&read_frame(&mut helper).await.unwrap()).unwrap();
            assert_eq!(request.method, "mail.accounts.list");
            write_frame(&mut helper, br#"{"jsonrpc":"2.0","ok":true}"#)
                .await
                .unwrap();
        });
        let body = exchange(&mut client_reader, &mut client_writer, &request())
            .await
            .unwrap();
        assert_eq!(body, br#"{"jsonrpc":"2.0","ok":true}"#);
        task.await.unwrap();
    }

    #[test]
    fn cli07_response_validation_rejects_non_json_and_bom() {
        assert!(validate_response(br#"{"ok":true}"#).is_ok());
        assert_eq!(
            validate_response(b"not-json"),
            Err(ErrorCode::CoreUnavailable)
        );
        assert_eq!(
            validate_response(b"\xef\xbb\xbf{}"),
            Err(ErrorCode::CoreUnavailable)
        );
    }

    #[test]
    fn cli07_response_id_mismatch_and_eof_fail_closed() {
        let request = request();
        assert_eq!(
            vec![].pipe_response(&request),
            Err(ErrorCode::CoreUnavailable)
        );
        let wrong = serde_json::to_vec(&serde_json::json!({
            "jsonrpc":"2.0", "id":"other", "result":{}
        }))
        .unwrap();
        assert_eq!(
            wrong.pipe_response(&request),
            Err(ErrorCode::CoreUnavailable)
        );
    }
}
