use crate::{
    Store,
    account_provisioning::{InitialImapProbeOutcome, imap_account_stage_payload_hash},
    imap_catalog::imap_catalog_payload_hash,
    mail_connection::MailMutation,
};
use hantong_contracts::ErrorCode;
use hantong_domain::{
    account_provisioning::NewImapAccountDraft,
    capabilities::ImapCapabilities,
    mail_connection::{ImapEndpointConfig, MailConnectionConfig},
    mailboxes::{
        ImapMailboxDescriptor, ImapMailboxName, ImapMailboxSnapshot, MailboxChildren,
        MailboxEncoding,
    },
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hantong-catalog-security-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn open(&self) -> Store {
        Store::open(self.0.join("data"), self.0.join("keys")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn endpoint(account: &str) -> ImapEndpointConfig {
    let json = serde_json::json!({"schema_version":1,"account_id":account,"imap":{"settings_version":1,"host":"imap.example.test","port":993,"tls":"implicit_tls","login_name":account,"authentication":"app_password"}});
    MailConnectionConfig::from_json(&serde_json::to_vec(&json).unwrap())
        .unwrap()
        .imap()
        .clone()
}
fn seed(store: &Store, account: &str) -> ImapEndpointConfig {
    let endpoint = endpoint(account);
    let draft = NewImapAccountDraft::new(account.into(), None, None).unwrap();
    let mutation = MailMutation::new(
        format!("stage-{account}"),
        format!("key-{account}"),
        0,
        imap_account_stage_payload_hash(&draft, &endpoint).unwrap(),
    )
    .unwrap();
    store
        .stage_imap_account(&draft, &endpoint, &mutation)
        .unwrap();
    assert_eq!(
        store
            .finish_initial_imap_probe(account, 1, 1, 0, &InitialImapProbeOutcome::Ready(vec![]))
            .unwrap()
            .account_version,
        2
    );
    endpoint
}
fn snapshot(account: &str, name: &str) -> ImapMailboxSnapshot {
    ImapMailboxSnapshot::new(
        account,
        1,
        ImapCapabilities::from_features([]),
        vec![
            ImapMailboxDescriptor::new(
                ImapMailboxName::from_wire(name.as_bytes(), MailboxEncoding::Utf8).unwrap(),
                None,
                false,
                false,
                MailboxChildren::Unknown,
                vec![],
            )
            .unwrap(),
        ],
    )
    .unwrap()
}
fn mutation(
    endpoint: &ImapEndpointConfig,
    snapshot: &ImapMailboxSnapshot,
    version: u64,
    id: &str,
    key: &str,
) -> MailMutation {
    MailMutation::new(
        id.into(),
        key.into(),
        version,
        imap_catalog_payload_hash(2, endpoint, snapshot, version).unwrap(),
    )
    .unwrap()
}
fn apply(
    store: &Store,
    endpoint: &ImapEndpointConfig,
    snapshot: &ImapMailboxSnapshot,
    version: u64,
    id: &str,
) -> u64 {
    store
        .replace_imap_mailbox_snapshot(
            2,
            endpoint,
            snapshot,
            &mutation(endpoint, snapshot, version, id, id),
        )
        .unwrap()
}
fn count(store: &Store) -> i64 {
    store
        .with_connection(|c| {
            c.query_row(
                "SELECT count(*) FROM imap_mailbox_catalog_operations",
                [],
                |r| r.get(0),
            )
        })
        .unwrap()
}

#[test]
fn ext06_catalog_same_version_endpoint_substitution_is_rejected_without_writes() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let endpoint = seed(&store, "acct");
    let snapshot = snapshot("acct", "INBOX");
    for (field, value) in [
        ("host", serde_json::json!("different.example.test")),
        ("port", serde_json::json!(994)),
        ("tls", serde_json::json!("starttls_required")),
        ("login_name", serde_json::json!("other-user")),
        ("authentication", serde_json::json!("oauth2")),
    ] {
        let mut changed = serde_json::to_value(&endpoint).unwrap();
        changed[field] = value;
        let config = serde_json::json!({"schema_version":1,"account_id":"acct","imap":changed});
        let altered =
            MailConnectionConfig::from_json(&serde_json::to_vec(&config).unwrap()).unwrap();
        assert_eq!(
            store.replace_imap_mailbox_snapshot(
                2,
                altered.imap(),
                &snapshot,
                &mutation(altered.imap(), &snapshot, 0, "bad-op", "bad-key")
            ),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(count(&store), 0);
        assert!(store.load_imap_mailbox_snapshot("acct").unwrap().is_none());
    }
}

#[test]
fn ext06_catalog_replay_collisions_and_old_generation_never_restore_old_content() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let endpoint = seed(&store, "acct");
    let old = snapshot("acct", "old");
    let new = snapshot("acct", "new");
    assert_eq!(apply(&store, &endpoint, &old, 0, "op-one"), 1);
    assert_eq!(
        store.replace_imap_mailbox_snapshot(
            2,
            &endpoint,
            &new,
            &mutation(&endpoint, &new, 0, "stale", "stale")
        ),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(apply(&store, &endpoint, &new, 1, "op-two"), 2);
    assert_eq!(apply(&store, &endpoint, &old, 0, "op-one"), 1);
    for (id, key) in [
        ("other-id", "op-one"),
        ("op-one", "other-key"),
        ("op-one", "op-two"),
    ] {
        assert_eq!(
            store.replace_imap_mailbox_snapshot(
                2,
                &endpoint,
                &old,
                &mutation(&endpoint, &old, 0, id, key)
            ),
            Err(ErrorCode::Conflict)
        );
    }
    let other = seed(&store, "other");
    let other_snapshot = snapshot("other", "private");
    assert_eq!(
        store.replace_imap_mailbox_snapshot(
            2,
            &other,
            &other_snapshot,
            &mutation(&other, &other_snapshot, 0, "op-one", "op-one")
        ),
        Err(ErrorCode::Conflict)
    );
    let loaded = store.load_imap_mailbox_snapshot("acct").unwrap().unwrap();
    assert_eq!(loaded.generation(), 2);
    assert_eq!(loaded.snapshot(), &new);
    assert_eq!(count(&store), 2);
}

#[test]
fn ext06_catalog_state_changes_invalidate_binding_and_unknown_state_is_corruption() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let endpoint = seed(&store, "acct");
    let snapshot = snapshot("acct", "INBOX");
    apply(&store, &endpoint, &snapshot, 0, "op");
    for state in [
        "disabled",
        "connecting",
        "reauth_required",
        "offline",
        "error",
    ] {
        store
            .with_connection(|c| {
                c.execute(
                    "UPDATE accounts SET status=?1 WHERE account_id='acct'",
                    [state],
                )
            })
            .unwrap();
        assert!(
            !store
                .load_imap_mailbox_snapshot("acct")
                .unwrap()
                .unwrap()
                .binding_current()
        );
        assert_eq!(
            store.replace_imap_mailbox_snapshot(
                2,
                &endpoint,
                &snapshot,
                &mutation(&endpoint, &snapshot, 1, "next", "next")
            ),
            Err(ErrorCode::Conflict)
        );
    }
    store
        .with_connection(|c| {
            c.execute(
                "UPDATE accounts SET status='unknown' WHERE account_id='acct'",
                [],
            )
        })
        .unwrap();
    assert_eq!(
        store.load_imap_mailbox_snapshot("acct"),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        store.replace_imap_mailbox_snapshot(
            2,
            &endpoint,
            &snapshot,
            &mutation(&endpoint, &snapshot, 1, "next", "next")
        ),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(count(&store), 1);
}

#[test]
fn ext06_catalog_corrupt_header_or_last_operation_cannot_be_read_or_overwritten() {
    for sql in [
        "UPDATE imap_mailbox_catalogs SET endpoint_version=2",
        "UPDATE imap_mailbox_catalogs SET generation=7",
        "UPDATE imap_mailbox_catalogs SET endpoint_hash=zeroblob(32)",
        "UPDATE imap_mailbox_catalogs SET snapshot_hash=zeroblob(32)",
        "UPDATE imap_mailbox_catalogs SET stored_at_utc_ms=stored_at_utc_ms+1",
        "UPDATE imap_mailbox_catalog_operations SET payload_hash=zeroblob(32)",
        "UPDATE imap_mailbox_catalog_operations SET account_version=3",
    ] {
        let fixture = Fixture::new();
        let store = fixture.open();
        let endpoint = seed(&store, "acct");
        let snapshot = snapshot("acct", "INBOX");
        apply(&store, &endpoint, &snapshot, 0, "op");
        store.with_connection(|c| c.execute_batch(sql)).unwrap();
        assert_eq!(
            store.load_imap_mailbox_snapshot("acct"),
            Err(ErrorCode::InternalError),
            "{sql}"
        );
        assert_eq!(
            store.replace_imap_mailbox_snapshot(
                2,
                &endpoint,
                &snapshot,
                &mutation(&endpoint, &snapshot, 1, "next", "next")
            ),
            Err(ErrorCode::InternalError),
            "{sql}"
        );
        assert_eq!(count(&store), 1);
    }
}

#[test]
fn ext06_catalog_update_abort_and_clock_rollback_preserve_previous_catalog_and_operation() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let endpoint = seed(&store, "acct");
    let old = snapshot("acct", "old");
    let new = snapshot("acct", "new");
    apply(&store, &endpoint, &old, 0, "op");
    store.with_connection(|c|c.execute_batch("CREATE TRIGGER stop_catalog BEFORE UPDATE ON imap_mailbox_catalogs BEGIN SELECT RAISE(ABORT,'fixture'); END")).unwrap();
    assert_eq!(
        store.replace_imap_mailbox_snapshot(
            2,
            &endpoint,
            &new,
            &mutation(&endpoint, &new, 1, "next", "next")
        ),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        store
            .load_imap_mailbox_snapshot("acct")
            .unwrap()
            .unwrap()
            .snapshot(),
        &old
    );
    assert_eq!(count(&store), 1);
    store.with_connection(|c|c.execute_batch("DROP TRIGGER stop_catalog; UPDATE imap_mailbox_catalogs SET stored_at_utc_ms=9223372036854775807; UPDATE imap_mailbox_catalog_operations SET completed_at_utc_ms=9223372036854775807;")).unwrap();
    assert_eq!(
        store.replace_imap_mailbox_snapshot(
            2,
            &endpoint,
            &new,
            &mutation(&endpoint, &new, 1, "next", "next")
        ),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(count(&store), 1);
}

#[test]
fn ext06_catalog_global_quota_rejects_new_work_but_preserves_exact_replay() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let endpoint = seed(&store, "acct");
    let snapshot = snapshot("acct", "INBOX");
    apply(&store, &endpoint, &snapshot, 0, "op");
    store.with_connection(|c|c.execute_batch("WITH RECURSIVE ids(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM ids WHERE n<99999) INSERT INTO imap_mailbox_catalog_operations(operation_id,account_id,idempotency_key,account_version,endpoint_version,endpoint_hash,snapshot_hash,expected_version,result_version,payload_hash,completed_at_utc_ms) SELECT 'quota-'||n,account_id,'quota-'||n,account_version,endpoint_version,endpoint_hash,snapshot_hash,expected_version,result_version,payload_hash,completed_at_utc_ms FROM ids CROSS JOIN imap_mailbox_catalog_operations WHERE operation_id='op'")).unwrap();
    assert_eq!(count(&store), 100_000);
    assert_eq!(
        store.replace_imap_mailbox_snapshot(
            2,
            &endpoint,
            &snapshot,
            &mutation(&endpoint, &snapshot, 1, "next", "next")
        ),
        Err(ErrorCode::QuotaExceeded)
    );
    assert_eq!(apply(&store, &endpoint, &snapshot, 0, "op"), 1);
    assert_eq!(
        store
            .load_imap_mailbox_snapshot("acct")
            .unwrap()
            .unwrap()
            .generation(),
        1
    );
    assert_eq!(count(&store), 100_000);
}

#[test]
fn ext06_catalog_commit_failure_rolls_back_snapshot_and_operation_together() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let endpoint = seed(&store, "acct");
    let old = snapshot("acct", "old");
    let new = snapshot("acct", "new");
    apply(&store, &endpoint, &old, 0, "op");
    store.with_connection(|c|c.execute_batch("CREATE TRIGGER fail_catalog_commit AFTER UPDATE ON imap_mailbox_catalogs BEGIN INSERT INTO client_registration_accounts(registration_id,account_id) VALUES(zeroblob(16),'acct'); END; PRAGMA defer_foreign_keys=ON;")).unwrap();
    assert_eq!(
        store.replace_imap_mailbox_snapshot(
            2,
            &endpoint,
            &new,
            &mutation(&endpoint, &new, 1, "next", "next")
        ),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        store
            .load_imap_mailbox_snapshot("acct")
            .unwrap()
            .unwrap()
            .snapshot(),
        &old
    );
    assert_eq!(count(&store), 1);
    assert_eq!(
        store
            .with_connection(|c| c.query_row(
                "SELECT count(*) FROM client_registration_accounts",
                [],
                |r| r.get::<_, i64>(0)
            ))
            .unwrap(),
        0
    );
}

#[test]
fn ext06_catalog_stale_endpoint_preserves_snapshot_but_cannot_hide_payload_corruption() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let old_endpoint = seed(&store, "acct");
    let old = snapshot("acct", "old");
    apply(&store, &old_endpoint, &old, 0, "op");
    let mut endpoint_json = serde_json::to_value(&old_endpoint).unwrap();
    endpoint_json["settings_version"] = serde_json::json!(2);
    endpoint_json["host"] = serde_json::json!("new.example.test");
    let config = MailConnectionConfig::from_json(
        &serde_json::to_vec(
            &serde_json::json!({"schema_version":1,"account_id":"acct","imap":endpoint_json}),
        )
        .unwrap(),
    )
    .unwrap();
    let endpoint = config.imap();
    let mutation = MailMutation::new(
        "endpoint-edit".into(),
        "endpoint-edit".into(),
        1,
        crate::mail_connection::imap_payload_hash("acct", endpoint, 1).unwrap(),
    )
    .unwrap();
    store
        .put_imap_connection("acct", endpoint, &mutation)
        .unwrap();
    let stale = store.load_imap_mailbox_snapshot("acct").unwrap().unwrap();
    assert!(!stale.binding_current());
    assert_eq!(stale.snapshot(), &old);
    store.with_connection(|c|c.execute("UPDATE imap_mailbox_catalog_operations SET payload_hash=zeroblob(32) WHERE operation_id='op'",[])).unwrap();
    assert_eq!(
        store.load_imap_mailbox_snapshot("acct"),
        Err(ErrorCode::InternalError)
    );
    let original_hash = imap_catalog_payload_hash(2, &old_endpoint, &old, 0).unwrap();
    store.with_connection(|c|c.execute("UPDATE imap_mailbox_catalog_operations SET payload_hash=?1 WHERE operation_id='op'",[original_hash.as_slice()])).unwrap();
    let refreshed = ImapMailboxSnapshot::new(
        "acct",
        2,
        ImapCapabilities::from_features([]),
        old.mailboxes().to_vec(),
    )
    .unwrap();
    let request = MailMutation::new(
        "refresh".into(),
        "refresh".into(),
        1,
        imap_catalog_payload_hash(2, endpoint, &refreshed, 1).unwrap(),
    )
    .unwrap();
    assert_eq!(
        store.replace_imap_mailbox_snapshot(2, endpoint, &refreshed, &request),
        Ok(2)
    );
    let current = store.load_imap_mailbox_snapshot("acct").unwrap().unwrap();
    assert!(current.binding_current());
    assert_eq!(current.snapshot(), &refreshed);
}

