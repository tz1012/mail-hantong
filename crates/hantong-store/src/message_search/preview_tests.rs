use super::*;
use crate::{
    initial_sync::{
        InitialSyncAdmission, InitialSyncFetch, InitialSyncJob, InitialSyncStart, UidQueueAdmission,
    },
    uid_reconcile_jobs::tests::metadata,
};
use hantong_contracts::{
    initial_sync::{InitialSyncMutation, InitialSyncRequest},
    message_search::ImapMetadataSource,
};
use hantong_domain::{
    imap_observation::ImapUidObservation, imap_reconcile::ImapUidReconcilePage,
    imap_sync::ImapMessageMetadata,
};

fn signed(
    op: &str,
    build: impl Fn(InitialSyncMutation) -> InitialSyncRequest,
) -> InitialSyncRequest {
    let raw = build(InitialSyncMutation::new(op, op, &"0".repeat(64)).unwrap());
    let hash = hantong_contracts::wire_hex::encode(
        &hantong_platform::sha256(&raw.intent_bytes()).unwrap(),
    );
    build(InitialSyncMutation::new(op, op, &hash).unwrap())
}
fn admit(f: &Fixture, id: u8, request: &InitialSyncRequest) {
    f.begin_for(id, 9);
    assert!(matches!(
        f.store()
            .admit_initial_sync([id; 32], request, UidQueueAdmission::Reserved, [1; 32])
            .unwrap(),
        InitialSyncAdmission::Admitted(_)
    ));
}
fn job(f: &Fixture, id: u8) -> InitialSyncJob {
    let InitialSyncStart::Ready(job) = f.store().start_initial_sync([id; 32]).unwrap() else {
        panic!("expected job")
    };
    *job
}
fn create(f: &Fixture, id: u8, next: u32, generation: u64) {
    let r = signed(&format!("preview-start-{id}"), |m| {
        InitialSyncRequest::new_start("acct", identity(b"INBOX"), m, 1, generation, 2).unwrap()
    });
    admit(f, id, &r);
    let job = job(f, id);
    let InitialSyncFetch::Observe(r) = job.fetch_request() else {
        panic!("expected EXAMINE")
    };
    f.store()
        .complete_initial_sync_observation(
            [id; 32],
            &ImapUidObservation::new(r.clone(), 7, next).unwrap(),
        )
        .unwrap();
}
fn resume(f: &Fixture, id: u8, plan: u8, version: u64, messages: Vec<ImapMessageMetadata>) {
    let r = signed(&format!("preview-resume-{id}"), |m| {
        InitialSyncRequest::new_resume("acct", identity(b"INBOX"), m, [plan; 32], version, 1)
            .unwrap()
    });
    admit(f, id, &r);
    let job = job(f, id);
    let InitialSyncFetch::Page(r) = job.fetch_request() else {
        panic!("expected page")
    };
    let page = ImapUidReconcilePage::new(r.clone(), r.minimum_uid_next(), messages).unwrap();
    f.store()
        .complete_initial_sync_page([id; 32], &page)
        .unwrap();
}
fn messages(uids: &[u32]) -> Vec<ImapMessageMetadata> {
    uids.iter().map(|uid| metadata(*uid, &["\\Seen"])).collect()
}
fn pause(f: &Fixture, id: u8, plan: u8, version: u64) {
    let r = signed(&format!("preview-pause-{id}"), |m| {
        InitialSyncRequest::new_pause("acct", identity(b"INBOX"), m, [plan; 32], version).unwrap()
    });
    admit(f, id, &r);
}

