use super::*;
use crate::{
    Store,
    account_provisioning::{InitialImapProbeOutcome, imap_account_stage_payload_hash},
    imap_catalog::imap_catalog_payload_hash,
    mail_connection::MailMutation,
};
use hantong_domain::{
    account_provisioning::NewImapAccountDraft,
    capabilities::ImapCapabilities,
    imap_sync::{
        ImapEnvelope, ImapEnvelopeAddress, ImapEnvelopeInput, ImapMessageMetadata, ImapUidPage,
        ImapUidPageRequest,
    },
    mail_connection::{ImapEndpointConfig, MailConnectionConfig},
    mailboxes::{
        ImapMailboxDescriptor, ImapMailboxName, ImapMailboxSnapshot, MailboxChildren,
        MailboxEncoding,
    },
};
use rusqlite::types::Value;
use std::{fs, path::PathBuf};

type Rows = Vec<Vec<Vec<Value>>>;
struct Fixture {
    root: PathBuf,
    store: Option<Store>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "hantong-uid-store-{}",
            hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
        ));
        fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        let f = Self {
            root,
            store: Some(store),
        };
        for account in ["acct", "other"] {
            let draft = NewImapAccountDraft::new(account.into(), None, None).unwrap();
            let ep = endpoint_for(account, 1, "imap.fixture.invalid");
            let mutation = MailMutation::new(
                format!("provision-{account}"),
                format!("provision-{account}"),
                0,
                imap_account_stage_payload_hash(&draft, &ep).unwrap(),
            )
            .unwrap();
            let staged = f
                .store()
                .stage_imap_account(&draft, &ep, &mutation)
                .unwrap();
            f.store()
                .finish_initial_imap_probe(
                    account,
                    staged.account_version,
                    staged.imap_version,
                    0,
                    &InitialImapProbeOutcome::Ready(vec![]),
                )
                .unwrap();
            f.catalog(account, 0, &ep, false, false);
        }
        f
    }
    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }
    fn reopen(&mut self) {
        drop(self.store.take());
        self.store = Some(Store::open(self.root.join("data"), self.root.join("keys")).unwrap());
    }
    fn sql(&self, sql: &str) {
        self.store()
            .with_connection(|c| c.execute_batch(sql))
            .unwrap();
    }
    fn rows(&self) -> Rows {
        self.store()
            .with_connection(|c| {
                [
                    "imap_uid_page_operations",
                    "imap_uid_cursors",
                    "imap_uid_messages",
                ]
                .into_iter()
                .map(|table| {
                    let mut q = c.prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))?;
                    let count = q.column_count();
                    q.query_map([], |r| {
                        (0..count)
                            .map(|i| r.get(i))
                            .collect::<rusqlite::Result<Vec<Value>>>()
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
                })
                .collect()
            })
            .unwrap()
    }
    fn counts(&self) -> (i64, i64, i64) {
        self.store().with_connection(|c| c.query_row("SELECT (SELECT count(*) FROM imap_uid_page_operations),(SELECT count(*) FROM imap_uid_cursors),(SELECT count(*) FROM imap_uid_messages)", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))).unwrap()
    }
    fn catalog(
        &self,
        account: &str,
        previous: u64,
        ep: &ImapEndpointConfig,
        omit_inbox: bool,
        no_select: bool,
    ) {
        let mut names = vec![descriptor(b"Archive", false)];
        if !omit_inbox {
            names.push(descriptor(b"inbox", no_select));
        }
        let snapshot = ImapMailboxSnapshot::new(
            account,
            ep.settings_version(),
            ImapCapabilities::from_features([]),
            names,
        )
        .unwrap();
        let id = format!("catalog-{account}-{}", previous + 1);
        let mutation = MailMutation::new(
            id.clone(),
            id,
            previous,
            imap_catalog_payload_hash(2, ep, &snapshot, previous).unwrap(),
        )
        .unwrap();
        assert_eq!(
            self.store()
                .replace_imap_mailbox_snapshot(2, ep, &snapshot, &mutation)
                .unwrap(),
            previous + 1
        );
    }
    fn apply(
        &self,
        page: &ImapUidPage,
        version: u64,
        id: &str,
    ) -> Result<ImapUidApplyReceipt, ErrorCode> {
        let ep = endpoint_for(page.request().account_id(), 1, "imap.fixture.invalid");
        let mutation = mutation(2, 1, &ep, page, version, id, id);
        self.store().apply_imap_uid_page(2, 1, &ep, page, &mutation)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.store.take());
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn endpoint(version: u64, host: &str) -> ImapEndpointConfig {
    endpoint_for("acct", version, host)
}
fn endpoint_for(account: &str, version: u64, host: &str) -> ImapEndpointConfig {
    let json = serde_json::json!({"schema_version":1,"account_id":account,"imap":{"settings_version":version,"host":host,"port":993,"tls":"implicit_tls","login_name":format!("synthetic-{account}"),"authentication":"app_password"}});
    MailConnectionConfig::from_json(&serde_json::to_vec(&json).unwrap())
        .unwrap()
        .imap()
        .clone()
}
fn mailbox() -> ImapMailboxName {
    name(b"INBOX", MailboxEncoding::Utf8)
}
fn name(wire: &[u8], encoding: MailboxEncoding) -> ImapMailboxName {
    ImapMailboxName::from_wire(wire, encoding).unwrap()
}
fn descriptor(wire: &[u8], no_select: bool) -> ImapMailboxDescriptor {
    ImapMailboxDescriptor::new(
        name(wire, MailboxEncoding::Utf8),
        Some('/'),
        no_select,
        false,
        MailboxChildren::Unknown,
        vec![],
    )
    .unwrap()
}
fn message(uid: u32) -> ImapMessageMetadata {
    let group = ImapEnvelopeAddress::new(None, None, Some(b"group".to_vec()), None).unwrap();
    let sender = ImapEnvelopeAddress::new(
        Some(vec![]),
        Some(b"route".to_vec()),
        Some(b"synthetic".to_vec()),
        Some(b"fixture.invalid".to_vec()),
    )
    .unwrap();
    let end = ImapEnvelopeAddress::new(None, None, None, None).unwrap();
    let envelope = ImapEnvelope::new(ImapEnvelopeInput {
        subject: Some(vec![0, 0xff, b'\n']),
        in_reply_to: Some(vec![]),
        message_id: Some(b"same-synthetic-message-id".to_vec()),
        from: Some(vec![sender]),
        to: Some(vec![group, end]),
        ..Default::default()
    })
    .unwrap();
    ImapMessageMetadata::new(
        uid,
        vec!["\\Seen".into(), "X-Tag".into(), "\\Recent".into()],
        -1000,
        42,
        envelope,
    )
    .unwrap()
}
fn page(after: u32, expected: Option<u32>, uids: &[u32]) -> ImapUidPage {
    page_for("acct", mailbox(), 1, after, expected, 7, 100, uids)
}
#[allow(clippy::too_many_arguments)]
fn page_for(
    account: &str,
    mailbox: ImapMailboxName,
    endpoint_version: u64,
    after: u32,
    expected: Option<u32>,
    validity: u32,
    next: u32,
    uids: &[u32],
) -> ImapUidPage {
    ImapUidPage::new(
        ImapUidPageRequest::new(account, endpoint_version, mailbox, expected, after).unwrap(),
        validity,
        next,
        uids.iter().copied().map(message).collect(),
    )
    .unwrap()
}
fn mutation(
    av: u64,
    cg: u64,
    ep: &ImapEndpointConfig,
    page: &ImapUidPage,
    cv: u64,
    op: &str,
    key: &str,
) -> MailMutation {
    MailMutation::new(
        op.into(),
        key.into(),
        cv,
        imap_uid_page_payload_hash(av, cg, ep, page, cv).unwrap(),
    )
    .unwrap()
}

