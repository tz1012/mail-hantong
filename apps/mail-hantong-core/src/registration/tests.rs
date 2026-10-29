use super::*;
use std::cell::RefCell;

fn public() -> [u8; 72] {
    let mut key = [0; 72];
    key[..8].copy_from_slice(b"ECS1\x20\0\0\0");
    // Exact public P-256 generator; no private key or actual registration.
    key[8..40].copy_from_slice(
        &hantong_contracts::wire_hex::decode_fixed::<32>(
            "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
        )
        .unwrap(),
    );
    key[40..].copy_from_slice(
        &hantong_contracts::wire_hex::decode_fixed::<32>(
            "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
        )
        .unwrap(),
    );
    key
}
fn request() -> RegistrationRequest {
    RegistrationRequest {
        request_id: [1; 32],
        client_kind: ClientKind::Cli,
        display_label: DEFAULT_CLI_LABEL.into(),
        scope: RequestedScope::new(vec!["a".into()], vec!["mail.accounts.list".into()]).unwrap(),
        state: RegistrationRequestState::Pending,
        version: 1,
        created_at_utc_ms: 1000,
        expires_at_utc_ms: 301000,
    }
}
fn review() -> RegistrationReview {
    RegistrationReview::new(
        [1; 32],
        1,
        vec!["a".into()],
        hantong_platform::sha256(&public()).unwrap(),
        1000,
        301000,
    )
    .unwrap()
}
fn record() -> RegistrationRecord {
    RegistrationRecord {
        registration_id: hantong_platform::registration_id_for_request([1; 32]).unwrap(),
        request_id: [1; 32],
        client_kind: ClientKind::Cli,
        display_label: DEFAULT_CLI_LABEL.into(),
        public_key: public(),
        profile_epoch: 1,
        state: RegistrationState::Provisioning,
        read_only: true,
        requests_per_minute: 60,
        approved_channel: "approval_helper".into(),
        version: 1,
        scope: request().scope,
    }
}
struct Fake {
    events: Rc<RefCell<Vec<&'static str>>>,
    fail: Option<&'static str>,
    current: RefCell<RegistrationRequest>,
    now: i64,
    wrong_key: bool,
    cleanup_fails: Cell<bool>,
    linked: Cell<bool>,
    missing: Cell<bool>,
    diagnostics: RefCell<Vec<ErrorCode>>,
    uncertain_commit: Cell<bool>,
}
impl Fake {
    fn new() -> Self {
        Self {
            events: Rc::new(RefCell::new(vec![])),
            fail: None,
            current: RefCell::new(request()),
            now: 2000,
            wrong_key: false,
            cleanup_fails: Cell::new(false),
            linked: Cell::new(false),
            missing: Cell::new(true),
            diagnostics: RefCell::new(vec![]),
            uncertain_commit: Cell::new(false),
        }
    }
    fn hit(&self, event: &'static str) -> Result<(), ErrorCode> {
        self.events.borrow_mut().push(event);
        if self.fail == Some(event) {
            Err(ErrorCode::Conflict)
        } else {
            Ok(())
        }
    }
    fn events(&self) -> Vec<&'static str> {
        self.events.borrow().clone()
    }
}
struct Publication(Rc<RefCell<Vec<&'static str>>>);
impl Drop for Publication {
    fn drop(&mut self) {
        self.0.borrow_mut().push("release_publication");
    }
}
impl Effects for Fake {
    type Key = ();
    type Publication = Publication;
    fn linked(&self, _: [u8; 32], _: [u8; 72]) -> Result<bool, ErrorCode> {
        self.hit("linked")?;
        Ok(self.linked.get())
    }
    fn orphans(&self) -> Result<Vec<RegistrationRequest>, ErrorCode> {
        self.hit("orphans")?;
        Ok(vec![self.current.borrow().clone()])
    }
    fn provisioning(&self) -> Result<Vec<RegistrationRecord>, ErrorCode> {
        self.hit("provisioning")?;
        Ok(vec![])
    }
    fn report(&self, error: ErrorCode) {
        self.diagnostics.borrow_mut().push(error);
    }
    fn request(&self, _: [u8; 32]) -> Result<RegistrationRequest, ErrorCode> {
        self.hit("request")?;
        Ok(self.current.borrow().clone())
    }
    fn server_public(&self) -> Result<[u8; 72], ErrorCode> {
        self.hit("server")?;
        Ok(public())
    }
    fn now(&self) -> Result<i64, ErrorCode> {
        Ok(self.now)
    }
    fn create_key(&self, id: [u8; 16]) -> Result<(), ErrorCode> {
        assert_eq!(
            id,
            hantong_platform::registration_id_for_request([1; 32]).unwrap()
        );
        self.hit("create")?;
        self.missing.set(false);
        Ok(())
    }
    fn open_key(&self, _: [u8; 16]) -> Result<(), ErrorCode> {
        self.hit("open")?;
        if self.missing.get() {
            Err(ErrorCode::NotFound)
        } else {
            Ok(())
        }
    }
    fn key_public(&self, _: &()) -> Result<[u8; 72], ErrorCode> {
        self.hit("key_public")?;
        let mut key = public();
        if self.wrong_key {
            key[8] ^= 1;
        }
        Ok(key)
    }
    fn delete_key(&self, _: ()) -> Result<(), ErrorCode> {
        self.hit("delete")?;
        if self.cleanup_fails.get() {
            Err(ErrorCode::InternalError)
        } else {
            self.missing.set(true);
            Ok(())
        }
    }
    fn stage(
        &self,
        r: RegistrationRequest,
        id: [u8; 16],
        key: [u8; 72],
        hash: [u8; 32],
    ) -> Result<RegistrationRecord, ErrorCode> {
        assert_eq!(r, request());
        assert_eq!(
            id,
            hantong_platform::registration_id_for_request([1; 32]).unwrap()
        );
        assert_eq!(key, public());
        assert_eq!(hash, *review().server_public_key_hash());
        if self.uncertain_commit.get() {
            self.linked.set(true);
        }
        self.hit("stage")?;
        Ok(record())
    }
    fn publish(&self, bytes: &[u8]) -> Result<Publication, ErrorCode> {
        let p = ParsedClientProfile::from_json(bytes).unwrap();
        assert_eq!(
            p.registration_id(),
            &hantong_platform::registration_id_for_request([1; 32]).unwrap()
        );
        assert_eq!(p.scope(), &request().scope);
        self.hit("publish")?;
        Ok(Publication(self.events.clone()))
    }
    fn activate(&self, r: &RegistrationRecord, hash: [u8; 32]) -> Result<(), ErrorCode> {
        assert_eq!(r, &record());
        assert_eq!(hash, *review().server_public_key_hash());
        self.hit("activate")
    }
}
#[test]
fn cli15_consumed_review_rechecks_then_stages_publishes_and_activates_with_guard() {
    let f = Fake::new();
    assert_eq!(commit_consumed_review(&f, &request(), &review()), Ok(()));
    assert_eq!(
        f.events(),
        vec![
            "request",
            "server",
            "open",
            "create",
            "key_public",
            "stage",
            "publish",
            "activate",
            "release_publication"
        ]
    );
}
#[test]
fn cli15_no_key_is_created_for_stale_expired_or_wrong_review() {
    for mode in 0..4 {
        let mut f = Fake::new();
        let mut review = review();
        match mode {
            0 => f.current.borrow_mut().version += 1,
            1 => f.now = 301000,
            2 => f.now = 999,
            _ => {
                review =
                    RegistrationReview::new([1; 32], 1, vec!["b".into()], [9; 32], 1000, 301000)
                        .unwrap()
            }
        };
        assert!(commit_consumed_review(&f, &request(), &review).is_err());
        assert!(!f.events().contains(&"create"));
    }
}
#[test]
fn cli15_only_new_key_before_successful_stage_is_deleted() {
    for failure in ["create", "key_public", "stage", "publish", "activate"] {
        let mut f = Fake::new();
        f.fail = Some(failure);
        assert_eq!(
            commit_consumed_review(&f, &request(), &review()),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(f.events().contains(&"delete"), matches!(failure, "stage"));
        if matches!(failure, "publish" | "activate") {
            assert!(f.events().contains(&"stage"));
        }
    }
}
#[test]
fn cli15_recovery_opens_existing_key_and_reuses_profile_without_create_or_delete() {
    let f = Fake::new();
    f.current.borrow_mut().state = RegistrationRequestState::Approved;
    f.missing.set(false);
    f.current.borrow_mut().version = 2;
    assert_eq!(recover_record(&f, &record()), Ok(()));
    assert_eq!(
        f.events(),
        vec![
            "request",
            "server",
            "open",
            "key_public",
            "publish",
            "activate",
            "release_publication"
        ]
    );
}
#[test]
fn cli15_recovery_failures_preserve_provisioning_key_and_do_not_activate() {
    for failure in ["open", "publish"] {
        let mut f = Fake::new();
        f.current.borrow_mut().state = RegistrationRequestState::Approved;
        f.missing.set(false);
        f.current.borrow_mut().version = 2;
        f.fail = Some(failure);
        assert_eq!(recover_record(&f, &record()), Err(ErrorCode::Conflict));
        assert!(!f.events().contains(&"create"));
        assert!(!f.events().contains(&"delete"));
        assert!(!f.events().contains(&"activate"));
    }
    let mut f = Fake::new();
    f.current.borrow_mut().state = RegistrationRequestState::Approved;
    f.missing.set(false);
    f.wrong_key = true;
    assert!(recover_record(&f, &record()).is_err());
    assert!(!f.events().contains(&"publish"));
}
#[test]
fn cli15_recovery_rejects_nondefault_or_changed_grants() {
    for mode in 0..3 {
        let f = Fake::new();
        f.current.borrow_mut().state = RegistrationRequestState::Approved;
        f.missing.set(false);
        let mut r = record();
        match mode {
            0 => r.display_label = "other".into(),
            1 => r.state = RegistrationState::Active,
            _ => r.scope = RequestedScope::new(vec![], vec!["mail.accounts.list".into()]).unwrap(),
        };
        assert!(recover_record(&f, &r).is_err());
        assert!(!f.events().contains(&"open"));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn cli15_pending_window_deduplicates_and_cancel_releases_for_explicit_retry() {
    let scheduled = Cell::new(None);
    let (sender, mut receiver) = mpsc::channel(1);
    enqueue(&scheduled, &sender, [1; 32]).unwrap();
    enqueue(&scheduled, &sender, [1; 32]).unwrap();
    assert_eq!(
        enqueue(&scheduled, &sender, [2; 32]),
        Err(ErrorCode::RateLimited)
    );
    assert_eq!(receiver.recv().await, Some([1; 32]));
    assert!(receiver.try_recv().is_err());
    let window = ScheduledWindow(&scheduled);
    // A slow/unfinished review does not hold a database borrow or block IPC.
    let responsive = tokio::time::timeout(std::time::Duration::from_millis(100), async {
        tokio::task::yield_now().await;
        enqueue(&scheduled, &sender, [1; 32])
    })
    .await
    .unwrap();
    assert_eq!(responsive, Ok(()));
    assert!(receiver.try_recv().is_err());
    drop(window); // cancellation or dropping the review clears only local UI state
    assert_eq!(scheduled.get(), None);
    assert!(receiver.try_recv().is_err()); // no automatic retry after cancellation
    enqueue(&scheduled, &sender, [1; 32]).unwrap(); // an explicit request can reopen
    assert_eq!(receiver.recv().await, Some([1; 32]));
}

#[test]
fn cli15_closed_worker_never_marks_a_review_as_scheduled() {
    let scheduled = Cell::new(None);
    let (sender, receiver) = mpsc::channel(1);
    drop(receiver);
    assert_eq!(
        enqueue(&scheduled, &sender, [1; 32]),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(scheduled.get(), None);
}

#[test]
fn cli15_crash_before_stage_cleanup_reopens_only_persisted_selector() {
    let f = Fake::new();
    f.missing.set(false);
    assert_eq!(cleanup_orphan(&f, &request()), Ok(()));
    assert_eq!(f.events(), vec!["open", "key_public", "linked", "delete"]);
    assert_eq!(cleanup_orphan(&f, &request()), Ok(()));
    assert!(!f.events().contains(&"create"));
}
#[test]
fn cli15_failed_cleanup_retries_and_never_deletes_linked_or_uncertain_key() {
    let f = Fake::new();
    f.missing.set(false);
    f.cleanup_fails.set(true);
    assert_eq!(
        cleanup_orphan(&f, &request()),
        Err(ErrorCode::InternalError)
    );
    f.cleanup_fails.set(false);
    assert_eq!(cleanup_orphan(&f, &request()), Ok(()));
    let f = Fake::new();
    f.missing.set(false);
    f.linked.set(true);
    assert_eq!(cleanup_orphan(&f, &request()), Err(ErrorCode::Conflict));
    assert!(!f.events().contains(&"delete"));
    let mut f = Fake::new();
    f.missing.set(false);
    f.fail = Some("linked");
    assert_eq!(cleanup_orphan(&f, &request()), Err(ErrorCode::Conflict));
    assert!(!f.events().contains(&"delete"));
}
#[test]
fn cli15_startup_list_failure_is_fatal_but_item_failure_is_diagnosed() {
    let mut f = Fake::new();
    f.fail = Some("orphans");
    assert_eq!(recover_startup(&f), Err(ErrorCode::Conflict));
    assert!(!f.events().contains(&"open"));
    let f = Fake::new();
    f.missing.set(false);
    f.cleanup_fails.set(true);
    assert_eq!(recover_startup(&f), Ok(()));
    assert_eq!(*f.diagnostics.borrow(), vec![ErrorCode::InternalError]);
}
#[test]
fn cli15_stage_error_keeps_original_code_when_cleanup_also_fails() {
    let mut f = Fake::new();
    f.fail = Some("stage");
    f.cleanup_fails.set(true);
    f.missing.set(true);
    assert_eq!(
        commit_consumed_review(&f, &request(), &review()),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        *f.diagnostics.borrow(),
        vec![ErrorCode::Conflict, ErrorCode::InternalError]
    );
}

#[test]
fn cli15_uncertain_stage_commit_preserves_linked_key() {
    let mut f = Fake::new();
    f.fail = Some("stage");
    f.uncertain_commit.set(true);
    assert_eq!(
        commit_consumed_review(&f, &request(), &review()),
        Err(ErrorCode::Conflict)
    );
    assert!(f.events().contains(&"linked"));
    assert!(!f.events().contains(&"delete"));
    assert!(!f.events().contains(&"publish"));
}
#[test]
fn cli15_cleanup_failure_blocks_new_creation_until_explicit_retry_succeeds() {
    let f = Fake::new();
    f.missing.set(false);
    f.cleanup_fails.set(true);
    assert_eq!(
        commit_consumed_review(&f, &request(), &review()),
        Err(ErrorCode::InternalError)
    );
    assert!(!f.events().contains(&"create"));
    f.cleanup_fails.set(false);
    assert_eq!(commit_consumed_review(&f, &request(), &review()), Ok(()));
    let events = f.events();
    let created = events.iter().position(|s| *s == "create").unwrap();
    assert_eq!(
        events[..created].iter().filter(|s| **s == "delete").count(),
        2
    );
}
#[test]
fn cli15_orphan_cleanup_preserves_explicit_denial_and_diagnostics_have_only_stable_fields() {
    let f = Fake::new();
    f.missing.set(false);
    let mut denied = request();
    denied.state = RegistrationRequestState::Denied;
    assert_eq!(cleanup_orphan(&f, &denied), Ok(()));
    assert_eq!(denied.state, RegistrationRequestState::Denied);
    let line = registration_failure_line(ErrorCode::InternalError, 1234).unwrap();
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    let object = value.as_object().unwrap();
    assert_eq!(object.len(), 5);
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["event"], "operation_failed");
    assert_eq!(value["error_code"], "INTERNAL_ERROR");
    assert!(!line.contains("request"));
    assert!(!line.contains("key"));
    assert!(!line.contains("path"));
}

#[test]
fn setup01_registration_and_imap_share_one_gate_and_status_never_schedules() {
    use hantong_contracts::imap_setup::{ImapSetupInput, ImapSetupState, ImapSetupTls};
    struct Key(Option<ServerSigningKey>);
    impl Drop for Key {
        fn drop(&mut self) {
            self.0.take().unwrap().delete().unwrap();
        }
    }
    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let key = Key(Some(
        ServerSigningKey::create(hantong_platform::random_nonce().unwrap()).unwrap(),
    ));
    let root = Directory(std::env::temp_dir().join(format!(
        "hantong-core-queue-{}",
        hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
    )));
    std::fs::create_dir(&root.0).unwrap();
    let store = Store::open(root.0.join("data"), root.0.join("keys")).unwrap();
    let secrets = root.0.join("keys");
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    let input = ImapSetupInput::new(
        None,
        None,
        "IMAP.EXAMPLE".into(),
        993,
        ImapSetupTls::ImplicitTls,
        "user".into(),
    )
    .unwrap();
    let cmd = || SetupCommand::Imap(ImapSetupCommand::Request(input.clone()));
    let SetupReceipt::Imap(receipt) = handle.setup_command(cmd()).unwrap() else {
        panic!("wrong family");
    };
    assert_eq!(receipt.state(), ImapSetupState::Pending);
    let id = *receipt.request_id();
    assert_eq!(worker.receiver.try_recv().unwrap(), Job::Imap(id));
    assert!(
        matches!(handle.setup_command(cmd()).unwrap(),SetupReceipt::Imap(value) if value==receipt)
    );
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(store.list_imap_setup_requests().unwrap().len(), 1);
    assert_eq!(
        handle.command(RegistrationCommand::Request),
        Err(ErrorCode::RateLimited)
    );
    let other = ImapSetupInput::new(
        None,
        None,
        "other.example".into(),
        993,
        ImapSetupTls::ImplicitTls,
        "user".into(),
    )
    .unwrap();
    assert!(matches!(
        handle.setup_command(SetupCommand::Imap(ImapSetupCommand::Request(other))),
        Err(ErrorCode::RateLimited)
    ));
    assert!(
        matches!(handle.setup_command(SetupCommand::Imap(ImapSetupCommand::Status(id))).unwrap(),SetupReceipt::Imap(value) if value==receipt)
    );
    assert!(worker.receiver.try_recv().is_err());
    drop(worker);
    let SetupReceipt::Imap(interrupted) = handle
        .setup_command(SetupCommand::Imap(ImapSetupCommand::Status(id)))
        .unwrap()
    else {
        panic!("wrong family");
    };
    assert_eq!(interrupted.state(), ImapSetupState::Interrupted);
    assert!(matches!(
        handle.setup_command(cmd()),
        Err(ErrorCode::InternalError)
    ));
    assert_eq!(store.list_imap_setup_requests().unwrap().len(), 1);
    assert!(store.list_registrable_account_ids().unwrap().is_empty());
    drop(handle);
    let resume = || SetupCommand::Imap(ImapSetupCommand::Resume(id));
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    let SetupReceipt::Imap(child) = handle.setup_command(resume()).unwrap() else {
        panic!("wrong family");
    };
    assert_ne!(child.request_id(), &id);
    assert_eq!(
        worker.receiver.try_recv().unwrap(),
        Job::Imap(*child.request_id())
    );
    assert!(
        matches!(handle.setup_command(resume()).unwrap(),SetupReceipt::Imap(value) if value==child)
    );
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(store.list_imap_setup_requests().unwrap().len(), 2);
    drop(worker);
    drop(handle);
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    let SetupReceipt::Imap(grandchild) = handle.setup_command(resume()).unwrap() else {
        panic!("wrong family");
    };
    assert_ne!(grandchild.request_id(), child.request_id());
    assert_eq!(
        worker.receiver.try_recv().unwrap(),
        Job::Imap(*grandchild.request_id())
    );
    assert_eq!(store.list_imap_setup_requests().unwrap().len(), 3);
    assert!(store.list_registrable_account_ids().unwrap().is_empty());
    drop(worker);
    drop(handle);
    let pending = store
        .load_imap_setup_request(*grandchild.request_id())
        .unwrap()
        .unwrap();
    let staged = store.stage_imap_setup_request(&pending).unwrap();
    let started = store.begin_imap_credential_store(&staged).unwrap();
    let returned = store
        .finish_imap_credential_store(&started, Ok(()))
        .unwrap();
    let probing = store.begin_initial_imap_probe(&returned).unwrap();
    store
        .finish_imap_setup_probe(
            &probing,
            &hantong_store::account_provisioning::InitialImapProbeOutcome::Ready(vec![]),
        )
        .unwrap();
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    let SetupReceipt::Imap(completed) = handle.setup_command(resume()).unwrap() else {
        panic!("wrong family");
    };
    assert_eq!(completed.request_id(), grandchild.request_id());
    assert_eq!(completed.state(), ImapSetupState::Ready);
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(store.list_imap_setup_requests().unwrap().len(), 3);
    drop(worker);
    drop(handle);
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    enqueue(
        &handle.state.scheduled,
        &handle.sender,
        Job::Registration([9; 32]),
    )
    .unwrap();
    assert!(matches!(
        handle.setup_command(cmd()),
        Err(ErrorCode::RateLimited)
    ));
    assert_eq!(
        worker.receiver.try_recv().unwrap(),
        Job::Registration([9; 32])
    );
    assert_eq!(store.list_imap_setup_requests().unwrap().len(), 3);
    assert!(matches!(
        handle.setup_command(resume()),
        Err(ErrorCode::RateLimited)
    ));
}
