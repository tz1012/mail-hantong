use hantong_contracts::{ErrorCode, auth::RequestedScope};
use hantong_ipc_client::PendingClientAuthentication;
use hantong_ipc_server::{auth::RegisteredClient, authenticated_wire::dispatch_connection};
use hantong_platform::{ClientSigningKey, LocalPipeServer, ServerSigningKey, random_nonce};
use hantong_store::Store;
use serde_json::json;
use std::cell::Cell;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[path = "folder_integration_tests.rs"]
mod folder_integration_tests;
#[path = "read_policy_tests.rs"]
mod read_policy_tests;
#[path = "uid_integration_tests.rs"]
mod uid_integration_tests;

struct TestKeys {
    client: Option<ClientSigningKey>,
    server: Option<ServerSigningKey>,
    wrong_server: Option<ServerSigningKey>,
}

impl TestKeys {
    fn new() -> Self {
        Self {
            client: Some(ClientSigningKey::create().unwrap()),
            server: Some(ServerSigningKey::create(random_nonce().unwrap()).unwrap()),
            wrong_server: Some(ServerSigningKey::create(random_nonce().unwrap()).unwrap()),
        }
    }
}

impl Drop for TestKeys {
    fn drop(&mut self) {
        if let Some(key) = self.client.take() {
            let _ = key.delete();
        }
        if let Some(key) = self.server.take() {
            let _ = key.delete();
        }
        if let Some(key) = self.wrong_server.take() {
            let _ = key.delete();
        }
    }
}

