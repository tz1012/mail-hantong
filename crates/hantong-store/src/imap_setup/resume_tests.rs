fn resume(s: &Store, parent: &ImapSetupRequest, n: u32) -> ImapSetupRequest {
    s.prepare_imap_setup_resume(
        parent,
        *intent(n).request_id(),
        format!("resume-op-{n}"),
        format!("resume-key-{n}"),
    )
    .unwrap()
}
#[test]
fn setup01_resume_before_stage_preserves_parent_and_requires_new_stage() {
    let f = Fixture::new();
    let parent = pending(&f.store, 1);
    let child = resume(&f.store, &parent, 2);
    let stored = f
        .store
        .load_imap_setup_request(parent.intent.request_id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.phase(), ImapSetupPhase::Superseded);
    assert_eq!(&stored.events()[..parent.events().len()], parent.events());
    assert_eq!(
        stored.events().last().unwrap().event(),
        &ImapSetupEvent::Superseded(child.intent.request_id)
    );
    assert_eq!(stored.updated_at_utc_ms(), child.created_at_utc_ms());
    let binding = child.intent.resume_binding().unwrap();
    assert_eq!(binding.parent_request_id(), parent.intent.request_id());
    assert_eq!(binding.parent_version(), parent.version());
    assert_eq!(binding.existing_versions(), None);
    assert_ne!(
        child.intent.review_payload_hash(),
        child.intent.mutation.payload_hash()
    );
    assert_eq!(child.intent.draft(), parent.intent.draft());
    assert_eq!(child.intent.endpoint(), parent.intent.endpoint());
    assert_eq!(
        f.store
            .resolve_imap_setup_leaf(parent.intent.request_id)
            .unwrap(),
        child
    );
    assert_eq!(
        f.store.stage_imap_setup_request(&parent),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store.stage_imap_setup_resume(&child),
        Err(ErrorCode::Conflict)
    );
    f.store.stage_imap_setup_request(&child).unwrap();
}
#[test]
fn setup01_resume_existing_stage_increments_versions_without_creating_operations() {
    let f = Fixture::new();
    let parent = started_probe(&f.store, 1);
    let child = resume(&f.store, &parent, 2);
    assert_eq!(
        child.intent.resume_binding().unwrap().existing_versions(),
        Some((1, 0))
    );
    assert_eq!(
        f.store.stage_imap_setup_request(&child),
        Err(ErrorCode::Conflict)
    );
    let a = f.store.stage_imap_setup_resume(&child).unwrap();
    assert_eq!(a.phase(), ImapSetupPhase::ResumeStaged);
    assert_eq!(
        f.store
            .finish_imap_setup_probe(&parent, &InitialImapProbeOutcome::Offline),
        Err(ErrorCode::Conflict)
    );
    let b = f.store.begin_imap_credential_store(&a).unwrap();
    let c = f.store.finish_imap_credential_store(&b, Ok(())).unwrap();
    let d = f.store.begin_initial_imap_probe(&c).unwrap();
    let e = f
        .store
        .finish_imap_setup_probe(&d, &InitialImapProbeOutcome::Offline)
        .unwrap();
    let state = f
        .store
        .load_imap_provisioning_state(e.intent.draft.account_id())
        .unwrap();
    assert_eq!(state.account_version, 3);
    assert_eq!(state.observation.unwrap().observation_version, 1);
    assert_eq!(snapshot(&f.store)[3].len(), 1);
    f.store.with_connection(validate_journal).unwrap();
}
#[test]
fn setup01_resume_each_interruption_and_failure_uses_new_request() {
    for step in 0..8 {
        let f = Fixture::new();
        let mut p = pending(&f.store, 1);
        if step == 1 {
            p = f
                .store
                .close_imap_setup_review(&p, ErrorCode::ApprovalDenied)
                .unwrap();
        }
        if step >= 2 {
            p = f.store.stage_imap_setup_request(&p).unwrap();
        }
        if step >= 3 {
            p = f.store.begin_imap_credential_store(&p).unwrap();
        }
        if step >= 4 {
            p = f
                .store
                .finish_imap_credential_store(
                    &p,
                    if step == 4 {
                        Err(ErrorCode::InternalError)
                    } else {
                        Ok(())
                    },
                )
                .unwrap();
        }
        if step >= 6 {
            p = f.store.begin_initial_imap_probe(&p).unwrap();
        }
        if step >= 7 {
            p = f
                .store
                .finish_imap_setup_probe(&p, &InitialImapProbeOutcome::AuthRequired)
                .unwrap();
        }
        let child = resume(&f.store, &p, 2);
        assert_eq!(child.phase(), ImapSetupPhase::ReviewPending);
        assert_eq!(child.version(), 1);
        assert_eq!(
            child.expires_at_utc_ms() - child.created_at_utc_ms(),
            300000
        );
        assert_eq!(
            child.intent.resume_binding().unwrap().existing_versions(),
            if step >= 7 {
                Some((2, 1))
            } else if step >= 2 {
                Some((1, 0))
            } else {
                None
            }
        );
        assert_eq!(
            f.store.prepare_imap_setup_resume(
                &p,
                *intent(3).request_id(),
                "r3".into(),
                "k3".into()
            ),
            Err(ErrorCode::Conflict)
        );
    }
}
#[test]
fn setup01_resume_ready_and_edited_target_or_versions_are_rejected() {
    let f = Fixture::new();
    let p = started_probe(&f.store, 1);
    let ready = f
        .store
        .finish_imap_setup_probe(&p, &InitialImapProbeOutcome::Ready(vec![]))
        .unwrap();
    assert_eq!(
        f.store
            .prepare_imap_setup_resume(&ready, [2; 32], "r2".into(), "k2".into()),
        Err(ErrorCode::Conflict)
    );
    for change in [
        "UPDATE accounts SET settings_version=2",
        "UPDATE accounts SET display_name='edited'",
        "UPDATE accounts SET status='disabled'",
        "UPDATE mail_connection_settings SET host='changed.example'",
        "UPDATE mail_connection_settings SET settings_version=2",
    ] {
        let f = Fixture::new();
        let p = staged(&f.store, 1);
        sql(&f.store, change);
        let before = snapshot(&f.store);
        assert_eq!(
            f.store
                .prepare_imap_setup_resume(&p, [2; 32], "r2".into(), "k2".into()),
            Err(ErrorCode::Conflict),
            "{change}"
        );
        assert_eq!(snapshot(&f.store), before);
    }
}
#[test]
fn setup01_resume_pending_child_can_be_resumed_again_and_chain_resolves() {
    let f = Fixture::new();
    let p = staged(&f.store, 1);
    let c = resume(&f.store, &p, 2);
    let d = resume(&f.store, &c, 3);
    assert_eq!(
        d.intent.resume_binding().unwrap().existing_versions(),
        Some((1, 0))
    );
    assert_eq!(
        f.store
            .resolve_imap_setup_leaf(p.intent.request_id)
            .unwrap(),
        d
    );
    assert_eq!(
        f.store
            .resolve_imap_setup_leaf(c.intent.request_id)
            .unwrap(),
        d
    );
    let d = f.store.stage_imap_setup_resume(&d).unwrap();
    let e = resume(&f.store, &d, 4);
    assert_eq!(
        e.intent.resume_binding().unwrap().existing_versions(),
        Some((2, 0))
    );
    f.store.with_connection(validate_journal).unwrap();
}
#[test]
fn setup01_resume_prepare_and_stage_faults_preserve_all_rows() {
    let f = Fixture::new();
    let p = staged(&f.store, 1);
    sql(
        &f.store,
        "CREATE TRIGGER fail BEFORE INSERT ON imap_setup_events WHEN NEW.kind='superseded' BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
    );
    let before = snapshot(&f.store);
    assert_eq!(
        f.store
            .prepare_imap_setup_resume(&p, [2; 32], "r2".into(), "k2".into()),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(snapshot(&f.store), before);
    for (verb, table) in [
        ("UPDATE", "imap_setup_requests"),
        ("INSERT", "imap_setup_requests"),
        ("INSERT", "imap_setup_events"),
    ] {
        let f = Fixture::new();
        let p = staged(&f.store, 1);
        sql(
            &f.store,
            &format!(
                "CREATE TRIGGER fail BEFORE {verb} ON {table} BEGIN SELECT RAISE(ABORT,'synthetic'); END;"
            ),
        );
        let before = snapshot(&f.store);
        assert_eq!(
            f.store
                .prepare_imap_setup_resume(&p, [2; 32], "r2".into(), "k2".into()),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(snapshot(&f.store), before);
    }
    for (verb, table) in [
        ("UPDATE", "accounts"),
        ("UPDATE", "imap_setup_requests"),
        ("INSERT", "imap_setup_events"),
    ] {
        let f = Fixture::new();
        let p = staged(&f.store, 1);
        let c = resume(&f.store, &p, 2);
        sql(
            &f.store,
            &format!(
                "CREATE TRIGGER fail BEFORE {verb} ON {table} BEGIN SELECT RAISE(ABORT,'synthetic'); END;"
            ),
        );
        let before = snapshot(&f.store);
        assert_eq!(
            f.store.stage_imap_setup_resume(&c),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(snapshot(&f.store), before);
    }
}
#[test]
fn setup01_resume_validates_bidirectional_links_hash_time_and_null_combinations() {
    for change in [
        "UPDATE imap_setup_requests SET resume_payload_hash=zeroblob(32) WHERE resume_of_request_id IS NOT NULL",
        "UPDATE imap_setup_requests SET resume_parent_version=7 WHERE resume_of_request_id IS NOT NULL",
        "UPDATE imap_setup_requests SET resume_account_version=NULL WHERE resume_of_request_id IS NOT NULL",
        "UPDATE imap_setup_requests SET resume_of_request_id=request_id WHERE resume_of_request_id IS NOT NULL",
        "UPDATE imap_setup_events SET child_request_id=zeroblob(32) WHERE kind='superseded'",
        "UPDATE imap_setup_events SET occurred_at_utc_ms=occurred_at_utc_ms+1 WHERE kind='superseded'",
    ] {
        let f = Fixture::new();
        let p = staged(&f.store, 1);
        let c = resume(&f.store, &p, 2);
        sql(
            &f.store,
            "PRAGMA foreign_keys=OFF; PRAGMA ignore_check_constraints=ON;",
        );
        sql(&f.store, change);
        assert_eq!(
            f.store.load_imap_setup_request(p.intent.request_id),
            Err(ErrorCode::InternalError),
            "{change}"
        );
        assert_eq!(
            f.store.load_imap_setup_request(c.intent.request_id),
            Err(ErrorCode::InternalError),
            "{change}"
        );
        assert!(
            f.store.with_connection(validate_journal).is_err(),
            "{change}"
        );
    }
}
#[test]
fn setup01_resume_rejects_identifier_collisions_and_clock_rollback() {
    let f = Fixture::new();
    let p = staged(&f.store, 1);
    let before = snapshot(&f.store);
    for (id, op, key, error) in [
        ([0; 32], "r2", "k2", ErrorCode::InvalidInput),
        (p.intent.request_id, "r2", "k2", ErrorCode::Conflict),
        ([2; 32], "operation-1", "k2", ErrorCode::Conflict),
        ([2; 32], "r2", "key-1", ErrorCode::Conflict),
    ] {
        assert_eq!(
            f.store
                .prepare_imap_setup_resume(&p, id, op.into(), key.into()),
            Err(error)
        );
        assert_eq!(snapshot(&f.store), before);
    }
    shift(&f.store, 400000);
    let p = f
        .store
        .load_imap_setup_request(p.intent.request_id)
        .unwrap()
        .unwrap();
    let before = snapshot(&f.store);
    assert_eq!(
        f.store
            .prepare_imap_setup_resume(&p, [2; 32], "r2".into(), "k2".into()),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(snapshot(&f.store), before);
}

#[test]
fn setup01_resume_result_versions_and_unrelated_account_data_are_preserved() {
    let f = Fixture::new();
    let p = started_probe(&f.store, 1);
    let p = f
        .store
        .finish_imap_setup_probe(&p, &InitialImapProbeOutcome::Offline)
        .unwrap();
    staged(&f.store, 10);
    sql(
        &f.store,
        "UPDATE accounts SET provider_subject='synthetic-subject',primary_address='synthetic@example.test',granted_scopes='[\"synthetic.read\"]' WHERE account_id='account-1'; INSERT INTO mail_connection_settings VALUES('account-1','smtp',2,'smtp.example',465,'implicit_tls','smtp-user','app_password',1);",
    );
    let before = snapshot(&f.store);
    let c = resume(&f.store, &p, 2);
    assert_eq!(
        c.intent.resume_binding().unwrap().existing_versions(),
        Some((2, 1))
    );
    let a = f.store.stage_imap_setup_resume(&c).unwrap();
    let after = snapshot(&f.store);
    assert_eq!(before[1], after[1]);
    assert_eq!(before[2], after[2]);
    assert_eq!(before[3], after[3]);
    let mut original = before[0].clone();
    let mut updated = after[0].clone();
    // Only status and account version may change during resume stage.
    for rows in [&mut original, &mut updated] {
        for row in rows {
            row.remove(10);
            row.remove(7);
        }
    }
    assert_eq!(original, updated);
    let b = f.store.begin_imap_credential_store(&a).unwrap();
    let c = f.store.finish_imap_credential_store(&b, Ok(())).unwrap();
    let d = f.store.begin_initial_imap_probe(&c).unwrap();
    f.store
        .finish_imap_setup_probe(&d, &InitialImapProbeOutcome::AuthRequired)
        .unwrap();
    let state = f.store.load_imap_provisioning_state("account-1").unwrap();
    assert_eq!(state.account_version, 4);
    assert_eq!(state.observation.unwrap().observation_version, 2);
    assert_eq!(snapshot(&f.store)[3], before[3]);
}

#[test]
fn setup01_resume_rechecks_drift_after_review_and_atomic_probe_finish() {
    for change in [
        "UPDATE accounts SET settings_version=2",
        "UPDATE mail_connection_settings SET host='changed.example'",
        "INSERT INTO mail_connection_observations VALUES('account-1','imap',1,1,'offline',1)",
    ] {
        let f = Fixture::new();
        let p = staged(&f.store, 1);
        let c = resume(&f.store, &p, 2);
        sql(&f.store, change);
        let before = snapshot(&f.store);
        assert_eq!(
            f.store.stage_imap_setup_resume(&c),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(snapshot(&f.store), before);
    }
    for (verb, table) in [
        ("UPDATE", "accounts"),
        ("INSERT", "mail_connection_observations"),
        ("UPDATE", "imap_setup_requests"),
        ("INSERT", "imap_setup_events"),
    ] {
        let f = Fixture::new();
        let p = staged(&f.store, 1);
        let c = resume(&f.store, &p, 2);
        let a = f.store.stage_imap_setup_resume(&c).unwrap();
        let b = f.store.begin_imap_credential_store(&a).unwrap();
        let c = f.store.finish_imap_credential_store(&b, Ok(())).unwrap();
        let d = f.store.begin_initial_imap_probe(&c).unwrap();
        sql(
            &f.store,
            &format!(
                "CREATE TRIGGER fail BEFORE {verb} ON {table} BEGIN SELECT RAISE(ABORT,'synthetic'); END;"
            ),
        );
        let before = snapshot(&f.store);
        assert_eq!(
            f.store
                .finish_imap_setup_probe(&d, &InitialImapProbeOutcome::Offline),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(snapshot(&f.store), before);
        assert_eq!(
            f.store.begin_initial_imap_probe(&d),
            Err(ErrorCode::Conflict)
        );
    }
}

#[test]
fn setup01_resume_missing_forward_edge_and_validly_hashed_cycle_are_rejected() {
    let f = Fixture::new();
    let p = staged(&f.store, 1);
    let c = resume(&f.store, &p, 2);
    // Remove only the isolated fixture's uniqueness guard to inject a missing
    // forward edge while leaving both individually valid request histories.
    sql(&f.store, "DROP INDEX imap_setup_active_account_uq");
    f.store.with_connection(|db| {
        db.execute("DELETE FROM imap_setup_events WHERE request_id=?1 AND kind='superseded'",[p.intent.request_id.as_slice()])?;
        db.execute("UPDATE imap_setup_requests SET phase='account_staged',version=2,updated_at_utc_ms=?2 WHERE request_id=?1",params![p.intent.request_id.as_slice(),p.updated_at_utc_ms])?;Ok(())
    }).unwrap();
    assert_eq!(
        f.store.load_imap_setup_request(p.intent.request_id),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        f.store.load_imap_setup_request(c.intent.request_id),
        Err(ErrorCode::InternalError)
    );
    let f = Fixture::new();
    let p = staged(&f.store, 1);
    let c = resume(&f.store, &p, 2);
    let mut bytes = b"mail-hantong.imap-setup-resume.v1\0".to_vec();
    bytes.extend_from_slice(&c.intent.request_id);
    bytes.extend_from_slice(&1_u64.to_be_bytes());
    bytes.extend_from_slice(p.intent.mutation.payload_hash());
    bytes.push(0);
    let hash = hantong_platform::sha256(&bytes).unwrap();
    f.store.with_connection(|db| {
        db.execute("UPDATE imap_setup_requests SET resume_of_request_id=?2,resume_parent_version=1,resume_payload_hash=?3 WHERE request_id=?1",params![p.intent.request_id.as_slice(),c.intent.request_id.as_slice(),hash.as_slice()])?;
        db.execute("UPDATE imap_setup_requests SET phase='superseded',version=2 WHERE request_id=?1",[c.intent.request_id.as_slice()])?;
        db.execute("INSERT INTO imap_setup_events(request_id,seq,kind,occurred_at_utc_ms,child_request_id) VALUES(?1,2,'superseded',?2,?3)",params![c.intent.request_id.as_slice(),c.updated_at_utc_ms,p.intent.request_id.as_slice()])?;
        Ok(())
    }).unwrap();
    assert_eq!(
        f.store.resolve_imap_setup_leaf(c.intent.request_id),
        Err(ErrorCode::InternalError)
    );
    assert!(f.store.with_connection(validate_journal).is_err());
}

#[test]
fn setup01_resume_raw_result_versions_are_bound_to_attempt() {
    let f = Fixture::new();
    let p = staged(&f.store, 1);
    let c = resume(&f.store, &p, 2);
    let a = f.store.stage_imap_setup_resume(&c).unwrap();
    sql(
        &f.store,
        "UPDATE imap_setup_events SET account_version=3 WHERE kind='resume_staged'",
    );
    assert_eq!(
        f.store.load_imap_setup_request(a.intent.request_id),
        Err(ErrorCode::InternalError)
    );
    let f = Fixture::new();
    let p = staged(&f.store, 1);
    let c = resume(&f.store, &p, 2);
    let a = f.store.stage_imap_setup_resume(&c).unwrap();
    let b = f.store.begin_imap_credential_store(&a).unwrap();
    let c = f.store.finish_imap_credential_store(&b, Ok(())).unwrap();
    let d = f.store.begin_initial_imap_probe(&c).unwrap();
    let e = f
        .store
        .finish_imap_setup_probe(&d, &InitialImapProbeOutcome::Offline)
        .unwrap();
    sql(
        &f.store,
        "UPDATE imap_setup_events SET account_version=2 WHERE kind='probe_finished'",
    );
    assert_eq!(
        f.store.load_imap_setup_request(e.intent.request_id),
        Err(ErrorCode::InternalError)
    );
}

#[test]
fn setup01_resume_hash_has_a_stable_domain_and_version_layout() {
    let f = Fixture::new();
    let p = staged(&f.store, 1);
    let c = resume(&f.store, &p, 2);
    assert_eq!(
        c.intent.review_payload_hash(),
        &[
            0x97, 0x48, 0x6f, 0xa1, 0x88, 0x5d, 0x81, 0x3a, 0xe9, 0x88, 0x46, 0x41, 0x03, 0x66,
            0xb5, 0x69, 0x33, 0x34, 0xc3, 0x7d, 0xc7, 0xe7, 0x6e, 0x7b, 0x9a, 0xe5, 0xf4, 0x14,
            0xbb, 0x2f, 0x22, 0xff,
        ]
    );
    assert_eq!(
        p.intent.review_payload_hash(),
        p.intent.mutation.payload_hash()
    );
    assert!(!format!("{:?}", c.intent.resume_binding()).contains("97486f"));
}

#[test]
fn setup01_resume_refreshes_expired_review_but_still_records_late_returns() {
    let f = Fixture::new();
    let p = staged(&f.store, 1);
    shift(&f.store, -400000);
    let p = f
        .store
        .load_imap_setup_request(p.intent.request_id)
        .unwrap()
        .unwrap();
    let c = resume(&f.store, &p, 2);
    assert!(c.created_at_utc_ms() > p.expires_at_utc_ms());
    let a = f.store.stage_imap_setup_resume(&c).unwrap();
    let b = f.store.begin_imap_credential_store(&a).unwrap();
    shift(&f.store, -400000);
    let b = f
        .store
        .load_imap_setup_request(b.intent.request_id)
        .unwrap()
        .unwrap();
    let returned = f.store.finish_imap_credential_store(&b, Ok(())).unwrap();
    assert_eq!(
        f.store.begin_initial_imap_probe(&returned),
        Err(ErrorCode::ApprovalExpired)
    );
    assert_eq!(
        f.store
            .resolve_imap_setup_leaf(p.intent.request_id)
            .unwrap(),
        returned
    );
}