#[test]
fn sync01_mail03_preview_pages_precede_base_without_changing_contiguous_coverage() {
    let f = Fixture::new();
    append(&f, 0, 0, 200, &[1, 64], "base");
    create(&f, 41, 200, 0);
    resume(&f, 42, 41, 1, messages(&[193, 199]));
    resume(&f, 43, 41, 3, messages(&[129, 192]));
    let first = search(&f, &request(ImapMetadataFilter::All, 1, None)).unwrap();
    assert_eq!(uids(&first), [199]);
    let token = first.next_cursor().unwrap();
    assert_eq!(
        (
            token.source_kind(),
            token.source_position(),
            token.resume_uid()
        ),
        (Some(ImapMetadataSource::Preview), 1, 199)
    );
    let mut next = first.next_cursor().cloned();
    let mut all = uids(&first);
    let expiry = token.expires_at_utc_ms();
    let mut visited = 0;
    while let Some(token) = next {
        assert_eq!(token.expires_at_utc_ms(), expiry);
        let p = search(&f, &request(ImapMetadataFilter::All, 1, Some(token))).unwrap();
        all.extend(uids(&p));
        next = p.next_cursor().cloned();
        visited += 1;
        assert!(visited < 8);
    }
    assert_eq!(all, [199, 193, 192, 129, 64, 1]);
    let cursor = f
        .store()
        .load_imap_uid_cursor("acct", &crate::uid_reconcile_jobs::tests::mailbox())
        .unwrap()
        .unwrap();
    assert_eq!((cursor.version(), cursor.covered_through_uid()), (1, 64));
}

#[test]
fn sync01_mail03_preview_examine_is_observation_but_pause_is_not_new_observation() {
    let f = Fixture::new();
    assert_eq!(
        search(&f, &request(ImapMetadataFilter::All, 16, None))
            .unwrap()
            .observed_at_utc_ms(),
        None
    );
    create(&f, 41, 200, 0);
    let p = search(&f, &request(ImapMetadataFilter::All, 16, None)).unwrap();
    assert!(p.items().is_empty());
    assert!(p.next_cursor().is_none());
    let at = p.observed_at_utc_ms().unwrap();
    pause(&f, 42, 41, 1);
    assert_eq!(
        search(&f, &request(ImapMetadataFilter::All, 16, None))
            .unwrap()
            .observed_at_utc_ms(),
        Some(at)
    );
    assert_eq!(f.count("imap_uid_cursors"), 0);
}

#[test]
fn sync01_mail03_preview_expels_m1_and_m2_binds_plan_base_filter_limit_and_cursor_parts() {
    let f = Fixture::new();
    append(&f, 0, 0, 200, &[1, 3], "base");
    let old = search(&f, &request(ImapMetadataFilter::All, 1, None))
        .unwrap()
        .next_cursor()
        .cloned();
    create(&f, 41, 200, 0);
    assert_eq!(
        search(&f, &request(ImapMetadataFilter::All, 1, old)),
        Err(ErrorCode::CursorExpired)
    );
    resume(&f, 42, 41, 1, messages(&[193, 199]));
    let p = search(&f, &request(ImapMetadataFilter::All, 1, None)).unwrap();
    let cursor = p.next_cursor().unwrap();
    for r in [
        request(ImapMetadataFilter::Unread, 1, Some(cursor.clone())),
        request(ImapMetadataFilter::All, 2, Some(cursor.clone())),
    ] {
        assert_eq!(search(&f, &r), Err(ErrorCode::CursorExpired));
    }
    for tail in [
        format!("p:1:198:{}", cursor.expires_at_utc_ms()),
        format!("p:2:199:{}", cursor.expires_at_utc_ms()),
        format!("b:1:199:{}", cursor.expires_at_utc_ms()),
        "p:1:199:1".into(),
    ] {
        let forged = ImapMetadataSearchCursor::parse(&format!(
            "m2:{}:{tail}",
            hantong_contracts::wire_hex::encode(cursor.digest())
        ))
        .unwrap();
        assert_eq!(
            search(&f, &request(ImapMetadataFilter::All, 1, Some(forged))),
            Err(ErrorCode::CursorExpired)
        );
    }
    pause(&f, 43, 41, 3);
    assert_eq!(
        search(
            &f,
            &request(ImapMetadataFilter::All, 1, Some(cursor.clone()))
        ),
        Err(ErrorCode::CursorExpired)
    );
    let token = search(&f, &request(ImapMetadataFilter::All, 1, None))
        .unwrap()
        .next_cursor()
        .cloned();
    append(&f, 1, 64, 200, &[65], "base-two");
    assert_eq!(
        search(&f, &request(ImapMetadataFilter::All, 1, token)),
        Err(ErrorCode::CursorExpired)
    );
    let token = search(&f, &request(ImapMetadataFilter::All, 1, None))
        .unwrap()
        .next_cursor()
        .cloned();
    create(&f, 44, 200, 1);
    assert_eq!(
        search(&f, &request(ImapMetadataFilter::All, 1, token)),
        Err(ErrorCode::CursorExpired)
    );
}

