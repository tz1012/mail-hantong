use super::*;
use crate::{
    audit::{insert_audit_terminal_tx, read_audit},
    imap_sync::storage::{Operation, read_cursor, read_operation},
    registrations::{blob_column, text_column},
    uid_page_jobs::storage::{Binding, binding, current_grants, mailbox, page_operation_exists},
};
use hantong_contracts::{
    audit::{AuditRecord, AuditTerminal, AuditTool},
    uid_page::{UidMailboxEncoding, UidMailboxIdentity, UidPageRequest, UidPageState},
    uid_reconcile::UidReconcileProgress,
};
use hantong_domain::imap_sync::ImapMessageMetadata;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};

const MAX_RESULT: usize = 1024 * 1024;
fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    r.map_err(|_| ErrorCode::InternalError)
}
fn hash(bytes: &[u8]) -> Result<[u8; 32], ErrorCode> {
    hantong_platform::sha256(bytes).map_err(|_| ErrorCode::InternalError)
}
fn now() -> Result<i64, ErrorCode> {
    db(crate::utc_millis())
}
fn optional_text(r: &rusqlite::Row<'_>, i: usize, max: usize) -> rusqlite::Result<Option<String>> {
    if matches!(r.get_ref(i)?, rusqlite::types::ValueRef::Null) {
        Ok(None)
    } else {
        Ok(Some(text_column(r, i, max)?.to_owned()))
    }
}
fn optional_hash(r: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<Option<[u8; 32]>> {
    if matches!(r.get_ref(i)?, rusqlite::types::ValueRef::Null) {
        Ok(None)
    } else {
        Ok(Some(blob_column::<32>(r, i)?))
    }
}
fn as_i64(value: u64) -> Result<i64, ErrorCode> {
    i64::try_from(value).map_err(|_| ErrorCode::InternalError)
}
fn bumped(value: u64) -> Result<u64, ErrorCode> {
    value
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or(ErrorCode::InternalError)
}
fn canonical(v: &Value) -> Result<String, ErrorCode> {
    serde_json::to_string(v).map_err(|_| ErrorCode::InternalError)
}

#[derive(Clone, Eq, PartialEq)]
struct Head {
    generation: u64,
    version: u64,
    ceiling: u64,
    last: u64,
    complete: bool,
    validity: u32,
    endpoint_version: u64,
    endpoint_hash: [u8; 32],
    at: i64,
    last_id: Option<[u8; 32]>,
}
struct Row {
    request: UidReconcileRequest,
    audit: AuditRecord,
    fetch: ImapUidReconcileRequest,
    av: u64,
    endpoint_hash: [u8; 32],
    generation: u64,
    head_version: u64,
    ceiling: u64,
    source: Operation,
    state: String,
    admitted: i64,
    started: Option<i64>,
    completed: Option<i64>,
    result: Option<ObservationResult>,
    receipt: UidReconcileReceipt,
}
#[derive(Clone, Eq, PartialEq)]
struct Observation {
    uid: u32,
    flags: Option<Vec<String>>,
}
struct ObservationResult {
    progress: UidReconcileProgress,
    observations: Vec<Observation>,
    json: String,
}
struct Raw {
    account: String,
    op: String,
    key: String,
    mailbox: String,
    encoding: String,
    intent: [u8; 32],
    n: [i64; 12],
    endpoint_hash: [u8; 32],
    source: String,
    source_hash: [u8; 32],
    state: String,
    admitted: i64,
    started: Option<i64>,
    completed: Option<i64>,
    result: Option<String>,
    result_hash: Option<[u8; 32]>,
    error: Option<String>,
}

fn public_as_page(request: &UidReconcileRequest) -> Result<UidPageRequest, ErrorCode> {
    UidPageRequest::new(
        request.account_id(),
        request.mailbox().clone(),
        request.operation_id(),
        request.idempotency_key(),
        request.expected_catalog_generation(),
        request.expected_cursor_version(),
        request.intent_hash(),
    )
}
fn current(
    c: &Transaction<'_>,
    request: &UidReconcileRequest,
) -> Result<(Binding, u32), ErrorCode> {
    let b = binding(c, &public_as_page(request)?)?;
    let cursor =
        read_cursor(c, request.account_id(), b.fetch.mailbox())?.ok_or(ErrorCode::Conflict)?;
    Ok((b, cursor.value.uid_next_at_start()))
}
fn source_matches(
    op: &Operation,
    request: &UidReconcileRequest,
    validity: u32,
    ev: u64,
    eh: &[u8; 32],
) -> bool {
    let r = op.page.request();
    r.account_id() == request.account_id()
        && hantong_contracts::wire_hex::encode(r.mailbox().wire_name())
            == request.mailbox().wire_hex()
        && match r.mailbox().encoding() {
            hantong_domain::mailboxes::MailboxEncoding::Utf8 => "utf8",
            hantong_domain::mailboxes::MailboxEncoding::ModifiedUtf7 => "modified_utf7",
        } == request.mailbox().encoding().as_str()
        && op.page.uid_validity() == validity
        && r.endpoint_version() == ev
        && &op.endpoint_hash == eh
}
/// Indexed numeric scan; only the selected source's bounded JSON is decoded.
/// Remaining counts do not assert semantic verification of unselected history.
fn source_scan(
    c: &Connection,
    request: &UidReconcileRequest,
    after: u64,
    ceiling: u64,
) -> Result<(Option<i64>, u32), ErrorCode> {
    let (total,count,first,corrupt):(i64,i64,Option<i64>,i64)=db(c.query_row("SELECT count(*),coalesce(sum(CASE WHEN after_uid<covered_through_uid AND inserted_count>0 THEN 1 ELSE 0 END),0),min(CASE WHEN after_uid<covered_through_uid AND inserted_count>0 THEN result_cursor_version END),coalesce(sum(CASE WHEN inserted_count<>(SELECT count(*) FROM imap_uid_messages m WHERE m.operation_id=o.operation_id) OR (inserted_count>0 AND after_uid>=covered_through_uid) THEN 1 ELSE 0 END),0) FROM imap_uid_page_operations o WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND result_cursor_version>?4 AND result_cursor_version<=?5",params![request.account_id(),request.mailbox().wire_hex(),request.mailbox().encoding().as_str(),as_i64(after)?,as_i64(ceiling)?],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))))?;
    // Existing mailbox/version and message/operation indexes bound the numeric scan.
    // A forged count or deleted child must not hide a previously nonempty source.
    // The UNIQUE mailbox/version index makes count == width a continuity check.
    if corrupt != 0
        || !(0..=100_000).contains(&total)
        || total as u64 != ceiling.checked_sub(after).ok_or(ErrorCode::InternalError)?
        || !(0..=total).contains(&count)
        || (count == 0) != first.is_none()
    {
        return Err(ErrorCode::InternalError);
    }
    Ok((first, count as u32))
}
fn sources(
    c: &Connection,
    request: &UidReconcileRequest,
    validity: u32,
    ev: u64,
    eh: &[u8; 32],
    after: u64,
    ceiling: u64,
) -> Result<(Option<Operation>, u32), ErrorCode> {
    let (first, count) = source_scan(c, request, after, ceiling)?;
    let Some(version) = first else {
        return Ok((None, 0));
    };
    let id:String=db(c.query_row("SELECT operation_id FROM imap_uid_page_operations WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND result_cursor_version=?4",params![request.account_id(),request.mailbox().wire_hex(),request.mailbox().encoding().as_str(),version],|r|Ok(text_column(r,0,128)?.to_owned())))?;
    let op = read_operation(c, &id)?.ok_or(ErrorCode::InternalError)?;
    if op.receipt.cursor_version() != version as u64
        || !source_matches(&op, request, validity, ev, eh)
        || op.page.request().after_uid() >= op.page.covered_through_uid()
        || op.page.messages().is_empty()
    {
        return Err(ErrorCode::InternalError);
    }
    Ok((Some(op), count))
}
fn raw_head(c: &Connection, request: &UidReconcileRequest) -> Result<Option<Head>, ErrorCode> {
    let raw=db(c.query_row("SELECT generation,version,frozen_cursor_version,last_source_cursor_version,phase,uid_validity,endpoint_version,endpoint_hash,updated_at_utc_ms,last_execution_id FROM imap_uid_reconcile_heads WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3",params![request.account_id(),request.mailbox().wire_hex(),request.mailbox().encoding().as_str()],|r|Ok(([r.get::<_,i64>(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(5)?,r.get(6)?],text_column(r,4,8)?.to_owned(),blob_column::<32>(r,7)?,r.get::<_,i64>(8)?,optional_hash(r,9)?))).optional())?;
    let Some((n, phase, eh, at, id)) = raw else {
        return Ok(None);
    };
    if n[0] <= 0
        || n[1] <= 0
        || n[2] <= 0
        || n[3] < 0
        || n[3] > n[2]
        || n[4] <= 0
        || n[4] > i64::from(u32::MAX)
        || n[5] <= 0
        || at <= 0
        || !matches!(phase.as_str(), "open" | "complete")
        || (n[3] == 0) != (id.is_none())
        || (n[3] == 0 && phase != "open")
        || id == Some([0; 32])
    {
        return Err(ErrorCode::InternalError);
    }
    Ok(Some(Head {
        generation: n[0] as u64,
        version: n[1] as u64,
        ceiling: n[2] as u64,
        last: n[3] as u64,
        complete: phase == "complete",
        validity: n[4] as u32,
        endpoint_version: n[5] as u64,
        endpoint_hash: eh,
        at,
        last_id: id,
    }))
}
fn read_head(c: &Connection, request: &UidReconcileRequest) -> Result<Option<Head>, ErrorCode> {
    let Some(head) = raw_head(c, request)? else {
        return Ok(None);
    };
    if let Some(id) = head.last_id {
        let row = read_row_core(c, id)?.ok_or(ErrorCode::InternalError)?;
        let p = row.result.as_ref().ok_or(ErrorCode::InternalError)?;
        if row.request.account_id() != request.account_id()
            || row.request.mailbox() != request.mailbox()
            || row.generation != head.generation
            || row.head_version.checked_add(1) != Some(head.version)
            || row.ceiling != head.ceiling
            || row.source.receipt.cursor_version() != head.last
            || row.completed != Some(head.at)
            || row.fetch.uid_validity() != head.validity
            || row.fetch.endpoint_version() != head.endpoint_version
            || row.endpoint_hash != head.endpoint_hash
            || p.progress.pass_complete() != head.complete
        {
            return Err(ErrorCode::InternalError);
        }
        validate_latest_overlay(c, &row)?;
    } else {
        let count:i64=db(c.query_row("SELECT count(*) FROM imap_uid_reconcile_requests WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND generation=?4 AND head_version=?5",params![request.account_id(),request.mailbox().wire_hex(),request.mailbox().encoding().as_str(),as_i64(head.generation)?,as_i64(head.version)?],|r|r.get(0)))?;
        if count == 0 {
            return Err(ErrorCode::InternalError);
        }
    }
    Ok(Some(head))
}

fn read_row_core(c: &Connection, id: [u8; 32]) -> Result<Option<Row>, ErrorCode> {
    let raw=db(c.query_row("SELECT account_id,operation_id,idempotency_key,mailbox_key,mailbox_encoding,intent_hash,expected_catalog_generation,expected_cursor_version,bound_account_version,bound_endpoint_version,uid_validity,minimum_uid_next,generation,head_version,frozen_cursor_version,source_cursor_version,after_uid,through_uid,bound_endpoint_hash,source_operation_id,source_page_hash,state,admitted_at_utc_ms,started_at_utc_ms,completed_at_utc_ms,result_json,result_hash,error_code FROM imap_uid_reconcile_requests WHERE execution_id=?1",[id.as_slice()],|r|{
        let mut n=[0i64;12];for (i,v) in n.iter_mut().take(12).enumerate(){*v=r.get(i+6)?;}
        Ok(Raw{account:text_column(r,0,128)?.into(),op:text_column(r,1,128)?.into(),key:text_column(r,2,128)?.into(),mailbox:text_column(r,3,2048)?.into(),encoding:text_column(r,4,13)?.into(),intent:blob_column::<32>(r,5)?,n,endpoint_hash:blob_column::<32>(r,18)?,source:text_column(r,19,128)?.into(),source_hash:blob_column::<32>(r,20)?,state:text_column(r,21,10)?.into(),admitted:r.get(22)?,started:r.get(23)?,completed:r.get(24)?,result:optional_text(r,25,MAX_RESULT)?,result_hash:optional_hash(r,26)?,error:optional_text(r,27,32)?})
    }).optional())?;
    let Some(raw) = raw else { return Ok(None) };
    let n = raw.n;
    if id == [0; 32]
        || n[..10].iter().any(|v| *v <= 0)
        || n[1] == i64::MAX
        || n[4] > i64::from(u32::MAX)
        || n[5] > i64::from(u32::MAX)
        || n[8] > n[1]
        || n[9] > n[8]
        || n[10] < 0
        || n[11] <= n[10]
        || n[11] >= i64::from(u32::MAX)
        || n[11] - n[10] > 64
        || n[5] <= n[11]
        || raw.admitted <= 0
        || raw.started.is_some_and(|v| v < raw.admitted)
        || raw
            .completed
            .is_some_and(|v| v < raw.started.unwrap_or(raw.admitted))
    {
        return Err(ErrorCode::InternalError);
    }
    let identity = UidMailboxIdentity::new(
        &raw.mailbox,
        UidMailboxEncoding::parse(&raw.encoding).map_err(|_| ErrorCode::InternalError)?,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    let name = mailbox(&identity).map_err(|_| ErrorCode::InternalError)?;
    let request = UidReconcileRequest::new(
        &raw.account,
        identity,
        &raw.op,
        &raw.key,
        n[0] as u64,
        n[1] as u64,
        &hantong_contracts::wire_hex::encode(&raw.intent),
    )
    .map_err(|_| ErrorCode::InternalError)?;
    if hash(&request.intent_bytes())? != raw.intent {
        return Err(ErrorCode::InternalError);
    }
    let fetch = ImapUidReconcileRequest::new(
        &raw.account,
        n[3] as u64,
        name,
        n[4] as u32,
        n[10] as u32,
        n[11] as u32,
        n[5] as u32,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    let audit = db(read_audit(c, &id))?.ok_or(ErrorCode::InternalError)?;
    if audit.tool() != AuditTool::SyncPage
        || audit.account_ids() != [raw.account.clone()]
        || raw.admitted < audit.started_at_utc_ms()
    {
        return Err(ErrorCode::InternalError);
    }
    let source = read_operation(c, &raw.source)?.ok_or(ErrorCode::InternalError)?;
    if !source_matches(
        &source,
        &request,
        fetch.uid_validity(),
        fetch.endpoint_version(),
        &raw.endpoint_hash,
    ) || source.receipt.cursor_version() != n[9] as u64
        || source.page.request().after_uid() != fetch.after_uid()
        || source.page.covered_through_uid() != fetch.through_uid()
        || source.page.messages().is_empty()
        || hash(source.json.as_bytes())? != raw.source_hash
        || source.at > raw.admitted
    {
        return Err(ErrorCode::InternalError);
    }
    let cursor = read_cursor(c, &raw.account, fetch.mailbox())?.ok_or(ErrorCode::InternalError)?;
    if cursor.value.version() < request.expected_cursor_version() {
        return Err(ErrorCode::InternalError);
    }
    let error = raw
        .error
        .as_deref()
        .map(|s| {
            ErrorCode::ALL
                .into_iter()
                .find(|e| e.as_str() == s)
                .ok_or(ErrorCode::InternalError)
        })
        .transpose()?;
    let state = match raw.state.as_str() {
        "admitted"
            if raw.started.is_none()
                && raw.completed.is_none()
                && raw.result.is_none()
                && raw.result_hash.is_none()
                && error.is_none() =>
        {
            UidPageState::Unresolved
        }
        "running"
            if raw.started.is_some()
                && raw.completed.is_none()
                && raw.result.is_none()
                && raw.result_hash.is_none()
                && error.is_none() =>
        {
            UidPageState::Unresolved
        }
        "failed"
            if raw.completed.is_some()
                && raw.result.is_none()
                && raw.result_hash.is_none()
                && error.is_some() =>
        {
            UidPageState::Failed
        }
        "succeeded"
            if raw.started.is_some()
                && raw.completed.is_some()
                && raw.result.is_some()
                && raw.result_hash.is_some()
                && error.is_none() =>
        {
            UidPageState::Succeeded
        }
        _ => return Err(ErrorCode::InternalError),
    };
    match (raw.completed, audit.completion()) {
        (None, None) => {}
        (Some(at), Some(t))
            if t.completed_at_utc_ms() == at
                && t.terminal().error() == error
                && t.terminal().is_success() == (state == UidPageState::Succeeded)
                && t.terminal().result_count().is_none() => {}
        _ => return Err(ErrorCode::InternalError),
    }
    let mut row = Row {
        request,
        audit,
        fetch,
        av: n[2] as u64,
        endpoint_hash: raw.endpoint_hash,
        generation: n[6] as u64,
        head_version: n[7] as u64,
        ceiling: n[8] as u64,
        source,
        state: raw.state,
        admitted: raw.admitted,
        started: raw.started,
        completed: raw.completed,
        result: None,
        receipt: UidReconcileReceipt::new(&raw.account, id, UidPageState::Unresolved, None, None)
            .map_err(|_| ErrorCode::InternalError)?,
    };
    if let Some(json) = raw.result {
        if Some(hash(json.as_bytes())?) != raw.result_hash {
            return Err(ErrorCode::InternalError);
        }
        row.result = Some(parse_result(&row, &json)?);
    }
    row.receipt = UidReconcileReceipt::new(
        &raw.account,
        id,
        state,
        row.result.as_ref().map(|v| v.progress.clone()),
        error,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    Ok(Some(row))
}

fn number(v: &Value, key: &str) -> Result<u64, ErrorCode> {
    v.get(key)
        .and_then(Value::as_u64)
        .ok_or(ErrorCode::InternalError)
}
fn flags_from_value(
    v: &Value,
    base: &ImapMessageMetadata,
) -> Result<Option<Vec<String>>, ErrorCode> {
    if v.is_null() {
        return Ok(None);
    }
    let flags = v
        .as_array()
        .filter(|a| a.len() <= 64)
        .ok_or(ErrorCode::InternalError)?
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| s.len() <= 128)
                .map(str::to_owned)
                .ok_or(ErrorCode::InternalError)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let validated = ImapMessageMetadata::new(
        base.uid(),
        flags.clone(),
        base.internal_date_utc_ms(),
        base.size(),
        base.envelope().clone(),
    )
    .map_err(|_| ErrorCode::InternalError)?;
    if validated.flags() != flags {
        return Err(ErrorCode::InternalError);
    }
    Ok(Some(flags))
}
fn result_value(
    row: &Row,
    p: &UidReconcileProgress,
    next: u32,
    observations: &[Observation],
) -> Result<Value, ErrorCode> {
    Ok(
        json!({"schema_version":1,"generation":p.generation(),"head_version":row.head_version,"frozen_cursor_version":p.frozen_cursor_version(),"source_operation_id":row.source.id,"source_page_hash":hantong_contracts::wire_hex::encode(&hash(row.source.json.as_bytes())?),"source_cursor_version":p.source_cursor_version(),"after_uid":p.after_uid(),"through_uid":p.through_uid(),"uid_next_at_start":next,"observed_count":p.observed_count(),"flags_changed_count":p.flags_changed_count(),"newly_absent_count":p.newly_absent_count(),"remaining_pages":p.remaining_pages(),"pass_complete":p.pass_complete(),"observations":observations.iter().map(|o|json!({"uid":o.uid,"flags":o.flags})).collect::<Vec<_>>()}),
    )
}
fn parse_result(row: &Row, json: &str) -> Result<ObservationResult, ErrorCode> {
    if json.len() > MAX_RESULT {
        return Err(ErrorCode::InternalError);
    }
    let v: Value = serde_json::from_str(json).map_err(|_| ErrorCode::InternalError)?;
    if canonical(&v)? != json || v.as_object().is_none_or(|m| m.len() != 16) {
        return Err(ErrorCode::InternalError);
    }
    let u32_value = |key| u32::try_from(number(&v, key)?).map_err(|_| ErrorCode::InternalError);
    let p = UidReconcileProgress::new(
        number(&v, "generation")?,
        number(&v, "frozen_cursor_version")?,
        number(&v, "source_cursor_version")?,
        u32_value("after_uid")?,
        u32_value("through_uid")?,
        u32_value("observed_count")?,
        u32_value("flags_changed_count")?,
        u32_value("newly_absent_count")?,
        u32_value("remaining_pages")?,
        v.get("pass_complete")
            .and_then(Value::as_bool)
            .ok_or(ErrorCode::InternalError)?,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    let next = u32_value("uid_next_at_start")?;
    if number(&v, "schema_version")? != 1
        || number(&v, "head_version")? != row.head_version
        || p.generation() != row.generation
        || p.frozen_cursor_version() != row.ceiling
        || p.source_cursor_version() != row.source.receipt.cursor_version()
        || p.after_uid() != row.fetch.after_uid()
        || p.through_uid() != row.fetch.through_uid()
        || p.observed_count() as usize != row.source.page.messages().len()
        || next < row.fetch.minimum_uid_next()
        || v.get("source_operation_id").and_then(Value::as_str) != Some(&row.source.id)
        || v.get("source_page_hash").and_then(Value::as_str)
            != Some(
                hantong_contracts::wire_hex::encode(&hash(row.source.json.as_bytes())?).as_str(),
            )
    {
        return Err(ErrorCode::InternalError);
    }
    let list = v
        .get("observations")
        .and_then(Value::as_array)
        .filter(|a| a.len() == row.source.page.messages().len())
        .ok_or(ErrorCode::InternalError)?;
    let mut observations = Vec::with_capacity(list.len());
    for (value, base) in list.iter().zip(row.source.page.messages()) {
        if value.as_object().is_none_or(|o| o.len() != 2)
            || number(value, "uid")? != u64::from(base.uid())
        {
            return Err(ErrorCode::InternalError);
        }
        observations.push(Observation {
            uid: base.uid(),
            flags: flags_from_value(value.get("flags").ok_or(ErrorCode::InternalError)?, base)?,
        });
    }
    if canonical(&result_value(row, &p, next, &observations)?)? != json {
        return Err(ErrorCode::InternalError);
    }
    Ok(ObservationResult {
        progress: p,
        observations,
        json: json.to_owned(),
    })
}
fn latest(c: &Connection, source: &str) -> Result<Option<Row>, ErrorCode> {
    let ids = {
        let mut stmt=db(c.prepare("SELECT execution_id,generation FROM imap_uid_reconcile_requests WHERE source_operation_id=?1 AND state='succeeded' ORDER BY generation DESC LIMIT 2"))?;
        db(db(stmt.query_map([source], |r| {
            Ok((blob_column::<32>(r, 0)?, r.get::<_, i64>(1)?))
        }))?
        .collect::<rusqlite::Result<Vec<_>>>())?
    };
    if ids.len() == 2 && ids[0].1 <= ids[1].1 {
        return Err(ErrorCode::InternalError);
    }
    ids.first()
        .map(|(id, _)| read_row_core(c, *id)?.ok_or(ErrorCode::InternalError))
        .transpose()
}
fn validate_overlay(c: &Connection, row: &Row) -> Result<(), ErrorCode> {
    let result = row.result.as_ref().ok_or(ErrorCode::InternalError)?;
    let mut stmt=db(c.prepare("SELECT account_id,mailbox_key,mailbox_encoding,uid_validity,uid,presence,flags_json,generation,observed_at_utc_ms FROM imap_uid_reconcile_messages WHERE last_execution_id=?1 ORDER BY uid LIMIT 65"))?;
    let values = db(
        db(stmt.query_map([row.audit.execution_id().as_slice()], |r| {
            Ok((
                text_column(r, 0, 128)?.to_owned(),
                text_column(r, 1, 2048)?.to_owned(),
                text_column(r, 2, 13)?.to_owned(),
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                text_column(r, 5, 13)?.to_owned(),
                optional_text(r, 6, 16384)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
            ))
        }))?
        .collect::<rusqlite::Result<Vec<_>>>(),
    )?;
    if values.len() != result.observations.len() {
        return Err(ErrorCode::InternalError);
    }
    for (v, o) in values.into_iter().zip(&result.observations) {
        let flags = o
            .flags
            .as_ref()
            .map(|flags| canonical(&json!(flags)))
            .transpose()?;
        if v.0 != row.request.account_id()
            || v.1 != row.request.mailbox().wire_hex()
            || v.2 != row.request.mailbox().encoding().as_str()
            || v.3 != i64::from(row.fetch.uid_validity())
            || v.4 != i64::from(o.uid)
            || v.5
                != if o.flags.is_some() {
                    "present"
                } else {
                    "remote_absent"
                }
            || v.6 != flags
            || v.7 != as_i64(row.generation)?
            || Some(v.8) != row.completed
        {
            return Err(ErrorCode::InternalError);
        }
    }
    Ok(())
}
fn validate_latest_overlay(c: &Connection, row: &Row) -> Result<(), ErrorCode> {
    if row.result.is_some() {
        let latest = latest(c, &row.source.id)?.ok_or(ErrorCode::InternalError)?;
        if latest.generation < row.generation {
            return Err(ErrorCode::InternalError);
        }
        validate_overlay(c, &latest)?;
    }
    Ok(())
}
fn read_row(c: &Connection, id: [u8; 32]) -> Result<Option<Row>, ErrorCode> {
    let row = read_row_core(c, id)?;
    if let Some(row) = &row {
        validate_latest_overlay(c, row)?;
    }
    Ok(row)
}
fn current_binding(c: &Transaction<'_>, row: &Row) -> Result<Binding, ErrorCode> {
    current_grants(c, &row.audit)?;
    let (b, next) = current(c, &row.request)?;
    let head = read_head(c, &row.request)?.ok_or(ErrorCode::InternalError)?;
    if b.av != row.av
        || b.endpoint_hash != row.endpoint_hash
        || b.endpoint.settings_version() != row.fetch.endpoint_version()
        || next != row.fetch.minimum_uid_next()
        || b.fetch.expected_uid_validity() != Some(row.fetch.uid_validity())
        || head.generation != row.generation
        || head.version != row.head_version
        || head.ceiling != row.ceiling
        || head.complete
        || head.last >= row.source.receipt.cursor_version()
        || head.validity != row.fetch.uid_validity()
        || head.endpoint_version != row.fetch.endpoint_version()
        || head.endpoint_hash != row.endpoint_hash
    {
        return Err(ErrorCode::Conflict);
    }
    let (source, _) = sources(
        c,
        &row.request,
        head.validity,
        head.endpoint_version,
        &head.endpoint_hash,
        head.last,
        head.ceiling,
    )?;
    if source.is_none_or(|s| s.id != row.source.id || s.json != row.source.json) {
        return Err(ErrorCode::Conflict);
    }
    Ok(b)
}
fn reject(
    tx: &Transaction<'_>,
    id: [u8; 32],
    error: ErrorCode,
) -> Result<UidReconcileAdmission, ErrorCode> {
    if error == ErrorCode::InternalError {
        return Err(error);
    }
    insert_audit_terminal_tx(tx, id, AuditTerminal::failed(error), now()?)?;
    Ok(UidReconcileAdmission::Rejected(error))
}
pub(crate) fn operation_reserved(
    c: &Connection,
    account: &str,
    operation: &str,
    key: &str,
) -> Result<bool, ErrorCode> {
    db(c.query_row("SELECT EXISTS(SELECT 1 FROM imap_uid_reconcile_requests WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3))",params![operation,account,key],|r|r.get(0)))
}
pub(crate) fn execution_reserved(c: &Connection, id: [u8; 32]) -> Result<bool, ErrorCode> {
    db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM imap_uid_reconcile_requests WHERE execution_id=?1)",
        [id.as_slice()],
        |r| r.get(0),
    ))
}
fn quota(c: &Connection, table: &str, max: i64, additional: i64) -> Result<(), ErrorCode> {
    let n: i64 = db(c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)))?;
    if n > max {
        Err(ErrorCode::InternalError)
    } else if n + additional > max {
        Err(ErrorCode::QuotaExceeded)
    } else {
        Ok(())
    }
}
pub(super) fn admit(
    store: &Store,
    id: [u8; 32],
    request: &UidReconcileRequest,
    queue: UidQueueAdmission,
) -> Result<UidReconcileAdmission, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate,|tx|{
        let audit=db(read_audit(tx,&id))?.ok_or(ErrorCode::NotFound)?;
        // Never finish another durable operation's unresolved audit on an attempted reuse.
        let occupied:bool=db(tx.query_row("SELECT EXISTS(SELECT 1 FROM imap_uid_page_requests WHERE execution_id=?1)",[id.as_slice()],|r|r.get(0)))?;
        if audit.completion().is_some() || execution_reserved(tx,id)? || occupied{return Err(ErrorCode::Conflict)}
        if id==[0;32] || audit.tool()!=AuditTool::SyncPage || audit.account_ids()!=[request.account_id()]{return Err(ErrorCode::InvalidInput)}
        if let Err(e)=current_grants(tx,&audit){return reject(tx,id,e)}
        if let Err(e)=mailbox(request.mailbox()){return reject(tx,id,e)}
        let intent=hash(&request.intent_bytes())?;
        if hantong_contracts::wire_hex::encode(&intent)!=request.intent_hash(){return reject(tx,id,ErrorCode::InvalidInput)}
        if crate::uid_page_jobs::operation_reserved(tx,request.account_id(),request.operation_id(),request.idempotency_key())? || page_operation_exists(tx,request.account_id(),request.operation_id(),request.idempotency_key())? {return reject(tx,id,ErrorCode::Conflict)}
        let ids={let mut stmt=db(tx.prepare("SELECT execution_id FROM imap_uid_reconcile_requests WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3) LIMIT 2"))?;
            db(db(stmt.query_map(params![request.operation_id(),request.account_id(),request.idempotency_key()],|r|blob_column::<32>(r,0)))?.collect::<rusqlite::Result<Vec<_>>>())?};
        if !ids.is_empty(){
            if ids.len()!=1{return reject(tx,id,ErrorCode::Conflict)}
            let row=read_row(tx,ids[0])?.ok_or(ErrorCode::InternalError)?;
            if row.request!=*request || row.audit.registration_id()!=audit.registration_id(){return reject(tx,id,ErrorCode::Conflict)}
            insert_audit_terminal_tx(tx,id,AuditTerminal::succeeded(None).map_err(|_|ErrorCode::InternalError)?,now()?)?;
            return Ok(UidReconcileAdmission::Existing(row.receipt))
        }
        let (current,minimum)=match current(tx,request){Ok(v)=>v,Err(e)=>return reject(tx,id,e)};
        let validity=current.fetch.expected_uid_validity().ok_or(ErrorCode::InternalError)?;
        let old=read_head(tx,request)?;
        if let Some(h)=&old && (h.validity!=validity || h.endpoint_version!=current.endpoint.settings_version() || h.endpoint_hash!=current.endpoint_hash || h.ceiling>request.expected_cursor_version()){return reject(tx,id,ErrorCode::Conflict)}
        let at=now()?;
        if at<audit.started_at_utc_ms() || old.as_ref().is_some_and(|h|at<h.at){return Err(ErrorCode::InternalError)}
        let new_pass=old.as_ref().is_none_or(|h|h.complete);
        let head=if new_pass {Head{generation:old.as_ref().map_or(Ok(1),|h|bumped(h.generation))?,version:old.as_ref().map_or(Ok(1),|h|bumped(h.version))?,ceiling:request.expected_cursor_version(),last:0,complete:false,validity,endpoint_version:current.endpoint.settings_version(),endpoint_hash:current.endpoint_hash,at,last_id:None}} else {old.as_ref().unwrap().clone()};
        let (source,_)=sources(tx,request,validity,head.endpoint_version,&head.endpoint_hash,head.last,head.ceiling)?;
        let Some(source)=source else{return reject(tx,id,ErrorCode::NotFound)};
        if let Err(e)=quota(tx,"imap_uid_reconcile_requests",100_000,1){return reject(tx,id,e)}
        if old.is_none() && let Err(e)=quota(tx,"imap_uid_reconcile_heads",100_000,1){return reject(tx,id,e)}
        let fetch=ImapUidReconcileRequest::new(request.account_id(),head.endpoint_version,current.fetch.mailbox().clone(),validity,source.page.request().after_uid(),source.page.covered_through_uid(),minimum).map_err(|_|ErrorCode::InternalError)?;
        if new_pass {
            let changed=db(tx.execute("INSERT INTO imap_uid_reconcile_heads(account_id,mailbox_key,mailbox_encoding,generation,version,frozen_cursor_version,last_source_cursor_version,phase,uid_validity,endpoint_version,endpoint_hash,updated_at_utc_ms,last_execution_id) VALUES(?1,?2,?3,?4,?5,?6,0,'open',?7,?8,?9,?10,NULL) ON CONFLICT(account_id,mailbox_key,mailbox_encoding) DO UPDATE SET generation=excluded.generation,version=excluded.version,frozen_cursor_version=excluded.frozen_cursor_version,last_source_cursor_version=0,phase='open',updated_at_utc_ms=excluded.updated_at_utc_ms,last_execution_id=NULL",params![request.account_id(),request.mailbox().wire_hex(),request.mailbox().encoding().as_str(),as_i64(head.generation)?,as_i64(head.version)?,as_i64(head.ceiling)?,i64::from(validity),as_i64(head.endpoint_version)?,head.endpoint_hash.as_slice(),at]))?;
            if changed!=1{return Err(ErrorCode::InternalError)}
        }
        let error=match queue{UidQueueAdmission::Reserved=>None,UidQueueAdmission::Full=>Some(ErrorCode::RateLimited),UidQueueAdmission::Closed=>Some(ErrorCode::CoreUnavailable)};
        let changed=db(tx.execute("INSERT INTO imap_uid_reconcile_requests(execution_id,operation_id,account_id,idempotency_key,mailbox_key,mailbox_encoding,intent_hash,expected_catalog_generation,expected_cursor_version,bound_account_version,bound_endpoint_version,bound_endpoint_hash,uid_validity,minimum_uid_next,generation,head_version,frozen_cursor_version,source_operation_id,source_cursor_version,source_page_hash,after_uid,through_uid,state,admitted_at_utc_ms,completed_at_utc_ms,error_code) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)",params![id.as_slice(),request.operation_id(),request.account_id(),request.idempotency_key(),request.mailbox().wire_hex(),request.mailbox().encoding().as_str(),intent.as_slice(),as_i64(request.expected_catalog_generation())?,as_i64(request.expected_cursor_version())?,as_i64(current.av)?,as_i64(head.endpoint_version)?,head.endpoint_hash.as_slice(),i64::from(validity),i64::from(minimum),as_i64(head.generation)?,as_i64(head.version)?,as_i64(head.ceiling)?,source.id,as_i64(source.receipt.cursor_version())?,hash(source.json.as_bytes())?.as_slice(),i64::from(fetch.after_uid()),i64::from(fetch.through_uid()),if error.is_some(){"failed"}else{"admitted"},at,error.map(|_|at),error.map(ErrorCode::as_str)]))?;
        if changed!=1{return Err(ErrorCode::InternalError)}
        if let Some(e)=error{insert_audit_terminal_tx(tx,id,AuditTerminal::failed(e),at)?;}
        let receipt=read_row(tx,id)?.ok_or(ErrorCode::InternalError)?.receipt;
        read_head(tx,request)?.ok_or(ErrorCode::InternalError)?;
        Ok(if error.is_some(){UidReconcileAdmission::Failed(receipt)}else{UidReconcileAdmission::Admitted(receipt)})
    })
}
fn fail_row(
    tx: &Transaction<'_>,
    row: &Row,
    error: ErrorCode,
) -> Result<UidReconcileReceipt, ErrorCode> {
    if row.completed.is_some() {
        return Err(ErrorCode::Conflict);
    }
    let at = now()?;
    if at < row.started.unwrap_or(row.admitted) {
        return Err(ErrorCode::InternalError);
    }
    let id = *row.audit.execution_id();
    if db(tx.execute("UPDATE imap_uid_reconcile_requests SET state='failed',completed_at_utc_ms=?2,error_code=?3 WHERE execution_id=?1 AND state IN ('admitted','running')",params![id.as_slice(),at,error.as_str()]))?!=1{return Err(ErrorCode::InternalError)}
    insert_audit_terminal_tx(tx, id, AuditTerminal::failed(error), at)?;
    Ok(read_row(tx, id)?.ok_or(ErrorCode::InternalError)?.receipt)
}
pub(super) fn start(store: &Store, id: [u8; 32]) -> Result<UidReconcileStart, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate,|tx|{
        let row=read_row(tx,id)?.ok_or(ErrorCode::NotFound)?;
        if row.completed.is_some(){return Ok(UidReconcileStart::Finished(row.receipt))}
        if row.state!="admitted"{return Err(ErrorCode::Conflict)}
        let current=match current_binding(tx,&row){Ok(v)=>v,Err(ErrorCode::InternalError)=>return Err(ErrorCode::InternalError),Err(e)=>return fail_row(tx,&row,e).map(UidReconcileStart::Finished)};
        let at=now()?;if at<row.admitted{return Err(ErrorCode::InternalError)}
        if db(tx.execute("UPDATE imap_uid_reconcile_requests SET state='running',started_at_utc_ms=?2 WHERE execution_id=?1 AND state='admitted'",params![id.as_slice(),at]))?!=1{return Err(ErrorCode::InternalError)}
        read_row(tx,id)?.ok_or(ErrorCode::InternalError)?;
        Ok(UidReconcileStart::Ready(Box::new(UidReconcileJob{execution_id:id,request:row.request,fetch_request:row.fetch,endpoint:current.endpoint})))
    })
}
pub(super) fn check(store: &Store, id: [u8; 32]) -> Result<(), ErrorCode> {
    store.mail_transaction(TransactionBehavior::Deferred, |tx| {
        let row = read_row(tx, id)?.ok_or(ErrorCode::NotFound)?;
        if row.state != "running" {
            return Err(ErrorCode::Conflict);
        }
        current_binding(tx, &row).map(|_| ())
    })
}
fn previous(c: &Connection, row: &Row) -> Result<Option<Row>, ErrorCode> {
    let last = latest(c, &row.source.id)?;
    if let Some(last) = &last {
        if last.generation >= row.generation {
            return Err(ErrorCode::InternalError);
        }
        validate_overlay(c, last)?;
    } else {
        let count:i64=db(c.query_row("SELECT count(*) FROM imap_uid_reconcile_messages m JOIN imap_uid_messages b USING(account_id,mailbox_key,mailbox_encoding,uid_validity,uid) WHERE b.operation_id=?1",[&row.source.id],|r|r.get(0)))?;
        if count != 0 {
            return Err(ErrorCode::InternalError);
        }
    }
    Ok(last)
}
fn prepare_result(
    c: &Connection,
    row: &Row,
    page: &ImapUidReconcilePage,
) -> Result<ObservationResult, ErrorCode> {
    if page.request() != &row.fetch {
        return Err(ErrorCode::InvalidInput);
    }
    let old = previous(c, row)?;
    let old_observations = old
        .as_ref()
        .and_then(|r| r.result.as_ref())
        .map(|r| r.observations.as_slice());
    let mut observations = Vec::with_capacity(row.source.page.messages().len());
    let mut changed = 0;
    let mut absent = 0;
    for incoming in page.messages() {
        let Some(base) = row
            .source
            .page
            .messages()
            .iter()
            .find(|m| m.uid() == incoming.uid())
        else {
            return Err(ErrorCode::Conflict);
        };
        if incoming.internal_date_utc_ms() != base.internal_date_utc_ms()
            || incoming.size() != base.size()
            || incoming.envelope() != base.envelope()
        {
            return Err(ErrorCode::Conflict);
        }
    }
    for (index, base) in row.source.page.messages().iter().enumerate() {
        let before = old_observations.map_or(Some(base.flags()), |old| old[index].flags.as_deref());
        let flags = page
            .messages()
            .iter()
            .find(|m| m.uid() == base.uid())
            .map(|m| m.flags().to_vec());
        if before.is_none() && flags.is_some() {
            return Err(ErrorCode::Conflict);
        }
        match (before, flags.as_deref()) {
            (Some(before), Some(after)) if before != after => changed += 1,
            (Some(_), None) => absent += 1,
            _ => {}
        }
        observations.push(Observation {
            uid: base.uid(),
            flags,
        });
    }
    let (_, remaining) = source_scan(
        c,
        &row.request,
        row.source.receipt.cursor_version(),
        row.ceiling,
    )?;
    let progress = UidReconcileProgress::new(
        row.generation,
        row.ceiling,
        row.source.receipt.cursor_version(),
        row.fetch.after_uid(),
        row.fetch.through_uid(),
        observations.len() as u32,
        changed,
        absent,
        remaining,
        remaining == 0,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    let json = canonical(&result_value(
        row,
        &progress,
        page.uid_next_at_start(),
        &observations,
    )?)?;
    if json.len() > MAX_RESULT {
        return Err(ErrorCode::QuotaExceeded);
    }
    Ok(ObservationResult {
        progress,
        observations,
        json,
    })
}
pub(super) fn succeed(
    store: &Store,
    id: [u8; 32],
    page: &ImapUidReconcilePage,
) -> Result<UidReconcileReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate,|tx|{
        let row=read_row(tx,id)?.ok_or(ErrorCode::NotFound)?;
        if row.state!="running"{return Err(ErrorCode::Conflict)}
        match current_binding(tx,&row){Ok(_)=>{},Err(ErrorCode::InternalError)=>return Err(ErrorCode::InternalError),Err(e)=>return fail_row(tx,&row,e)}
        let result=match prepare_result(tx,&row,page){Ok(v)=>v,Err(e @ (ErrorCode::Conflict|ErrorCode::InvalidInput|ErrorCode::QuotaExceeded))=>return fail_row(tx,&row,e),Err(e)=>return Err(e)};
        let previous=previous(tx,&row)?;
        if let Err(e)=quota(tx,"imap_uid_reconcile_messages",1_000_000,if previous.is_none(){result.observations.len() as i64}else{0}){
            if e==ErrorCode::InternalError{return Err(e)} return fail_row(tx,&row,e)
        }
        let at=now()?;
        let head=read_head(tx,&row.request)?.ok_or(ErrorCode::InternalError)?;
        if at<row.started.ok_or(ErrorCode::InternalError)? || at<head.at || previous.as_ref().and_then(|r|r.completed).is_some_and(|previous|at<previous){return Err(ErrorCode::InternalError)}
        for o in &result.observations {
            let flags=o.flags.as_ref().map(|v|canonical(&json!(v))).transpose()?;
            if db(tx.execute("INSERT INTO imap_uid_reconcile_messages(account_id,mailbox_key,mailbox_encoding,uid_validity,uid,presence,flags_json,generation,observed_at_utc_ms,last_execution_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(account_id,mailbox_key,mailbox_encoding,uid_validity,uid) DO UPDATE SET presence=excluded.presence,flags_json=excluded.flags_json,generation=excluded.generation,observed_at_utc_ms=excluded.observed_at_utc_ms,last_execution_id=excluded.last_execution_id",params![row.request.account_id(),row.request.mailbox().wire_hex(),row.request.mailbox().encoding().as_str(),i64::from(row.fetch.uid_validity()),i64::from(o.uid),if o.flags.is_some(){"present"}else{"remote_absent"},flags,as_i64(row.generation)?,at,id.as_slice()]))?!=1{return Err(ErrorCode::InternalError)}
        }
        if db(tx.execute("UPDATE imap_uid_reconcile_heads SET version=?4,last_source_cursor_version=?5,phase=?6,updated_at_utc_ms=?7,last_execution_id=?8 WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND version=?9 AND generation=?10 AND phase='open'",params![row.request.account_id(),row.request.mailbox().wire_hex(),row.request.mailbox().encoding().as_str(),as_i64(bumped(row.head_version)?)?,as_i64(row.source.receipt.cursor_version())?,if result.progress.pass_complete(){"complete"}else{"open"},at,id.as_slice(),as_i64(row.head_version)?,as_i64(row.generation)?]))?!=1{return Err(ErrorCode::InternalError)}
        if db(tx.execute("UPDATE imap_uid_reconcile_requests SET state='succeeded',completed_at_utc_ms=?2,result_json=?3,result_hash=?4 WHERE execution_id=?1 AND state='running'",params![id.as_slice(),at,result.json,hash(result.json.as_bytes())?.as_slice()]))?!=1{return Err(ErrorCode::InternalError)}
        insert_audit_terminal_tx(tx,id,AuditTerminal::succeeded(None).map_err(|_|ErrorCode::InternalError)?,at)?;
        let receipt=read_row(tx,id)?.ok_or(ErrorCode::InternalError)?.receipt;
        read_head(tx,&row.request)?.ok_or(ErrorCode::InternalError)?;
        Ok(receipt)
    })
}
pub(super) fn fail(
    store: &Store,
    id: [u8; 32],
    error: ErrorCode,
) -> Result<UidReconcileReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate, |tx| {
        let row = read_row(tx, id)?.ok_or(ErrorCode::NotFound)?;
        fail_row(tx, &row, error)
    })
}
pub(super) fn receipt(
    store: &Store,
    owner: [u8; 16],
    request: &UidPageStatusRequest,
) -> Result<UidReconcileReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Deferred, |tx| {
        let row = read_row(tx, *request.execution_id())?.ok_or(ErrorCode::NotFound)?;
        if row.audit.registration_id() != &owner || row.request.account_id() != request.account_id()
        {
            return Err(ErrorCode::NotFound);
        }
        Ok(row.receipt)
    })
}
pub(super) fn observation(
    store: &Store,
    account: &str,
    name: &ImapMailboxName,
    validity: u32,
    uid: u32,
) -> Result<Option<StoredImapUidObservation>, ErrorCode> {
    // Existing strict metadata lookup detects deleted base rows hidden as UID gaps.
    if store
        .load_imap_uid_metadata(account, name, validity, uid)?
        .is_none()
    {
        return Ok(None);
    }
    store.mail_transaction(TransactionBehavior::Deferred,|tx|{
        let encoding=match name.encoding(){hantong_domain::mailboxes::MailboxEncoding::ModifiedUtf7=>"modified_utf7",hantong_domain::mailboxes::MailboxEncoding::Utf8=>"utf8"};
        let source:String=db(tx.query_row("SELECT operation_id FROM imap_uid_messages WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4 AND uid=?5",params![account,hantong_contracts::wire_hex::encode(name.wire_name()),encoding,i64::from(validity),i64::from(uid)],|r|Ok(text_column(r,0,128)?.to_owned())))?;
        let Some(row)=latest(tx,&source)? else {
            let exists:bool=db(tx.query_row("SELECT EXISTS(SELECT 1 FROM imap_uid_reconcile_messages WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4 AND uid=?5)",params![account,hantong_contracts::wire_hex::encode(name.wire_name()),encoding,i64::from(validity),i64::from(uid)],|r|r.get(0)))?;
            if exists{return Err(ErrorCode::InternalError)}return Ok(None)
        };
        validate_overlay(tx,&row)?;
        let o=row.result.as_ref().ok_or(ErrorCode::InternalError)?.observations.iter().find(|o|o.uid==uid).ok_or(ErrorCode::InternalError)?;
        Ok(Some(StoredImapUidObservation{flags:o.flags.clone(),generation:row.generation,observed_at_utc_ms:row.completed.ok_or(ErrorCode::InternalError)?,execution_id:*row.audit.execution_id()}))
    })
}
pub(crate) fn validate_history(c: &Connection) -> rusqlite::Result<()> {
    for (table, limit) in [
        ("imap_uid_reconcile_heads", 100_000),
        ("imap_uid_reconcile_requests", 100_000),
        ("imap_uid_reconcile_messages", 1_000_000),
    ] {
        let n: i64 = c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
        if n > limit {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    Ok(())
}
