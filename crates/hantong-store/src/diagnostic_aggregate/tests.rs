use super::*;
use crate::uid_reconcile_jobs::tests::Fixture;
use hantong_contracts::diagnostic_aggregate::{DiagnosticAggregateOwner, DiagnosticSection};
use hantong_contracts::diagnostic_aggregate::{
    DiagnosticAuditState, DiagnosticJobKind, DiagnosticJobState, DiagnosticTool,
};

fn id(n: u64) -> [u8; 32] {
    let mut id = [0; 32];
    id[..8].copy_from_slice(&n.to_be_bytes());
    id
}
fn audit(
    f: &Fixture,
    n: u64,
    owner: u8,
    tool: DiagnosticTool,
    accounts: &[&str],
    at: i64,
    terminal: Option<Result<(), ErrorCode>>,
) {
    f.store().with_connection(|c| {
        c.execute("INSERT INTO audit_starts(execution_id,registration_id,profile_epoch,tool_code,started_at_utc_ms,client_kind,display_label) VALUES(?1,?2,1,?3,?4,'cli','private-label@example.invalid')",rusqlite::params![id(n).as_slice(),[owner;16].as_slice(),tool.as_str(),at])?;
        for account in accounts { c.execute("INSERT INTO audit_start_accounts VALUES(?1,?2)",rusqlite::params![id(n).as_slice(),account])?; }
        c.execute("UPDATE audit_capacity SET retained_executions=retained_executions+1,retained_account_refs=retained_account_refs+?1",[accounts.len() as i64])?;
        let error=terminal.and_then(Result::err);
        if terminal.is_some() { c.execute("INSERT INTO audit_terminals VALUES(?1,?2,?3,?4,NULL)",rusqlite::params![id(n).as_slice(),at+1,if error.is_some(){"returned_error"}else{"returned_ok"},error.map(ErrorCode::as_str)])?; }
        Ok(())
    }).unwrap();
}
fn query(
    owner: u8,
    epoch: u64,
    limit: u32,
    sections: &[DiagnosticSection],
) -> DiagnosticAggregateQuery {
    DiagnosticAggregateQuery::new(
        DiagnosticAggregateOwner::new([owner; 16], epoch, None).unwrap(),
        100,
        1000,
        limit,
        sections,
    )
    .unwrap()
}
fn collect(f: &Fixture, q: &DiagnosticAggregateQuery) -> Result<DiagnosticAggregate, ErrorCode> {
    let changes = f
        .store()
        .with_connection(|c| Ok(c.total_changes()))
        .unwrap();
    let result = f.store().collect_diagnostic_aggregate(q);
    assert_eq!(
        f.store()
            .with_connection(|c| Ok(c.total_changes()))
            .unwrap(),
        changes
    );
    result
}

