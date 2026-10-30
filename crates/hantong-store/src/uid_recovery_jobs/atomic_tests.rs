type RecoveryAtomicRows = Vec<(&'static str, Vec<Vec<rusqlite::types::Value>>)>;

fn recovery_atomic_rows(f: &Fixture) -> RecoveryAtomicRows {
    f.store()
        .with_connection(|c| {
            [
                "imap_uid_epochs",
                "imap_uid_cursors",
                "imap_uid_page_operations",
                "imap_uid_messages",
                "imap_uid_page_requests",
                "imap_uid_recovery_requests",
                "imap_uid_reconcile_heads",
                "imap_uid_reconcile_requests",
                "imap_uid_reconcile_messages",
                "audit_starts",
                "audit_start_accounts",
                "audit_terminals",
            ]
            .into_iter()
            .map(|table| {
                let mut statement =
                    c.prepare(&format!("SELECT rowid,* FROM {table} ORDER BY rowid"))?;
                let width = statement.column_count();
                let rows = statement
                    .query_map([], |row| {
                        (0..width)
                            .map(|column| row.get::<_, rusqlite::types::Value>(column))
                            .collect::<rusqlite::Result<Vec<_>>>()
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok((table, rows))
            })
            .collect()
        })
        .unwrap()
}

fn recovery_atomic_fixture() -> Fixture {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1, 3], "base");
    reconcile(&f, 21, 1, &[(1, &["\\Answered"])]);
    f
}

fn recovery_atomic_page(job: &UidRecoveryJob) -> ImapUidRecoveryPage {
    let request = job.fetch_request();
    ImapUidRecoveryPage::new(
        request.clone(),
        ImapUidPage::new(
            ImapUidPageRequest::new(
                request.account_id(),
                request.endpoint_version(),
                request.mailbox().clone(),
                Some(9),
                0,
            )
            .unwrap(),
            9,
            11,
            vec![metadata(1, &["\\Flagged"]), metadata(3, &[])],
        )
        .unwrap(),
    )
    .unwrap()
}

fn assert_recovery_atomic_unresolved(f: &Fixture) {
    let receipt = f
        .store()
        .load_uid_recovery_receipt(
            [9; 16],
            &UidPageStatusRequest::new("acct", [31; 32]).unwrap(),
        )
        .unwrap();
    assert_eq!(receipt.state(), UidPageState::Unresolved);
    assert!(receipt.result().is_none());
    assert!(receipt.error().is_none());
    assert!(
        f.store()
            .load_audit([31; 32])
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
    let cursor = f
        .store()
        .load_imap_uid_cursor("acct", &mailbox())
        .unwrap()
        .unwrap();
    assert_eq!((cursor.version(), cursor.uid_validity()), (1, 7));
    assert_eq!(cursor.covered_through_uid(), 10);
    for uid in [1, 3] {
        assert_eq!(
            f.store()
                .load_imap_uid_metadata("acct", &mailbox(), 7, uid)
                .unwrap()
                .unwrap(),
            metadata(uid, &["\\Seen"])
        );
        assert!(
            f.store()
                .load_imap_uid_metadata("acct", &mailbox(), 9, uid)
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        f.store()
            .load_imap_uid_observation("acct", &mailbox(), 7, 1)
            .unwrap()
            .unwrap()
            .flags()
            .unwrap(),
        ["\\answered"]
    );
    assert!(
        f.store()
            .load_imap_uid_observation("acct", &mailbox(), 7, 3)
            .unwrap()
            .unwrap()
            .remote_absent()
    );
}

#[test]
fn sync02_recovery_atomic_success_rolls_back_every_write_on_abort_and_ignore() {
    for action in ["ABORT,'synthetic'", "IGNORE"] {
        for (table, event, predicate) in [
            ("imap_uid_page_operations", "INSERT", ""),
            ("imap_uid_cursors", "UPDATE", ""),
            ("imap_uid_messages", "INSERT", "WHEN NEW.uid=3"),
            ("imap_uid_epochs", "UPDATE", ""),
            ("imap_uid_epochs", "INSERT", ""),
            ("imap_uid_recovery_requests", "UPDATE", ""),
            ("audit_terminals", "INSERT", ""),
        ] {
            let f = recovery_atomic_fixture();
            admit(&f, 31, 1);
            let job = start(&f, 31);
            let page = recovery_atomic_page(&job);
            let before = recovery_atomic_rows(&f);
            // The second metadata row fails after the first row has been written.
            f.sql(&format!(
                "CREATE TRIGGER recovery_atomic_fault BEFORE {event} ON {table} {predicate} BEGIN SELECT RAISE({action}); END"
            ));
            assert_eq!(
                f.store().complete_uid_recovery_success([31; 32], &page),
                Err(ErrorCode::InternalError),
                "{table} {event} {action}"
            );
            assert_eq!(recovery_atomic_rows(&f), before, "{table} {event} {action}");
            assert_recovery_atomic_unresolved(&f);

            f.sql("DROP TRIGGER recovery_atomic_fault");
            let receipt = f
                .store()
                .complete_uid_recovery_success([31; 32], &page)
                .unwrap();
            let result = receipt.result().unwrap();
            assert_eq!(receipt.state(), UidPageState::Succeeded);
            assert_eq!((result.epoch_generation(), result.cursor_version()), (2, 2));
            assert_eq!(result.inserted_count(), 2);
            assert_eq!(f.count("imap_uid_epochs"), 2);
            assert_eq!(f.count("imap_uid_page_operations"), 2);
            assert_eq!(f.count("imap_uid_messages"), 4);
        }
    }
}

#[test]
fn sec03_recovery_atomic_failure_rolls_back_job_and_audit_on_abort_and_ignore() {
    for action in ["ABORT,'synthetic'", "IGNORE"] {
        for (table, event) in [
            ("imap_uid_recovery_requests", "UPDATE"),
            ("audit_terminals", "INSERT"),
        ] {
            let f = recovery_atomic_fixture();
            admit(&f, 31, 1);
            start(&f, 31);
            let before = recovery_atomic_rows(&f);
            f.sql(&format!(
                "CREATE TRIGGER recovery_atomic_fault BEFORE {event} ON {table} BEGIN SELECT RAISE({action}); END"
            ));
            assert_eq!(
                f.store()
                    .complete_uid_recovery_failure([31; 32], ErrorCode::Offline),
                Err(ErrorCode::InternalError),
                "{table} {action}"
            );
            assert_eq!(recovery_atomic_rows(&f), before, "{table} {action}");
            assert_recovery_atomic_unresolved(&f);

            f.sql("DROP TRIGGER recovery_atomic_fault");
            let receipt = f
                .store()
                .complete_uid_recovery_failure([31; 32], ErrorCode::Offline)
                .unwrap();
            assert_eq!(receipt.state(), UidPageState::Failed);
            assert_eq!(receipt.error(), Some(ErrorCode::Offline));
            assert_eq!(f.error(31), Some(ErrorCode::Offline));
            assert_eq!(f.count("imap_uid_epochs"), 1);
            assert_eq!(f.count("imap_uid_page_operations"), 1);
            assert_eq!(f.count("imap_uid_messages"), 2);
        }
    }
}

#[test]
fn sync02_recovery_atomic_clock_regression_keeps_completion_unresolved() {
    let f = recovery_atomic_fixture();
    admit(&f, 31, 1);
    let job = start(&f, 31);
    f.sql("UPDATE imap_uid_recovery_requests SET started_at_utc_ms=started_at_utc_ms+60000 WHERE operation_id='recover-31'");
    let before = recovery_atomic_rows(&f);
    assert_eq!(
        f.store()
            .complete_uid_recovery_success([31; 32], &recovery_atomic_page(&job)),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(recovery_atomic_rows(&f), before);
    assert_recovery_atomic_unresolved(&f);
    assert_eq!(
        f.store()
            .complete_uid_recovery_failure([31; 32], ErrorCode::Offline),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(recovery_atomic_rows(&f), before);
    assert_recovery_atomic_unresolved(&f);
}

#[test]
fn sync02_recovery_atomic_clock_regression_does_not_start_admitted_job() {
    let f = recovery_atomic_fixture();
    admit(&f, 31, 1);
    f.sql("UPDATE imap_uid_recovery_requests SET admitted_at_utc_ms=admitted_at_utc_ms+60000 WHERE operation_id='recover-31'");
    let before = recovery_atomic_rows(&f);
    assert_eq!(
        f.store().start_uid_recovery([31; 32]),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(recovery_atomic_rows(&f), before);
    assert_recovery_atomic_unresolved(&f);
}
