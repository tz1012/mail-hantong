#![cfg(windows)]

use super::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: PathBuf,
    store: Option<Store>,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-accounts-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        Self {
            root,
            store: Some(store),
        }
    }
    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }
    fn insert(&self, id: &str, number: i64, status: &str, caps: &str) {
        self.store().with_connection(|c| c.execute(
            "INSERT INTO accounts(account_id,provider,provider_subject,primary_address,display_name,color,account_number,status,capabilities,granted_scopes,settings_version) VALUES(?1,'imap',NULL,?2,?3,?4,?5,?6,?7,'[]',1)",
            rusqlite::params![id, format!("{id}@example.test"), format!("{id} name"), "#123456", number, status, caps],
        )).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.store.take();
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}

#[test]
fn mail01_reads_only_requested_accounts_in_stable_order_and_keeps_reauth_rows() {
    let fixture = Fixture::new();
    fixture.insert("account-b", 2, "ready", r#"["mail.send","mail.read"]"#);
    fixture.insert("account-a", 1, "reauth_required", r#"["mail.read"]"#);
    fixture.insert("account-c", 3, "offline", r#"[]"#);
    fixture.insert("account-z", 2, "ready", r#"[]"#);
    let snapshot = fixture
        .store()
        .list_account_summaries(&ids(&["account-b", "account-a"]), true)
        .unwrap();
    assert_eq!(
        snapshot
            .items
            .iter()
            .map(|v| v.account_id.as_str())
            .collect::<Vec<_>>(),
        ["account-a", "account-b"]
    );
    assert_eq!(snapshot.items[0].status, AccountStatus::ReauthRequired);
    assert_eq!(
        snapshot.items[1].capabilities.as_ref().unwrap(),
        &["mail.read".to_owned(), "mail.send".to_owned()]
    );
}

#[test]
fn mail01_false_capabilities_does_not_decode_corrupt_json() {
    let fixture = Fixture::new();
    fixture.insert("account-a", 1, "ready", r#"["mail.read"]"#);
    fixture.store().with_connection(|c| c.execute("UPDATE accounts SET capabilities='[\"mail.read\",\"mail.read\"]' WHERE account_id='account-a'", [])).unwrap();
    let snapshot = fixture
        .store()
        .list_account_summaries(&ids(&["account-a"]), false)
        .unwrap();
    assert_eq!(snapshot.items[0].capabilities, None);
    assert_eq!(
        fixture
            .store()
            .list_account_summaries(&ids(&["account-a"]), true),
        Err(ErrorCode::InternalError)
    );
}

#[test]
fn mail01_missing_or_invalid_scope_is_atomic_and_never_means_all() {
    let fixture = Fixture::new();
    fixture.insert("account-a", 1, "ready", r#"[]"#);
    assert_eq!(
        fixture
            .store()
            .list_account_summaries(&ids(&["account-a", "missing"]), false),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(
        fixture.store().list_account_summaries(&[], false),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        fixture
            .store()
            .list_account_summaries(&ids(&["account-a", "account-a"]), false),
        Err(ErrorCode::InvalidInput)
    );
    for invalid in ["한글", "bad id", "bad\n"] {
        assert_eq!(
            fixture
                .store()
                .list_account_summaries(&ids(&[invalid]), false),
            Err(ErrorCode::InvalidInput)
        );
    }
    let long = "a".repeat(129);
    assert_eq!(
        fixture
            .store()
            .list_account_summaries(&ids(&[&long]), false),
        Err(ErrorCode::InvalidInput)
    );
    let too_many = (0..129).map(|i| format!("a{i}")).collect::<Vec<_>>();
    assert_eq!(
        fixture.store().list_account_summaries(&too_many, false),
        Err(ErrorCode::InvalidInput)
    );
}

#[test]
fn mail01_maximum_scope_and_capability_bounds_are_enforced_without_echo() {
    let fixture = Fixture::new();
    let caps = (0..128).map(|i| format!("cap{i:03}")).collect::<Vec<_>>();
    fixture.insert("a", 1, "ready", &serde_json::to_string(&caps).unwrap());
    let one = fixture
        .store()
        .list_account_summaries(&ids(&["a"]), true)
        .unwrap();
    assert_eq!(one.items[0].capabilities.as_ref().unwrap().len(), 128);
    let max_ids = (0..128).map(|i| format!("account-{i}")).collect::<Vec<_>>();
    for (i, id) in max_ids.iter().enumerate() {
        fixture.insert(id, i as i64 + 2, "ready", "[]");
    }
    assert_eq!(
        fixture
            .store()
            .list_account_summaries(&max_ids, false)
            .unwrap()
            .items
            .len(),
        128
    );
    fixture.store().with_connection(|c| c.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE accounts SET capabilities='[\"a\",\"a\"]' WHERE account_id='a'")).unwrap();
    assert_eq!(
        fixture.store().list_account_summaries(&ids(&["a"]), true),
        Err(ErrorCode::InternalError)
    );
}

#[test]
fn mail01_corrupt_display_columns_fail_closed_and_false_capability_read_stays_safe() {
    let mutations = [
        "UPDATE accounts SET provider='unknown' WHERE account_id='account-a'",
        "UPDATE accounts SET status='unknown' WHERE account_id='account-a'",
        "UPDATE accounts SET account_number=-1 WHERE account_id='account-a'",
        "UPDATE accounts SET primary_address=char(10) WHERE account_id='account-a'",
        "UPDATE accounts SET display_name=char(10) WHERE account_id='account-a'",
        "UPDATE accounts SET color='blue' WHERE account_id='account-a'",
    ];
    for mutation in mutations {
        let fixture = Fixture::new();
        fixture.insert("account-a", 1, "ready", "[]");
        fixture
            .store()
            .with_connection(|c| c.execute_batch("PRAGMA ignore_check_constraints=ON"))
            .unwrap();
        fixture
            .store()
            .with_connection(|c| c.execute(mutation, []))
            .unwrap();
        assert_eq!(
            fixture
                .store()
                .list_account_summaries(&ids(&["account-a"]), false),
            Err(ErrorCode::InternalError)
        );
    }
    let fixture = Fixture::new();
    fixture.insert("account-a", 1, "ready", "[]");
    fixture.store().with_connection(|c| c.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE accounts SET capabilities='not-json' WHERE account_id='account-a'")).unwrap();
    assert_eq!(
        fixture
            .store()
            .list_account_summaries(&ids(&["account-a"]), true),
        Err(ErrorCode::InternalError)
    );
    assert!(
        fixture
            .store()
            .list_account_summaries(&ids(&["account-a"]), false)
            .is_ok()
    );
}

#[test]
fn mail01_reads_do_not_change_total_changes_or_debug_echo() {
    let fixture = Fixture::new();
    fixture.insert("account-a", 1, "ready", "[]");
    let before: i64 = fixture
        .store()
        .with_connection(|c| c.query_row("SELECT total_changes()", [], |r| r.get(0)))
        .unwrap();
    let snapshot = fixture
        .store()
        .list_account_summaries(&ids(&["account-a"]), false)
        .unwrap();
    let after: i64 = fixture
        .store()
        .with_connection(|c| c.query_row("SELECT total_changes()", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(before, after);
    let debug = format!("{snapshot:?}");
    assert!(!debug.contains("account-a"));
    assert!(!debug.contains("example.test"));
}

#[test]
fn mail01_reopen_preserves_rows_and_read_does_not_write() {
    let mut fixture = Fixture::new();
    fixture.insert("account-a", 1, "disabled", r#"[]"#);
    let before: i64 = fixture
        .store()
        .with_connection(|c| c.query_row("SELECT count(*) FROM accounts", [], |r| r.get(0)))
        .unwrap();
    fixture
        .store()
        .list_account_summaries(&ids(&["account-a"]), false)
        .unwrap();
    let after: i64 = fixture
        .store()
        .with_connection(|c| c.query_row("SELECT count(*) FROM accounts", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(before, after);
    drop(fixture.store.take());
    fixture.store =
        Some(Store::open(fixture.root.join("data"), fixture.root.join("keys")).unwrap());
    assert_eq!(
        fixture
            .store()
            .list_account_summaries(&ids(&["account-a"]), false)
            .unwrap()
            .items[0]
            .status,
        AccountStatus::Disabled
    );
}
