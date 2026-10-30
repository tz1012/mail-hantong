//! Recovery through the signed production pipe, using only an in-memory provider.
use super::*;
use hantong_contracts::uid_recovery::{UidRecoveryReceipt, UidRecoveryRequest};
use hantong_domain::{imap_recovery::ImapUidRecoveryPage, imap_sync::ImapUidPageRequest};
use hantong_store::uid_recovery_jobs::UidRecoveryJob;

struct RecoveryFake {
    active_validity: Rc<Cell<u32>>,
    append_calls: Rc<Cell<u32>>,
    reconcile_calls: Rc<Cell<u32>>,
    recovery_calls: Rc<Cell<u32>>,
    recovery_dropped: Rc<Cell<u32>>,
    entered: Rc<Notify>,
    release: Rc<Notify>,
}
fn epoch_metadata(uid: u32, validity: u32, flags: Vec<String>) -> ImapMessageMetadata {
    ImapMessageMetadata::new(
        uid,
        flags,
        0,
        validity,
        ImapEnvelope::new(ImapEnvelopeInput {
            subject: Some(format!("epoch-{validity}-uid-{uid}").into_bytes()),
            ..ImapEnvelopeInput::default()
        })
        .unwrap(),
    )
    .unwrap()
}
impl crate::uid_worker::Discovery for RecoveryFake {
    async fn discover(&self, job: &UidPageJob) -> Result<ImapUidPage, ErrorCode> {
        self.append_calls.set(self.append_calls.get() + 1);
        let validity = self.active_validity.get();
        let first = job.fetch_request().after_uid() + 1;
        Ok(ImapUidPage::new(
            job.fetch_request().clone(),
            validity,
            130,
            vec![
                epoch_metadata(first, validity, vec![]),
                epoch_metadata(first + 2, validity, vec![]),
            ],
        )
        .unwrap())
    }
    async fn reconcile(&self, job: &UidReconcileJob) -> Result<ImapUidReconcilePage, ErrorCode> {
        self.reconcile_calls.set(self.reconcile_calls.get() + 1);
        Ok(ImapUidReconcilePage::new(
            job.fetch_request().clone(),
            130,
            vec![epoch_metadata(
                job.fetch_request().after_uid() + 1,
                job.fetch_request().uid_validity(),
                vec!["\\Flagged".into()],
            )],
        )
        .unwrap())
    }
    async fn recover(&self, job: &UidRecoveryJob) -> Result<ImapUidRecoveryPage, ErrorCode> {
        let _drop = Dropped(self.recovery_dropped.clone());
        self.recovery_calls.set(self.recovery_calls.get() + 1);
        self.entered.notify_one();
        self.release.notified().await;
        let request = job.fetch_request();
        let validity = self.active_validity.get();
        let page = ImapUidPage::new(
            ImapUidPageRequest::new(
                request.account_id(),
                request.endpoint_version(),
                request.mailbox().clone(),
                Some(validity),
                0,
            )
            .unwrap(),
            validity,
            11,
            vec![
                epoch_metadata(1, validity, vec![]),
                epoch_metadata(3, validity, vec![]),
            ],
        )
        .unwrap();
        Ok(ImapUidRecoveryPage::new(request.clone(), page).unwrap())
    }
}
struct NoProvider;
impl crate::uid_worker::Discovery for NoProvider {
    async fn discover(&self, _: &UidPageJob) -> Result<ImapUidPage, ErrorCode> {
        panic!("unexpected append after recovery restart")
    }
    async fn reconcile(&self, _: &UidReconcileJob) -> Result<ImapUidReconcilePage, ErrorCode> {
        panic!("unexpected reconcile after recovery restart")
    }
    async fn recover(&self, _: &UidRecoveryJob) -> Result<ImapUidRecoveryPage, ErrorCode> {
        panic!("unexpected automatic recovery after restart")
    }
}
fn recovery_request(id: &str, catalog: u64, cursor: u64) -> UidRecoveryRequest {
    hantong_ipc_client::uid_recovery::prepare_recovery_request(
        "account_a",
        mailbox(),
        id,
        id,
        catalog,
        cursor,
    )
    .unwrap()
}
fn recovery_params(request: &UidRecoveryRequest) -> Value {
    json!({"action":"recover","account_id":request.account_id(),"wire_hex":request.mailbox().wire_hex(),"encoding":request.mailbox().encoding().as_str(),"operation_id":request.operation_id(),"idempotency_key":request.idempotency_key(),"expected_catalog_generation":request.expected_catalog_generation(),"expected_cursor_version":request.expected_cursor_version(),"intent_hash":request.intent_hash()})
}
impl Harness<'_> {
    async fn recover(&mut self, request: &UidRecoveryRequest) -> UidRecoveryReceipt {
        let raw = self.rpc("mail.sync.page", recovery_params(request)).await;
        hantong_ipc_client::uid_recovery::parse_recovery_receipt(&raw, "uid-call", request).unwrap()
    }
    async fn recovery_status(&mut self, id: [u8; 32]) -> UidRecoveryReceipt {
        let request = UidPageStatusRequest::new("account_a", id).unwrap();
        let raw=self.rpc("mail.sync.page",json!({"action":"recover-status","account_id":"account_a","execution_id":hantong_contracts::wire_hex::encode(&id)})).await;
        hantong_ipc_client::uid_recovery::parse_status_receipt(&raw, "uid-call", &request).unwrap()
    }
    async fn recovery_terminal(&mut self, id: [u8; 32]) -> UidRecoveryReceipt {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let receipt = self.recovery_status(id).await;
                if receipt.state() != UidPageState::Unresolved {
                    return receipt;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("bounded recovery terminal observation")
    }
}
pub(super) async fn run_scenarios(
    listener: &mut LocalPipeServer,
    client: &ClientSigningKey,
    server: &ServerSigningKey,
    instance: [u8; 32],
) {
    // The existing folder grant does not gain the shared UID tool implicitly.
    {
        let fixture = PolicyStoreFixture::new(server, client.public_key().unwrap()).unwrap();
        fixture.mutate(PolicyFixtureMutation::AllowFolders).unwrap();
        let store = fixture.open().unwrap();
        let shared = Rc::new(Cell::new(None));
        let (folder, _fw) = crate::folder_worker::coordinator_with_discovery_and_slot(
            &store,
            crate::folder_worker::NoDiscovery,
            shared.clone(),
        );
        let (uid, _uw) = crate::uid_worker::coordinator_with_discovery(&store, NoProvider, shared);
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
        error(
            &h.rpc(
                "mail.sync.page",
                recovery_params(&recovery_request("recovery-denied", 1, 1)),
            )
            .await,
            ErrorCode::ScopeDenied,
        );
        error(&h.rpc("mail.sync.page",json!({"action":"recover-status","account_id":"account_a","execution_id":"01".repeat(32)})).await,ErrorCode::ScopeDenied);
        error(&h.rpc("mail.sync.page",json!({"action":"inspect","account_id":"account_a","wire_hex":"494e424f58","encoding":"modified_utf7"})).await,ErrorCode::ScopeDenied);
    }
    let fixture = PolicyStoreFixture::new(server, client.public_key().unwrap()).unwrap();
    fixture.mutate(PolicyFixtureMutation::AllowUidPage).unwrap();
    let store = fixture.open().unwrap();
    catalog(&store, 0);
    let active_validity = Rc::new(Cell::new(7));
    let append_calls = Rc::new(Cell::new(0));
    let reconcile_calls = Rc::new(Cell::new(0));
    let recovery_calls = Rc::new(Cell::new(0));
    let recovery_dropped = Rc::new(Cell::new(0));
    let entered = Rc::new(Notify::new());
    let release = Rc::new(Notify::new());
    let shared = Rc::new(Cell::new(None));
    let (folder, _fw) = crate::folder_worker::coordinator_with_discovery_and_slot(
        &store,
        crate::folder_worker::NoDiscovery,
        shared.clone(),
    );
    let (uid, worker) = crate::uid_worker::coordinator_with_discovery(
        &store,
        RecoveryFake {
            active_validity: active_validity.clone(),
            append_calls: append_calls.clone(),
            reconcile_calls: reconcile_calls.clone(),
            recovery_calls: recovery_calls.clone(),
            recovery_dropped: recovery_dropped.clone(),
            entered: entered.clone(),
            release: release.clone(),
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
    let name = ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap();
    {
        let flow = async {
            let unobserved = h.inspect().await;
            assert!(unobserved.observation().is_none());
            assert!(unobserved.binding_current());
            assert_eq!(unobserved.catalog_generation(), 1);
            assert_eq!(
                (
                    append_calls.get(),
                    reconcile_calls.get(),
                    recovery_calls.get()
                ),
                (0, 0, 0)
            );
            assert_eq!(shared.get(), None);
            error(&h.rpc("mail.sync.page",json!({"action":"inspect","account_id":"account_b","wire_hex":"494e424f58","encoding":"modified_utf7"})).await,ErrorCode::ScopeDenied);
            h.append_complete("recovery-old-page", 0).await;
            let old_inspection = h.inspect().await;
            let observed = old_inspection.observation().unwrap();
            assert_eq!(
                (
                    observed.cursor_version(),
                    observed.epoch_generation(),
                    observed.covered_through_uid(),
                    observed.observed_uid_ceiling()
                ),
                (1, 1, 64, 129)
            );
            assert!(!observed.coverage_at_observed_ceiling());
            assert_eq!(
                observed.stored_at_utc_ms(),
                store
                    .load_imap_uid_cursor("account_a", &name)
                    .unwrap()
                    .unwrap()
                    .stored_at_utc_ms()
            );
            assert_eq!(append_calls.get(), 1);
            assert_eq!(shared.get(), None);

            let old_source = store
                .load_imap_uid_metadata("account_a", &name, 7, 1)
                .unwrap()
                .unwrap();
            let old_request = reconcile_request("recovery-old-observation", 1, 1);
            let old = h.reconcile(&old_request).await;
            let old_done = h.reconcile_terminal(*old.execution_id()).await;
            assert!(old_done.result().unwrap().pass_complete());
            assert_eq!(reconcile_calls.get(), 1);
            active_validity.set(9);
            let first_request = recovery_request("recovery-first", 1, 1);
            let first = h.recover(&first_request).await;
            let id = *first.execution_id();
            assert_eq!(first.state(), UidPageState::Unresolved);
            entered.notified().await;
            assert_eq!(
                h.recovery_status(id).await.state(),
                UidPageState::Unresolved
            );
            assert_eq!(h.recover(&first_request).await.execution_id(), &id);
            // A local read keeps the already-owned network permit and never invokes a provider.
            assert_eq!(h.inspect().await, old_inspection);
            assert_eq!(shared.get(), Some(id));
            assert_eq!(recovery_calls.get(), 1);
            assert_eq!(
                h.page(&request("append-during-recovery", 1, 1))
                    .await
                    .error(),
                Some(ErrorCode::RateLimited)
            );
            assert_eq!(
                h.reconcile(&reconcile_request("reconcile-during-recovery", 1, 1))
                    .await
                    .error(),
                Some(ErrorCode::RateLimited)
            );
            assert_eq!(
                h.recover(&recovery_request("recovery-during-recovery", 1, 1))
                    .await
                    .error(),
                Some(ErrorCode::RateLimited)
            );
            let refresh = hantong_ipc_client::folders::prepare_refresh_request(
                "account_a",
                "folder-during-recovery",
                "folder-during-recovery",
                1,
            )
            .unwrap();
            let raw=h.rpc("mail.folders.refresh",json!({"action":"request","account_id":"account_a","operation_id":refresh.operation_id(),"idempotency_key":refresh.idempotency_key(),"expected_generation":1,"payload_hash":refresh.payload_hash()})).await;
            assert_eq!(
                hantong_ipc_client::folders::parse_refresh_receipt(&raw, "uid-call", "account_a")
                    .unwrap()
                    .value()["error_code"],
                "RATE_LIMITED"
            );
            assert_eq!(
                (
                    append_calls.get(),
                    reconcile_calls.get(),
                    recovery_calls.get()
                ),
                (1, 1, 1)
            );
            release.notify_one();
            let done = h.recovery_terminal(id).await;
            let progress = done.result().unwrap();
            assert_eq!(
                (
                    progress.epoch_generation(),
                    progress.cursor_version(),
                    progress.covered_through_uid(),
                    progress.inserted_count()
                ),
                (2, 2, 10, 2)
            );
            assert_eq!(h.cursor().await.cursor_version(), 2);
            let recovered_inspection = h.inspect().await;
            let observed = recovered_inspection.observation().unwrap();
            assert_eq!(
                (
                    observed.cursor_version(),
                    observed.epoch_generation(),
                    observed.covered_through_uid(),
                    observed.observed_uid_ceiling()
                ),
                (2, 2, 10, 10)
            );
            assert!(observed.coverage_at_observed_ceiling());
            assert_eq!(
                (
                    append_calls.get(),
                    reconcile_calls.get(),
                    recovery_calls.get()
                ),
                (1, 1, 1)
            );
            assert_eq!(shared.get(), None);

            let current = store
                .load_imap_uid_cursor("account_a", &name)
                .unwrap()
                .unwrap();
            assert_eq!(current.uid_validity(), 9);
            assert_eq!(
                store
                    .load_imap_uid_metadata("account_a", &name, 7, 1)
                    .unwrap()
                    .unwrap(),
                old_source
            );
            let replacement = store
                .load_imap_uid_metadata("account_a", &name, 9, 1)
                .unwrap()
                .unwrap();
            assert_ne!(replacement, old_source);
            assert_eq!(
                replacement.envelope().fields().subject.as_deref(),
                Some(b"epoch-9-uid-1".as_slice())
            );
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
                    .load_imap_uid_observation("account_a", &name, 7, 3)
                    .unwrap()
                    .unwrap()
                    .remote_absent()
            );
            assert_eq!(h.reconcile(&old_request).await, old_done);
            assert_eq!(
                h.page(&request("recovery-old-page", 1, 0))
                    .await
                    .cursor_version(),
                Some(1)
            );
            h.append_complete("recovery-new-append", 2).await;
            assert_eq!(h.cursor().await.cursor_version(), 3);
            assert_eq!(h.recover(&first_request).await, done);
            assert_eq!(h.recovery_status(id).await, done);
            assert_eq!(recovery_calls.get(), 1);
            assert_eq!(append_calls.get(), 2);
            let first_new = h
                .reconcile(&reconcile_request("recovery-new-observation", 1, 3))
                .await;
            let first_new = h.reconcile_terminal(*first_new.execution_id()).await;
            let progress = first_new.result().unwrap();
            assert_eq!(
                (
                    progress.generation(),
                    progress.source_cursor_version(),
                    progress.frozen_cursor_version(),
                    progress.remaining_pages()
                ),
                (1, 2, 3, 1)
            );
            let next_new = h
                .reconcile(&reconcile_request("recovery-next-observation", 1, 3))
                .await;
            let next_new = h.reconcile_terminal(*next_new.execution_id()).await;
            assert_eq!(next_new.result().unwrap().source_cursor_version(), 3);
            assert!(next_new.result().unwrap().pass_complete());
            assert_eq!(reconcile_calls.get(), 3);
            active_validity.set(11);
            let changing = h.recover(&recovery_request("recovery-cancel", 1, 3)).await;
            entered.notified().await;
            let before_catalog = h.inspect().await;
            catalog(&store, 1);
            let after_catalog = h.inspect().await;
            assert_eq!(after_catalog.catalog_generation(), 2);
            assert!(after_catalog.binding_current());
            assert_eq!(after_catalog.observation(), before_catalog.observation());
            assert_eq!(recovery_calls.get(), 2);

            assert_eq!(
                h.recovery_terminal(*changing.execution_id()).await.error(),
                Some(ErrorCode::Conflict)
            );
            assert_eq!(recovery_dropped.get(), 2);
            let current = store
                .load_imap_uid_cursor("account_a", &name)
                .unwrap()
                .unwrap();
            assert_eq!((current.version(), current.uid_validity()), (3, 9));
            assert!(
                store
                    .load_imap_uid_metadata("account_a", &name, 11, 1)
                    .unwrap()
                    .is_none()
            );
            let pending = h
                .recover(&recovery_request("recovery-interrupted", 2, 3))
                .await;
            interrupted.set(*pending.execution_id());
            entered.notified().await;
            assert_eq!(recovery_calls.get(), 3);
        };
        tokio::pin!(flow);
        let run = worker.run();
        tokio::pin!(run);
        tokio::time::timeout(Duration::from_secs(10),async {
            tokio::select!{_=&mut flow=>{},result=&mut run=>panic!("recovery worker stopped: {result:?}")}
        }).await.expect("bounded signed-pipe recovery scenario");
    }
    assert_eq!(shared.get(), None);
    assert_eq!(recovery_dropped.get(), 3);
    assert!(
        store
            .load_audit(interrupted.get())
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
    let (restart, restart_worker) =
        crate::uid_worker::coordinator_with_discovery(&store, NoProvider, shared.clone());
    h.uid = &restart;
    {
        let flow = async {
            assert_eq!(
                h.recovery_status(interrupted.get()).await.state(),
                UidPageState::Unresolved
            );
            let retry = h
                .recover(&recovery_request("recovery-interrupted", 2, 3))
                .await;
            assert_eq!(retry.execution_id(), &interrupted.get());
            assert_eq!(retry.state(), UidPageState::Unresolved);
            assert_eq!(shared.get(), None);
            tokio::task::yield_now().await;
            assert_eq!(recovery_calls.get(), 3);
            assert_eq!(
                h.recovery_status(interrupted.get()).await.state(),
                UidPageState::Unresolved
            );
        };
        tokio::pin!(flow);
        let run = restart_worker.run();
        tokio::pin!(run);
        tokio::time::timeout(Duration::from_secs(3),async {
            tokio::select!{_=&mut flow=>{},result=&mut run=>panic!("restarted recovery worker stopped: {result:?}")}
        }).await.expect("bounded signed-pipe recovery restart scenario");
    }
    for audit in store.list_audit_page(None, 100).unwrap() {
        assert_eq!(
            audit.completion().is_none(),
            *audit.execution_id() == interrupted.get()
        );
    }
    assert_eq!(
        store
            .load_imap_uid_cursor("account_a", &name)
            .unwrap()
            .unwrap()
            .version(),
        3
    );
}
