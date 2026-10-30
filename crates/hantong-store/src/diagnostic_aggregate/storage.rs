use super::*;
use crate::registrations::{blob_column, read_registration, text_column};
use hantong_contracts::{audit::AuditTool, diagnostic_aggregate::*};
use rusqlite::{Connection, OptionalExtension, params};

const MAX_SCANNED: usize = 100_000;
const JOBS: [(DiagnosticJobKind, &str); 4] = [
    (DiagnosticJobKind::FolderRefresh, "folder_refresh_requests"),
    (DiagnosticJobKind::UidAppend, "imap_uid_page_requests"),
    (
        DiagnosticJobKind::UidReconcile,
        "imap_uid_reconcile_requests",
    ),
    (DiagnosticJobKind::UidRecovery, "imap_uid_recovery_requests"),
];

fn db<T>(value: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    value.map_err(|_| ErrorCode::InternalError)
}
fn stored<T>(value: Result<T, ErrorCode>) -> Result<T, ErrorCode> {
    value.map_err(|_| ErrorCode::InternalError)
}
fn optional_text(
    row: &rusqlite::Row<'_>,
    index: usize,
    max: usize,
) -> rusqlite::Result<Option<String>> {
    if matches!(row.get_ref(index)?, rusqlite::types::ValueRef::Null) {
        Ok(None)
    } else {
        Ok(Some(text_column(row, index, max)?.to_owned()))
    }
}
fn error_code(text: Option<&str>) -> Result<Option<ErrorCode>, ErrorCode> {
    text.map(|text| {
        DIAGNOSTIC_ERRORS
            .into_iter()
            .find(|code| code.as_str() == text)
            .ok_or(ErrorCode::InternalError)
    })
    .transpose()
}

struct Terminal {
    at: i64,
    state: DiagnosticAuditState,
    error: Option<ErrorCode>,
    result_count: Option<i64>,
}
struct Audit {
    sequence: i64,
    epoch: i64,
    tool: DiagnosticTool,
    started: i64,
    accounts: Vec<String>,
    terminal: Option<Terminal>,
}

