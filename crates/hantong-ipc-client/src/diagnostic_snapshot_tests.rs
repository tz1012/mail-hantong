use super::*;
use serde_json::{Value, json};

fn request(sections: &[DiagnosticSection]) -> DiagnosticSnapshotRequest {
    DiagnosticSnapshotRequest::new(0, 1000, 100, sections).unwrap()
}
fn snapshot(sections: &[DiagnosticSection]) -> Value {
    let mut value = json!({
        "schema_version":2,"generated_at_utc_ms":1000,
        "selected_sections":sections.iter().map(|v|v.as_str()).collect::<Vec<_>>(),
        "window":{"from_utc_ms":0,"to_utc_ms":1000,"max_executions":100,"included_executions":1,"truncated":false}
    });
    if sections.contains(&DiagnosticSection::Sync) {
        value["sync"] = json!({"source":"durable_job_ledgers","count":1,
            "state_counts":{"admitted":0,"running":0,"succeeded":0,"failed":1},
            "kind_counts":{"folder_refresh":0,"uid_append":1,"uid_reconcile":0,"uid_recovery":0}});
    }
    if sections.contains(&DiagnosticSection::RecentWork) {
        value["recent_work"] = json!({"source":"audit_executions","count":1,
            "state_counts":{"unresolved":0,"returned_ok":0,"returned_error":1},
            "tool_counts":{"mail.accounts.list":0,"mail.folders.list":0,"mail.folders.refresh":0,"mail.sync.page":1,
                "mail.messages.search":0,"mail.send.execute":0,"app.settings.display.get":0,"app.diagnostics.snapshot":0}});
    }
    if sections.contains(&DiagnosticSection::Errors) {
        value["errors"] =
            json!({"count":1,"last_error_code":"OFFLINE","by_code":[{"code":"OFFLINE","count":1}]});
    }
    value
}
fn envelope(inner: &str) -> String {
    json!({"jsonrpc":"2.0","id":"diag-1","result":{"schema_version":1,"ok":true,"snapshot_json":inner}}).to_string()
}
fn parse(
    inner: &Value,
    req: &DiagnosticSnapshotRequest,
) -> Result<ValidatedDiagnosticSnapshot, ErrorCode> {
    parse_snapshot_response(envelope(&inner.to_string()).as_bytes(), "diag-1", req)
}

#[test]
fn ops05_diagnostic_snapshot_all_section_selections_preserve_canonical_bytes() {
    for mask in 1..8 {
        let sections: Vec<_> = DiagnosticSection::ALL
            .into_iter()
            .filter(|section| mask & (1 << section.index()) != 0)
            .collect();
        let value = snapshot(&sections);
        let parsed = parse(&value, &request(&sections)).unwrap();
        assert_eq!(parsed.snapshot_json(), value.to_string());
        assert_eq!(parsed.clone().snapshot_json(), parsed.snapshot_json());
        assert_eq!(parsed.aggregate().sections(), sections);
        assert_eq!(parsed.aggregate().window().included_executions(), 1);
        assert_eq!(
            format!("{parsed:?}"),
            "ValidatedDiagnosticSnapshot(<redacted>)"
        );
    }
    let req = request(&[DiagnosticSection::Errors]);
    let mut empty = snapshot(req.sections());
    empty["window"]["included_executions"] = json!(0);
    empty["errors"] = json!({"count":0,"last_error_code":null,"by_code":[]});
    assert_eq!(
        parse(&empty, &req)
            .unwrap()
            .aggregate()
            .errors()
            .unwrap()
            .last_error_code(),
        None
    );
}

