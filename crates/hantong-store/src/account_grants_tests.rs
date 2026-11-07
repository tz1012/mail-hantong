use super::*;
use crate::{CliRegistrationStage, ClientKind, DEFAULT_CLI_LABEL, Store};
use hantong_contracts::ErrorCode;
use hantong_contracts::auth::{RequestedScope, default_cli_legacy_tools};
use rusqlite::params;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const A: &str = "grant-account-a";
const B: &str = "grant-account-b";

struct Fixture {
    store: Store,
    root: PathBuf,
    registration_id: [u8; 16],
    version: u64,
    epoch: u64,
    _clock: crate::TestUtcClock,
}

impl Fixture {
    fn new() -> Self {
        let clock = crate::TestUtcClock::freeze(crate::utc_millis().unwrap());
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-account-grants-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        crate::test_support::ensure_test_backend();
        let root = fs::canonicalize(root).unwrap();
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        let mut key = [0u8; 72];
        key[..8].copy_from_slice(b"ECS1\x20\0\0\0");
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
        let pin = hantong_platform::sha256(&key).unwrap();
        store
            .with_connection(|c| {
                c.execute(
                    "INSERT INTO server_identity VALUES(1,?1,'active',?2,1,1,1,1)",
                    params![pin.as_slice(), key.as_slice()],
                )?;
                for account in [A, B] {
                    c.execute(
                        "INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES(?1,'imap',1,'ready','[]','[]',1)",
                        [account],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        let request = store
            .create_registration_request(
                [0x41; 32],
                ClientKind::Cli,
                DEFAULT_CLI_LABEL,
                &RequestedScope::new(vec![A.into()], default_cli_legacy_tools()).unwrap(),
            )
            .unwrap();
        let registration_id =
            hantong_platform::registration_id_for_request(request.request_id).unwrap();
        let staged = store
            .stage_terminal_cli_registration(
                &CliRegistrationStage::new_terminal_cli(request, registration_id, key, pin)
                    .unwrap(),
            )
            .unwrap();
        store
            .activate_cli_registration(registration_id, staged.version, pin)
            .unwrap();
        let refresh = store
            .create_terminal_cli_scope_refresh(
                [0x43; 32],
                RequestedScope::new(
                    vec![A.into()],
                    hantong_contracts::auth::default_cli_body_tools(),
                )
                .unwrap(),
                pin,
            )
            .unwrap();
        let staged = store.stage_cli_scope_refresh(&refresh).unwrap();
        store.activate_cli_scope_refresh(&staged).unwrap();
        let registration = store.load_registration(registration_id).unwrap().unwrap();
        Self {
            store,
            root,
            registration_id,
            version: registration.version,
            epoch: registration.profile_epoch,
            _clock: clock,
        }
    }

    fn publication(&self, decision: [u8; 32]) -> PublishedBinding {
        let stage = self
            .store
            .staged_full_mail_decision(self.registration_id, decision)
            .unwrap();
        let key = stage.base.public_key;
        let profile = hantong_contracts::account_profile::ParsedAccountClientProfile::new(
            hantong_contracts::account_profile::AccountProfileSelector::Default,
            self.registration_id,
            hantong_platform::client_key_id_for_registration(self.registration_id).unwrap(),
            key,
            key,
            stage.target_epoch,
            stage.target.clone(),
        )
        .unwrap();
        PublishedBinding {
            registration_id: self.registration_id,
            epoch: stage.target_epoch,
            client_key_hash: hantong_platform::sha256(&key).unwrap(),
            server_key_hash: stage.server_public_key_hash,
            scope_hash: hantong_platform::sha256(&stage.target.to_json().unwrap()).unwrap(),
            profile_hash: hantong_platform::sha256(&profile.to_json().unwrap()).unwrap(),
        }
    }

    fn begin(&self, decision: u8, baseline: [u8; 32], expiry: i64) -> MailGrantDecision {
        let baseline = if baseline == [0; 32] {
            self.store
                .full_mail_baseline_hash(self.registration_id)
                .unwrap()
        } else {
            baseline
        };
        self.store
            .begin_full_mail_decision(
                [decision; 32],
                self.registration_id,
                self.version,
                self.epoch,
                baseline,
                B,
                MailGrantPurpose::Add,
                &format!("op-{decision}"),
                &format!("idem-{decision}"),
                [decision; 32],
                expiry,
            )
            .unwrap()
    }
}

#[test]
fn sec03_account_grants_test_clock_is_scoped_nested_and_thread_local() {
    assert_eq!(crate::TEST_UTC_CLOCK.with(std::cell::Cell::get), None);
    {
        let _outer = crate::TestUtcClock::freeze(1_000);
        assert_eq!(crate::utc_millis().unwrap(), 1_000);
        {
            let _inner = crate::TestUtcClock::freeze(500);
            assert_eq!(crate::utc_millis().unwrap(), 500);
        }
        assert_eq!(crate::utc_millis().unwrap(), 1_000);
        assert_eq!(
            std::thread::spawn(|| crate::TEST_UTC_CLOCK.with(std::cell::Cell::get))
                .join()
                .unwrap(),
            None
        );
    }
    assert_eq!(crate::TEST_UTC_CLOCK.with(std::cell::Cell::get), None);
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn acc02_begin_is_pending_and_does_not_change_legacy_rights() {
    let f = Fixture::new();
    let before = f
        .store
        .load_registration(f.registration_id)
        .unwrap()
        .unwrap();
    let decision = f.begin(1, [0; 32], crate::utc_millis().unwrap() + 60_000);
    assert_eq!(decision.state, "pending");
    assert!(
        f.store
            .load_account_scope_v2(f.registration_id)
            .unwrap()
            .is_none()
    );
    let after = f
        .store
        .load_registration(f.registration_id)
        .unwrap()
        .unwrap();
    assert_eq!(before.version, after.version);
    assert_eq!(before.profile_epoch, after.profile_epoch);
    assert_eq!(before.scope, after.scope);
}

#[test]
fn sec03_cancel_is_bound_and_single_use() {
    let f = Fixture::new();
    let d = f.begin(2, [0; 32], crate::utc_millis().unwrap() + 60_000);
    assert!(
        f.store
            .cancel_full_mail_decision([0x42; 16], d.decision_id, [2; 32])
            .is_err()
    );
    f.store
        .cancel_full_mail_decision(f.registration_id, d.decision_id, [2; 32])
        .unwrap();
    assert!(
        f.store
            .cancel_full_mail_decision(f.registration_id, d.decision_id, [2; 32])
            .is_err()
    );
}

#[test]
fn sec03_expiry_is_terminal_before_stage() {
    let f = Fixture::new();
    let d = f.begin(3, [0; 32], crate::utc_millis().unwrap() + 60_000);
    f.store
        .with_connection(|c| {
            c.execute(
                "UPDATE account_scope_decisions_v2 SET created_at_utc_ms=1,expires_at_utc_ms=2 WHERE decision_id=?1",
                [d.decision_id.as_slice()],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.store.consume_and_stage_full_mail_decision(
            f.registration_id,
            d.decision_id,
            f.version,
            f.epoch,
            [3; 32],
            [81; 32]
        ),
        Err(ErrorCode::ApprovalExpired)
    );
}

#[test]
fn sec03_stale_baseline_hash_has_no_effect() {
    let f = Fixture::new();
    assert_eq!(
        f.store.begin_full_mail_decision(
            [4; 32],
            f.registration_id,
            f.version,
            f.epoch,
            [9; 32],
            B,
            MailGrantPurpose::Add,
            "op-stale",
            "idem-stale",
            [4; 32],
            crate::utc_millis().unwrap() + 60_000
        ),
        Err(ErrorCode::Conflict)
    );
}

#[test]
fn acc02_activation_preserves_a_and_grants_b() {
    let f = Fixture::new();
    let d = f.begin(5, [0; 32], crate::utc_millis().unwrap() + 60_000);
    f.store
        .consume_and_stage_full_mail_decision(
            f.registration_id,
            d.decision_id,
            f.version,
            f.epoch,
            [5; 32],
            [81; 32],
        )
        .unwrap();
    let publication = f.publication(d.decision_id);
    f.store
        .activate_bound_publication(
            f.registration_id,
            d.decision_id,
            f.version + 1,
            f.epoch + 1,
            f.store
                .full_mail_legacy_binding_hash(f.registration_id)
                .unwrap(),
            publication,
        )
        .unwrap();
    let scope = f
        .store
        .load_account_scope_v2(f.registration_id)
        .unwrap()
        .unwrap();
    assert!(scope.scope.permits_account(A, "mail.messages.search"));
    assert!(scope.scope.permits_account(A, "mail.messages.get"));
    assert!(scope.scope.permits_account(B, "mail.send.execute"));
    assert!(!scope.scope.permits_account(A, "mail.send.execute"));
    assert!(
        f.store
            .activate_bound_publication(
                f.registration_id,
                d.decision_id,
                f.version + 2,
                f.epoch + 2,
                f.store
                    .full_mail_legacy_binding_hash(f.registration_id)
                    .unwrap(),
                PublishedBinding {
                    registration_id: f.registration_id,
                    epoch: f.epoch + 2,
                    client_key_hash: [1; 32],
                    server_key_hash: [1; 32],
                    scope_hash: [1; 32],
                    profile_hash: [91; 32]
                }
            )
            .is_err()
    );
}

#[test]
fn sec03_committed_cleanup_requires_complete_active_history() {
    let f = Fixture::new();
    let d = f.begin(11, [0; 32], crate::utc_millis().unwrap() + 60_000);
    f.store
        .consume_and_stage_full_mail_decision(
            f.registration_id,
            d.decision_id,
            f.version,
            f.epoch,
            [11; 32],
            [81; 32],
        )
        .unwrap();
    f.store
        .activate_bound_publication(
            f.registration_id,
            d.decision_id,
            f.version + 1,
            f.epoch + 1,
            f.store
                .full_mail_legacy_binding_hash(f.registration_id)
                .unwrap(),
            f.publication(d.decision_id),
        )
        .unwrap();

    let rows = f.store.list_committed_full_mail_cleanup().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].registration_id, f.registration_id);
    assert_eq!(rows[0].decision_id, d.decision_id);
    assert_eq!(rows[0].expected_registration_version, f.version);
    assert_eq!(rows[0].expected_profile_epoch, f.epoch);
    assert_eq!(rows[0].target_account_id, B);
    let legacy_read_only = f
        .store
        .with_connection(|c| {
            crate::registrations::read_registration_legacy(c, &f.registration_id)
                .map(|record| record.unwrap().read_only)
        })
        .unwrap();
    assert_eq!(rows[0].legacy_read_only, legacy_read_only);
    assert_eq!(rows[0].client_public_key.len(), 72);

    f.store
        .with_connection(|c| {
            c.execute(
                "UPDATE account_scope_decisions_v2 SET target_scope_json='{}' WHERE decision_id=?1",
                [d.decision_id.as_slice()],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.store.list_committed_full_mail_cleanup(),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        f.store
            .with_connection(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM account_scope_v2 WHERE registration_id=?1",
                    [f.registration_id.as_slice()],
                    |r| r.get::<_, i64>(0),
                )
            })
            .unwrap(),
        1
    );
}

#[test]
fn sec03_pending_peer_replay_and_parallel_decision_have_no_effect() {
    let f = Fixture::new();
    let d = f.begin(6, [0; 32], crate::utc_millis().unwrap() + 60_000);
    assert_eq!(
        f.store.consume_and_stage_full_mail_decision(
            f.registration_id,
            d.decision_id,
            f.version,
            f.epoch,
            [7; 32],
            [81; 32]
        ),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store.begin_full_mail_decision(
            [7; 32],
            f.registration_id,
            f.version,
            f.epoch,
            f.store.full_mail_baseline_hash(f.registration_id).unwrap(),
            B,
            MailGrantPurpose::Add,
            "parallel-op",
            "parallel-key",
            [7; 32],
            crate::utc_millis().unwrap() + 60_000
        ),
        Err(ErrorCode::Conflict)
    );
    assert!(
        f.store
            .load_account_scope_v2(f.registration_id)
            .unwrap()
            .is_none()
    );
    let before = f
        .store
        .load_registration(f.registration_id)
        .unwrap()
        .unwrap();
    assert_eq!(before.profile_epoch, f.epoch);
    assert_eq!(before.version, f.version);
    assert!(!before.scope.permits_account(B, "mail.messages.get"));
}

#[test]
fn sec03_cancel_expires_old_pending_before_recording_cancellation() {
    let f = Fixture::new();
    let d = f.begin(8, [0; 32], crate::utc_millis().unwrap() + 60_000);
    f.store.with_connection(|c|c.execute("UPDATE account_scope_decisions_v2 SET created_at_utc_ms=1,expires_at_utc_ms=2 WHERE decision_id=?1",[d.decision_id.as_slice()])).unwrap();
    assert_eq!(
        f.store
            .cancel_full_mail_decision(f.registration_id, d.decision_id, [8; 32]),
        Err(ErrorCode::ApprovalExpired)
    );
    f.store
        .with_connection(|c| {
            let state: String = c.query_row(
                "SELECT state FROM account_scope_decisions_v2 WHERE decision_id=?1",
                [d.decision_id.as_slice()],
                |r| r.get(0),
            )?;
            assert_eq!(state, "expired");
            Ok(())
        })
        .unwrap();
    assert!(
        f.store
            .load_account_scope_v2(f.registration_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn sec03_staged_decision_cannot_activate_after_raw_registration_suspension() {
    let f = Fixture::new();
    let d = f.begin(9, [0; 32], crate::utc_millis().unwrap() + 60_000);
    let binding = f
        .store
        .full_mail_legacy_binding_hash(f.registration_id)
        .unwrap();
    f.store
        .consume_and_stage_full_mail_decision(
            f.registration_id,
            d.decision_id,
            f.version,
            f.epoch,
            [9; 32],
            [81; 32],
        )
        .unwrap();
    let publication = f.publication(d.decision_id);
    f.store.with_connection(|c|c.execute("UPDATE client_registrations SET state='suspended',version=version+1 WHERE registration_id=?1",[f.registration_id.as_slice()])).unwrap();
    assert_eq!(
        f.store.activate_bound_publication(
            f.registration_id,
            d.decision_id,
            f.version + 1,
            f.epoch + 1,
            binding,
            publication
        ),
        Err(ErrorCode::ClientSuspended)
    );
    assert!(
        f.store
            .load_account_scope_v2(f.registration_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn sec03_cancellation_remains_available_after_registration_suspension() {
    let f = Fixture::new();
    let d = f.begin(10, [0; 32], crate::utc_millis().unwrap() + 60_000);
    f.store.with_connection(|c| c.execute("UPDATE client_registrations SET state='suspended',version=version+1 WHERE registration_id=?1",[f.registration_id.as_slice()])).unwrap();
    f.store
        .cancel_full_mail_decision(f.registration_id, d.decision_id, [10; 32])
        .unwrap();
    assert_eq!(
        f.store
            .cancel_full_mail_decision(f.registration_id, d.decision_id, [10; 32]),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store
            .load_registration(f.registration_id)
            .unwrap()
            .unwrap()
            .state,
        crate::RegistrationState::Suspended
    );
}

#[test]
fn sec03_runtime_projection_denies_staged_auth_and_exposes_exact_active_matrix() {
    let f = Fixture::new();
    let original = f
        .store
        .load_registration(f.registration_id)
        .unwrap()
        .unwrap();
    let d = f.begin(11, [0; 32], crate::utc_millis().unwrap() + 60_000);
    f.store
        .consume_and_stage_full_mail_decision(
            f.registration_id,
            d.decision_id,
            f.version,
            f.epoch,
            [11; 32],
            [81; 32],
        )
        .unwrap();
    let staged = f
        .store
        .load_registration(f.registration_id)
        .unwrap()
        .unwrap();
    assert_eq!(staged.state, crate::RegistrationState::Provisioning);
    assert_eq!(staged.scope, original.scope);
    assert!(!staged.scope.permits_account(B, "mail.send.execute"));
    f.store
        .activate_bound_publication(
            f.registration_id,
            d.decision_id,
            f.version + 1,
            f.epoch + 1,
            f.store
                .full_mail_legacy_binding_hash(f.registration_id)
                .unwrap(),
            f.publication(d.decision_id),
        )
        .unwrap();
    let active = f
        .store
        .load_registration(f.registration_id)
        .unwrap()
        .unwrap();
    assert_eq!(active.state, crate::RegistrationState::Active);
    assert_eq!(active.scope.scope_model_version(), 2);
    assert_eq!(
        (active.version, active.profile_epoch),
        (f.version + 1, f.epoch + 1)
    );
    assert!(active.scope.permits_account(A, "mail.messages.get"));
    assert!(!active.scope.permits_account(A, "mail.send.execute"));
    for tool in FULL_MAIL {
        assert!(active.scope.permits_account(B, tool));
    }
    assert!(
        !active
            .scope
            .permits_account("not-selected", "mail.messages.get")
    );
    f.store
        .with_connection(|c| {
            let raw =
                crate::registrations::read_registration_legacy(c, &f.registration_id)?.unwrap();
            assert_eq!(raw.scope, original.scope);
            assert_eq!(
                (raw.version, raw.profile_epoch),
                (original.version, original.profile_epoch)
            );
            Ok(())
        })
        .unwrap();
    f.store
        .with_connection(|c| {
            c.execute(
                "UPDATE account_scope_decisions_v2 SET baseline_scope_hash=?1 WHERE decision_id=?2",
                params![[99_u8; 32].as_slice(), d.decision_id.as_slice()],
            )
        })
        .unwrap();
    assert!(f.store.load_registration(f.registration_id).is_err());
    assert!(f.store.load_account_scope_v2(f.registration_id).is_err());
}

#[test]
fn sec03_completed_duplicate_returns_receipt_without_advancing_authority() {
    let f = Fixture::new();
    let baseline = f.store.full_mail_baseline_hash(f.registration_id).unwrap();
    let expiry = crate::utc_millis().unwrap() + 60_000;
    let d = f.begin(12, baseline, expiry);
    f.store
        .consume_and_stage_full_mail_decision(
            f.registration_id,
            d.decision_id,
            f.version,
            f.epoch,
            [12; 32],
            [81; 32],
        )
        .unwrap();
    f.store
        .activate_bound_publication(
            f.registration_id,
            d.decision_id,
            f.version + 1,
            f.epoch + 1,
            f.store
                .full_mail_legacy_binding_hash(f.registration_id)
                .unwrap(),
            f.publication(d.decision_id),
        )
        .unwrap();
    let duplicate = f
        .store
        .begin_full_mail_decision(
            [12; 32],
            f.registration_id,
            f.version,
            f.epoch,
            baseline,
            B,
            MailGrantPurpose::Add,
            "op-12",
            "idem-12",
            [12; 32],
            expiry,
        )
        .unwrap();
    assert_eq!(duplicate.state, "active");
    assert_eq!(
        f.store
            .load_registration(f.registration_id)
            .unwrap()
            .unwrap()
            .profile_epoch,
        f.epoch + 1
    );
    assert_eq!(
        f.store.begin_full_mail_decision(
            [13; 32],
            f.registration_id,
            f.version,
            f.epoch,
            baseline,
            B,
            MailGrantPurpose::Add,
            "op-12",
            "idem-12",
            [12; 32],
            expiry
        ),
        Err(ErrorCode::Conflict)
    );
}

#[test]
fn sec03_new_decision_expires_stale_pending_without_grant() {
    let f = Fixture::new();
    let old = f.begin(13, [0; 32], crate::utc_millis().unwrap() + 60_000);
    f.store.with_connection(|c|c.execute("UPDATE account_scope_decisions_v2 SET created_at_utc_ms=1,expires_at_utc_ms=2 WHERE decision_id=?1",[old.decision_id.as_slice()])).unwrap();
    let fresh = f.begin(14, [0; 32], crate::utc_millis().unwrap() + 60_000);
    assert_eq!(fresh.state, "pending");
    f.store
        .with_connection(|c| {
            let state: String = c.query_row(
                "SELECT state FROM account_scope_decisions_v2 WHERE decision_id=?1",
                [old.decision_id.as_slice()],
                |r| r.get(0),
            )?;
            assert_eq!(state, "expired");
            Ok(())
        })
        .unwrap();
    assert!(
        !f.store
            .load_registration(f.registration_id)
            .unwrap()
            .unwrap()
            .scope
            .permits_account(B, "mail.messages.get")
    );
}

#[test]
fn sec03_legacy_pending_after_intent_blocks_stage_without_consuming_intent() {
    let f = Fixture::new();
    let d = f.begin(15, [0; 32], crate::utc_millis().unwrap() + 60_000);
    let pin = f
        .store
        .with_connection(|c| Ok(server_pin(c)))
        .unwrap()
        .unwrap();
    f.store
        .create_terminal_cli_scope_refresh(
            [0x61; 32],
            RequestedScope::new(
                vec![A.into()],
                hantong_contracts::auth::default_cli_manage_tools(),
            )
            .unwrap(),
            pin,
        )
        .unwrap();
    assert_eq!(
        f.store.consume_and_stage_full_mail_decision(
            f.registration_id,
            d.decision_id,
            f.version,
            f.epoch,
            [15; 32],
            [81; 32]
        ),
        Err(ErrorCode::Conflict)
    );
    f.store
        .with_connection(|c| {
            let state: String = c.query_row(
                "SELECT state FROM account_scope_decisions_v2 WHERE decision_id=?1",
                [d.decision_id.as_slice()],
                |r| r.get(0),
            )?;
            assert_eq!(state, "pending");
            Ok(())
        })
        .unwrap();
}

#[test]
fn acc02_native_v27_startup_migration_preserves_existing_registration_and_rights() {
    let f = Fixture::new();
    let original = f
        .store
        .load_registration(f.registration_id)
        .unwrap()
        .unwrap();
    f.store.with_connection(|c| {
        c.execute_batch("DROP TABLE account_scope_decisions_v2; DROP TABLE account_scope_v2; DROP TABLE account_full_mail_readiness; UPDATE app_metadata SET schema_version=27; PRAGMA user_version=27;")?;
        crate::validate_schema(c,crate::SCHEMA_V27,27)
    }).unwrap();
    f.store.ensure_schema().unwrap();
    let after = f
        .store
        .load_registration(f.registration_id)
        .unwrap()
        .unwrap();
    assert_eq!(after, original);
    assert_eq!(after.scope.scope_model_version(), 1);
    assert!(!after.scope.permits_account(B, "mail.messages.get"));
    f.store
        .with_connection(|c| {
            for table in [
                "account_scope_v2",
                "account_scope_decisions_v2",
                "account_full_mail_readiness",
            ] {
                assert_eq!(
                    c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                        .get::<_, i64>(0))?,
                    0
                );
            }
            assert!(
                c.prepare("PRAGMA foreign_key_check")?
                    .query([])?
                    .next()?
                    .is_none()
            );
            crate::validate_schema(c, crate::SCHEMA, crate::CURRENT_SCHEMA_VERSION)
        })
        .unwrap();
}
