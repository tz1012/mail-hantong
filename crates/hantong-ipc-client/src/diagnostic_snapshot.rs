//! Strict decoding of bounded, identifier-free diagnostic snapshots.
use crate::{
    accounts::Object,
    display_settings::{Nullable, decode_app_response},
};
use hantong_contracts::{
    ErrorCode,
    diagnostic_aggregate::{
        DIAGNOSTIC_ERRORS, DiagnosticAggregate, DiagnosticErrors, DiagnosticRecentWork,
        DiagnosticSection, DiagnosticSync, DiagnosticWindow, MAX_SNAPSHOT_BYTES,
    },
    diagnostic_snapshot::DiagnosticSnapshotRequest,
};
use serde::{Deserialize, Deserializer};
use std::fmt;

#[derive(Clone, Eq, PartialEq)]
pub struct ValidatedDiagnosticSnapshot {
    aggregate: DiagnosticAggregate,
    json: String,
}
impl ValidatedDiagnosticSnapshot {
    pub fn snapshot_json(&self) -> &str {
        &self.json
    }
    pub fn aggregate(&self) -> &DiagnosticAggregate {
        &self.aggregate
    }
}
impl fmt::Debug for ValidatedDiagnosticSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ValidatedDiagnosticSnapshot(<redacted>)")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wrapper {
    schema_version: u32,
    ok: bool,
    snapshot_json: String,
}

