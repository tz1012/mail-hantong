use super::*;
use hantong_contracts::account_auth::{AccountAuthRevision, AccountAuthTranscript};
use hantong_contracts::account_scope::{AccountGrantV2, AccountScopeV2};
use hantong_contracts::auth::SignerRole;
use hantong_contracts::platform::{OpaqueSigner, PlatformError};
use hantong_contracts::wire_hex;
use hantong_platform::unix_transport::{self, UnixLocalConnection, UnixLocalServer};
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Signer(EcdsaKeyPair);
impl Signer {
    fn new() -> Self {
        let bytes = EcdsaKeyPair::generate_pkcs8(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            &ring::rand::SystemRandom::new(),
        )
        .expect("key");
        Self(
            EcdsaKeyPair::from_pkcs8(
                &ECDSA_P256_SHA256_FIXED_SIGNING,
                bytes.as_ref(),
                &ring::rand::SystemRandom::new(),
            )
            .expect("parse key"),
        )
    }
    fn blob(&self) -> [u8; 72] {
        let point = self.0.public_key().as_ref();
        let mut out = [0; 72];
        out[..4].copy_from_slice(b"ECS1");
        out[4..8].copy_from_slice(&32_u32.to_le_bytes());
        out[8..].copy_from_slice(&point[1..]);
        out
    }
}
impl OpaqueSigner for Signer {
    fn public_key(&self) -> Result<[u8; 72], PlatformError> {
        Ok(self.blob())
    }
    fn sign(&mut self, bytes: &[u8]) -> Result<[u8; 64], PlatformError> {
        self.0
            .sign(&ring::rand::SystemRandom::new(), bytes)
            .map_err(|_| PlatformError::Denied)
            .and_then(|sig| sig.as_ref().try_into().map_err(|_| PlatformError::Denied))
    }
}

fn full_scope() -> AccountScopeV2 {
    AccountScopeV2::new(
        vec![
            "app.diagnostics.snapshot".into(),
            "app.settings.display.get".into(),
            "mail.accounts.list".into(),
        ],
        vec![
            AccountGrantV2::new(
                "account_a".into(),
                vec!["mail.messages.get".into(), "mail.messages.search".into()],
                false,
            )
            .expect("grant"),
        ],
    )
    .expect("scope")
}
fn requested_scope() -> AccountScopeV2 {
    AccountScopeV2::new(
        vec![
            "app.diagnostics.snapshot".into(),
            "app.settings.display.get".into(),
            "mail.accounts.list".into(),
        ],
        vec![
            AccountGrantV2::new("account_a".into(), vec!["mail.messages.get".into()], false)
                .expect("grant"),
        ],
    )
    .expect("scope")
}
fn reduced_scope() -> AccountScopeV2 {
    AccountScopeV2::new(
        vec!["mail.accounts.list".into()],
        vec![
            AccountGrantV2::new(
                "account_a".into(),
                vec!["mail.messages.search".into()],
                false,
            )
            .expect("grant"),
        ],
    )
    .expect("scope")
}
fn isolated_runtime() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "hantong-ipc-model2-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir(&root).expect("runtime dir");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).expect("mode");
    root
}

async fn write_frame(connection: &mut UnixLocalConnection, value: &Value) {
    let body = serde_json::to_vec(value).expect("json");
    connection
        .write_u32_le(body.len() as u32)
        .await
        .expect("length");
    connection.write_all(&body).await.expect("body");
    connection.flush().await.expect("flush");
}
async fn read_frame(connection: &mut UnixLocalConnection) -> Vec<u8> {
    let length = connection.read_u32_le().await.expect("length") as usize;
    assert!(length > 0 && length <= 131_072);
    let mut body = vec![0; length];
    connection.read_exact(&mut body).await.expect("body");
    body
}

