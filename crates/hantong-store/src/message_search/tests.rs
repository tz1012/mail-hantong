use super::*;
use crate::uid_reconcile_jobs::tests::{Fixture, append, identity, reconcile};
use hantong_contracts::message_search::{ImapMetadataFilter, ImapMetadataSearchCursor};

fn request(
    filter: ImapMetadataFilter,
    limit: u32,
    cursor: Option<ImapMetadataSearchCursor>,
) -> ImapMetadataSearchRequest {
    ImapMetadataSearchRequest::new(
        "acct",
        identity(b"INBOX"),
        Some(filter),
        Some(limit),
        cursor,
    )
    .unwrap()
}
fn uids(page: &ImapMetadataSearchPage) -> Vec<u32> {
    page.items().iter().map(|m| m.uid()).collect()
}
type Rows = Vec<(String, Vec<Vec<rusqlite::types::Value>>)>;
fn rows(f: &Fixture) -> Rows {
    f.store().with_connection(|c| {
        let tables={let mut s=c.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;s.query_map([],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?};
        tables.into_iter().map(|table| {
            let mut s=c.prepare(&format!("SELECT * FROM \"{table}\" ORDER BY rowid"))?;
            let width=s.column_count();
            let values=s.query_map([],|r|(0..width).map(|n|r.get(n)).collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>())?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((table,values))
        }).collect()
    }).unwrap()
}
fn search(
    f: &Fixture,
    request: &ImapMetadataSearchRequest,
) -> Result<ImapMetadataSearchPage, ErrorCode> {
    let before = rows(f);
    let result = f.store().search_imap_metadata(request);
    assert_eq!(rows(f), before, "search mutated the database");
    result
}

#[test]
fn mail03_search_distinguishes_unobserved_from_observed_empty() {
    let f = Fixture::new();
    let r = request(ImapMetadataFilter::All, 16, None);
    let p = search(&f, &r).unwrap();
    assert!(p.items().is_empty());
    assert!(p.observed_at_utc_ms().is_none());
    assert!(p.next_cursor().is_none());
    append(&f, 0, 0, 1, &[], "empty");
    let p = search(&f, &r).unwrap();
    assert!(p.items().is_empty());
    assert!(p.observed_at_utc_ms().is_some());
    assert!(p.next_cursor().is_none());
}
#[test]
fn mail03_search_traverses_one_source_at_a_time_in_descending_uid_order() {
    let f = Fixture::new();
    append(&f, 0, 0, 200, &[1, 3, 64], "first");
    append(&f, 1, 64, 200, &[65, 99, 128], "second");
    let p = search(&f, &request(ImapMetadataFilter::All, 2, None)).unwrap();
    assert_eq!(uids(&p), [128, 99]);
    assert!(p.binding_current());
    assert_eq!(p.headers().len(), 2);
    let first = p.next_cursor().unwrap().clone();
    assert_eq!((first.source_cursor_version(), first.resume_uid()), (2, 99));
    let p = search(
        &f,
        &request(ImapMetadataFilter::All, 2, Some(first.clone())),
    )
    .unwrap();
    assert_eq!(uids(&p), [65]);
    let next = p.next_cursor().unwrap().clone();
    assert_eq!(
        (next.source_cursor_version(), next.resume_uid()),
        (1, u32::MAX)
    );
    assert_eq!(next.expires_at_utc_ms(), first.expires_at_utc_ms());
    let p = search(&f, &request(ImapMetadataFilter::All, 2, Some(next))).unwrap();
    assert_eq!(uids(&p), [64, 3]);
    let p = search(
        &f,
        &request(ImapMetadataFilter::All, 2, p.next_cursor().cloned()),
    )
    .unwrap();
    assert_eq!(uids(&p), [1]);
    assert!(p.next_cursor().is_none());
}
#[test]
fn mail03_search_empty_and_filtered_sources_still_make_progress() {
    let f = Fixture::new();
    append(&f, 0, 0, 200, &[1], "first");
    append(&f, 1, 64, 200, &[], "second");
    let p = search(&f, &request(ImapMetadataFilter::Unread, 16, None)).unwrap();
    assert!(p.items().is_empty());
    assert!(p.next_cursor().is_some());
    let p = search(
        &f,
        &request(ImapMetadataFilter::Unread, 16, p.next_cursor().cloned()),
    )
    .unwrap();
    assert!(p.items().is_empty());
    assert!(p.next_cursor().is_none());
}
#[test]
fn mail03_search_uses_latest_present_flags_and_excludes_absent_uids() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1, 3, 5], "base");
    reconcile(&f, 21, 1, &[(1, &["\\Flagged"]), (5, &["\\Seen"])]);
    assert_eq!(
        uids(&search(&f, &request(ImapMetadataFilter::All, 16, None)).unwrap()),
        [5, 1]
    );
    assert_eq!(
        uids(&search(&f, &request(ImapMetadataFilter::Unread, 16, None)).unwrap()),
        [1]
    );
    let p = search(&f, &request(ImapMetadataFilter::Favorite, 16, None)).unwrap();
    assert_eq!(uids(&p), [1]);
    assert!(p.items()[0].is_favorite());
    assert!(!p.items()[0].is_read());
}
#[test]
fn mail03_search_corrupt_mapping_or_overlay_never_falls_back() {
    for sql in [
        "DELETE FROM imap_message_identities WHERE uid=1",
        "UPDATE imap_message_identities SET internal_message_id='msg_'||printf('%064d',0) WHERE uid=1",
        "DELETE FROM imap_uid_reconcile_messages WHERE uid=1",
        "PRAGMA foreign_keys=OFF; DELETE FROM imap_uid_reconcile_heads; PRAGMA foreign_keys=ON;",
        "UPDATE imap_uid_reconcile_requests SET result_hash=zeroblob(32)",
    ] {
        let f = Fixture::new();
        append(&f, 0, 0, 11, &[1, 3], "base");
        reconcile(&f, 21, 1, &[(1, &[]), (3, &[])]);
        f.sql(sql);
        assert_eq!(
            search(&f, &request(ImapMetadataFilter::All, 16, None)),
            Err(ErrorCode::InternalError),
            "{sql}"
        );
    }
}
#[test]
fn mail03_search_cursor_expires_on_state_filter_limit_and_position_changes() {
    for sql in [
        "UPDATE accounts SET status='reauth_required'",
        "UPDATE accounts SET provider='gmail'",
        "UPDATE accounts SET settings_version=settings_version+1",
    ] {
        let f = Fixture::new();
        append(&f, 0, 0, 11, &[1, 3, 5], "base");
        let p = search(&f, &request(ImapMetadataFilter::All, 1, None)).unwrap();
        f.sql(sql);
        assert_eq!(
            search(
                &f,
                &request(ImapMetadataFilter::All, 1, p.next_cursor().cloned())
            ),
            Err(ErrorCode::CursorExpired),
            "{sql}"
        );
    }
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1, 3, 5], "base");
    let p = search(&f, &request(ImapMetadataFilter::All, 1, None)).unwrap();
    let c = p.next_cursor().unwrap();
    assert_eq!(
        search(&f, &request(ImapMetadataFilter::Unread, 1, Some(c.clone()))),
        Err(ErrorCode::CursorExpired)
    );
    assert_eq!(
        search(&f, &request(ImapMetadataFilter::All, 2, Some(c.clone()))),
        Err(ErrorCode::CursorExpired)
    );
    for resume in [0, 1, 2, 6] {
        let token = format!(
            "m1:{}:{}:{}:{}",
            hantong_contracts::wire_hex::encode(c.digest()),
            c.source_cursor_version(),
            resume,
            c.expires_at_utc_ms()
        );
        assert_eq!(
            search(
                &f,
                &request(
                    ImapMetadataFilter::All,
                    1,
                    Some(ImapMetadataSearchCursor::parse(&token).unwrap())
                )
            ),
            Err(ErrorCode::CursorExpired)
        );
    }
}
#[test]
fn mail03_search_requires_current_entities_and_never_hides_orphan_history() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1, 3], "base");
    let p = search(&f, &request(ImapMetadataFilter::All, 1, None)).unwrap();
    f.sql("DELETE FROM imap_mailbox_catalogs");
    assert_eq!(
        search(&f, &request(ImapMetadataFilter::All, 1, None)),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(
        search(
            &f,
            &request(ImapMetadataFilter::All, 1, p.next_cursor().cloned())
        ),
        Err(ErrorCode::CursorExpired)
    );
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    f.sql("PRAGMA foreign_keys=OFF; DELETE FROM imap_uid_cursors; PRAGMA foreign_keys=ON;");
    assert_eq!(
        search(&f, &request(ImapMetadataFilter::All, 16, None)),
        Err(ErrorCode::InternalError)
    );
}

