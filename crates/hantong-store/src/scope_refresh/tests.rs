use super::*;
use crate::{CliRegistrationStage, ClientKind, DEFAULT_CLI_LABEL, RegistrationState};
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
    registration: RegistrationRecord,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-scope-refresh-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        store.with_connection(|c| {
            c.execute("INSERT INTO server_identity VALUES(1,?1,'active',?2,1,1,1,1)",params![[9_u8;32].as_slice(),public_key().as_slice()])?;
            for id in ["a","b","c"] {c.execute("INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES(?1,'imap',1,'ready','[]','[]',1)",[id])?;}
            Ok(())
        }).unwrap();
        let request = store
            .create_registration_request(
                [1; 32],
                ClientKind::Cli,
                DEFAULT_CLI_LABEL,
                &scope(&["a"]),
            )
            .unwrap();
        let id = hantong_platform::registration_id_for_request(request.request_id).unwrap();
        let staged = store
            .stage_cli_registration(
                &CliRegistrationStage::new(request, id, public_key(), pin()).unwrap(),
            )
            .unwrap();
        let registration = store
            .activate_cli_registration(id, staged.version, pin())
            .unwrap();
        Self {
            store: Some(store),
            root,
            registration,
        }
    }
    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }
    fn create(&self, id: u8, target: &[&str]) -> CliScopeRefresh {
        self.store()
            .create_cli_scope_refresh(
                [id; 32],
                target.iter().map(|v| (*v).into()).collect(),
                pin(),
            )
            .unwrap()
    }
    fn create_scope(&self, id: u8, accounts: &[&str], tools: Vec<String>) -> CliScopeRefresh {
        self.store()
            .create_cli_scope_refresh_with_scope(
                [id; 32],
                RequestedScope::new(accounts.iter().map(|v| (*v).into()).collect(), tools).unwrap(),
                pin(),
            )
            .unwrap()
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
fn pin() -> [u8; 32] {
    hantong_platform::sha256(&public_key()).unwrap()
}
fn scope(accounts: &[&str]) -> RequestedScope {
    RequestedScope::new(
        accounts.iter().map(|v| (*v).into()).collect(),
        vec!["mail.accounts.list".into()],
    )
    .unwrap()
}
fn sql(f: &Fixture, sql: &str) {
    f.store().with_connection(|c| c.execute_batch(sql)).unwrap();
}
fn snapshot(f: &Fixture) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
    f.store()
        .with_connection(|c| {
            [
                "client_registration_requests",
                "client_registrations",
                "client_registration_accounts",
                "client_registration_tools",
                "server_identity",
                "client_scope_refreshes",
            ]
            .iter()
            .map(|table| {
                let mut stmt = c.prepare(&format!("SELECT * FROM {table} ORDER BY 1,2"))?;
                let len = stmt.column_count();
                stmt.query_map([], |row| (0..len).map(|i| row.get(i)).collect())?
                    .collect()
            })
            .collect()
        })
        .unwrap()
}
fn independent_hash(r: &CliScopeRefresh) -> [u8; 32] {
    let b = &r.base_registration;
    let mut bytes = match r.hash_version {
        1 => b"mail-hantong.cli-scope-refresh.v1\0".to_vec(),
        2 => b"mail-hantong.cli-scope-refresh.v2\0".to_vec(),
        3 => b"mail-hantong.cli-scope-refresh.v3\0".to_vec(),
        4 => b"mail-hantong.cli-scope-refresh.v4\0".to_vec(),
        5 => b"mail-hantong.cli-scope-refresh.v5\0".to_vec(),
        _ => panic!("invalid fixture hash version"),
    };
    bytes.extend_from_slice(&r.refresh_id);
    bytes.extend_from_slice(&b.registration_id);
    bytes.extend_from_slice(&b.request_id);
    bytes.extend_from_slice(&b.version.to_be_bytes());
    bytes.extend_from_slice(&b.profile_epoch.to_be_bytes());
    bytes.extend_from_slice(&r.client_public_key_hash);
    bytes.extend_from_slice(&r.server_public_key_hash);
    bytes.extend_from_slice(&r.created_at_utc_ms.to_be_bytes());
    bytes.extend_from_slice(&r.expires_at_utc_ms.to_be_bytes());
    for values in [b.scope.accounts(), r.target_accounts.as_slice()] {
        bytes.extend_from_slice(&(values.len() as u32).to_be_bytes());
        for value in values {
            bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
            bytes.extend_from_slice(value.as_bytes());
        }
    }
    if r.hash_version >= 2 {
        for values in [b.scope.tools(), r.target_tools.as_slice()] {
            bytes.extend_from_slice(&(values.len() as u32).to_be_bytes());
            for value in values {
                bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
                bytes.extend_from_slice(value.as_bytes());
            }
        }
    }
    hantong_platform::sha256(&bytes).unwrap()
}

#[path = "app_read_tests.rs"]
mod app_read_tests;
#[path = "metadata_tests.rs"]
mod metadata_tests;
fn shift(f: &Fixture, r: &CliScopeRefresh, delta: i64) -> CliScopeRefresh {
    let mut changed = r.clone();
    changed.created_at_utc_ms += delta;
    changed.expires_at_utc_ms += delta;
    changed.updated_at_utc_ms += delta;
    changed.decided_at_utc_ms = changed.decided_at_utc_ms.map(|v| v + delta);
    changed.activated_at_utc_ms = changed.activated_at_utc_ms.map(|v| v + delta);
    changed.payload_hash = independent_hash(&changed);
    f.store().with_connection(|c| {
        c.execute("UPDATE client_scope_refreshes SET created_at_utc_ms=?2,expires_at_utc_ms=?3,updated_at_utc_ms=?4,decided_at_utc_ms=?5,activated_at_utc_ms=?6,payload_hash=?7 WHERE refresh_id=?1",params![changed.refresh_id.as_slice(),changed.created_at_utc_ms,changed.expires_at_utc_ms,changed.updated_at_utc_ms,changed.decided_at_utc_ms,changed.activated_at_utc_ms,changed.payload_hash.as_slice()])?;
        if delta<0 {
            c.execute("UPDATE client_registrations SET approved_at_utc_ms=approved_at_utc_ms+?1,updated_at_utc_ms=updated_at_utc_ms+?1",[delta])?;
            c.execute("UPDATE client_registration_requests SET created_at_utc_ms=created_at_utc_ms+?1,expires_at_utc_ms=expires_at_utc_ms+?1,decided_at_utc_ms=decided_at_utc_ms+?1",[delta])?;
        }
        Ok(())
    }).unwrap();
    changed
}