#[test]
fn sync01_mail03_base_prefix_excludes_preview_gaps_and_absent_but_preserves_ids() {
    let f = Fixture::new();
    create(&f, 41, 66, 0);
    resume(&f, 42, 41, 1, messages(&[65]));
    resume(&f, 43, 41, 3, messages(&[1, 3, 5]));
    let first = search(&f, &request(ImapMetadataFilter::All, 16, None)).unwrap();
    let second = search(
        &f,
        &request(ImapMetadataFilter::All, 16, first.next_cursor().cloned()),
    )
    .unwrap();
    let id = second
        .items()
        .iter()
        .find(|m| m.uid() == 1)
        .unwrap()
        .internal_message_id()
        .to_owned();
    append(&f, 0, 0, 66, &[1, 5], "adopt-base");
    reconcile(&f, 51, 1, &[(1, &["\\Flagged"])]);
    let p = search(&f, &request(ImapMetadataFilter::All, 16, None)).unwrap();
    assert_eq!(uids(&p), [65]);
    assert_eq!(
        p.next_cursor().unwrap().source_kind(),
        Some(ImapMetadataSource::Base)
    );
    let p = search(
        &f,
        &request(ImapMetadataFilter::All, 16, p.next_cursor().cloned()),
    )
    .unwrap();
    assert_eq!(uids(&p), [1]);
    assert_eq!(p.items()[0].internal_message_id(), id);
    assert!(p.items()[0].is_favorite());
    assert!(p.next_cursor().is_none());
}

#[test]
fn sync01_mail03_preview_empty_filtered_and_page_boundary_continue_without_overread() {
    for first in [messages(&[]), messages(&[199])] {
        let f = Fixture::new();
        create(&f, 41, 200, 0);
        resume(&f, 42, 41, 1, first);
        resume(&f, 43, 41, 3, vec![metadata(129, &["\\Flagged"])]);
        let p = search(&f, &request(ImapMetadataFilter::Unread, 16, None)).unwrap();
        assert!(p.items().is_empty());
        assert_eq!(p.next_cursor().unwrap().source_position(), 2);
        let p = search(
            &f,
            &request(ImapMetadataFilter::Unread, 16, p.next_cursor().cloned()),
        )
        .unwrap();
        assert_eq!(uids(&p), [129]);
        assert!(p.next_cursor().is_none());
    }
}

#[test]
fn sync01_mail03_preview_decodes_only_selected_page_and_keeps_no_history_guard() {
    let f = Fixture::new();
    create(&f, 41, 200, 0);
    resume(&f, 42, 41, 1, messages(&[199]));
    resume(&f, 43, 41, 3, messages(&[129]));
    f.sql("UPDATE imap_initial_preview_pages SET page_json='{}' WHERE ordinal=1");
    let p = search(&f, &request(ImapMetadataFilter::All, 16, None)).unwrap();
    assert_eq!(uids(&p), [199]);
    assert_eq!(
        search(
            &f,
            &request(ImapMetadataFilter::All, 16, p.next_cursor().cloned())
        ),
        Err(ErrorCode::InternalError)
    );
    let f = Fixture::new();
    create(&f, 41, 200, 0);
    append(&f, 0, 0, 200, &[1], "base");
    f.sql("PRAGMA foreign_keys=OFF; DELETE FROM imap_uid_cursors; PRAGMA foreign_keys=ON;");
    assert_eq!(
        search(&f, &request(ImapMetadataFilter::All, 16, None)),
        Err(ErrorCode::InternalError)
    );
}

