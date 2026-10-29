//! Runs inside the existing exclusive real-pipe test, never on a second pipe.
use super::*;
use hantong_policy::ReadPolicy;
use hantong_test_support::policy_store::{PolicyFixtureMutation, PolicyStoreFixture};
use std::cell::RefCell;

struct Harness<'a> {
    listener: &'a mut LocalPipeServer,
    client: &'a ClientSigningKey,
    server: &'a ServerSigningKey,
    instance: [u8; 32],
    fixture: PolicyStoreFixture,
    policy: ReadPolicy,
}
impl<'a> Harness<'a> {
    fn new(
        listener: &'a mut LocalPipeServer,
        client: &'a ClientSigningKey,
        server: &'a ServerSigningKey,
        instance: [u8; 32],
    ) -> Self {
        Self {
            listener,
            client,
            server,
            instance,
            fixture: PolicyStoreFixture::new(server, client.public_key().unwrap()).unwrap(),
            policy: ReadPolicy::new(),
        }
    }
    async fn exchange(
        &mut self,
        envelope: serde_json::Value,
        mutation: Option<PolicyFixtureMutation>,
    ) -> (Result<(), ErrorCode>, Result<serde_json::Value, ErrorCode>) {
        let store = RefCell::new(Some(self.fixture.open().unwrap()));
        let current = store
            .borrow()
            .as_ref()
            .unwrap()
            .load_registration(self.fixture.registration_id)
            .unwrap()
            .unwrap();
        let method = envelope["tool"].as_str().unwrap().to_owned();
        let server_key = self.server;
        let client_key = self.client;
        let fixture = &self.fixture;
        let policy = &mut self.policy;
        let instance = self.instance;
        let server = async {
            let connection = self.listener.accept().await.unwrap();
            let mut lookup = |id: &[u8; 16]| {
                crate::execution_adapter::load_registration(store.borrow().as_ref().unwrap(), id)
            };
            let mut mutation = mutation;
            let mut execute =
                |session: &mut hantong_ipc_server::auth::AuthenticatedConnection<'_>,
                 request: &hantong_ipc_server::RpcRequest| {
                    if let Some(change) = mutation.take() {
                        drop(store.borrow_mut().take());
                        fixture.mutate(change).unwrap();
                        *store.borrow_mut() = Some(fixture.open().unwrap());
                    }
                    let borrowed = store.borrow();
                    let current_store = borrowed.as_ref().unwrap();
                    let (handle, _worker) = crate::folder_worker::coordinator_with_discovery(
                        current_store,
                        crate::folder_worker::NoDiscovery,
                    );
                    crate::execution_adapter::execute(
                        current_store,
                        policy,
                        &handle,
                        session,
                        request,
                    )
                };
            dispatch_connection(connection, server_key, instance, &mut lookup, &mut execute).await
        };
        let client = async {
            let pending = PendingClientAuthentication::begin(
                client_key.name(),
                server_key.public_key().unwrap(),
                current.registration_id,
                current.profile_epoch,
                current.scope,
                now_ms(),
            )
            .unwrap();
            let connection =
                hantong_ipc_client::authenticated_wire::authenticate(pending, "policy-auth")
                    .await?;
            let response = connection
                .exchange("policy-call", &method, &envelope)
                .await?;
            let value: serde_json::Value = serde_json::from_slice(&response).unwrap();
            if method == "mail.accounts.list" && value.get("result").is_some() {
                let ids = if let Some(id) = envelope["params"]["account_id"].as_str() {
                    vec![id.to_owned()]
                } else {
                    envelope["params"]["account_ids"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|id| id.as_str().unwrap().to_owned())
                        .collect()
                };
                let include_capabilities = envelope["params"]["include_capabilities"]
                    .as_bool()
                    .unwrap_or(false);
                let decoded = hantong_ipc_client::accounts::parse_account_list_response(
                    &response,
                    "policy-call",
                    &ids,
                    include_capabilities,
                )
                .unwrap();
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&decoded.to_json()).unwrap(),
                    value["result"]
                );
            }
            Ok(value)
        };
        tokio::join!(server, client)
    }
    async fn response(&mut self, envelope: serde_json::Value) -> serde_json::Value {
        let (server, response) = self.exchange(envelope, None).await;
        server.unwrap();
        response.unwrap()
    }
    fn audit(&self) -> Vec<hantong_contracts::audit::AuditRecord> {
        self.fixture
            .open()
            .unwrap()
            .list_audit_page(None, 100)
            .unwrap()
    }
}
fn read(account: &str) -> serde_json::Value {
    json!({"schema_version":1,"request_id":"fixture-read","tool":"mail.accounts.list","params":{"account_id":account}})
}
fn send(account: &str) -> serde_json::Value {
    json!({"schema_version":1,"request_id":"fixture-send","tool":"mail.send.execute","params":{"account_id":account,"draft_id":"draft","operation_id":"op","expected_version":1,"payload_hash":"aa00000000000000000000000000000000000000000000000000000000000000","approval_id":"untrusted-approval","idempotency_key":"key"}})
}
fn assert_error(value: &serde_json::Value, error: ErrorCode) {
    assert_eq!(value["error"]["code"], error.as_str());
    assert!(value.get("result").is_none());
}
fn assert_terminal(h: &Harness<'_>, error: Option<ErrorCode>, count: Option<u64>) {
    let rows = h.audit();
    let terminal = rows[0].completion().unwrap().terminal();
    assert_eq!(terminal.error(), error);
    assert_eq!(terminal.result_count(), count);
}

