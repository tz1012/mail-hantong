//! Public UID flow over the production authenticated pipe, with a fake provider.
use super::*;
use hantong_contracts::uid_page::*;
use hantong_domain::{
    capabilities::ImapCapabilities,
    imap_sync::{ImapEnvelope, ImapEnvelopeInput, ImapMessageMetadata, ImapUidPage},
    mailboxes::{
        ImapMailboxDescriptor, ImapMailboxName, ImapMailboxSnapshot, MailboxChildren,
        MailboxEncoding,
    },
};
use hantong_ipc_client::uid_page::{
    parse_cursor_snapshot, parse_page_receipt, parse_status_receipt, prepare_page_request,
};
use hantong_store::{mail_connection::MailMutation, uid_page_jobs::UidPageJob};
use hantong_test_support::policy_store::{PolicyFixtureMutation, PolicyStoreFixture};
use serde_json::Value;
use std::rc::Rc;
use tokio::sync::Notify;
#[path = "uid_recovery_integration_tests.rs"]
mod recovery;

fn mailbox() -> UidMailboxIdentity {
    UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap()
}
fn request(id: &str, generation: u64, version: u64) -> UidPageRequest {
    prepare_page_request("account_a", mailbox(), id, id, generation, version).unwrap()
}
fn catalog(store: &Store, previous: u64) {
    let endpoint = store.load_mail_connection("account_a").unwrap().unwrap();
    let descriptor = ImapMailboxDescriptor::new(
        ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap(),
        Some('/'),
        false,
        false,
        MailboxChildren::Unknown,
        vec![],
    )
    .unwrap();
    let snapshot = ImapMailboxSnapshot::new(
        "account_a",
        1,
        ImapCapabilities::from_features([]),
        vec![descriptor],
    )
    .unwrap();
    let id = format!("uid-catalog-{previous}");
    let hash = hantong_store::imap_catalog::imap_catalog_payload_hash(
        1,
        endpoint.imap(),
        &snapshot,
        previous,
    )
    .unwrap();
    store
        .replace_imap_mailbox_snapshot(
            1,
            endpoint.imap(),
            &snapshot,
            &MailMutation::new(id.clone(), id, previous, hash).unwrap(),
        )
        .unwrap();
}
struct Fake {
    calls: Rc<Cell<u32>>,
    dropped: Rc<Cell<u32>>,
    entered: Rc<Notify>,
    release: Rc<Notify>,
}
struct Dropped(Rc<Cell<u32>>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1)
    }
}
impl crate::uid_worker::Discovery for Fake {
    async fn discover(&self, job: &UidPageJob) -> Result<ImapUidPage, ErrorCode> {
        let _drop = Dropped(self.dropped.clone());
        self.calls.set(self.calls.get() + 1);
        self.entered.notify_one();
        self.release.notified().await;
        let uid = job.fetch_request().after_uid() + 1;
        let message = ImapMessageMetadata::new(
            uid,
            vec![],
            0,
            0,
            ImapEnvelope::new(ImapEnvelopeInput::default()).unwrap(),
        )
        .unwrap();
        Ok(ImapUidPage::new(job.fetch_request().clone(), 7, 130, vec![message]).unwrap())
    }
}
struct Harness<'a> {
    listener: &'a mut LocalPipeServer,
    client: &'a ClientSigningKey,
    server: &'a ServerSigningKey,
    instance: [u8; 32],
    store: &'a Store,
    folder: &'a crate::folder_worker::FolderWorkerHandle<'a>,
    uid: &'a crate::uid_worker::UidWorkerHandle<'a>,
    registration: [u8; 16],
    policy: hantong_policy::ReadPolicy,
}
impl Harness<'_> {
    async fn rpc(&mut self, tool: &str, params: Value) -> Vec<u8> {
        let envelope =
            json!({"schema_version":1,"request_id":"uid-fixture","tool":tool,"params":params});
        let registration = self
            .store
            .load_registration(self.registration)
            .unwrap()
            .unwrap();
        let pending = PendingClientAuthentication::begin(
            self.client.name(),
            self.server.public_key().unwrap(),
            registration.registration_id,
            registration.profile_epoch,
            registration.scope,
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
                    crate::execution_adapter::execute_with_uid(
                        self.store,
                        &mut self.policy,
                        self.folder,
                        Some(self.uid),
                        session,
                        request,
                    )
                };
            dispatch_connection(pipe, self.server, self.instance, &mut lookup, &mut execute).await
        };
        let client = async {
            hantong_ipc_client::authenticated_wire::authenticate(pending, "uid-auth")
                .await?
                .exchange("uid-call", tool, &envelope)
                .await
        };
        let (served, response) = tokio::join!(server, client);
        served.unwrap();
        response.unwrap()
    }
    async fn page(&mut self, request: &UidPageRequest) -> UidPageReceipt {
        let raw=self.rpc("mail.sync.page",json!({"action":"request","account_id":request.account_id(),"wire_hex":request.mailbox().wire_hex(),"encoding":request.mailbox().encoding().as_str(),"operation_id":request.operation_id(),"idempotency_key":request.idempotency_key(),"expected_catalog_generation":request.expected_catalog_generation(),"expected_cursor_version":request.expected_cursor_version(),"intent_hash":request.intent_hash()})).await;
        parse_page_receipt(&raw, "uid-call", request).unwrap()
    }
    async fn status(&mut self, id: [u8; 32]) -> UidPageReceipt {
        let request = UidPageStatusRequest::new("account_a", id).unwrap();
        let raw=self.rpc("mail.sync.page",json!({"action":"status","account_id":"account_a","execution_id":hantong_contracts::wire_hex::encode(&id)})).await;
        parse_status_receipt(&raw, "uid-call", &request).unwrap()
    }
    async fn cursor(&mut self) -> UidCursorSnapshot {
        let request = UidCursorRequest::new("account_a", mailbox()).unwrap();
        let raw=self.rpc("mail.sync.page",json!({"action":"cursor","account_id":"account_a","wire_hex":"494e424f58","encoding":"modified_utf7"})).await;
        parse_cursor_snapshot(&raw, "uid-call", &request).unwrap()
    }
}
fn error(raw: &[u8], expected: ErrorCode) {
    assert_eq!(
        serde_json::from_slice::<Value>(raw).unwrap()["error"]["code"],
        expected.as_str()
    )
}
async fn frame_limits(
    listener: &mut LocalPipeServer,
    client: &ClientSigningKey,
    server: &ServerSigningKey,
    instance: [u8; 32],
) {
    use hantong_ipc_client::auth_wire;
    let scope =
        RequestedScope::new(vec!["account_a".into()], vec!["mail.sync.page".into()]).unwrap();
    let registration = RegisteredClient::from_registration_store(
        [81; 16],
        1,
        client.public_key().unwrap(),
        scope.clone(),
        true,
    )
    .unwrap();
    for (length, large_response) in [(8193, false), (8192, false), (8192, true)] {
        let serve = async {
            let connection = listener.accept().await.unwrap();
            let calls = Cell::new(0);
            let mut lookup = |_: &[u8; 16]| Ok(Some(registration.clone()));
            let mut execute = |_: &mut hantong_ipc_server::auth::AuthenticatedConnection<'_>,
                               _: &hantong_ipc_server::RpcRequest| {
                calls.set(calls.get() + 1);
                Ok(if large_response {
                    json!({"payload":"x".repeat(8192)})
                } else {
                    json!({"ok":true})
                })
            };
            let result =
                dispatch_connection(connection, server, instance, &mut lookup, &mut execute).await;
            assert_eq!(
                result,
                if length > 8192 {
                    Err(ErrorCode::InvalidInput)
                } else if large_response {
                    Err(ErrorCode::InternalError)
                } else {
                    Ok(())
                }
            );
            assert_eq!(calls.get(), if length > 8192 { 0 } else { 1 });
        };
        let send = async {
            let mut pipe = hantong_platform::connect().await.unwrap();
            let pending = PendingClientAuthentication::begin(
                client.name(),
                server.public_key().unwrap(),
                [81; 16],
                1,
                scope.clone(),
                now_ms(),
            )
            .unwrap();
            send_json(
                &mut pipe,
                auth_wire::begin_request(
                    "uid-limit",
                    [81; 16],
                    1,
                    pending.client_nonce(),
                    pending.requested_scope(),
                )
                .unwrap(),
            )
            .await;
            let challenge = receive_json(&mut pipe).await;
            let (transcript, signature) =
                auth_wire::parse_challenge(&serde_json::to_vec(&challenge).unwrap(), "uid-limit")
                    .unwrap();
            let proof = pending.respond(transcript, &signature, now_ms()).unwrap();
            send_json(
                &mut pipe,
                auth_wire::finish_request("uid-limit", &proof).unwrap(),
            )
            .await;
            let ready = receive_json(&mut pipe).await;
            auth_wire::parse_ready(&serde_json::to_vec(&ready).unwrap(), "uid-limit").unwrap();
            let mut raw=serde_json::to_vec(&json!({"jsonrpc":"2.0","id":"uid-frame","method":"mail.sync.page","params":{"schema_version":1,"request_id":"frame","tool":"mail.sync.page","params":{"action":"cursor","account_id":"account_a","wire_hex":"494e424f58","encoding":"modified_utf7"}}})).unwrap();
            raw.resize(length, b' ');
            pipe.write_u32_le(raw.len() as u32).await.unwrap();
            pipe.write_all(&raw).await.unwrap();
            pipe.flush().await.unwrap();
            if length > 8192 || large_response {
                assert!(pipe.read_u8().await.is_err())
            } else {
                assert_eq!(receive_json(&mut pipe).await["result"]["ok"], true);
                pipe.write_u8(0x06).await.unwrap();
            }
        };
        tokio::join!(serve, send);
    }
}
pub(super) async fn run_uid_scenarios(
    listener: &mut LocalPipeServer,
    client: &ClientSigningKey,
    server: &ServerSigningKey,
    instance: [u8; 32],
) {
    frame_limits(listener, client, server, instance).await;
    // A folder-only registration cannot invoke the new tool.
    {
        let fixture = PolicyStoreFixture::new(server, client.public_key().unwrap()).unwrap();
        fixture.mutate(PolicyFixtureMutation::AllowFolders).unwrap();
        let store = fixture.open().unwrap();
        let slot = Rc::new(Cell::new(None));
        let (folder, _fw) = crate::folder_worker::coordinator_with_discovery_and_slot(
            &store,
            crate::folder_worker::NoDiscovery,
            slot.clone(),
        );
        let (uid, _uw) = crate::uid_worker::coordinator_with_discovery(
            &store,
            crate::uid_worker::NoDiscovery,
            slot,
        );
        let mut h = Harness {
            listener,
            client,
            server,
            instance,
            store: &store,
            folder: &folder,
            uid: &uid,
            registration: fixture.registration_id,
            policy: hantong_policy::ReadPolicy::new(),
        };
        error(&h.rpc("mail.sync.page",json!({"action":"cursor","account_id":"account_a","wire_hex":"494e424f58","encoding":"modified_utf7"})).await,ErrorCode::ScopeDenied);
    }
    let fixture = PolicyStoreFixture::new(server, client.public_key().unwrap()).unwrap();
    fixture.mutate(PolicyFixtureMutation::AllowUidPage).unwrap();
    let store = fixture.open().unwrap();
    catalog(&store, 0);
    let calls = Rc::new(Cell::new(0));
    let dropped = Rc::new(Cell::new(0));
    let entered = Rc::new(Notify::new());
    let release = Rc::new(Notify::new());
    let fake = || Fake {
        calls: calls.clone(),
        dropped: dropped.clone(),
        entered: entered.clone(),
        release: release.clone(),
    };
    let shared = Rc::new(Cell::new(None));
    let (folder, _folder_worker) = crate::folder_worker::coordinator_with_discovery_and_slot(
        &store,
        crate::folder_worker::NoDiscovery,
        shared.clone(),
    );
    let (uid, worker) =
        crate::uid_worker::coordinator_with_discovery(&store, fake(), shared.clone());
    let mut h = Harness {
        listener,
        client,
        server,
        instance,
        store: &store,
        folder: &folder,
        uid: &uid,
        registration: fixture.registration_id,
        policy: hantong_policy::ReadPolicy::new(),
    };
    let interrupted = Cell::new([0; 32]);
    {
        let flow = async {
            assert_eq!(h.cursor().await.cursor_version(), 0);
            error(&h.rpc("mail.sync.page",json!({"action":"cursor","account_id":"account_b","wire_hex":"494e424f58","encoding":"modified_utf7"})).await,ErrorCode::ScopeDenied);
            let first = request("first", 1, 0);
            let receipt = h.page(&first).await;
            let id = *receipt.execution_id();
            assert_eq!(receipt.state(), UidPageState::Unresolved);
            entered.notified().await;
            assert_eq!(calls.get(), 1);
            assert_eq!(h.status(id).await.state(), UidPageState::Unresolved);
            assert_eq!(h.page(&first).await.execution_id(), &id);
            assert_eq!(calls.get(), 1);
            let full = h.page(&request("full", 1, 0)).await;
            assert_eq!(full.error(), Some(ErrorCode::RateLimited));
            // Folder and UID work share the same active network slot.
            let f = hantong_ipc_client::folders::prepare_refresh_request(
                "account_a",
                "folder-full",
                "folder-full",
                1,
            )
            .unwrap();
            let raw=h.rpc("mail.folders.refresh",json!({"action":"request","account_id":"account_a","operation_id":f.operation_id(),"idempotency_key":f.idempotency_key(),"expected_generation":1,"payload_hash":f.payload_hash()})).await;
            let full =
                hantong_ipc_client::folders::parse_refresh_receipt(&raw, "uid-call", "account_a")
                    .unwrap();
            assert_eq!(full.value()["error_code"], "RATE_LIMITED");
            release.notify_one();
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if h.status(id).await.state() == UidPageState::Succeeded {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            let current = h.cursor().await;
            assert_eq!(
                (current.cursor_version(), current.covered_through_uid()),
                (1, 64)
            );
            assert!(current.binding_current());
            let done = h.page(&first).await;
            assert_eq!(done.inserted_count(), Some(1));
            assert_eq!(calls.get(), 1);
            // Generation changes cancel an in-flight observation without cursor writes.
            let changing = h.page(&request("changing", 1, 1)).await;
            let changed = *changing.execution_id();
            entered.notified().await;
            catalog(&store, 1);
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let r = h.status(changed).await;
                    if r.state() == UidPageState::Failed {
                        assert_eq!(r.error(), Some(ErrorCode::Conflict));
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(30)).await;
                }
            })
            .await
            .unwrap();
            assert_eq!(h.cursor().await.cursor_version(), 1);
            assert_eq!(dropped.get(), 2);
            let pending = h.page(&request("interrupted", 2, 1)).await;
            interrupted.set(*pending.execution_id());
            entered.notified().await;
            assert_eq!(calls.get(), 3);
        };
        tokio::pin!(flow);
        let run = worker.run();
        tokio::pin!(run);
        tokio::select! {_=&mut flow=>{},result=&mut run=>panic!("UID worker stopped: {result:?}")}
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
    let (restart, restart_worker) =
        crate::uid_worker::coordinator_with_discovery(&store, fake(), shared);
    h.uid = &restart;
    {
        let flow = async {
            assert_eq!(
                h.status(interrupted.get()).await.state(),
                UidPageState::Unresolved
            );
            assert_eq!(
                h.page(&request("interrupted", 2, 1)).await.execution_id(),
                &interrupted.get()
            );
            assert_eq!(calls.get(), 3);
            assert_eq!(h.cursor().await.cursor_version(), 1);
        };
        tokio::pin!(flow);
        let run = restart_worker.run();
        tokio::pin!(run);
        tokio::select! {_=&mut flow=>{},result=&mut run=>panic!("restarted UID worker stopped: {result:?}")}
    }
    for audit in store.list_audit_page(None, 100).unwrap() {
        if *audit.execution_id() != interrupted.get() {
            assert!(audit.completion().is_some())
        }
    }
    run_reconcile_scenarios(listener, client, server, instance).await;
    recovery::run_scenarios(listener, client, server, instance).await;
}

