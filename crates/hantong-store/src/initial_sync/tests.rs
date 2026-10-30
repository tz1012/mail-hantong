use super::*;
use crate::uid_reconcile_jobs::tests::{Fixture, append, identity, mailbox, metadata};

fn signed(
    build: impl Fn(InitialSyncMutation) -> InitialSyncRequest,
    op: &str,
) -> InitialSyncRequest {
    let r = build(InitialSyncMutation::new(op, op, &"0".repeat(64)).unwrap());
    let hash =
        hantong_contracts::wire_hex::encode(&hantong_platform::sha256(&r.intent_bytes()).unwrap());
    build(InitialSyncMutation::new(op, op, &hash).unwrap())
}
fn start_request(op: &str, generation: u64, catalog: u64) -> InitialSyncRequest {
    signed(
        |m| {
            InitialSyncRequest::new_start("acct", identity(b"INBOX"), m, catalog, generation, 2)
                .unwrap()
        },
        op,
    )
}
fn resume_request(op: &str, plan: u8, version: u64, catalog: u64) -> InitialSyncRequest {
    signed(
        |m| {
            InitialSyncRequest::new_resume(
                "acct",
                identity(b"INBOX"),
                m,
                [plan; 32],
                version,
                catalog,
            )
            .unwrap()
        },
        op,
    )
}
fn pause_request(op: &str, plan: u8, version: u64) -> InitialSyncRequest {
    signed(
        |m| {
            InitialSyncRequest::new_pause("acct", identity(b"INBOX"), m, [plan; 32], version)
                .unwrap()
        },
        op,
    )
}
fn admit(f: &Fixture, id: u8, r: &InitialSyncRequest, instance: u8) -> InitialSyncReceipt {
    f.begin_for(id, 9);
    let result = f
        .store()
        .admit_initial_sync([id; 32], r, UidQueueAdmission::Reserved, [instance; 32])
        .unwrap();
    let InitialSyncAdmission::Admitted(receipt) = result else {
        panic!("expected admitted: {result:?}")
    };
    receipt
}
fn job(f: &Fixture, id: u8) -> InitialSyncJob {
    let result = f.store().start_initial_sync([id; 32]).unwrap();
    let InitialSyncStart::Ready(job) = result else {
        panic!("expected ready: {result:?}")
    };
    *job
}
fn plan(f: &Fixture, id: u8) -> InitialSyncPlanSnapshot {
    f.store()
        .load_initial_sync_plan(
            [9; 16],
            &InitialSyncPlanRequest::new("acct", identity(b"INBOX"), [id; 32]).unwrap(),
        )
        .unwrap()
}
fn status(f: &Fixture, id: u8) -> InitialSyncReceipt {
    f.store()
        .load_initial_sync_receipt(
            [9; 16],
            &UidPageStatusRequest::new("acct", [id; 32]).unwrap(),
        )
        .unwrap()
}
fn create(f: &Fixture, id: u8, validity: u32, next: u32) -> InitialSyncReceipt {
    admit(f, id, &start_request(&format!("start-{id}"), 0, 1), 1);
    let j = job(f, id);
    let InitialSyncFetch::Observe(request) = j.fetch_request() else {
        panic!("observation required")
    };
    f.store()
        .complete_initial_sync_observation(
            [id; 32],
            &ImapUidObservation::new(request.clone(), validity, next).unwrap(),
        )
        .unwrap()
}
fn page(j: &InitialSyncJob, uids: &[u32]) -> ImapUidReconcilePage {
    let InitialSyncFetch::Page(request) = j.fetch_request() else {
        panic!("page required")
    };
    ImapUidReconcilePage::new(
        request.clone(),
        request.minimum_uid_next(),
        uids.iter().map(|uid| metadata(*uid, &["\\Seen"])).collect(),
    )
    .unwrap()
}