#[test]
fn ext06_catalog_last_account_version_is_valid_without_incrementing_the_account() {
    let fixture = Fixture::new();
    let store = fixture.open();
    let endpoint = seed(&store, "acct");
    let snapshot = snapshot("acct", "INBOX");
    store
        .with_connection(|c| {
            c.execute(
                "UPDATE accounts SET settings_version=9223372036854775807 WHERE account_id='acct'",
                [],
            )
        })
        .unwrap();
    let version = i64::MAX as u64;
    let mutation = MailMutation::new(
        "last-version".into(),
        "last-version".into(),
        0,
        imap_catalog_payload_hash(version, &endpoint, &snapshot, 0).unwrap(),
    )
    .unwrap();
    assert_eq!(
        store.replace_imap_mailbox_snapshot(version, &endpoint, &snapshot, &mutation),
        Ok(1)
    );
    let loaded = store.load_imap_mailbox_snapshot("acct").unwrap().unwrap();
    assert_eq!(loaded.account_version(), version);
    assert!(loaded.binding_current());
}

#[test]
fn ext06_catalog_corrupt_current_account_and_operation_rows_fail_closed() {
    for sql in [
        "PRAGMA ignore_check_constraints=ON; UPDATE accounts SET provider='bogus'",
        "PRAGMA ignore_check_constraints=ON; UPDATE accounts SET settings_version=0",
        "PRAGMA ignore_check_constraints=ON; UPDATE imap_mailbox_catalog_operations SET idempotency_key='bad key'",
        "UPDATE imap_mailbox_catalog_operations SET payload_hash=zeroblob(32)",
    ] {
        let fixture = Fixture::new();
        let store = fixture.open();
        let endpoint = seed(&store, "acct");
        let snapshot = snapshot("acct", "INBOX");
        apply(&store, &endpoint, &snapshot, 0, "op");
        store.with_connection(|c| c.execute_batch(sql)).unwrap();
        assert_eq!(
            store.load_imap_mailbox_snapshot("acct"),
            Err(ErrorCode::InternalError),
            "{sql}"
        );
        if sql.contains("catalog_operations") {
            assert_eq!(
                store.replace_imap_mailbox_snapshot(
                    2,
                    &endpoint,
                    &snapshot,
                    &mutation(&endpoint, &snapshot, 0, "op", "op")
                ),
                Err(ErrorCode::InternalError),
                "{sql}"
            );
        }
        assert_eq!(count(&store), 1);
    }
}
