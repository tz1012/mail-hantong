//! Exercises the production worker algorithm over the existing exclusive local pipe.
use super::*;
use hantong_contracts::folders::{
    FolderCursor, FolderListRequest, FolderRefreshRequest, FolderRefreshStatusRequest,
};
use hantong_domain::{
    capabilities::ImapCapabilities,
    mailboxes::{
        ImapMailboxDescriptor, ImapMailboxName, ImapMailboxSnapshot, MailboxChildren,
        MailboxEncoding,
    },
};
use hantong_ipc_client::folders::{
    parse_folder_page, parse_refresh_receipt, parse_refresh_receipt_for_status,
    prepare_refresh_request,
};
use hantong_store::folder_refresh::FolderRefreshJob;
use hantong_test_support::policy_store::{PolicyFixtureMutation, PolicyStoreFixture};
use serde_json::Value;
use std::rc::Rc;
use tokio::sync::Notify;

fn snapshot() -> ImapMailboxSnapshot {
    ImapMailboxSnapshot::new(
        "account_a",
        1,
        ImapCapabilities::from_features([]),
        [b"Archive".as_slice(), b"INBOX".as_slice()]
            .into_iter()
            .map(|name| {
                ImapMailboxDescriptor::new(
                    ImapMailboxName::from_wire(name, MailboxEncoding::Utf8).unwrap(),
                    Some('/'),
                    false,
                    false,
                    MailboxChildren::HasNoChildren,
                    vec![],
                )
                .unwrap()
            })
            .collect(),
    )
    .unwrap()
}
struct FakeDiscovery {
    calls: Rc<Cell<u32>>,
    dropped: Rc<Cell<u32>>,
    gate: Rc<Notify>,
    entered: Rc<Notify>,
}
struct DropSignal(Rc<Cell<u32>>);
impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
impl crate::folder_worker::Discovery for FakeDiscovery {
    async fn discover(&self, job: &FolderRefreshJob) -> Result<ImapMailboxSnapshot, ErrorCode> {
        assert_eq!(job.request().account_id(), "account_a");
        let _drop = DropSignal(self.dropped.clone());
        self.calls.set(self.calls.get() + 1);
        self.entered.notify_one();
        self.gate.notified().await;
        Ok(snapshot())
    }
}
struct Harness<'a> {
    listener: &'a mut LocalPipeServer,
    client: &'a ClientSigningKey,
    server: &'a ServerSigningKey,
    instance: [u8; 32],
    store: &'a Store,
    handle: &'a crate::folder_worker::FolderWorkerHandle<'a>,
    registration_id: [u8; 16],
    policy: hantong_policy::ReadPolicy,
}
impl Harness<'_> {
    async fn rpc(&mut self, tool: &str, params: Value) -> Vec<u8> {
        let envelope =
            json!({"schema_version":1,"request_id":"folder-fixture","tool":tool,"params":params});
        let current = self
            .store
            .load_registration(self.registration_id)
            .unwrap()
            .unwrap();
        let pending = PendingClientAuthentication::begin(
            self.client.name(),
            self.server.public_key().unwrap(),
            current.registration_id,
            current.profile_epoch,
            current.scope,
            now_ms(),
        )
        .unwrap();
        let server = async {
            let pipe = self.listener.accept().await.unwrap();
            let mut lookup =
                |id: &[u8; 16]| crate::execution_adapter::load_registration(self.store, id);
            let mut execute =
                |session: &mut hantong_ipc_server::auth::AuthenticatedConnection<'_>,
                 request: &hantong_ipc_server::RpcRequest| {
                    crate::execution_adapter::execute(
                        self.store,
                        &mut self.policy,
                        self.handle,
                        session,
                        request,
                    )
                };
            dispatch_connection(pipe, self.server, self.instance, &mut lookup, &mut execute).await
        };
        let client = async {
            let connection =
                hantong_ipc_client::authenticated_wire::authenticate(pending, "folder-auth")
                    .await?;
            connection.exchange("folder-call", tool, &envelope).await
        };
        let (served, response) = tokio::join!(server, client);
        served.unwrap();
        response.unwrap()
    }
    async fn refresh(&mut self, request: &FolderRefreshRequest) -> Value {
        let raw=self.rpc("mail.folders.refresh",json!({"action":"request","account_id":request.account_id(),"operation_id":request.operation_id(),"idempotency_key":request.idempotency_key(),"expected_generation":request.expected_generation(),"payload_hash":request.payload_hash()})).await;
        parse_refresh_receipt(&raw, "folder-call", request.account_id())
            .unwrap()
            .value()
            .clone()
    }
    async fn status(&mut self, id: [u8; 32]) -> Value {
        let request = FolderRefreshStatusRequest::new("account_a", id).unwrap();
        let raw=self.rpc("mail.folders.refresh",json!({"action":"status","account_id":"account_a","execution_id":hantong_contracts::wire_hex::encode(&id)})).await;
        parse_refresh_receipt_for_status(&raw, "folder-call", &request)
            .unwrap()
            .value()
            .clone()
    }
}
fn execution(value: &Value) -> [u8; 32] {
    hantong_contracts::wire_hex::decode_fixed(value["execution_id"].as_str().unwrap()).unwrap()
}
fn assert_error(raw: &[u8], code: ErrorCode) {
    assert_eq!(
        serde_json::from_slice::<Value>(raw).unwrap()["error"]["code"],
        code.as_str()
    );
}

