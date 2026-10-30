use super::*;
use hantong_contracts::diagnostic_aggregate::*;

fn aggregate(sections: &[DiagnosticSection]) -> DiagnosticAggregate {
    let mut errors = [0; 22];
    errors[6] = 1;
    DiagnosticAggregate::new(
        1000,
        sections,
        DiagnosticWindow::new(0, 1000, 100, 1, false).unwrap(),
        sections
            .contains(&DiagnosticSection::Sync)
            .then(|| DiagnosticSync::new([0, 0, 0, 1], [0, 1, 0, 0]).unwrap()),
        sections
            .contains(&DiagnosticSection::RecentWork)
            .then(|| DiagnosticRecentWork::new([0, 0, 1], [0, 0, 0, 1, 0, 0, 0, 0]).unwrap()),
        sections
            .contains(&DiagnosticSection::Errors)
            .then(|| DiagnosticErrors::new(errors, Some(ErrorCode::Offline)).unwrap()),
    )
    .unwrap()
}

#[test]
fn ops05_sec08_diagnostic_v2_uses_one_canonical_immutable_snapshot() {
    let snapshot =
        DiagnosticSnapshot::from_aggregate(aggregate(&[DiagnosticSection::Errors])).unwrap();
    let expected = r#"{"errors":{"by_code":[{"code":"OFFLINE","count":1}],"count":1,"last_error_code":"OFFLINE"},"generated_at_utc_ms":1000,"schema_version":2,"selected_sections":["errors"],"window":{"from_utc_ms":0,"included_executions":1,"max_executions":100,"to_utc_ms":1000,"truncated":false}}"#;
    assert_eq!(snapshot.preview_json().unwrap(), expected);
    assert_eq!(snapshot.export_json().unwrap(), expected);
    assert_eq!(snapshot.clone().export_json().unwrap(), expected);
    assert_eq!(
        snapshot.clone().with_error_summary(0, None),
        Err(SnapshotError::ImmutableSnapshot)
    );
    assert_eq!(snapshot.preview_json().unwrap(), expected);
}

#[test]
fn ops05_diagnostic_v2_has_exact_selected_keys_and_all_fixed_counters() {
    let snapshot = DiagnosticSnapshot::from_aggregate(aggregate(&[
        DiagnosticSection::Errors,
        DiagnosticSection::RecentWork,
        DiagnosticSection::Sync,
    ]))
    .unwrap();
    let bytes = snapshot.preview_json().unwrap();
    let v: Value = serde_json::from_str(&bytes).unwrap();
    assert_eq!(
        v["selected_sections"],
        serde_json::json!(["sync", "recent_work", "errors"])
    );
    assert_eq!(v.as_object().unwrap().len(), 7);
    assert_eq!(v["sync"]["state_counts"].as_object().unwrap().len(), 4);
    assert_eq!(v["sync"]["kind_counts"].as_object().unwrap().len(), 4);
    assert_eq!(
        v["recent_work"]["tool_counts"].as_object().unwrap().len(),
        8
    );
    assert_eq!(
        v["recent_work"]["state_counts"].as_object().unwrap().len(),
        3
    );
    assert_eq!(v["sync"]["count"], 1);
    assert_eq!(v["errors"]["count"], 1);
    assert!(bytes.len() <= MAX_SNAPSHOT_BYTES && !bytes.ends_with('\n'));
    for forbidden in [
        "registration_id",
        "execution_id",
        "account_id",
        "mailbox",
        "token",
        "path",
        "subject",
        "@",
    ] {
        assert!(!bytes.contains(forbidden));
    }
    for sections in [[DiagnosticSection::Sync], [DiagnosticSection::RecentWork]] {
        let bytes = DiagnosticSnapshot::from_aggregate(aggregate(&sections))
            .unwrap()
            .export_json()
            .unwrap();
        let v: Value = serde_json::from_str(&bytes).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 5);
        assert!(v.get("errors").is_none());
    }
}

#[test]
fn ops05_diagnostic_v1_bytes_and_unavailable_behavior_remain_unchanged() {
    let selection = DiagnosticSelection::try_from_sections(&[
        SnapshotSection::RecentWork,
        SnapshotSection::Sync,
    ])
    .unwrap();
    let snapshot = DiagnosticSnapshot::new(7, selection).unwrap();
    assert_eq!(
        snapshot.preview_json().unwrap(),
        r#"{"generated_at_utc_ms":7,"recent_work":"unavailable","schema_version":1,"selected_sections":["sync","recent_work"],"sync":"unavailable"}"#
    );
    assert_eq!(
        snapshot.preview_json().unwrap(),
        snapshot.export_json().unwrap()
    );
}

#[test]
fn sec08_diagnostic_v2_all_error_codes_are_bounded_fixed_and_in_contract_order() {
    let aggregate = DiagnosticAggregate::new(
        i64::MAX,
        &[DiagnosticSection::Errors],
        DiagnosticWindow::new(i64::MAX - 1, i64::MAX, 1000, 22, false).unwrap(),
        None,
        None,
        Some(DiagnosticErrors::new([1; 22], Some(ErrorCode::InternalError)).unwrap()),
    )
    .unwrap();
    let bytes = DiagnosticSnapshot::from_aggregate(aggregate)
        .unwrap()
        .export_json()
        .unwrap();
    assert!(bytes.len() <= MAX_SNAPSHOT_BYTES);
    let value: Value = serde_json::from_str(&bytes).unwrap();
    let codes = value["errors"]["by_code"].as_array().unwrap();
    assert_eq!(codes.len(), 22);
    for (row, code) in codes.iter().zip(DIAGNOSTIC_ERRORS) {
        assert_eq!(row["code"], code.as_str());
        assert_eq!(row["count"], 1);
    }
}