#[test]
fn sync01_mail03_preview_selected_page_hash_children_and_ids_are_strict() {
    for sql in [
        "UPDATE imap_initial_preview_pages SET page_hash=zeroblob(32)",
        "DELETE FROM imap_initial_preview_messages",
        "UPDATE imap_initial_preview_messages SET metadata_hash=zeroblob(32)",
        "UPDATE imap_initial_preview_messages SET internal_message_id='msg_'||printf('%064d',0)",
    ] {
        let f = Fixture::new();
        create(&f, 41, 200, 0);
        resume(&f, 42, 41, 1, messages(&[199]));
        f.sql(sql);
        assert_eq!(
            search(&f, &request(ImapMetadataFilter::All, 16, None)),
            Err(ErrorCode::InternalError),
            "{sql}"
        );
    }
}

#[test]
fn sync01_mail03_preview_large_headers_keep_rejected_candidate_and_expiry() {
    use hantong_domain::imap_sync::{ImapEnvelope, ImapEnvelopeAddress, ImapEnvelopeInput};
    let f = Fixture::new();
    create(&f, 41, 200, 0);
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
    resume(
        &f,
        42,
        41,
        1,
        (193..=195)
            .map(|uid| {
                ImapMessageMetadata::new(uid, vec![], 1767225600000, 12, envelope.clone()).unwrap()
            })
            .collect(),
    );
    let mut next = None;
    let mut expiry = None;
    for uid in [195, 194, 193] {
        let p = search(&f, &request(ImapMetadataFilter::All, 32, next)).unwrap();
        assert_eq!(uids(&p), [uid]);
        next = p.next_cursor().cloned();
        if let Some(c) = &next {
            assert_eq!(c.resume_uid(), uid);
            assert_eq!(
                *expiry.get_or_insert(c.expires_at_utc_ms()),
                c.expires_at_utc_ms()
            );
        }
    }
    assert!(next.is_none());
}

#[test]
fn sync01_mail03_new_base_namespace_hides_stale_preview_and_keeps_m2_binding() {
    use crate::uid_recovery_jobs::{UidRecoveryAdmission, UidRecoveryStart};
    use hantong_contracts::uid_recovery::UidRecoveryRequest;
    use hantong_domain::{
        imap_recovery::ImapUidRecoveryPage,
        imap_sync::{ImapUidPage, ImapUidPageRequest},
    };
    let f = Fixture::new();
    append(&f, 0, 0, 200, &[1], "base");
    create(&f, 41, 200, 0);
    resume(&f, 42, 41, 1, messages(&[193, 199]));
    let old = search(&f, &request(ImapMetadataFilter::All, 1, None))
        .unwrap()
        .next_cursor()
        .cloned();
    let raw = UidRecoveryRequest::new(
        "acct",
        identity(b"INBOX"),
        "recover-new",
        "recover-new",
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
        "recover-new",
        "recover-new",
        1,
        1,
        &hash,
    )
    .unwrap();
    f.begin_for(51, 9);
    assert!(matches!(
        f.store()
            .admit_uid_recovery([51; 32], &r, UidQueueAdmission::Reserved)
            .unwrap(),
        UidRecoveryAdmission::Admitted(_)
    ));
    let UidRecoveryStart::Ready(job) = f.store().start_uid_recovery([51; 32]).unwrap() else {
        panic!("expected recovery")
    };
    let request = job.fetch_request();
    let page = ImapUidPage::new(
        ImapUidPageRequest::new("acct", 1, request.mailbox().clone(), Some(9), 0).unwrap(),
        9,
        11,
        messages(&[1, 3]),
    )
    .unwrap();
    f.store()
        .complete_uid_recovery_success(
            [51; 32],
            &ImapUidRecoveryPage::new(request.clone(), page).unwrap(),
        )
        .unwrap();
    assert_eq!(
        search(&f, &super::request(ImapMetadataFilter::All, 1, old)),
        Err(ErrorCode::CursorExpired)
    );
    let p = search(&f, &super::request(ImapMetadataFilter::All, 1, None)).unwrap();
    assert_eq!(uids(&p), [3]);
    assert!(!p.binding_current());
    assert_eq!(
        p.next_cursor().unwrap().source_kind(),
        Some(ImapMetadataSource::Base)
    );
    let p = search(
        &f,
        &super::request(ImapMetadataFilter::All, 1, p.next_cursor().cloned()),
    )
    .unwrap();
    assert_eq!(uids(&p), [1]);
    assert!(!p.binding_current());
    assert!(p.next_cursor().is_none());
}
