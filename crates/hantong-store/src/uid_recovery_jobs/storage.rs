use super::*;
use crate::uid_epochs::Epoch;
use crate::{
    audit::{insert_audit_terminal_tx, read_audit},
    imap_catalog::{endpoint_hash, load_catalog},
    imap_sync::storage::{apply_recovery_tx, current_endpoint, read_cursor, read_operation},
    mail_connection::{MailMutation, check_account},
    registrations::{blob_column, text_column},
    uid_page_jobs::storage::current_grants,
};
use hantong_contracts::uid_recovery::UidRecoveryProgress;
use hantong_contracts::{
    audit::{AuditRecord, AuditTerminal, AuditTool},
    uid_page::{UidMailboxEncoding, UidMailboxIdentity, UidPageState},
};
use hantong_domain::mailboxes::{ImapMailboxName, MailboxEncoding};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    r.map_err(|_| ErrorCode::InternalError)
}
fn hash(bytes: &[u8]) -> Result<[u8; 32], ErrorCode> {
    hantong_platform::sha256(bytes).map_err(|_| ErrorCode::InternalError)
}
fn now() -> Result<i64, ErrorCode> {
    db(crate::utc_millis())
}
pub(crate) fn mailbox(identity: &UidMailboxIdentity) -> Result<ImapMailboxName, ErrorCode> {
    let wire = hantong_contracts::wire_hex::decode_bounded(identity.wire_hex(), 1024)?;
    let encoding = match identity.encoding() {
        UidMailboxEncoding::ModifiedUtf7 => MailboxEncoding::ModifiedUtf7,
        UidMailboxEncoding::Utf8 => MailboxEncoding::Utf8,
    };
    let name = ImapMailboxName::from_wire(&wire, encoding).map_err(|_| ErrorCode::InvalidInput)?;
    if name.wire_name() != wire || name.encoding() != encoding {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(name)
}
struct RawRow {
    account: String,
    operation: String,
    key: String,
    mailbox_key: String,
    encoding: String,
    intent: [u8; 32],
    cg: i64,
    cv: i64,
    av: i64,
    ev: i64,
    endpoint_hash: [u8; 32],
    validity: i64,
    epoch: i64,
    state: String,
    admitted: i64,
    started: Option<i64>,
    completed: Option<i64>,
    result_version: Option<i64>,
    result_epoch: Option<i64>,
    result_validity: Option<i64>,
    covered: Option<i64>,
    inserted: Option<i64>,
    error: Option<String>,
    page_operation: Option<String>,
    page_hash: Option<[u8; 32]>,
    payload_hash: Option<[u8; 32]>,
}
struct Row {
    request: UidRecoveryRequest,
    audit: AuditRecord,
    fetch: ImapUidRecoveryRequest,
    av: u64,
    epoch: u64,
    endpoint_hash: [u8; 32],
    state: String,
    admitted: i64,
    started: Option<i64>,
    completed: Option<i64>,
    receipt: UidRecoveryReceipt,
}
fn optional_text(
    r: &rusqlite::Row<'_>,
    index: usize,
    max: usize,
) -> rusqlite::Result<Option<String>> {
    if matches!(r.get_ref(index)?, rusqlite::types::ValueRef::Null) {
        Ok(None)
    } else {
        Ok(Some(text_column(r, index, max)?.to_owned()))
    }
}
fn optional_hash(r: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<[u8; 32]>> {
    if matches!(r.get_ref(index)?, rusqlite::types::ValueRef::Null) {
        Ok(None)
    } else {
        Ok(Some(blob_column::<32>(r, index)?))
    }
}
fn read_row_core(c: &Connection, id: [u8; 32]) -> Result<Option<Row>, ErrorCode> {
    let raw=db(c.query_row("SELECT account_id,operation_id,idempotency_key,mailbox_key,mailbox_encoding,intent_hash,expected_catalog_generation,expected_cursor_version,bound_account_version,bound_endpoint_version,bound_endpoint_hash,previous_uid_validity,bound_epoch_generation,state,admitted_at_utc_ms,started_at_utc_ms,completed_at_utc_ms,result_cursor_version,result_covered_through_uid,result_inserted_count,error_code,page_operation_id,result_page_hash,result_payload_hash,result_epoch_generation,result_uid_validity FROM imap_uid_recovery_requests WHERE execution_id=?1",[id.as_slice()],|r|Ok(RawRow{
        account:text_column(r,0,128)?.into(),operation:text_column(r,1,128)?.into(),key:text_column(r,2,128)?.into(),mailbox_key:text_column(r,3,2048)?.into(),encoding:text_column(r,4,13)?.into(),intent:blob_column::<32>(r,5)?,cg:r.get(6)?,cv:r.get(7)?,av:r.get(8)?,ev:r.get(9)?,endpoint_hash:blob_column::<32>(r,10)?,validity:r.get(11)?,epoch:r.get(12)?,state:text_column(r,13,10)?.into(),admitted:r.get(14)?,started:r.get(15)?,completed:r.get(16)?,result_version:r.get(17)?,covered:r.get(18)?,inserted:r.get(19)?,error:optional_text(r,20,32)?,page_operation:optional_text(r,21,128)?,page_hash:optional_hash(r,22)?,payload_hash:optional_hash(r,23)?,result_epoch:r.get(24)?,result_validity:r.get(25)?
    })).optional())?;
    let Some(raw) = raw else { return Ok(None) };
    if id == [0; 32]
        || raw.cg <= 0
        || !(1..i64::MAX).contains(&raw.cv)
        || raw.av <= 0
        || raw.ev <= 0
        || !(1..=i64::from(u32::MAX)).contains(&raw.validity)
        || raw.epoch <= 0
        || raw.epoch > raw.cv
        || raw.admitted <= 0
        || raw.started.is_some_and(|v| v < raw.admitted)
        || raw
            .completed
            .is_some_and(|v| v < raw.started.unwrap_or(raw.admitted))
    {
        return Err(ErrorCode::InternalError);
    }
    let identity = UidMailboxIdentity::new(
        &raw.mailbox_key,
        UidMailboxEncoding::parse(&raw.encoding).map_err(|_| ErrorCode::InternalError)?,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    let name = mailbox(&identity).map_err(|_| ErrorCode::InternalError)?;
    let request = UidRecoveryRequest::new(
        &raw.account,
        identity,
        &raw.operation,
        &raw.key,
        raw.cg as u64,
        raw.cv as u64,
        &hantong_contracts::wire_hex::encode(&raw.intent),
    )
    .map_err(|_| ErrorCode::InternalError)?;
    if hash(&request.intent_bytes())? != raw.intent {
        return Err(ErrorCode::InternalError);
    }
    let fetch = ImapUidRecoveryRequest::new(&raw.account, raw.ev as u64, name, raw.validity as u32)
        .map_err(|_| ErrorCode::InternalError)?;
    let audit = db(read_audit(c, &id))?.ok_or(ErrorCode::InternalError)?;
    if audit.tool() != AuditTool::SyncPage
        || audit.account_ids() != [raw.account.clone()]
        || raw.admitted < audit.started_at_utc_ms()
    {
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
    let no_result = raw.result_version.is_none()
        && raw.result_epoch.is_none()
        && raw.result_validity.is_none()
        && raw.covered.is_none()
        && raw.inserted.is_none()
        && raw.page_operation.is_none()
        && raw.page_hash.is_none()
        && raw.payload_hash.is_none();
    let state = match raw.state.as_str() {
        "admitted"
            if raw.started.is_none() && raw.completed.is_none() && no_result && error.is_none() =>
        {
            UidPageState::Unresolved
        }
        "running"
            if raw.started.is_some() && raw.completed.is_none() && no_result && error.is_none() =>
        {
            UidPageState::Unresolved
        }
        "failed" if raw.completed.is_some() && no_result && error.is_some() => UidPageState::Failed,
        "succeeded"
            if raw.started.is_some()
                && raw.completed.is_some()
                && raw.result_version == Some(raw.cv + 1)
                && raw.result_epoch == Some(raw.epoch + 1)
                && raw.result_validity.is_some_and(|v| {
                    (1..=i64::from(u32::MAX)).contains(&v) && v != raw.validity
                })
                && raw.covered.is_some_and(|v| (0..=64).contains(&v))
                && raw.inserted.is_some_and(|v| (0..=64).contains(&v))
                && raw.page_operation.as_deref() == Some(&raw.operation)
                && raw.page_hash.is_some()
                && raw.payload_hash.is_some()
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
    let result = if state == UidPageState::Succeeded {
        let op = read_operation(c, &raw.operation)?.ok_or(ErrorCode::InternalError)?;
        ImapUidRecoveryPage::new(fetch.clone(), op.page.clone())
            .map_err(|_| ErrorCode::InternalError)?;
        if op.id != raw.operation
            || op.key != raw.key
            || op.av != raw.av as u64
            || op.cg != raw.cg as u64
            || op.expected != raw.cv as u64
            || op.endpoint_hash != raw.endpoint_hash
            || Some(op.receipt.cursor_version() as i64) != raw.result_version
            || Some(i64::from(op.receipt.covered_through_uid())) != raw.covered
            || Some(i64::from(op.receipt.inserted_count())) != raw.inserted
            || Some(i64::from(op.page.uid_validity())) != raw.result_validity
            || Some(hash(op.json.as_bytes())?) != raw.page_hash
            || Some(op.payload_hash) != raw.payload_hash
            || op.at < raw.started.unwrap()
            || op.at > raw.completed.unwrap()
        {
            return Err(ErrorCode::InternalError);
        }
        Some(
            UidRecoveryProgress::new(
                raw.result_epoch.unwrap() as u64,
                op.receipt.cursor_version(),
                op.receipt.covered_through_uid(),
                op.receipt.inserted_count(),
            )
            .map_err(|_| ErrorCode::InternalError)?,
        )
    } else {
        if page_operation_exists(c, &raw.account, &raw.operation, &raw.key)? {
            return Err(ErrorCode::InternalError);
        }
        None
    };
    let receipt = UidRecoveryReceipt::new(&raw.account, id, state, result, error)
        .map_err(|_| ErrorCode::InternalError)?;
    Ok(Some(Row {
        request,
        audit,
        fetch,
        av: raw.av as u64,
        epoch: raw.epoch as u64,
        endpoint_hash: raw.endpoint_hash,
        state: raw.state,
        admitted: raw.admitted,
        started: raw.started,
        completed: raw.completed,
        receipt,
    }))
}
fn read_row(c: &Connection, id: [u8; 32]) -> Result<Option<Row>, ErrorCode> {
    let row = read_row_core(c, id)?;
    if let Some(row) = &row {
        let cursor = read_cursor(c, row.request.account_id(), row.fetch.mailbox())?
            .ok_or(ErrorCode::InternalError)?;
        if cursor.value.version() < row.request.expected_cursor_version() {
            return Err(ErrorCode::InternalError);
        }
        let old = crate::uid_epochs::read(
            c,
            row.request.account_id(),
            row.fetch.mailbox(),
            row.fetch.previous_uid_validity(),
        )?
        .ok_or(ErrorCode::InternalError)?;
        if old.generation != row.epoch
            || old.first > row.request.expected_cursor_version()
            || old
                .last
                .is_some_and(|last| last < row.request.expected_cursor_version())
        {
            return Err(ErrorCode::InternalError);
        }
        if let Some(result) = row.receipt.result() {
            let op =
                read_operation(c, row.request.operation_id())?.ok_or(ErrorCode::InternalError)?;
            let epoch = crate::uid_epochs::read(
                c,
                row.request.account_id(),
                row.fetch.mailbox(),
                op.page.uid_validity(),
            )?
            .ok_or(ErrorCode::InternalError)?;
            if epoch.generation != result.epoch_generation()
                || epoch.first != result.cursor_version()
                || epoch.first_id != op.id
                || epoch.recovery != Some(id)
                || old.last != Some(row.request.expected_cursor_version())
                || cursor.value.version() < result.cursor_version()
            {
                return Err(ErrorCode::InternalError);
            }
        }
    }
    Ok(row)
}
pub(crate) fn validate_epoch_link(
    c: &Connection,
    previous: &Epoch,
    epoch: &Epoch,
    first: &crate::imap_sync::storage::Operation,
) -> Result<(), ErrorCode> {
    let id = epoch.recovery.ok_or(ErrorCode::InternalError)?;
    let row = read_row_core(c, id)?.ok_or(ErrorCode::InternalError)?;
    let result = row.receipt.result().ok_or(ErrorCode::InternalError)?;
    if row.request.operation_id() != first.id
        || row.request.account_id() != epoch.account
        || row.fetch.mailbox().wire_name() != epoch.mailbox.wire_name()
        || row.fetch.mailbox().encoding() != epoch.mailbox.encoding()
        || row.fetch.previous_uid_validity() != previous.validity
        || row.epoch != previous.generation
        || Some(row.request.expected_cursor_version()) != previous.last
        || row.epoch.checked_add(1) != Some(epoch.generation)
        || result.epoch_generation() != epoch.generation
        || result.cursor_version() != epoch.first
        || epoch.first_id != first.id
        || epoch.created != first.at
        || epoch.endpoint_hash != row.endpoint_hash
        || epoch.endpoint_version != row.fetch.endpoint_version()
    {
        return Err(ErrorCode::InternalError);
    }
    Ok(())
}
struct Binding {
    av: u64,
    epoch: u64,
    endpoint: ImapEndpointConfig,
    endpoint_hash: [u8; 32],
    fetch: ImapUidRecoveryRequest,
}
fn binding(c: &Transaction<'_>, request: &UidRecoveryRequest) -> Result<Binding, ErrorCode> {
    let name = mailbox(request.mailbox())?;
    check_account(c, request.account_id())?;
    let (av, state): (i64, String) = db(c.query_row(
        "SELECT settings_version,status FROM accounts WHERE account_id=?1",
        [request.account_id()],
        |r| Ok((r.get(0)?, text_column(r, 1, 32)?.into())),
    ))?;
    if av <= 0 {
        return Err(ErrorCode::InternalError);
    }
    if hantong_domain::accounts::AccountStatus::parse(&state)
        .map_err(|_| ErrorCode::InternalError)?
        != hantong_domain::accounts::AccountStatus::Ready
    {
        return Err(ErrorCode::Conflict);
    }
    let catalog = load_catalog(c, request.account_id())?.ok_or(ErrorCode::NotFound)?;
    if catalog.generation() != request.expected_catalog_generation()
        || !catalog.binding_current()
        || catalog.account_version() != av as u64
    {
        return Err(ErrorCode::Conflict);
    }
    if !catalog.snapshot().mailboxes().iter().any(|m| {
        m.selectable()
            && m.name().wire_name() == name.wire_name()
            && m.name().encoding() == name.encoding()
    }) {
        return Err(ErrorCode::NotFound);
    }
    let endpoint = current_endpoint(c, request.account_id())?.ok_or(ErrorCode::Conflict)?;
    let endpoint_hash = endpoint_hash(request.account_id(), &endpoint)?;
    if endpoint.settings_version() != catalog.endpoint_version() {
        return Err(ErrorCode::Conflict);
    }
    let cursor = read_cursor(c, request.account_id(), &name)?.ok_or(ErrorCode::NotFound)?;
    if cursor.value.version() != request.expected_cursor_version()
        || cursor.value.endpoint_version() != endpoint.settings_version()
        || cursor.endpoint_hash != endpoint_hash
    {
        return Err(ErrorCode::Conflict);
    }
    let epoch =
        crate::uid_epochs::read(c, request.account_id(), &name, cursor.value.uid_validity())?
            .ok_or(ErrorCode::InternalError)?;
    if epoch.last.is_some() {
        return Err(ErrorCode::Conflict);
    }
    let fetch = ImapUidRecoveryRequest::new(
        request.account_id(),
        endpoint.settings_version(),
        name,
        cursor.value.uid_validity(),
    )
    .map_err(|_| ErrorCode::InternalError)?;
    Ok(Binding {
        av: av as u64,
        epoch: epoch.generation,
        endpoint,
        endpoint_hash,
        fetch,
    })
}
fn current_binding(c: &Transaction<'_>, row: &Row) -> Result<Binding, ErrorCode> {
    current_grants(c, &row.audit)?;
    let current = binding(c, &row.request)?;
    if current.av != row.av
        || current.epoch != row.epoch
        || current.endpoint_hash != row.endpoint_hash
        || current.fetch != row.fetch
    {
        return Err(ErrorCode::Conflict);
    }
    Ok(current)
}
fn reject(
    tx: &Transaction<'_>,
    id: [u8; 32],
    error: ErrorCode,
) -> Result<UidRecoveryAdmission, ErrorCode> {
    if error == ErrorCode::InternalError {
        return Err(error);
    }
    insert_audit_terminal_tx(tx, id, AuditTerminal::failed(error), now()?)?;
    Ok(UidRecoveryAdmission::Rejected(error))
}
pub(crate) fn operation_reserved(
    c: &Connection,
    account: &str,
    operation: &str,
    key: &str,
) -> Result<bool, ErrorCode> {
    db(c.query_row("SELECT EXISTS(SELECT 1 FROM imap_uid_recovery_requests WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3))",params![operation,account,key],|r|r.get(0)))
}
pub(crate) fn page_operation_exists(
    c: &Connection,
    account: &str,
    operation: &str,
    key: &str,
) -> Result<bool, ErrorCode> {
    db(c.query_row("SELECT EXISTS(SELECT 1 FROM imap_uid_page_operations WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3))",params![operation,account,key],|r|r.get(0)))
}
pub(super) fn admit(
    store: &Store,
    id: [u8; 32],
    request: &UidRecoveryRequest,
    queue: UidQueueAdmission,
) -> Result<UidRecoveryAdmission, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate,|tx|{
        let audit=db(read_audit(tx,&id))?.ok_or(ErrorCode::NotFound)?;
        if audit.completion().is_some() || execution_reserved(tx,id)? || crate::uid_page_jobs::execution_reserved(tx,id)? || crate::uid_reconcile_jobs::execution_reserved(tx,id)? {return Err(ErrorCode::Conflict)}
        if id==[0;32] || audit.tool()!=AuditTool::SyncPage || audit.account_ids()!=[request.account_id()]{return Err(ErrorCode::InvalidInput)}
        if let Err(e)=current_grants(tx,&audit){return reject(tx,id,e)}
        if let Err(e)=mailbox(request.mailbox()){return reject(tx,id,e)}
        let intent=hash(&request.intent_bytes())?;
        if hantong_contracts::wire_hex::encode(&intent)!=request.intent_hash(){return reject(tx,id,ErrorCode::InvalidInput)}
        if crate::uid_page_jobs::operation_reserved(tx,request.account_id(),request.operation_id(),request.idempotency_key())? || crate::uid_reconcile_jobs::operation_reserved(tx,request.account_id(),request.operation_id(),request.idempotency_key())? {return reject(tx,id,ErrorCode::Conflict)}
        let ids={let mut s=db(tx.prepare("SELECT execution_id FROM imap_uid_recovery_requests WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3) LIMIT 2"))?;db(db(s.query_map(params![request.operation_id(),request.account_id(),request.idempotency_key()],|r|blob_column::<32>(r,0)))?.collect::<rusqlite::Result<Vec<_>>>())?};
        if !ids.is_empty(){
            if ids.len()!=1{return reject(tx,id,ErrorCode::Conflict)}
            if ids[0]==id{return Err(ErrorCode::Conflict)}
            let row=read_row(tx,ids[0])?.ok_or(ErrorCode::InternalError)?;
            if row.request!=*request || row.audit.registration_id()!=audit.registration_id(){return reject(tx,id,ErrorCode::Conflict)}
            insert_audit_terminal_tx(tx,id,AuditTerminal::succeeded(None).map_err(|_|ErrorCode::InternalError)?,now()?)?;
            return Ok(UidRecoveryAdmission::Existing(row.receipt))
        }
        if page_operation_exists(tx,request.account_id(),request.operation_id(),request.idempotency_key())? {return reject(tx,id,ErrorCode::Conflict)}
        let current=match binding(tx,request){Ok(v)=>v,Err(e)=>return reject(tx,id,e)};
        let count:i64=db(tx.query_row("SELECT count(*) FROM imap_uid_recovery_requests",[],|r|r.get(0)))?;
        if count>100_000{return Err(ErrorCode::InternalError)}
        if count==100_000{return reject(tx,id,ErrorCode::QuotaExceeded)}
        let at=now()?;if at<audit.started_at_utc_ms(){return Err(ErrorCode::InternalError)}
        let error=match queue{UidQueueAdmission::Reserved=>None,UidQueueAdmission::Full=>Some(ErrorCode::RateLimited),UidQueueAdmission::Closed=>Some(ErrorCode::CoreUnavailable)};
        db(tx.execute("INSERT INTO imap_uid_recovery_requests(execution_id,operation_id,account_id,idempotency_key,mailbox_key,mailbox_encoding,intent_hash,expected_catalog_generation,expected_cursor_version,bound_account_version,bound_endpoint_version,bound_endpoint_hash,previous_uid_validity,bound_epoch_generation,state,admitted_at_utc_ms,completed_at_utc_ms,error_code) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",params![id.as_slice(),request.operation_id(),request.account_id(),request.idempotency_key(),request.mailbox().wire_hex(),request.mailbox().encoding().as_str(),intent.as_slice(),request.expected_catalog_generation() as i64,request.expected_cursor_version() as i64,current.av as i64,current.endpoint.settings_version() as i64,current.endpoint_hash.as_slice(),i64::from(current.fetch.previous_uid_validity()),current.epoch as i64,if error.is_some(){"failed"}else{"admitted"},at,error.map(|_|at),error.map(ErrorCode::as_str)]))?;
        if let Some(error)=error {insert_audit_terminal_tx(tx,id,AuditTerminal::failed(error),at)?;}
        let receipt=read_row(tx,id)?.ok_or(ErrorCode::InternalError)?.receipt;
        Ok(if error.is_some(){UidRecoveryAdmission::Failed(receipt)}else{UidRecoveryAdmission::Admitted(receipt)})
    })
}
fn fail_row(
    tx: &Transaction<'_>,
    row: &Row,
    error: ErrorCode,
) -> Result<UidRecoveryReceipt, ErrorCode> {
    if row.completed.is_some() {
        return Err(ErrorCode::Conflict);
    }
    let at = now()?;
    if at < row.started.unwrap_or(row.admitted) {
        return Err(ErrorCode::InternalError);
    }
    let id = *row.audit.execution_id();
    if db(tx.execute("UPDATE imap_uid_recovery_requests SET state='failed',completed_at_utc_ms=?2,error_code=?3 WHERE execution_id=?1 AND state IN ('admitted','running')",params![id.as_slice(),at,error.as_str()]))?!=1{return Err(ErrorCode::InternalError)}
    insert_audit_terminal_tx(tx, id, AuditTerminal::failed(error), at)?;
    Ok(read_row(tx, id)?.ok_or(ErrorCode::InternalError)?.receipt)
}
pub(super) fn start(store: &Store, id: [u8; 32]) -> Result<UidRecoveryStart, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate,|tx|{
        let row=read_row(tx,id)?.ok_or(ErrorCode::NotFound)?;
        if row.completed.is_some(){return Ok(UidRecoveryStart::Finished(row.receipt))}
        if row.state!="admitted"{return Err(ErrorCode::Conflict)}
        let current=match current_binding(tx,&row){Ok(v)=>v,Err(ErrorCode::InternalError)=>return Err(ErrorCode::InternalError),Err(e)=>return fail_row(tx,&row,e).map(UidRecoveryStart::Finished)};
        let at=now()?;if at<row.admitted{return Err(ErrorCode::InternalError)}
        if db(tx.execute("UPDATE imap_uid_recovery_requests SET state='running',started_at_utc_ms=?2 WHERE execution_id=?1 AND state='admitted'",params![id.as_slice(),at]))?!=1{return Err(ErrorCode::InternalError)}
        read_row(tx,id)?.ok_or(ErrorCode::InternalError)?;
        Ok(UidRecoveryStart::Ready(Box::new(UidRecoveryJob{execution_id:id,request:row.request,fetch_request:row.fetch,endpoint:current.endpoint})))
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
pub(super) fn succeed(
    store: &Store,
    id: [u8; 32],
    page: &ImapUidRecoveryPage,
) -> Result<UidRecoveryReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate,|tx|{
        let row=read_row(tx,id)?.ok_or(ErrorCode::NotFound)?;if row.state!="running"{return Err(ErrorCode::Conflict)}
        let current=match current_binding(tx,&row){Ok(v)=>v,Err(ErrorCode::InternalError)=>return Err(ErrorCode::InternalError),Err(e)=>return fail_row(tx,&row,e)};
        if page.request()!=&row.fetch{return fail_row(tx,&row,ErrorCode::InvalidInput)}
        let mutation=MailMutation::new(row.request.operation_id().into(),row.request.idempotency_key().into(),row.request.expected_cursor_version(),crate::imap_sync::imap_uid_page_payload_hash(row.av,row.request.expected_catalog_generation(),&current.endpoint,page.page(),row.request.expected_cursor_version())?).map_err(|_|ErrorCode::InternalError)?;
        let evidence=RecoveryTransition{request:row.request.clone(),fetch:row.fetch.clone(),av:row.av,epoch:row.epoch,endpoint_hash:row.endpoint_hash,execution_id:id};
        let applied=match apply_recovery_tx(tx,row.av,row.request.expected_catalog_generation(),&current.endpoint,page.page(),&mutation,&evidence){
            Ok(value)=>value,
            // These checks precede all writes in apply_tx. Any other failure
            // rolls back the outer transaction and leaves the job unresolved.
            Err(e @ (ErrorCode::InvalidInput|ErrorCode::Conflict|ErrorCode::NotFound|ErrorCode::QuotaExceeded))=>return fail_row(tx,&row,e),
            Err(e)=>return Err(e),
        };
        let op=read_operation(tx,row.request.operation_id())?.ok_or(ErrorCode::InternalError)?;
        let at=now()?;if at<row.started.ok_or(ErrorCode::InternalError)? || at<op.at{return Err(ErrorCode::InternalError)}
        if db(tx.execute("UPDATE imap_uid_recovery_requests SET state='succeeded',completed_at_utc_ms=?2,result_cursor_version=?3,result_covered_through_uid=?4,result_inserted_count=?5,page_operation_id=operation_id,result_page_hash=?6,result_payload_hash=?7,result_epoch_generation=?8,result_uid_validity=?9 WHERE execution_id=?1 AND state='running'",params![id.as_slice(),at,applied.cursor_version() as i64,i64::from(applied.covered_through_uid()),i64::from(applied.inserted_count()),hash(op.json.as_bytes())?.as_slice(),op.payload_hash.as_slice(),(row.epoch+1) as i64,i64::from(page.page().uid_validity())]))?!=1{return Err(ErrorCode::InternalError)}
        insert_audit_terminal_tx(tx,id,AuditTerminal::succeeded(None).map_err(|_|ErrorCode::InternalError)?,at)?;
        Ok(read_row(tx,id)?.ok_or(ErrorCode::InternalError)?.receipt)
    })
}
pub(super) fn fail(
    store: &Store,
    id: [u8; 32],
    error: ErrorCode,
) -> Result<UidRecoveryReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate, |tx| {
        let row = read_row(tx, id)?.ok_or(ErrorCode::NotFound)?;
        fail_row(tx, &row, error)
    })
}
pub(super) fn receipt(
    store: &Store,
    owner: [u8; 16],
    request: &UidPageStatusRequest,
) -> Result<UidRecoveryReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Deferred, |tx| {
        let row = read_row(tx, *request.execution_id())?.ok_or(ErrorCode::NotFound)?;
        if row.audit.registration_id() != &owner || row.request.account_id() != request.account_id()
        {
            return Err(ErrorCode::NotFound);
        }
        Ok(row.receipt)
    })
}

