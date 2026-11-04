//! Verified Unix Core stop entrypoint used during self-update.
//!
//! This command has no provider, Store, signer, or secret access.  It only
//! proves the installed image and asks the current Core socket which kernel
//! peer is the Core process to stop.

use hantong_contracts::ErrorCode;
#[cfg(target_os = "macos")]
use serde::Deserialize;
#[cfg(target_os = "macos")]
use serde_json::json;
#[cfg(target_os = "macos")]
use std::time::Duration;
#[cfg(target_os = "macos")]
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(crate) fn run() -> Result<(), ErrorCode> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    runtime.block_on(run_async())
}

pub(crate) async fn run_async() -> Result<(), ErrorCode> {
    let image = hantong_platform::unix_release::verified_installed_core("mail-hantong-core")?;
    let Some(connection) = connect_if_running().await? else {
        return Ok(());
    };
    let peer = connection.peer_context().clone();
    #[cfg(target_os = "macos")]
    {
        request_macos_stop(connection, &peer, &image).await
    }
    #[cfg(target_os = "linux")]
    {
        drop(connection);
        hantong_platform::unix_lifecycle::stop_verified_core(&peer, &image)
    }
}

#[cfg(target_os = "macos")]
async fn request_macos_stop(
    mut connection: hantong_platform::unix_transport::UnixLocalConnection,
    _peer: &hantong_contracts::unix_auth::UnixPeerContextV2,
    verified_image: &std::path::Path,
) -> Result<(), ErrorCode> {
    if !connection.peer_matches_current_image()? {
        return Err(ErrorCode::ScopeDenied);
    }
    let expected = std::fs::canonicalize(verified_image).map_err(|_| ErrorCode::CoreUnavailable)?;
    let current =
        std::fs::canonicalize(std::env::current_exe().map_err(|_| ErrorCode::CoreUnavailable)?)
            .map_err(|_| ErrorCode::CoreUnavailable)?;
    if expected != current {
        return Err(ErrorCode::ScopeDenied);
    }
    let request_id = "unix-stop-v1";
    let request = json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "core.stop",
        "params": { "protocol_version": 1 }
    });
    let body = serde_json::to_vec(&request).map_err(|_| ErrorCode::InternalError)?;
    if body.is_empty() || body.len() > hantong_ipc_client::unix_relay::MAX_FRAME {
        return Err(ErrorCode::QuotaExceeded);
    }
    connection
        .write_u32_le(body.len() as u32)
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
    let length = tokio::time::timeout(Duration::from_secs(5), connection.read_u32_le())
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .map_err(|_| ErrorCode::CoreUnavailable)? as usize;
    if !(1..=hantong_ipc_client::unix_relay::MAX_FRAME).contains(&length) {
        return Err(ErrorCode::InvalidInput);
    }
    let mut response = vec![0_u8; length];
    tokio::time::timeout(Duration::from_secs(5), connection.read_exact(&mut response))
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    validate_stop_response(&response, request_id)?;
    connection
        .write_u8(0x06)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    connection
        .flush()
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    drop(connection);
    wait_for_socket_disappearance().await
}

#[cfg(target_os = "macos")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StopResponse {
    jsonrpc: String,
    id: String,
    result: StopResult,
}

#[cfg(target_os = "macos")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StopResult {
    state: String,
    protocol_version: u8,
}

#[cfg(target_os = "macos")]
fn validate_stop_response(response: &[u8], request_id: &str) -> Result<(), ErrorCode> {
    let value: StopResponse =
        serde_json::from_slice(response).map_err(|_| ErrorCode::InvalidInput)?;
    if value.jsonrpc != "2.0"
        || value.id != request_id
        || value.result.state != "stopping"
        || value.result.protocol_version != 1
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
async fn wait_for_socket_disappearance() -> Result<(), ErrorCode> {
    let path = hantong_platform::unix_paths::socket_path()?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if !path.exists() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(ErrorCode::Conflict);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// An updater may legitimately find that the old Core already exited.  Treat
/// only a missing endpoint as that state; an existing malformed or hostile
/// endpoint must still go through the normal transport admission checks.
async fn connect_if_running()
-> Result<Option<hantong_platform::unix_transport::UnixLocalConnection>, ErrorCode> {
    let path = hantong_platform::unix_paths::socket_path()?;
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ErrorCode::CoreUnavailable),
        Ok(_) => {}
    }
    match hantong_platform::unix_transport::connect().await {
        Ok(connection) => Ok(Some(connection)),
        Err(ErrorCode::CoreUnavailable) if !path.exists() => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn isolated_missing_socket() -> bool {
        if std::env::var_os("HANTONG_STOP_CHILD").is_some() {
            return false;
        }
        let root = std::env::temp_dir().join(format!(
            "hantong-stop-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir(&root).expect("runtime root");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .expect("private runtime root");
        let status = Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("unix_stop::tests::cli07_missing_core_socket_is_a_noop")
            .arg("--nocapture")
            .env("HANTONG_STOP_CHILD", "1")
            .env("XDG_RUNTIME_DIR", &root)
            .status()
            .expect("child test");
        let _ = std::fs::remove_dir_all(root);
        assert!(status.success());
        true
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cli07_stop_response_requires_exact_id_and_state() {
        let good = serde_json::to_vec(&serde_json::json!({
            "jsonrpc":"2.0", "id":"unix-stop-v1",
            "result":{"state":"stopping", "protocol_version":1}
        }))
        .unwrap();
        assert!(super::validate_stop_response(&good, "unix-stop-v1").is_ok());
        let bad = serde_json::to_vec(&serde_json::json!({
            "jsonrpc":"2.0", "id":"other",
            "result":{"state":"stopping", "protocol_version":1}
        }))
        .unwrap();
        assert_eq!(
            super::validate_stop_response(&bad, "unix-stop-v1"),
            Err(ErrorCode::InvalidInput)
        );
        let duplicate = br#"{"jsonrpc":"2.0","jsonrpc":"2.0","id":"unix-stop-v1","result":{"state":"stopping","protocol_version":1}}"#;
        assert_eq!(
            super::validate_stop_response(duplicate, "unix-stop-v1"),
            Err(ErrorCode::InvalidInput)
        );
    }

    #[tokio::test]
    async fn cli07_missing_core_socket_is_a_noop() {
        if isolated_missing_socket() {
            return;
        }
        let path = hantong_platform::unix_paths::socket_path().expect("socket path");
        assert!(!path.exists());
        assert!(super::connect_if_running().await.unwrap().is_none());
    }
}