#[test]
fn ext04_store_binary_envelope_and_same_message_id_roundtrip_after_reopen() {
    let mut f = Fixture::new();
    let p = page(0, None, &[2, 9]);
    assert_eq!(f.apply(&p, 0, "first").unwrap().inserted_count(), 2);
    f.reopen();
    for uid in [2, 9] {
        let saved = f
            .store()
            .load_imap_uid_metadata("acct", &mailbox(), 7, uid)
            .unwrap()
            .unwrap();
        assert_eq!(saved, message(uid));
        assert_eq!(
            saved.envelope().fields().subject.as_deref(),
            Some(&[0, 255, b'\n'][..])
        );
        assert_eq!(
            saved.envelope().fields().in_reply_to.as_deref(),
            Some(&[][..])
        );
        assert_eq!(saved.flags(), &["\\seen", "x-tag"]);
    }
    assert!(
        f.store()
            .load_imap_uid_metadata("acct", &mailbox(), 7, 3)
            .unwrap()
            .is_none()
    );
    let cursor = f
        .store()
        .load_imap_uid_cursor("acct", &mailbox())
        .unwrap()
        .unwrap();
    assert!(cursor.binding_current());
    assert_eq!(cursor.account_version(), 2);
    assert_eq!(cursor.endpoint_version(), 1);
    assert_eq!(cursor.uid_validity(), 7);
    assert_eq!(f.counts(), (1, 1, 2));
}