pub(super) async fn run_folder_scenarios(
    listener: &mut LocalPipeServer,
    client: &ClientSigningKey,
    server: &ServerSigningKey,
    instance: [u8; 32],
) {
    let fixture = PolicyStoreFixture::new(server, client.public_key().unwrap()).unwrap();
    fixture.mutate(PolicyFixtureMutation::AllowFolders).unwrap();
    let store = fixture.open().unwrap();
    let calls = Rc::new(Cell::new(0));
    let dropped = Rc::new(Cell::new(0));
    let gate = Rc::new(Notify::new());
    let entered = Rc::new(Notify::new());
    let discovery = FakeDiscovery {
        calls: calls.clone(),
        dropped: dropped.clone(),
        gate: gate.clone(),
        entered: entered.clone(),
    };
    let (handle, worker) = crate::folder_worker::coordinator_with_discovery(&store, discovery);
    let mut h = Harness {
        listener,
        client,
        server,
        instance,
        store: &store,
        handle: &handle,
        registration_id: fixture.registration_id,
        policy: hantong_policy::ReadPolicy::new(),
    };
    let request = prepare_refresh_request("account_a", "op", "key", 0).unwrap();
    let interrupted = Cell::new([0; 32]);
    {
        let flow = async {
            assert_error(
                &h.rpc("mail.folders.list", json!({"account_id":"account_a"}))
                    .await,
                ErrorCode::NotFound,
            );
            assert_error(
                &h.rpc("mail.folders.list", json!({"account_id":"account_b"}))
                    .await,
                ErrorCode::ScopeDenied,
            );
            assert_eq!(calls.get(), 0);
            let first = h.refresh(&request).await;
            let id = execution(&first);
            assert_eq!(first["state"], "unresolved");
            entered.notified().await;
            assert_eq!(calls.get(), 1);
            assert_eq!(h.status(id).await["state"], "unresolved");
            assert_eq!(execution(&h.refresh(&request).await), id);
            assert_eq!(calls.get(), 1);
            let full = h
                .refresh(&prepare_refresh_request("account_a", "full", "full", 0).unwrap())
                .await;
            assert_eq!(full["state"], "failed");
            assert_eq!(full["error_code"], "RATE_LIMITED");
            assert_eq!(calls.get(), 1);
            gate.notify_one();
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if h.status(id).await["state"] == "succeeded" {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert_eq!(dropped.get(), 1);
            let page_request = FolderListRequest::new("account_a", Some(1), None).unwrap();
            let raw = h
                .rpc(
                    "mail.folders.list",
                    json!({"account_id":"account_a","limit":1}),
                )
                .await;
            let page = parse_folder_page(&raw, "folder-call", &page_request).unwrap();
            assert_eq!(page.items().len(), 1);
            let cursor = page.next_cursor().unwrap().to_owned();
            let next = FolderListRequest::new(
                "account_a",
                Some(1),
                Some(FolderCursor::parse(&cursor).unwrap()),
            )
            .unwrap();
            let raw = h
                .rpc(
                    "mail.folders.list",
                    json!({"account_id":"account_a","limit":1,"cursor":cursor}),
                )
                .await;
            let page = parse_folder_page(&raw, "folder-call", &next).unwrap();
            assert_eq!(page.items().len(), 1);
            assert_eq!(page.next_cursor(), None);
            assert_eq!(h.refresh(&request).await["state"], "succeeded");
            assert_eq!(calls.get(), 1);
            // The same production polling path cancels a discovery when its bound generation changes.
            let changed = h
                .refresh(&prepare_refresh_request("account_a", "change", "change", 1).unwrap())
                .await;
            let changed_id = execution(&changed);
            entered.notified().await;
            let endpoint = store.load_mail_connection("account_a").unwrap().unwrap();
            let s = snapshot();
            let mutation = hantong_store::mail_connection::MailMutation::new(
                "external".into(),
                "external".into(),
                1,
                hantong_store::imap_catalog::imap_catalog_payload_hash(1, endpoint.imap(), &s, 1)
                    .unwrap(),
            )
            .unwrap();
            store
                .replace_imap_mailbox_snapshot(1, endpoint.imap(), &s, &mutation)
                .unwrap();
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let status = h.status(changed_id).await;
                    if status["state"] == "failed" {
                        assert_eq!(status["error_code"], "CONFLICT");
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(40)).await;
                }
            })
            .await
            .unwrap();
            assert_eq!(dropped.get(), 2);
            assert_eq!(
                store
                    .load_imap_mailbox_snapshot("account_a")
                    .unwrap()
                    .unwrap()
                    .generation(),
                2
            );
            // Shutdown leaves a running observation unresolved; no reconnect is inferred.
            let receipt = h
                .refresh(
                    &prepare_refresh_request("account_a", "interrupted", "interrupted", 2).unwrap(),
                )
                .await;
            interrupted.set(execution(&receipt));
            entered.notified().await;
            assert_eq!(calls.get(), 3);
        };
        tokio::pin!(flow);
        let run = worker.run();
        tokio::pin!(run);
        tokio::select! { _=&mut flow=>{},result=&mut run=>panic!("worker stopped: {result:?}") }
    }
    assert_eq!(dropped.get(), 3);
    assert!(
        store
            .load_audit(interrupted.get())
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
    let discovery = FakeDiscovery {
        calls: calls.clone(),
        dropped: dropped.clone(),
        gate: gate.clone(),
        entered: entered.clone(),
    };
    let (restarted, restarted_worker) =
        crate::folder_worker::coordinator_with_discovery(&store, discovery);
    h.handle = &restarted;
    {
        let flow = async {
            assert_eq!(h.status(interrupted.get()).await["state"], "unresolved");
            let receipt = h
                .refresh(
                    &prepare_refresh_request("account_a", "interrupted", "interrupted", 2).unwrap(),
                )
                .await;
            assert_eq!(execution(&receipt), interrupted.get());
            assert_eq!(receipt["state"], "unresolved");
            assert_eq!(calls.get(), 3);
        };
        tokio::pin!(flow);
        let run = restarted_worker.run();
        tokio::pin!(run);
        tokio::select! {_=&mut flow=>{},result=&mut run=>panic!("restarted worker stopped: {result:?}")}
    }
    assert_eq!(calls.get(), 3);
    for audit in store.list_audit_page(None, 100).unwrap() {
        if *audit.execution_id() != interrupted.get() {
            assert!(audit.completion().is_some());
        }
    }
}
