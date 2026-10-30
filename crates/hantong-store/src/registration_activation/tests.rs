use super::*;
mod gui_tests;
use crate::RegistrationState;
use hantong_contracts::{auth::RequestedScope, wire_hex};
use rusqlite::params;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    store: Option<Store>,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-registration-activation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let mut value = Self { store: None, root };
        value.store = Some(Store::open(value.root.join("data"), value.root.join("keys")).unwrap());
        value.store().with_connection(|c| {
            c.execute("INSERT INTO server_identity(singleton,key_id,state,public_key,key_epoch,version,created_at_utc_ms,updated_at_utc_ms) VALUES(1,?1,'active',?2,1,1,1,1)",params![[9_u8;32].as_slice(),public_key().as_slice()])?;
            Ok(())
        }).unwrap();
        value
    }
    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }
    fn account(&self, id: &str) {
        self.store().with_connection(|c| {
            c.execute("INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES(?1,'imap',1,'ready','[]','[]',1)",[id])?;
            Ok(())
        }).unwrap();
    }
    fn request(&self, id: u8, accounts: &[&str]) -> RegistrationRequest {
        self.store()
            .create_registration_request(
                [id; 32],
                ClientKind::Cli,
                DEFAULT_CLI_LABEL,
                &scope(accounts),
            )
            .unwrap()
    }
    fn stage(&self, request: RegistrationRequest, _id: u8) -> CliRegistrationStage {
        let id = hantong_platform::registration_id_for_request(request.request_id).unwrap();
        CliRegistrationStage::new(request, id, public_key(), server_hash()).unwrap()
    }
    fn reopen(&mut self) {
        drop(self.store.take());
        self.store = Some(Store::open(self.root.join("data"), self.root.join("keys")).unwrap());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.store.take());
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn public_key() -> [u8; 72] {
    // Public NIST P-256 generator; no private key or persisted CNG key is used.
    let mut key = [0; 72];
    key[..8].copy_from_slice(b"ECS1\x20\0\0\0");
    key[8..40].copy_from_slice(
        &wire_hex::decode_fixed::<32>(
            "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
        )
        .unwrap(),
    );
    key[40..].copy_from_slice(
        &wire_hex::decode_fixed::<32>(
            "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
        )
        .unwrap(),
    );
    key
}
fn rid(request: u8) -> [u8; 16] {
    hantong_platform::registration_id_for_request([request; 32]).unwrap()
}
fn server_hash() -> [u8; 32] {
    hantong_platform::sha256(&public_key()).unwrap()
}
fn scope(accounts: &[&str]) -> RequestedScope {
    RequestedScope::new(
        accounts.iter().map(|s| (*s).into()).collect(),
        vec!["mail.accounts.list".into()],
    )
    .unwrap()
}
fn count(store: &Store, table: &str) -> i64 {
    // Test-owned fixed table identifiers only.
    store
        .with_connection(|c| {
            c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        })
        .unwrap()
}
fn unchanged(f: &Fixture, request: &RegistrationRequest) {
    assert_eq!(
        f.store()
            .get_registration_request(request.request_id)
            .unwrap(),
        Some(request.clone())
    );
    assert_eq!(count(f.store(), "client_registrations"), 0);
    assert_eq!(count(f.store(), "client_registration_accounts"), 0);
    assert_eq!(count(f.store(), "client_registration_tools"), 0);
}
fn other_registration(
    f: &Fixture,
    request_id: u8,
    registration_id: u8,
    label: &str,
    key: [u8; 72],
) {
    let now = crate::utc_millis().unwrap();
    f.store().with_connection(|c| {
        c.execute("INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,decision_channel,version,created_at_utc_ms,expires_at_utc_ms,decided_at_utc_ms) VALUES(?1,'cli',?2,'[]','[\"mail.accounts.list\"]','approved','approval_helper',2,?3,?4,?3)",params![[request_id;32].as_slice(),label,now,now+300000])?;
        c.execute("INSERT INTO client_registrations(registration_id,request_id,client_kind,display_label,public_key,profile_epoch,state,read_only,requests_per_minute,approved_channel,approved_at_utc_ms,updated_at_utc_ms,revoked_at_utc_ms,version) VALUES(?1,?2,'cli',?3,?4,1,'provisioning',1,60,'approval_helper',?5,?5,NULL,1)",params![[registration_id;16].as_slice(),[request_id;32].as_slice(),label,key.as_slice(),now])?;
        c.execute("INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,'mail.accounts.list')",[[registration_id;16].as_slice()])?;
        Ok(())
    }).unwrap();
}