#[test]
fn sync01_preview_observation_and_recent_pages_preserve_base_cursor_and_identity() {
    let mut f = Fixture::new();
    let original = create(&f, 21, 7, 200);
    assert_eq!(
        original.plan().unwrap().progress().selected_lower_uid(),
        Some(129)
    );
    assert_eq!(f.count("imap_uid_cursors"), 0);
    assert_eq!(f.count("imap_uid_epochs"), 0);
    admit(&f, 22, &resume_request("resume-22", 21, 1, 1), 1);
    let j = job(&f, 22);
    let p = page(&j, &[193, 199]);
    assert_eq!(p.request().after_uid(), 192);
    let first = f.store().complete_initial_sync_page([22; 32], &p).unwrap();
    assert_eq!(first.plan().unwrap().version(), 3);
    assert_eq!(
        first.plan().unwrap().progress().state(),
        InitialSyncState::PausedBudget
    );
    admit(&f, 23, &resume_request("resume-23", 21, 3, 1), 1);
    let j = job(&f, 23);
    assert_eq!(page(&j, &[]).request().through_uid(), 192);
    let done = f
        .store()
        .complete_initial_sync_page([23; 32], &page(&j, &[]))
        .unwrap();
    assert_eq!(
        done.plan().unwrap().progress().state(),
        InitialSyncState::Complete
    );
    assert_eq!(f.count("imap_uid_messages"), 0);
    assert_eq!(f.count("imap_message_identities"), 0);
    assert_eq!(f.count("imap_initial_preview_messages"), 2);
    assert_eq!(status(&f, 21), original);
    assert_eq!(status(&f, 22), first);
    f.reopen();
    assert_eq!(status(&f, 23), done);
}
#[test]
fn sync01_empty_observation_creates_complete_plan_without_page_or_base_writes() {
    let f = Fixture::new();
    let r = create(&f, 21, 7, 1);
    assert_eq!(
        r.plan().unwrap().progress().state(),
        InitialSyncState::Complete
    );
    assert_eq!(f.count("imap_initial_preview_pages"), 0);
    assert_eq!(f.count("imap_uid_cursors"), 0);
    f.begin_for(22, 9);
    assert_eq!(
        f.store()
            .admit_initial_sync(
                [22; 32],
                &resume_request("resume-empty", 21, 1, 1),
                UidQueueAdmission::Reserved,
                [1; 32]
            )
            .unwrap(),
        InitialSyncAdmission::Rejected(ErrorCode::Conflict)
    );
}
#[test]
fn sync01_preview_pause_invalidates_running_step_and_acknowledges_without_page() {
    let f = Fixture::new();
    create(&f, 21, 7, 200);
    admit(&f, 22, &resume_request("resume", 21, 1, 1), 1);
    let j = job(&f, 22);
    let paused = admit(&f, 23, &pause_request("pause", 21, 2), 1);
    assert_eq!(
        paused.plan().unwrap().progress().state(),
        InitialSyncState::Pausing
    );
    assert_eq!(
        f.store().check_initial_sync([22; 32]),
        Err(ErrorCode::Conflict)
    );
    let old = f
        .store()
        .complete_initial_sync_page([22; 32], &page(&j, &[193]))
        .unwrap();
    assert_eq!(old.state(), InitialSyncJobState::Failed);
    assert_eq!(
        plan(&f, 21).progress().state(),
        InitialSyncState::PausedUser
    );
    assert_eq!(f.count("imap_initial_preview_pages"), 0);
    assert_eq!(status(&f, 23), paused);
}
#[test]
fn sync01_preview_explicit_new_instance_resumes_old_run_only_once() {
    let mut f = Fixture::new();
    create(&f, 21, 7, 200);
    admit(&f, 22, &resume_request("run-old", 21, 1, 1), 1);
    job(&f, 22);
    f.reopen();
    f.begin_for(23, 9);
    assert_eq!(
        f.store()
            .admit_initial_sync(
                [23; 32],
                &resume_request("same-instance", 21, 2, 1),
                UidQueueAdmission::Reserved,
                [1; 32]
            )
            .unwrap(),
        InitialSyncAdmission::Rejected(ErrorCode::Conflict)
    );
    assert_eq!(status(&f, 22).state(), InitialSyncJobState::Running);
    admit(&f, 24, &resume_request("new-instance", 21, 2, 1), 2);
    assert_eq!(status(&f, 22).error(), Some(ErrorCode::CoreUnavailable));
    assert_eq!(plan(&f, 21).version(), 4);
    let j = job(&f, 24);
    f.store()
        .complete_initial_sync_page([24; 32], &page(&j, &[199]))
        .unwrap();
    assert_eq!(plan(&f, 21).version(), 5);
}
#[test]
fn sync01_preview_catalog_change_requires_explicit_resume_rebinding() {
    let f = Fixture::new();
    create(&f, 21, 7, 200);
    admit(&f, 22, &resume_request("before-refresh", 21, 1, 1), 1);
    let j = job(&f, 22);
    f.catalog(1);
    assert_eq!(
        f.store().check_initial_sync([22; 32]),
        Err(ErrorCode::Conflict)
    );
    f.store()
        .complete_initial_sync_page([22; 32], &page(&j, &[193]))
        .unwrap();
    assert!(!plan(&f, 21).binding_current());
    admit(&f, 23, &resume_request("after-refresh", 21, 3, 2), 1);
    let j = job(&f, 23);
    f.store()
        .complete_initial_sync_page([23; 32], &page(&j, &[193]))
        .unwrap();
    assert!(plan(&f, 21).binding_current());
}
#[test]
fn sync01_preview_exact_replay_ignores_full_queue_but_execution_ownership_is_strict() {
    let f = Fixture::new();
    let original = create(&f, 21, 7, 200);
    let request = start_request("start-21", 0, 1);
    f.begin_for(22, 9);
    assert_eq!(
        f.store()
            .admit_initial_sync([22; 32], &request, UidQueueAdmission::Full, [1; 32])
            .unwrap(),
        InitialSyncAdmission::Existing(original)
    );
    assert_eq!(
        f.store()
            .admit_initial_sync([21; 32], &request, UidQueueAdmission::Reserved, [1; 32]),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(f.count("imap_initial_preview_plans"), 1);
}
#[test]
fn sync01_preview_selected_reader_detects_missing_child_hash_and_identity() {
    let f = Fixture::new();
    create(&f, 21, 7, 200);
    admit(&f, 22, &resume_request("read-page", 21, 1, 1), 1);
    let j = job(&f, 22);
    f.store()
        .complete_initial_sync_page([22; 32], &page(&j, &[193]))
        .unwrap();
    f.sql("UPDATE imap_initial_preview_messages SET internal_message_id='msg_0000000000000000000000000000000000000000000000000000000000000000'");
    assert_eq!(
        f.store().load_initial_sync_receipt(
            [9; 16],
            &UidPageStatusRequest::new("acct", [22; 32]).unwrap()
        ),
        Err(ErrorCode::InternalError)
    );
}
#[test]
fn sync01_preview_base_adoption_keeps_namespace_and_cursor_coverage() {
    let f = Fixture::new();
    create(&f, 21, 7, 200);
    append(&f, 0, 0, 200, &[1], "base-after-preview");
    admit(&f, 22, &resume_request("adopt", 21, 1, 1), 1);
    let j = job(&f, 22);
    f.store()
        .complete_initial_sync_page([22; 32], &page(&j, &[193]))
        .unwrap();
    let cursor = f
        .store()
        .load_imap_uid_cursor("acct", &mailbox())
        .unwrap()
        .unwrap();
    assert_eq!(cursor.version(), 1);
    assert_eq!(cursor.covered_through_uid(), 64);
    assert_eq!(f.count("imap_uid_messages"), 1);
    assert_eq!(f.count("imap_initial_preview_messages"), 1);
}
#[test]
fn sync01_preview_pause_wrapper_rejects_network_actions_without_reserving_fake_slot() {
    let f = Fixture::new();
    f.begin_for(21, 9);
    assert_eq!(
        f.store()
            .pause_initial_sync([21; 32], &start_request("bad-pause", 0, 1), [1; 32]),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(f.count("imap_initial_preview_requests"), 0);
    assert!(
        f.store()
            .load_audit([21; 32])
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
}

#[test]
fn sync01_preview_explicit_start_recovers_preplan_crash_without_automatic_resume() {
    let mut f = Fixture::new();
    let request = start_request("preplan-start", 0, 1);
    admit(&f, 21, &request, 1);
    job(&f, 21);
    f.reopen();
    let old = status(&f, 21);
    assert!(old.plan().is_none());
    f.begin_for(22, 9);
    assert_eq!(
        f.store()
            .admit_initial_sync([22; 32], &request, UidQueueAdmission::Reserved, [2; 32])
            .unwrap(),
        InitialSyncAdmission::Existing(old.clone())
    );
    for (id, owner, instance, queue, error) in [
        (23, 9, 1, UidQueueAdmission::Reserved, ErrorCode::Conflict),
        (24, 8, 2, UidQueueAdmission::Reserved, ErrorCode::Conflict),
        (25, 9, 2, UidQueueAdmission::Full, ErrorCode::RateLimited),
    ] {
        if owner == 8 {
            f.add_registration(8);
        }
        f.begin_for(id, owner);
        assert_eq!(
            f.store()
                .admit_initial_sync(
                    [id; 32],
                    &start_request(&format!("blocked-{id}"), 0, 1),
                    queue,
                    [instance; 32]
                )
                .unwrap(),
            InitialSyncAdmission::Rejected(error)
        );
        assert_eq!(status(&f, 21), old);
    }
    let admitted = admit(&f, 26, &start_request("fresh-start", 0, 1), 2);
    assert_eq!(admitted.state(), InitialSyncJobState::Admitted);
    assert_eq!(status(&f, 21).error(), Some(ErrorCode::CoreUnavailable));
    assert_eq!(f.count("imap_initial_preview_plans"), 0);
    let j = job(&f, 26);
    let InitialSyncFetch::Observe(request) = j.fetch_request() else {
        panic!("observation required")
    };
    let done = f
        .store()
        .complete_initial_sync_observation(
            [26; 32],
            &ImapUidObservation::new(request.clone(), 7, 200).unwrap(),
        )
        .unwrap();
    assert_eq!(done.plan().unwrap().plan_id(), &[26; 32]);
}

#[test]
fn sync01_preview_preplan_takeover_rolls_back_old_terminal_if_new_admission_fails() {
    let f = Fixture::new();
    admit(&f, 21, &start_request("old", 0, 1), 1);
    job(&f, 21);
    let original = status(&f, 21);
    f.begin_for(22, 9);
    f.sql("CREATE TRIGGER fail_new_preview BEFORE INSERT ON imap_initial_preview_requests BEGIN SELECT RAISE(IGNORE); END;");
    assert_eq!(
        f.store().admit_initial_sync(
            [22; 32],
            &start_request("new", 0, 1),
            UidQueueAdmission::Reserved,
            [2; 32]
        ),
        Err(ErrorCode::InternalError)
    );
    f.sql("DROP TRIGGER fail_new_preview");
    assert_eq!(status(&f, 21), original);
    assert!(
        f.store()
            .load_audit([22; 32])
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
    assert!(matches!(
        f.store()
            .admit_initial_sync(
                [22; 32],
                &start_request("new", 0, 1),
                UidQueueAdmission::Reserved,
                [2; 32]
            )
            .unwrap(),
        InitialSyncAdmission::Admitted(_)
    ));
}

#[test]
fn sync01_preview_new_instance_pause_cannot_take_over_old_running_step() {
    let f = Fixture::new();
    create(&f, 21, 7, 200);
    admit(&f, 22, &resume_request("old-run", 21, 1, 1), 1);
    job(&f, 22);
    let before = plan(&f, 21);
    f.begin_for(23, 9);
    assert_eq!(
        f.store()
            .pause_initial_sync([23; 32], &pause_request("new-pause", 21, 2), [2; 32])
            .unwrap(),
        InitialSyncAdmission::Rejected(ErrorCode::Conflict)
    );
    assert_eq!(plan(&f, 21), before);
    assert_eq!(status(&f, 22).state(), InitialSyncJobState::Running);
    admit(&f, 24, &resume_request("explicit-resume", 21, 2, 1), 2);
    assert_eq!(status(&f, 22).error(), Some(ErrorCode::CoreUnavailable));
}

#[test]
fn sync01_preview_endpoint_change_before_catalog_refresh_marks_stale_binding() {
    let f = Fixture::new();
    create(&f, 21, 7, 200);
    admit(&f, 22, &resume_request("endpoint-run", 21, 1, 1), 1);
    let j = job(&f, 22);
    f.sql("UPDATE mail_connection_settings SET host='changed.fixture.invalid' WHERE account_id='acct' AND endpoint_kind='imap'");
    let result = f
        .store()
        .complete_initial_sync_page([22; 32], &page(&j, &[193]))
        .unwrap();
    assert_eq!(result.error(), Some(ErrorCode::Conflict));
    assert_eq!(
        result.plan().unwrap().progress().state(),
        InitialSyncState::StaleBinding
    );
    assert_eq!(f.count("imap_initial_preview_pages"), 0);
    assert_eq!(f.count("imap_uid_cursors"), 0);
}
#[test]
fn sync01_preview_page_every_write_rolls_back_on_abort_and_ignore() {
    for mode in ["ABORT", "IGNORE"] {
        for (table, event) in [
            ("imap_initial_preview_pages", "INSERT"),
            ("imap_initial_preview_messages", "INSERT"),
            ("imap_initial_preview_plans", "UPDATE"),
            ("imap_initial_preview_requests", "UPDATE"),
            ("audit_terminals", "INSERT"),
        ] {
            let f = Fixture::new();
            create(&f, 21, 7, 200);
            admit(&f, 22, &resume_request("atomic", 21, 1, 1), 1);
            let j = job(&f, 22);
            let raise = if mode == "ABORT" {
                "RAISE(ABORT,'injected')"
            } else {
                "RAISE(IGNORE)"
            };
            f.sql(&format!(
                "CREATE TRIGGER preview_fault BEFORE {event} ON {table} BEGIN SELECT {raise}; END;"
            ));
            assert_eq!(
                f.store()
                    .complete_initial_sync_page([22; 32], &page(&j, &[193, 199])),
                Err(ErrorCode::InternalError),
                "{table} {mode}"
            );
            f.sql("DROP TRIGGER preview_fault");
            assert_eq!(f.count("imap_initial_preview_pages"), 0);
            assert_eq!(f.count("imap_initial_preview_messages"), 0);
            assert_eq!(plan(&f, 21).version(), 2);
            assert!(
                f.store()
                    .load_audit([22; 32])
                    .unwrap()
                    .unwrap()
                    .completion()
                    .is_none()
            );
            f.store()
                .complete_initial_sync_page([22; 32], &page(&j, &[193, 199]))
                .unwrap();
        }
    }
}
