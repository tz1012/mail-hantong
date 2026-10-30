use super::*;
use crate::{
    audit::{insert_audit_terminal_tx, read_audit},
    imap_catalog::{endpoint_hash, load_catalog},
    imap_sync::storage::{apply_tx, current_endpoint, read_cursor, read_operation},
    mail_connection::{MailMutation, check_account},
    registrations::{blob_column, read_registration, text_column},
};
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
    validity: Option<i64>,
    after: i64,
    state: String,
    admitted: i64,
    started: Option<i64>,
    completed: Option<i64>,
    result_version: Option<i64>,
    covered: Option<i64>,
    inserted: Option<i64>,
    error: Option<String>,
    page_operation: Option<String>,
    page_hash: Option<[u8; 32]>,
    payload_hash: Option<[u8; 32]>,
}
struct Row {
    request: UidPageRequest,
    audit: AuditRecord,
    fetch: ImapUidPageRequest,
    av: u64,
    endpoint_hash: [u8; 32],
    state: String,
    admitted: i64,
    started: Option<i64>,
    completed: Option<i64>,
    receipt: UidPageReceipt,
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
fn read_row(c: &Connection, id: [u8; 32]) -> Result<Option<Row>, ErrorCode> {
    let raw=db(c.query_row("SELECT account_id,operation_id,idempotency_key,mailbox_key,mailbox_encoding,intent_hash,expected_catalog_generation,expected_cursor_version,bound_account_version,bound_endpoint_version,bound_endpoint_hash,expected_uid_validity,after_uid,state,admitted_at_utc_ms,started_at_utc_ms,completed_at_utc_ms,result_cursor_version,result_covered_through_uid,result_inserted_count,error_code,page_operation_id,result_page_hash,result_payload_hash FROM imap_uid_page_requests WHERE execution_id=?1",[id.as_slice()],|r|Ok(RawRow{
        account:text_column(r,0,128)?.to_owned(),operation:text_column(r,1,128)?.to_owned(),key:text_column(r,2,128)?.to_owned(),mailbox_key:text_column(r,3,2048)?.to_owned(),encoding:text_column(r,4,13)?.to_owned(),
        intent:blob_column::<32>(r,5)?,cg:r.get(6)?,cv:r.get(7)?,av:r.get(8)?,ev:r.get(9)?,endpoint_hash:blob_column::<32>(r,10)?,validity:r.get(11)?,after:r.get(12)?,
        state:text_column(r,13,10)?.to_owned(),admitted:r.get(14)?,started:r.get(15)?,completed:r.get(16)?,result_version:r.get(17)?,covered:r.get(18)?,inserted:r.get(19)?,error:optional_text(r,20,32)?,page_operation:optional_text(r,21,128)?,page_hash:optional_hash(r,22)?,payload_hash:optional_hash(r,23)?,
    })).optional())?;
    let Some(raw) = raw else { return Ok(None) };
    if id == [0; 32]
        || raw.cg <= 0
        || !(0..i64::MAX).contains(&raw.cv)
        || raw.av <= 0
        || raw.ev <= 0
        || !(0..i64::from(u32::MAX)).contains(&raw.after)
        || raw
            .validity
            .is_some_and(|v| !(1..=i64::from(u32::MAX)).contains(&v))
        || (raw.cv == 0 && (raw.validity.is_some() || raw.after != 0))
        || (raw.cv > 0 && raw.validity.is_none())
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
    let request = UidPageRequest::new(
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
    let fetch = ImapUidPageRequest::new(
        &raw.account,
        raw.ev as u64,
        name,
        raw.validity.map(|v| v as u32),
        raw.after as u32,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    let audit = db(read_audit(c, &id))?.ok_or(ErrorCode::InternalError)?;
    if audit.tool() != AuditTool::SyncPage
        || audit.account_ids() != [raw.account.clone()]
        || raw.admitted < audit.started_at_utc_ms()
    {
        return Err(ErrorCode::InternalError);
    }
    let provider: Option<String> = db(c
        .query_row(
            "SELECT provider FROM accounts WHERE account_id=?1",
            [&raw.account],
            |r| Ok(text_column(r, 0, 16)?.to_owned()),
        )
        .optional())?;
    if provider.as_deref() != Some("imap") {
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
                && raw.covered.is_some_and(|v| {
                    v >= raw.after && v < i64::from(u32::MAX) && v <= raw.after + 64
                })
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
        (Some(at), Some(terminal))
            if terminal.completed_at_utc_ms() == at
                && terminal.terminal().error() == error
                && terminal.terminal().is_success() == (state == UidPageState::Succeeded)
                && terminal.terminal().result_count().is_none() => {}
        _ => return Err(ErrorCode::InternalError),
    }
    if state == UidPageState::Succeeded {
        let op = read_operation(c, &raw.operation)?.ok_or(ErrorCode::InternalError)?;
        crate::uid_epochs::validate_operation(c, &op)?;
        if op.id != raw.operation
            || op.key != raw.key
            || op.page.request() != &fetch
            || op.av != raw.av as u64
            || op.cg != raw.cg as u64
            || op.expected != raw.cv as u64
            || op.endpoint_hash != raw.endpoint_hash
            || Some(op.receipt.cursor_version() as i64) != raw.result_version
            || Some(i64::from(op.receipt.covered_through_uid())) != raw.covered
            || Some(i64::from(op.receipt.inserted_count())) != raw.inserted
            || Some(hash(op.json.as_bytes())?) != raw.page_hash
            || Some(op.payload_hash) != raw.payload_hash
            || op.at < raw.started.unwrap()
            || op.at > raw.completed.unwrap()
        {
            return Err(ErrorCode::InternalError);
        }
        let cursor =
            read_cursor(c, &raw.account, fetch.mailbox())?.ok_or(ErrorCode::InternalError)?;
        if cursor.value.version() < op.receipt.cursor_version() {
            return Err(ErrorCode::InternalError);
        }
    } else if page_operation_exists(c, &raw.account, &raw.operation, &raw.key)? {
        return Err(ErrorCode::InternalError);
    }
    let receipt = UidPageReceipt::new(
        &raw.account,
        id,
        state,
        raw.result_version.map(|v| v as u64),
        raw.covered.map(|v| v as u32),
        raw.inserted.map(|v| v as u32),
        error,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    Ok(Some(Row {
        request,
        audit,
        fetch,
        av: raw.av as u64,
        endpoint_hash: raw.endpoint_hash,
        state: raw.state,
        admitted: raw.admitted,
        started: raw.started,
        completed: raw.completed,
        receipt,
    }))
}
pub(crate) fn current_grants(c: &Connection, audit: &AuditRecord) -> Result<(), ErrorCode> {
    let registration =
        db(read_registration(c, audit.registration_id()))?.ok_or(ErrorCode::ClientNotRegistered)?;
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
        .any(|t| t == "mail.sync.page")
        || !audit
            .account_ids()
            .iter()
            .all(|a| registration.scope.accounts().contains(a))
    {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(())
}
pub(crate) struct Binding {
    pub(crate) av: u64,
    pub(crate) endpoint: ImapEndpointConfig,
    pub(crate) endpoint_hash: [u8; 32],
    pub(crate) fetch: ImapUidPageRequest,
}
pub(crate) fn binding(c: &Transaction<'_>, request: &UidPageRequest) -> Result<Binding, ErrorCode> {
    let name = mailbox(request.mailbox())?;
    check_account(c, request.account_id())?;
    let (av, state): (i64, String) = db(c.query_row(
        "SELECT settings_version,status FROM accounts WHERE account_id=?1",
        [request.account_id()],
        |r| Ok((r.get(0)?, text_column(r, 1, 32)?.to_owned())),
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
    if endpoint.settings_version() != catalog.endpoint_version() {
        return Err(ErrorCode::Conflict);
    }
    let endpoint_hash = endpoint_hash(request.account_id(), &endpoint)?;
    let cursor = read_cursor(c, request.account_id(), &name)?;
    let (validity, after) = match cursor {
        None if request.expected_cursor_version() == 0 => (None, 0),
        Some(cursor)
            if cursor.value.version() == request.expected_cursor_version()
                && cursor.value.binding_current()
                && cursor.endpoint_hash == endpoint_hash =>
        {
            (
                Some(cursor.value.uid_validity()),
                cursor.value.covered_through_uid(),
            )
        }
        _ => return Err(ErrorCode::Conflict),
    };
    let fetch = ImapUidPageRequest::new(
        request.account_id(),
        endpoint.settings_version(),
        name,
        validity,
        after,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    Ok(Binding {
        av: av as u64,
        endpoint,
        endpoint_hash,
        fetch,
    })
}
fn current_binding(c: &Transaction<'_>, row: &Row) -> Result<Binding, ErrorCode> {
    current_grants(c, &row.audit)?;
    let current = binding(c, &row.request)?;
    if current.av != row.av
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
) -> Result<UidPageAdmission, ErrorCode> {
    if error == ErrorCode::InternalError {
        return Err(error);
    }
    insert_audit_terminal_tx(tx, id, AuditTerminal::failed(error), now()?)?;
    Ok(UidPageAdmission::Rejected(error))
}
pub(crate) fn execution_reserved(c: &Connection, id: [u8; 32]) -> Result<bool, ErrorCode> {
    db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM imap_uid_page_requests WHERE execution_id=?1)",
        [id.as_slice()],
        |r| r.get(0),
    ))
}
pub(crate) fn operation_reserved(
    c: &Connection,
    account: &str,
    operation: &str,
    key: &str,
) -> Result<bool, ErrorCode> {
    db(c.query_row("SELECT EXISTS(SELECT 1 FROM imap_uid_page_requests WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3))",params![operation,account,key],|r|r.get(0)))
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
    request: &UidPageRequest,
    queue: UidQueueAdmission,
) -> Result<UidPageAdmission, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate,|tx|{
        let audit=db(read_audit(tx,&id))?.ok_or(ErrorCode::NotFound)?;
        if audit.completion().is_some() || execution_reserved(tx,id)? || crate::uid_reconcile_jobs::execution_reserved(tx,id)? || crate::uid_recovery_jobs::execution_reserved(tx,id)? {return Err(ErrorCode::Conflict)}
        if id==[0;32] || audit.tool()!=AuditTool::SyncPage || audit.account_ids()!=[request.account_id()]{return Err(ErrorCode::InvalidInput)}
        if let Err(e)=current_grants(tx,&audit){return reject(tx,id,e)}
        if let Err(e)=mailbox(request.mailbox()){return reject(tx,id,e)}
        let intent=hash(&request.intent_bytes())?;
        if hantong_contracts::wire_hex::encode(&intent)!=request.intent_hash(){return reject(tx,id,ErrorCode::InvalidInput)}
        if crate::uid_recovery_jobs::operation_reserved(tx,request.account_id(),request.operation_id(),request.idempotency_key())? || crate::uid_reconcile_jobs::operation_reserved(tx,request.account_id(),request.operation_id(),request.idempotency_key())? {return reject(tx,id,ErrorCode::Conflict)}
        let ids={let mut s=db(tx.prepare("SELECT execution_id FROM imap_uid_page_requests WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3) LIMIT 2"))?;db(db(s.query_map(params![request.operation_id(),request.account_id(),request.idempotency_key()],|r|blob_column::<32>(r,0)))?.collect::<rusqlite::Result<Vec<_>>>())?};
        if !ids.is_empty(){
            if ids.len()!=1{return reject(tx,id,ErrorCode::Conflict)}
            if ids[0]==id{return Err(ErrorCode::Conflict)}
            let row=read_row(tx,ids[0])?.ok_or(ErrorCode::InternalError)?;
            if row.request!=*request || row.audit.registration_id()!=audit.registration_id(){return reject(tx,id,ErrorCode::Conflict)}
            insert_audit_terminal_tx(tx,id,AuditTerminal::succeeded(None).map_err(|_|ErrorCode::InternalError)?,now()?)?;
            return Ok(UidPageAdmission::Existing(row.receipt))
        }
        if page_operation_exists(tx,request.account_id(),request.operation_id(),request.idempotency_key())? {return reject(tx,id,ErrorCode::Conflict)}
        let current=match binding(tx,request){Ok(v)=>v,Err(e)=>return reject(tx,id,e)};
        let count:i64=db(tx.query_row("SELECT count(*) FROM imap_uid_page_requests",[],|r|r.get(0)))?;
        if count>100_000{return Err(ErrorCode::InternalError)}
        if count==100_000{return reject(tx,id,ErrorCode::QuotaExceeded)}
        let at=now()?;if at<audit.started_at_utc_ms(){return Err(ErrorCode::InternalError)}
        let error=match queue{UidQueueAdmission::Reserved=>None,UidQueueAdmission::Full=>Some(ErrorCode::RateLimited),UidQueueAdmission::Closed=>Some(ErrorCode::CoreUnavailable)};
        db(tx.execute("INSERT INTO imap_uid_page_requests(execution_id,operation_id,account_id,idempotency_key,mailbox_key,mailbox_encoding,intent_hash,expected_catalog_generation,expected_cursor_version,bound_account_version,bound_endpoint_version,bound_endpoint_hash,expected_uid_validity,after_uid,state,admitted_at_utc_ms,completed_at_utc_ms,error_code) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",params![id.as_slice(),request.operation_id(),request.account_id(),request.idempotency_key(),request.mailbox().wire_hex(),request.mailbox().encoding().as_str(),intent.as_slice(),request.expected_catalog_generation() as i64,request.expected_cursor_version() as i64,current.av as i64,current.endpoint.settings_version() as i64,current.endpoint_hash.as_slice(),current.fetch.expected_uid_validity().map(i64::from),i64::from(current.fetch.after_uid()),if error.is_some(){"failed"}else{"admitted"},at,error.map(|_|at),error.map(ErrorCode::as_str)]))?;
        if let Some(error)=error {insert_audit_terminal_tx(tx,id,AuditTerminal::failed(error),at)?;}
        let receipt=read_row(tx,id)?.ok_or(ErrorCode::InternalError)?.receipt;
        Ok(if error.is_some(){UidPageAdmission::Failed(receipt)}else{UidPageAdmission::Admitted(receipt)})
    })
}
fn fail_row(
    tx: &Transaction<'_>,
    row: &Row,
    error: ErrorCode,
) -> Result<UidPageReceipt, ErrorCode> {
    if row.completed.is_some() {
        return Err(ErrorCode::Conflict);
    }
    let at = now()?;
    if at < row.started.unwrap_or(row.admitted) {
        return Err(ErrorCode::InternalError);
    }
    let id = *row.audit.execution_id();
    if db(tx.execute("UPDATE imap_uid_page_requests SET state='failed',completed_at_utc_ms=?2,error_code=?3 WHERE execution_id=?1 AND state IN ('admitted','running')",params![id.as_slice(),at,error.as_str()]))?!=1{return Err(ErrorCode::Conflict)}
    insert_audit_terminal_tx(tx, id, AuditTerminal::failed(error), at)?;
    Ok(read_row(tx, id)?.ok_or(ErrorCode::InternalError)?.receipt)
}
pub(super) fn start(store: &Store, id: [u8; 32]) -> Result<UidPageStart, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate,|tx|{
        let row=read_row(tx,id)?.ok_or(ErrorCode::NotFound)?;
        if row.completed.is_some(){return Ok(UidPageStart::Finished(row.receipt))}
        if row.state!="admitted"{return Err(ErrorCode::Conflict)}
        let current=match current_binding(tx,&row){Ok(v)=>v,Err(ErrorCode::InternalError)=>return Err(ErrorCode::InternalError),Err(e)=>return fail_row(tx,&row,e).map(UidPageStart::Finished)};
        let at=now()?;if at<row.admitted{return Err(ErrorCode::InternalError)}
        if db(tx.execute("UPDATE imap_uid_page_requests SET state='running',started_at_utc_ms=?2 WHERE execution_id=?1 AND state='admitted'",params![id.as_slice(),at]))?!=1{return Err(ErrorCode::Conflict)}
        read_row(tx,id)?.ok_or(ErrorCode::InternalError)?;
        Ok(UidPageStart::Ready(Box::new(UidPageJob{execution_id:id,request:row.request,fetch_request:row.fetch,endpoint:current.endpoint})))
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
    page: &ImapUidPage,
) -> Result<UidPageReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate,|tx|{
        let row=read_row(tx,id)?.ok_or(ErrorCode::NotFound)?;if row.state!="running"{return Err(ErrorCode::Conflict)}
        let current=match current_binding(tx,&row){Ok(v)=>v,Err(ErrorCode::InternalError)=>return Err(ErrorCode::InternalError),Err(e)=>return fail_row(tx,&row,e)};
        if page.request()!=&row.fetch{return fail_row(tx,&row,ErrorCode::InvalidInput)}
        let mutation=MailMutation::new(row.request.operation_id().into(),row.request.idempotency_key().into(),row.request.expected_cursor_version(),crate::imap_sync::imap_uid_page_payload_hash(row.av,row.request.expected_catalog_generation(),&current.endpoint,page,row.request.expected_cursor_version())?).map_err(|_|ErrorCode::InternalError)?;
        let applied=match apply_tx(tx,row.av,row.request.expected_catalog_generation(),&current.endpoint,page,&mutation){
            Ok(value)=>value,
            // These checks precede all writes in apply_tx. Any other failure
            // rolls back the outer transaction and leaves the job unresolved.
            Err(e @ (ErrorCode::InvalidInput|ErrorCode::Conflict|ErrorCode::NotFound|ErrorCode::QuotaExceeded))=>return fail_row(tx,&row,e),
            Err(e)=>return Err(e),
        };
        let op=read_operation(tx,row.request.operation_id())?.ok_or(ErrorCode::InternalError)?;
        let at=now()?;if at<row.started.ok_or(ErrorCode::InternalError)? || at<op.at{return Err(ErrorCode::InternalError)}
        if db(tx.execute("UPDATE imap_uid_page_requests SET state='succeeded',completed_at_utc_ms=?2,result_cursor_version=?3,result_covered_through_uid=?4,result_inserted_count=?5,page_operation_id=operation_id,result_page_hash=?6,result_payload_hash=?7 WHERE execution_id=?1 AND state='running'",params![id.as_slice(),at,applied.cursor_version() as i64,i64::from(applied.covered_through_uid()),i64::from(applied.inserted_count()),hash(op.json.as_bytes())?.as_slice(),op.payload_hash.as_slice()]))?!=1{return Err(ErrorCode::Conflict)}
        insert_audit_terminal_tx(tx,id,AuditTerminal::succeeded(None).map_err(|_|ErrorCode::InternalError)?,at)?;
        Ok(read_row(tx,id)?.ok_or(ErrorCode::InternalError)?.receipt)
    })
}
pub(super) fn fail(
    store: &Store,
    id: [u8; 32],
    error: ErrorCode,
) -> Result<UidPageReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate, |tx| {
        let row = read_row(tx, id)?.ok_or(ErrorCode::NotFound)?;
        fail_row(tx, &row, error)
    })
}
pub(super) fn receipt(
    store: &Store,
    owner: [u8; 16],
    request: &UidPageStatusRequest,
) -> Result<UidPageReceipt, ErrorCode> {
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
    let count: i64 = c.query_row("SELECT count(*) FROM imap_uid_page_requests", [], |r| {
        r.get(0)
    })?;
    if count > 100_000 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}
