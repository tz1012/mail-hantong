use super::*;

fn owner() -> DiagnosticAggregateOwner {
    DiagnosticAggregateOwner::new([17; 16], 1, Some([19; 32])).unwrap()
}
fn window(included: u32) -> DiagnosticWindow {
    DiagnosticWindow::new(0, 10, 100, included, false).unwrap()
}

#[test]
fn ops05_diagnostic_query_canonicalizes_sections_and_rejects_invalid_bounds() {
    let query = DiagnosticAggregateQuery::new(
        owner(),
        0,
        MAX_WINDOW_MS,
        1000,
        &[
            DiagnosticSection::Errors,
            DiagnosticSection::Sync,
            DiagnosticSection::RecentWork,
        ],
    )
    .unwrap();
    assert_eq!(query.sections(), DiagnosticSection::ALL);
    assert_eq!(query.max_executions(), 1000);
    for (from, to, limit) in [
        (-1, 10, 1),
        (0, 0, 1),
        (10, 9, 1),
        (0, MAX_WINDOW_MS + 1, 1),
        (0, 1, 0),
        (0, 1, 1001),
        (i64::MAX - 1, i64::MAX, 1),
    ] {
        let result =
            DiagnosticAggregateQuery::new(owner(), from, to, limit, &[DiagnosticSection::Sync]);
        if from == i64::MAX - 1 {
            assert!(result.is_ok());
        } else {
            assert_eq!(result, Err(ErrorCode::InvalidInput));
        }
    }
    for sections in [
        vec![],
        vec![DiagnosticSection::Sync, DiagnosticSection::Sync],
    ] {
        assert_eq!(
            DiagnosticAggregateQuery::new(owner(), 0, 1, 1, &sections),
            Err(ErrorCode::InvalidInput)
        );
    }
    for epoch in [0, i64::MAX as u64 + 1] {
        assert_eq!(
            DiagnosticAggregateOwner::new([1; 16], epoch, None),
            Err(ErrorCode::InvalidInput)
        );
    }
}

#[test]
fn sec08_diagnostic_owner_and_query_debug_hide_identifiers() {
    let owner = owner();
    let query =
        DiagnosticAggregateQuery::new(owner.clone(), 0, 10, 100, &[DiagnosticSection::Errors])
            .unwrap();
    for text in [format!("{owner:?}"), format!("{query:?}")] {
        assert!(text.contains("redacted"));
        assert!(!text.contains("17") && !text.contains("19"));
    }
    assert_eq!(owner.registration_id(), &[17; 16]);
    assert_eq!(owner.excluded_execution_id(), Some(&[19; 32]));
}

#[test]
fn ops05_diagnostic_counts_are_bounded_and_do_not_double_count_errors() {
    assert!(DiagnosticRecentWork::new([0, 1, 1], [0, 0, 0, 2, 0, 0, 0, 0]).is_ok());
    assert_eq!(
        DiagnosticRecentWork::new([0, 1, 1], [0; 8]),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        DiagnosticSync::new([0, 0, 0, 1001], [1001, 0, 0, 0]),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        DiagnosticSync::new([0, 0, 0, 1], [0; 4]),
        Err(ErrorCode::InvalidInput)
    );
    let mut counts = [0; 22];
    counts[6] = 1;
    let errors = DiagnosticErrors::new(counts, Some(ErrorCode::Offline)).unwrap();
    assert_eq!(errors.count(), 1);
    assert_eq!(errors.count_for(ErrorCode::Offline), 1);
    assert_eq!(
        DiagnosticErrors::new(counts, None),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        DiagnosticErrors::new(counts, Some(ErrorCode::InternalError)),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        DiagnosticErrors::new([0; 22], Some(ErrorCode::Offline)),
        Err(ErrorCode::InvalidInput)
    );
    assert!(DiagnosticErrors::new([0; 22], None).is_ok());
}

#[test]
fn ops05_diagnostic_aggregate_binds_selection_window_and_subset_counts() {
    let recent = DiagnosticRecentWork::new([0, 1, 1], [0, 0, 0, 2, 0, 0, 0, 0]).unwrap();
    let sync = DiagnosticSync::new([0, 0, 0, 1], [0, 1, 0, 0]).unwrap();
    let mut counts = [0; 22];
    counts[6] = 1;
    let errors = DiagnosticErrors::new(counts, Some(ErrorCode::Offline)).unwrap();
    let value = DiagnosticAggregate::new(
        10,
        &DiagnosticSection::ALL,
        window(2),
        Some(sync.clone()),
        Some(recent.clone()),
        Some(errors.clone()),
    )
    .unwrap();
    assert_eq!(value.clone(), value);
    assert_eq!(value.window().included_executions(), 2);
    assert_eq!(value.sync().unwrap().count(), 1);
    assert_eq!(value.errors().unwrap().count(), 1);
    assert_eq!(
        DiagnosticAggregate::new(
            9,
            &DiagnosticSection::ALL,
            window(2),
            Some(sync.clone()),
            Some(recent.clone()),
            Some(errors.clone())
        ),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        DiagnosticAggregate::new(
            10,
            &[DiagnosticSection::Errors],
            window(2),
            Some(sync),
            None,
            Some(errors)
        ),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        DiagnosticAggregate::new(
            10,
            &[DiagnosticSection::RecentWork],
            window(1),
            None,
            Some(recent),
            None
        ),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        DiagnosticWindow::new(0, 10, 10, 9, true),
        Err(ErrorCode::InvalidInput)
    );
    assert!(DiagnosticWindow::new(0, 10, 10, 10, true).is_ok());
}

#[test]
fn sec08_diagnostic_vocabularies_are_fixed_and_unknown_text_is_rejected() {
    assert_eq!(DiagnosticTool::ALL.len(), 8);
    for tool in DiagnosticTool::ALL {
        assert_eq!(DiagnosticTool::parse(tool.as_str()), Ok(tool));
    }
    assert_eq!(
        DiagnosticTool::parse("app.private.dump"),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        DiagnosticSection::parse("Errors"),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        DiagnosticJobState::parse("completed"),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        DiagnosticAuditState::parse("succeeded"),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(DiagnosticJobKind::ALL.len(), 4);
    assert_eq!(DIAGNOSTIC_ERRORS.len(), 22);
}
