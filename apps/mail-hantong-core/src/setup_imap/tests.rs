use super::*;
use std::{
    cell::{Cell, RefCell},
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    store: Store,
    root: Cleanup,
}
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-core-imap-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self {
            store: Store::open(root.join("data"), root.join("keys")).unwrap(),
            root: Cleanup(root),
        }
    }
    fn pending(&self) -> ImapSetupRequest {
        self.store
            .create_imap_setup_request(
                &intent(&input(), [1; 32], [2; 32], [3; 32], [4; 32]).unwrap(),
            )
            .unwrap()
    }
}
fn input() -> ImapSetupInput {
    ImapSetupInput::new(
        Some("이름".into()),
        None,
        "IMAP.EXAMPLE".into(),
        993,
        ImapSetupTls::ImplicitTls,
        "user name".into(),
    )
    .unwrap()
}
fn run<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}
struct Fake<'a> {
    store: &'a Store,
    events: RefCell<Vec<&'static str>>,
    fail: Option<&'static str>,
    uncertain: bool,
    credential_error: bool,
    outcome: InitialImapProbeOutcome,
    probes: Cell<usize>,
    block_probe: bool,
}
impl<'a> Fake<'a> {
    fn new(store: &'a Store) -> Self {
        Self {
            store,
            events: RefCell::new(vec![]),
            fail: None,
            uncertain: false,
            credential_error: false,
            outcome: InitialImapProbeOutcome::Ready(vec!["imap.idle".into()]),
            probes: Cell::new(0),
            block_probe: false,
        }
    }
    fn hit(&self, name: &'static str) -> Result<(), ErrorCode> {
        self.events.borrow_mut().push(name);
        if self.fail == Some(name) && !self.uncertain {
            Err(ErrorCode::InternalError)
        } else {
            Ok(())
        }
    }
    fn returned(
        &self,
        name: &str,
        request: ImapSetupRequest,
    ) -> Result<ImapSetupRequest, ErrorCode> {
        if self.fail == Some(name) && self.uncertain {
            Err(ErrorCode::InternalError)
        } else {
            Ok(request)
        }
    }
}
impl Effects for Fake<'_> {
    type Pending = ();
    fn load(&self, id: [u8; 32]) -> Result<ImapSetupRequest, ErrorCode> {
        self.hit("load")?;
        self.store
            .load_imap_setup_request(id)?
            .ok_or(ErrorCode::NotFound)
    }
    fn stage(&self, r: &ImapSetupRequest) -> Result<ImapSetupRequest, ErrorCode> {
        self.hit("stage")?;
        self.returned("stage", stage(self.store, r)?)
    }
    fn begin_credential(&self, r: &ImapSetupRequest) -> Result<ImapSetupRequest, ErrorCode> {
        self.hit("begin_credential")?;
        self.returned(
            "begin_credential",
            self.store.begin_imap_credential_store(r)?,
        )
    }
    fn store_credential(&self, _: &ImapSetupRequest, _: ()) -> Result<(), ErrorCode> {
        self.hit("credential")?;
        if self.credential_error {
            Err(ErrorCode::ScopeDenied)
        } else {
            Ok(())
        }
    }
    fn finish_credential(
        &self,
        r: &ImapSetupRequest,
        result: Result<(), ErrorCode>,
    ) -> Result<ImapSetupRequest, ErrorCode> {
        self.hit("finish_credential")?;
        self.returned(
            "finish_credential",
            self.store.finish_imap_credential_store(r, result)?,
        )
    }
    fn begin_probe(&self, r: &ImapSetupRequest) -> Result<ImapSetupRequest, ErrorCode> {
        self.hit("begin_probe")?;
        self.returned("begin_probe", self.store.begin_initial_imap_probe(r)?)
    }
    async fn probe(&self, _: &ImapSetupRequest) -> InitialImapProbeOutcome {
        self.events.borrow_mut().push("probe");
        self.probes.set(self.probes.get() + 1);
        if self.block_probe {
            std::future::pending::<()>().await;
        }
        self.outcome.clone()
    }
    fn finish_probe(
        &self,
        r: &ImapSetupRequest,
        outcome: &InitialImapProbeOutcome,
    ) -> Result<ImapSetupRequest, ErrorCode> {
        self.hit("finish_probe")?;
        self.returned(
            "finish_probe",
            self.store.finish_imap_setup_probe(r, outcome)?,
        )
    }
}

