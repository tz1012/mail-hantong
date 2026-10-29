#[cfg(windows)]
mod native {
    include!("resume_tests.rs");
    use super::super::*;
    use crate::account_provisioning::imap_account_stage_payload_hash;
    use hantong_domain::mail_connection::MailConnectionConfig;
    use rusqlite::params;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Fixture {
        store: Store,
        _root: Cleanup,
    }
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("owned fixture closed");
        }
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "hantong-setup-journal-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self {
                store: Store::open(root.join("data"), root.join("keys")).unwrap(),
                _root: Cleanup(root),
            }
        }
    }
    fn intent(n: u32) -> NewImapSetupRequest {
        let account = format!("account-{n}");
        let draft = NewImapAccountDraft::new(
            account.clone(),
            Some("Private display".into()),
            Some("#112233".into()),
        )
        .unwrap();
        let endpoint = MailConnectionConfig::from_json(&serde_json::to_vec(&serde_json::json!({"schema_version":1,"account_id":account,"imap":{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":format!("user-{n}"),"authentication":"app_password"}})).unwrap()).unwrap().imap().clone();
        let mut id = [0; 32];
        id[..4].copy_from_slice(&n.to_be_bytes());
        let mutation = MailMutation::new(
            format!("operation-{n}"),
            format!("key-{n}"),
            0,
            imap_account_stage_payload_hash(&draft, &endpoint).unwrap(),
        )
        .unwrap();
        NewImapSetupRequest::new(id, draft, endpoint, mutation).unwrap()
    }
    fn sql(s: &Store, query: &str) {
        s.with_connection(|c| c.execute_batch(query)).unwrap();
    }
    fn snapshot(s: &Store) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
        s.with_connection(|c| {
            [
                "accounts",
                "mail_connection_settings",
                "mail_connection_observations",
                "mail_connection_operations",
                "imap_setup_requests",
                "imap_setup_events",
            ]
            .iter()
            .map(|table| {
                let mut stmt = c.prepare(&format!("SELECT * FROM {table} ORDER BY 1,2"))?;
                let count = stmt.column_count();
                stmt.query_map([], |r| (0..count).map(|i| r.get(i)).collect())?
                    .collect()
            })
            .collect()
        })
        .unwrap()
    }
    fn pending(s: &Store, n: u32) -> ImapSetupRequest {
        s.create_imap_setup_request(&intent(n)).unwrap()
    }
    fn staged(s: &Store, n: u32) -> ImapSetupRequest {
        s.stage_imap_setup_request(&pending(s, n)).unwrap()
    }
    fn started_probe(s: &Store, n: u32) -> ImapSetupRequest {
        let a = staged(s, n);
        let b = s.begin_imap_credential_store(&a).unwrap();
        let c = s.finish_imap_credential_store(&b, Ok(())).unwrap();
        s.begin_initial_imap_probe(&c).unwrap()
    }
    fn shift(s: &Store, delta: i64) {
        s.with_connection(|c| {
            c.execute("UPDATE imap_setup_requests SET created_at_utc_ms=created_at_utc_ms+?1,expires_at_utc_ms=expires_at_utc_ms+?1,updated_at_utc_ms=updated_at_utc_ms+?1",[delta])?;
            c.execute("UPDATE imap_setup_events SET occurred_at_utc_ms=occurred_at_utc_ms+?1",[delta])?;
            c.execute("UPDATE mail_connection_operations SET completed_at_utc_ms=completed_at_utc_ms+?1",[delta])?;
            Ok(())
        }).unwrap();
    }

    #[test]
    fn setup01_intent_rejects_zero_ids_hash_version_and_oauth() {
        let i = intent(1);
        assert_eq!(
            NewImapSetupRequest::new(
                [0; 32],
                i.draft.clone(),
                i.endpoint.clone(),
                i.mutation.clone()
            ),
            Err(ErrorCode::InvalidInput)
        );
        for (version, hash) in [(1, *i.mutation.payload_hash()), (0, [0; 32]), (0, [8; 32])] {
            let m = MailMutation::new("op".into(), "key".into(), version, hash).unwrap();
            assert_eq!(
                NewImapSetupRequest::new([1; 32], i.draft.clone(), i.endpoint.clone(), m),
                Err(ErrorCode::InvalidInput)
            );
        }
        for (version, auth) in [(2, "app_password"), (1, "oauth2")] {
            let mut wire = serde_json::to_value(&i.endpoint).unwrap();
            wire["settings_version"] = version.into();
            wire["authentication"] = auth.into();
            let e = MailConnectionConfig::from_json(
                &serde_json::to_vec(
                    &serde_json::json!({"schema_version":1,"account_id":"a","imap":wire}),
                )
                .unwrap(),
            )
            .unwrap()
            .imap()
            .clone();
            assert_eq!(
                NewImapSetupRequest::new([1; 32], i.draft.clone(), e, i.mutation.clone()),
                Err(ErrorCode::InvalidInput)
            );
        }
    }
    #[test]
    fn setup01_create_replay_load_and_redacted_snapshot() {
        let f = Fixture::new();
        let p = pending(&f.store, 1);
        assert_eq!(p.phase(), ImapSetupPhase::ReviewPending);
        assert_eq!(p.version(), 1);
        assert_eq!(p.expires_at_utc_ms() - p.created_at_utc_ms(), 300000);
        assert_eq!(
            f.store
                .load_imap_setup_request(*p.intent.request_id())
                .unwrap(),
            Some(p.clone())
        );
        assert_eq!(f.store.list_imap_setup_requests().unwrap(), vec![p.clone()]);
        assert_eq!(f.store.load_imap_setup_request([9; 32]).unwrap(), None);
        let next = f.store.stage_imap_setup_request(&p).unwrap();
        assert_eq!(f.store.create_imap_setup_request(p.intent()).unwrap(), next);
        assert!(!format!("{p:?}{:?}{:?}", p.intent(), p.events()).contains("Private"));
        assert!(!format!("{p:?}").contains("user-1"));
    }
    #[test]
    fn setup01_create_collisions_and_out_of_band_stage_are_conflicts() {
        let f = Fixture::new();
        let p = pending(&f.store, 1);
        let before = snapshot(&f.store);
        let mut other = intent(2);
        other.request_id = p.intent.request_id;
        assert_eq!(
            f.store.create_imap_setup_request(&other),
            Err(ErrorCode::Conflict)
        );
        other = intent(2);
        other.mutation = MailMutation::new(
            "operation-1".into(),
            "k2".into(),
            0,
            *other.mutation.payload_hash(),
        )
        .unwrap();
        assert_eq!(
            f.store.create_imap_setup_request(&other),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(snapshot(&f.store), before);
        f.store
            .stage_imap_account(p.intent.draft(), p.intent.endpoint(), p.intent.mutation())
            .unwrap();
        let before = snapshot(&f.store);
        assert_eq!(
            f.store.stage_imap_setup_request(&p),
            Err(ErrorCode::Conflict)
        );
        let mut new = intent(3);
        new.mutation = MailMutation::new(
            "operation-1".into(),
            "k3".into(),
            0,
            *new.mutation.payload_hash(),
        )
        .unwrap();
        assert_eq!(
            f.store.create_imap_setup_request(&new),
            Err(ErrorCode::Conflict)
        );
        let mut same = p.intent.clone();
        same.request_id = [4; 32];
        same.mutation =
            MailMutation::new("o4".into(), "k4".into(), 0, *same.mutation.payload_hash()).unwrap();
        assert_eq!(
            f.store.create_imap_setup_request(&same),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(snapshot(&f.store), before);
    }
    #[test]
    fn setup01_other_account_operation_race_cannot_be_adopted() {
        let f = Fixture::new();
        let p = pending(&f.store, 1);
        let other = intent(2);
        let mutation = MailMutation::new(
            p.intent.mutation.operation_id().into(),
            "other-key".into(),
            0,
            *other.mutation.payload_hash(),
        )
        .unwrap();
        f.store
            .stage_imap_account(&other.draft, &other.endpoint, &mutation)
            .unwrap();
        let before = snapshot(&f.store);
        assert_eq!(
            f.store.stage_imap_setup_request(&p),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(snapshot(&f.store), before);
    }
    #[test]
    fn setup01_exact_snapshot_and_three_current_state_checks_precede_effects() {
        for modify in 0..6 {
            let f = Fixture::new();
            let p = staged(&f.store, 1);
            let mut changed = p.clone();
            match modify {
                0 => changed.intent.request_id = [9; 32],
                1 => changed.intent = intent(2),
                2 => changed.created_at_utc_ms += 1,
                3 => changed.expires_at_utc_ms += 1,
                4 => changed.version += 1,
                _ => changed.phase = ImapSetupPhase::ReviewPending,
            }
            let before = snapshot(&f.store);
            let expected = if modify < 2 {
                ErrorCode::NotFound
            } else {
                ErrorCode::Conflict
            };
            assert_eq!(f.store.begin_imap_credential_store(&changed), Err(expected));
            assert_eq!(snapshot(&f.store), before);
        }
        let f = Fixture::new();
        let p = staged(&f.store, 1);
        sql(
            &f.store,
            "INSERT INTO mail_connection_observations VALUES('account-1','imap',1,1,'offline',1)",
        );
        let before = snapshot(&f.store);
        assert_eq!(
            f.store.begin_imap_credential_store(&p),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(snapshot(&f.store), before);
    }
    #[test]
    fn setup01_complete_history_and_all_probe_outcomes() {
        for (n, outcome) in [
            InitialImapProbeOutcome::Ready(vec!["imap.idle".into(), "imap.uidplus".into()]),
            InitialImapProbeOutcome::AuthRequired,
            InitialImapProbeOutcome::TlsFailed,
            InitialImapProbeOutcome::Offline,
            InitialImapProbeOutcome::Error,
        ]
        .into_iter()
        .enumerate()
        {
            let f = Fixture::new();
            let p = started_probe(&f.store, n as u32 + 1);
            let done = f.store.finish_imap_setup_probe(&p, &outcome).unwrap();
            assert_eq!(done.phase(), ImapSetupPhase::ProbeFinished);
            assert_eq!(done.version(), 6);
            assert_eq!(done.events().len(), 6);
            assert_eq!(
                done.events()[5].event(),
                &ImapSetupEvent::ProbeFinished(outcome)
            );
            let state = f
                .store
                .load_imap_provisioning_state(done.intent.draft.account_id())
                .unwrap();
            assert_eq!(state.account_version, 2);
            assert_eq!(state.endpoint.settings_version(), 1);
            assert_eq!(state.observation.unwrap().observation_version, 1);
            f.store.with_connection(validate_journal).unwrap();
            assert_eq!(
                f.store
                    .finish_imap_setup_probe(&p, &InitialImapProbeOutcome::Error),
                Err(ErrorCode::Conflict)
            );
        }
    }
    #[test]
    fn setup01_started_events_cannot_be_replayed_or_skipped() {
        let f = Fixture::new();
        let p = pending(&f.store, 1);
        assert_eq!(
            f.store.begin_imap_credential_store(&p),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            f.store.finish_imap_credential_store(&p, Ok(())),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            f.store.begin_initial_imap_probe(&p),
            Err(ErrorCode::Conflict)
        );
        let a = f.store.stage_imap_setup_request(&p).unwrap();
        let before = snapshot(&f.store);
        assert_eq!(
            f.store.stage_imap_setup_request(&p),
            Err(ErrorCode::Conflict)
        );
        let mut tampered = a.clone();
        tampered.events[0].occurred_at_utc_ms += 1;
        assert_eq!(
            f.store.begin_imap_credential_store(&tampered),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(snapshot(&f.store), before);
        let b = f.store.begin_imap_credential_store(&a).unwrap();
        assert_eq!(
            f.store.begin_imap_credential_store(&a),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            f.store.begin_imap_credential_store(&b),
            Err(ErrorCode::Conflict)
        );
        let c = f.store.finish_imap_credential_store(&b, Ok(())).unwrap();
        let d = f.store.begin_initial_imap_probe(&c).unwrap();
        assert_eq!(
            f.store.begin_initial_imap_probe(&c),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            f.store.begin_initial_imap_probe(&d),
            Err(ErrorCode::Conflict)
        );
    }
    #[test]
    fn setup01_failure_whitelists_and_terminal_review_or_credential_failure() {
        let f = Fixture::new();
        let p = pending(&f.store, 1);
        assert_eq!(
            f.store.close_imap_setup_review(&p, ErrorCode::Offline),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(
            f.store
                .close_imap_setup_review(&p, ErrorCode::ApprovalExpired),
            Err(ErrorCode::Conflict)
        );
        let closed = f
            .store
            .close_imap_setup_review(&p, ErrorCode::ApprovalRequired)
            .unwrap();
        assert_eq!(
            closed.events()[1].event(),
            &ImapSetupEvent::ReviewClosed(ErrorCode::ApprovalRequired)
        );
        assert_eq!(
            f.store.stage_imap_setup_request(&closed),
            Err(ErrorCode::Conflict)
        );
        let a = staged(&f.store, 2);
        let b = f.store.begin_imap_credential_store(&a).unwrap();
        assert_eq!(
            f.store
                .finish_imap_credential_store(&b, Err(ErrorCode::SendUnknown)),
            Err(ErrorCode::InvalidInput)
        );
        let c = f
            .store
            .finish_imap_credential_store(&b, Err(ErrorCode::InternalError))
            .unwrap();
        assert_eq!(
            f.store.begin_initial_imap_probe(&c),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            f.store.finish_imap_credential_store(&b, Ok(())),
            Err(ErrorCode::Conflict)
        );
    }
    #[test]
    fn setup01_expiry_blocks_new_effects_but_records_started_results() {
        let f = Fixture::new();
        let p = pending(&f.store, 1);
        shift(&f.store, -400000);
        let p = f
            .store
            .load_imap_setup_request(p.intent.request_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            f.store.stage_imap_setup_request(&p),
            Err(ErrorCode::ApprovalExpired)
        );
        f.store
            .close_imap_setup_review(&p, ErrorCode::ApprovalExpired)
            .unwrap();
        let a = staged(&f.store, 2);
        shift(&f.store, -400000);
        let a = f
            .store
            .load_imap_setup_request(a.intent.request_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            f.store.begin_imap_credential_store(&a),
            Err(ErrorCode::ApprovalExpired)
        );
        let a = staged(&f.store, 3);
        let b = f.store.begin_imap_credential_store(&a).unwrap();
        shift(&f.store, -400000);
        let b = f
            .store
            .load_imap_setup_request(b.intent.request_id)
            .unwrap()
            .unwrap();
        let c = f.store.finish_imap_credential_store(&b, Ok(())).unwrap();
        assert_eq!(
            f.store.begin_initial_imap_probe(&c),
            Err(ErrorCode::ApprovalExpired)
        );
        let d = started_probe(&f.store, 4);
        shift(&f.store, -400000);
        let d = f
            .store
            .load_imap_setup_request(d.intent.request_id)
            .unwrap()
            .unwrap();
        f.store
            .finish_imap_setup_probe(&d, &InitialImapProbeOutcome::Offline)
            .unwrap();
    }
    #[test]
    fn setup01_clock_rollback_is_preserved_without_mutation() {
        let f = Fixture::new();
        let p = pending(&f.store, 1);
        shift(&f.store, 400000);
        let p = f
            .store
            .load_imap_setup_request(p.intent.request_id)
            .unwrap()
            .unwrap();
        let before = snapshot(&f.store);
        assert_eq!(
            f.store.stage_imap_setup_request(&p),
            Err(ErrorCode::ApprovalExpired)
        );
        assert_eq!(snapshot(&f.store), before);
        let f = Fixture::new();
        let a = staged(&f.store, 1);
        let b = f.store.begin_imap_credential_store(&a).unwrap();
        shift(&f.store, 400000);
        let b = f
            .store
            .load_imap_setup_request(b.intent.request_id)
            .unwrap()
            .unwrap();
        let before = snapshot(&f.store);
        assert_eq!(
            f.store.finish_imap_credential_store(&b, Ok(())),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(snapshot(&f.store), before);
    }
    #[test]
    fn setup01_current_edits_block_effect_but_do_not_corrupt_history() {
        for change in [
            "UPDATE accounts SET settings_version=2",
            "UPDATE mail_connection_settings SET settings_version=2,host='other.example'",
            "UPDATE accounts SET status='offline'",
        ] {
            let f = Fixture::new();
            let p = staged(&f.store, 1);
            sql(&f.store, change);
            let before = snapshot(&f.store);
            assert_eq!(
                f.store
                    .load_imap_setup_request(p.intent.request_id)
                    .unwrap(),
                Some(p.clone())
            );
            f.store.with_connection(validate_journal).unwrap();
            assert_eq!(
                f.store.begin_imap_credential_store(&p),
                Err(ErrorCode::Conflict)
            );
            assert_eq!(snapshot(&f.store), before);
        }
        let f = Fixture::new();
        let p = started_probe(&f.store, 1);
        sql(
            &f.store,
            "UPDATE mail_connection_settings SET host='other.example'",
        );
        let before = snapshot(&f.store);
        assert_eq!(
            f.store
                .finish_imap_setup_probe(&p, &InitialImapProbeOutcome::Error),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(snapshot(&f.store), before);
    }
    #[test]
    fn setup01_stage_fault_rolls_back_request_account_endpoint_operation_and_event() {
        for table in [
            "accounts",
            "mail_connection_settings",
            "mail_connection_operations",
            "imap_setup_events",
        ] {
            let f = Fixture::new();
            let p = pending(&f.store, 1);
            sql(
                &f.store,
                &format!(
                    "CREATE TRIGGER fail BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'synthetic'); END;"
                ),
            );
            let before = snapshot(&f.store);
            assert_eq!(
                f.store.stage_imap_setup_request(&p),
                Err(ErrorCode::InternalError)
            );
            assert_eq!(snapshot(&f.store), before);
        }
        let f = Fixture::new();
        let p = pending(&f.store, 1);
        sql(
            &f.store,
            "CREATE TRIGGER fail BEFORE UPDATE ON imap_setup_requests BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
        );
        let before = snapshot(&f.store);
        assert_eq!(
            f.store.stage_imap_setup_request(&p),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(snapshot(&f.store), before);
    }
    #[test]
    fn setup01_result_fault_keeps_durable_started_boundary_and_atomic_probe_state() {
        let f = Fixture::new();
        let a = staged(&f.store, 1);
        let b = f.store.begin_imap_credential_store(&a).unwrap();
        sql(
            &f.store,
            "CREATE TRIGGER fail BEFORE INSERT ON imap_setup_events BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
        );
        let before = snapshot(&f.store);
        assert_eq!(
            f.store.finish_imap_credential_store(&b, Ok(())),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(snapshot(&f.store), before);
        assert_eq!(
            f.store.begin_initial_imap_probe(&b),
            Err(ErrorCode::Conflict)
        );
        for (verb, table) in [
            ("UPDATE", "accounts"),
            ("INSERT", "mail_connection_observations"),
            ("UPDATE", "imap_setup_requests"),
            ("INSERT", "imap_setup_events"),
        ] {
            let f = Fixture::new();
            let p = started_probe(&f.store, 1);
            sql(
                &f.store,
                &format!(
                    "CREATE TRIGGER fail BEFORE {verb} ON {table} BEGIN SELECT RAISE(ABORT,'synthetic'); END;"
                ),
            );
            let before = snapshot(&f.store);
            assert_eq!(
                f.store
                    .finish_imap_setup_probe(&p, &InitialImapProbeOutcome::Ready(vec![])),
                Err(ErrorCode::InternalError)
            );
            assert_eq!(snapshot(&f.store), before);
            assert_eq!(
                f.store.begin_initial_imap_probe(&p),
                Err(ErrorCode::Conflict)
            );
        }
    }
    #[test]
    fn setup01_corrupt_history_is_not_partial_success() {
        for change in [
            "UPDATE imap_setup_requests SET payload_hash=zeroblob(32)",
            "UPDATE imap_setup_requests SET host='IMAP.example'",
            "UPDATE imap_setup_requests SET origin='other'",
            "UPDATE imap_setup_requests SET version=2",
            "UPDATE imap_setup_requests SET phase='probe_started'",
            "UPDATE imap_setup_requests SET updated_at_utc_ms=updated_at_utc_ms+1",
            "UPDATE imap_setup_events SET seq=2",
            "UPDATE imap_setup_events SET kind='probe_started'",
            "UPDATE imap_setup_events SET error_code='INVALID_INPUT'",
            "UPDATE imap_setup_events SET occurred_at_utc_ms=occurred_at_utc_ms-1",
            "DELETE FROM imap_setup_events",
        ] {
            let f = Fixture::new();
            let p = pending(&f.store, 1);
            sql(&f.store, "PRAGMA ignore_check_constraints=ON");
            sql(&f.store, change);
            assert_eq!(
                f.store.load_imap_setup_request(p.intent.request_id),
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
    fn setup01_missing_or_corrupt_stage_link_fails_closed() {
        for change in [
            "DELETE FROM mail_connection_operations",
            "DELETE FROM mail_connection_settings",
            "UPDATE mail_connection_operations SET payload_hash=zeroblob(32)",
            "UPDATE mail_connection_operations SET completed_at_utc_ms=completed_at_utc_ms+1",
        ] {
            let f = Fixture::new();
            let p = staged(&f.store, 1);
            sql(&f.store, change);
            assert_eq!(
                f.store.load_imap_setup_request(p.intent.request_id),
                Err(ErrorCode::InternalError)
            );
            assert!(f.store.with_connection(validate_journal).is_err());
        }
    }
    #[test]
    fn setup01_quota_has_no_eviction_and_exact_replay_still_works() {
        let f = Fixture::new();
        let first = pending(&f.store, 1);
        for n in 2..=1024 {
            pending(&f.store, n);
        }
        let before = snapshot(&f.store);
        assert_eq!(
            f.store.create_imap_setup_request(&intent(1025)),
            Err(ErrorCode::QuotaExceeded)
        );
        assert_eq!(
            f.store.create_imap_setup_request(first.intent()).unwrap(),
            first
        );
        assert_eq!(
            f.store.prepare_imap_setup_resume(
                &first,
                [8; 32],
                "quota-resume".into(),
                "quota-key".into()
            ),
            Err(ErrorCode::QuotaExceeded)
        );
        assert_eq!(snapshot(&f.store), before);
        assert_eq!(f.store.list_imap_setup_requests().unwrap().len(), 1024);
        f.store.with_connection(|c| {
            c.execute("INSERT INTO imap_setup_requests(request_id,account_id,operation_id,idempotency_key,display_name,color,endpoint_version,host,port,tls,login_name,authentication,payload_hash,origin,phase,version,created_at_utc_ms,expires_at_utc_ms,updated_at_utc_ms) SELECT ?1,'overflow','overflow','overflow',display_name,color,endpoint_version,host,port,tls,login_name,authentication,payload_hash,origin,phase,version,created_at_utc_ms,expires_at_utc_ms,updated_at_utc_ms FROM imap_setup_requests LIMIT 1",params![[9_u8;32].as_slice()])?;
            Ok(())
        }).unwrap();
        assert_eq!(
            f.store.list_imap_setup_requests(),
            Err(ErrorCode::QuotaExceeded)
        );
        assert!(f.store.with_connection(validate_journal).is_err());
    }
}

#[cfg(not(windows))]
#[test]
fn setup01_journal_does_not_enable_a_non_windows_store() {
    use super::*;
    let store = Store {};
    assert_eq!(
        store.list_imap_setup_requests(),
        Err(ErrorCode::CapabilityUnsupported)
    );
    assert_eq!(
        store.load_imap_setup_request([1; 32]),
        Err(ErrorCode::CapabilityUnsupported)
    );
    assert_eq!(
        store.resolve_imap_setup_leaf([1; 32]),
        Err(ErrorCode::CapabilityUnsupported)
    );
}