fn read_audit(
    c: &Connection,
    id: &[u8; 32],
    owner: &DiagnosticAggregateOwner,
    generated: i64,
) -> Result<Audit, ErrorCode> {
    let mut statement=db(c.prepare("SELECT audit_seq,profile_epoch,tool_code,started_at_utc_ms FROM audit_starts WHERE execution_id=?1 AND registration_id=?2"))?;
    let (sequence, epoch, tool, started) = db(statement
        .query_row(
            params![id.as_slice(), owner.registration_id().as_slice()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    text_column(row, 2, 96)?.to_owned(),
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional())?
    .ok_or(ErrorCode::InternalError)?;
    drop(statement);
    if sequence <= 0
        || epoch <= 0
        || epoch as u64 > owner.expected_profile_epoch()
        || started <= 0
        || started > generated
    {
        return Err(ErrorCode::InternalError);
    }
    let tool = stored(DiagnosticTool::parse(&tool))?;
    let global = stored(AuditTool::parse(tool.as_str()))?.is_global_read();
    let mut statement=db(c.prepare("SELECT account_id FROM audit_start_accounts WHERE execution_id=?1 ORDER BY account_id LIMIT 129"))?;
    let account_rows = db(statement.query_map([id.as_slice()], |row| {
        Ok(text_column(row, 0, 128)?.to_owned())
    }))?;
    let accounts = db(account_rows.collect::<rusqlite::Result<Vec<_>>>())?;
    drop(statement);
    if (global && !accounts.is_empty())
        || (!global && !(1..=128).contains(&accounts.len()))
        || accounts.iter().any(|account| {
            account.is_empty()
                || !account
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
    {
        return Err(ErrorCode::InternalError);
    }
    let mut statement=db(c.prepare("SELECT completed_at_utc_ms,result_kind,error_code,result_count FROM audit_terminals WHERE execution_id=?1"))?;
    let raw = db(statement
        .query_row([id.as_slice()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                text_column(row, 1, 14)?.to_owned(),
                optional_text(row, 2, 32)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })
        .optional())?;
    let terminal = if let Some((at, state, error, result_count)) = raw {
        let state = stored(DiagnosticAuditState::parse(&state))?;
        let error = error_code(error.as_deref())?;
        if at < started
            || at > generated
            || result_count.is_some_and(|n| n < 0)
            || (global && result_count.is_some())
            || match state {
                DiagnosticAuditState::ReturnedOk => error.is_some(),
                DiagnosticAuditState::ReturnedError => error.is_none() || result_count.is_some(),
                DiagnosticAuditState::Unresolved => true,
            }
        {
            return Err(ErrorCode::InternalError);
        }
        Some(Terminal {
            at,
            state,
            error,
            result_count,
        })
    } else {
        None
    };
    Ok(Audit {
        sequence,
        epoch,
        tool,
        started,
        accounts,
        terminal,
    })
}

struct Job {
    kind: DiagnosticJobKind,
    state: DiagnosticJobState,
}
fn read_job(
    c: &Connection,
    id: &[u8; 32],
    audit: &Audit,
    generated: i64,
) -> Result<Option<Job>, ErrorCode> {
    let mut job = None;
    for (kind, table) in JOBS {
        // The table identifier comes only from the four literal entries above.
        let mut statement=db(c.prepare(&format!("SELECT account_id,state,admitted_at_utc_ms,started_at_utc_ms,completed_at_utc_ms,error_code FROM {table} WHERE execution_id=?1")))?;
        let raw = db(statement
            .query_row([id.as_slice()], |row| {
                Ok((
                    text_column(row, 0, 128)?.to_owned(),
                    text_column(row, 1, 10)?.to_owned(),
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    optional_text(row, 5, 32)?,
                ))
            })
            .optional())?;
        let Some((account, state, admitted, started, completed, error)) = raw else {
            continue;
        };
        let state = stored(DiagnosticJobState::parse(&state))?;
        let error = error_code(error.as_deref())?;
        let expected_tool = if kind == DiagnosticJobKind::FolderRefresh {
            DiagnosticTool::FoldersRefresh
        } else {
            DiagnosticTool::SyncPage
        };
        if job.is_some()
            || audit.tool != expected_tool
            || audit.accounts != [account]
            || admitted < audit.started
            || admitted > generated
            || started.is_some_and(|at| at < admitted || at > generated)
            || completed.is_some_and(|at| at < started.unwrap_or(admitted) || at > generated)
        {
            return Err(ErrorCode::InternalError);
        }
        match (state, started, completed, error, audit.terminal.as_ref()) {
            (DiagnosticJobState::Admitted, None, None, None, None) => {}
            (DiagnosticJobState::Running, Some(_), None, None, None) => {}
            (DiagnosticJobState::Succeeded, Some(_), Some(at), None, Some(t))
                if t.at == at
                    && t.state == DiagnosticAuditState::ReturnedOk
                    && t.error.is_none()
                    && t.result_count.is_none() => {}
            (DiagnosticJobState::Failed, _, Some(at), Some(error), Some(t))
                if t.at == at
                    && t.state == DiagnosticAuditState::ReturnedError
                    && t.error == Some(error)
                    && t.result_count.is_none() => {}
            _ => return Err(ErrorCode::InternalError),
        }
        job = Some(Job { kind, state });
    }
    Ok(job)
}

pub(super) fn collect(
    c: &Connection,
    query: &DiagnosticAggregateQuery,
) -> Result<DiagnosticAggregate, ErrorCode> {
    let owner = query.owner();
    // This first read establishes the Deferred transaction's SQLite snapshot.
    let registration =
        db(read_registration(c, owner.registration_id()))?.ok_or(ErrorCode::ClientNotRegistered)?;
    if registration.state != crate::RegistrationState::Active {
        return Err(ErrorCode::ClientSuspended);
    }
    if registration.profile_epoch != owner.expected_profile_epoch() {
        return Err(ErrorCode::AuthRequired);
    }
    let generated = db(crate::utc_millis())?;
    if query.to_utc_ms() > generated {
        return Err(ErrorCode::InvalidInput);
    }
    if let Some(excluded) = owner.excluded_execution_id() {
        let audit = read_audit(c, excluded, owner, generated)?;
        if audit.epoch as u64 != owner.expected_profile_epoch()
            || audit.tool != DiagnosticTool::DiagnosticsSnapshot
            || audit.terminal.is_some()
        {
            return Err(ErrorCode::InternalError);
        }
    }
    let has_sync = query.sections().contains(&DiagnosticSection::Sync);
    let mut audit_states = [0u32; 3];
    let mut tools = [0u32; 8];
    let mut job_states = [0u32; 4];
    let mut job_kinds = [0u32; 4];
    let mut errors = [0u32; 22];
    let mut last_error = None;
    let mut included = 0u32;
    let mut truncated = false;
    let mut statement=db(c.prepare("SELECT execution_id,registration_id=?1,started_at_utc_ms FROM audit_starts ORDER BY audit_seq DESC LIMIT 100001"))?;
    let mut rows = db(statement.query([owner.registration_id().as_slice()]))?;
    let mut scanned = 0usize;
    while let Some(row) = db(rows.next())? {
        scanned += 1;
        if scanned > MAX_SCANNED {
            return Err(ErrorCode::InternalError);
        }
        if !db(row.get::<_, bool>(1))? {
            continue;
        }
        let id = db(blob_column::<32>(row, 0))?;
        if owner.excluded_execution_id() == Some(&id) {
            continue;
        }
        let started = db(row.get::<_, i64>(2))?;
        if started <= 0 {
            return Err(ErrorCode::InternalError);
        }
        if started < query.from_utc_ms() || started >= query.to_utc_ms() {
            continue;
        }
        let audit = read_audit(c, &id, owner, generated)?;
        if audit
            .accounts
            .iter()
            .any(|a| !registration.scope.accounts().contains(a))
        {
            continue;
        }
        if included == query.max_executions() {
            truncated = true;
            break;
        }
        included += 1;
        let state = audit
            .terminal
            .as_ref()
            .map_or(DiagnosticAuditState::Unresolved, |t| t.state);
        audit_states[state.index()] += 1;
        tools[audit.tool.index()] += 1;
        if let Some(t) = &audit.terminal
            && let Some(error) = t.error
        {
            let index = DIAGNOSTIC_ERRORS
                .iter()
                .position(|code| *code == error)
                .ok_or(ErrorCode::InternalError)?;
            errors[index] += 1;
            if last_error
                .as_ref()
                .is_none_or(|(at, seq, _)| (t.at, audit.sequence) > (*at, *seq))
            {
                last_error = Some((t.at, audit.sequence, error));
            }
        }
        if has_sync && let Some(job) = read_job(c, &id, &audit, generated)? {
            job_states[job.state.index()] += 1;
            job_kinds[job.kind.index()] += 1;
        }
    }
    let window = stored(DiagnosticWindow::new(
        query.from_utc_ms(),
        query.to_utc_ms(),
        query.max_executions(),
        included,
        truncated,
    ))?;
    let sync = has_sync
        .then(|| DiagnosticSync::new(job_states, job_kinds))
        .transpose();
    let recent = query
        .sections()
        .contains(&DiagnosticSection::RecentWork)
        .then(|| DiagnosticRecentWork::new(audit_states, tools))
        .transpose();
    let errors = query
        .sections()
        .contains(&DiagnosticSection::Errors)
        .then(|| DiagnosticErrors::new(errors, last_error.map(|(_, _, code)| code)))
        .transpose();
    stored(DiagnosticAggregate::new(
        generated,
        query.sections(),
        window,
        stored(sync)?,
        stored(recent)?,
        stored(errors)?,
    ))
}