fn object_paths(value: &Value, path: String, output: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            output.push(path.clone());
            for (key, child) in map {
                object_paths(child, format!("{path}/{key}"), output);
            }
        }
        Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                object_paths(child, format!("{path}/{index}"), output);
            }
        }
        _ => {}
    }
}
fn duplicate_at(value: &Value, target: &str, path: &str) -> String {
    match value {
        Value::Object(map) => {
            let mut fields: Vec<_> = map
                .iter()
                .map(|(key, child)| {
                    format!(
                        "{}:{}",
                        json!(key),
                        duplicate_at(child, target, &format!("{path}/{key}"))
                    )
                })
                .collect();
            if path == target {
                fields.push(fields[0].clone());
            }
            format!("{{{}}}", fields.join(","))
        }
        Value::Array(array) => format!(
            "[{}]",
            array
                .iter()
                .enumerate()
                .map(|(index, child)| duplicate_at(child, target, &format!("{path}/{index}")))
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => value.to_string(),
    }
}

#[test]
fn sec08_diagnostic_snapshot_every_nested_object_rejects_duplicate_missing_unknown_null_array() {
    let req = request(&DiagnosticSection::ALL);
    let good = snapshot(req.sections());
    let mut paths = vec![];
    object_paths(&good, String::new(), &mut paths);
    for path in paths {
        let duplicate = duplicate_at(&good, &path, "");
        assert_eq!(
            parse_snapshot_response(envelope(&duplicate).as_bytes(), "diag-1", &req),
            Err(ErrorCode::InvalidInput),
            "duplicate at {path}"
        );
        for key in good.pointer(&path).unwrap().as_object().unwrap().keys() {
            let mut missing = good.clone();
            missing
                .pointer_mut(&path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key);
            assert_eq!(
                parse(&missing, &req),
                Err(ErrorCode::InvalidInput),
                "missing {path}/{key}"
            );
        }
        let mut unknown = good.clone();
        unknown
            .pointer_mut(&path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("private_account_id".into(), json!("forbidden"));
        assert_eq!(
            parse(&unknown, &req),
            Err(ErrorCode::InvalidInput),
            "unknown at {path}"
        );
        for replacement in [Value::Null, json!([])] {
            let mut bad = good.clone();
            *bad.pointer_mut(&path).unwrap() = replacement;
            assert_eq!(
                parse(&bad, &req),
                Err(ErrorCode::InvalidInput),
                "type at {path}"
            );
        }
    }
}

#[test]
fn ops05_diagnostic_snapshot_binds_request_and_rejects_inconsistent_aggregates() {
    let req = request(&DiagnosticSection::ALL);
    let good = snapshot(req.sections());
    for (path, replacement) in [
        ("/schema_version", json!(1)),
        ("/schema_version", json!(2.0)),
        ("/generated_at_utc_ms", json!(999)),
        ("/window/from_utc_ms", json!(1)),
        ("/window/to_utc_ms", json!(999)),
        ("/window/max_executions", json!(99)),
        ("/window/included_executions", json!(2)),
        ("/window/truncated", json!(true)),
        (
            "/selected_sections",
            json!(["errors", "recent_work", "sync"]),
        ),
        (
            "/selected_sections",
            json!(["sync", "recent_work", "errors", "errors"]),
        ),
        ("/sync/source", json!("live_workers")),
        ("/sync/count", json!(2)),
        ("/sync/state_counts/failed", json!(0)),
        ("/sync/kind_counts/uid_append", json!(2)),
        ("/recent_work/source", json!("all_accounts")),
        ("/recent_work/count", json!(0)),
        ("/recent_work/state_counts/returned_error", json!(0)),
        ("/recent_work/tool_counts/mail.sync.page", json!(0)),
        ("/errors/count", json!(2)),
        ("/errors/last_error_code", json!("NOT_FOUND")),
        ("/errors/last_error_code", Value::Null),
        ("/errors/by_code/0/code", json!("PRIVATE_ERROR")),
        ("/errors/by_code/0/count", json!(0)),
        ("/errors/by_code/0/count", json!(-1)),
        ("/errors/by_code/0/count", json!(u64::MAX)),
    ] {
        let mut bad = good.clone();
        *bad.pointer_mut(path).unwrap() = replacement;
        assert_eq!(parse(&bad, &req), Err(ErrorCode::InvalidInput), "{path}");
    }
    let mut mismatch = good.clone();
    mismatch["sync"]["state_counts"] = json!({"admitted":0,"running":0,"succeeded":1,"failed":0});
    assert_eq!(parse(&mismatch, &req), Err(ErrorCode::InvalidInput));
    let req = request(&[DiagnosticSection::Errors]);
    for extra in [json!(null), json!({})] {
        let mut bad = snapshot(req.sections());
        bad["sync"] = extra;
        assert_eq!(parse(&bad, &req), Err(ErrorCode::InvalidInput));
    }
}

#[test]
fn sec08_diagnostic_error_order_and_count_are_frozen() {
    let req = request(&[DiagnosticSection::Errors]);
    let mut value = snapshot(req.sections());
    value["window"]["included_executions"] = json!(2);
    value["errors"] = json!({"count":2,"last_error_code":"NOT_FOUND","by_code":[{"code":"OFFLINE","count":1},{"code":"NOT_FOUND","count":1}]});
    assert!(parse(&value, &req).is_ok());
    value["errors"]["by_code"].as_array_mut().unwrap().reverse();
    assert_eq!(parse(&value, &req), Err(ErrorCode::InvalidInput));
    value["errors"]["by_code"] = json!([{"code":"OFFLINE","count":1},{"code":"OFFLINE","count":1}]);
    assert_eq!(parse(&value, &req), Err(ErrorCode::InvalidInput));
}

#[test]
fn sec08_diagnostic_snapshot_raw_outer_and_inner_limits_and_canonical_encoding() {
    let req = request(&DiagnosticSection::ALL);
    let inner = snapshot(req.sections()).to_string();
    let raw = envelope(&inner);
    let padded = format!("{raw}{}", " ".repeat(36_864 - raw.len()));
    assert!(parse_snapshot_response(padded.as_bytes(), "diag-1", &req).is_ok());
    assert_eq!(
        parse_snapshot_response(format!("{padded} ").as_bytes(), "diag-1", &req),
        Err(ErrorCode::InvalidInput)
    );
    for bad in [
        format!(" {inner}"),
        format!("{inner}\n"),
        format!("\u{feff}{inner}"),
        format!("{inner}{{}}"),
        inner.replace("\"schema_version\"", "\"schema_vers\\u0069on\""),
        format!("{inner}{}", " ".repeat(16_385 - inner.len())),
    ] {
        assert_eq!(
            parse_snapshot_response(envelope(&bad).as_bytes(), "diag-1", &req),
            Err(ErrorCode::InvalidInput)
        );
    }
    for bad in [
        raw.replace("\"id\":\"diag-1\"","\"id\":\"other\""),
        raw.replace("\"ok\":true","\"ok\":true,\"ok\":true"),
        raw.replace("\"ok\":true,",""),
        raw.replace("\"ok\":true","\"ok\":false"),
        raw.replace("\"ok\":true","\"ok\":true,\"private\":0"),
        r#"{"jsonrpc":"2.0","id":"diag-1","result":[1,true,"{}"]}"#.into(),
        r#"{"jsonrpc":"2.0","id":"diag-1","result":{"schema_version":1,"ok":true,"snapshot_json":null}}"#.into(),
    ] { assert_eq!(parse_snapshot_response(bad.as_bytes(),"diag-1",&req),Err(ErrorCode::InvalidInput)); }
    let error=json!({"jsonrpc":"2.0","id":"diag-1","error":{"code":"SCOPE_DENIED","message":"SCOPE_DENIED"}}).to_string();
    assert_eq!(
        parse_snapshot_response(error.as_bytes(), "diag-1", &req),
        Err(ErrorCode::ScopeDenied)
    );
}
