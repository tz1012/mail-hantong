//! The real authenticated pipe and Core coordinator, with isolated GUI registration data.
use super::*;
use hantong_contracts::{
    audit::AuditTool,
    folders::FolderListRequest,
    message_search::{ImapMetadataFilter, ImapMetadataSearchRequest},
    uid_page::{UidMailboxEncoding, UidMailboxIdentity},
};
use hantong_domain::{
    capabilities::ImapCapabilities,
    imap_reconcile::ImapUidReconcilePage,
    imap_recovery::ImapUidRecoveryPage,
    imap_sync::{
        ImapEnvelope, ImapEnvelopeInput, ImapMessageMetadata, ImapUidPage, ImapUidPageRequest,
    },
    mailboxes::{
        ImapMailboxDescriptor, ImapMailboxName, ImapMailboxSnapshot, MailboxChildren,
        MailboxEncoding,
    },
};
use hantong_store::{
    ClientKind, DEFAULT_GUI_LABEL, GuiRegistrationStage, RegistrationState,
    mail_connection::MailMutation,
};
use hantong_test_support::policy_store::{PolicyFixtureMutation, PolicyStoreFixture};
use serde_json::Value;
use std::rc::Rc;

struct NoProvider(Rc<Cell<u32>>);
impl NoProvider {
    fn called(&self) -> ErrorCode {
        self.0.set(self.0.get() + 1);
        ErrorCode::Offline
    }
}
impl crate::folder_worker::Discovery for NoProvider {
    async fn discover(
        &self,
        _: &hantong_store::folder_refresh::FolderRefreshJob,
    ) -> Result<ImapMailboxSnapshot, ErrorCode> {
        Err(self.called())
    }
}
impl crate::uid_worker::Discovery for NoProvider {
    async fn discover(
        &self,
        _: &hantong_store::uid_page_jobs::UidPageJob,
    ) -> Result<ImapUidPage, ErrorCode> {
        Err(self.called())
    }
    async fn initial(
        &self,
        _: &hantong_store::initial_sync::InitialSyncJob,
    ) -> Result<crate::uid_worker::InitialObservation, ErrorCode> {
        Err(self.called())
    }
    async fn reconcile(
        &self,
        _: &hantong_store::uid_reconcile_jobs::UidReconcileJob,
    ) -> Result<ImapUidReconcilePage, ErrorCode> {
        Err(self.called())
    }
    async fn recover(
        &self,
        _: &hantong_store::uid_recovery_jobs::UidRecoveryJob,
    ) -> Result<ImapUidRecoveryPage, ErrorCode> {
        Err(self.called())
    }
}