async fn handshake(
    registration: RegisteredUnixClient,
    requested: AccountScopeV2,
    mut client_key: Signer,
) -> AuthenticatedUnixConnection {
    let root = std::env::var_os("XDG_RUNTIME_DIR").expect("isolated runtime");
    let server = UnixLocalServer::bind().expect("isolated bind");
    let server_key = Signer::new();
    let server_key_blob = server_key.blob();
    let client_id = registration.id();
    let epoch = registration.epoch();
    let nonce = [9; 32];
    let (server_connection, client_connection) =
        tokio::join!(server.accept(), unix_transport::connect());
    let mut server_connection = server_connection.expect("accept");
    let mut client_connection = client_connection.expect("connect");
    let begin = json!({
        "jsonrpc":"2.0", "id":"m2", "method":"auth.begin",
        "params": {
            "protocol_version":1, "auth_revision":3, "scope_model_version":2,
            "registration_id":wire_hex::encode(&client_id), "profile_epoch":epoch,
            "client_nonce":wire_hex::encode(&nonce),
            "scope":serde_json::from_slice::<Value>(&requested.to_json().expect("scope json")).expect("scope value")
        }
    });
    write_frame(&mut client_connection, &begin).await;
    let registration_for_lookup = registration.clone();
    let server_fut = async move {
        let body = read_frame(&mut server_connection).await;
        let mut frame = Vec::with_capacity(body.len() + 4);
        frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
        frame.extend_from_slice(&body);
        let first = crate::decode_request(&frame).expect("begin frame");
        let mut signer = server_key;
        let mut lookup = move |id: &[u8; 16]| {
            if id == &registration_for_lookup.id() {
                Ok(Some(registration_for_lookup.clone()))
            } else {
                Ok(None)
            }
        };
        crate::unix_authenticated_wire::authenticate_after_begin(
            server_connection,
            first,
            &mut signer,
            [8; 32],
            &mut lookup,
        )
        .await
    };
    let client_fut = async move {
        let challenge_body = read_frame(&mut client_connection).await;
        let envelope: Value = serde_json::from_slice(&challenge_body).expect("challenge json");
        let result = envelope.get("result").expect("result");
        assert_eq!(result.get("auth_revision").and_then(Value::as_u64), Some(3));
        assert_eq!(
            result.get("scope_model_version").and_then(Value::as_u64),
            Some(2)
        );
        let transcript_bytes = wire_hex::decode_bounded(
            result
                .get("transcript_hex")
                .and_then(Value::as_str)
                .expect("transcript"),
            32_768,
        )
        .expect("transcript hex");
        let transcript = AccountAuthTranscript::decode_for_revision(
            &transcript_bytes,
            SignerRole::Server,
            AccountAuthRevision::UnixV3,
        )
        .expect("transcript");
        let server_signature = wire_hex::decode_fixed::<64>(
            result
                .get("server_signature_hex")
                .and_then(Value::as_str)
                .expect("signature"),
        )
        .expect("server signature");
        assert!(
            hantong_platform::verify_signature(
                &server_key_blob,
                &transcript_bytes,
                &server_signature
            )
            .is_ok()
        );
        let client_bytes = transcript
            .encode(SignerRole::Client)
            .expect("client transcript");
        let signature = client_key.sign(&client_bytes).expect("client signature");
        let finish = json!({
            "jsonrpc":"2.0", "id":"m2", "method":"auth.finish",
            "params":{"signature_hex":wire_hex::encode(&signature)}
        });
        write_frame(&mut client_connection, &finish).await;
        let ready: Value =
            serde_json::from_slice(&read_frame(&mut client_connection).await).expect("ready");
        assert_eq!(ready["result"]["authenticated"], true);
    };
    let (authenticated, ()) = tokio::join!(server_fut, client_fut);
    let _ = std::fs::remove_dir_all(root);
    authenticated.expect("typed handshake")
}

#[tokio::test(flavor = "current_thread")]
async fn sec03_unix_model2_socket_handshake_operation_and_revocation() {
    if std::env::var_os("HANTONG_MODEL2_ISOLATED").is_none() {
        let root = isolated_runtime();
        let status = Command::new(std::env::current_exe().expect("test executable"))
                .arg("--exact").arg("unix_auth::model2_tests::sec03_unix_model2_socket_handshake_operation_and_revocation")
                .arg("--nocapture").env("HANTONG_MODEL2_ISOLATED", "1")
                .env("XDG_RUNTIME_DIR", &root).status().expect("isolated test");
        let _ = std::fs::remove_dir_all(root);
        assert!(status.success());
        return;
    }
    let client_key = Signer::new();
    let full = full_scope();
    let registration = RegisteredUnixClient::from_registration_store(
        [7; 16],
        4,
        client_key.blob(),
        RequestedScope::from_account_scope(full.clone()).expect("scope"),
        true,
    )
    .expect("registration");
    let mut authenticated = handshake(registration.clone(), requested_scope(), client_key).await;
    assert_eq!(
        authenticated.check_account_scope(Some(&registration), "account_a", "mail.messages.get"),
        Ok(())
    );
    assert_eq!(
        authenticated.check_account_scope(Some(&registration), "account_b", "mail.messages.get"),
        Err(ErrorCode::ScopeDenied)
    );
    assert_eq!(
        authenticated.check_app_read_scope(
            Some(&registration),
            hantong_contracts::auth::AppReadTool::DisplaySettingsGet,
        ),
        Ok(())
    );
    assert_eq!(
        authenticated.check_app_read_scope(
            Some(&registration),
            hantong_contracts::auth::AppReadTool::DiagnosticsSnapshot,
        ),
        Ok(())
    );
    let mut revoked = registration.clone();
    revoked.active = false;
    assert_eq!(
        authenticated.check_account_scope(Some(&revoked), "account_a", "mail.messages.get"),
        Err(ErrorCode::ClientSuspended)
    );

    let client_key = Signer::new();
    let registration = RegisteredUnixClient::from_registration_store(
        [7; 16],
        4,
        client_key.blob(),
        RequestedScope::from_account_scope(full).expect("scope"),
        true,
    )
    .expect("registration");
    let mut authenticated = handshake(registration.clone(), requested_scope(), client_key).await;
    let changed = RegisteredUnixClient::from_registration_store(
        [7; 16],
        4,
        registration.public_key,
        RequestedScope::from_account_scope(reduced_scope()).expect("scope"),
        true,
    )
    .expect("changed registration");
    assert_eq!(
        authenticated.check_account_scope(Some(&changed), "account_a", "mail.messages.get"),
        Err(ErrorCode::ScopeDenied)
    );
}