pub(crate) fn execution_reserved(c: &Connection, id: [u8; 32]) -> Result<bool, ErrorCode> {
    db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM imap_uid_recovery_requests WHERE execution_id=?1)",
        [id.as_slice()],
        |r| r.get(0),
    ))
}
/// Only the validated running recovery completion below can construct this evidence.
pub(crate) struct RecoveryTransition {
    request: UidRecoveryRequest,
    fetch: ImapUidRecoveryRequest,
    av: u64,
    epoch: u64,
    endpoint_hash: [u8; 32],
    execution_id: [u8; 32],
}
impl RecoveryTransition {
    pub(crate) fn validate(
        &self,
        c: &Connection,
        old: &crate::imap_sync::storage::Cursor,
        binding_versions: (u64, u64),
        endpoint: &ImapEndpointConfig,
        page: &hantong_domain::imap_sync::ImapUidPage,
        mutation: &MailMutation,
    ) -> Result<(), ErrorCode> {
        ImapUidRecoveryPage::new(self.fetch.clone(), page.clone())
            .map_err(|_| ErrorCode::InvalidInput)?;
        if self.av != binding_versions.0
            || self.request.expected_catalog_generation() != binding_versions.1
            || self.request.expected_cursor_version() != old.value.version()
            || mutation.expected_version() != old.value.version()
            || mutation.operation_id() != self.request.operation_id()
            || mutation.idempotency_key() != self.request.idempotency_key()
            || old.value.uid_validity() != self.fetch.previous_uid_validity()
            || old.endpoint_hash != self.endpoint_hash
            || endpoint.settings_version() != self.fetch.endpoint_version()
        {
            return Err(ErrorCode::Conflict);
        }
        let epoch = crate::uid_epochs::read(
            c,
            self.request.account_id(),
            self.fetch.mailbox(),
            self.fetch.previous_uid_validity(),
        )?
        .ok_or(ErrorCode::InternalError)?;
        if epoch.last.is_some() || epoch.generation != self.epoch {
            return Err(ErrorCode::Conflict);
        }
        if crate::uid_epochs::raw(
            c,
            self.request.account_id(),
            self.fetch.mailbox(),
            page.uid_validity(),
        )?
        .is_some()
        {
            return Err(ErrorCode::Conflict);
        }
        if self.epoch >= i64::MAX as u64 {
            return Err(ErrorCode::InternalError);
        }
        crate::uid_epochs::check_quota(c)
    }
    pub(crate) fn persist_boundary(
        &self,
        c: &Connection,
        op: &crate::imap_sync::storage::Operation,
    ) -> Result<(), ErrorCode> {
        let account = self.request.account_id();
        let name = self.fetch.mailbox();
        let cv = self.request.expected_cursor_version();
        let last:String=db(c.query_row("SELECT operation_id FROM imap_uid_page_operations WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND result_cursor_version=?4",params![account,crate::uid_epochs::key(name),crate::uid_epochs::encoding(name),cv as i64],|r|Ok(text_column(r,0,128)?.to_owned())))?;
        if db(c.execute("UPDATE imap_uid_epochs SET last_cursor_version=?5,retired_last_operation_id=?6,retired_at_utc_ms=?7 WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4 AND last_cursor_version IS NULL AND epoch_generation=?8",params![account,crate::uid_epochs::key(name),crate::uid_epochs::encoding(name),i64::from(self.fetch.previous_uid_validity()),cv as i64,last,op.at,self.epoch as i64]))?!=1{return Err(ErrorCode::InternalError)}
        if db(c.execute("INSERT INTO imap_uid_epochs(account_id,mailbox_key,mailbox_encoding,uid_validity,epoch_generation,first_cursor_version,first_operation_id,endpoint_version,endpoint_hash,created_at_utc_ms,recovery_execution_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![account,crate::uid_epochs::key(name),crate::uid_epochs::encoding(name),i64::from(op.page.uid_validity()),(self.epoch+1) as i64,(cv+1) as i64,op.id,self.fetch.endpoint_version() as i64,self.endpoint_hash.as_slice(),op.at,self.execution_id.as_slice()]))?!=1{return Err(ErrorCode::InternalError)}
        Ok(())
    }
}
pub(crate) fn validate_history(c: &Connection) -> rusqlite::Result<()> {
    let count: i64 = c.query_row("SELECT count(*) FROM imap_uid_recovery_requests", [], |r| {
        r.get(0)
    })?;
    if count > 100_000 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}