#[test]
fn mail03_search_large_headers_obey_budget_without_consuming_rejected_candidate() {
    use crate::uid_reconcile_jobs::tests::{endpoint, mailbox};
    use hantong_domain::imap_sync::{
        ImapEnvelope, ImapEnvelopeAddress, ImapEnvelopeInput, ImapMessageMetadata, ImapUidPage,
        ImapUidPageRequest,
    };
    let f = Fixture::new();
    let a = ImapEnvelopeAddress::new(
        Some(vec![b'a'; 8192]),
        Some(vec![b'b'; 8192]),
        Some(vec![b'c'; 8192]),
        Some(vec![b'd'; 8192]),
    )
    .unwrap();
    let envelope = ImapEnvelope::new(ImapEnvelopeInput {
        from: Some(vec![a.clone(), a]),
        ..Default::default()
    })
    .unwrap();
    let page = ImapUidPage::new(
        ImapUidPageRequest::new("acct", 1, mailbox(), None, 0).unwrap(),
        7,
        11,
        (1..=3)
            .map(|uid| {
                ImapMessageMetadata::new(uid, vec![], 1767225600000, 12, envelope.clone()).unwrap()
            })
            .collect(),
    )
    .unwrap();
    let mutation = crate::mail_connection::MailMutation::new(
        "big".into(),
        "big".into(),
        0,
        crate::imap_sync::imap_uid_page_payload_hash(2, 1, &endpoint(), &page, 0).unwrap(),
    )
    .unwrap();
    f.store()
        .apply_imap_uid_page(2, 1, &endpoint(), &page, &mutation)
        .unwrap();
    let p = search(&f, &request(ImapMetadataFilter::All, 32, None)).unwrap();
    assert_eq!(uids(&p), [3]);
    assert_eq!(p.next_cursor().unwrap().resume_uid(), 3);
    assert!(p.headers()[0].projection_json().len() > 131072);
    let p = search(
        &f,
        &request(ImapMetadataFilter::All, 32, p.next_cursor().cloned()),
    )
    .unwrap();
    assert_eq!(uids(&p), [2]);
    assert_eq!(p.next_cursor().unwrap().resume_uid(), 2);
    let p = search(
        &f,
        &request(ImapMetadataFilter::All, 32, p.next_cursor().cloned()),
    )
    .unwrap();
    assert_eq!(uids(&p), [1]);
    assert!(p.next_cursor().is_none());
}