fn mailbox() -> ImapMailboxName {
    ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap()
}
fn identity() -> UidMailboxIdentity {
    UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap()
}
fn seed_catalog(store: &Store) {
    let endpoint = store.load_mail_connection("account_a").unwrap().unwrap();
    let snapshot = ImapMailboxSnapshot::new(
        "account_a",
        1,
        ImapCapabilities::from_features([]),
        vec![
            ImapMailboxDescriptor::new(
                mailbox(),
                Some('/'),
                false,
                false,
                MailboxChildren::Unknown,
                vec![],
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let hash =
        hantong_store::imap_catalog::imap_catalog_payload_hash(1, endpoint.imap(), &snapshot, 0)
            .unwrap();
    store
        .replace_imap_mailbox_snapshot(
            1,
            endpoint.imap(),
            &snapshot,
            &MailMutation::new("gui-catalog".into(), "gui-catalog".into(), 0, hash).unwrap(),
        )
        .unwrap();
}
fn seed_metadata(store: &Store) {
    let endpoint = store.load_mail_connection("account_a").unwrap().unwrap();
    let messages = [
        (3, vec!["\\Flagged".into()]),
        (5, vec!["\\Seen".into()]),
        (7, vec![]),
    ]
    .into_iter()
    .map(|(uid, flags)| {
        ImapMessageMetadata::new(
            uid,
            flags,
            1767225600000,
            12,
            ImapEnvelope::new(ImapEnvelopeInput {
                subject: Some(b"=?UTF-8?B?7ZWc7Ya1?=".to_vec()),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap()
    })
    .collect();
    let page = ImapUidPage::new(
        ImapUidPageRequest::new("account_a", 1, mailbox(), None, 0).unwrap(),
        7,
        11,
        messages,
    )
    .unwrap();
    let hash =
        hantong_store::imap_sync::imap_uid_page_payload_hash(1, 1, endpoint.imap(), &page, 0)
            .unwrap();
    store
        .apply_imap_uid_page(
            1,
            1,
            endpoint.imap(),
            &page,
            &MailMutation::new("gui-page".into(), "gui-page".into(), 0, hash).unwrap(),
        )
        .unwrap();
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
        let record = self
            .store
            .load_registration(self.registration)
            .unwrap()
            .unwrap();
        assert_eq!(record.client_kind, ClientKind::Gui);
        assert_eq!(
            record.scope.tools(),
            hantong_contracts::auth::gui_read_tools()
        );
        let pending = PendingClientAuthentication::begin(
            self.client.name(),
            self.server.public_key().unwrap(),
            record.registration_id,
            record.profile_epoch,
            record.scope,
            now_ms(),
        )
        .unwrap();
        let envelope =
            json!({"schema_version":1,"request_id":"gui-fixture","tool":tool,"params":params});
        let server = async {
            let pipe = self.listener.accept().await.unwrap();
            let mut lookup =
                |id: &[u8; 16]| crate::execution_adapter::load_registration(self.store, id);
            let mut execute =
                |session: &mut hantong_ipc_server::auth::AuthenticatedConnection<'_>,
                 request: &hantong_ipc_server::RpcRequest| {
                    assert_eq!(session.registration_id(), &self.registration);
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
            hantong_ipc_client::authenticated_wire::authenticate(pending, "gui-fixture-auth")
                .await?
                .exchange("gui-call", tool, &envelope)
                .await
        };
        let (served, response) = tokio::join!(server, client);
        served.unwrap();
        response.unwrap()
    }
    async fn metadata(&mut self, filter: ImapMetadataFilter) -> Value {
        let r =
            ImapMetadataSearchRequest::new("account_a", identity(), Some(filter), Some(16), None)
                .unwrap();
        let raw=self.rpc("mail.messages.search",json!({"mode":"imap_metadata","account_id":"account_a","wire_hex":r.mailbox().wire_hex(),"encoding":"modified_utf7","filter":filter.as_str(),"limit":16,"cursor":null})).await;
        hantong_ipc_client::message_search::parse_search_response(&raw, "gui-call", &r)
            .unwrap()
            .value()
            .clone()
    }
}
fn denied(raw: &[u8]) {
    let value: Value = serde_json::from_slice(raw).unwrap();
    assert_eq!(value["error"]["code"], ErrorCode::ScopeDenied.as_str());
    assert!(value.get("result").is_none());
}

pub(super) async fn run_gui_read_scenarios(
    listener: &mut LocalPipeServer,
    client: &ClientSigningKey,
    server: &ServerSigningKey,
    instance: [u8; 32],
) {
    // Keep the existing CLI fixture genuinely distinct from the GUI identity.
    let cli_keys = TestKeys {
        client: Some(ClientSigningKey::create().unwrap()),
        server: None,
        wrong_server: None,
    };
    let fixture = PolicyStoreFixture::new(
        server,
        cli_keys.client.as_ref().unwrap().public_key().unwrap(),
    )
    .unwrap();
    fixture.mutate(PolicyFixtureMutation::AllowFolders).unwrap();
    let store = fixture.open().unwrap();
    let request_id = random_nonce().unwrap();
    let registration = hantong_platform::registration_id_for_request(request_id).unwrap();
    let scope = RequestedScope::new(
        vec!["account_a".into()],
        hantong_contracts::auth::gui_read_tools(),
    )
    .unwrap();
    let request = store
        .create_registration_request(request_id, ClientKind::Gui, DEFAULT_GUI_LABEL, &scope)
        .unwrap();
    let pin = hantong_platform::sha256(&server.public_key().unwrap()).unwrap();
    let stage = GuiRegistrationStage::new(request, registration, client.public_key().unwrap(), pin)
        .unwrap();
    let staged = store.stage_gui_registration(&stage).unwrap();
    assert_eq!(staged.state, RegistrationState::Provisioning);
    let active = store
        .activate_gui_registration(registration, staged.version, pin)
        .unwrap();
    assert_eq!(active.state, RegistrationState::Active);
    assert_eq!(active.client_kind, ClientKind::Gui);
    assert_ne!(registration, fixture.registration_id);
    assert_eq!(active.scope, scope);
    assert!(active.read_only);
    assert_eq!(active.requests_per_minute, 60);
    seed_catalog(&store);

    let calls = Rc::new(Cell::new(0));
    let slot = Rc::new(Cell::new(None));
    let (folder, folder_worker) = crate::folder_worker::coordinator_with_discovery_and_slot(
        &store,
        NoProvider(calls.clone()),
        slot.clone(),
    );
    let (uid, uid_worker) = crate::uid_worker::coordinator_with_instance(
        &store,
        NoProvider(calls.clone()),
        slot.clone(),
        instance,
    );
    let mut h = Harness {
        listener,
        client,
        server,
        instance,
        store: &store,
        folder: &folder,
        uid: &uid,
        registration,
        policy: hantong_policy::ReadPolicy::new(),
    };
    {
        let flow = async {
            let raw = h
                .rpc(
                    "mail.accounts.list",
                    json!({"account_ids":["account_a"],"include_capabilities":false}),
                )
                .await;
            let accounts = hantong_ipc_client::accounts::parse_account_list_response(
                &raw,
                "gui-call",
                &["account_a".into()],
                false,
            )
            .unwrap();
            assert_eq!(accounts.items().len(), 1);
            assert_eq!(accounts.items()[0].account_id(), "account_a");
            let folders = FolderListRequest::new("account_a", Some(16), None).unwrap();
            let raw = h
                .rpc(
                    "mail.folders.list",
                    json!({"account_id":"account_a","limit":16}),
                )
                .await;
            let folders =
                hantong_ipc_client::folders::parse_folder_page(&raw, "gui-call", &folders).unwrap();
            assert_eq!(folders.items().len(), 1);
            assert_eq!(folders.items()[0].value()["wire_hex"], "494e424f58");
            let unobserved = h.metadata(ImapMetadataFilter::All).await;
            assert_eq!(unobserved["completeness"], "unavailable");
            assert_eq!(unobserved["items"], json!([]));
            assert!(unobserved["observed_at"].is_null());
            seed_metadata(&store);
            let observed = h.metadata(ImapMetadataFilter::All).await;
            assert_eq!(observed["completeness"], "partial");
            assert_eq!(observed["items"].as_array().unwrap().len(), 3);
            assert_eq!(observed["items"][0]["uid"], 7);
            assert_eq!(observed["items"][0]["account_id"], "account_a");
            assert!(observed["observed_at"].is_string());
            let header: Value = serde_json::from_str(
                observed["untrusted_content"][0]["content"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                header["subject_hex"],
                "3d3f5554462d383f423f375a5763375961313f3d"
            );
            let unread = h.metadata(ImapMetadataFilter::Unread).await;
            let unread = unread["items"].as_array().unwrap();
            assert_eq!(
                unread
                    .iter()
                    .map(|row| row["uid"].as_u64().unwrap())
                    .collect::<Vec<_>>(),
                vec![7, 3]
            );
            assert!(unread.iter().all(|row| row["is_read"] == false));
            let favorite = h.metadata(ImapMetadataFilter::Favorite).await;
            let favorite = favorite["items"].as_array().unwrap();
            assert_eq!(favorite.len(), 1);
            assert_eq!(favorite[0]["uid"], 3);
            assert_eq!(favorite[0]["is_favorite"], true);
            denied(
                &h.rpc("mail.accounts.list", json!({"account_id":"account_b"}))
                    .await,
            );
            let sync = hantong_ipc_client::uid_page::prepare_page_request(
                "account_a",
                identity(),
                "gui-forbidden-sync",
                "gui-forbidden-sync",
                1,
                1,
            )
            .unwrap();
            denied(&h.rpc("mail.sync.page",json!({"action":"request","account_id":"account_a","wire_hex":"494e424f58","encoding":"modified_utf7","operation_id":sync.operation_id(),"idempotency_key":sync.idempotency_key(),"expected_catalog_generation":1,"expected_cursor_version":1,"intent_hash":sync.intent_hash()})).await);
            let refresh = hantong_ipc_client::folders::prepare_refresh_request(
                "account_a",
                "gui-forbidden-refresh",
                "gui-forbidden-refresh",
                1,
            )
            .unwrap();
            denied(&h.rpc("mail.folders.refresh",json!({"action":"request","account_id":"account_a","operation_id":refresh.operation_id(),"idempotency_key":refresh.idempotency_key(),"expected_generation":1,"payload_hash":refresh.payload_hash()})).await);
            denied(&h.rpc("app.settings.display.get", json!({})).await);
            denied(&h.rpc("mail.send.execute",json!({"account_id":"account_a","draft_id":"draft","operation_id":"gui-forbidden-send","expected_version":1,"payload_hash":"aa00000000000000000000000000000000000000000000000000000000000000","approval_id":"untrusted-approval","idempotency_key":"gui-forbidden-send"})).await);
            // Let active worker loops observe any accidentally enqueued work.
            tokio::task::yield_now().await;
            assert_eq!(calls.get(), 0);
            assert_eq!(slot.get(), None);
        };
        tokio::pin!(flow);
        tokio::select! {
            _=&mut flow=>{},
            result=folder_worker.run()=>panic!("folder worker stopped: {result:?}"),
            result=uid_worker.run()=>panic!("UID worker stopped: {result:?}"),
        }
    }
    let audits = store.list_audit_page(None, 100).unwrap();
    assert_eq!(audits.len(), 11);
    assert_eq!(
        audits
            .iter()
            .filter(|a| a.completion().unwrap().terminal().is_success())
            .count(),
        6
    );
    for record in &audits {
        assert_eq!(record.registration_id(), &registration);
        assert_eq!(record.actor().kind(), ClientKind::Gui);
        assert_eq!(record.actor().display_label(), DEFAULT_GUI_LABEL);
        let terminal = record.completion().unwrap().terminal();
        if !terminal.is_success() {
            assert_eq!(terminal.error(), Some(ErrorCode::ScopeDenied));
        }
    }
    let counts = audits
        .iter()
        .filter(|a| a.tool() == AuditTool::MessagesSearch)
        .map(|a| a.completion().unwrap().terminal().result_count().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(counts.len(), 4);
    for count in [0, 1, 2, 3] {
        assert!(counts.contains(&count));
    }
    assert_eq!(calls.get(), 0);
    assert_eq!(
        store
            .load_registration(fixture.registration_id)
            .unwrap()
            .unwrap()
            .client_kind,
        ClientKind::Cli
    );
}