pub(super) async fn run_policy_scenarios(
    listener: &mut LocalPipeServer,
    client: &ClientSigningKey,
    server: &ServerSigningKey,
    instance: [u8; 32],
) {
    {
        let mut h = Harness::new(listener, client, server, instance);
        h.fixture
            .mutate(PolicyFixtureMutation::Limit {
                read_only: true,
                requests_per_minute: 1,
            })
            .unwrap();
        assert_error(
            &h.response(read("unknown_account")).await,
            ErrorCode::ScopeDenied,
        );
        assert!(h.audit().is_empty());
        assert_error(&h.response(read("account_b")).await, ErrorCode::ScopeDenied);
        assert_terminal(&h, Some(ErrorCode::ScopeDenied), None);
        let mut with_capabilities = read("account_a");
        with_capabilities["params"]["include_capabilities"] = json!(true);
        let response = h.response(with_capabilities).await;
        assert_eq!(response["result"]["items"][0]["account_id"], "account_a");
        assert_eq!(response["result"]["items"][0]["capabilities"], json!([]));
        assert_eq!(response["result"]["items"].as_array().unwrap().len(), 1);
        assert_terminal(&h, None, Some(1));
        // A new authenticated connection still shares the same registration bucket.
        assert_error(&h.response(read("account_a")).await, ErrorCode::RateLimited);
        assert_terminal(&h, Some(ErrorCode::RateLimited), None);
        let before_unknown = h.audit().len();
        assert_error(
            &h.response(read("unknown_account")).await,
            ErrorCode::ScopeDenied,
        );
        assert_eq!(h.audit().len(), before_unknown);
        // Scope rejection precedes rate policy, even when the bucket is already full.
        assert_error(&h.response(send("account_a")).await, ErrorCode::ScopeDenied);
        assert_terminal(&h, Some(ErrorCode::ScopeDenied), None);
        assert_eq!(h.audit().len(), 4);
    }
    {
        let mut h = Harness::new(listener, client, server, instance);
        h.fixture.mutate(PolicyFixtureMutation::AllowSend).unwrap();
        h.fixture
            .mutate(PolicyFixtureMutation::Limit {
                read_only: false,
                requests_per_minute: 2,
            })
            .unwrap();
        assert_error(
            &h.response(send("account_a")).await,
            ErrorCode::PolicyBlocked,
        );
        assert_terminal(&h, Some(ErrorCode::PolicyBlocked), None);
        h.fixture
            .mutate(PolicyFixtureMutation::Limit {
                read_only: false,
                requests_per_minute: 1,
            })
            .unwrap();
        assert_error(&h.response(read("account_a")).await, ErrorCode::RateLimited);
        h.fixture
            .mutate(PolicyFixtureMutation::Limit {
                read_only: false,
                requests_per_minute: 2,
            })
            .unwrap();
        assert_eq!(
            h.response(read("account_a")).await["result"]["items"][0]["account_id"],
            "account_a"
        );
        assert_terminal(&h, None, Some(1));
        assert_error(&h.response(send("account_a")).await, ErrorCode::RateLimited);
        assert_terminal(&h, Some(ErrorCode::RateLimited), None);
        assert_eq!(h.audit().len(), 4);
    }
    for mutation in [
        PolicyFixtureMutation::Revoke,
        PolicyFixtureMutation::AdvanceEpoch,
    ] {
        let mut h = Harness::new(listener, client, server, instance);
        let (server, response) = h.exchange(read("account_a"), Some(mutation)).await;
        assert!(matches!(
            server,
            Err(ErrorCode::AuthRequired | ErrorCode::ClientSuspended)
        ));
        assert_eq!(response, Err(ErrorCode::CoreUnavailable));
        assert!(h.audit().is_empty());
    }
}
