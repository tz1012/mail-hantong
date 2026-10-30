use crate::ToolRequest;
use hantong_contracts::{ErrorCode, audit::AuditTool};
use serde_json::{Value, json};

#[test]
fn sync01_initial_inputs_reject_caller_selected_target_or_runtime_authority() {
    let initial = json!({"action":"initial-start","account_id":"acct","wire_hex":"494e424f58","encoding":"modified_utf7","operation_id":"op","idempotency_key":"key","expected_catalog_generation":1,"expected_preview_generation":0,"recent_pages":2,"intent_hash":"00".repeat(32)});
    assert!(ToolRequest::validate(&envelope(initial.clone())).is_ok());
    for field in [
        "instance_id",
        "uid_validity",
        "uid_next",
        "after_uid",
        "through_uid",
        "model_version",
        "approved",
        "owner_registration_id",
        "execution_id",
    ] {
        let mut invalid = initial.clone();
        invalid[field] = json!(1);
        assert_eq!(
            ToolRequest::validate(&envelope(invalid)),
            Err(ErrorCode::InvalidInput)
        );
    }
    for (field, value) in [
        ("expected_preview_generation", json!(-1)),
        ("expected_catalog_generation", json!(0)),
        ("recent_pages", json!(0)),
        ("recent_pages", json!(9)),
        ("recent_pages", json!(258)),
        ("recent_pages", json!(1.0)),
        ("wire_hex", json!("696e626f78")),
        ("intent_hash", json!("AA".repeat(32))),
    ] {
        let mut invalid = initial.clone();
        invalid[field] = value;
        assert_eq!(
            ToolRequest::validate(&envelope(invalid)),
            Err(ErrorCode::InvalidInput)
        );
    }
    for key in initial.as_object().unwrap().keys() {
        let mut invalid = initial.clone();
        invalid.as_object_mut().unwrap().remove(key);
        assert_eq!(
            ToolRequest::validate(&envelope(invalid)),
            Err(ErrorCode::InvalidInput)
        );
    }
}

#[test]
fn sync02_inspection_renderer_preserves_absent_empty_and_sparse_observations() {
    use hantong_contracts::{
        uid_inspection::{UidInspectionSnapshot, UidObservationSummary},
        uid_page::{UidMailboxEncoding, UidMailboxIdentity},
    };
    let mailbox = UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap();
    for observation in [
        None,
        Some(UidObservationSummary::new(1, 1, 0, 0, 1).unwrap()),
        Some(UidObservationSummary::new(3, 2, 64, 500, 1767225600000).unwrap()),
    ] {
        let snapshot =
            UidInspectionSnapshot::new("acct", mailbox.clone(), 4, false, observation.clone())
                .unwrap();
        let actual = crate::render_uid_inspection(&snapshot);
        let nested = observation.map(|v| json!({"cursor_version":v.cursor_version(),"epoch_generation":v.epoch_generation(),"covered_through_uid":v.covered_through_uid(),"observed_uid_ceiling":v.observed_uid_ceiling(),"stored_at_utc_ms":v.stored_at_utc_ms(),"coverage_at_observed_ceiling":v.coverage_at_observed_ceiling()}));
        assert_eq!(
            actual,
            json!({"schema_version":1,"ok":true,"account_id":"acct","wire_hex":"494e424f58","encoding":"modified_utf7","catalog_generation":4,"binding_current":false,"observation":nested})
        );
    }
}

