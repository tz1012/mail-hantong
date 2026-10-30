use super::*;
use hantong_contracts::initial_sync::{
    InitialSyncJobState, InitialSyncMutation, InitialSyncPlanRequest, InitialSyncState,
};

fn identity() -> UidMailboxIdentity {
    UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap()
}
fn mutation(op: &str) -> InitialSyncMutation {
    InitialSyncMutation::new(op, op, &"00".repeat(32)).unwrap()
}
fn prepare(request: InitialSyncRequest) -> InitialSyncRequest {
    hantong_ipc_client::initial_sync::prepare_request(request).unwrap()
}
fn start_request() -> InitialSyncRequest {
    prepare(
        InitialSyncRequest::new_start("account_a", identity(), mutation("initial"), 1, 0, 2)
            .unwrap(),
    )
}
fn resume_request(op: &str, version: u64) -> InitialSyncRequest {
    prepare(
        InitialSyncRequest::new_resume("account_a", identity(), mutation(op), [1; 32], version, 1)
            .unwrap(),
    )
}
fn current(
    store: &Store,
    fixture: &PolicyStoreFixture,
) -> hantong_contracts::initial_sync::InitialSyncPlanSnapshot {
    store
        .load_initial_sync_plan(
            fixture.registration_id,
            &InitialSyncPlanRequest::new("account_a", identity(), [1; 32]).unwrap(),
        )
        .unwrap()
}
fn initial_receipt(
    store: &Store,
    fixture: &PolicyStoreFixture,
    id: u8,
) -> hantong_contracts::initial_sync::InitialSyncReceipt {
    store
        .load_initial_sync_receipt(
            fixture.registration_id,
            &UidPageStatusRequest::new("account_a", [id; 32]).unwrap(),
        )
        .unwrap()
}
#[derive(Clone)]
struct InitialFake {
    signals: Fake,
    observations: Rc<Cell<u32>>,
    pages: Rc<Cell<u32>>,
    page_error: Option<ErrorCode>,
}
impl InitialFake {
    fn new(wait: bool) -> Self {
        Self {
            signals: Fake::new(wait),
            observations: Rc::new(Cell::new(0)),
            pages: Rc::new(Cell::new(0)),
            page_error: None,
        }
    }
}
impl Discovery for InitialFake {
    async fn discover(&self, _: &UidPageJob) -> Result<ImapUidPage, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    async fn initial(&self, job: &InitialSyncJob) -> Result<InitialObservation, ErrorCode> {
        match job.fetch_request() {
            InitialSyncFetch::Observe(request) => {
                self.observations.set(self.observations.get() + 1);
                Ok(InitialObservation::Target(
                    ImapUidObservation::new(request.clone(), 7, 513).unwrap(),
                ))
            }
            InitialSyncFetch::Page(request) => {
                self.pages.set(self.pages.get() + 1);
                let _drop = DropSignal(self.signals.dropped.clone());
                self.signals.entered.notify_one();
                if self.signals.wait {
                    self.signals.release.notified().await;
                }
                if let Some(error) = self.page_error {
                    return Err(error);
                }
                Ok(InitialObservation::Page(
                    ImapUidReconcilePage::new(request.clone(), 513, vec![]).unwrap(),
                ))
            }
        }
    }
}
async fn start_plan(store: &Store, fixture: &PolicyStoreFixture, fake: &InitialFake) {
    let (handle, worker) =
        coordinator_with_instance(store, fake.clone(), Rc::new(Cell::new(None)), [9; 32]);
    begin(store, fixture, 1);
    assert!(matches!(
        handle.admit_initial([1; 32], &start_request()).unwrap(),
        InitialSyncAdmission::Admitted(_)
    ));
    drop(handle);
    worker.run().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn sync01_initial_start_only_observes_and_each_explicit_resume_fetches_one_page() {
    let (fixture, _keys) = fixture();
    let store = fixture.open().unwrap();
    catalog(&store, 0);
    let fake = InitialFake::new(false);
    start_plan(&store, &fixture, &fake).await;
    assert_eq!((fake.observations.get(), fake.pages.get()), (1, 0));
    let original = initial_receipt(&store, &fixture, 1);
    assert_eq!(
        current(&store, &fixture).progress().state(),
        InitialSyncState::Ready
    );
    for (id, state) in [
        (2, InitialSyncState::PausedBudget),
        (3, InitialSyncState::Complete),
    ] {
        let plan = current(&store, &fixture);
        let request = resume_request(&format!("resume-{id}"), plan.version());
        let (handle, worker) =
            coordinator_with_instance(&store, fake.clone(), Rc::new(Cell::new(None)), [9; 32]);
        begin(&store, &fixture, id);
        assert!(matches!(
            handle.admit_initial([id; 32], &request).unwrap(),
            InitialSyncAdmission::Admitted(_)
        ));
        drop(handle);
        worker.run().await.unwrap();
        assert_eq!(current(&store, &fixture).progress().state(), state);
        assert_eq!(
            current(&store, &fixture).progress().completed_page_count(),
            id - 1
        );
        assert_eq!(fake.pages.get(), u32::from(id - 1));
        let (handle, worker) =
            coordinator_with_instance(&store, fake.clone(), Rc::new(Cell::new(None)), [9; 32]);
        begin(&store, &fixture, id + 10);
        assert!(matches!(
            handle.admit_initial([id + 10; 32], &request).unwrap(),
            InitialSyncAdmission::Existing(_)
        ));
        drop(handle);
        worker.run().await.unwrap();
        assert_eq!(fake.pages.get(), u32::from(id - 1));
    }
    assert_eq!(initial_receipt(&store, &fixture, 1), original);
    let mailbox = ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap();
    assert!(
        store
            .load_imap_uid_cursor("account_a", &mailbox)
            .unwrap()
            .is_none()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn sync01_initial_pause_interrupts_network_without_committing_or_resubmitting_page() {
    let (fixture, _keys) = fixture();
    let store = fixture.open().unwrap();
    catalog(&store, 0);
    let fake = InitialFake::new(true);
    start_plan(&store, &fixture, &fake).await;
    let (handle, worker) =
        coordinator_with_instance(&store, fake.clone(), Rc::new(Cell::new(None)), [9; 32]);
    begin(&store, &fixture, 2);
    handle
        .admit_initial(
            [2; 32],
            &resume_request("resume", current(&store, &fixture).version()),
        )
        .unwrap();
    let pause = async {
        fake.signals.entered.notified().await;
        let plan = current(&store, &fixture);
        let req = prepare(
            InitialSyncRequest::new_pause(
                "account_a",
                identity(),
                mutation("pause"),
                [1; 32],
                plan.version(),
            )
            .unwrap(),
        );
        begin(&store, &fixture, 3);
        let InitialSyncAdmission::Admitted(receipt) = handle.admit_initial([3; 32], &req).unwrap()
        else {
            panic!("pause must finish locally")
        };
        assert_eq!(receipt.state(), InitialSyncJobState::Succeeded);
    };
    let (result, ()) = tokio::join!(worker.run_initial([2; 32]), pause);
    result.unwrap();
    assert_eq!(fake.pages.get(), 1);
    assert_eq!(fake.signals.dropped.get(), 1);
    let plan = current(&store, &fixture);
    assert_eq!(plan.progress().completed_page_count(), 0);
    assert_eq!(plan.progress().state(), InitialSyncState::PausedUser);
    assert_eq!(
        initial_receipt(&store, &fixture, 2).state(),
        InitialSyncJobState::Failed
    );
}

#[tokio::test(flavor = "current_thread")]
async fn sync01_initial_provider_failure_is_terminal_and_has_no_automatic_retry() {
    let (fixture, _keys) = fixture();
    let store = fixture.open().unwrap();
    catalog(&store, 0);
    let mut fake = InitialFake::new(false);
    fake.page_error = Some(ErrorCode::Offline);
    start_plan(&store, &fixture, &fake).await;
    let (handle, worker) =
        coordinator_with_instance(&store, fake.clone(), Rc::new(Cell::new(None)), [9; 32]);
    begin(&store, &fixture, 2);
    handle
        .admit_initial([2; 32], &resume_request("offline", 1))
        .unwrap();
    drop(handle);
    worker.run().await.unwrap();
    assert_eq!(fake.pages.get(), 1);
    assert_eq!(
        initial_receipt(&store, &fixture, 2).error(),
        Some(ErrorCode::Offline)
    );
    assert_eq!(
        current(&store, &fixture).progress().completed_page_count(),
        0
    );
}