#[test]
fn mail03_search_next_source_token_does_not_decode_the_next_page() {
    let f = Fixture::new();
    for (cv, after, uid, id) in [
        (0, 0, 1, "first"),
        (1, 64, 65, "second"),
        (2, 128, 129, "third"),
        (3, 192, 193, "fourth"),
    ] {
        append(&f, cv, after, 300, &[uid], id);
    }
    f.sql("UPDATE imap_uid_page_operations SET page_json='{}' WHERE operation_id='second'");
    let p = search(&f, &request(ImapMetadataFilter::All, 16, None)).unwrap();
    assert_eq!(uids(&p), [193]);
    let p = search(
        &f,
        &request(ImapMetadataFilter::All, 16, p.next_cursor().cloned()),
    )
    .unwrap();
    assert_eq!(uids(&p), [129]);
    assert_eq!(p.next_cursor().unwrap().source_cursor_version(), 2);
    assert_eq!(
        search(
            &f,
            &request(ImapMetadataFilter::All, 16, p.next_cursor().cloned())
        ),
        Err(ErrorCode::InternalError)
    );
}

#[test]
fn mail03_search_catalog_refresh_append_and_reconcile_expire_prior_cursor() {
    for change in 0..3 {
        let f = Fixture::new();
        append(&f, 0, 0, 200, &[1, 3], "base");
        let p = search(&f, &request(ImapMetadataFilter::All, 1, None)).unwrap();
        match change {
            0 => f.catalog(1),
            1 => append(&f, 1, 64, 200, &[65], "later"),
            _ => {
                reconcile(&f, 21, 1, &[(1, &[]), (3, &[])]);
            }
        }
        assert_eq!(
            search(
                &f,
                &request(ImapMetadataFilter::All, 1, p.next_cursor().cloned())
            ),
            Err(ErrorCode::CursorExpired)
        );
        assert!(search(&f, &request(ImapMetadataFilter::All, 1, None)).is_ok());
    }
}