#[test]
fn setup01_intent_revalidates_domain_normalizes_host_and_preserves_login() {
    let i = intent(&input(), [1; 32], [2; 32], [3; 32], [4; 32]).unwrap();
    assert_eq!(i.endpoint().host(), "imap.example");
    assert_eq!(i.endpoint().login_name(), "user name");
    assert_eq!(
        i.mutation().payload_hash(),
        &imap_account_stage_payload_hash(i.draft(), i.endpoint()).unwrap()
    );
    let bad = ImapSetupInput::new(
        None,
        None,
        "https://example".into(),
        993,
        ImapSetupTls::ImplicitTls,
        "user".into(),
    )
    .unwrap();
    assert_eq!(
        intent(&bad, [1; 32], [2; 32], [3; 32], [4; 32]),
        Err(ErrorCode::InvalidInput)
    );
    for slot in 0..4 {
        let mut ids = [[1; 32]; 4];
        ids[slot] = [0; 32];
        assert!(intent(&input(), ids[0], ids[1], ids[2], ids[3]).is_err());
    }
}
#[test]
fn setup01_effects_commit_before_each_external_action_and_finish_atomically() {
    let f = Fixture::new();
    let r = f.pending();
    let effects = Fake::new(&f.store);
    run(finish_verified(&effects, &r, ())).unwrap();
    assert_eq!(
        *effects.events.borrow(),
        [
            "load",
            "stage",
            "begin_credential",
            "credential",
            "finish_credential",
            "begin_probe",
            "probe",
            "finish_probe"
        ]
    );
    let r = f
        .store
        .load_imap_setup_request(*r.intent().request_id())
        .unwrap()
        .unwrap();
    assert_eq!(r.phase(), ImapSetupPhase::ProbeFinished);
    assert_eq!(
        progress(&r, false, false).unwrap().next_action(),
        ImapSetupNextAction::ClientRegistrationRequired
    );
    assert_eq!(
        progress(&r, false, true).unwrap().next_action(),
        ImapSetupNextAction::ProfileScopeReviewRequired
    );
    assert!(f.root.0.exists());
}
#[test]
fn setup01_failures_and_uncertain_commits_never_advance_to_next_effect() {
    let sequence = [
        "load",
        "stage",
        "begin_credential",
        "credential",
        "finish_credential",
        "begin_probe",
        "probe",
        "finish_probe",
    ];
    for fail in [
        "load",
        "stage",
        "begin_credential",
        "finish_credential",
        "begin_probe",
        "finish_probe",
    ] {
        for uncertain in [false, true] {
            let f = Fixture::new();
            let r = f.pending();
            let mut effects = Fake::new(&f.store);
            effects.fail = Some(fail);
            effects.uncertain = uncertain;
            if fail == "load" && uncertain {
                continue;
            }
            assert!(run(finish_verified(&effects, &r, ())).is_err());
            let end = sequence.iter().position(|v| *v == fail).unwrap();
            assert_eq!(*effects.events.borrow(), sequence[..=end]);
            let snapshot = f
                .store
                .load_imap_setup_request(*r.intent().request_id())
                .unwrap()
                .unwrap();
            let count = effects.probes.get();
            let _ = progress(&snapshot, false, false).unwrap();
            assert_eq!(effects.probes.get(), count);
        }
    }
}
#[test]
fn setup01_credential_error_is_durable_and_never_probes() {
    let f = Fixture::new();
    let r = f.pending();
    let mut e = Fake::new(&f.store);
    e.credential_error = true;
    assert_eq!(
        run(finish_verified(&e, &r, ())),
        Err(ErrorCode::ScopeDenied)
    );
    assert_eq!(e.probes.get(), 0);
    let r = f
        .store
        .load_imap_setup_request(*r.intent().request_id())
        .unwrap()
        .unwrap();
    assert_eq!(
        r.events().last().unwrap().event(),
        &ImapSetupEvent::CredentialStoreReturned(Err(ErrorCode::ScopeDenied))
    );
    assert_eq!(
        progress(&r, false, false).unwrap().state(),
        ImapSetupState::Error
    );
}
#[test]
fn setup01_stale_snapshot_and_second_attempt_cannot_repeat_effects() {
    let f = Fixture::new();
    let r = f.pending();
    let e = Fake::new(&f.store);
    run(finish_verified(&e, &r, ())).unwrap();
    e.events.borrow_mut().clear();
    assert_eq!(run(finish_verified(&e, &r, ())), Err(ErrorCode::Conflict));
    assert_eq!(*e.events.borrow(), ["load"]);
    assert_eq!(e.probes.get(), 1);
}
#[test]
fn setup01_pending_and_interrupted_progress_never_claim_running_without_job() {
    let f = Fixture::new();
    let r = f.pending();
    assert_eq!(
        progress(&r, true, false).unwrap().state(),
        ImapSetupState::Pending
    );
    assert_eq!(
        progress(&r, false, false).unwrap().state(),
        ImapSetupState::Interrupted
    );
    let r = f.store.stage_imap_setup_request(&r).unwrap();
    assert_eq!(
        progress(&r, true, false).unwrap().state(),
        ImapSetupState::Connecting
    );
    assert_eq!(
        progress(&r, false, false).unwrap().state(),
        ImapSetupState::Interrupted
    );
    let r = f.store.begin_imap_credential_store(&r).unwrap();
    assert_eq!(
        progress(&r, false, false).unwrap().state(),
        ImapSetupState::Interrupted
    );
}
#[test]
fn setup01_all_probe_outcomes_have_explicit_next_actions() {
    for (outcome, state) in [
        (
            InitialImapProbeOutcome::AuthRequired,
            ImapSetupState::AuthRequired,
        ),
        (
            InitialImapProbeOutcome::TlsFailed,
            ImapSetupState::TlsFailed,
        ),
        (InitialImapProbeOutcome::Offline, ImapSetupState::Offline),
        (InitialImapProbeOutcome::Error, ImapSetupState::Error),
    ] {
        let f = Fixture::new();
        let r = f.pending();
        let mut e = Fake::new(&f.store);
        e.outcome = outcome;
        run(finish_verified(&e, &r, ())).unwrap();
        let r = f
            .store
            .load_imap_setup_request(*r.intent().request_id())
            .unwrap()
            .unwrap();
        let p = progress(&r, false, false).unwrap();
        assert_eq!(p.state(), state);
        assert_eq!(p.next_action(), ImapSetupNextAction::ResumeRequired);
    }
}
#[test]
fn setup06_rejected_or_cancelled_review_creates_no_account_or_effect() {
    for (error, state) in [
        (ErrorCode::ApprovalDenied, ImapSetupState::Denied),
        (ErrorCode::ApprovalRequired, ImapSetupState::Cancelled),
        (ErrorCode::HumanStepRequired, ImapSetupState::Error),
    ] {
        let f = Fixture::new();
        let r = f.pending();
        let closed = f.store.close_imap_setup_review(&r, error).unwrap();
        assert_eq!(progress(&closed, false, false).unwrap().state(), state);
        assert!(f.store.list_registrable_account_ids().unwrap().is_empty());
        let e = Fake::new(&f.store);
        assert_eq!(
            run(finish_verified(&e, &closed, ())),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(*e.events.borrow(), ["load"]);
    }
}

#[test]
fn setup01_shutdown_during_probe_preserves_started_state_without_repeating() {
    use std::future::Future;
    let f = Fixture::new();
    let request = f.pending();
    let mut effects = Fake::new(&f.store);
    effects.block_probe = true;
    let mut future = Box::pin(finish_verified(&effects, &request, ()));
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(future.as_mut().poll(&mut context).is_pending());
    drop(future);
    let interrupted = f
        .store
        .load_imap_setup_request(*request.intent().request_id())
        .unwrap()
        .unwrap();
    assert_eq!(interrupted.phase(), ImapSetupPhase::ProbeStarted);
    assert_eq!(
        progress(&interrupted, false, false).unwrap().state(),
        ImapSetupState::Interrupted
    );
    assert_eq!(effects.probes.get(), 1);
    assert!(!effects.events.borrow().contains(&"finish_probe"));
    assert_eq!(
        run(finish_verified(&effects, &request, ())),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(effects.probes.get(), 1);
}

#[test]
fn setup01_probe_errors_and_secret_errors_have_stable_categories() {
    use hantong_secrets::{MailProbeError as P, SecretError as S};
    for (error, outcome) in [
        (P::Authentication, InitialImapProbeOutcome::AuthRequired),
        (P::Tls, InitialImapProbeOutcome::TlsFailed),
        (P::Connection, InitialImapProbeOutcome::Offline),
        (P::Timeout, InitialImapProbeOutcome::Offline),
        (P::Protocol, InitialImapProbeOutcome::Error),
        (P::MechanismUnsupported, InitialImapProbeOutcome::Error),
        (P::Credential(S::NotFound), InitialImapProbeOutcome::Error),
    ] {
        assert_eq!(probe_outcome(Err(error), 1), outcome);
    }
    for (error, code) in [
        (S::InputExpired, ErrorCode::ApprovalExpired),
        (S::CredentialMismatch, ErrorCode::ScopeDenied),
        (S::InvalidInputWindow, ErrorCode::ScopeDenied),
        (S::UnsupportedPlatform, ErrorCode::CapabilityUnsupported),
        (S::InvalidCredential, ErrorCode::InvalidInput),
        (S::Io, ErrorCode::InternalError),
        (S::Protection, ErrorCode::InternalError),
    ] {
        assert_eq!(secret_error(error), code);
    }
}

#[test]
fn setup01_resume_effects_use_fresh_binding_and_existing_account_stage() {
    let f = Fixture::new();
    let original = f.pending();
    let mut effects = Fake::new(&f.store);
    effects.outcome = InitialImapProbeOutcome::AuthRequired;
    run(finish_verified(&effects, &original, ())).unwrap();
    let leaf = f
        .store
        .resolve_imap_setup_leaf(*original.intent().request_id())
        .unwrap();
    let child = prepare_resume(&f.store, &leaf).unwrap();
    assert_eq!(
        child.intent().resume_binding().unwrap().existing_versions(),
        Some((2, 1))
    );
    assert_eq!(
        child.intent().draft().account_id(),
        original.intent().draft().account_id()
    );
    let review = review_snapshot(&child, [9; 32]).unwrap();
    assert!(review.is_resume());
    assert_eq!(review.payload_hash(), child.intent().review_payload_hash());
    assert_ne!(
        review.payload_hash(),
        child.intent().mutation().payload_hash()
    );
    let effects = Fake::new(&f.store);
    run(finish_verified(&effects, &child, ())).unwrap();
    let state = f
        .store
        .load_imap_provisioning_state(original.intent().draft().account_id())
        .unwrap();
    assert_eq!(state.account_version, 4);
    assert_eq!(state.observation.unwrap().observation_version, 2);
    let parent = f
        .store
        .load_imap_setup_request(*original.intent().request_id())
        .unwrap()
        .unwrap();
    assert_eq!(
        progress(&parent, true, false).unwrap().state(),
        ImapSetupState::Interrupted
    );
    assert_eq!(
        run(finish_verified(&effects, &original, ())),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(effects.probes.get(), 1);
}

#[test]
fn setup01_pre_stage_resume_retains_new_account_action_and_fresh_hash() {
    let f = Fixture::new();
    let parent = f.pending();
    let child = prepare_resume(&f.store, &parent).unwrap();
    assert!(!review_snapshot(&child, [9; 32]).unwrap().is_resume());
    assert_ne!(child.intent().request_id(), parent.intent().request_id());
    assert_ne!(
        child.intent().review_payload_hash(),
        parent.intent().review_payload_hash()
    );
    assert!(f.store.list_registrable_account_ids().unwrap().is_empty());
    let effects = Fake::new(&f.store);
    run(finish_verified(&effects, &child, ())).unwrap();
    let state = f
        .store
        .load_imap_provisioning_state(parent.intent().draft().account_id())
        .unwrap();
    assert_eq!(state.account_version, 2);
}