#[test]
fn cli07_stage_is_atomic_provisioning_then_activation_survives_reopen() {
    let mut f = Fixture::new();
    f.account("b");
    f.account("a");
    let request = f.request(1, &["b", "a"]);
    let staged = f
        .store()
        .stage_cli_registration(&f.stage(request.clone(), 2))
        .unwrap();
    assert_eq!(staged.state, RegistrationState::Provisioning);
    assert_eq!(staged.version, 1);
    assert_eq!(staged.profile_epoch, 1);
    assert!(staged.read_only);
    assert_eq!(staged.requests_per_minute, 60);
    assert_eq!(staged.approved_channel, "approval_helper");
    assert_eq!(staged.scope, scope(&["a", "b"]));
    let approved = f
        .store()
        .get_registration_request([1; 32])
        .unwrap()
        .unwrap();
    assert_eq!(approved.state, RegistrationRequestState::Approved);
    assert_eq!(approved.version, 2);
    assert_eq!(
        f.store().load_registration_by_request([1; 32]).unwrap(),
        Some(staged.clone())
    );
    f.reopen();
    assert_eq!(
        f.store().list_provisioning_registrations(1).unwrap(),
        vec![staged]
    );
    let active = f
        .store()
        .activate_cli_registration(rid(1), 1, server_hash())
        .unwrap();
    assert_eq!(active.state, RegistrationState::Active);
    assert_eq!(active.version, 2);
    assert_eq!(active.profile_epoch, 1);
    assert_eq!(active.scope, scope(&["a", "b"]));
    assert!(
        f.store()
            .list_provisioning_registrations(1)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        f.store()
            .activate_cli_registration(rid(1), 2, server_hash()),
        Err(ErrorCode::Conflict)
    );
    f.reopen();
    assert_eq!(f.store().load_registration(rid(1)).unwrap(), Some(active));
}
#[test]
fn cli07_stage_empty_scope_grants_zero_and_input_validation_is_fixed() {
    let f = Fixture::new();
    let request = f.request(1, &[]);
    for mutate in 0..9 {
        let mut invalid = request.clone();
        match mutate {
            0 => invalid.request_id = [0; 32],
            1 => invalid.version = 0,
            2 => invalid.version = i64::MAX as u64,
            3 => invalid.client_kind = ClientKind::Mcp,
            4 => invalid.display_label = "other".into(),
            5 => invalid.state = RegistrationRequestState::Approved,
            6 => {
                invalid.scope =
                    RequestedScope::new(vec![], vec!["mail.send.execute".into()]).unwrap()
            }
            7 => invalid.created_at_utc_ms = 0,
            _ => invalid.expires_at_utc_ms = invalid.created_at_utc_ms + 300001,
        }
        assert!(matches!(
            CliRegistrationStage::new(invalid, rid(1), public_key(), server_hash()),
            Err(ErrorCode::InvalidInput)
        ));
    }
    assert!(matches!(
        CliRegistrationStage::new(request.clone(), [0; 16], public_key(), server_hash()),
        Err(ErrorCode::InvalidInput)
    ));
    assert!(matches!(
        CliRegistrationStage::new(request.clone(), rid(1), public_key(), [0; 32]),
        Err(ErrorCode::InvalidInput)
    ));
    assert!(matches!(
        CliRegistrationStage::new(request.clone(), rid(1), [0; 72], server_hash()),
        Err(ErrorCode::InvalidInput)
    ));
    let stage = f.stage(request, 2);
    assert_eq!(format!("{stage:?}"), "CliRegistrationStage { <redacted> }");
    let record = f.store().stage_cli_registration(&stage).unwrap();
    assert!(record.scope.accounts().is_empty());
    assert_eq!(count(f.store(), "client_registration_accounts"), 0);
}
#[test]
fn cli07_stage_stale_snapshot_scope_and_wrong_pin_leave_no_partial_grant() {
    for variant in 0..4 {
        let f = Fixture::new();
        f.account("a");
        f.account("b");
        let current = f.request(1, &["a"]);
        let mut expected = current.clone();
        if variant == 0 {
            expected.version += 1;
        }
        if variant == 1 {
            expected.scope = scope(&["b"]);
        }
        if variant == 2 {
            expected.expires_at_utc_ms -= 1;
        }
        let stage = CliRegistrationStage::new(
            expected,
            rid(1),
            public_key(),
            if variant == 3 { [3; 32] } else { server_hash() },
        )
        .unwrap();
        assert_eq!(
            f.store().stage_cli_registration(&stage),
            Err(ErrorCode::Conflict)
        );
        unchanged(&f, &current);
    }
}
#[test]
fn cli07_stage_expired_future_and_missing_account_or_request_fail_closed() {
    for future in [false, true] {
        let f = Fixture::new();
        let request = f.request(1, &[]);
        let now = crate::utc_millis().unwrap();
        let created = if future { now + 60000 } else { now - 60000 };
        let expires = if future { now + 120000 } else { now - 1 };
        f.store().with_connection(|c| { c.execute("UPDATE client_registration_requests SET created_at_utc_ms=?1,expires_at_utc_ms=?2 WHERE request_id=?3",params![created,expires,request.request_id.as_slice()])?; Ok(()) }).unwrap();
        let current = f
            .store()
            .get_registration_request(request.request_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            f.store()
                .stage_cli_registration(&f.stage(current.clone(), 2)),
            Err(ErrorCode::ApprovalExpired)
        );
        unchanged(&f, &current);
    }
    let f = Fixture::new();
    let request = f.request(1, &["absent"]);
    assert_eq!(
        f.store()
            .stage_cli_registration(&f.stage(request.clone(), 2)),
        Err(ErrorCode::NotFound)
    );
    unchanged(&f, &request);
    f.store()
        .with_connection(|c| {
            c.execute("DELETE FROM client_registration_requests", [])?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.store().stage_cli_registration(&f.stage(request, 2)),
        Err(ErrorCode::NotFound)
    );
}
#[test]
fn cli07_stage_rejects_invalid_public_point_and_missing_or_corrupt_server() {
    let f = Fixture::new();
    let request = f.request(1, &[]);
    let mut invalid = public_key();
    invalid[8..].fill(0);
    let stage = CliRegistrationStage::new(request.clone(), rid(1), invalid, server_hash()).unwrap();
    assert_eq!(
        f.store().stage_cli_registration(&stage),
        Err(ErrorCode::InvalidInput)
    );
    unchanged(&f, &request);
    f.store()
        .with_connection(|c| {
            c.execute("DELETE FROM server_identity", [])?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.store()
            .stage_cli_registration(&f.stage(request.clone(), 2)),
        Err(ErrorCode::Conflict)
    );
    unchanged(&f, &request);
    for variant in 0..3 {
        let f = Fixture::new();
        let request = f.request(1, &[]);
        f.store()
            .with_connection(|c| {
                match variant {
                    0 => c.execute(
                        "UPDATE server_identity SET state='creating',public_key=NULL",
                        [],
                    )?,
                    1 => c.execute(
                        "UPDATE server_identity SET public_key=?1",
                        [invalid.as_slice()],
                    )?,
                    _ => c.execute("UPDATE server_identity SET key_id=zeroblob(32)", [])?,
                };
                Ok(())
            })
            .unwrap();
        assert_eq!(
            f.store()
                .stage_cli_registration(&f.stage(request.clone(), 2)),
            Err(if variant == 0 {
                ErrorCode::Conflict
            } else {
                ErrorCode::InternalError
            })
        );
        unchanged(&f, &request);
    }
}
#[test]
fn cli07_duplicate_default_id_key_and_request_do_not_reapprove() {
    for variant in 0..3 {
        let f = Fixture::new();
        let current = f.request(1, &[]);
        let mut other_key = public_key();
        other_key[8] = other_key[8].wrapping_add(1);
        other_registration(
            &f,
            3,
            4,
            if variant == 1 {
                DEFAULT_CLI_LABEL
            } else {
                "other"
            },
            if variant == 2 {
                public_key()
            } else {
                other_key
            },
        );
        if variant == 0 {
            f.store()
                .with_connection(|c| {
                    c.execute("DELETE FROM client_registration_tools", [])?;
                    c.execute(
                        "UPDATE client_registrations SET registration_id=?1",
                        [rid(1).as_slice()],
                    )?;
                    Ok(())
                })
                .unwrap();
        }
        assert_eq!(
            f.store()
                .stage_cli_registration(&f.stage(current.clone(), 2)),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            f.store().get_registration_request([1; 32]).unwrap(),
            Some(current)
        );
        assert_eq!(count(f.store(), "client_registrations"), 1);
    }
    for state in ["active", "suspended", "revoked"] {
        let f = Fixture::new();
        let current = f.request(1, &[]);
        let mut other_key = public_key();
        other_key[8] += 1;
        other_registration(&f, 3, 4, DEFAULT_CLI_LABEL, other_key);
        f.store().with_connection(|c| {
            c.execute("UPDATE client_registrations SET state=?1,revoked_at_utc_ms=CASE WHEN ?1='revoked' THEN approved_at_utc_ms ELSE NULL END",[state])?;
            Ok(())
        }).unwrap();
        assert_eq!(
            f.store()
                .stage_cli_registration(&f.stage(current.clone(), 2)),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            f.store()
                .get_registration_request(current.request_id)
                .unwrap(),
            Some(current)
        );
    }
    let f = Fixture::new();
    let request = f.request(1, &[]);
    let stage = f.stage(request, 2);
    f.store().stage_cli_registration(&stage).unwrap();
    assert_eq!(
        f.store().stage_cli_registration(&stage),
        Err(ErrorCode::Conflict)
    );
}
#[test]
fn cli07_stage_database_failure_rolls_back_request_registration_and_grants() {
    let f = Fixture::new();
    f.account("a");
    let request = f.request(1, &["a"]);
    f.store().with_connection(|c| c.execute_batch("CREATE TRIGGER fail_registration_grant BEFORE INSERT ON client_registration_tools BEGIN SELECT RAISE(ABORT,'fixture'); END;")).unwrap();
    assert_eq!(
        f.store()
            .stage_cli_registration(&f.stage(request.clone(), 2)),
        Err(ErrorCode::InternalError)
    );
    unchanged(&f, &request);
}
#[test]
fn cli07_activation_checks_version_pin_grant_and_clock_without_mutation() {
    let f = Fixture::new();
    f.account("a");
    let request = f.request(1, &["a"]);
    let staged = f
        .store()
        .stage_cli_registration(&f.stage(request, 2))
        .unwrap();
    for version in [0, u64::MAX] {
        assert_eq!(
            f.store()
                .activate_cli_registration(rid(1), version, server_hash()),
            Err(ErrorCode::InvalidInput)
        );
    }
    assert_eq!(
        f.store()
            .activate_cli_registration(rid(1), i64::MAX as u64, server_hash()),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store()
            .activate_cli_registration([0; 16], 1, server_hash()),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        f.store().activate_cli_registration(rid(1), 1, [0; 32]),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        f.store()
            .activate_cli_registration(rid(1), 2, server_hash()),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store().activate_cli_registration(rid(1), 1, [6; 32]),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store()
            .activate_cli_registration([8; 16], 1, server_hash()),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(
        f.store().load_registration(rid(1)).unwrap(),
        Some(staged.clone())
    );
    f.store()
        .with_connection(|c| {
            c.execute(
                "UPDATE client_registrations SET updated_at_utc_ms=?1",
                [crate::utc_millis()? + 60000],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.store()
            .activate_cli_registration(rid(1), 1, server_hash()),
        Err(ErrorCode::ApprovalExpired)
    );
    f.store()
        .with_connection(|c| {
            c.execute(
                "UPDATE client_registrations SET updated_at_utc_ms=approved_at_utc_ms",
                [],
            )?;
            c.execute("DELETE FROM client_registration_accounts", [])?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.store()
            .activate_cli_registration(rid(1), 1, server_hash()),
        Err(ErrorCode::InternalError)
    );
    // Default CLI grants must retain approved provenance even before initial
    // activation; the deliberately removed grant is corrupt stored state.
    assert_eq!(
        f.store().load_registration(rid(1)),
        Err(ErrorCode::InternalError)
    );
}
#[test]
fn cli07_explicit_denial_is_terminal_and_stale_or_expired_denials_do_not_write() {
    let f = Fixture::new();
    let request = f.request(1, &[]);
    let stage = f.stage(request.clone(), 2);
    assert_eq!(
        f.store().deny_cli_registration_request([0; 32], 1),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        f.store().deny_cli_registration_request([1; 32], 0),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        f.store()
            .deny_cli_registration_request([1; 32], i64::MAX as u64),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store().deny_cli_registration_request([8; 32], 1),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(
        f.store().deny_cli_registration_request([1; 32], 2),
        Err(ErrorCode::Conflict)
    );
    unchanged(&f, &request);
    f.store().deny_cli_registration_request([1; 32], 1).unwrap();
    let denied = f
        .store()
        .get_registration_request([1; 32])
        .unwrap()
        .unwrap();
    assert_eq!(denied.state, RegistrationRequestState::Denied);
    assert_eq!(denied.version, 2);
    assert_eq!(
        f.store().deny_cli_registration_request([1; 32], 2),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store().stage_cli_registration(&stage),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(count(f.store(), "client_registrations"), 0);
    let request = f.request(3, &[]);
    f.store().with_connection(|c| { c.execute("UPDATE client_registration_requests SET created_at_utc_ms=1,expires_at_utc_ms=2 WHERE request_id=?1",[request.request_id.as_slice()])?; Ok(()) }).unwrap();
    assert_eq!(
        f.store().deny_cli_registration_request([3; 32], 1),
        Err(ErrorCode::ApprovalExpired)
    );
    assert_eq!(
        f.store()
            .get_registration_request([3; 32])
            .unwrap()
            .unwrap()
            .state,
        RegistrationRequestState::Pending
    );
}
#[test]
fn cli07_recovery_queries_are_bounded_and_reject_corrupt_records() {
    let f = Fixture::new();
    assert_eq!(
        f.store().load_registration_by_request([1; 32]).unwrap(),
        None
    );
    assert!(
        f.store()
            .list_provisioning_registrations(1)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        f.store().list_provisioning_registrations(0),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        f.store().list_provisioning_registrations(129),
        Err(ErrorCode::InvalidInput)
    );
    other_registration(&f, 1, 2, "first", public_key());
    let mut other_key = public_key();
    other_key[8] += 1;
    other_registration(&f, 3, 4, "second", other_key);
    assert_eq!(
        f.store().list_provisioning_registrations(1),
        Err(ErrorCode::QuotaExceeded)
    );
    assert_eq!(
        f.store().list_provisioning_registrations(2).unwrap().len(),
        2
    );
    f.store().with_connection(|c| { c.execute("UPDATE client_registration_requests SET display_label='changed' WHERE request_id=?1",[[1;32].as_slice()])?; Ok(()) }).unwrap();
    assert_eq!(
        f.store().load_registration_by_request([1; 32]),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        f.store().list_provisioning_registrations(2),
        Err(ErrorCode::InternalError)
    );
}
#[test]
fn cli07_account_choices_sort_validate_and_never_truncate_or_expand_empty_scope() {
    let f = Fixture::new();
    assert!(f.store().list_registrable_account_ids().unwrap().is_empty());
    f.account("z");
    f.account("a");
    assert_eq!(
        f.store().list_registrable_account_ids().unwrap(),
        vec!["a", "z"]
    );
    f.account("bad space");
    assert_eq!(
        f.store().list_registrable_account_ids(),
        Err(ErrorCode::InternalError)
    );
    f.store().with_connection(|c| { c.execute("DELETE FROM accounts",[])?; for n in 0..129 { c.execute("INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES(?1,'imap',1,'ready','[]','[]',1)",[format!("a{n:03}")])?; } Ok(()) }).unwrap();
    assert_eq!(
        f.store().list_registrable_account_ids(),
        Err(ErrorCode::QuotaExceeded)
    );
}

#[test]
fn cli15_default_queries_bound_pending_and_keep_terminal_registration() {
    let f = Fixture::new();
    assert_eq!(f.store().load_default_cli_registration().unwrap(), None);
    assert_eq!(f.store().load_pending_default_cli_request().unwrap(), None);
    let request = f.request(1, &[]);
    assert_eq!(
        f.store().load_pending_default_cli_request().unwrap(),
        Some(request.clone())
    );
    f.request(2, &[]);
    assert_eq!(
        f.store().load_pending_default_cli_request(),
        Err(ErrorCode::Conflict)
    );
    f.store()
        .with_connection(|c| {
            c.execute(
                "UPDATE client_registration_requests SET created_at_utc_ms=1,expires_at_utc_ms=2",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(f.store().load_pending_default_cli_request().unwrap(), None);
    let current = f.request(3, &[]);
    let staged = f
        .store()
        .stage_cli_registration(&f.stage(current, 4))
        .unwrap();
    assert_eq!(
        f.store().load_default_cli_registration().unwrap(),
        Some(staged)
    );
    assert_eq!(f.store().load_pending_default_cli_request().unwrap(), None);
    f.store().with_connection(|c| { c.execute("UPDATE client_registrations SET state='revoked',revoked_at_utc_ms=approved_at_utc_ms",[])?; Ok(()) }).unwrap();
    assert_eq!(
        f.store()
            .load_default_cli_registration()
            .unwrap()
            .unwrap()
            .state,
        RegistrationState::Revoked
    );
}

#[test]
fn cli15_pending_query_rejects_clock_rollback_and_invalid_metadata() {
    let f = Fixture::new();
    f.request(1, &[]);
    f.store()
        .with_connection(|c| {
            let now = crate::utc_millis()?;
            c.execute(
                "UPDATE client_registration_requests SET created_at_utc_ms=?1,expires_at_utc_ms=?2",
                params![now + 60000, now + 120000],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.store().load_pending_default_cli_request(),
        Err(ErrorCode::ApprovalExpired)
    );
    f.store()
        .with_connection(|c| {
            c.execute(
                "UPDATE client_registration_requests SET requested_tools_json='[\"wrong.tool\"]'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.store().load_pending_default_cli_request(),
        Err(ErrorCode::InternalError)
    );
}

#[test]
fn cli15_durable_request_reserves_only_derived_registration_and_rejects_zero_id() {
    let f = Fixture::new();
    let request = f.request(1, &[]);
    let wrong =
        CliRegistrationStage::new(request.clone(), [2; 16], public_key(), server_hash()).unwrap();
    assert_eq!(
        f.store().stage_cli_registration(&wrong),
        Err(ErrorCode::InvalidInput)
    );
    unchanged(&f, &request);
    assert_eq!(
        f.store().create_registration_request(
            [0; 32],
            ClientKind::Cli,
            DEFAULT_CLI_LABEL,
            &scope(&[])
        ),
        Err(ErrorCode::InvalidInput)
    );
}
#[test]
fn cli15_orphan_scan_keeps_denial_and_rejects_missing_approved_registration() {
    let f = Fixture::new();
    let pending = f.request(1, &[]);
    f.request(2, &[]);
    f.store().deny_cli_registration_request([2; 32], 1).unwrap();
    let rows = f.store().list_orphan_cli_registration_requests().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], pending);
    assert_eq!(rows[1].state, RegistrationRequestState::Denied);
    f.store().with_connection(|c| {c.execute("UPDATE client_registration_requests SET state='approved',decision_channel='approval_helper',decided_at_utc_ms=created_at_utc_ms WHERE request_id=?1",[[1;32].as_slice()])?;Ok(())}).unwrap();
    assert_eq!(
        f.store().list_orphan_cli_registration_requests(),
        Err(ErrorCode::InternalError)
    );
}
#[test]
fn cli15_orphan_ownership_never_deletes_linked_public_key_or_id_collision() {
    let f = Fixture::new();
    f.request(1, &[]);
    assert_eq!(
        f.store().registration_key_is_linked([1; 32], public_key()),
        Ok(false)
    );
    other_registration(&f, 3, 4, "other", public_key());
    assert_eq!(
        f.store().registration_key_is_linked([1; 32], public_key()),
        Ok(true)
    );
    let mut key = public_key();
    key[8] ^= 1;
    f.store().with_connection(|c| {c.execute("DELETE FROM client_registration_tools",[])?;c.execute("UPDATE client_registrations SET registration_id=?1",[rid(1).as_slice()])?;c.execute("INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,'mail.accounts.list')",[rid(1).as_slice()])?;Ok(())}).unwrap();
    assert_eq!(f.store().registration_key_is_linked([1; 32], key), Ok(true));
    assert_eq!(
        f.store().registration_key_is_linked([0; 32], key),
        Err(ErrorCode::InvalidInput)
    );
}
#[test]
fn cli15_orphan_scan_rejects_zero_persisted_id_and_overflow() {
    let f = Fixture::new();
    f.request(1, &[]);
    f.store()
        .with_connection(|c| {
            c.execute(
                "UPDATE client_registration_requests SET request_id=zeroblob(32)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.store().list_orphan_cli_registration_requests(),
        Err(ErrorCode::InternalError)
    );
    f.store().with_connection(|c| {c.execute("DELETE FROM client_registration_requests",[])?;let now=crate::utc_millis()?;for n in 1_u32..=1025 {let mut id=[1_u8;32];id[..4].copy_from_slice(&n.to_le_bytes());c.execute("INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,version,created_at_utc_ms,expires_at_utc_ms) VALUES(?1,'cli',?2,'[]','[\"mail.accounts.list\"]','pending',1,?3,?4)",params![id.as_slice(),DEFAULT_CLI_LABEL,now,now+300000])?;}Ok(())}).unwrap();
    assert_eq!(
        f.store().list_orphan_cli_registration_requests(),
        Err(ErrorCode::QuotaExceeded)
    );
}
