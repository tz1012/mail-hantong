use super::*;

fn mailbox() -> UidMailboxIdentity {
    UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap()
}
fn mutation() -> InitialSyncMutation {
    InitialSyncMutation::new("op", "key", &"00".repeat(32)).unwrap()
}
fn start() -> InitialSyncRequest {
    InitialSyncRequest::new_start("acct", mailbox(), mutation(), 1, 0, 8).unwrap()
}
fn snapshot(
    state: InitialSyncState,
    completed: u8,
    model: u64,
    version: u64,
) -> InitialSyncPlanSnapshot {
    InitialSyncPlanSnapshot::new(
        InitialSyncPlanIdentity::new([0xab; 32], "acct", mailbox(), 1).unwrap(),
        version,
        InitialSyncProgress::new(7, u32::MAX, 8, completed, state, model).unwrap(),
        true,
        true,
        1000,
    )
    .unwrap()
}
fn successful_start() -> InitialSyncReceipt {
    InitialSyncReceipt::new(
        "acct",
        [0xab; 32],
        InitialSyncAction::Start,
        InitialSyncJobState::Succeeded,
        Some(snapshot(InitialSyncState::Ready, 0, 1, 1)),
        None,
    )
    .unwrap()
}
fn envelope(result: &Value) -> Vec<u8> {
    json!({"jsonrpc":"2.0","id":"r","result":result})
        .to_string()
        .into_bytes()
}
fn receipt_value(v: &InitialSyncReceipt) -> Value {
    serde_json::from_str(&receipt_json(v)).unwrap()
}

#[test]
fn sync01_initial_receipts_and_current_plan_roundtrip_without_mail_data() {
    let start = start();
    let pairs = [
        (start.clone(), successful_start()),
        (
            InitialSyncRequest::new_resume("acct", mailbox(), mutation(), [0xab; 32], 1, 1)
                .unwrap(),
            InitialSyncReceipt::new(
                "acct",
                [0xcd; 32],
                InitialSyncAction::Resume,
                InitialSyncJobState::Succeeded,
                Some(snapshot(InitialSyncState::PausedBudget, 1, 3, 3)),
                None,
            )
            .unwrap(),
        ),
        (
            InitialSyncRequest::new_pause("acct", mailbox(), mutation(), [0xab; 32], 1).unwrap(),
            InitialSyncReceipt::new(
                "acct",
                [0xef; 32],
                InitialSyncAction::Pause,
                InitialSyncJobState::Succeeded,
                Some(snapshot(InitialSyncState::PausedUser, 0, 2, 2)),
                None,
            )
            .unwrap(),
        ),
    ];
    for (request, value) in pairs {
        let raw = envelope(&receipt_value(&value));
        assert_eq!(parse_mutation_receipt(&raw, "r", &request).unwrap(), value);
        let status = UidPageStatusRequest::new("acct", *value.execution_id()).unwrap();
        assert_eq!(parse_status_receipt(&raw, "r", &status).unwrap(), value);
    }
    for state in [
        InitialSyncJobState::Admitted,
        InitialSyncJobState::Running,
        InitialSyncJobState::Failed,
    ] {
        let value = InitialSyncReceipt::new(
            "acct",
            [0xab; 32],
            InitialSyncAction::Start,
            state,
            None,
            (state == InitialSyncJobState::Failed).then_some(ErrorCode::Offline),
        )
        .unwrap();
        assert_eq!(
            parse_mutation_receipt(&envelope(&receipt_value(&value)), "r", &start).unwrap(),
            value
        );
    }
    let p = snapshot(InitialSyncState::Ready, 0, 1, 1);
    let value: Value = serde_json::from_str(&plan_json(&p)).unwrap();
    assert_eq!(
        parse_plan_snapshot(
            &envelope(&value),
            "r",
            &InitialSyncPlanRequest::new("acct", mailbox(), [0xab; 32]).unwrap()
        )
        .unwrap(),
        p
    );
    assert_eq!(p.progress().remaining_uid_span(), 510);
}

