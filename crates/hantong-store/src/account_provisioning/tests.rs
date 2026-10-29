use super::*;

#[cfg(windows)]
mod native {
    use super::*;
    use crate::mail_connection::{
        MailConnectionState, MailEndpointKind, imap_payload_hash, smtp_payload_hash,
    };
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
            fs::remove_dir_all(&self.0).expect("owned fixture handles closed");
        }
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "hantong-account-provision-{}-{}",
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
    fn draft(id: &str) -> NewImapAccountDraft {
        NewImapAccountDraft::new(id.into(), None, None).unwrap()
    }
    fn endpoint(version: u64, host: &str, login: &str) -> ImapEndpointConfig {
        MailConnectionConfig::from_json(&serde_json::to_vec(&serde_json::json!({"schema_version":1,"account_id":"fixture","imap":{"settings_version":version,"host":host,"port":993,"tls":"implicit_tls","login_name":login,"authentication":"app_password"}})).unwrap()).unwrap().imap().clone()
    }
    fn mutation(
        d: &NewImapAccountDraft,
        e: &ImapEndpointConfig,
        op: &str,
        key: &str,
    ) -> MailMutation {
        MailMutation::new(
            op.into(),
            key.into(),
            0,
            imap_account_stage_payload_hash(d, e).unwrap(),
        )
        .unwrap()
    }
    fn stage(s: &Store, id: &str) -> StagedImapAccount {
        let d = draft(id);
        let e = endpoint(1, "imap.example", id);
        s.stage_imap_account(
            &d,
            &e,
            &mutation(&d, &e, &format!("op-{id}"), &format!("key-{id}")),
        )
        .unwrap()
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
    fn finish(
        s: &Store,
        id: &str,
        outcome: &InitialImapProbeOutcome,
    ) -> Result<InitialImapProbeReceipt, ErrorCode> {
        s.finish_initial_imap_probe(id, 1, 1, 0, outcome)
    }

    #[test]
    fn acc01_stage_hash_has_fixed_domain_and_unambiguous_optional_fields() {
        let e = endpoint(1, "IMAP.Example", "u");
        let h = imap_account_stage_payload_hash(&draft("a"), &e).unwrap();
        assert_eq!(
            h,
            [
                0x44, 0xe5, 0x3c, 0x3f, 0xdf, 0x4b, 0xf1, 0x71, 0x69, 0x14, 0x5f, 0x68, 0x3f, 0x54,
                0x43, 0x3a, 0xa8, 0xf3, 0x80, 0x6e, 0x51, 0x62, 0x94, 0x38, 0xc4, 0x59, 0x59, 0xa6,
                0x39, 0xb6, 0x4a, 0xac
            ]
        );
        assert_eq!(
            h,
            imap_account_stage_payload_hash(&draft("a"), &endpoint(1, "imap.example", "u"))
                .unwrap()
        );
        assert_ne!(h, imap_payload_hash("a", &e, 0).unwrap());
        for d in [
            draft("b"),
            NewImapAccountDraft::new("a".into(), Some("label".into()), None).unwrap(),
            NewImapAccountDraft::new("a".into(), None, Some("#123456".into())).unwrap(),
        ] {
            assert_ne!(h, imap_account_stage_payload_hash(&d, &e).unwrap());
        }
        for changed in [
            endpoint(1, "other.example", "u"),
            endpoint(1, "imap.example", "U"),
        ] {
            assert_ne!(
                h,
                imap_account_stage_payload_hash(&draft("a"), &changed).unwrap()
            );
        }
        assert_eq!(
            imap_account_stage_payload_hash(&draft("a"), &endpoint(2, "imap.example", "u")),
            Err(ErrorCode::InvalidInput)
        );
    }

    #[test]
    fn acc01_stage_atomically_creates_connecting_without_invented_address_or_scope() {
        let f = Fixture::new();
        let s = &f.store;
        let d = NewImapAccountDraft::new(
            "private_id".into(),
            Some("Private Label".into()),
            Some("#123456".into()),
        )
        .unwrap();
        let e = endpoint(1, "IMAP.EXAMPLE", "PrivateLogin");
        let receipt = s
            .stage_imap_account(&d, &e, &mutation(&d, &e, "op", "key"))
            .unwrap();
        assert_eq!(
            receipt,
            StagedImapAccount {
                account_id: "private_id".into(),
                account_version: 1,
                imap_version: 1
            }
        );
        let state = s.load_imap_provisioning_state("private_id").unwrap();
        assert_eq!(state.status, AccountStatus::Connecting);
        assert_eq!(state.account_version, 1);
        assert_eq!(state.endpoint, e);
        assert_eq!(state.capabilities, Vec::<String>::new());
        assert_eq!(state.observation, None);
        s.with_connection(|c| { let row=c.query_row("SELECT provider,provider_subject,primary_address,display_name,color,account_number,granted_scopes FROM accounts",[],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,i64>(5)?,r.get::<_,String>(6)?)))?; assert_eq!(row,("imap".into(),None,None,"Private Label".into(),"#123456".into(),1,"[]".into())); Ok(()) }).unwrap();
        stage(s, "next");
        assert_eq!(
            s.list_account_summaries(&["next".into()], false)
                .unwrap()
                .items[0]
                .account_number,
            2
        );
        assert!(
            s.load_mail_connection("private_id")
                .unwrap()
                .unwrap()
                .smtp()
                .is_none()
        );
        for output in [
            format!("{receipt:?}"),
            format!("{state:?}"),
            format!(
                "{:?}",
                InitialImapProbeOutcome::Ready(vec!["private_capability".into()])
            ),
        ] {
            for private in [
                "private_id",
                "Private Label",
                "PrivateLogin",
                "imap.example",
                "private_capability",
            ] {
                assert!(!output.contains(private));
            }
        }
    }

    #[test]
    fn acc03_host_is_normalized_but_login_is_exact_for_every_account_state() {
        for status in [
            "connecting",
            "ready",
            "reauth_required",
            "offline",
            "disabled",
            "error",
        ] {
            let f = Fixture::new();
            let s = &f.store;
            stage(s, "a");
            sql(s, &format!("UPDATE accounts SET status='{status}'"));
            let d = draft("b");
            let e=MailConnectionConfig::from_json(br#"{"schema_version":1,"account_id":"b","imap":{"settings_version":1,"host":"IMAP.EXAMPLE","port":143,"tls":"starttls_required","login_name":"a","authentication":"oauth2"}}"#).unwrap().imap().clone();
            let before = snapshot(s);
            assert_eq!(
                s.stage_imap_account(&d, &e, &mutation(&d, &e, "op-b", "key-b")),
                Err(ErrorCode::Conflict)
            );
            assert_eq!(snapshot(s), before);
            let e = endpoint(1, "imap.example", "A");
            s.stage_imap_account(&d, &e, &mutation(&d, &e, "op-b", "key-b"))
                .unwrap();
        }
    }

    #[test]
    fn acc03_new_account_rejects_corrupt_unrelated_imap_endpoints_without_writes() {
        for corrupt in [
            "UPDATE mail_connection_settings SET host='IMAP.OTHER.EXAMPLE'",
            "UPDATE mail_connection_settings SET host='bad host'",
            "UPDATE mail_connection_settings SET host='2001:0db8::1'",
            "UPDATE mail_connection_settings SET login_name='other'||char(10)",
            "UPDATE mail_connection_settings SET port=0",
            "UPDATE mail_connection_settings SET authentication='unknown'",
            "UPDATE mail_connection_settings SET settings_version=0",
            "UPDATE mail_connection_settings SET updated_at_utc_ms=0",
            "UPDATE accounts SET provider='graph'",
        ] {
            let f = Fixture::new();
            let s = &f.store;
            stage(s, "existing");
            sql(s, "PRAGMA ignore_check_constraints=ON");
            sql(s, corrupt);
            sql(s, "PRAGMA ignore_check_constraints=OFF");
            let before = snapshot(s);
            let d = draft("new");
            let e = endpoint(1, "new.example", "NewLogin");
            assert_eq!(
                s.stage_imap_account(&d, &e, &mutation(&d, &e, "op-new", "key-new")),
                Err(ErrorCode::InternalError)
            );
            assert_eq!(snapshot(s), before);
        }
    }

    #[test]
    fn acc03_duplicate_scan_checks_other_rows_before_returning_conflict() {
        let f = Fixture::new();
        let s = &f.store;
        stage(s, "a");
        stage(s, "z");
        sql(
            s,
            "UPDATE mail_connection_settings SET host='BAD.EXAMPLE' WHERE account_id='z'",
        );
        let before = snapshot(s);
        let d = draft("new");
        let e = endpoint(1, "IMAP.EXAMPLE", "a");
        assert_eq!(
            s.stage_imap_account(&d, &e, &mutation(&d, &e, "op-new", "key-new")),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(snapshot(s), before);
    }

    #[test]
    fn acc01_exact_stage_replay_returns_fixed_versions_without_overwriting_current_data() {
        let f = Fixture::new();
        let s = &f.store;
        let d = draft("a");
        let e = endpoint(1, "imap.example", "a");
        let m = mutation(&d, &e, "op", "key");
        let receipt = s.stage_imap_account(&d, &e, &m).unwrap();
        finish(
            s,
            "a",
            &InitialImapProbeOutcome::Ready(vec!["imap.idle".into()]),
        )
        .unwrap();
        sql(
            s,
            "UPDATE accounts SET display_name='Updated',account_number=42",
        );
        let e2 = endpoint(2, "changed.example", "changed");
        s.put_imap_connection(
            "a",
            &e2,
            &MailMutation::new(
                "edit".into(),
                "edit-key".into(),
                1,
                imap_payload_hash("a", &e2, 1).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let before = snapshot(s);
        assert_eq!(s.stage_imap_account(&d, &e, &m), Ok(receipt));
        assert_eq!(snapshot(s), before);
        assert_eq!(s.load_imap_provisioning_state("a").unwrap().endpoint, e2);
    }

    #[test]
    fn acc01_stage_replay_requires_valid_linked_account_and_endpoint() {
        for corrupt in [
            "DELETE FROM mail_connection_settings",
            "UPDATE accounts SET provider='gmail'",
            "UPDATE accounts SET status='unknown'",
            "UPDATE accounts SET display_name=''",
            "UPDATE mail_connection_settings SET host='IMAP.EXAMPLE'",
        ] {
            let f = Fixture::new();
            let s = &f.store;
            stage(s, "a");
            sql(s, corrupt);
            let d = draft("a");
            let e = endpoint(1, "imap.example", "a");
            let before = snapshot(s);
            assert_eq!(
                s.stage_imap_account(&d, &e, &mutation(&d, &e, "op-a", "key-a")),
                Err(ErrorCode::InternalError)
            );
            assert_eq!(snapshot(s), before);
        }
    }

    #[test]
    fn acc01_stage_conflicts_on_operation_key_account_and_payload_collisions() {
        let f = Fixture::new();
        let s = &f.store;
        stage(s, "a");
        let before = snapshot(s);
        for (id, host, op, key) in [
            ("a", "imap.example", "different-op", "key-a"),
            ("a", "imap.example", "op-a", "different-key"),
            ("a", "other.example", "op-a", "key-a"),
            ("b", "imap.example", "op-a", "key-b"),
            ("a", "imap.example", "new-op", "new-key"),
        ] {
            let d = draft(id);
            let e = endpoint(1, host, id);
            assert_eq!(
                s.stage_imap_account(&d, &e, &mutation(&d, &e, op, key)),
                Err(ErrorCode::Conflict)
            );
            assert_eq!(snapshot(s), before);
        }
        let d = draft("a");
        let e = endpoint(1, "imap.example", "a");
        assert_eq!(
            s.put_imap_connection("a", &e, &mutation(&d, &e, "op-a", "key-a")),
            Err(ErrorCode::InvalidInput)
        );
        let m = MailMutation::new(
            "op-a".into(),
            "key-a".into(),
            0,
            imap_payload_hash("a", &e, 0).unwrap(),
        )
        .unwrap();
        assert_eq!(
            s.stage_imap_account(&d, &e, &m),
            Err(ErrorCode::InvalidInput)
        );
    }

    #[test]
    fn acc01_smtp_ledger_collision_cannot_be_replayed_as_account_creation() {
        let f = Fixture::new();
        let s = &f.store;
        stage(s, "a");
        let config=MailConnectionConfig::from_json(br#"{"schema_version":1,"account_id":"a","imap":{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"a","authentication":"app_password"},"smtp":{"settings_version":1,"host":"smtp.example","port":465,"tls":"implicit_tls","login_name":"a","authentication":"app_password"}}"#).unwrap();
        let smtp = config.smtp().unwrap();
        s.put_smtp_connection(
            "a",
            smtp,
            &MailMutation::new(
                "smtp-op".into(),
                "smtp-key".into(),
                0,
                smtp_payload_hash("a", smtp, 0).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let d = draft("b");
        let e = endpoint(1, "other.example", "b");
        let before = snapshot(s);
        assert_eq!(
            s.stage_imap_account(&d, &e, &mutation(&d, &e, "smtp-op", "key-b")),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(snapshot(s), before);
    }

    #[test]
    fn acc01_stage_rejects_nonzero_expected_or_wrong_hash_without_writes() {
        let f = Fixture::new();
        let s = &f.store;
        let d = draft("a");
        let e = endpoint(1, "imap.example", "u");
        let before = snapshot(s);
        for m in [
            MailMutation::new(
                "op".into(),
                "key".into(),
                1,
                imap_account_stage_payload_hash(&d, &e).unwrap(),
            )
            .unwrap(),
            MailMutation::new("op".into(), "key".into(), 0, [9; 32]).unwrap(),
        ] {
            assert_eq!(
                s.stage_imap_account(&d, &e, &m),
                Err(ErrorCode::InvalidInput)
            );
            assert_eq!(snapshot(s), before);
        }
    }

    #[test]
    fn acc01_each_stage_insert_fault_rolls_back_all_three_rows() {
        for table in [
            "accounts",
            "mail_connection_settings",
            "mail_connection_operations",
        ] {
            let f = Fixture::new();
            let s = &f.store;
            stage(s, "existing");
            sql(
                s,
                &format!(
                    "CREATE TRIGGER fail_stage AFTER INSERT ON {table} BEGIN SELECT RAISE(FAIL,'fixture'); END"
                ),
            );
            let before = snapshot(s);
            let d = draft("new");
            let e = endpoint(1, "new.example", "new");
            assert_eq!(
                s.stage_imap_account(&d, &e, &mutation(&d, &e, "op-new", "key-new")),
                Err(ErrorCode::InternalError)
            );
            assert_eq!(snapshot(s), before);
        }
    }

    #[test]
    fn acc01_number_exhaustion_and_global_operation_quota_preserve_data() {
        let f = Fixture::new();
        let s = &f.store;
        stage(s, "a");
        sql(s, "UPDATE accounts SET account_number=9223372036854775807");
        let d = draft("b");
        let e = endpoint(1, "imap.example", "b");
        let m = mutation(&d, &e, "op-b", "key-b");
        let before = snapshot(s);
        assert_eq!(s.stage_imap_account(&d, &e, &m), Err(ErrorCode::Conflict));
        assert_eq!(snapshot(s), before);
        sql(
            s,
            "UPDATE accounts SET account_number=1; WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<99999) INSERT INTO mail_connection_operations(operation_id,account_id,idempotency_key,endpoint_kind,payload_hash,expected_version,result_version,completed_at_utc_ms) SELECT 'quota-'||x,'a','quota-'||x,'imap',zeroblob(32),0,1,1 FROM n",
        );
        assert_eq!(
            s.stage_imap_account(&d, &e, &m),
            Err(ErrorCode::QuotaExceeded)
        );
        assert_eq!(
            s.load_imap_provisioning_state("b"),
            Err(ErrorCode::NotFound)
        );
        assert_eq!(stage(s, "a").account_version, 1);
        assert_eq!(
            s.with_connection(|c| c.query_row(
                "SELECT count(*) FROM mail_connection_operations",
                [],
                |r| r.get::<_, i64>(0)
            ))
            .unwrap(),
            100000
        );
    }

    #[test]
    fn acc01_probe_outcomes_map_atomically_and_cannot_finalize_twice() {
        for (outcome, status, observed) in [
            (
                InitialImapProbeOutcome::Ready(vec!["imap.idle".into(), "imap.uidplus".into()]),
                AccountStatus::Ready,
                MailConnectionState::Ready,
            ),
            (
                InitialImapProbeOutcome::AuthRequired,
                AccountStatus::ReauthRequired,
                MailConnectionState::AuthRequired,
            ),
            (
                InitialImapProbeOutcome::TlsFailed,
                AccountStatus::Error,
                MailConnectionState::TlsFailed,
            ),
            (
                InitialImapProbeOutcome::Offline,
                AccountStatus::Offline,
                MailConnectionState::Offline,
            ),
            (
                InitialImapProbeOutcome::Error,
                AccountStatus::Error,
                MailConnectionState::Error,
            ),
        ] {
            let f = Fixture::new();
            let s = &f.store;
            stage(s, "a");
            stage(s, "b");
            let other = s.load_imap_provisioning_state("b").unwrap();
            let receipt = finish(s, "a", &outcome).unwrap();
            assert_eq!(
                receipt,
                InitialImapProbeReceipt {
                    account_version: 2,
                    endpoint_version: 1,
                    observation_version: 1
                }
            );
            let state = s.load_imap_provisioning_state("a").unwrap();
            assert_eq!(state.status, status);
            assert_eq!(state.account_version, 2);
            assert_eq!(
                state.capabilities,
                if let InitialImapProbeOutcome::Ready(values) = &outcome {
                    values.clone()
                } else {
                    Vec::new()
                }
            );
            let observation = state.observation.unwrap();
            assert_eq!(observation.state, observed);
            assert_eq!(observation.observation_version, 1);
            assert_eq!(observation.observed_settings_version, 1);
            assert!(observation.observed_at_utc_ms > 0);
            assert_eq!(s.load_imap_provisioning_state("b").unwrap(), other);
            let before = snapshot(s);
            assert_eq!(
                s.finish_initial_imap_probe("a", 2, 1, 1, &outcome),
                Err(ErrorCode::Conflict)
            );
            assert_eq!(snapshot(s), before);
        }
    }

    #[test]
    fn acc01_probe_requires_sorted_unique_known_capabilities_and_valid_versions() {
        let f = Fixture::new();
        let s = &f.store;
        stage(s, "a");
        let before = snapshot(s);
        for caps in [
            vec!["unknown"],
            vec!["imap.idle", "imap.idle"],
            vec!["imap.uidplus", "imap.idle"],
            vec!["imap.idle\n"],
        ] {
            assert_eq!(
                finish(
                    s,
                    "a",
                    &InitialImapProbeOutcome::Ready(caps.into_iter().map(str::to_owned).collect())
                ),
                Err(ErrorCode::InvalidInput)
            );
            assert_eq!(snapshot(s), before);
        }
        for (a, e, o) in [
            (0, 1, 0),
            (1, 0, 0),
            (i64::MAX as u64, 1, 0),
            (1, i64::MAX as u64 + 1, 0),
            (1, 1, i64::MAX as u64),
        ] {
            assert_eq!(
                s.finish_initial_imap_probe("a", a, e, o, &InitialImapProbeOutcome::Error),
                Err(ErrorCode::InvalidInput)
            );
            assert_eq!(snapshot(s), before);
        }
        assert_eq!(
            finish(s, "../a", &InitialImapProbeOutcome::Error),
            Err(ErrorCode::InvalidInput)
        );
        finish(
            s,
            "a",
            &InitialImapProbeOutcome::Ready(
                [
                    "imap.condstore",
                    "imap.idle",
                    "imap.move",
                    "imap.qresync",
                    "imap.special_use",
                    "imap.uidplus",
                ]
                .map(str::to_owned)
                .to_vec(),
            ),
        )
        .unwrap();
    }

    #[test]
    fn acc01_probe_checks_all_three_versions_and_missing_or_other_provider() {
        let f = Fixture::new();
        let s = &f.store;
        stage(s, "a");
        let before = snapshot(s);
        for (a, e, o) in [(2, 1, 0), (1, 2, 0), (1, 1, 1)] {
            assert_eq!(
                s.finish_initial_imap_probe("a", a, e, o, &InitialImapProbeOutcome::Error),
                Err(ErrorCode::Conflict)
            );
            assert_eq!(snapshot(s), before);
        }
        assert_eq!(
            finish(s, "missing", &InitialImapProbeOutcome::Error),
            Err(ErrorCode::NotFound)
        );
        sql(s, "UPDATE accounts SET provider='gmail'");
        assert_eq!(
            finish(s, "a", &InitialImapProbeOutcome::Error),
            Err(ErrorCode::CapabilityUnsupported)
        );
        assert_eq!(
            s.load_imap_provisioning_state("a"),
            Err(ErrorCode::CapabilityUnsupported)
        );
        sql(
            s,
            "UPDATE accounts SET provider='imap'; DELETE FROM mail_connection_settings",
        );
        assert_eq!(
            finish(s, "a", &InitialImapProbeOutcome::Error),
            Err(ErrorCode::NotFound)
        );
        assert_eq!(
            s.load_imap_provisioning_state("a"),
            Err(ErrorCode::NotFound)
        );
    }

    #[test]
    fn acc01_probe_clock_rollback_and_insert_update_faults_preserve_both_rows() {
        for (observation, table, event) in [
            (false, "accounts", "UPDATE"),
            (false, "mail_connection_observations", "INSERT"),
            (true, "mail_connection_observations", "UPDATE"),
        ] {
            let f = Fixture::new();
            let s = &f.store;
            stage(s, "a");
            let expected = if observation {
                s.record_mail_observation(
                    "a",
                    MailEndpointKind::Imap,
                    1,
                    0,
                    MailConnectionState::Offline,
                )
                .unwrap()
            } else {
                0
            };
            sql(
                s,
                &format!(
                    "CREATE TRIGGER fail_probe AFTER {event} ON {table} BEGIN SELECT RAISE(FAIL,'fixture'); END"
                ),
            );
            let before = snapshot(s);
            assert_eq!(
                s.finish_initial_imap_probe(
                    "a",
                    1,
                    1,
                    expected,
                    &InitialImapProbeOutcome::Ready(vec!["imap.idle".into()])
                ),
                Err(ErrorCode::InternalError)
            );
            assert_eq!(snapshot(s), before);
        }
        let f = Fixture::new();
        let s = &f.store;
        stage(s, "a");
        s.record_mail_observation(
            "a",
            MailEndpointKind::Imap,
            1,
            0,
            MailConnectionState::Offline,
        )
        .unwrap();
        s.with_connection(|c| {
            c.execute(
                "UPDATE mail_connection_observations SET observed_at_utc_ms=?1",
                params![crate::utc_millis()? + 600000],
            )
        })
        .unwrap();
        let before = snapshot(s);
        assert_eq!(
            s.finish_initial_imap_probe("a", 1, 1, 1, &InitialImapProbeOutcome::Error),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(snapshot(s), before);
    }

    #[test]
    fn acc01_probe_preserves_identity_grants_smtp_and_accepts_empty_ready_capabilities() {
        let f = Fixture::new();
        let s = &f.store;
        stage(s, "a");
        sql(
            s,
            "UPDATE accounts SET primary_address='display@example.invalid',provider_subject='synthetic-subject',granted_scopes='[\"scope\"]'; INSERT INTO mail_connection_settings SELECT account_id,'smtp',1,'smtp.example',465,'implicit_tls','out','app_password',updated_at_utc_ms FROM mail_connection_settings",
        );
        let before = s.load_mail_connection("a").unwrap().unwrap();
        finish(s, "a", &InitialImapProbeOutcome::Ready(Vec::new())).unwrap();
        assert_eq!(s.load_mail_connection("a").unwrap().unwrap(), before);
        s.with_connection(|c| {
            let preserved = c.query_row(
                "SELECT primary_address,provider_subject,granted_scopes FROM accounts",
                [],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )?;
            assert_eq!(
                preserved,
                (
                    "display@example.invalid".into(),
                    "synthetic-subject".into(),
                    "[\"scope\"]".into()
                )
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn acc01_snapshot_rejects_corrupt_settings_capabilities_status_and_observation() {
        for corrupt in [
            "UPDATE accounts SET status='unknown'",
            "UPDATE accounts SET capabilities='[\"imap.uidplus\",\"imap.idle\"]'",
            "UPDATE accounts SET capabilities='[\"imap.unknown\"]'",
            "UPDATE accounts SET capabilities='[1]'",
            "UPDATE mail_connection_settings SET host='IMAP.EXAMPLE'",
            "UPDATE mail_connection_observations SET observed_settings_version=2",
            "PRAGMA ignore_check_constraints=ON; UPDATE mail_connection_observations SET observed_at_utc_ms=0; PRAGMA ignore_check_constraints=OFF",
        ] {
            let f = Fixture::new();
            let s = &f.store;
            stage(s, "a");
            s.record_mail_observation(
                "a",
                MailEndpointKind::Imap,
                1,
                0,
                MailConnectionState::Offline,
            )
            .unwrap();
            sql(s, corrupt);
            let before = snapshot(s);
            assert_eq!(
                s.load_imap_provisioning_state("a"),
                Err(ErrorCode::InternalError)
            );
            assert_eq!(
                s.finish_initial_imap_probe("a", 1, 1, 1, &InitialImapProbeOutcome::Error),
                Err(ErrorCode::InternalError)
            );
            assert_eq!(snapshot(s), before);
        }
        let f = Fixture::new();
        assert_eq!(
            f.store.load_imap_provisioning_state("none"),
            Err(ErrorCode::NotFound)
        );
        assert_eq!(
            f.store.load_imap_provisioning_state(""),
            Err(ErrorCode::InvalidInput)
        );
    }
}

#[cfg(not(windows))]
#[test]
fn acc01_provisioning_is_unsupported_without_windows_store() {
    let store = Store {};
    assert_eq!(
        store.load_imap_provisioning_state("a"),
        Err(ErrorCode::CapabilityUnsupported)
    );
    assert_eq!(
        store.finish_initial_imap_probe("a", 1, 1, 0, &InitialImapProbeOutcome::Error),
        Err(ErrorCode::CapabilityUnsupported)
    );
}
