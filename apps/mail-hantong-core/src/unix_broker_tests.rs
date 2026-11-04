//! Narrow protocol regressions for the Unix Core relay entrypoint.
//!
//! This file is included by `unix_core_runtime` so it can exercise the same
//! private runtime boundary without adding a second production transport or
//! test-only bypass.  Socket lifecycle and authenticated dispatch remain in
//! the existing Unix transport/auth tests.

use hantong_contracts::ErrorCode;
use hantong_ipc_client::unix_relay::{MAX_FRAME, RelayProfile, RelayRequest};
use hantong_ipc_server::{decode_core_relay_request, decode_request};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;

fn frame(value: &Value) -> Vec<u8> {
    let bytes = serde_json::to_vec(value).expect("test JSON");
    let mut framed = (bytes.len() as u32).to_le_bytes().to_vec();
    framed.extend_from_slice(&bytes);
    framed
}

#[test]
fn sec03_core_relay_preserves_request_id_and_method_bounds() {
    let relay = RelayRequest::new(
        RelayProfile::Default,
        "broker-1",
        "mail.accounts.list",
        json!({"account_ids":["account_a"]}),
    )
    .expect("valid relay");
    let outer = json!({
        "jsonrpc":"2.0",
        "id":"broker-1",
        "method":"core.relay",
        "params":serde_json::to_value(relay).expect("relay JSON")
    });
    let decoded = decode_core_relay_request(&frame(&outer)).expect("core relay frame");
    assert_eq!(decoded.id, "broker-1");
    assert_eq!(decoded.method, "core.relay");
    assert_eq!(decoded.params["request_id"], "broker-1");
    assert_eq!(decoded.params["method"], "mail.accounts.list");

    let wrong_method = json!({
        "jsonrpc":"2.0",
        "id":"broker-1",
        "method":"mail.accounts.list",
        "params":{}
    });
    assert_eq!(
        decode_core_relay_request(&frame(&wrong_method)),
        Err(ErrorCode::InvalidInput)
    );
}

#[test]
fn sec03_core_relay_rejects_oversized_or_duplicate_outer_frames() {
    let oversized = json!({
        "jsonrpc":"2.0",
        "id":"broker-1",
        "method":"core.relay",
        "params":{"padding":"x".repeat(1_050_113)}
    });
    assert_eq!(
        decode_core_relay_request(&frame(&oversized)),
        Err(ErrorCode::InvalidInput)
    );

    let duplicate = br#"{"jsonrpc":"2.0","id":"broker-1","method":"core.relay","params":{"profile":"default","profile":"mcp"}}"#;
    let mut framed = (duplicate.len() as u32).to_le_bytes().to_vec();
    framed.extend_from_slice(duplicate);
    assert_eq!(
        decode_core_relay_request(&framed),
        Err(ErrorCode::InvalidInput)
    );

    let ordinary = frame(&json!({
        "jsonrpc":"2.0",
        "id":"broker-1",
        "method":"core.relay",
        "params":{"padding":"x".repeat(MAX_FRAME)}
    }));
    assert_eq!(decode_request(&ordinary), Err(ErrorCode::InvalidInput));
}

fn isolated_child(test_name: &str) -> bool {
    if std::env::var_os("HANTONG_UNIX_BROKER_CHILD").is_some() {
        return false;
    }
    let root = std::env::temp_dir().join(format!(
        "hantong-unix-broker-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir(&root).expect("runtime directory");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
        .expect("private runtime directory");
    let status = Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env("HANTONG_UNIX_BROKER_CHILD", "1")
        .env("XDG_RUNTIME_DIR", &root)
        .status()
        .expect("child test");
    let _ = std::fs::remove_dir_all(root);
    assert!(status.success());
    true
}

#[tokio::test(flavor = "current_thread")]
async fn sec03_core_broker_readiness_round_trip_and_ack() {
    if isolated_child(
        "unix_core_runtime::broker_tests::sec03_core_broker_readiness_round_trip_and_ack",
    ) {
        return;
    }
    let server = hantong_platform::unix_transport::UnixLocalServer::bind().expect("bind");
    let server_task = async move {
        let mut connection = server.accept().await.expect("accept");
        let request = super::read_first(&mut connection, 1_050_112)
            .await
            .expect("readiness request");
        assert_eq!(request.id, "broker-readiness-1");
        assert_eq!(request.method, "system.readiness");
        let response = hantong_ipc_server::bootstrap::response(request);
        super::send_value(&mut connection, &response)
            .await
            .expect("response and ack");
    };
    let client_task = async {
        let mut connection = hantong_platform::unix_transport::connect()
            .await
            .expect("connect");
        let request = hantong_ipc_client::readiness_request("broker-readiness-1").unwrap();
        hantong_ipc_client::unix_relay::write_frame(&mut connection, &request)
            .await
            .expect("write request");
        let response = hantong_ipc_client::unix_relay::read_frame(&mut connection)
            .await
            .expect("read response");
        let readiness =
            hantong_ipc_client::parse_readiness_response(&response, "broker-readiness-1")
                .expect("readiness response");
        assert!(readiness.transport_ready);
        connection.write_u8(0x06).await.expect("ack");
        connection.flush().await.expect("flush ack");
    };
    tokio::join!(server_task, client_task);
}

#[tokio::test(flavor = "current_thread")]
async fn sec03_core_broker_rejects_oversized_first_frame() {
    if isolated_child(
        "unix_core_runtime::broker_tests::sec03_core_broker_rejects_oversized_first_frame",
    ) {
        return;
    }
    let server = hantong_platform::unix_transport::UnixLocalServer::bind().expect("bind");
    let server_task = async move {
        let mut connection = server.accept().await.expect("accept");
        assert_eq!(
            super::read_first(&mut connection, 1_050_112).await,
            Err(ErrorCode::InvalidInput)
        );
    };
    let client_task = async {
        let mut connection = hantong_platform::unix_transport::connect()
            .await
            .expect("connect");
        connection
            .write_u32_le(1_050_113)
            .await
            .expect("write oversized length");
        connection.flush().await.expect("flush length");
    };
    tokio::join!(server_task, client_task);
}