fn paths(value: &Value, path: String, out: &mut Vec<String>) {
    if let Value::Object(map) = value {
        out.push(path.clone());
        for (key, child) in map {
            paths(child, format!("{path}/{key}"), out);
        }
    }
}
fn duplicate(value: &Value, target: &str, path: &str) -> String {
    if let Value::Object(map) = value {
        let mut fields: Vec<_> = map
            .iter()
            .map(|(key, child)| {
                format!(
                    "{}:{}",
                    json!(key),
                    duplicate(child, target, &format!("{path}/{key}"))
                )
            })
            .collect();
        if target == path {
            fields.push(fields[0].clone());
        }
        format!("{{{}}}", fields.join(","))
    } else {
        value.to_string()
    }
}
#[test]
fn sec03_initial_response_rejects_missing_duplicate_unknown_null_and_array_at_every_object() {
    let request = start();
    let value = receipt_value(&successful_start());
    let mut locations = vec![];
    paths(&value, String::new(), &mut locations);
    for path in locations {
        let raw = format!(
            r#"{{"jsonrpc":"2.0","id":"r","result":{}}}"#,
            duplicate(&value, &path, "")
        );
        assert_eq!(
            parse_mutation_receipt(raw.as_bytes(), "r", &request),
            Err(ErrorCode::InvalidInput)
        );
        for key in value.pointer(&path).unwrap().as_object().unwrap().keys() {
            let mut bad = value.clone();
            bad.pointer_mut(&path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key);
            assert_eq!(
                parse_mutation_receipt(&envelope(&bad), "r", &request),
                Err(ErrorCode::InvalidInput),
                "{path}/{key}"
            );
        }
        let mut bad = value.clone();
        bad.pointer_mut(&path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("approved".into(), json!(true));
        assert_eq!(
            parse_mutation_receipt(&envelope(&bad), "r", &request),
            Err(ErrorCode::InvalidInput)
        );
        for replacement in [json!([]), Value::Null] {
            let mut bad = value.clone();
            *bad.pointer_mut(&path).unwrap() = replacement;
            assert_eq!(
                parse_mutation_receipt(&envelope(&bad), "r", &request),
                Err(ErrorCode::InvalidInput)
            );
        }
    }
}

#[test]
fn sec03_initial_response_binds_account_action_plan_mailbox_progress_and_cas() {
    let request = start();
    let value = receipt_value(&successful_start());
    for (path, replacement) in [
        ("/account_id", json!("other")),
        ("/action", json!("initial-resume")),
        ("/state", json!("live")),
        ("/error_code", json!("OFFLINE")),
        ("/execution_id", json!("cd".repeat(32))),
        ("/plan/plan_id", json!("AB".repeat(32))),
        ("/plan/account_id", json!("other")),
        ("/plan/wire_hex", json!("61")),
        ("/plan/encoding", json!("utf8")),
        ("/plan/generation", json!(2)),
        ("/plan/version", json!(2)),
        ("/plan/progress/model_version", json!(0)),
        ("/plan/progress/uid_validity", json!(0)),
        ("/plan/progress/recent_pages", json!(7)),
        ("/plan/progress/target_uid_ceiling", json!(1)),
        ("/plan/progress/selected_lower_uid", Value::Null),
        ("/plan/progress/selected_page_count", json!(7)),
        ("/plan/progress/remaining_page_count", json!(7)),
        ("/plan/progress/remaining_uid_span", json!(512)),
        ("/plan/progress/not_selected_before_uid", Value::Null),
        ("/plan/progress/completed_page_count", json!(1)),
        ("/plan/progress/uid_next", json!(4294967296u64)),
        ("/plan/stored_at_utc_ms", json!(-1)),
        ("/schema_version", json!(1.0)),
        ("/plan/version", json!(u64::MAX)),
    ] {
        let mut bad = value.clone();
        *bad.pointer_mut(path).unwrap() = replacement;
        assert_eq!(
            parse_mutation_receipt(&envelope(&bad), "r", &request),
            Err(ErrorCode::InvalidInput),
            "{path}"
        );
    }
    let status = UidPageStatusRequest::new("acct", [1; 32]).unwrap();
    assert_eq!(
        parse_status_receipt(&envelope(&value), "r", &status),
        Err(ErrorCode::InvalidInput)
    );
    let resume =
        InitialSyncRequest::new_resume("acct", mailbox(), mutation(), [0xab; 32], 3, 1).unwrap();
    let receipt = InitialSyncReceipt::new(
        "acct",
        [0xcd; 32],
        InitialSyncAction::Resume,
        InitialSyncJobState::Succeeded,
        Some(snapshot(InitialSyncState::PausedBudget, 1, 3, 3)),
        None,
    )
    .unwrap();
    assert_eq!(
        parse_mutation_receipt(&envelope(&receipt_value(&receipt)), "r", &resume),
        Err(ErrorCode::InvalidInput)
    );
    let p = snapshot(InitialSyncState::Ready, 0, 1, 1);
    let raw = envelope(&serde_json::from_str(&plan_json(&p)).unwrap());
    for req in [
        InitialSyncPlanRequest::new("other", mailbox(), [0xab; 32]).unwrap(),
        InitialSyncPlanRequest::new("acct", mailbox(), [1; 32]).unwrap(),
    ] {
        assert_eq!(
            parse_plan_snapshot(&raw, "r", &req),
            Err(ErrorCode::InvalidInput)
        );
    }
}

#[test]
fn sec03_initial_rpc_preserves_raw_eight_kib_bound_and_stable_error_shape() {
    let request = start();
    let raw = envelope(&receipt_value(&successful_start()));
    let mut padded = raw.clone();
    padded.resize(8192, b' ');
    assert!(parse_mutation_receipt(&padded, "r", &request).is_ok());
    padded.push(b' ');
    assert_eq!(
        parse_mutation_receipt(&padded, "r", &request),
        Err(ErrorCode::InvalidInput)
    );
    for malformed in [
        b"[]".to_vec(),
        b"\xef\xbb\xbf{}".to_vec(),
        vec![0xff],
        [raw.clone(), b"{}".to_vec()].concat(),
        String::from_utf8(raw.clone())
            .unwrap()
            .replace("\"id\":\"r\"", "\"id\":\"r\",\"id\":\"r\"")
            .into_bytes(),
        br#"{"jsonrpc":"2.0","id":"r","error":["OFFLINE","OFFLINE"]}"#.to_vec(),
        br#"{"jsonrpc":"2.0","id":"r","error":{"code":"OFFLINE","message":"provider secret"}}"#
            .to_vec(),
    ] {
        assert_eq!(
            parse_mutation_receipt(&malformed, "r", &request),
            Err(ErrorCode::InvalidInput)
        );
    }
    assert_eq!(
        parse_mutation_receipt(&raw, "other", &request),
        Err(ErrorCode::InvalidInput)
    );
    for code in ErrorCode::ALL {
        let raw=json!({"jsonrpc":"2.0","id":"r","error":{"code":code.as_str(),"message":code.as_str()}}).to_string();
        assert_eq!(
            parse_mutation_receipt(raw.as_bytes(), "r", &request),
            Err(code)
        );
    }
}

#[test]
fn sync01_initial_request_wire_contains_only_each_actions_fields() {
    for request in [
        start(),
        InitialSyncRequest::new_resume("acct", mailbox(), mutation(), [1; 32], 1, 1).unwrap(),
        InitialSyncRequest::new_pause("acct", mailbox(), mutation(), [1; 32], 1).unwrap(),
    ] {
        let params = mutation_params(&request);
        assert_eq!(params["action"], request.action().as_str());
        assert_eq!(
            params.get("recent_pages").is_some(),
            request.action() == InitialSyncAction::Start
        );
        assert_eq!(
            params.get("expected_preview_generation").is_some(),
            request.action() == InitialSyncAction::Start
        );
        assert_eq!(
            params.get("plan_id").is_some(),
            request.action() != InitialSyncAction::Start
        );
        assert_eq!(
            params.get("expected_catalog_generation").is_some(),
            request.action() != InitialSyncAction::Pause
        );
        assert!(
            params
                .as_object()
                .unwrap()
                .values()
                .all(|value| !value.is_null())
        );
        #[cfg(windows)]
        {
            let prepared = prepare_request(request.clone()).unwrap();
            assert_eq!(prepared.intent_bytes(), request.intent_bytes());
            assert_eq!(
                prepared.intent_hash(),
                wire_hex::encode(&hantong_platform::sha256(&request.intent_bytes()).unwrap())
            );
            assert_ne!(prepared.intent_hash(), request.intent_hash());
        }
    }
}