#[test]
fn mail03_search_recovered_epoch_uses_new_ids_and_keeps_old_ids() {
    use crate::uid_reconcile_jobs::tests::{mailbox, metadata};
    use hantong_contracts::uid_recovery::UidRecoveryRequest;
    use hantong_domain::{
        imap_recovery::ImapUidRecoveryPage,
        imap_sync::{ImapUidPage, ImapUidPageRequest},
    };
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1, 3], "base");
    let old = search(&f, &request(ImapMetadataFilter::All, 1, None)).unwrap();
    let raw = UidRecoveryRequest::new(
        "acct",
        identity(b"INBOX"),
        "recover",
        "recover",
        1,
        1,
        &"0".repeat(64),
    )
    .unwrap();
    let hash = hantong_contracts::wire_hex::encode(
        &hantong_platform::sha256(&raw.intent_bytes()).unwrap(),
    );
    let recovery = UidRecoveryRequest::new(
        "acct",
        identity(b"INBOX"),
        "recover",
        "recover",
        1,
        1,
        &hash,
    )
    .unwrap();
    f.begin_for(31, 9);
    f.store()
        .admit_uid_recovery(
            [31; 32],
            &recovery,
            crate::uid_page_jobs::UidQueueAdmission::Reserved,
        )
        .unwrap();
    let crate::uid_recovery_jobs::UidRecoveryStart::Ready(job) =
        f.store().start_uid_recovery([31; 32]).unwrap()
    else {
        panic!("ready")
    };
    let page = ImapUidPage::new(
        ImapUidPageRequest::new("acct", 1, mailbox(), Some(9), 0).unwrap(),
        9,
        11,
        vec![metadata(3, &[])],
    )
    .unwrap();
    f.store()
        .complete_uid_recovery_success(
            [31; 32],
            &ImapUidRecoveryPage::new(job.fetch_request().clone(), page).unwrap(),
        )
        .unwrap();
    assert_eq!(
        search(
            &f,
            &request(ImapMetadataFilter::All, 1, old.next_cursor().cloned())
        ),
        Err(ErrorCode::CursorExpired)
    );
    let new = search(&f, &request(ImapMetadataFilter::All, 16, None)).unwrap();
    assert_eq!(uids(&new), [3]);
    assert!(new.next_cursor().is_none());
    assert_ne!(
        old.items()[0].internal_message_id(),
        new.items()[0].internal_message_id()
    );
    assert_eq!(f.count("imap_message_identities"), 3);
    f.sql("DELETE FROM imap_message_identities WHERE uid_validity=9");
    assert_eq!(
        f.store().load_uid_recovery_receipt(
            [9; 16],
            &hantong_contracts::uid_page::UidPageStatusRequest::new("acct", [31; 32]).unwrap()
        ),
        Err(ErrorCode::InternalError)
    );
}

