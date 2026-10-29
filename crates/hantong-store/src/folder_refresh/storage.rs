use super::*;
use crate::{
    audit::{insert_audit_terminal_tx, read_audit},
    imap_catalog::{apply_new_catalog, endpoint_hash, load_catalog},
    mail_connection::{MailEndpointKind, MailMutation, check_account, read_endpoint},
    registrations::{blob_column, read_registration, text_column},
};
use hantong_contracts::audit::{AuditRecord, AuditTerminal, AuditTool};
use hantong_domain::mail_connection::MailConnectionConfig;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

fn db<T>(result: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    result.map_err(|_| ErrorCode::InternalError)
}
fn now() -> Result<i64, ErrorCode> {
    db(crate::utc_millis())
}

struct Row {
    request: FolderRefreshRequest,
    audit: AuditRecord,
    account_version: u64,
    endpoint_version: u64,
    endpoint_hash: [u8; 32],
    state: String,
    admitted: i64,
    started: Option<i64>,
    completed: Option<i64>,
    receipt: FolderRefreshReceipt,
}

fn read_row(c: &Connection, id: [u8; 32]) -> Result<Option<Row>, ErrorCode> {
    let raw=db(c.query_row("SELECT account_id,operation_id,idempotency_key,intent_hash,expected_generation,bound_account_version,bound_endpoint_version,bound_endpoint_hash,state,admitted_at_utc_ms,started_at_utc_ms,completed_at_utc_ms,result_generation,error_code,catalog_operation_id,result_snapshot_hash,result_payload_hash FROM folder_refresh_requests WHERE execution_id=?1",[id.as_slice()],|r| {
        Ok((text_column(r,0,128)?.to_owned(),text_column(r,1,128)?.to_owned(),text_column(r,2,128)?.to_owned(),blob_column::<32>(r,3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?,r.get::<_,i64>(6)?,blob_column::<32>(r,7)?,text_column(r,8,10)?.to_owned(),r.get::<_,i64>(9)?,r.get::<_,Option<i64>>(10)?,r.get::<_,Option<i64>>(11)?,r.get::<_,Option<i64>>(12)?,if matches!(r.get_ref(13)?,rusqlite::types::ValueRef::Null){None}else{Some(text_column(r,13,32)?.to_owned())},if matches!(r.get_ref(14)?,rusqlite::types::ValueRef::Null){None}else{Some(text_column(r,14,128)?.to_owned())},if matches!(r.get_ref(15)?,rusqlite::types::ValueRef::Null){None}else{Some(blob_column::<32>(r,15)?)},if matches!(r.get_ref(16)?,rusqlite::types::ValueRef::Null){None}else{Some(blob_column::<32>(r,16)?)}))
    }).optional())?;
    let Some((
        account,
        operation,
        key,
        hash,
        expected,
        av,
        ev,
        eh,
        state,
        admitted,
        started,
        completed,
        generation,
        error,
        catalog_op,
        result_snapshot_hash,
        result_payload_hash,
    )) = raw
    else {
        return Ok(None);
    };
    if id == [0; 32]
        || expected < 0
        || expected == i64::MAX
        || av <= 0
        || ev <= 0
        || admitted <= 0
        || started.is_some_and(|v| v < admitted)
        || completed.is_some_and(|v| v < started.unwrap_or(admitted))
    {
        return Err(ErrorCode::InternalError);
    }
    let provider = db(c
        .query_row(
            "SELECT provider FROM accounts WHERE account_id=?1",
            [&account],
            |r| Ok(text_column(r, 0, 16)?.to_owned()),
        )
        .optional())?;
    if provider.as_deref() != Some("imap") {
        return Err(ErrorCode::InternalError);
    }
    let request = FolderRefreshRequest::new(
        &account,
        &operation,
        &key,
        expected as u64,
        &hantong_contracts::wire_hex::encode(&hash),
    )
    .map_err(|_| ErrorCode::InternalError)?;
    if db_hash(&request.intent_bytes())? != hash {
        return Err(ErrorCode::InternalError);
    }
    let audit = db(read_audit(c, &id))?.ok_or(ErrorCode::InternalError)?;
    if audit.tool() != AuditTool::FoldersRefresh
        || audit.account_ids() != [account.clone()]
        || admitted < audit.started_at_utc_ms()
    {
        return Err(ErrorCode::InternalError);
    }
    let error = error
        .map(|v| {
            ErrorCode::ALL
                .into_iter()
                .find(|e| e.as_str() == v)
                .ok_or(ErrorCode::InternalError)
        })
        .transpose()?;
    let wire_state = match state.as_str() {
        "admitted"
            if started.is_none()
                && completed.is_none()
                && generation.is_none()
                && error.is_none()
                && catalog_op.is_none()
                && result_snapshot_hash.is_none()
                && result_payload_hash.is_none() =>
        {
            FolderRefreshState::Unresolved
        }
        "running"
            if started.is_some()
                && completed.is_none()
                && generation.is_none()
                && error.is_none()
                && catalog_op.is_none()
                && result_snapshot_hash.is_none()
                && result_payload_hash.is_none() =>
        {
            FolderRefreshState::Unresolved
        }
        "succeeded"
            if started.is_some()
                && completed.is_some()
                && generation == Some(expected + 1)
                && error.is_none()
                && catalog_op.as_deref() == Some(&operation)
                && result_snapshot_hash.is_some()
                && result_payload_hash.is_some() =>
        {
            FolderRefreshState::Succeeded
        }
        "failed"
            if completed.is_some()
                && generation.is_none()
                && error.is_some()
                && catalog_op.is_none()
                && result_snapshot_hash.is_none()
                && result_payload_hash.is_none() =>
        {
            FolderRefreshState::Failed
        }
        _ => return Err(ErrorCode::InternalError),
    };
    match (completed, audit.completion()) {
        (None, None) => {}
        (Some(at), Some(terminal))
            if terminal.completed_at_utc_ms() == at
                && terminal.terminal().error() == error
                && terminal.terminal().is_success()
                    == (wire_state == FolderRefreshState::Succeeded)
                && terminal.terminal().result_count().is_none() => {}
        _ => return Err(ErrorCode::InternalError),
    }
    if wire_state == FolderRefreshState::Succeeded {
        let op =
            crate::imap_catalog::read_operation(c, &operation)?.ok_or(ErrorCode::InternalError)?;
        if op.0 != operation
            || op.1 != account
            || op.2 != key
            || op.3 != av
            || op.4 != ev
            || op.5 != eh
            || op.6 != result_snapshot_hash.unwrap()
            || op.7 != expected
            || Some(op.8) != generation
            || op.9 != result_payload_hash.unwrap()
            || op.10 < started.unwrap()
            || op.10 > completed.unwrap()
        {
            return Err(ErrorCode::InternalError);
        }
        let last = db(c
            .query_row(
                "SELECT last_operation_id FROM imap_mailbox_catalogs WHERE account_id=?1",
                [&account],
                |r| Ok(text_column(r, 0, 128)?.to_owned()),
            )
            .optional())?;
        if last.as_deref() == Some(&operation) {
            load_catalog(c, &account)?.ok_or(ErrorCode::InternalError)?;
        }
    } else if catalog_operation_exists(c, &account, &operation, &key)? {
        return Err(ErrorCode::InternalError);
    }
    let receipt = FolderRefreshReceipt::new(
        &account,
        id,
        wire_state,
        generation.map(|v| v as u64),
        error,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    Ok(Some(Row {
        request,
        audit,
        account_version: av as u64,
        endpoint_version: ev as u64,
        endpoint_hash: eh,
        state,
        admitted,
        started,
        completed,
        receipt,
    }))
}
fn db_hash(bytes: &[u8]) -> Result<[u8; 32], ErrorCode> {
    hantong_platform::sha256(bytes).map_err(|_| ErrorCode::InternalError)
}

fn current_grants(tx: &Connection, audit: &AuditRecord) -> Result<(), ErrorCode> {
    let registration = db(read_registration(tx, audit.registration_id()))?
        .ok_or(ErrorCode::ClientNotRegistered)?;
    if registration.state != crate::RegistrationState::Active {
        return Err(ErrorCode::ClientSuspended);
    }
    if registration.profile_epoch != audit.profile_epoch() {
        return Err(ErrorCode::AuthRequired);
    }
    if !registration
        .scope
        .tools()
        .iter()
        .any(|t| t == "mail.folders.refresh")
        || !audit
            .account_ids()
            .iter()
            .all(|a| registration.scope.accounts().contains(a))
    {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(())
}
fn binding(
    tx: &Transaction<'_>,
    request: &FolderRefreshRequest,
) -> Result<(u64, ImapEndpointConfig, [u8; 32]), ErrorCode> {
    check_account(tx, request.account_id())?;
    let (av, state) = db(tx.query_row(
        "SELECT settings_version,status FROM accounts WHERE account_id=?1",
        [request.account_id()],
        |r| Ok((r.get::<_, i64>(0)?, text_column(r, 1, 32)?.to_owned())),
    ))?;
    if av <= 0 {
        return Err(ErrorCode::InternalError);
    }
    let status = hantong_domain::accounts::AccountStatus::parse(&state)
        .map_err(|_| ErrorCode::InternalError)?;
    if status != hantong_domain::accounts::AccountStatus::Ready {
        return Err(ErrorCode::Conflict);
    }
    let endpoint = read_endpoint(tx, request.account_id(), MailEndpointKind::Imap)?
        .ok_or(ErrorCode::Conflict)?;
    let json = serde_json::json!({"schema_version":1,"account_id":request.account_id(),"imap":endpoint.json});
    let endpoint = MailConnectionConfig::from_json(
        &serde_json::to_vec(&json).map_err(|_| ErrorCode::InternalError)?,
    )
    .map_err(|_| ErrorCode::InternalError)?
    .imap()
    .clone();
    let generation = load_catalog(tx, request.account_id())?.map_or(0, |c| c.generation());
    if generation != request.expected_generation() {
        return Err(ErrorCode::Conflict);
    }
    let hash = endpoint_hash(request.account_id(), &endpoint)?;
    Ok((av as u64, endpoint, hash))
}
fn current_binding(tx: &Transaction<'_>, row: &Row) -> Result<ImapEndpointConfig, ErrorCode> {
    current_grants(tx, &row.audit)?;
    let (av, endpoint, hash) = binding(tx, &row.request)?;
    if av != row.account_version
        || endpoint.settings_version() != row.endpoint_version
        || hash != row.endpoint_hash
    {
        return Err(ErrorCode::Conflict);
    }
    Ok(endpoint)
}
fn reject(
    tx: &Transaction<'_>,
    id: [u8; 32],
    error: ErrorCode,
) -> Result<FolderRefreshAdmission, ErrorCode> {
    if error == ErrorCode::InternalError {
        return Err(error);
    }
    insert_audit_terminal_tx(tx, id, AuditTerminal::failed(error), now()?)?;
    Ok(FolderRefreshAdmission::Rejected(error))
}

pub(crate) fn operation_reserved(
    c: &Connection,
    account: &str,
    operation: &str,
    key: &str,
) -> Result<bool, ErrorCode> {
    db(c.query_row("SELECT EXISTS(SELECT 1 FROM folder_refresh_requests WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3))",params![operation,account,key],|r|r.get(0)))
}
fn catalog_operation_exists(
    c: &Connection,
    account: &str,
    operation: &str,
    key: &str,
) -> Result<bool, ErrorCode> {
    db(c.query_row("SELECT EXISTS(SELECT 1 FROM imap_mailbox_catalog_operations WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3))",params![operation,account,key],|r|r.get(0)))
}
pub(super) fn admit(
    store: &Store,
    id: [u8; 32],
    request: &FolderRefreshRequest,
    queue: FolderQueueAdmission,
) -> Result<FolderRefreshAdmission, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate,|tx| {
        let audit=db(read_audit(tx,&id))?.ok_or(ErrorCode::NotFound)?;
        if audit.completion().is_some() {return Err(ErrorCode::Conflict)}
        if id==[0;32] || audit.tool()!=AuditTool::FoldersRefresh || audit.account_ids()!=[request.account_id()] {return Err(ErrorCode::InvalidInput)}
        if let Err(e)=current_grants(tx,&audit) {return reject(tx,id,e)}
        if hantong_contracts::wire_hex::encode(&db_hash(&request.intent_bytes())?)!=request.payload_hash() {return reject(tx,id,ErrorCode::InvalidInput)}
        let mut stmt=db(tx.prepare("SELECT execution_id FROM folder_refresh_requests WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3) LIMIT 2"))?;
        let ids=db(db(stmt.query_map(params![request.operation_id(),request.account_id(),request.idempotency_key()],|r|blob_column::<32>(r,0)))?.collect::<rusqlite::Result<Vec<_>>>())?;
        drop(stmt);
        if !ids.is_empty() {
            if ids.len()!=1 {return reject(tx,id,ErrorCode::Conflict)}
            let row=read_row(tx,ids[0])?.ok_or(ErrorCode::InternalError)?;
            if row.request!=*request || row.audit.registration_id()!=audit.registration_id() {return reject(tx,id,ErrorCode::Conflict)}
            insert_audit_terminal_tx(tx,id,AuditTerminal::succeeded(None).map_err(|_|ErrorCode::InternalError)?,now()?)?;
            return Ok(FolderRefreshAdmission::Existing(row.receipt))
        }
        if catalog_operation_exists(tx,request.account_id(),request.operation_id(),request.idempotency_key())? {return reject(tx,id,ErrorCode::Conflict)}
        let (av,endpoint,hash)=match binding(tx,request) {Ok(v)=>v,Err(e)=>return reject(tx,id,e)};
        let count:i64=db(tx.query_row("SELECT count(*) FROM folder_refresh_requests",[],|r|r.get(0)))?;
        if count>=100_000 {return reject(tx,id,ErrorCode::QuotaExceeded)}
        let at=now()?;
        if at<audit.started_at_utc_ms() {return Err(ErrorCode::InternalError)}
        let error=match queue {FolderQueueAdmission::Reserved=>None,FolderQueueAdmission::Full=>Some(ErrorCode::RateLimited),FolderQueueAdmission::Closed=>Some(ErrorCode::CoreUnavailable)};
        let state=if error.is_some(){"failed"}else{"admitted"};
        db(tx.execute("INSERT INTO folder_refresh_requests(execution_id,operation_id,account_id,idempotency_key,intent_hash,expected_generation,bound_account_version,bound_endpoint_version,bound_endpoint_hash,state,admitted_at_utc_ms,completed_at_utc_ms,error_code) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",params![id.as_slice(),request.operation_id(),request.account_id(),request.idempotency_key(),db_hash(&request.intent_bytes())?.as_slice(),request.expected_generation() as i64,av as i64,endpoint.settings_version() as i64,hash.as_slice(),state,at,error.map(|_|at),error.map(ErrorCode::as_str)]))?;
        if let Some(error)=error {insert_audit_terminal_tx(tx,id,AuditTerminal::failed(error),at)?;}
        let receipt=read_row(tx,id)?.ok_or(ErrorCode::InternalError)?.receipt;
        Ok(if error.is_some(){FolderRefreshAdmission::Failed(receipt)}else{FolderRefreshAdmission::Admitted(receipt)})
    })
}
fn fail_row(
    tx: &Transaction<'_>,
    row: &Row,
    error: ErrorCode,
) -> Result<FolderRefreshReceipt, ErrorCode> {
    if row.completed.is_some() {
        return Err(ErrorCode::Conflict);
    }
    let at = now()?;
    if at < row.started.unwrap_or(row.admitted) {
        return Err(ErrorCode::InternalError);
    }
    let id = *row.audit.execution_id();
    if db(tx.execute("UPDATE folder_refresh_requests SET state='failed',completed_at_utc_ms=?2,error_code=?3 WHERE execution_id=?1 AND state IN ('admitted','running')",params![id.as_slice(),at,error.as_str()]))?!=1 {return Err(ErrorCode::Conflict)}
    insert_audit_terminal_tx(tx, id, AuditTerminal::failed(error), at)?;
    Ok(read_row(tx, id)?.ok_or(ErrorCode::InternalError)?.receipt)
}
pub(super) fn start(store: &Store, id: [u8; 32]) -> Result<FolderRefreshStart, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate,|tx| {
        let row=read_row(tx,id)?.ok_or(ErrorCode::NotFound)?;
        if row.completed.is_some() {return Ok(FolderRefreshStart::Finished(row.receipt))}
        if row.state!="admitted" {return Err(ErrorCode::Conflict)}
        let endpoint=match current_binding(tx,&row) {Ok(v)=>v,Err(ErrorCode::InternalError)=>return Err(ErrorCode::InternalError),Err(e)=>return fail_row(tx,&row,e).map(FolderRefreshStart::Finished)};
        let at=now()?;if at<row.admitted {return Err(ErrorCode::InternalError)}
        if db(tx.execute("UPDATE folder_refresh_requests SET state='running',started_at_utc_ms=?2 WHERE execution_id=?1 AND state='admitted'",params![id.as_slice(),at]))?!=1 {return Err(ErrorCode::Conflict)}
        read_row(tx,id)?.ok_or(ErrorCode::InternalError)?;
        Ok(FolderRefreshStart::Ready(FolderRefreshJob{execution_id:id,request:row.request,endpoint}))
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
    snapshot: &ImapMailboxSnapshot,
) -> Result<FolderRefreshReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate,|tx| {
        let row=read_row(tx,id)?.ok_or(ErrorCode::NotFound)?;
        if row.state!="running" {return Err(ErrorCode::Conflict)}
        let endpoint=match current_binding(tx,&row){Ok(v)=>v,Err(ErrorCode::InternalError)=>return Err(ErrorCode::InternalError),Err(e)=>return fail_row(tx,&row,e)};
        if snapshot.account_id()!=row.request.account_id() || snapshot.settings_version()!=row.endpoint_version {return fail_row(tx,&row,ErrorCode::InvalidInput)}
        let mutation=MailMutation::new(row.request.operation_id().into(),row.request.idempotency_key().into(),row.request.expected_generation(),crate::imap_catalog::imap_catalog_payload_hash(row.account_version,&endpoint,snapshot,row.request.expected_generation())?).map_err(|_|ErrorCode::InternalError)?;
        let count:i64=db(tx.query_row("SELECT count(*) FROM imap_mailbox_catalog_operations",[],|r|r.get(0)))?;
        if count>=100_000 {return fail_row(tx,&row,ErrorCode::QuotaExceeded)}
        let generation=apply_new_catalog(tx,row.account_version,&endpoint,snapshot,&mutation)?;
        let op=crate::imap_catalog::read_operation(tx,row.request.operation_id())?.ok_or(ErrorCode::InternalError)?;
        let at=now()?;
        if at<row.started.ok_or(ErrorCode::InternalError)? {return Err(ErrorCode::InternalError)}
        if db(tx.execute("UPDATE folder_refresh_requests SET state='succeeded',completed_at_utc_ms=?2,result_generation=?3,catalog_operation_id=operation_id,result_snapshot_hash=?4,result_payload_hash=?5 WHERE execution_id=?1 AND state='running'",params![id.as_slice(),at,generation as i64,op.6,op.9]))?!=1 {return Err(ErrorCode::Conflict)}
        insert_audit_terminal_tx(tx,id,AuditTerminal::succeeded(None).map_err(|_|ErrorCode::InternalError)?,at)?;
        Ok(read_row(tx,id)?.ok_or(ErrorCode::InternalError)?.receipt)
    })
}
pub(super) fn fail(
    store: &Store,
    id: [u8; 32],
    error: ErrorCode,
) -> Result<FolderRefreshReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate, |tx| {
        let row = read_row(tx, id)?.ok_or(ErrorCode::NotFound)?;
        fail_row(tx, &row, error)
    })
}
pub(super) fn receipt(
    store: &Store,
    owner: [u8; 16],
    request: &FolderRefreshStatusRequest,
) -> Result<FolderRefreshReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Deferred, |tx| {
        let row = read_row(tx, *request.execution_id())?.ok_or(ErrorCode::NotFound)?;
        if row.audit.registration_id() != &owner || row.request.account_id() != request.account_id()
        {
            return Err(ErrorCode::NotFound);
        }
        Ok(row.receipt)
    })
}
pub(crate) fn validate_history(c: &Connection) -> rusqlite::Result<()> {
    let mut statement = c.prepare(
        "SELECT execution_id FROM folder_refresh_requests ORDER BY execution_id LIMIT 100001",
    )?;
    let ids = statement
        .query_map([], |r| blob_column::<32>(r, 0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if ids.len() > 100_000 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    for id in ids {
        read_row(c, id)
            .map_err(|_| rusqlite::Error::InvalidQuery)?
            .ok_or(rusqlite::Error::InvalidQuery)?;
    }
    Ok(())
}