#[test]
fn sync02_inspection_accepts_only_canonical_read_identity() {
    let p = json!({"action":"inspect","account_id":"acct","wire_hex":"494e424f58","encoding":"modified_utf7"});
    let parsed = ToolRequest::validate(&envelope(p.clone())).unwrap();
    assert_eq!(parsed.tool(), AuditTool::SyncPage);
    assert_eq!(parsed.account_ids(), ["acct"]);
    for field in ["action", "account_id", "wire_hex", "encoding"] {
        let mut invalid = p.clone();
        invalid.as_object_mut().unwrap().remove(field);
        assert_eq!(
            ToolRequest::validate(&envelope(invalid)),
            Err(ErrorCode::InvalidInput)
        );
        for value in [Value::Null, json!(1), json!(false), json!([])] {
            let mut invalid = p.clone();
            invalid[field] = value;
            assert_eq!(
                ToolRequest::validate(&envelope(invalid)),
                Err(ErrorCode::InvalidInput)
            );
        }
    }
    for (field, value) in [
        ("account_id", json!("")),
        ("wire_hex", json!("696e626f78")),
        ("wire_hex", json!("494E424F58")),
        ("encoding", json!("UTF8")),
        ("operation_id", json!("op")),
        ("expected_cursor_version", json!(1)),
        ("uid_validity", json!(9)),
        ("approved", json!(true)),
    ] {
        let mut invalid = p.clone();
        invalid[field] = value;
        assert_eq!(
            ToolRequest::validate(&envelope(invalid)),
            Err(ErrorCode::InvalidInput)
        );
    }
}
fn envelope(params: Value) -> Value {
    json!({"schema_version":1,"request_id":"uid-test","tool":"mail.sync.page","params":params})
}
fn request() -> Value {
    json!({"action":"request","account_id":"acct","wire_hex":"494e424f58","encoding":"modified_utf7","operation_id":"operation","idempotency_key":"key","expected_catalog_generation":1,"expected_cursor_version":0,"intent_hash":"00".repeat(32)})
}
#[test]
fn sync02_recovery_actions_validate_without_client_supplied_epoch_state() {
    let mut p = request();
    p["action"] = json!("recover");
    p["expected_cursor_version"] = json!(4);
    let parsed = ToolRequest::validate(&envelope(p.clone())).unwrap();
    assert_eq!(parsed.tool(), AuditTool::SyncPage);
    assert_eq!(parsed.account_ids(), ["acct"]);
    let status = ToolRequest::validate(&envelope(
        json!({"action":"recover-status","account_id":"acct","execution_id":"01".repeat(32)}),
    ))
    .unwrap();
    assert_eq!(status.account_ids(), ["acct"]);
    for field in [
        "previous_uid_validity",
        "uid_validity",
        "epoch_generation",
        "endpoint_version",
        "after_uid",
        "through_uid",
        "approved",
    ] {
        let mut invalid = p.clone();
        invalid[field] = json!(1);
        assert_eq!(
            ToolRequest::validate(&envelope(invalid)),
            Err(ErrorCode::InvalidInput)
        );
    }
    for (field, value) in [
        ("expected_cursor_version", json!(0)),
        ("wire_hex", json!("696e626f78")),
        ("expected_cursor_version", json!(i64::MAX)),
        ("expected_catalog_generation", json!(1.0)),
    ] {
        let mut invalid = p.clone();
        invalid[field] = value;
        assert_eq!(
            ToolRequest::validate(&envelope(invalid)),
            Err(ErrorCode::InvalidInput)
        );
    }
}
#[test]
fn ext04_reconcile_tool_binds_explicit_account_and_rejects_private_state() {
    let mut p = request();
    p["action"] = json!("reconcile");
    p["expected_cursor_version"] = json!(2);
    let parsed = ToolRequest::validate(&envelope(p.clone())).unwrap();
    assert!(matches!(
        parsed,
        ToolRequest::SyncPage {
            action: crate::UidPageAction::Reconcile(_),
            ..
        }
    ));
    assert_eq!(parsed.account_ids(), ["acct"]);
    for field in [
        "generation",
        "through_uid",
        "minimum_uid_next",
        "endpoint_version",
        "uid_validity",
        "after_uid",
        "approved",
    ] {
        let mut invalid = p.clone();
        invalid[field] = json!(1);
        assert_eq!(
            ToolRequest::validate(&envelope(invalid)),
            Err(ErrorCode::InvalidInput)
        );
    }
    for (field, value) in [
        ("expected_cursor_version", json!(0)),
        ("wire_hex", json!("696e626f78")),
        ("expected_catalog_generation", json!(1.0)),
    ] {
        let mut invalid = p.clone();
        invalid[field] = value;
        assert_eq!(
            ToolRequest::validate(&envelope(invalid)),
            Err(ErrorCode::InvalidInput)
        );
    }
    let status = ToolRequest::validate(&envelope(
        json!({"action":"reconcile-status","account_id":"acct","execution_id":"01".repeat(32)}),
    ))
    .unwrap();
    assert!(matches!(
        status,
        ToolRequest::SyncPage {
            action: crate::UidPageAction::ReconcileStatus(_),
            ..
        }
    ));
}
#[test]
fn ext04_uid_tool_routes_exact_three_actions_and_account() {
    for params in [
        request(),
        json!({"action":"cursor","account_id":"acct","wire_hex":"494e424f58","encoding":"modified_utf7"}),
        json!({"action":"status","account_id":"acct","execution_id":"01".repeat(32)}),
    ] {
        let value = ToolRequest::validate_method("mail.sync.page", &envelope(params)).unwrap();
        assert_eq!(value.tool(), AuditTool::SyncPage);
        assert_eq!(value.account_ids(), ["acct"]);
        assert_eq!(value.request_id(), "uid-test");
        assert!(!format!("{value:?}").contains("acct"));
    }
}
#[test]
fn ext04_uid_request_rejects_noncanonical_mailbox_and_injected_fetch_state() {
    for field in [
        "endpoint_version",
        "uid_validity",
        "after_uid",
        "limit",
        "account_ids",
        "approved",
    ] {
        let mut p = request();
        p[field] = json!(1);
        assert_eq!(
            ToolRequest::validate(&envelope(p)),
            Err(ErrorCode::InvalidInput)
        );
    }
    for (field, value) in [
        ("wire_hex", json!("696e626f78")),
        ("wire_hex", json!("00")),
        ("encoding", json!("UTF8")),
        ("expected_catalog_generation", json!(0)),
        ("expected_cursor_version", json!(i64::MAX)),
        ("intent_hash", Value::Null),
    ] {
        let mut p = request();
        p[field] = value;
        assert_eq!(
            ToolRequest::validate(&envelope(p)),
            Err(ErrorCode::InvalidInput)
        );
    }
    assert_eq!(
        ToolRequest::validate_method("mail.folders.refresh", &envelope(request())),
        Err(ErrorCode::InvalidInput)
    );
}