#[test]
fn cli15_uid_scope_historical_v3_domain_stays_exact() {
    let f = Fixture::new();
    let pending = set_historical_hash(&f, f.create(90, &["b"]), 3);
    assert_eq!(pending.hash_version, 3);
    assert_eq!(pending.payload_hash(), &independent_hash(&pending));
    let mut previous = pending.clone();
    previous.hash_version = 2;
    assert_ne!(pending.payload_hash(), &independent_hash(&previous));
}

#[test]
fn cli15_uid_scope_upgrade_keeps_old_grants_until_atomic_activation_and_rejects_downgrades() {
    let f = Fixture::new();
    let folder = hantong_contracts::auth::default_cli_folder_tools();
    let uid = hantong_contracts::auth::default_cli_uid_tools();
    let first = f.create_scope(81, &["a"], folder.clone());
    let first = f.store().stage_cli_scope_refresh(&first).unwrap();
    f.store().activate_cli_scope_refresh(&first).unwrap();
    let pending = f.create_scope(82, &["a"], uid.clone());
    let staged = f.store().stage_cli_scope_refresh(&pending).unwrap();
    assert_eq!(
        f.store()
            .load_default_cli_registration()
            .unwrap()
            .unwrap()
            .scope
            .tools(),
        folder
    );
    let before = snapshot(&f);
    sql(
        &f,
        "CREATE TRIGGER fail_uid_tool BEFORE INSERT ON client_registration_tools WHEN NEW.tool_id='mail.sync.page' BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
    );
    assert_eq!(
        f.store().activate_cli_scope_refresh(&staged),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(snapshot(&f), before);
    sql(&f, "DROP TRIGGER fail_uid_tool");
    let active = f.store().activate_cli_scope_refresh(&staged).unwrap();
    assert_eq!(active.scope.tools(), uid);
    let before = snapshot(&f);
    for target in [folder, default_cli_legacy_tools(), uid] {
        assert_eq!(
            f.store().create_cli_scope_refresh_with_scope(
                [83; 32],
                RequestedScope::new(vec!["a".into()], target).unwrap(),
                pin()
            ),
            Err(ErrorCode::Conflict)
        );
    }
    assert_eq!(snapshot(&f), before);
}

fn set_historical_hash(f: &Fixture, mut row: CliScopeRefresh, version: u8) -> CliScopeRefresh {
    row.hash_version = version;
    row.payload_hash = independent_hash(&row);
    f.store().with_connection(|c| c.execute("UPDATE client_scope_refreshes SET hash_version=?1,payload_hash=?2 WHERE refresh_id=?3", params![version, row.payload_hash.as_slice(), row.refresh_id.as_slice()])).unwrap();
    row
}

#[test]
fn cli15_uid_scope_v1_v2_pending_staged_and_active_history_remain_immutable() {
    for version in [1, 2] {
        for phase in [
            ScopeRefreshPhase::Pending,
            ScopeRefreshPhase::Staged,
            ScopeRefreshPhase::Active,
        ] {
            let mut f = Fixture::new();
            let tools = if version == 1 {
                default_cli_legacy_tools()
            } else {
                hantong_contracts::auth::default_cli_folder_tools()
            };
            let fresh = f.create_scope(84, &["b"], tools.clone());
            let mut old = set_historical_hash(&f, fresh, version);
            if phase != ScopeRefreshPhase::Pending {
                old = f.store().stage_cli_scope_refresh(&old).unwrap();
            }
            if phase == ScopeRefreshPhase::Active {
                f.store().activate_cli_scope_refresh(&old).unwrap();
                old = f.store().load_cli_scope_refresh([84; 32]).unwrap().unwrap();
            }
            let before = snapshot(&f);
            f.store()
                .with_connection(super::validate_history_v11)
                .unwrap();
            f.reopen();
            assert_eq!(snapshot(&f), before);
            assert_eq!(
                f.store().load_cli_scope_refresh([84; 32]).unwrap(),
                Some(old.clone())
            );
            assert_eq!(old.target_tools(), tools);
            if phase == ScopeRefreshPhase::Active {
                let next =
                    f.create_scope(85, &["b"], hantong_contracts::auth::default_cli_uid_tools());
                assert_eq!(next.hash_version, 5);
                assert!(
                    f.store()
                        .with_connection(super::validate_history_v11)
                        .is_err()
                );
                let staged = f.store().stage_cli_scope_refresh(&next).unwrap();
                f.store().activate_cli_scope_refresh(&staged).unwrap();
                assert_eq!(
                    f.store().load_cli_scope_refresh([84; 32]).unwrap(),
                    Some(old)
                );
            }
        }
    }
}

#[test]
fn sec03_uid_scope_old_hash_versions_cannot_encode_uid_even_with_recomputed_hash() {
    for version in [1, 2] {
        let f = Fixture::new();
        let fresh = f.create_scope(86, &["b"], hantong_contracts::auth::default_cli_uid_tools());
        sql(&f, "PRAGMA ignore_check_constraints=ON");
        let forged = set_historical_hash(&f, fresh, version);
        sql(&f, "PRAGMA ignore_check_constraints=OFF");
        assert_eq!(
            f.store().load_cli_scope_refresh(*forged.refresh_id()),
            Err(ErrorCode::InternalError)
        );
    }
}

#[test]
fn sec03_uid_scope_unreviewed_uid_grant_is_rejected_at_initial_and_staged_epochs() {
    for staged in [false, true] {
        let f = Fixture::new();
        if staged {
            let pending =
                f.create_scope(87, &["b"], hantong_contracts::auth::default_cli_uid_tools());
            f.store().stage_cli_scope_refresh(&pending).unwrap();
        }
        f.store().with_connection(|c| {
            for tool in ["mail.folders.list", "mail.folders.refresh", "mail.sync.page"] {
                c.execute("INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,?2)", params![f.registration.registration_id.as_slice(), tool])?;
            }
            Ok(())
        }).unwrap();
        assert_eq!(
            f.store().load_registration(f.registration.registration_id),
            Err(ErrorCode::InternalError)
        );
    }
}

#[test]
fn cli15_refresh_pending_is_data_only_and_hash_is_exact() {
    let f = Fixture::new();
    let before = snapshot(&f);
    let p = f.create(2, &["a", "b"]);
    let after = snapshot(&f);
    assert_eq!(&before[..5], &after[..5]);
    assert_eq!(p.base_registration(), &f.registration);
    assert_eq!(p.phase(), ScopeRefreshPhase::Pending);
    assert_eq!(p.version(), 1);
    assert_eq!(p.expires_at_utc_ms() - p.created_at_utc_ms(), 300000);
    assert_eq!(p.payload_hash(), &independent_hash(&p));
    assert_eq!(p.decided_at_utc_ms(), None);
    assert_eq!(p.activated_at_utc_ms(), None);
    assert_eq!(
        f.store().load_cli_scope_refresh([2; 32]).unwrap(),
        Some(p.clone())
    );
    assert_eq!(f.store().load_pending_cli_scope_refresh().unwrap(), Some(p));
    assert!(
        f.store()
            .list_staged_cli_scope_refreshes()
            .unwrap()
            .is_empty()
    );
    assert!(
        !format!("{:?}", f.store().load_cli_scope_refresh([2; 32])).contains(DEFAULT_CLI_LABEL)
    );
}

#[test]
fn cli15_refresh_folder_tool_upgrade_binds_scope_and_replaces_tools_atomically() {
    let f = Fixture::new();
    let pending = f.create_scope(
        9,
        &["a"],
        hantong_contracts::auth::default_cli_folder_tools(),
    );
    assert_eq!(
        pending.base_registration().scope.tools(),
        &["mail.accounts.list"]
    );
    assert_eq!(
        pending.target_tools(),
        hantong_contracts::auth::default_cli_folder_tools()
    );
    assert_eq!(
        pending.target_scope().tools(),
        hantong_contracts::auth::default_cli_folder_tools()
    );
    let staged = f.store().stage_cli_scope_refresh(&pending).unwrap();
    assert_eq!(
        f.store()
            .load_default_cli_registration()
            .unwrap()
            .unwrap()
            .scope
            .tools(),
        &["mail.accounts.list"]
    );
    let active = f.store().activate_cli_scope_refresh(&staged).unwrap();
    assert_eq!(
        active.scope.tools(),
        hantong_contracts::auth::default_cli_folder_tools().as_slice()
    );
    f.store()
        .with_connection(|c| {
            let mut stmt =
                c.prepare("SELECT tool_id FROM client_registration_tools ORDER BY tool_id")?;
            let tools: Vec<String> = stmt
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            assert_eq!(tools, hantong_contracts::auth::default_cli_folder_tools());
            Ok(())
        })
        .unwrap();
}

#[test]
fn cli15_refresh_rejects_folder_downgrade_noop_and_tampered_tool_history() {
    let f = Fixture::new();
    let pending = f.create_scope(
        10,
        &["a"],
        hantong_contracts::auth::default_cli_folder_tools(),
    );
    assert_eq!(
        f.store()
            .create_cli_scope_refresh_with_scope([11; 32], pending.target_scope(), pin()),
        Err(ErrorCode::Conflict)
    );
    let staged = f.store().stage_cli_scope_refresh(&pending).unwrap();
    f.store().activate_cli_scope_refresh(&staged).unwrap();
    assert_eq!(
        f.store()
            .create_cli_scope_refresh([12; 32], vec!["a".into()], pin()),
        Err(ErrorCode::Conflict)
    );
    sql(
        &f,
        "PRAGMA ignore_check_constraints=ON; UPDATE client_scope_refreshes SET target_tools_json='[\"mail.accounts.list\"]'",
    );
    assert_eq!(
        f.store().load_cli_scope_refresh([10; 32]),
        Err(ErrorCode::InternalError)
    );
    let f = Fixture::new();
    let pending = f.create_scope(
        13,
        &["a"],
        hantong_contracts::auth::default_cli_folder_tools(),
    );
    sql(
        &f,
        "PRAGMA ignore_check_constraints=ON; UPDATE client_scope_refreshes SET hash_version=1",
    );
    assert_eq!(
        f.store().load_cli_scope_refresh(*pending.refresh_id()),
        Err(ErrorCode::InternalError)
    );
}

#[test]
fn cli15_refresh_tool_grant_insert_failure_rolls_back_everything() {
    let f = Fixture::new();
    let pending = f.create_scope(
        14,
        &["a"],
        hantong_contracts::auth::default_cli_folder_tools(),
    );
    let staged = f.store().stage_cli_scope_refresh(&pending).unwrap();
    sql(
        &f,
        "CREATE TRIGGER fail_tool_insert BEFORE INSERT ON client_registration_tools BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
    );
    let before = snapshot(&f);
    assert_eq!(
        f.store().activate_cli_scope_refresh(&staged),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(snapshot(&f), before);
}

#[test]
fn cli15_refresh_folder_history_chains_without_rewriting_prior_scope() {
    let f = Fixture::new();
    let first = f.create_scope(
        15,
        &["a", "b"],
        hantong_contracts::auth::default_cli_folder_tools(),
    );
    let first = f.store().stage_cli_scope_refresh(&first).unwrap();
    let first = f.store().activate_cli_scope_refresh(&first).unwrap();
    assert_eq!(first.profile_epoch, 2);
    let second = f.create_scope(
        16,
        &["c"],
        hantong_contracts::auth::default_cli_folder_tools(),
    );
    let second = f.store().stage_cli_scope_refresh(&second).unwrap();
    f.store().activate_cli_scope_refresh(&second).unwrap();
    let old = f.store().load_cli_scope_refresh([15; 32]).unwrap().unwrap();
    assert_eq!(
        old.target_tools(),
        hantong_contracts::auth::default_cli_folder_tools()
    );
    assert_eq!(old.target_accounts(), &["a".to_string(), "b".to_string()]);
}
#[test]
fn cli15_refresh_stage_and_activate_preserve_original_request_key_and_old_grants_until_commit() {
    let mut f = Fixture::new();
    let before = snapshot(&f);
    let p = f.create(2, &["a", "b"]);
    let staged = f.store().stage_cli_scope_refresh(&p).unwrap();
    assert_eq!(staged.phase(), ScopeRefreshPhase::Staged);
    assert_eq!(staged.version(), 2);
    let current = f.store().load_default_cli_registration().unwrap().unwrap();
    assert_eq!(current.profile_epoch, 2);
    assert_eq!(current.version, 3);
    assert_eq!(current.state, RegistrationState::Provisioning);
    assert_eq!(current.scope, scope(&["a"]));
    assert_eq!(
        f.store().list_staged_cli_scope_refreshes().unwrap(),
        vec![staged.clone()]
    );
    assert_eq!(
        f.store()
            .activate_cli_registration(current.registration_id, current.version, pin()),
        Err(ErrorCode::Conflict)
    );
    let active = f.store().activate_cli_scope_refresh(&staged).unwrap();
    assert_eq!(active.profile_epoch, 2);
    assert_eq!(active.version, 4);
    assert_eq!(active.state, RegistrationState::Active);
    assert_eq!(active.scope, scope(&["a", "b"]));
    assert_eq!(active.public_key, f.registration.public_key);
    assert_eq!(active.request_id, f.registration.request_id);
    let after = snapshot(&f);
    assert_eq!(before[0], after[0]);
    assert_eq!(before[3], after[3]);
    assert_eq!(before[4], after[4]);
    f.reopen();
    assert_eq!(
        f.store().load_registration(active.registration_id).unwrap(),
        Some(active)
    );
    assert_eq!(
        f.store()
            .load_cli_scope_refresh([2; 32])
            .unwrap()
            .unwrap()
            .phase(),
        ScopeRefreshPhase::Active
    );
    assert_eq!(
        f.store().activate_cli_scope_refresh(&staged),
        Err(ErrorCode::Conflict)
    );
}
#[test]
fn cli15_refresh_empty_scope_and_multiple_successful_epochs_keep_history() {
    let f = Fixture::new();
    for (id, target) in [(2, vec!["a", "b"]), (3, vec![]), (4, vec!["c"])] {
        let p = f.create(id, &target);
        let s = f.store().stage_cli_scope_refresh(&p).unwrap();
        let a = f.store().activate_cli_scope_refresh(&s).unwrap();
        assert_eq!(a.profile_epoch, id as u64);
        assert_eq!(a.scope, scope(&target));
    }
    assert_eq!(snapshot(&f)[5].len(), 3);
    f.store().with_connection(validate_history).unwrap();
    assert_eq!(
        f.store()
            .load_cli_scope_refresh([2; 32])
            .unwrap()
            .unwrap()
            .base_registration()
            .scope,
        scope(&["a"])
    );
}
#[test]
fn cli15_refresh_denied_or_expired_can_retry_same_epoch_without_grant_changes() {
    let f = Fixture::new();
    let p = f.create(2, &["b"]);
    let before = snapshot(&f);
    assert!(!f.store().expire_cli_scope_refresh(&p).unwrap());
    assert_eq!(snapshot(&f), before);
    let denied = f.store().deny_cli_scope_refresh(&p).unwrap();
    assert_eq!(denied.phase(), ScopeRefreshPhase::Denied);
    assert_eq!(
        f.store().stage_cli_scope_refresh(&p),
        Err(ErrorCode::Conflict)
    );
    let p = f.create(3, &["b"]);
    let p = shift(&f, &p, -400000);
    assert_eq!(
        f.store().deny_cli_scope_refresh(&p),
        Err(ErrorCode::ApprovalExpired)
    );
    assert_eq!(
        f.store().stage_cli_scope_refresh(&p),
        Err(ErrorCode::ApprovalExpired)
    );
    assert!(f.store().expire_cli_scope_refresh(&p).unwrap());
    assert_eq!(
        f.store().expire_cli_scope_refresh(&p),
        Err(ErrorCode::Conflict)
    );
    let p = f.create(4, &["b"]);
    assert_eq!(p.target_profile_epoch(), 2);
    assert_eq!(
        f.store().load_default_cli_registration().unwrap().unwrap(),
        f.registration
    );
}
#[test]
fn cli15_refresh_invalid_input_missing_targets_duplicate_and_noop_do_not_write() {
    let f = Fixture::new();
    let before = snapshot(&f);
    for (id, accounts, hash, error) in [
        ([0; 32], vec!["b"], pin(), ErrorCode::InvalidInput),
        ([2; 32], vec!["b", "a"], pin(), ErrorCode::InvalidInput),
        ([2; 32], vec!["b", "b"], pin(), ErrorCode::InvalidInput),
        ([2; 32], vec!["a"], pin(), ErrorCode::Conflict),
        ([2; 32], vec!["missing"], pin(), ErrorCode::NotFound),
        ([2; 32], vec!["b"], [8; 32], ErrorCode::Conflict),
    ] {
        assert_eq!(
            f.store().create_cli_scope_refresh(
                id,
                accounts.into_iter().map(String::from).collect(),
                hash
            ),
            Err(error)
        );
        assert_eq!(snapshot(&f), before);
    }
    let p = f.create(2, &["b"]);
    let before = snapshot(&f);
    assert_eq!(
        f.store()
            .create_cli_scope_refresh([2; 32], vec!["b".into()], pin()),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store()
            .create_cli_scope_refresh([3; 32], vec!["c".into()], pin()),
        Err(ErrorCode::Conflict)
    );
    let mut stale = p.clone();
    stale.version += 1;
    assert_eq!(
        f.store().stage_cli_scope_refresh(&stale),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(snapshot(&f), before);
}
#[test]
fn cli15_refresh_stage_and_activation_faults_rollback_registration_grants_and_history() {
    for (verb, table) in [
        ("UPDATE", "client_registrations"),
        ("UPDATE", "client_scope_refreshes"),
    ] {
        let f = Fixture::new();
        let p = f.create(2, &["b"]);
        sql(
            &f,
            &format!(
                "CREATE TRIGGER fail BEFORE {verb} ON {table} BEGIN SELECT RAISE(ABORT,'synthetic'); END;"
            ),
        );
        let before = snapshot(&f);
        assert_eq!(
            f.store().stage_cli_scope_refresh(&p),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(snapshot(&f), before);
    }
    for (verb, table) in [
        ("UPDATE", "client_registrations"),
        ("UPDATE", "client_scope_refreshes"),
        ("DELETE", "client_registration_accounts"),
        ("INSERT", "client_registration_accounts"),
    ] {
        let f = Fixture::new();
        let p = f.create(2, &["b"]);
        let s = f.store().stage_cli_scope_refresh(&p).unwrap();
        sql(
            &f,
            &format!(
                "CREATE TRIGGER fail BEFORE {verb} ON {table} BEGIN SELECT RAISE(ABORT,'synthetic'); END;"
            ),
        );
        let before = snapshot(&f);
        assert_eq!(
            f.store().activate_cli_scope_refresh(&s),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(snapshot(&f), before);
    }
}
#[test]
fn cli15_refresh_server_change_and_account_removal_do_not_grant_scope() {
    let f = Fixture::new();
    let p = f.create(2, &["b"]);
    sql(
        &f,
        "UPDATE server_identity SET state='creating',public_key=NULL",
    );
    let before = snapshot(&f);
    assert_eq!(
        f.store().stage_cli_scope_refresh(&p),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(snapshot(&f), before);
    let f = Fixture::new();
    let p = f.create(2, &["b"]);
    sql(&f, "DELETE FROM accounts WHERE account_id='b'");
    let before = snapshot(&f);
    assert_eq!(
        f.store().stage_cli_scope_refresh(&p),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(snapshot(&f), before);
    let f = Fixture::new();
    let p = f.create(2, &["b"]);
    let s = f.store().stage_cli_scope_refresh(&p).unwrap();
    sql(&f, "DELETE FROM accounts WHERE account_id='b'");
    let before = snapshot(&f);
    assert_eq!(
        f.store().activate_cli_scope_refresh(&s),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(snapshot(&f), before);
}
#[test]
fn cli15_refresh_terminal_registration_reads_are_preserved_but_cannot_revive() {
    for state in ["suspended", "revoked"] {
        for staged in [false, true] {
            let f = Fixture::new();
            let p = f.create(2, &["b"]);
            let p = if staged {
                f.store().stage_cli_scope_refresh(&p).unwrap()
            } else {
                p
            };
            sql(
                &f,
                &format!(
                    "UPDATE client_registrations SET state='{state}',version=version+1,revoked_at_utc_ms={} ",
                    if state == "revoked" {
                        "updated_at_utc_ms"
                    } else {
                        "NULL"
                    }
                ),
            );
            let record = f
                .store()
                .load_registration(f.registration.registration_id)
                .unwrap()
                .unwrap();
            assert_eq!(
                record.state,
                if state == "revoked" {
                    RegistrationState::Revoked
                } else {
                    RegistrationState::Suspended
                }
            );
            let before = snapshot(&f);
            assert_eq!(
                f.store()
                    .create_cli_scope_refresh([3; 32], vec!["c".into()], pin()),
                Err(ErrorCode::Conflict)
            );
            if staged {
                assert_eq!(
                    f.store().activate_cli_scope_refresh(&p),
                    Err(ErrorCode::Conflict)
                );
            } else {
                assert_eq!(
                    f.store().stage_cli_scope_refresh(&p),
                    Err(ErrorCode::Conflict)
                );
            }
            assert_eq!(snapshot(&f), before);
        }
    }
}
#[test]
fn cli15_refresh_authentication_reader_enforces_default_scope_provenance() {
    for change in [
        "UPDATE client_registration_accounts SET account_id='b'",
        "UPDATE client_registrations SET profile_epoch=2",
        "UPDATE client_registration_tools SET tool_id='mail.send.execute'",
        "UPDATE client_registrations SET read_only=0",
    ] {
        let f = Fixture::new();
        sql(&f, change);
        assert_eq!(
            f.store().load_registration(f.registration.registration_id),
            Err(ErrorCode::InternalError)
        );
        assert!(f.store().with_connection(validate_history).is_err());
    }
    let f = Fixture::new();
    let p = f.create(2, &["b"]);
    let s = f.store().stage_cli_scope_refresh(&p).unwrap();
    f.store().activate_cli_scope_refresh(&s).unwrap();
    sql(&f, "DELETE FROM client_scope_refreshes");
    assert_eq!(
        f.store().load_registration(f.registration.registration_id),
        Err(ErrorCode::InternalError)
    );
}
#[test]
fn cli15_refresh_corrupt_hash_nullable_clock_and_policy_history_fail_closed() {
    for change in [
        "UPDATE client_scope_refreshes SET payload_hash=zeroblob(32)",
        "UPDATE client_scope_refreshes SET decided_at_utc_ms=created_at_utc_ms",
        "UPDATE client_scope_refreshes SET version=2",
        "UPDATE client_scope_refreshes SET updated_at_utc_ms=updated_at_utc_ms+1",
        "UPDATE client_scope_refreshes SET base_accounts_json='[\"b\"]'",
        "UPDATE client_scope_refreshes SET target_accounts_json='[\"b\",\"a\"]'",
        "UPDATE client_scope_refreshes SET target_profile_epoch=3",
    ] {
        let f = Fixture::new();
        f.create(2, &["b"]);
        sql(&f, "PRAGMA ignore_check_constraints=ON");
        sql(&f, change);
        assert_eq!(
            f.store().load_cli_scope_refresh([2; 32]),
            Err(ErrorCode::InternalError),
            "{change}"
        );
        assert!(f.store().with_connection(validate_history).is_err());
    }
}
#[test]
fn cli15_refresh_clock_rollback_is_not_an_approval_and_late_activation_is_allowed() {
    let f = Fixture::new();
    let p = f.create(2, &["b"]);
    let p = shift(&f, &p, 400000);
    let before = snapshot(&f);
    assert_eq!(
        f.store().stage_cli_scope_refresh(&p),
        Err(ErrorCode::ApprovalExpired)
    );
    assert_eq!(snapshot(&f), before);
    let f = Fixture::new();
    let p = f.create(2, &["b"]);
    let s = f.store().stage_cli_scope_refresh(&p).unwrap();
    let s = shift(&f, &s, -400000);
    f.store().activate_cli_scope_refresh(&s).unwrap();
}
#[test]
fn cli15_refresh_generic_historical_registration_policy_is_unchanged() {
    let f = Fixture::new();
    sql(
        &f,
        "UPDATE client_registration_requests SET display_label='generic fixture'; UPDATE client_registrations SET display_label='generic fixture',read_only=0,requests_per_minute=80; UPDATE client_registration_tools SET tool_id='mail.send.execute';",
    );
    let record = f
        .store()
        .load_registration(f.registration.registration_id)
        .unwrap()
        .unwrap();
    assert!(!record.read_only);
    assert_eq!(record.requests_per_minute, 80);
    assert_eq!(record.scope.tools(), &["mail.send.execute"]);
    f.store().with_connection(validate_history).unwrap();
}

fn insert_snapshot(
    connection: &rusqlite::Connection,
    row: &CliScopeRefresh,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO client_scope_refreshes VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
        params![row.refresh_id.as_slice(),row.base_registration.registration_id.as_slice(),row.base_registration.version as i64,row.base_registration.profile_epoch as i64,row.target_profile_epoch() as i64,serde_json::to_string(row.base_registration.scope.accounts()).unwrap(),serde_json::to_string(&row.target_accounts).unwrap(),row.client_public_key_hash.as_slice(),row.server_public_key_hash.as_slice(),row.payload_hash.as_slice(),match row.phase {ScopeRefreshPhase::Pending=>"pending",ScopeRefreshPhase::Staged=>"staged",ScopeRefreshPhase::Active=>"active",ScopeRefreshPhase::Denied=>"denied",ScopeRefreshPhase::Expired=>"expired"},row.version as i64,row.created_at_utc_ms,row.expires_at_utc_ms,row.updated_at_utc_ms,row.decided_at_utc_ms,row.activated_at_utc_ms,i64::from(row.hash_version),serde_json::to_string(row.base_registration.scope.tools()).unwrap(),serde_json::to_string(&row.target_tools).unwrap()],
    )?;
    Ok(())
}

#[test]
fn cli15_refresh_retained_quota_and_duplicate_pending_or_staged_are_bounded() {
    let f = Fixture::new();
    let p = f.create(2, &["b"]);
    let denied = f.store().deny_cli_scope_refresh(&p).unwrap();
    f.store()
        .with_connection(|c| {
            let tx =
                rusqlite::Transaction::new_unchecked(c, rusqlite::TransactionBehavior::Immediate)?;
            for n in 1_u32..1024 {
                let mut row = denied.clone();
                row.refresh_id = [0; 32];
                row.refresh_id[..4].copy_from_slice(&n.to_be_bytes());
                row.payload_hash = independent_hash(&row);
                insert_snapshot(&tx, &row)?;
            }
            tx.commit()
        })
        .unwrap();
    let before = snapshot(&f);
    assert_eq!(
        f.store()
            .create_cli_scope_refresh([8; 32], vec!["b".into()], pin()),
        Err(ErrorCode::QuotaExceeded)
    );
    assert_eq!(snapshot(&f), before);
    assert_eq!(
        f.store().load_cli_scope_refresh([2; 32]).unwrap(),
        Some(denied.clone())
    );
    let mut overflow = denied;
    overflow.refresh_id = [8; 32];
    overflow.payload_hash = independent_hash(&overflow);
    f.store()
        .with_connection(|c| insert_snapshot(c, &overflow))
        .unwrap();
    assert_eq!(
        f.store().load_cli_scope_refresh([2; 32]),
        Err(ErrorCode::QuotaExceeded)
    );
    assert!(f.store().with_connection(validate_history).is_err());
    for staged in [false, true] {
        let f = Fixture::new();
        let p = f.create(2, &["b"]);
        let mut duplicate = if staged {
            f.store().stage_cli_scope_refresh(&p).unwrap()
        } else {
            p
        };
        sql(
            &f,
            "DROP INDEX client_scope_refresh_pending_uq; DROP INDEX client_scope_refresh_epoch_uq;",
        );
        duplicate.refresh_id = [3; 32];
        duplicate.payload_hash = independent_hash(&duplicate);
        f.store()
            .with_connection(|c| insert_snapshot(c, &duplicate))
            .unwrap();
        if staged {
            assert_eq!(
                f.store().list_staged_cli_scope_refreshes(),
                Err(ErrorCode::Conflict)
            );
        } else {
            assert_eq!(
                f.store().load_pending_cli_scope_refresh(),
                Err(ErrorCode::Conflict)
            );
        }
        assert!(f.store().with_connection(validate_history).is_err());
    }
}

#[test]
fn cli15_refresh_cas_boundaries_do_not_wrap_and_client_key_tampering_fails_closed() {
    for version in [i64::MAX - 1, i64::MAX] {
        let f = Fixture::new();
        f.store()
            .with_connection(|c| {
                c.execute("UPDATE client_registrations SET version=?1", [version])?;
                Ok(())
            })
            .unwrap();
        let before = snapshot(&f);
        assert_eq!(
            f.store()
                .create_cli_scope_refresh([2; 32], vec!["b".into()], pin()),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(snapshot(&f), before);
    }
    let f = Fixture::new();
    sql(
        &f,
        "UPDATE client_registrations SET version=9223372036854775805",
    );
    let p = f.create(2, &["b"]);
    let s = f.store().stage_cli_scope_refresh(&p).unwrap();
    let a = f.store().activate_cli_scope_refresh(&s).unwrap();
    assert_eq!(a.version, i64::MAX as u64);
    assert_eq!(
        f.store()
            .create_cli_scope_refresh([3; 32], vec!["c".into()], pin()),
        Err(ErrorCode::Conflict)
    );
    let f = Fixture::new();
    let p = f.create(2, &["b"]);
    let mut changed = public_key();
    changed[40..].copy_from_slice(
        &wire_hex::decode_fixed::<32>(
            "b01cbd1c01e58065711814b583f061e9d431cca994cea1313449bf97c840ae0a",
        )
        .unwrap(),
    );
    hantong_platform::validate_public_key(&changed).unwrap();
    f.store()
        .with_connection(|c| {
            c.execute(
                "UPDATE client_registrations SET public_key=?1",
                [changed.as_slice()],
            )?;
            Ok(())
        })
        .unwrap();
    let before = snapshot(&f);
    assert_eq!(
        f.store().stage_cli_scope_refresh(&p),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(snapshot(&f), before);
    let f = Fixture::new();
    let p = f.create(2, &["b"]);
    f.store()
        .with_connection(|c| {
            c.execute(
                "UPDATE server_identity SET public_key=?1",
                [changed.as_slice()],
            )?;
            Ok(())
        })
        .unwrap();
    let before = snapshot(&f);
    assert_eq!(
        f.store().stage_cli_scope_refresh(&p),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(snapshot(&f), before);
}

#[test]
fn cli15_refresh_valid_hash_cannot_replace_successful_scope_history() {
    let f = Fixture::new();
    let p = f.create(2, &["b"]);
    let s = f.store().stage_cli_scope_refresh(&p).unwrap();
    f.store().activate_cli_scope_refresh(&s).unwrap();
    let mut p = f.create(3, &["a"]);
    p.base_registration.scope = scope(&["c"]);
    p.payload_hash = independent_hash(&p);
    f.store().with_connection(|c|{c.execute("UPDATE client_scope_refreshes SET base_accounts_json='[\"c\"]',payload_hash=?2 WHERE refresh_id=?1",params![p.refresh_id.as_slice(),p.payload_hash.as_slice()])?;Ok(())}).unwrap();
    assert_eq!(
        f.store().load_cli_scope_refresh([3; 32]),
        Err(ErrorCode::InternalError)
    );
    assert!(f.store().with_connection(validate_history).is_err());
    let f = Fixture::new();
    for id in [2, 3] {
        let p = f.create(id, if id == 2 { &["b"] } else { &["c"] });
        let s = f.store().stage_cli_scope_refresh(&p).unwrap();
        f.store().activate_cli_scope_refresh(&s).unwrap();
    }
    f.store()
        .with_connection(|c| {
            c.execute(
                "DELETE FROM client_scope_refreshes WHERE refresh_id=?1",
                [[2_u8; 32].as_slice()],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.store().load_registration(f.registration.registration_id),
        Err(ErrorCode::InternalError)
    );
}

#[test]
fn sec03_scope_v2_rejects_unreviewed_tool_grants_at_initial_and_staged_epochs() {
    for staged in [false, true] {
        let f = Fixture::new();
        if staged {
            let p = f.create(91, &["b"]);
            f.store().stage_cli_scope_refresh(&p).unwrap();
        }
        f.store().with_connection(|c| {
            for tool in ["mail.folders.list","mail.folders.refresh"] {
                c.execute("INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,?2)",params![f.registration.registration_id.as_slice(),tool])?;
            }
            Ok(())
        }).unwrap();
        assert_eq!(
            f.store().load_registration(f.registration.registration_id),
            Err(ErrorCode::InternalError)
        );
    }
}

#[test]
fn cli15_v8_real_legacy_history_migrates_without_rehash_or_scope_change() {
    for phase in [
        ScopeRefreshPhase::Pending,
        ScopeRefreshPhase::Staged,
        ScopeRefreshPhase::Active,
    ] {
        let mut f = Fixture::new();
        let mut old = f.create(92, &["b"]);
        old.hash_version = 1;
        old.payload_hash = independent_hash(&old);
        f.store()
            .with_connection(|c| {
                c.execute(
                    "UPDATE client_scope_refreshes SET hash_version=1,payload_hash=?1",
                    [old.payload_hash.as_slice()],
                )?;
                Ok(())
            })
            .unwrap();
        if phase != ScopeRefreshPhase::Pending {
            old = f.store().stage_cli_scope_refresh(&old).unwrap();
        }
        if phase == ScopeRefreshPhase::Active {
            f.store().activate_cli_scope_refresh(&old).unwrap();
            old = f.store().load_cli_scope_refresh([92; 32]).unwrap().unwrap();
        }
        let before = f
            .store()
            .load_registration(f.registration.registration_id)
            .unwrap();
        restore_v8_scope_schema(&f);
        f.reopen();
        assert_eq!(
            f.store().load_cli_scope_refresh([92; 32]).unwrap(),
            Some(old.clone())
        );
        assert_eq!(
            f.store()
                .load_registration(f.registration.registration_id)
                .unwrap(),
            before
        );
        assert!(f.root.join("data/backup-before-v8.db").is_file());
        if phase == ScopeRefreshPhase::Active {
            let next = f.create_scope(
                93,
                &["b"],
                hantong_contracts::auth::default_cli_folder_tools(),
            );
            assert_eq!(next.hash_version, 5);
            let next = f.store().stage_cli_scope_refresh(&next).unwrap();
            f.store().activate_cli_scope_refresh(&next).unwrap();
            assert_eq!(
                f.store().load_cli_scope_refresh([92; 32]).unwrap(),
                Some(old)
            );
        }
    }
}

fn restore_v8_scope_schema(f: &Fixture) {
    f.store().with_connection(|c| {
            let tx=rusqlite::Transaction::new_unchecked(c,rusqlite::TransactionBehavior::Immediate)?;
            let columns="refresh_id,registration_id,base_registration_version,base_profile_epoch,target_profile_epoch,base_accounts_json,target_accounts_json,client_public_key_hash,server_public_key_hash,payload_hash,phase,version,created_at_utc_ms,expires_at_utc_ms,updated_at_utc_ms,decided_at_utc_ms,activated_at_utc_ms";
            tx.execute_batch(&format!("CREATE TEMP TABLE saved_v1 AS SELECT {columns} FROM client_scope_refreshes; DROP TABLE client_scope_refreshes;"))?;
            let schema=include_str!("../../../../docs/contracts/migrations/schema-v8.sql");
            // This fixture has no audit jobs: restore the complete v8 audit DDL,
            // rather than leaving v10 tables behind under an old version number.
            assert_eq!(tx.query_row("SELECT count(*) FROM audit_starts",[],|r|r.get::<_,i64>(0))?,0);
            assert_eq!(tx.query_row("SELECT count(*) FROM folder_refresh_requests",[],|r|r.get::<_,i64>(0))?,0);
            assert_eq!(tx.query_row("SELECT (SELECT count(*) FROM imap_uid_messages)+(SELECT count(*) FROM imap_uid_cursors)+(SELECT count(*) FROM imap_uid_page_operations)",[],|r|r.get::<_,i64>(0))?,0);
            assert_eq!(tx.query_row("SELECT (SELECT count(*) FROM imap_initial_preview_requests)+(SELECT count(*) FROM imap_initial_preview_plans)+(SELECT count(*) FROM imap_initial_preview_pages)+(SELECT count(*) FROM imap_initial_preview_messages)",[],|r|r.get::<_,i64>(0))?,0);
            tx.execute_batch("DROP TABLE imap_initial_preview_messages; DROP TABLE imap_initial_preview_pages; DROP TABLE imap_initial_preview_plans; DROP TABLE imap_initial_preview_requests;")?;
            tx.execute_batch("DROP TABLE imap_message_identities; DROP TABLE imap_uid_reconcile_messages; DROP TABLE imap_uid_reconcile_heads; DROP TABLE imap_uid_reconcile_requests; DROP TABLE imap_uid_epochs; DROP TABLE imap_uid_recovery_requests; DROP TABLE imap_uid_page_requests; DROP TABLE imap_uid_messages; DROP TABLE imap_uid_cursors; DROP TABLE imap_uid_page_operations;")?;
            tx.execute_batch("DROP TABLE folder_refresh_requests; DROP TABLE audit_terminals; DROP TABLE audit_start_accounts; DROP TABLE audit_starts;")?;
            let audit_start=schema.find("CREATE TABLE audit_starts (").unwrap();
            let audit_end=schema.find("-- Provider-neutral IMAP/SMTP").unwrap();
            tx.execute_batch(&schema[audit_start..audit_end])?;
            let start=schema.find("CREATE TABLE client_scope_refreshes (").unwrap();
            let end=schema[start..].find("-- Folder observations only;").unwrap()+start;
            tx.execute_batch(&schema[start..end])?;
            tx.execute_batch(&format!("INSERT INTO client_scope_refreshes ({columns}) SELECT {columns} FROM saved_v1; DROP TABLE saved_v1; UPDATE app_metadata SET schema_version=8; PRAGMA user_version=8;"))?;
            tx.commit()
        }).unwrap();
}

#[test]
fn sec03_v8_legacy_invalid_provenance_is_rejected_before_backup_or_migration() {
    for fault in 0..4 {
        let mut f = Fixture::new();
        let mut old = f.create(94, &["b"]);
        old.hash_version = 1;
        if fault == 0 {
            old.base_registration.scope = scope(&["c"]);
        }
        old.payload_hash = independent_hash(&old);
        f.store().with_connection(|c| {
            c.execute("UPDATE client_scope_refreshes SET hash_version=1,base_accounts_json=?1,payload_hash=?2",params![serde_json::to_string(old.base_registration.scope.accounts()).unwrap(),old.payload_hash.as_slice()])?;
            match fault {
                1=>{c.execute("INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,'mail.folders.list')",[f.registration.registration_id.as_slice()])?;},
                2=>{c.execute("UPDATE client_registrations SET display_label='nondefault'",[])?;},
                3=>{c.execute("UPDATE client_scope_refreshes SET payload_hash=?1",[[99_u8;32].as_slice()])?;},
                _=>{}
            }
            Ok(())
        }).unwrap();
        restore_v8_scope_schema(&f);
        drop(f.store.take());
        assert!(matches!(
            Store::open(f.root.join("data"), f.root.join("keys")),
            Err(ErrorCode::InternalError)
        ));
        assert!(!f.root.join("data/backup-before-v8.db").exists());
    }
}