#[test]
fn mail03_search_default_max_limits_and_expiry_are_bounded() {
    let f = Fixture::new();
    append(&f, 0, 0, 65, &(1..=64).collect::<Vec<_>>(), "base");
    let before = crate::utc_millis().unwrap();
    let default =
        ImapMetadataSearchRequest::new("acct", identity(b"INBOX"), None, None, None).unwrap();
    let p = search(&f, &default).unwrap();
    let after = crate::utc_millis().unwrap();
    assert_eq!(p.items().len(), 16);
    let cursor = p.next_cursor().unwrap();
    assert!((before + 300_000..=after + 300_000).contains(&cursor.expires_at_utc_ms()));
    let expired = ImapMetadataSearchCursor::parse(&format!(
        "m1:{}:{}:{}:1",
        hantong_contracts::wire_hex::encode(cursor.digest()),
        cursor.source_cursor_version(),
        cursor.resume_uid()
    ))
    .unwrap();
    assert_eq!(
        search(&f, &request(ImapMetadataFilter::All, 16, Some(expired))),
        Err(ErrorCode::CursorExpired)
    );
    let p = search(&f, &request(ImapMetadataFilter::All, 32, None)).unwrap();
    assert_eq!(p.items().len(), 32);
    let p = search(
        &f,
        &request(ImapMetadataFilter::All, 32, p.next_cursor().cloned()),
    )
    .unwrap();
    assert_eq!(p.items().len(), 32);
    assert!(p.next_cursor().is_none());
}

#[test]
fn mail03_search_selected_corruption_in_children_neighbors_and_orphan_overlay_fails() {
    for sql in [
        "DELETE FROM imap_uid_messages WHERE uid=65",
        "UPDATE imap_uid_page_operations SET after_uid=63,covered_through_uid=127 WHERE operation_id='second'",
        "UPDATE imap_uid_page_operations SET covered_through_uid=63,uid_next_at_start=64 WHERE operation_id='first'",
        "UPDATE imap_uid_page_operations SET page_hash=zeroblob(32) WHERE operation_id='second'",
        "PRAGMA foreign_keys=OFF; INSERT INTO imap_uid_reconcile_messages(account_id,mailbox_key,mailbox_encoding,uid_validity,uid,presence,flags_json,generation,observed_at_utc_ms,last_execution_id) VALUES('acct','494e424f58','modified_utf7',7,65,'present','[]',1,1,randomblob(32)); PRAGMA foreign_keys=ON;",
    ] {
        let f = Fixture::new();
        append(&f, 0, 0, 200, &[1], "first");
        append(&f, 1, 64, 200, &[65, 66], "second");
        f.sql("PRAGMA foreign_keys=OFF");
        f.sql(sql);
        assert_eq!(
            search(&f, &request(ImapMetadataFilter::All, 16, None)),
            Err(ErrorCode::InternalError),
            "{sql}"
        );
    }
}

#[test]
fn mail03_search_changed_endpoint_returns_stale_snapshot_and_expires_old_token() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1, 3], "base");
    let old = search(&f, &request(ImapMetadataFilter::All, 1, None)).unwrap();
    let config=hantong_domain::mail_connection::MailConnectionConfig::from_json(br#"{"schema_version":1,"account_id":"acct","imap":{"settings_version":2,"host":"changed.fixture.invalid","port":993,"tls":"implicit_tls","login_name":"synthetic-uid","authentication":"app_password"}}"#).unwrap();
    let mutation = crate::mail_connection::MailMutation::new(
        "changed".into(),
        "changed".into(),
        1,
        crate::mail_connection::imap_payload_hash("acct", config.imap(), 1).unwrap(),
    )
    .unwrap();
    f.store()
        .put_imap_connection("acct", config.imap(), &mutation)
        .unwrap();
    assert_eq!(
        search(
            &f,
            &request(ImapMetadataFilter::All, 1, old.next_cursor().cloned())
        ),
        Err(ErrorCode::CursorExpired)
    );
    let new = search(&f, &request(ImapMetadataFilter::All, 16, None)).unwrap();
    assert!(!new.binding_current());
    assert_eq!(uids(&new), [3, 1]);
}

