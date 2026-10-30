use super::*;
use hantong_contracts::{
    audit::{AuditBegin, AuditTool},
    uid_page::{UidMailboxEncoding, UidMailboxIdentity, UidPageState, UidPageStatusRequest},
};
use hantong_domain::{
    capabilities::ImapCapabilities,
    mail_connection::MailConnectionConfig,
    mailboxes::{
        ImapMailboxDescriptor, ImapMailboxName, ImapMailboxSnapshot, MailboxChildren,
        MailboxEncoding,
    },
};
use hantong_store::{imap_catalog::imap_catalog_payload_hash, mail_connection::MailMutation};
use hantong_test_support::policy_store::{PolicyFixtureMutation, PolicyStoreFixture};
use tokio::sync::Notify;
struct Keys(
    Option<hantong_platform::ServerSigningKey>,
    Option<hantong_platform::ClientSigningKey>,
);
impl Drop for Keys {
    fn drop(&mut self) {
        if let Some(key) = self.0.take() {
            let _ = key.delete();
        }
        if let Some(key) = self.1.take() {
            let _ = key.delete();
        }
    }
}
fn fixture() -> (PolicyStoreFixture, Keys) {
    let keys = Keys(
        Some(
            hantong_platform::ServerSigningKey::create(hantong_platform::random_nonce().unwrap())
                .unwrap(),
        ),
        Some(hantong_platform::ClientSigningKey::create().unwrap()),
    );
    let fixture = PolicyStoreFixture::new(
        keys.0.as_ref().unwrap(),
        keys.1.as_ref().unwrap().public_key().unwrap(),
    )
    .unwrap();
    fixture.mutate(PolicyFixtureMutation::AllowUidPage).unwrap();
    (fixture, keys)
}
fn catalog(store: &Store, previous: u64) {
    let endpoint=MailConnectionConfig::from_json(br#"{"schema_version":1,"account_id":"account_a","imap":{"settings_version":1,"host":"imap.fixture.invalid","port":993,"tls":"implicit_tls","login_name":"fixture-user","authentication":"app_password"}}"#).unwrap().imap().clone();
    let mailbox = ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap();
    let snapshot = ImapMailboxSnapshot::new(
        "account_a",
        1,
        ImapCapabilities::from_features([]),
        vec![
            ImapMailboxDescriptor::new(
                mailbox,
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
    let id = format!("catalog-{previous}");
    let mutation = MailMutation::new(
        id.clone(),
        id,
        previous,
        imap_catalog_payload_hash(1, &endpoint, &snapshot, previous).unwrap(),
    )
    .unwrap();
    store
        .replace_imap_mailbox_snapshot(1, &endpoint, &snapshot, &mutation)
        .unwrap();
}
fn request(op: &str) -> UidPageRequest {
    hantong_ipc_client::uid_page::prepare_page_request(
        "account_a",
        UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap(),
        op,
        op,
        1,
        0,
    )
    .unwrap()
}
fn begin(store: &Store, fixture: &PolicyStoreFixture, id: u8) {
    let registration = store
        .load_registration(fixture.registration_id)
        .unwrap()
        .unwrap();
    store
        .begin_audit(
            &AuditBegin::new(
                [id; 32],
                fixture.registration_id,
                registration.profile_epoch,
                AuditTool::SyncPage,
                vec!["account_a".into()],
            )
            .unwrap(),
        )
        .unwrap();
}
fn receipt(
    store: &Store,
    fixture: &PolicyStoreFixture,
    id: u8,
) -> hantong_contracts::uid_page::UidPageReceipt {
    store
        .load_uid_page_receipt(
            fixture.registration_id,
            &UidPageStatusRequest::new("account_a", [id; 32]).unwrap(),
        )
        .unwrap()
}
#[derive(Clone)]
struct Fake {
    calls: Rc<Cell<u32>>,
    dropped: Rc<Cell<u32>>,
    entered: Rc<Notify>,
    release: Rc<Notify>,
    wait: bool,
}
impl Fake {
    fn new(wait: bool) -> Self {
        Self {
            calls: Rc::new(Cell::new(0)),
            dropped: Rc::new(Cell::new(0)),
            entered: Rc::new(Notify::new()),
            release: Rc::new(Notify::new()),
            wait,
        }
    }
}
struct DropSignal(Rc<Cell<u32>>);
impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
impl Discovery for Fake {
    async fn discover(&self, job: &UidPageJob) -> Result<ImapUidPage, ErrorCode> {
        self.calls.set(self.calls.get() + 1);
        let _drop = DropSignal(self.dropped.clone());
        self.entered.notify_one();
        if self.wait {
            self.release.notified().await;
        }
        ImapUidPage::new(job.fetch_request().clone(), 7, 65, vec![])
            .map_err(|_| ErrorCode::InternalError)
    }
}
#[tokio::test(flavor = "current_thread")]
async fn sync02_uid_worker_success_and_replay_never_repeat_provider() {
    let (fixture, _keys) = fixture();
    let store = fixture.open().unwrap();
    catalog(&store, 0);
    let fake = Fake::new(false);
    let (handle, worker) =
        coordinator_with_discovery(&store, fake.clone(), Rc::new(Cell::new(None)));
    let req = request("first");
    begin(&store, &fixture, 1);
    assert!(matches!(
        handle.admit([1; 32], &req).unwrap(),
        UidPageAdmission::Admitted(_)
    ));
    drop(handle);
    worker.run().await.unwrap();
    assert_eq!(
        receipt(&store, &fixture, 1).state(),
        UidPageState::Succeeded
    );
    assert_eq!(fake.calls.get(), 1);
    let (handle, worker) =
        coordinator_with_discovery(&store, fake.clone(), Rc::new(Cell::new(None)));
    begin(&store, &fixture, 2);
    assert!(matches!(
        handle.admit([2; 32], &req).unwrap(),
        UidPageAdmission::Existing(_)
    ));
    drop(handle);
    worker.run().await.unwrap();
    assert_eq!(fake.calls.get(), 1);
}
#[tokio::test(flavor = "current_thread")]
async fn sync02_uid_worker_stale_catalog_before_start_never_calls_provider() {
    let (fixture, _keys) = fixture();
    let store = fixture.open().unwrap();
    catalog(&store, 0);
    let fake = Fake::new(false);
    let (handle, worker) =
        coordinator_with_discovery(&store, fake.clone(), Rc::new(Cell::new(None)));
    begin(&store, &fixture, 1);
    handle.admit([1; 32], &request("stale")).unwrap();
    catalog(&store, 1);
    worker.run_one([1; 32]).await.unwrap();
    assert_eq!(fake.calls.get(), 0);
    assert_eq!(
        receipt(&store, &fixture, 1).error(),
        Some(ErrorCode::Conflict)
    );
}
#[tokio::test(flavor = "current_thread")]
async fn sec03_uid_worker_drops_network_future_when_binding_changes() {
    let (fixture, _keys) = fixture();
    let store = fixture.open().unwrap();
    catalog(&store, 0);
    let fake = Fake::new(true);
    let (handle, worker) =
        coordinator_with_discovery(&store, fake.clone(), Rc::new(Cell::new(None)));
    begin(&store, &fixture, 1);
    handle.admit([1; 32], &request("cancel")).unwrap();
    let change = async {
        fake.entered.notified().await;
        catalog(&store, 1);
    };
    let (result, ()) = tokio::join!(worker.run_one([1; 32]), change);
    result.unwrap();
    assert_eq!(fake.calls.get(), 1);
    assert_eq!(fake.dropped.get(), 1);
    assert_eq!(
        receipt(&store, &fixture, 1).error(),
        Some(ErrorCode::Conflict)
    );
}
#[tokio::test(flavor = "current_thread")]
async fn sec03_uid_worker_shared_slot_queue_failure_and_idle_drop_preserve_other_owner() {
    let (fixture, _keys) = fixture();
    let store = fixture.open().unwrap();
    catalog(&store, 0);
    let shared = Rc::new(Cell::new(Some([99; 32])));
    let (handle, worker) = coordinator_with_discovery(&store, NoDiscovery, shared.clone());
    begin(&store, &fixture, 1);
    assert!(matches!(
        handle.admit([1; 32], &request("full")).unwrap(),
        UidPageAdmission::Failed(_)
    ));
    assert_eq!(
        receipt(&store, &fixture, 1).error(),
        Some(ErrorCode::RateLimited)
    );
    drop(worker);
    assert_eq!(shared.get(), Some([99; 32]));
    begin(&store, &fixture, 2);
    assert!(matches!(
        handle.admit([2; 32], &request("closed")).unwrap(),
        UidPageAdmission::Failed(_)
    ));
    assert_eq!(
        receipt(&store, &fixture, 2).error(),
        Some(ErrorCode::CoreUnavailable)
    );
}