// Sections may be absent, but explicit null is never an omitted section.
#[derive(Default)]
enum Section<T> {
    #[default]
    Missing,
    Present(T),
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Section<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Object::<T>::deserialize(deserializer).map(|value| Self::Present(value.0))
    }
}
impl<T> Section<T> {
    fn into_option(self) -> Option<T> {
        match self {
            Self::Missing => None,
            Self::Present(value) => Some(value),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotWire {
    schema_version: u32,
    generated_at_utc_ms: i64,
    selected_sections: Vec<String>,
    window: Object<WindowWire>,
    #[serde(default)]
    sync: Section<SyncWire>,
    #[serde(default)]
    recent_work: Section<RecentWire>,
    #[serde(default)]
    errors: Section<ErrorsWire>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowWire {
    from_utc_ms: i64,
    to_utc_ms: i64,
    max_executions: u32,
    included_executions: u32,
    truncated: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncWire {
    source: String,
    count: u32,
    state_counts: Object<JobStates>,
    kind_counts: Object<JobKinds>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobStates {
    admitted: u32,
    running: u32,
    succeeded: u32,
    failed: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobKinds {
    folder_refresh: u32,
    uid_append: u32,
    uid_reconcile: u32,
    uid_recovery: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecentWire {
    source: String,
    count: u32,
    state_counts: Object<AuditStates>,
    tool_counts: Object<ToolsWire>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditStates {
    unresolved: u32,
    returned_ok: u32,
    returned_error: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolsWire {
    #[serde(rename = "mail.accounts.list")]
    accounts: u32,
    #[serde(rename = "mail.folders.list")]
    folders: u32,
    #[serde(rename = "mail.folders.refresh")]
    refresh: u32,
    #[serde(rename = "mail.sync.page")]
    sync: u32,
    #[serde(rename = "mail.messages.search")]
    search: u32,
    #[serde(rename = "mail.send.execute")]
    send: u32,
    #[serde(rename = "app.settings.display.get")]
    display: u32,
    #[serde(rename = "app.diagnostics.snapshot")]
    diagnostics: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorsWire {
    count: u32,
    last_error_code: Nullable<String>,
    by_code: Vec<Object<CodeCount>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CodeCount {
    code: String,
    count: u32,
}

fn sync(value: SyncWire) -> Result<DiagnosticSync, ErrorCode> {
    let s = value.state_counts.0;
    let k = value.kind_counts.0;
    let result = DiagnosticSync::new(
        [s.admitted, s.running, s.succeeded, s.failed],
        [
            k.folder_refresh,
            k.uid_append,
            k.uid_reconcile,
            k.uid_recovery,
        ],
    )?;
    if value.source != "durable_job_ledgers" || value.count != result.count() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(result)
}
fn recent(value: RecentWire) -> Result<DiagnosticRecentWork, ErrorCode> {
    let s = value.state_counts.0;
    let t = value.tool_counts.0;
    let result = DiagnosticRecentWork::new(
        [s.unresolved, s.returned_ok, s.returned_error],
        [
            t.accounts,
            t.folders,
            t.refresh,
            t.sync,
            t.search,
            t.send,
            t.display,
            t.diagnostics,
        ],
    )?;
    if value.source != "audit_executions" || value.count != result.count() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(result)
}
fn errors(value: ErrorsWire) -> Result<DiagnosticErrors, ErrorCode> {
    let mut counts = [0; 22];
    let mut previous = None;
    for item in value.by_code {
        let index = DIAGNOSTIC_ERRORS
            .iter()
            .position(|code| code.as_str() == item.0.code)
            .ok_or(ErrorCode::InvalidInput)?;
        if item.0.count == 0 || previous.is_some_and(|prior| prior >= index) {
            return Err(ErrorCode::InvalidInput);
        }
        counts[index] = item.0.count;
        previous = Some(index);
    }
    let last = value
        .last_error_code
        .into_option()
        .map(|text| {
            DIAGNOSTIC_ERRORS
                .into_iter()
                .find(|code| code.as_str() == text)
                .ok_or(ErrorCode::InvalidInput)
        })
        .transpose()?;
    let result = DiagnosticErrors::new(counts, last)?;
    if value.count != result.count() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(result)
}

fn decode_snapshot(
    json: &str,
    request: &DiagnosticSnapshotRequest,
) -> Result<DiagnosticAggregate, ErrorCode> {
    if json.is_empty() || json.len() > MAX_SNAPSHOT_BYTES || json.starts_with('\u{feff}') {
        return Err(ErrorCode::InvalidInput);
    }
    let raw: Object<SnapshotWire> =
        serde_json::from_str(json).map_err(|_| ErrorCode::InvalidInput)?;
    let raw = raw.0;
    let sections = raw
        .selected_sections
        .iter()
        .map(|text| DiagnosticSection::parse(text))
        .collect::<Result<Vec<_>, _>>()?;
    let w = raw.window.0;
    if raw.schema_version != 2
        || sections != request.sections()
        || w.from_utc_ms != request.from_utc_ms()
        || w.to_utc_ms != request.to_utc_ms()
        || w.max_executions != request.max_executions()
    {
        return Err(ErrorCode::InvalidInput);
    }
    let window = DiagnosticWindow::new(
        w.from_utc_ms,
        w.to_utc_ms,
        w.max_executions,
        w.included_executions,
        w.truncated,
    )?;
    let aggregate = DiagnosticAggregate::new(
        raw.generated_at_utc_ms,
        &sections,
        window,
        raw.sync.into_option().map(sync).transpose()?,
        raw.recent_work.into_option().map(recent).transpose()?,
        raw.errors.into_option().map(errors).transpose()?,
    )?;
    // All object keys/types/duplicates have been validated from the original
    // stream. Value is used only now to verify compact ASCII-key canonical bytes.
    let canonical: serde_json::Value =
        serde_json::from_str(json).map_err(|_| ErrorCode::InvalidInput)?;
    if serde_json::to_string(&canonical).map_err(|_| ErrorCode::InvalidInput)? != json {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(aggregate)
}

pub fn parse_snapshot_response(
    bytes: &[u8],
    expected_rpc_id: &str,
    request: &DiagnosticSnapshotRequest,
) -> Result<ValidatedDiagnosticSnapshot, ErrorCode> {
    let result: Wrapper = decode_app_response(bytes, expected_rpc_id, 36_864)?;
    if result.schema_version != 1 || !result.ok {
        return Err(ErrorCode::InvalidInput);
    }
    let aggregate = decode_snapshot(&result.snapshot_json, request)?;
    Ok(ValidatedDiagnosticSnapshot {
        aggregate,
        json: result.snapshot_json,
    })
}

pub fn snapshot_default(
    request: DiagnosticSnapshotRequest,
) -> Result<ValidatedDiagnosticSnapshot, ErrorCode> {
    #[cfg(windows)]
    {
        tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build()
            .map_err(|_|ErrorCode::CoreUnavailable)?.block_on(async move {
                use hantong_contracts::auth::AppReadTool;
                let profile=crate::authenticate_default_app_read(AppReadTool::DiagnosticsSnapshot).await?;
                let id="diagnostic-snapshot-1";
                let tool=AppReadTool::DiagnosticsSnapshot.as_str();
                let params=serde_json::json!({"from_utc_ms":request.from_utc_ms(),"to_utc_ms":request.to_utc_ms(),
                    "max_executions":request.max_executions(),"sections":request.sections().iter().map(|section|section.as_str()).collect::<Vec<_>>()});
                let envelope=serde_json::json!({"schema_version":1,"request_id":id,"tool":tool,"params":params});
                let raw=profile.exchange(id,tool,&envelope).await?;
                parse_snapshot_response(&raw,id,&request)
            })
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

#[cfg(test)]
#[path = "diagnostic_snapshot_tests.rs"]
mod tests;
