use super::{SnapshotError, Value};
use hantong_contracts::diagnostic_aggregate::*;
use serde_json::json;

fn counts<const N: usize, T: Copy>(
    values: [T; N],
    name: impl Fn(T) -> &'static str,
    count: impl Fn(T) -> u32,
) -> Value {
    Value::Object(
        values
            .into_iter()
            .map(|value| (name(value).to_owned(), Value::from(count(value))))
            .collect(),
    )
}

pub(super) fn serialize(value: &DiagnosticAggregate) -> Result<String, SnapshotError> {
    let window = value.window();
    let mut result = serde_json::Map::new();
    result.insert("schema_version".into(), Value::from(2));
    result.insert(
        "generated_at_utc_ms".into(),
        Value::from(value.generated_at_utc_ms()),
    );
    result.insert(
        "selected_sections".into(),
        Value::Array(
            value
                .sections()
                .iter()
                .map(|s| Value::from(s.as_str()))
                .collect(),
        ),
    );
    result.insert("window".into(),json!({"from_utc_ms":window.from_utc_ms(),"to_utc_ms":window.to_utc_ms(),"max_executions":window.max_executions(),"included_executions":window.included_executions(),"truncated":window.truncated()}));
    if let Some(sync) = value.sync() {
        result.insert("sync".into(),json!({"source":"durable_job_ledgers","count":sync.count(),
            "state_counts":counts(DiagnosticJobState::ALL,DiagnosticJobState::as_str,|state|sync.state_count(state)),
            "kind_counts":counts(DiagnosticJobKind::ALL,DiagnosticJobKind::as_str,|kind|sync.kind_count(kind))}));
    }
    if let Some(recent) = value.recent_work() {
        result.insert("recent_work".into(),json!({"source":"audit_executions","count":recent.count(),
            "state_counts":counts(DiagnosticAuditState::ALL,DiagnosticAuditState::as_str,|state|recent.state_count(state)),
            "tool_counts":counts(DiagnosticTool::ALL,DiagnosticTool::as_str,|tool|recent.tool_count(tool))}));
    }
    if let Some(errors) = value.errors() {
        let by_code: Vec<_> = DIAGNOSTIC_ERRORS
            .into_iter()
            .filter(|code| errors.count_for(*code) > 0)
            .map(|code| json!({"code":code.as_str(),"count":errors.count_for(code)}))
            .collect();
        result.insert("errors".into(),json!({"count":errors.count(),"last_error_code":errors.last_error_code().map(|c|c.as_str()),"by_code":by_code}));
    }
    let mut result = Value::Object(result);
    result.sort_all_objects();
    let json = serde_json::to_string(&result).map_err(|_| SnapshotError::Serialization)?;
    if json.len() > MAX_SNAPSHOT_BYTES {
        return Err(SnapshotError::MetricOverflow);
    }
    Ok(json)
}