type StoredRows = Vec<(String, Vec<Vec<rusqlite::types::Value>>)>;
fn all_rows(f: &Fixture) -> StoredRows {
    f.store()
        .with_connection(|c| {
            let tables = {
                let mut s =
                    c.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;
                s.query_map([], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            tables
                .into_iter()
                .map(|table| {
                    let mut s = c.prepare(&format!("SELECT * FROM \"{table}\" ORDER BY rowid"))?;
                    let width = s.column_count();
                    let values = s
                        .query_map([], |r| {
                            (0..width)
                                .map(|n| r.get(n))
                                .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
                        })?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    Ok((table, values))
                })
                .collect()
        })
        .unwrap()
}

#[test]
fn ops05_diagnostic_empty_store_has_available_zero_counts_without_writes() {
    let f = Fixture::new();
    let rows = all_rows(&f);
    let query = DiagnosticAggregateQuery::new(
        DiagnosticAggregateOwner::new([9; 16], 1, None).unwrap(),
        0,
        1000,
        100,
        &DiagnosticSection::ALL,
    )
    .unwrap();
    let before = f
        .store()
        .with_connection(|c| Ok(c.total_changes()))
        .unwrap();
    let value = f.store().collect_diagnostic_aggregate(&query).unwrap();
    assert_eq!(value.window().included_executions(), 0);
    assert!(!value.window().truncated());
    assert_eq!(value.sync().unwrap().count(), 0);
    assert_eq!(value.recent_work().unwrap().count(), 0);
    assert_eq!(value.errors().unwrap().count(), 0);
    assert_eq!(all_rows(&f), rows);
    assert_eq!(
        f.store()
            .with_connection(|c| Ok(c.total_changes()))
            .unwrap(),
        before
    );
}

#[test]
fn ops05_diagnostic_window_uses_start_cohort_and_sequence_limit() {
    let f = Fixture::new();
    audit(
        &f,
        1,
        9,
        DiagnosticTool::AccountsList,
        &["acct"],
        99,
        Some(Ok(())),
    );
    audit(
        &f,
        2,
        9,
        DiagnosticTool::AccountsList,
        &["acct"],
        100,
        Some(Ok(())),
    );
    audit(
        &f,
        3,
        9,
        DiagnosticTool::SyncPage,
        &["acct"],
        200,
        Some(Err(ErrorCode::Offline)),
    );
    audit(&f, 4, 9, DiagnosticTool::SyncPage, &["acct"], 200, None);
    audit(
        &f,
        5,
        9,
        DiagnosticTool::AccountsList,
        &["acct"],
        1000,
        Some(Ok(())),
    );
    let q = query(9, 1, 2, &DiagnosticSection::ALL);
    let a = collect(&f, &q).unwrap();
    assert_eq!(a.window().included_executions(), 2);
    assert!(a.window().truncated());
    assert_eq!(
        a.recent_work()
            .unwrap()
            .state_count(DiagnosticAuditState::Unresolved),
        1
    );
    assert_eq!(a.errors().unwrap().count(), 1);
    assert_eq!(a.sync().unwrap().count(), 0);
    let a = collect(&f, &query(9, 1, 3, &DiagnosticSection::ALL)).unwrap();
    assert_eq!(a.window().included_executions(), 3);
    assert!(!a.window().truncated());
    f.sql("UPDATE audit_terminals SET completed_at_utc_ms=2000 WHERE error_code='OFFLINE'");
    assert_eq!(
        collect(&f, &query(9, 1, 3, &DiagnosticSection::ALL))
            .unwrap()
            .errors()
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn sec08_diagnostic_owner_scope_and_old_epoch_filter_whole_executions() {
    let f = Fixture::new();
    f.add_registration(10);
    audit(
        &f,
        1,
        9,
        DiagnosticTool::AccountsList,
        &["acct"],
        100,
        Some(Ok(())),
    );
    audit(
        &f,
        2,
        10,
        DiagnosticTool::SyncPage,
        &["acct"],
        200,
        Some(Err(ErrorCode::Offline)),
    );
    audit(
        &f,
        3,
        9,
        DiagnosticTool::AccountsList,
        &["acct", "removed-account"],
        300,
        Some(Ok(())),
    );
    audit(
        &f,
        4,
        9,
        DiagnosticTool::DiagnosticsSnapshot,
        &[],
        400,
        Some(Ok(())),
    );
    f.sql("UPDATE client_registrations SET profile_epoch=2 WHERE registration_id=CAST(X'09090909090909090909090909090909' AS BLOB)");
    let a = collect(&f, &query(9, 2, 100, &DiagnosticSection::ALL)).unwrap();
    assert_eq!(a.window().included_executions(), 2);
    assert_eq!(a.errors().unwrap().count(), 0);
    assert_eq!(
        a.recent_work()
            .unwrap()
            .tool_count(DiagnosticTool::DiagnosticsSnapshot),
        1
    );
    assert_eq!(
        collect(&f, &query(9, 1, 100, &DiagnosticSection::ALL)),
        Err(ErrorCode::AuthRequired)
    );
    f.sql("DELETE FROM client_registration_accounts WHERE registration_id=X'09090909090909090909090909090909'");
    let a = collect(&f, &query(9, 2, 100, &DiagnosticSection::ALL)).unwrap();
    assert_eq!(a.window().included_executions(), 1);
    assert_eq!(a.sync().unwrap().count(), 0);
}

#[test]
fn sec08_diagnostic_rejects_inactive_unknown_and_future_requests() {
    let f = Fixture::new();
    assert_eq!(
        collect(&f, &query(33, 1, 1, &[DiagnosticSection::Sync])),
        Err(ErrorCode::ClientNotRegistered)
    );
    for state in ["provisioning", "suspended", "revoked"] {
        f.sql(&format!("UPDATE client_registrations SET state='{state}',revoked_at_utc_ms={} WHERE registration_id=X'09090909090909090909090909090909'",if state=="revoked"{"2"}else{"NULL"}));
        assert_eq!(
            collect(&f, &query(9, 1, 1, &[DiagnosticSection::Sync])),
            Err(ErrorCode::ClientSuspended)
        );
    }
    f.sql("UPDATE client_registrations SET state='active',revoked_at_utc_ms=NULL");
    let q = DiagnosticAggregateQuery::new(
        DiagnosticAggregateOwner::new([9; 16], 1, None).unwrap(),
        i64::MAX - 1,
        i64::MAX,
        1,
        &[DiagnosticSection::Sync],
    )
    .unwrap();
    assert_eq!(collect(&f, &q), Err(ErrorCode::InvalidInput));
}

#[test]
fn sec08_diagnostic_excludes_only_its_current_unresolved_global_audit() {
    let f = Fixture::new();
    audit(
        &f,
        1,
        9,
        DiagnosticTool::DiagnosticsSnapshot,
        &[],
        100,
        Some(Ok(())),
    );
    audit(
        &f,
        2,
        9,
        DiagnosticTool::DiagnosticsSnapshot,
        &[],
        200,
        None,
    );
    let q = DiagnosticAggregateQuery::new(
        DiagnosticAggregateOwner::new([9; 16], 1, Some(id(2))).unwrap(),
        100,
        1000,
        100,
        &DiagnosticSection::ALL,
    )
    .unwrap();
    let a = collect(&f, &q).unwrap();
    assert_eq!(a.window().included_executions(), 1);
    assert_eq!(
        a.recent_work()
            .unwrap()
            .state_count(DiagnosticAuditState::ReturnedOk),
        1
    );
    for sql in [
        "UPDATE audit_starts SET tool_code='app.settings.display.get'",
        "UPDATE audit_starts SET profile_epoch=2",
        "INSERT INTO audit_terminals SELECT execution_id,500,'returned_ok',NULL,NULL FROM audit_starts",
    ] {
        let f = Fixture::new();
        audit(
            &f,
            2,
            9,
            DiagnosticTool::DiagnosticsSnapshot,
            &[],
            200,
            None,
        );
        f.sql(sql);
        assert_eq!(collect(&f, &q), Err(ErrorCode::InternalError));
    }
}

#[test]
fn sec08_diagnostic_rejects_scalar_corruption_without_reading_sensitive_payloads() {
    for sql in [
        "DELETE FROM audit_start_accounts",
        "UPDATE audit_starts SET tool_code='app.settings.display.get'",
        "UPDATE audit_terminals SET completed_at_utc_ms=1",
        "PRAGMA ignore_check_constraints=ON; UPDATE audit_terminals SET error_code='private exception C:/secret'; PRAGMA ignore_check_constraints=OFF",
        "PRAGMA ignore_check_constraints=ON; UPDATE audit_starts SET tool_code='private@example.invalid'; PRAGMA ignore_check_constraints=OFF",
        "PRAGMA ignore_check_constraints=ON; UPDATE audit_terminals SET completed_at_utc_ms=9223372036854775807; PRAGMA ignore_check_constraints=OFF",
    ] {
        let f = Fixture::new();
        audit(
            &f,
            1,
            9,
            DiagnosticTool::AccountsList,
            &["acct"],
            100,
            Some(Err(ErrorCode::Offline)),
        );
        f.sql(sql);
        assert_eq!(
            collect(&f, &query(9, 1, 100, &DiagnosticSection::ALL)),
            Err(ErrorCode::InternalError),
            "{sql}"
        );
    }
}

#[test]
fn sec08_diagnostic_result_is_identifier_free_and_unchanged_after_new_terminal() {
    let f = Fixture::new();
    audit(&f, 1, 9, DiagnosticTool::AccountsList, &["acct"], 100, None);
    let q = query(9, 1, 100, &[DiagnosticSection::RecentWork]);
    let first = collect(&f, &q).unwrap();
    let frozen = first.clone();
    f.sql("INSERT INTO audit_terminals SELECT execution_id,200,'returned_ok',NULL,4294967295 FROM audit_starts");
    let second = collect(&f, &q).unwrap();
    assert_eq!(first, frozen);
    assert_eq!(
        first
            .recent_work()
            .unwrap()
            .state_count(DiagnosticAuditState::Unresolved),
        1
    );
    assert_eq!(
        second
            .recent_work()
            .unwrap()
            .state_count(DiagnosticAuditState::ReturnedOk),
        1
    );
    assert!(first.sync().is_none() && first.errors().is_none());
    let shown = format!("{second:?}");
    for secret in ["acct", "private-label", "@", "4294967295"] {
        assert!(!shown.contains(secret));
    }
}

#[test]
fn ops05_diagnostic_last_error_uses_completion_time_then_audit_sequence() {
    let f = Fixture::new();
    audit(
        &f,
        1,
        9,
        DiagnosticTool::AccountsList,
        &["acct"],
        100,
        Some(Err(ErrorCode::Offline)),
    );
    audit(
        &f,
        2,
        9,
        DiagnosticTool::AccountsList,
        &["acct"],
        200,
        Some(Err(ErrorCode::Conflict)),
    );
    f.sql("UPDATE audit_terminals SET completed_at_utc_ms=500");
    let q = query(9, 1, 100, &[DiagnosticSection::Errors]);
    assert_eq!(
        collect(&f, &q).unwrap().errors().unwrap().last_error_code(),
        Some(ErrorCode::Conflict)
    );
    f.sql("UPDATE audit_terminals SET completed_at_utc_ms=501 WHERE error_code='OFFLINE'");
    let a = collect(&f, &q).unwrap();
    assert_eq!(
        a.errors().unwrap().last_error_code(),
        Some(ErrorCode::Offline)
    );
    assert_eq!(a.errors().unwrap().count(), 2);
}

#[test]
fn ops05_diagnostic_max_sample_is_exact_and_reports_only_eligible_truncation() {
    let f = Fixture::new();
    f.sql("WITH RECURSIVE n(x) AS(SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<1001) INSERT INTO audit_starts(execution_id,registration_id,profile_epoch,tool_code,started_at_utc_ms,client_kind,display_label) SELECT CAST(printf('%032x',x) AS BLOB),X'09090909090909090909090909090909',1,'app.settings.display.get',100,'cli','fixture' FROM n; UPDATE audit_capacity SET retained_executions=1001");
    let a = collect(&f, &query(9, 1, 1000, &DiagnosticSection::ALL)).unwrap();
    assert_eq!(a.window().included_executions(), 1000);
    assert!(a.window().truncated());
    assert_eq!(
        a.recent_work()
            .unwrap()
            .state_count(DiagnosticAuditState::Unresolved),
        1000
    );
    assert_eq!(a.sync().unwrap().count(), 0);
}

#[test]
fn ops05_diagnostic_sparse_owner_scans_retention_bound_without_allocating_history() {
    let f = Fixture::new();
    f.add_registration(10);
    audit(
        &f,
        1,
        9,
        DiagnosticTool::DisplaySettingsGet,
        &[],
        100,
        Some(Ok(())),
    );
    f.sql("WITH RECURSIVE n(x) AS(SELECT 2 UNION ALL SELECT x+1 FROM n WHERE x<100000) INSERT INTO audit_starts(execution_id,registration_id,profile_epoch,tool_code,started_at_utc_ms,client_kind,display_label) SELECT CAST(printf('%032x',x) AS BLOB),X'0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a',1,'app.settings.display.get',100,'cli','fixture' FROM n; UPDATE audit_capacity SET retained_executions=100000");
    let q = query(9, 1, 1, &[DiagnosticSection::RecentWork]);
    let a = collect(&f, &q).unwrap();
    assert_eq!(a.window().included_executions(), 1);
    assert!(!a.window().truncated());
    f.sql("INSERT INTO audit_starts(execution_id,registration_id,profile_epoch,tool_code,started_at_utc_ms,client_kind,display_label) VALUES(CAST(printf('%032x',100001) AS BLOB),X'0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a',1,'app.settings.display.get',100,'cli','fixture')");
    assert_eq!(collect(&f, &q), Err(ErrorCode::InternalError));
}

#[test]
fn sec08_diagnostic_oversized_account_set_cannot_hide_behind_scope_filter() {
    let f = Fixture::new();
    audit(&f, 1, 9, DiagnosticTool::AccountsList, &["acct"], 100, None);
    f.sql("WITH RECURSIVE n(x) AS(SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<128) INSERT INTO audit_start_accounts SELECT execution_id,printf('not-granted-%d',x) FROM n,audit_starts; UPDATE audit_capacity SET retained_account_refs=129");
    assert_eq!(
        collect(&f, &query(9, 1, 100, &DiagnosticSection::ALL)),
        Err(ErrorCode::InternalError)
    );
}

fn seed_jobs(f: &Fixture) {
    use crate::uid_page_jobs::UidQueueAdmission;
    use crate::uid_reconcile_jobs::tests::{append, append_request, identity, reconcile, request};
    use hantong_contracts::{
        audit::{AuditBegin, AuditTool},
        folders::FolderRefreshRequest,
        uid_recovery::UidRecoveryRequest,
    };
    append(f, 0, 0, 200, &[1], "base");
    f.sql("INSERT INTO client_registration_tools VALUES(X'09090909090909090909090909090909','mail.folders.refresh')");
    f.store()
        .begin_audit(
            &AuditBegin::new(
                [30; 32],
                [9; 16],
                1,
                AuditTool::FoldersRefresh,
                vec!["acct".into()],
            )
            .unwrap(),
        )
        .unwrap();
    let raw = FolderRefreshRequest::new("acct", "folder", "folder", 1, &"0".repeat(64)).unwrap();
    let hash = hantong_contracts::wire_hex::encode(
        &hantong_platform::sha256(&raw.intent_bytes()).unwrap(),
    );
    let r = FolderRefreshRequest::new("acct", "folder", "folder", 1, &hash).unwrap();
    assert!(matches!(
        f.store()
            .admit_folder_refresh(
                [30; 32],
                &r,
                crate::folder_refresh::FolderQueueAdmission::Reserved
            )
            .unwrap(),
        crate::folder_refresh::FolderRefreshAdmission::Admitted(_)
    ));
    f.begin_for(31, 9);
    f.store()
        .admit_uid_page(
            [31; 32],
            &append_request("append", "append", 1),
            UidQueueAdmission::Reserved,
        )
        .unwrap();
    assert!(matches!(
        f.store().start_uid_page([31; 32]).unwrap(),
        crate::uid_page_jobs::UidPageStart::Ready(_)
    ));
    reconcile(f, 32, 1, &[(1, &["\\Seen"])]);
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
    let r = UidRecoveryRequest::new(
        "acct",
        identity(b"INBOX"),
        "recover",
        "recover",
        1,
        1,
        &hash,
    )
    .unwrap();
    f.begin_for(33, 9);
    assert!(matches!(
        f.store()
            .admit_uid_recovery([33; 32], &r, UidQueueAdmission::Full)
            .unwrap(),
        crate::uid_recovery_jobs::UidRecoveryAdmission::Failed(_)
    ));
    // A replay has its own audit but keeps the original durable job.
    assert!(matches!(
        f.admit(34, &request("reconcile-32", 1), UidQueueAdmission::Full),
        crate::uid_reconcile_jobs::UidReconcileAdmission::Existing(_)
    ));
    audit(
        f,
        35,
        9,
        DiagnosticTool::SyncPage,
        &["acct"],
        100,
        Some(Ok(())),
    );
    // The collector validates scalar state, not the source-page timeline.
    f.sql("UPDATE audit_starts SET started_at_utc_ms=100; UPDATE audit_terminals SET completed_at_utc_ms=300");
    for table in [
        "folder_refresh_requests",
        "imap_uid_page_requests",
        "imap_uid_reconcile_requests",
        "imap_uid_recovery_requests",
    ] {
        f.sql(&format!("UPDATE {table} SET admitted_at_utc_ms=100,started_at_utc_ms=CASE WHEN started_at_utc_ms IS NULL THEN NULL ELSE 200 END,completed_at_utc_ms=CASE WHEN completed_at_utc_ms IS NULL THEN NULL ELSE 300 END"));
    }
}

#[test]
fn ops05_diagnostic_counts_four_job_kinds_once_and_keeps_replay_audits_separate() {
    let f = Fixture::new();
    seed_jobs(&f);
    let q = query(9, 1, 100, &DiagnosticSection::ALL);
    let a = collect(&f, &q).unwrap();
    assert_eq!(a.window().included_executions(), 6);
    let sync = a.sync().unwrap();
    assert_eq!(sync.count(), 4);
    for state in DiagnosticJobState::ALL {
        assert_eq!(sync.state_count(state), 1);
    }
    for kind in DiagnosticJobKind::ALL {
        assert_eq!(sync.kind_count(kind), 1);
    }
    assert_eq!(a.errors().unwrap().count(), 1);
    assert_eq!(a.errors().unwrap().count_for(ErrorCode::RateLimited), 1);
    assert_eq!(
        a.recent_work()
            .unwrap()
            .state_count(DiagnosticAuditState::Unresolved),
        2
    );
    assert_eq!(
        a.recent_work()
            .unwrap()
            .state_count(DiagnosticAuditState::ReturnedOk),
        3
    );
    f.sql("UPDATE imap_uid_page_operations SET page_json='private-mail-subject@example.invalid'; UPDATE imap_uid_reconcile_requests SET result_json='private-payload'");
    assert_eq!(collect(&f, &q).unwrap().sync(), a.sync());
}

#[test]
fn sec08_diagnostic_job_mismatches_fail_only_when_sync_source_is_selected() {
    for sql in [
        "UPDATE imap_uid_recovery_requests SET error_code='OFFLINE'",
        "UPDATE imap_uid_page_requests SET started_at_utc_ms=99,admitted_at_utc_ms=99",
        "INSERT INTO audit_terminals SELECT execution_id,300,'returned_ok',NULL,NULL FROM imap_uid_page_requests",
        "UPDATE audit_starts SET tool_code='mail.accounts.list' WHERE execution_id IN (SELECT execution_id FROM imap_uid_reconcile_requests)",
        "INSERT INTO folder_refresh_requests(execution_id,operation_id,account_id,idempotency_key,intent_hash,expected_generation,bound_account_version,bound_endpoint_version,bound_endpoint_hash,state,admitted_at_utc_ms,completed_at_utc_ms,error_code) SELECT execution_id,'collision','acct','collision',zeroblob(32),1,2,1,zeroblob(32),'failed',100,300,error_code FROM imap_uid_recovery_requests",
    ] {
        let f = Fixture::new();
        seed_jobs(&f);
        f.sql(sql);
        assert!(collect(&f, &query(9, 1, 100, &[DiagnosticSection::RecentWork])).is_ok());
        assert_eq!(
            collect(&f, &query(9, 1, 100, &[DiagnosticSection::Sync])),
            Err(ErrorCode::InternalError),
            "{sql}"
        );
    }
}