#[test]
fn mail03_search_missing_cursor_cannot_hide_orphan_reconcile_history() {
    for remaining in [
        "imap_uid_reconcile_heads",
        "imap_uid_reconcile_requests",
        "imap_uid_reconcile_messages",
    ] {
        let f = Fixture::new();
        append(&f, 0, 0, 11, &[1], "base");
        reconcile(&f, 21, 1, &[(1, &[])]);
        f.sql("PRAGMA foreign_keys=OFF; DELETE FROM imap_uid_cursors; DELETE FROM imap_uid_epochs; DELETE FROM imap_uid_page_operations; DELETE FROM imap_uid_messages; DELETE FROM imap_message_identities;");
        for table in [
            "imap_uid_reconcile_heads",
            "imap_uid_reconcile_requests",
            "imap_uid_reconcile_messages",
        ] {
            if table != remaining {
                f.sql(&format!("DELETE FROM {table}"));
            }
        }
        assert_eq!(
            search(&f, &request(ImapMetadataFilter::All, 16, None)),
            Err(ErrorCode::InternalError),
            "{remaining}"
        );
    }
}

#[test]
fn mail03_search_unobserved_allows_legitimate_first_append_jobs_and_audits() {
    use crate::uid_page_jobs::UidQueueAdmission;
    for queue in [
        UidQueueAdmission::Reserved,
        UidQueueAdmission::Full,
        UidQueueAdmission::Closed,
    ] {
        let f = Fixture::new();
        f.begin_for(21, 9);
        assert!(
            search(&f, &request(ImapMetadataFilter::All, 16, None))
                .unwrap()
                .observed_at_utc_ms()
                .is_none()
        );
        f.store()
            .admit_uid_page(
                [21; 32],
                &crate::uid_reconcile_jobs::tests::append_request("first", "first", 0),
                queue,
            )
            .unwrap();
        assert!(
            search(&f, &request(ImapMetadataFilter::All, 16, None))
                .unwrap()
                .observed_at_utc_ms()
                .is_none()
        );
        if queue == UidQueueAdmission::Reserved {
            f.store().start_uid_page([21; 32]).unwrap();
            assert!(
                search(&f, &request(ImapMetadataFilter::All, 16, None))
                    .unwrap()
                    .observed_at_utc_ms()
                    .is_none()
            );
        }
    }
}

#[test]
fn mail03_search_missing_cursor_rejects_jobs_that_require_prior_observation() {
    use crate::uid_page_jobs::{UidPageStart, UidQueueAdmission};
    use crate::uid_reconcile_jobs::tests::{append_request, metadata};
    use hantong_contracts::uid_recovery::UidRecoveryRequest;
    use hantong_domain::imap_sync::ImapUidPage;
    for kind in 0..3 {
        let f = Fixture::new();
        if kind < 2 {
            append(&f, 0, 0, 11, &[1], "base");
        }
        f.begin_for(31, 9);
        if kind == 1 {
            let raw = UidRecoveryRequest::new(
                "acct",
                identity(b"INBOX"),
                "recovery",
                "recovery",
                1,
                1,
                &"0".repeat(64),
            )
            .unwrap();
            let hash = hantong_contracts::wire_hex::encode(
                &hantong_platform::sha256(&raw.intent_bytes()).unwrap(),
            );
            let r = UidRecoveryRequest::new(
                "acct",
                identity(b"INBOX"),
                "recovery",
                "recovery",
                1,
                1,
                &hash,
            )
            .unwrap();
            f.store()
                .admit_uid_recovery([31; 32], &r, UidQueueAdmission::Full)
                .unwrap();
        } else {
            f.store()
                .admit_uid_page(
                    [31; 32],
                    &append_request("job", "job", u64::from(kind == 0)),
                    UidQueueAdmission::Reserved,
                )
                .unwrap();
            if kind == 2 {
                let UidPageStart::Ready(job) = f.store().start_uid_page([31; 32]).unwrap() else {
                    panic!("ready")
                };
                let p =
                    ImapUidPage::new(job.fetch_request().clone(), 7, 11, vec![metadata(1, &[])])
                        .unwrap();
                f.store().complete_uid_page_success([31; 32], &p).unwrap();
            }
        }
        f.sql("PRAGMA foreign_keys=OFF; DELETE FROM imap_uid_cursors; DELETE FROM imap_uid_epochs; DELETE FROM imap_uid_page_operations; DELETE FROM imap_uid_messages; DELETE FROM imap_message_identities; PRAGMA foreign_keys=ON;");
        assert_eq!(
            search(&f, &request(ImapMetadataFilter::All, 16, None)),
            Err(ErrorCode::InternalError),
            "kind {kind}"
        );
    }
}