use hantong_contracts::uid_reconcile::{UidReconcileReceipt, UidReconcileRequest};
use hantong_domain::imap_reconcile::ImapUidReconcilePage;
use hantong_store::uid_reconcile_jobs::UidReconcileJob;
struct ReconcileFake {
    calls: Rc<Cell<u32>>,
    entered: Rc<Notify>,
    release: Rc<Notify>,
    flagged: Rc<Cell<bool>>,
}
fn metadata(uid: u32, flags: Vec<String>) -> ImapMessageMetadata {
    ImapMessageMetadata::new(
        uid,
        flags,
        0,
        0,
        ImapEnvelope::new(ImapEnvelopeInput::default()).unwrap(),
    )
    .unwrap()
}
impl crate::uid_worker::Discovery for ReconcileFake {
    async fn discover(&self, job: &UidPageJob) -> Result<ImapUidPage, ErrorCode> {
        let first = job.fetch_request().after_uid() + 1;
        Ok(ImapUidPage::new(
            job.fetch_request().clone(),
            7,
            200,
            vec![metadata(first, vec![]), metadata(first + 1, vec![])],
        )
        .unwrap())
    }
    async fn reconcile(&self, job: &UidReconcileJob) -> Result<ImapUidReconcilePage, ErrorCode> {
        self.calls.set(self.calls.get() + 1);
        self.entered.notify_one();
        self.release.notified().await;
        let flag = if self.flagged.get() {
            "\\Flagged"
        } else {
            "\\Seen"
        };
        Ok(ImapUidReconcilePage::new(
            job.fetch_request().clone(),
            200,
            vec![metadata(
                job.fetch_request().after_uid() + 1,
                vec![flag.into()],
            )],
        )
        .unwrap())
    }
}
impl Harness<'_> {
    async fn reconcile(&mut self, request: &UidReconcileRequest) -> UidReconcileReceipt {
        let raw=self.rpc("mail.sync.page",json!({"action":"reconcile","account_id":request.account_id(),"wire_hex":request.mailbox().wire_hex(),"encoding":request.mailbox().encoding().as_str(),"operation_id":request.operation_id(),"idempotency_key":request.idempotency_key(),"expected_catalog_generation":request.expected_catalog_generation(),"expected_cursor_version":request.expected_cursor_version(),"intent_hash":request.intent_hash()})).await;
        hantong_ipc_client::uid_reconcile::parse_reconcile_receipt(&raw, "uid-call", request)
            .unwrap()
    }
    async fn reconcile_status(&mut self, id: [u8; 32]) -> UidReconcileReceipt {
        let request = UidPageStatusRequest::new("account_a", id).unwrap();
        let raw=self.rpc("mail.sync.page",json!({"action":"reconcile-status","account_id":"account_a","execution_id":hantong_contracts::wire_hex::encode(&id)})).await;
        hantong_ipc_client::uid_reconcile::parse_status_receipt(&raw, "uid-call", &request).unwrap()
    }
    async fn reconcile_terminal(&mut self, id: [u8; 32]) -> UidReconcileReceipt {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let receipt = self.reconcile_status(id).await;
                if receipt.state() != UidPageState::Unresolved {
                    return receipt;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap()
    }
    async fn append_complete(&mut self, id: &str, version: u64) {
        let r = self.page(&request(id, 1, version)).await;
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let r = self.status(*r.execution_id()).await;
                if r.state() == UidPageState::Succeeded {
                    break;
                }
                assert_eq!(r.state(), UidPageState::Unresolved);
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }
}
fn reconcile_request(id: &str, generation: u64, version: u64) -> UidReconcileRequest {
    hantong_ipc_client::uid_reconcile::prepare_reconcile_request(
        "account_a",
        mailbox(),
        id,
        id,
        generation,
        version,
    )
    .unwrap()
}
async fn run_reconcile_scenarios(
    listener: &mut LocalPipeServer,
    client: &ClientSigningKey,
    server: &ServerSigningKey,
    instance: [u8; 32],
) {
    let fixture = PolicyStoreFixture::new(server, client.public_key().unwrap()).unwrap();
    fixture.mutate(PolicyFixtureMutation::AllowUidPage).unwrap();
    let store = fixture.open().unwrap();
    catalog(&store, 0);
    let calls = Rc::new(Cell::new(0));
    let entered = Rc::new(Notify::new());
    let release = Rc::new(Notify::new());
    let flagged = Rc::new(Cell::new(false));
    let shared = Rc::new(Cell::new(None));
    let (folder, _fw) = crate::folder_worker::coordinator_with_discovery_and_slot(
        &store,
        crate::folder_worker::NoDiscovery,
        shared.clone(),
    );
    let (uid, worker) = crate::uid_worker::coordinator_with_discovery(
        &store,
        ReconcileFake {
            calls: calls.clone(),
            entered: entered.clone(),
            release: release.clone(),
            flagged: flagged.clone(),
        },
        shared.clone(),
    );
    let mut h = Harness {
        listener,
        client,
        server,
        instance,
        store: &store,
        folder: &folder,
        uid: &uid,
        registration: fixture.registration_id,
        policy: hantong_policy::ReadPolicy::new(),
    };
    let interrupted = Cell::new([0; 32]);
    {
        let flow = async {
            h.append_complete("reconcile-base-one", 0).await;
            h.append_complete("reconcile-base-two", 1).await;
            let first = reconcile_request("reconcile-first", 1, 2);
            let receipt = h.reconcile(&first).await;
            let id = *receipt.execution_id();
            entered.notified().await;
            assert_eq!(h.reconcile(&first).await.execution_id(), &id);
            assert_eq!(calls.get(), 1);
            assert_eq!(
                h.page(&request("append-during-reconcile", 1, 2))
                    .await
                    .error(),
                Some(ErrorCode::RateLimited)
            );
            let f = hantong_ipc_client::folders::prepare_refresh_request(
                "account_a",
                "folder-during-reconcile",
                "folder-during-reconcile",
                1,
            )
            .unwrap();
            let raw=h.rpc("mail.folders.refresh",json!({"action":"request","account_id":"account_a","operation_id":f.operation_id(),"idempotency_key":f.idempotency_key(),"expected_generation":1,"payload_hash":f.payload_hash()})).await;
            assert_eq!(
                hantong_ipc_client::folders::parse_refresh_receipt(&raw, "uid-call", "account_a")
                    .unwrap()
                    .value()["error_code"],
                "RATE_LIMITED"
            );
            release.notify_one();
            let done = h.reconcile_terminal(id).await;
            let progress = done.result().unwrap();
            assert_eq!(
                (
                    progress.observed_count(),
                    progress.flags_changed_count(),
                    progress.newly_absent_count(),
                    progress.remaining_pages()
                ),
                (2, 1, 1, 1)
            );
            assert!(!progress.pass_complete());
            assert_eq!(h.cursor().await.cursor_version(), 2);
            h.append_complete("reconcile-base-three", 2).await;
            let next = h
                .reconcile(&reconcile_request("reconcile-second", 1, 3))
                .await;
            entered.notified().await;
            release.notify_one();
            let done = h.reconcile_terminal(*next.execution_id()).await;
            let progress = done.result().unwrap();
            assert_eq!(
                (
                    progress.frozen_cursor_version(),
                    progress.source_cursor_version()
                ),
                (2, 2)
            );
            assert!(progress.pass_complete());
            flagged.set(true);
            let again = h
                .reconcile(&reconcile_request("reconcile-new-pass", 1, 3))
                .await;
            entered.notified().await;
            release.notify_one();
            let done = h.reconcile_terminal(*again.execution_id()).await;
            assert_eq!(done.result().unwrap().generation(), 2);
            assert_eq!(done.result().unwrap().frozen_cursor_version(), 3);
            assert_eq!(h.reconcile(&first).await.execution_id(), &id);
            assert_eq!(calls.get(), 3);
            let name = ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap();
            assert_eq!(
                store
                    .load_imap_uid_observation("account_a", &name, 7, 1)
                    .unwrap()
                    .unwrap()
                    .flags()
                    .unwrap(),
                ["\\flagged"]
            );
            assert!(
                store
                    .load_imap_uid_metadata("account_a", &name, 7, 1)
                    .unwrap()
                    .unwrap()
                    .flags()
                    .is_empty()
            );
            assert!(
                store
                    .load_imap_uid_observation("account_a", &name, 7, 2)
                    .unwrap()
                    .unwrap()
                    .remote_absent()
            );
            let changing = h
                .reconcile(&reconcile_request("reconcile-cancel", 1, 3))
                .await;
            entered.notified().await;
            catalog(&store, 1);
            let cancelled = h.reconcile_terminal(*changing.execution_id()).await;
            assert_eq!(cancelled.error(), Some(ErrorCode::Conflict));
            assert_eq!(h.cursor().await.cursor_version(), 3);
            assert_eq!(
                store
                    .load_imap_uid_observation("account_a", &name, 7, 65)
                    .unwrap()
                    .unwrap()
                    .generation(),
                1
            );
            let pending = h
                .reconcile(&reconcile_request("reconcile-interrupted", 2, 3))
                .await;
            interrupted.set(*pending.execution_id());
            entered.notified().await;
            assert_eq!(calls.get(), 5);
        };
        tokio::pin!(flow);
        let run = worker.run();
        tokio::pin!(run);
        tokio::time::timeout(Duration::from_secs(15), async {
        tokio::select! {_=&mut flow=>{},result=&mut run=>panic!("reconcile worker stopped: {result:?}")}
    }).await.expect("bounded reconciliation pipe scenario");
    }
    assert_eq!(shared.get(), None);
    assert!(
        store
            .load_audit(interrupted.get())
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
    let (restart, restart_worker) = crate::uid_worker::coordinator_with_discovery(
        &store,
        crate::uid_worker::NoDiscovery,
        shared,
    );
    h.uid = &restart;
    let flow = async {
        assert_eq!(
            h.reconcile_status(interrupted.get()).await.state(),
            UidPageState::Unresolved
        );
        assert_eq!(
            h.reconcile(&reconcile_request("reconcile-interrupted", 2, 3))
                .await
                .execution_id(),
            &interrupted.get()
        );
        assert_eq!(calls.get(), 5);
    };
    tokio::pin!(flow);
    let run = restart_worker.run();
    tokio::pin!(run);
    tokio::time::timeout(Duration::from_secs(3),async {
        tokio::select! {_=&mut flow=>{},result=&mut run=>panic!("restarted reconcile worker stopped: {result:?}")}
    }).await.expect("bounded reconciliation restart scenario");
}