#[test]
fn sync02_store_gaps_empty_page_and_exact_replay_preserve_later_cursor() {
    let f = Fixture::new();
    let first = page(0, None, &[2, 9, 64]);
    let receipt = f.apply(&first, 0, "first").unwrap();
    assert_eq!(
        (
            receipt.cursor_version(),
            receipt.covered_through_uid(),
            receipt.inserted_count()
        ),
        (1, 64, 3)
    );
    let second = f.apply(&page(64, Some(7), &[]), 1, "second").unwrap();
    assert_eq!(
        (
            second.cursor_version(),
            second.covered_through_uid(),
            second.inserted_count()
        ),
        (2, 99, 0)
    );
    let before = f.rows();
    assert_eq!(f.apply(&first, 0, "first").unwrap(), receipt);
    assert_eq!(before, f.rows());
    let cursor = f
        .store()
        .load_imap_uid_cursor("acct", &mailbox())
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            cursor.version(),
            cursor.covered_through_uid(),
            cursor.uid_next_at_start()
        ),
        (2, 99, 100)
    );
    assert_eq!(f.counts(), (2, 1, 3));
}

#[test]
fn ext04_store_account_mailbox_and_encoding_identity_are_distinct() {
    let f = Fixture::new();
    let inbox = page_for(
        "acct",
        name(b"inbox", MailboxEncoding::Utf8),
        1,
        0,
        None,
        7,
        100,
        &[1],
    );
    f.apply(&inbox, 0, "inbox").unwrap();
    assert_eq!(
        f.store()
            .load_imap_uid_metadata("acct", &mailbox(), 7, 1)
            .unwrap(),
        Some(message(1))
    );
    for other in [
        page_for("other", mailbox(), 1, 0, None, 7, 100, &[1]),
        page_for(
            "acct",
            name(b"Archive", MailboxEncoding::Utf8),
            1,
            0,
            None,
            7,
            100,
            &[1],
        ),
    ] {
        f.apply(
            &other,
            0,
            if other.request().account_id() == "other" {
                "other"
            } else {
                "archive"
            },
        )
        .unwrap();
    }
    assert_eq!(f.counts(), (3, 3, 3));
    assert!(
        f.store()
            .load_imap_uid_metadata("acct", &mailbox(), 8, 1)
            .unwrap()
            .is_none()
    );
    let wrong_encoding = page_for(
        "acct",
        name(b"INBOX", MailboxEncoding::ModifiedUtf7),
        1,
        0,
        None,
        7,
        100,
        &[1],
    );
    let before = f.rows();
    assert_eq!(
        f.apply(&wrong_encoding, 0, "encoding"),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(before, f.rows());
    assert!(
        f.store()
            .load_imap_uid_cursor("acct", wrong_encoding.request().mailbox())
            .unwrap()
            .is_none()
    );
}

#[test]
fn sync02_store_stale_bindings_and_wrong_hash_never_write() {
    for fault in 0..7 {
        let f = Fixture::new();
        let first = page(0, None, &[2]);
        f.apply(&first, 0, "first").unwrap();
        let mut next = page(64, Some(7), &[65]);
        let mut ep = endpoint(1, "imap.fixture.invalid");
        let (mut av, mut cg, mut cv) = (2, 1, 1);
        match fault {
            0 => av = 3,
            1 => cg = 2,
            2 => cv = 0,
            3 => next = page(0, Some(7), &[2]),
            4 => ep = endpoint(1, "changed.fixture.invalid"),
            5 => {
                ep = endpoint(2, "imap.fixture.invalid");
                next = page_for("acct", mailbox(), 2, 64, Some(7), 7, 100, &[65]);
            }
            _ => {}
        }
        let mut m = mutation(av, cg, &ep, &next, cv, "next", "next");
        if fault == 6 {
            m = MailMutation::new("next".into(), "next".into(), cv, [0; 32]).unwrap();
        }
        let before = f.rows();
        assert_eq!(
            f.store().apply_imap_uid_page(av, cg, &ep, &next, &m),
            Err(if fault == 6 {
                ErrorCode::InvalidInput
            } else {
                ErrorCode::Conflict
            })
        );
        assert_eq!(before, f.rows());
    }
}

#[test]
fn sync02_store_operation_and_idempotency_collisions_are_not_replay() {
    let f = Fixture::new();
    let first = page(0, None, &[2]);
    f.apply(&first, 0, "first").unwrap();
    let ep = endpoint(1, "imap.fixture.invalid");
    for (op, key, request) in [
        ("first", "different", first.clone()),
        ("different", "first", first.clone()),
        ("first", "first", page(0, None, &[3])),
        (
            "first",
            "first",
            page_for("other", mailbox(), 1, 0, None, 7, 100, &[2]),
        ),
    ] {
        let m = mutation(2, 1, &ep, &request, 0, op, key);
        let before = f.rows();
        assert_eq!(
            f.store().apply_imap_uid_page(2, 1, &ep, &request, &m),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(before, f.rows());
    }
}

#[test]
fn ext04_store_unrelated_catalog_refresh_requires_fresh_cas_without_cursor_reset() {
    let f = Fixture::new();
    f.apply(&page(0, None, &[2]), 0, "first").unwrap();
    let ep = endpoint(1, "imap.fixture.invalid");
    f.catalog("acct", 1, &ep, false, false);
    let next = page(64, Some(7), &[65]);
    assert_eq!(f.apply(&next, 1, "stale-catalog"), Err(ErrorCode::Conflict));
    let m = mutation(2, 2, &ep, &next, 1, "fresh-catalog", "fresh-catalog");
    assert_eq!(
        f.store()
            .apply_imap_uid_page(2, 2, &ep, &next, &m)
            .unwrap()
            .cursor_version(),
        2
    );
    let cursor = f
        .store()
        .load_imap_uid_cursor("acct", &mailbox())
        .unwrap()
        .unwrap();
    assert_eq!(cursor.catalog_generation(), 2);
    assert!(cursor.binding_current());
    assert_eq!(
        f.store()
            .load_imap_uid_metadata("acct", &mailbox(), 7, 2)
            .unwrap(),
        Some(message(2))
    );
}

#[test]
fn ext04_store_missing_or_nonselectable_catalog_mailbox_blocks_apply() {
    for omit in [false, true] {
        let f = Fixture::new();
        let ep = endpoint(1, "imap.fixture.invalid");
        f.catalog("acct", 1, &ep, omit, !omit);
        let p = page(0, None, &[2]);
        let m = mutation(2, 2, &ep, &p, 0, "blocked", "blocked");
        let before = f.rows();
        assert_eq!(
            f.store().apply_imap_uid_page(2, 2, &ep, &p, &m),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(before, f.rows());
    }
}

#[test]
fn ext04_store_empty_cursor_also_rejects_changed_endpoint_without_reset() {
    let f = Fixture::new();
    let empty = page_for("acct", mailbox(), 1, 0, None, 7, 1, &[]);
    f.apply(&empty, 0, "empty").unwrap();
    f.sql("UPDATE mail_connection_settings SET host='changed.fixture.invalid' WHERE account_id='acct' AND endpoint_kind='imap'");
    let cursor = f
        .store()
        .load_imap_uid_cursor("acct", &mailbox())
        .unwrap()
        .unwrap();
    assert!(!cursor.binding_current());
    assert_eq!(cursor.covered_through_uid(), 0);
    let ep = endpoint(1, "changed.fixture.invalid");
    f.catalog("acct", 1, &ep, false, false);
    let next = page_for("acct", mailbox(), 1, 0, Some(7), 7, 2, &[1]);
    let m = mutation(2, 2, &ep, &next, 1, "changed", "changed");
    let before = f.rows();
    assert_eq!(
        f.store().apply_imap_uid_page(2, 2, &ep, &next, &m),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(before, f.rows());
}

#[test]
fn ext04_store_uidvalidity_and_uidnext_regressions_preserve_prior_data() {
    for changed_validity in [false, true] {
        let f = Fixture::new();
        f.apply(&page(0, None, &[2]), 0, "first").unwrap();
        let next = if changed_validity {
            page_for("acct", mailbox(), 1, 64, Some(8), 8, 100, &[65])
        } else {
            page_for("acct", mailbox(), 1, 64, Some(7), 7, 80, &[65])
        };
        let before = f.rows();
        assert_eq!(f.apply(&next, 1, "changed"), Err(ErrorCode::Conflict));
        assert_eq!(before, f.rows());
    }
}

#[test]
fn sync02_store_transaction_failure_rolls_back_operation_cursor_and_messages() {
    for target in ["operation", "cursor", "message"] {
        let f = Fixture::new();
        f.apply(&page(0, None, &[2]), 0, "first").unwrap();
        let trigger = match target {
            "operation" => {
                "CREATE TRIGGER inject_uid_failure BEFORE INSERT ON imap_uid_page_operations BEGIN SELECT RAISE(ABORT,'synthetic'); END;"
            }
            "cursor" => {
                "CREATE TRIGGER inject_uid_failure BEFORE UPDATE ON imap_uid_cursors BEGIN SELECT RAISE(ABORT,'synthetic'); END;"
            }
            _ => {
                "CREATE TRIGGER inject_uid_failure BEFORE INSERT ON imap_uid_messages BEGIN SELECT RAISE(ABORT,'synthetic'); END;"
            }
        };
        f.sql(trigger);
        let before = f.rows();
        assert_eq!(
            f.apply(&page(64, Some(7), &[65]), 1, "second"),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(before, f.rows());
        f.sql("DROP TRIGGER inject_uid_failure");
        assert_eq!(
            f.apply(&page(64, Some(7), &[65]), 1, "second")
                .unwrap()
                .cursor_version(),
            2
        );
    }
}

#[test]
fn sync02_store_declared_metadata_missing_from_old_or_current_page_is_corruption() {
    for deleted_uid in [2, 130] {
        let f = Fixture::new();
        for (version, after, uids) in [(0, 0, vec![2, 9]), (1, 64, vec![65]), (2, 128, vec![130])] {
            let page = page_for(
                "acct",
                mailbox(),
                1,
                after,
                (version > 0).then_some(7),
                7,
                300,
                &uids,
            );
            f.apply(&page, version, &format!("page-{version}")).unwrap();
        }
        assert_eq!(f.counts(), (3, 1, 4));
        assert_eq!(
            f.store()
                .load_imap_uid_metadata("acct", &mailbox(), 7, deleted_uid)
                .unwrap(),
            Some(message(deleted_uid))
        );
        f.sql(&format!(
            "DELETE FROM imap_uid_messages WHERE account_id='acct' AND uid={deleted_uid}"
        ));
        assert_eq!(f.counts(), (3, 1, 3));
        let before = f.rows();
        assert_eq!(
            f.store()
                .load_imap_uid_metadata("acct", &mailbox(), 7, deleted_uid),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(before, f.rows());
    }
}

#[test]
fn ext04_store_legitimate_uid_gaps_and_uncovered_uids_remain_absent() {
    let f = Fixture::new();
    for (version, after, uids) in [(0, 0, vec![2, 9]), (1, 64, vec![65]), (2, 128, vec![130])] {
        let page = page_for(
            "acct",
            mailbox(),
            1,
            after,
            (version > 0).then_some(7),
            7,
            300,
            &uids,
        );
        f.apply(&page, version, &format!("page-{version}")).unwrap();
    }
    assert_eq!(
        f.store()
            .load_imap_uid_cursor("acct", &mailbox())
            .unwrap()
            .unwrap()
            .covered_through_uid(),
        192
    );
    for uid in [3, 66, 129, 193, 300] {
        assert_eq!(
            f.store().load_imap_uid_metadata("acct", &mailbox(), 7, uid),
            Ok(None)
        );
    }
    assert_eq!(f.counts(), (3, 1, 4));
}

#[test]
fn sync02_store_metadata_hash_canonical_json_and_operation_links_are_revalidated() {
    for corruption in 0..5 {
        let f = Fixture::new();
        f.apply(&page(0, None, &[2]), 0, "first").unwrap();
        f.apply(&page(64, Some(7), &[65]), 1, "second").unwrap();
        match corruption {
            0 => f.sql("UPDATE imap_uid_messages SET metadata_hash=zeroblob(32) WHERE uid=2"),
            1 => f.sql("UPDATE imap_uid_messages SET metadata_json=metadata_json||' ' WHERE uid=2"),
            2 => f.sql("UPDATE imap_uid_messages SET operation_id='second' WHERE uid=2"),
            3 => f.sql(
                "UPDATE imap_uid_messages SET stored_at_utc_ms=stored_at_utc_ms+1 WHERE uid=2",
            ),
            _ => {
                let mut fields = message(2).envelope().fields().clone();
                fields.subject = Some(b"forged-synthetic-subject".to_vec());
                let forged = ImapMessageMetadata::new(
                    2,
                    vec!["\\seen".into(), "x-tag".into()],
                    -1000,
                    42,
                    ImapEnvelope::new(fields).unwrap(),
                )
                .unwrap();
                let bytes = forged.to_storage_json().unwrap();
                let hash = hantong_platform::sha256(&bytes).unwrap();
                let json = String::from_utf8(bytes).unwrap();
                f.store().with_connection(|c| c.execute("UPDATE imap_uid_messages SET metadata_json=?1,metadata_hash=?2 WHERE uid=2", rusqlite::params![json, hash.as_slice()])).unwrap();
            }
        }
        assert_eq!(
            f.store().load_imap_uid_metadata("acct", &mailbox(), 7, 2),
            Err(ErrorCode::InternalError)
        );
    }
}

#[test]
fn sync02_store_old_operation_hash_and_page_corruption_reject_exact_replay() {
    for corruption in ["page_hash", "payload_hash", "page_json", "account_version"] {
        let f = Fixture::new();
        let first = page(0, None, &[2]);
        f.apply(&first, 0, "first").unwrap();
        f.apply(&page(64, Some(7), &[65]), 1, "second").unwrap();
        match corruption {
            "page_json" => f.sql("UPDATE imap_uid_page_operations SET page_json=page_json||' ' WHERE operation_id='first'"),
            "account_version" => f.sql("UPDATE imap_uid_page_operations SET account_version=account_version+1 WHERE operation_id='first'"),
            field => f.sql(&format!("UPDATE imap_uid_page_operations SET {field}=zeroblob(32) WHERE operation_id='first'"))
        }
        let before = f.rows();
        assert_eq!(f.apply(&first, 0, "first"), Err(ErrorCode::InternalError));
        assert_eq!(before, f.rows());
    }
}

#[test]
fn sync02_store_cursor_latest_operation_relationship_is_revalidated() {
    for corruption in [
        "last_operation_id='first'",
        "covered_through_uid=98",
        "version=3",
    ] {
        let f = Fixture::new();
        f.apply(&page(0, None, &[2]), 0, "first").unwrap();
        f.apply(&page(64, Some(7), &[]), 1, "second").unwrap();
        f.sql(&format!(
            "UPDATE imap_uid_cursors SET {corruption} WHERE account_id='acct'"
        ));
        assert_eq!(
            f.store().load_imap_uid_cursor("acct", &mailbox()),
            Err(ErrorCode::InternalError)
        );
    }
}

#[test]
fn sync02_store_operation_quota_preserves_exact_replay_and_blocks_new_effects() {
    let f = Fixture::new();
    let first = page(0, None, &[2]);
    let receipt = f.apply(&first, 0, "first").unwrap();
    // Count-only startup quota fixture: unrelated synthetic history is not read.
    f.sql("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<99999) INSERT INTO imap_uid_page_operations SELECT 'fill-'||x,account_id,'fill-'||x,printf('%016x',x),mailbox_encoding,expected_cursor_version,result_cursor_version,account_version,catalog_generation,endpoint_version,endpoint_hash,page_json,page_hash,payload_hash,uid_validity,uid_next_at_start,after_uid,covered_through_uid,inserted_count,applied_at_utc_ms FROM imap_uid_page_operations CROSS JOIN n WHERE operation_id='first'");
    assert_eq!(f.counts(), (100000, 1, 1));
    assert_eq!(f.apply(&first, 0, "first").unwrap(), receipt);
    assert_eq!(
        f.apply(&page(64, Some(7), &[65]), 1, "second"),
        Err(ErrorCode::QuotaExceeded)
    );
    assert_eq!(f.counts(), (100000, 1, 1));
    assert_eq!(
        f.store()
            .load_imap_uid_cursor("acct", &mailbox())
            .unwrap()
            .unwrap()
            .version(),
        1
    );
}

#[test]
fn ext04_store_lookup_rejects_empty_account_zero_identifiers_and_does_not_cross_scope() {
    let f = Fixture::new();
    f.apply(&page(0, None, &[2]), 0, "first").unwrap();
    assert_eq!(
        f.store().load_imap_uid_cursor("", &mailbox()),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        f.store().load_imap_uid_metadata("acct", &mailbox(), 0, 2),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        f.store().load_imap_uid_metadata("acct", &mailbox(), 7, 0),
        Err(ErrorCode::InvalidInput)
    );
    assert!(
        f.store()
            .load_imap_uid_metadata("other", &mailbox(), 7, 2)
            .unwrap()
            .is_none()
    );
}

#[test]
fn ext04_store_none_validity_is_only_for_absent_cursor_and_some_zero_coverage_resumes() {
    let f = Fixture::new();
    let initial_with_validity = page_for("acct", mailbox(), 1, 0, Some(7), 7, 1, &[]);
    assert_eq!(
        f.apply(&initial_with_validity, 0, "invalid-initial"),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(f.counts(), (0, 0, 0));
    let initial = page_for("acct", mailbox(), 1, 0, None, 7, 1, &[]);
    f.apply(&initial, 0, "empty").unwrap();
    let before = f.rows();
    assert_eq!(
        f.apply(&initial, 1, "invalid-resume"),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(before, f.rows());
    assert_eq!(
        f.apply(&initial_with_validity, 1, "resume")
            .unwrap()
            .cursor_version(),
        2
    );
    assert_eq!(f.counts(), (2, 1, 0));
}

#[test]
fn sync02_store_exact_replay_rejects_orphan_empty_operation_without_cursor() {
    let f = Fixture::new();
    let p = page_for("acct", mailbox(), 1, 0, None, 7, 1, &[]);
    f.apply(&p, 0, "empty").unwrap();
    f.sql("DELETE FROM imap_uid_cursors WHERE account_id='acct'");
    assert_eq!(f.counts(), (1, 0, 0));
    assert_eq!(f.apply(&p, 0, "empty"), Err(ErrorCode::InternalError));
    assert_eq!(f.counts(), (1, 0, 0));
}

#[test]
fn sync02_store_clock_regression_does_not_advance_empty_cursor() {
    let f = Fixture::new();
    let first = page_for("acct", mailbox(), 1, 0, None, 7, 1, &[]);
    f.apply(&first, 0, "empty").unwrap();
    f.sql("UPDATE imap_uid_page_operations SET applied_at_utc_ms=applied_at_utc_ms+60000 WHERE operation_id='empty'; UPDATE imap_uid_cursors SET stored_at_utc_ms=stored_at_utc_ms+60000 WHERE account_id='acct'");
    let before = f.rows();
    let next = page_for("acct", mailbox(), 1, 0, Some(7), 7, 1, &[]);
    assert_eq!(f.apply(&next, 1, "next"), Err(ErrorCode::InternalError));
    assert_eq!(before, f.rows());
}

#[test]
fn sync02_store_historical_replay_does_not_require_current_ready_binding() {
    let f = Fixture::new();
    let p = page(0, None, &[2]);
    let receipt = f.apply(&p, 0, "first").unwrap();
    f.sql("UPDATE accounts SET status='reauth_required',settings_version=settings_version+1 WHERE account_id='acct'");
    let before = f.rows();
    assert!(
        !f.store()
            .load_imap_uid_cursor("acct", &mailbox())
            .unwrap()
            .unwrap()
            .binding_current()
    );
    assert_eq!(f.apply(&p, 0, "first").unwrap(), receipt);
    assert_eq!(before, f.rows());
    assert_eq!(
        f.apply(&page(64, Some(7), &[65]), 1, "next"),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(before, f.rows());
}