// These tests use the same production pipe name. Acquire ownership before each
// scenario's deadline so unrelated scenarios do not consume its time budget.
static PIPE_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn bind_fixture_pipe() -> LocalPipeServer {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        match LocalPipeServer::bind() {
            Ok(listener) => return listener,
            // A prior runtime's IOCP cancellation may briefly retain the handle.
            Err(ErrorCode::Conflict) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(code) => panic!("fixture pipe could not bind: {code:?}"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum PipeScenario {
    AppReads,
    Policy,
    Folders,
    UidAppend,
    UidReconcile,
    UidRecovery,
}

async fn run_isolated_pipe_scenario(scenario: PipeScenario) {
    let _owner = PIPE_TEST.lock().await;
    tokio::time::timeout(Duration::from_secs(20), async {
        let keys = TestKeys::new();
        let client = keys.client.as_ref().unwrap();
        let server = keys.server.as_ref().unwrap();
        let instance = random_nonce().unwrap();
        let mut listener = bind_fixture_pipe().await;
        match scenario {
            PipeScenario::AppReads => {
                read_policy_tests::run_app_read_scenarios(&mut listener, client, server, instance)
                    .await;
            }
            PipeScenario::Policy => {
                read_policy_tests::run_policy_scenarios(&mut listener, client, server, instance)
                    .await;
            }
            PipeScenario::Folders => {
                folder_integration_tests::run_folder_scenarios(
                    &mut listener,
                    client,
                    server,
                    instance,
                )
                .await;
            }
            PipeScenario::UidAppend => {
                uid_integration_tests::run_uid_scenarios(&mut listener, client, server, instance)
                    .await;
            }
            PipeScenario::UidReconcile => {
                uid_integration_tests::run_reconcile_scenarios(
                    &mut listener,
                    client,
                    server,
                    instance,
                )
                .await;
            }
            PipeScenario::UidRecovery => {
                uid_integration_tests::run_recovery_scenarios(
                    &mut listener,
                    client,
                    server,
                    instance,
                )
                .await;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("authenticated pipe scenario timed out: {scenario:?}"));
}

#[tokio::test(flavor = "current_thread")]
async fn cli16_ops05_app_reads_use_authenticated_scope_policy_and_audit() {
    run_isolated_pipe_scenario(PipeScenario::AppReads).await;
}

#[tokio::test(flavor = "current_thread")]
async fn sec03_policy_is_enforced_over_the_authenticated_pipe() {
    run_isolated_pipe_scenario(PipeScenario::Policy).await;
}

#[tokio::test(flavor = "current_thread")]
async fn ext06_folders_are_observed_over_the_authenticated_pipe() {
    run_isolated_pipe_scenario(PipeScenario::Folders).await;
}

#[tokio::test(flavor = "current_thread")]
async fn sync02_uid_append_is_observed_over_the_authenticated_pipe() {
    run_isolated_pipe_scenario(PipeScenario::UidAppend).await;
}

#[tokio::test(flavor = "current_thread")]
async fn m204_uid_reconcile_is_observed_over_the_authenticated_pipe() {
    run_isolated_pipe_scenario(PipeScenario::UidReconcile).await;
}

#[tokio::test(flavor = "current_thread")]
async fn mail03_sync02_recovery_and_metadata_are_bound_over_the_authenticated_pipe() {
    run_isolated_pipe_scenario(PipeScenario::UidRecovery).await;
}

#[tokio::test(flavor = "current_thread")]
async fn m102_core_authentication_uses_real_pipe_and_consumes_one_tool_request() {
    let _owner = PIPE_TEST.lock().await;
    tokio::time::timeout(Duration::from_secs(20), async {
        let keys = TestKeys::new();
        let client_key = keys.client.as_ref().unwrap();
        let server_key = keys.server.as_ref().unwrap();
        let wrong_server = keys.wrong_server.as_ref().unwrap();
        let scope =
            RequestedScope::new(vec!["account_a".into()], vec!["mail.accounts.list".into()])
                .unwrap();
        let registration = RegisteredClient::from_registration_store(
            [1; 16],
            1,
            client_key.public_key().unwrap(),
            scope.clone(),
            true,
        )
        .unwrap();
        let instance = random_nonce().unwrap();
        let mut listener = bind_fixture_pipe().await;

        for scenario in ["readiness", "unauthenticated_mail", "unexpected_finish"] {
            let server_task = async {
                let connection = listener.accept().await.unwrap();
                let lookups = Cell::new(0);
                let executions = Cell::new(0);
                let mut lookup = |_id: &[u8; 16]| {
                    lookups.set(lookups.get() + 1);
                    Ok(Some(registration.clone()))
                };
                let mut execute =
                    |_session: &mut hantong_ipc_server::auth::AuthenticatedConnection<'_>,
                     _request: &hantong_ipc_server::RpcRequest| {
                        executions.set(executions.get() + 1);
                        Ok(json!({"unexpected":true}))
                    };
                let result = dispatch_connection(
                    connection,
                    server_key,
                    instance,
                    &mut lookup,
                    &mut execute,
                )
                .await;
                assert_eq!(
                    result,
                    if scenario == "unexpected_finish" {
                        Err(ErrorCode::InvalidInput)
                    } else {
                        Ok(())
                    }
                );
                assert_eq!(lookups.get(), 0);
                assert_eq!(executions.get(), 0);
            };
            let client_task = async {
                let mut pipe = hantong_platform::connect().await.unwrap();
                let (id, method, params) = if scenario == "readiness" {
                    ("boot", "system.readiness", json!({"protocol_version":1}))
                } else if scenario == "unexpected_finish" {
                    (
                        "finish",
                        "auth.finish",
                        json!({"signature_hex":"00".repeat(64)}),
                    )
                } else {
                    ("mail", "mail.accounts.list", json!({}))
                };
                send_json(
                    &mut pipe,
                    json!({
                        "jsonrpc":"2.0", "id":id, "method":method, "params":params
                    }),
                )
                .await;
                if scenario == "unexpected_finish" {
                    assert!(pipe.read_u8().await.is_err());
                    return;
                }
                let response = receive_json(&mut pipe).await;
                if scenario == "readiness" {
                    let body = serde_json::to_vec(&response).unwrap();
                    let status =
                        hantong_ipc_client::parse_readiness_response(&body, "boot").unwrap();
                    assert!(status.transport_ready);
                    assert!(!status.mail_ready);
                    assert!(!status.authenticated);
                } else {
                    assert_eq!(response["error"]["code"], "CLIENT_NOT_REGISTERED");
                }
                pipe.write_u8(0x06).await.unwrap();
            };
            tokio::join!(server_task, client_task);
        }

        let store_fixture = StoreFixture::new();
        let store = Store::open(&store_fixture.data, &store_fixture.keys).unwrap();
        let mut read_policy = hantong_policy::ReadPolicy::new();
        let (folder_handle, _folder_worker) = crate::folder_worker::coordinator_with_discovery(
            &store,
            crate::folder_worker::NoDiscovery,
        );
        let server_task = async {
            let connection = listener.accept().await.unwrap();
            let mut lookup = |_id: &[u8; 16]| Ok(Some(registration.clone()));
            let mut execute =
                |session: &mut hantong_ipc_server::auth::AuthenticatedConnection<'_>,
                 request: &hantong_ipc_server::RpcRequest| {
                    crate::execution_adapter::execute(
                        &store,
                        &mut read_policy,
                        &folder_handle,
                        session,
                        request,
                    )
                };
            let result =
                dispatch_connection(connection, server_key, instance, &mut lookup, &mut execute)
                    .await;
            assert!(matches!(result, Err(ErrorCode::AuthRequired)));
        };
        let client_task = async {
            let pending = PendingClientAuthentication::begin(
                client_key.name(),
                server_key.public_key().unwrap(),
                [1; 16],
                1,
                scope.clone(),
                now_ms(),
            )
            .unwrap();
            let client = hantong_ipc_client::authenticated_wire::authenticate(pending, "store")
                .await
                .unwrap();
            assert_eq!(
                client
                    .exchange(
                        "tool",
                        "mail.accounts.list",
                        &json!({
                            "schema_version":1,
                            "request_id":"store-read",
                            "tool":"mail.accounts.list",
                            "params":{"account_id":"account_a"}
                        })
                    )
                    .await,
                Err(ErrorCode::CoreUnavailable)
            );
        };
        tokio::join!(server_task, client_task);
        assert!(store.list_audit_page(None, 100).unwrap().is_empty());
        drop(folder_handle);
        drop(_folder_worker);
        drop(store);
        drop(store_fixture);

        for scenario in [
            "valid",
            "wrong_pin",
            "missing",
            "epoch_changed",
            "revoked",
            "policy_error",
            "large_response",
        ] {
            let server_task = async {
                let connection = listener.accept().await.unwrap();
                let mut lookups = 0usize;
                let mut lookup = |id: &[u8; 16]| {
                    assert_eq!(id, &[1; 16]);
                    lookups += 1;
                    match scenario {
                        "missing" => Ok(None),
                        "epoch_changed" if lookups == 2 => Ok(Some(
                            RegisteredClient::from_registration_store(
                                [1; 16],
                                2,
                                client_key.public_key().unwrap(),
                                scope.clone(),
                                true,
                            )
                            .unwrap(),
                        )),
                        "revoked" if lookups == 2 => Ok(Some(
                            RegisteredClient::from_registration_store(
                                [1; 16],
                                1,
                                client_key.public_key().unwrap(),
                                scope.clone(),
                                false,
                            )
                            .unwrap(),
                        )),
                        _ => Ok(Some(registration.clone())),
                    }
                };
                let calls = Cell::new(0);
                let mut execute =
                    |_: &mut hantong_ipc_server::auth::AuthenticatedConnection<'_>,
                     request: &hantong_ipc_server::RpcRequest| {
                        calls.set(calls.get() + 1);
                        assert_eq!(request.method, "mail.accounts.list");
                        match scenario {
                            "policy_error" => Err(ErrorCode::PolicyBlocked),
                            "large_response" => Ok(json!({"payload":"x".repeat(140_000)})),
                            _ => Ok(json!({"items":[{"account_id":"account_a"}]})),
                        }
                    };
                let result = dispatch_connection(
                    connection,
                    server_key,
                    instance,
                    &mut lookup,
                    &mut execute,
                )
                .await;
                match scenario {
                    "missing" => assert!(matches!(result, Err(ErrorCode::ClientNotRegistered))),
                    "epoch_changed" => assert!(matches!(result, Err(ErrorCode::AuthRequired))),
                    "revoked" => assert!(matches!(result, Err(ErrorCode::ClientSuspended))),
                    "wrong_pin" => assert!(matches!(result, Err(ErrorCode::AuthRequired))),
                    "valid" | "policy_error" | "large_response" => {
                        result.unwrap();
                        assert_eq!(calls.get(), 1);
                    }
                    _ => unreachable!(),
                }
            };

            let client_task = async {
                let pending = PendingClientAuthentication::begin(
                    client_key.name(),
                    if scenario == "wrong_pin" {
                        wrong_server.public_key().unwrap()
                    } else {
                        server_key.public_key().unwrap()
                    },
                    [1; 16],
                    1,
                    scope.clone(),
                    now_ms(),
                )
                .unwrap();
                let client =
                    hantong_ipc_client::authenticated_wire::authenticate(pending, "begin").await;
                if matches!(
                    scenario,
                    "wrong_pin" | "missing" | "epoch_changed" | "revoked"
                ) {
                    assert!(matches!(client, Err(ErrorCode::AuthRequired)));
                    return;
                }
                let client = client.unwrap();
                let response = client
                    .exchange("tool", "mail.accounts.list", &json!({}))
                    .await
                    .unwrap();
                let response: serde_json::Value = serde_json::from_slice(&response).unwrap();
                assert_eq!(response["id"], "tool");
                if scenario == "policy_error" {
                    assert_eq!(response["error"]["code"], "POLICY_BLOCKED");
                } else if scenario == "large_response" {
                    assert!(response.to_string().len() > 131_072);
                } else {
                    assert_eq!(response["result"]["items"][0]["account_id"], "account_a");
                }
            };
            tokio::join!(server_task, client_task);
        }
        drop(listener);
        // Fatal local state failures must leave the serving loop with the
        // original stable code; reconnect cannot hide corruption from core.
        for fail_lookup in [true, false] {
            let server = async {
                let settle_deadline = tokio::time::Instant::now() + Duration::from_secs(1);
                loop {
                    let result = hantong_ipc_server::authenticated_wire::serve_until(
                        std::future::pending::<Result<(), ErrorCode>>(),
                        || {},
                        server_key,
                        instance,
                        |_| {
                            if fail_lookup {
                                Err(ErrorCode::InternalError)
                            } else {
                                Ok(Some(registration.clone()))
                            }
                        },
                        |_, _| Err(ErrorCode::InternalError),
                    )
                    .await;
                    // The previous fixture's IOCP cancellation can briefly retain
                    // the closed pipe handle. Retry only startup name conflicts.
                    if result == Err(ErrorCode::Conflict)
                        && tokio::time::Instant::now() < settle_deadline
                    {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        continue;
                    }
                    break result;
                }
            };
            let client = async {
                let pending = PendingClientAuthentication::begin(
                    client_key.name(),
                    server_key.public_key().unwrap(),
                    [1; 16],
                    1,
                    scope.clone(),
                    now_ms(),
                )
                .unwrap();
                let result =
                    hantong_ipc_client::authenticated_wire::authenticate(pending, "fatal").await;
                if fail_lookup {
                    assert!(matches!(result, Err(ErrorCode::AuthRequired)));
                } else {
                    assert_eq!(
                        result
                            .unwrap()
                            .exchange("tool", "mail.accounts.list", &json!({}))
                            .await,
                        Err(ErrorCode::CoreUnavailable)
                    );
                }
            };
            let (result, ()) = tokio::join!(server, client);
            assert_eq!(result, Err(ErrorCode::InternalError));
        }
    })
    .await
    .unwrap();
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}

struct StoreFixture {
    root: PathBuf,
    data: PathBuf,
    keys: PathBuf,
}

impl StoreFixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-core-auth-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let data = root.join("data");
        let keys = root.join("keys");
        Self { root, data, keys }
    }
}

impl Drop for StoreFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

async fn send_json<W: AsyncWrite + Unpin>(pipe: &mut W, value: serde_json::Value) {
    let bytes = serde_json::to_vec(&value).unwrap();
    pipe.write_u32_le(bytes.len() as u32).await.unwrap();
    pipe.write_all(&bytes).await.unwrap();
    pipe.flush().await.unwrap();
}

async fn receive_json<R: AsyncRead + Unpin>(pipe: &mut R) -> serde_json::Value {
    let length = pipe.read_u32_le().await.unwrap() as usize;
    assert!((1..=1_048_576).contains(&length));
    let mut bytes = vec![0; length];
    pipe.read_exact(&mut bytes).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
