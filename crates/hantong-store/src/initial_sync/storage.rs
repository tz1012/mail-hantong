use super::*;
use crate::{
    audit::{insert_audit_terminal_tx, read_audit},
    imap_catalog::{endpoint_hash, load_catalog},
    imap_sync::storage::{current_endpoint, read_cursor},
    mail_connection::check_account,
    registrations::{blob_column, text_column},
    uid_page_jobs::storage::{current_grants, mailbox},
};
use hantong_contracts::{
    audit::{AuditRecord, AuditTerminal, AuditTool},
    uid_page::{UidMailboxEncoding, UidMailboxIdentity},
};
use hantong_domain::initial_sync_plan::{
    InitialSyncPlan, InitialSyncState as ModelState, InitialSyncStep,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
mod codec;
mod operations;
mod pages;
pub(super) use operations::{admit, check, fail, observe, page, plan_snapshot, receipt, start};
pub(crate) use pages::{PreviewSource, preview_source, read_preview_page};
fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    r.map_err(|_| ErrorCode::InternalError)
}
fn hash(v: &[u8]) -> Result<[u8; 32], ErrorCode> {
    hantong_platform::sha256_storage_payload(v).map_err(|_| ErrorCode::InternalError)
}
fn hex(v: &[u8]) -> String {
    hantong_contracts::wire_hex::encode(v)
}
fn now() -> Result<i64, ErrorCode> {
    db(crate::utc_millis())
}
fn u64_column(r: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<u64> {
    u64::try_from(r.get::<_, i64>(i)?).map_err(|_| rusqlite::Error::InvalidQuery)
}
fn optional_u64_column(r: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<Option<u64>> {
    r.get::<_, Option<i64>>(i)?
        .map(|value| u64::try_from(value).map_err(|_| rusqlite::Error::InvalidQuery))
        .transpose()
}
fn change(n: usize) -> Result<(), ErrorCode> {
    if n == 1 {
        Ok(())
    } else {
        Err(ErrorCode::InternalError)
    }
}
fn opt_hash<const N: usize>(r: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<Option<[u8; N]>> {
    if matches!(r.get_ref(i)?, rusqlite::types::ValueRef::Null) {
        Ok(None)
    } else {
        blob_column(r, i).map(Some)
    }
}
fn opt_text(r: &rusqlite::Row<'_>, i: usize, max: usize) -> rusqlite::Result<Option<String>> {
    if matches!(r.get_ref(i)?, rusqlite::types::ValueRef::Null) {
        Ok(None)
    } else {
        Ok(Some(text_column(r, i, max)?.into()))
    }
}
fn checked_json(s: &str, digest: &[u8; 32]) -> Result<(), ErrorCode> {
    if hash(s.as_bytes())? == *digest {
        Ok(())
    } else {
        Err(ErrorCode::InternalError)
    }
}
fn model(p: &InitialSyncProgress) -> Result<InitialSyncPlan, ErrorCode> {
    InitialSyncPlan::from_stored_parts(
        p.uid_validity(),
        p.uid_next(),
        p.recent_pages(),
        p.completed_page_count(),
        model_state(p.state()),
        p.model_version(),
    )
    .map_err(|_| ErrorCode::InternalError)
}
fn model_state(s: InitialSyncState) -> ModelState {
    match s {
        InitialSyncState::Ready => ModelState::Ready,
        InitialSyncState::Running => ModelState::Running,
        InitialSyncState::Pausing => ModelState::Pausing,
        InitialSyncState::PausedUser => ModelState::PausedUser,
        InitialSyncState::PausedBudget => ModelState::PausedBudget,
        InitialSyncState::Interrupted => ModelState::Interrupted,
        InitialSyncState::Complete => ModelState::Complete,
        InitialSyncState::Failed => ModelState::Failed,
        InitialSyncState::StaleBinding => ModelState::StaleBinding,
        InitialSyncState::NeedsRecovery => ModelState::NeedsRecovery,
    }
}
fn public_state(s: ModelState) -> InitialSyncState {
    match s {
        ModelState::Ready => InitialSyncState::Ready,
        ModelState::Running => InitialSyncState::Running,
        ModelState::Pausing => InitialSyncState::Pausing,
        ModelState::PausedUser => InitialSyncState::PausedUser,
        ModelState::PausedBudget => InitialSyncState::PausedBudget,
        ModelState::Interrupted => InitialSyncState::Interrupted,
        ModelState::Complete => InitialSyncState::Complete,
        ModelState::Failed => InitialSyncState::Failed,
        ModelState::StaleBinding => InitialSyncState::StaleBinding,
        ModelState::NeedsRecovery => InitialSyncState::NeedsRecovery,
    }
}
#[derive(Clone, Eq, PartialEq)]
struct Binding {
    av: u64,
    cg: u64,
    ev: u64,
    endpoint_hash: [u8; 32],
    catalog_hash: [u8; 32],
    validity: Option<u32>,
    epoch: Option<u64>,
}
struct Row {
    id: [u8; 32],
    request: InitialSyncRequest,
    audit: AuditRecord,
    binding: Binding,
    instance: [u8; 32],
    plan: Option<[u8; 32]>,
    step: Option<(u64, u8, u32, u32)>,
    state: InitialSyncJobState,
    admitted: i64,
    started: Option<i64>,
    completed: Option<i64>,
    receipt: InitialSyncReceipt,
}
pub(crate) struct PreviewPlan {
    pub(crate) snapshot: InitialSyncPlanSnapshot,
    owner: [u8; 16],
    binding: Binding,
    active: Option<[u8; 32]>,
    created: i64,
}
impl PreviewPlan {
    pub(crate) fn observed_at_utc_ms(&self) -> i64 {
        self.created
    }
    fn id(&self) -> &[u8; 32] {
        self.snapshot.plan_id()
    }
    fn progress(&self) -> &InitialSyncProgress {
        self.snapshot.progress()
    }
    fn model(&self) -> Result<InitialSyncPlan, ErrorCode> {
        model(self.progress())
    }
}
impl PreviewPlan {
    pub(crate) fn binding_current(&self, c: &Connection) -> Result<bool, ErrorCode> {
        if !self.snapshot.is_current() {
            return Ok(false);
        }
        let Some(catalog) = load_catalog(c, self.snapshot.account_id())? else {
            return Ok(false);
        };
        let name = mailbox(self.snapshot.mailbox())?;
        if !catalog.binding_current()
            || catalog.generation() != self.binding.cg
            || catalog.account_version() != self.binding.av
            || catalog.endpoint_version() != self.binding.ev
            || !catalog
                .snapshot()
                .mailboxes()
                .iter()
                .any(|m| m.selectable() && m.name() == &name)
            || hash(
                &catalog
                    .snapshot()
                    .to_storage_json()
                    .map_err(|_| ErrorCode::InternalError)?,
            )? != self.binding.catalog_hash
        {
            return Ok(false);
        }
        let Some(endpoint) = current_endpoint(c, self.snapshot.account_id())? else {
            return Ok(false);
        };
        if endpoint.settings_version() != self.binding.ev
            || endpoint_hash(self.snapshot.account_id(), &endpoint)? != self.binding.endpoint_hash
        {
            return Ok(false);
        }
        let cursor = read_cursor(c, self.snapshot.account_id(), &name)?;
        match cursor {
            Some(v) => {
                let epoch = crate::uid_epochs::read(
                    c,
                    self.snapshot.account_id(),
                    &name,
                    v.value.uid_validity(),
                )?
                .ok_or(ErrorCode::InternalError)?;
                Ok(v.value.uid_validity() == self.progress().uid_validity()
                    && self.binding.epoch.is_none_or(|e| e == epoch.generation))
            }
            None => Ok(self.binding.epoch.is_none()),
        }
    }
}

fn read_row(c: &Connection, id: [u8; 32]) -> Result<Option<Row>, ErrorCode> {
    let raw=db(c.query_row("SELECT account_id,mailbox_key,mailbox_encoding,operation_id,idempotency_key,action,request_json,request_hash,binding_json,binding_hash,instance_id,plan_id,step_version,step_ordinal,step_after_uid,step_through_uid,state,admitted_at_utc_ms,started_at_utc_ms,completed_at_utc_ms,result_json,result_hash,error_code FROM imap_initial_preview_requests WHERE execution_id=?1",[id.as_slice()],|r|Ok((text_column(r,0,128)?.to_owned(),text_column(r,1,2048)?.to_owned(),text_column(r,2,13)?.to_owned(),text_column(r,3,128)?.to_owned(),text_column(r,4,128)?.to_owned(),text_column(r,5,14)?.to_owned(),text_column(r,6,8192)?.to_owned(),blob_column::<32>(r,7)?,text_column(r,8,4096)?.to_owned(),blob_column::<32>(r,9)?,blob_column::<32>(r,10)?,opt_hash::<32>(r,11)?,optional_u64_column(r,12)?,r.get::<_,Option<u8>>(13)?,r.get::<_,Option<u32>>(14)?,r.get::<_,Option<u32>>(15)?,text_column(r,16,10)?.to_owned(),r.get::<_,i64>(17)?,r.get::<_,Option<i64>>(18)?,r.get::<_,Option<i64>>(19)?,opt_text(r,20,8192)?,opt_hash::<32>(r,21)?,opt_text(r,22,32)?))).optional())?;
    let Some((
        account,
        key,
        encoding,
        operation,
        idem,
        action,
        json,
        digest,
        bjson,
        bhash,
        instance,
        plan,
        sv,
        so,
        sa,
        st,
        state,
        admitted,
        started,
        completed,
        result,
        result_hash,
        error,
    )) = raw
    else {
        return Ok(None);
    };
    checked_json(&json, &digest)?;
    checked_json(&bjson, &bhash)?;
    let request = codec::request(&json)?;
    let binding = codec::binding(&bjson)?;
    mailbox(request.mailbox()).map_err(|_| ErrorCode::InternalError)?;
    if id == [0; 32]
        || instance == [0; 32]
        || account != request.account_id()
        || key != request.mailbox().wire_hex()
        || encoding != request.mailbox().encoding().as_str()
        || operation != request.operation_id()
        || idem != request.idempotency_key()
        || action != request.action().as_str()
        || request.plan_id().is_some_and(|p| Some(*p) != plan)
    {
        return Err(ErrorCode::InternalError);
    }
    let step = match (sv, so, sa, st) {
        (Some(v), Some(o), Some(a), Some(t))
            if request.action() == InitialSyncAction::Resume
                && v > 0
                && v <= i64::MAX as u64
                && o < 8
                && a < t
                && t - a <= 64
                && t < u32::MAX =>
        {
            Some((v, o, a, t))
        }
        (None, None, None, None) if request.action() != InitialSyncAction::Resume => None,
        _ => return Err(ErrorCode::InternalError),
    };
    let audit = db(read_audit(c, &id))?.ok_or(ErrorCode::InternalError)?;
    if audit.tool() != AuditTool::SyncPage
        || audit.account_ids() != request.account_ids()
        || admitted < audit.started_at_utc_ms()
        || started.is_some_and(|t| t < admitted)
        || completed.is_some_and(|t| t < started.unwrap_or(admitted))
    {
        return Err(ErrorCode::InternalError);
    }
    let state = InitialSyncJobState::parse(&state).map_err(|_| ErrorCode::InternalError)?;
    if (state == InitialSyncJobState::Admitted && started.is_some())
        || (state == InitialSyncJobState::Running && started.is_none())
        || state.is_terminal() != completed.is_some()
    {
        return Err(ErrorCode::InternalError);
    }
    let result = match (result, result_hash) {
        (None, None) => None,
        (Some(s), Some(h)) => {
            checked_json(&s, &h)?;
            Some(codec::snapshot(&s)?)
        }
        _ => return Err(ErrorCode::InternalError),
    };
    if result.as_ref().is_some_and(|s| {
        Some(*s.plan_id()) != plan
            || s.mailbox() != request.mailbox()
            || Some(s.stored_at_utc_ms()) != completed
    }) {
        return Err(ErrorCode::InternalError);
    }
    let error = error
        .map(|s| {
            ErrorCode::ALL
                .into_iter()
                .find(|e| e.as_str() == s)
                .ok_or(ErrorCode::InternalError)
        })
        .transpose()?;
    let receipt = InitialSyncReceipt::new(&account, id, request.action(), state, result, error)
        .map_err(|_| ErrorCode::InternalError)?;
    if let Some(s) = receipt.plan()
        && state == InitialSyncJobState::Succeeded
    {
        let valid = match request.action() {
            InitialSyncAction::Start => {
                s.plan_id() == &id
                    && s.generation() == request.expected_preview_generation().unwrap() + 1
                    && s.version() == 1
                    && s.progress().model_version() == 1
                    && s.progress().completed_page_count() == 0
                    && s.progress().recent_pages() == request.recent_pages().unwrap()
                    && matches!(
                        s.progress().state(),
                        InitialSyncState::Ready | InitialSyncState::Complete
                    )
            }
            InitialSyncAction::Resume => {
                s.version() > request.expected_plan_version().unwrap()
                    && step.is_some_and(|(v, o, _, _)| {
                        s.progress().model_version() == v + 1
                            && s.progress().completed_page_count() == o + 1
                    })
                    && matches!(
                        s.progress().state(),
                        InitialSyncState::PausedBudget | InitialSyncState::Complete
                    )
            }
            InitialSyncAction::Pause => {
                s.version() > request.expected_plan_version().unwrap()
                    && matches!(
                        s.progress().state(),
                        InitialSyncState::Pausing | InitialSyncState::PausedUser
                    )
            }
        };
        if !valid {
            return Err(ErrorCode::InternalError);
        }
    }
    match (completed, audit.completion()) {
        (None, None) => {}
        (Some(t), Some(a))
            if a.completed_at_utc_ms() == t
                && a.terminal().error() == error
                && a.terminal().is_success() == (state == InitialSyncJobState::Succeeded)
                && a.terminal().result_count().is_none() => {}
        _ => return Err(ErrorCode::InternalError),
    }
    Ok(Some(Row {
        id,
        request,
        audit,
        binding,
        instance,
        plan,
        step,
        state,
        admitted,
        started,
        completed,
        receipt,
    }))
}

fn read_plan(c: &Connection, id: [u8; 32]) -> Result<Option<PreviewPlan>, ErrorCode> {
    let raw=db(c.query_row("SELECT owner_registration_id,account_id,mailbox_key,mailbox_encoding,generation,is_current,version,uid_validity,uid_next,recent_pages,completed_pages,state,model_version,binding_json,binding_hash,active_execution_id,latest_execution_id,created_at_utc_ms,updated_at_utc_ms FROM imap_initial_preview_plans WHERE plan_id=?1",[id.as_slice()],|r|Ok((blob_column::<16>(r,0)?,text_column(r,1,128)?.to_owned(),text_column(r,2,2048)?.to_owned(),text_column(r,3,13)?.to_owned(),u64_column(r,4)?,r.get::<_,bool>(5)?,u64_column(r,6)?,r.get::<_,u32>(7)?,r.get::<_,u32>(8)?,r.get::<_,u8>(9)?,r.get::<_,u8>(10)?,text_column(r,11,16)?.to_owned(),u64_column(r,12)?,text_column(r,13,4096)?.to_owned(),blob_column::<32>(r,14)?,opt_hash::<32>(r,15)?,blob_column::<32>(r,16)?,r.get::<_,i64>(17)?,r.get::<_,i64>(18)?))).optional())?;
    let Some((
        owner,
        account,
        key,
        encoding,
        generation,
        current,
        version,
        validity,
        next,
        recent,
        completed,
        state,
        mv,
        bjson,
        bhash,
        active,
        latest,
        created,
        at,
    )) = raw
    else {
        return Ok(None);
    };
    checked_json(&bjson, &bhash)?;
    let binding = codec::binding(&bjson)?;
    let identity = UidMailboxIdentity::new(
        &key,
        UidMailboxEncoding::parse(&encoding).map_err(|_| ErrorCode::InternalError)?,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    mailbox(&identity).map_err(|_| ErrorCode::InternalError)?;
    let progress = InitialSyncProgress::new(
        validity,
        next,
        recent,
        completed,
        InitialSyncState::parse(&state).map_err(|_| ErrorCode::InternalError)?,
        mv,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    let m = model(&progress)?;
    if created <= 0
        || at < created
        || matches!(m.state(), ModelState::Running | ModelState::Pausing) != active.is_some()
    {
        return Err(ErrorCode::InternalError);
    }
    let snapshot = InitialSyncPlanSnapshot::new(
        InitialSyncPlanIdentity::new(id, &account, identity, generation)
            .map_err(|_| ErrorCode::InternalError)?,
        version,
        progress,
        current,
        current,
        at,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    let initial = read_row(c, id)?.ok_or(ErrorCode::InternalError)?;
    let original = initial.receipt.plan().ok_or(ErrorCode::InternalError)?;
    if initial.state != InitialSyncJobState::Succeeded
        || initial.request.action() != InitialSyncAction::Start
        || initial.audit.registration_id() != &owner
        || original.generation() != generation
        || original.account_id() != account
        || original.mailbox() != snapshot.mailbox()
        || original.progress().uid_validity() != validity
        || original.progress().uid_next() != next
        || original.progress().recent_pages() != recent
        || original.stored_at_utc_ms() != created
    {
        return Err(ErrorCode::InternalError);
    }
    let latest_row = read_row(c, latest)?.ok_or(ErrorCode::InternalError)?;
    if latest_row.plan != Some(id) || latest_row.audit.registration_id() != &owner {
        return Err(ErrorCode::InternalError);
    }
    if latest_row.binding != binding || binding.validity.is_some_and(|v| v != validity) {
        return Err(ErrorCode::InternalError);
    }
    if let Some(r) = latest_row.receipt.plan() {
        if r.version() != version
            || r.progress() != snapshot.progress()
            || r.stored_at_utc_ms() != at
        {
            return Err(ErrorCode::InternalError);
        }
    } else if active != Some(latest)
        || m.state() != ModelState::Running
        || latest_row.step.map(|s| s.0) != Some(m.version())
    {
        return Err(ErrorCode::InternalError);
    }
    if let Some(a) = active {
        let r = read_row(c, a)?.ok_or(ErrorCode::InternalError)?;
        if r.state.is_terminal()
            || r.plan != Some(id)
            || r.request.action() != InitialSyncAction::Resume
        {
            return Err(ErrorCode::InternalError);
        }
    }
    let(count,min,max):(u64,Option<u8>,Option<u8>)=db(c.query_row("SELECT count(*),min(ordinal),max(ordinal) FROM imap_initial_preview_pages WHERE plan_id=?1",[id.as_slice()],|r|Ok((u64_column(r,0)?,r.get(1)?,r.get(2)?))))?;
    if count != u64::from(completed)
        || (count == 0 && (min.is_some() || max.is_some()))
        || (count > 0 && (min != Some(0) || max != Some(completed - 1)))
    {
        return Err(ErrorCode::InternalError);
    }
    Ok(Some(PreviewPlan {
        snapshot,
        owner,
        binding,
        active,
        created,
    }))
}
pub(crate) fn current_preview(
    c: &Connection,
    account: &str,
    identity: &UidMailboxIdentity,
) -> Result<Option<PreviewPlan>, ErrorCode> {
    let id:Option<[u8;32]>=db(c.query_row("SELECT plan_id FROM imap_initial_preview_plans WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND is_current=1",params![account,identity.wire_hex(),identity.encoding().as_str()],|r|blob_column(r,0)).optional())?;
    id.map(|id| read_plan(c, id)?.ok_or(ErrorCode::InternalError))
        .transpose()
}
fn binding(
    c: &Transaction<'_>,
    account: &str,
    identity: &UidMailboxIdentity,
    cg: Option<u64>,
) -> Result<(Binding, ImapEndpointConfig), ErrorCode> {
    check_account(c, account)?;
    let (av, state): (u64, String) = db(c.query_row(
        "SELECT settings_version,status FROM accounts WHERE account_id=?1",
        [account],
        |r| Ok((u64_column(r, 0)?, text_column(r, 1, 32)?.into())),
    ))?;
    if av == 0 {
        return Err(ErrorCode::InternalError);
    }
    if state != "ready" {
        return Err(ErrorCode::Conflict);
    }
    let name = mailbox(identity)?;
    let catalog = load_catalog(c, account)?.ok_or(ErrorCode::NotFound)?;
    if !catalog.binding_current()
        || catalog.account_version() != av
        || cg.is_some_and(|v| v != catalog.generation())
    {
        return Err(ErrorCode::Conflict);
    }
    if !catalog
        .snapshot()
        .mailboxes()
        .iter()
        .any(|m| m.selectable() && m.name() == &name)
    {
        return Err(ErrorCode::NotFound);
    }
    let endpoint = current_endpoint(c, account)?.ok_or(ErrorCode::Conflict)?;
    let eh = endpoint_hash(account, &endpoint)?;
    if endpoint.settings_version() != catalog.endpoint_version() {
        return Err(ErrorCode::Conflict);
    }
    let cursor = read_cursor(c, account, &name)?;
    let (validity, epoch) = if let Some(cursor) = cursor {
        if cursor.value.endpoint_version() != endpoint.settings_version()
            || cursor.endpoint_hash != eh
        {
            return Err(ErrorCode::Conflict);
        }
        let epoch = crate::uid_epochs::read(c, account, &name, cursor.value.uid_validity())?
            .ok_or(ErrorCode::InternalError)?;
        (Some(epoch.validity), Some(epoch.generation))
    } else {
        let orphan:bool=db(c.query_row("SELECT EXISTS(SELECT 1 FROM imap_uid_epochs WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3) OR EXISTS(SELECT 1 FROM imap_uid_page_operations WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3) OR EXISTS(SELECT 1 FROM imap_uid_messages WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3)",params![account,identity.wire_hex(),identity.encoding().as_str()],|r|r.get(0)))?;
        if orphan {
            return Err(ErrorCode::InternalError);
        }
        (None, None)
    };
    let b = Binding {
        av,
        cg: catalog.generation(),
        ev: endpoint.settings_version(),
        endpoint_hash: eh,
        catalog_hash: hash(
            &catalog
                .snapshot()
                .to_storage_json()
                .map_err(|_| ErrorCode::InternalError)?,
        )?,
        validity,
        epoch,
    };
    Ok((b, endpoint))
}
fn current_binding(
    c: &Transaction<'_>,
    r: &Row,
) -> Result<(Binding, ImapEndpointConfig), ErrorCode> {
    current_grants(c, &r.audit)?;
    let b = binding(
        c,
        r.request.account_id(),
        r.request.mailbox(),
        Some(r.binding.cg),
    )?;
    if b.0 != r.binding {
        return Err(ErrorCode::Conflict);
    }
    if r.request.action() == InitialSyncAction::Resume {
        let p = read_plan(c, r.plan.ok_or(ErrorCode::InternalError)?)?
            .ok_or(ErrorCode::InternalError)?;
        if !p.snapshot.is_current()
            || p.active != Some(r.id)
            || p.progress().state() != InitialSyncState::Running
            || r.step.map(|s| s.0) != Some(p.progress().model_version())
        {
            return Err(ErrorCode::Conflict);
        }
    }
    Ok(b)
}
pub(crate) fn operation_reserved(
    c: &Connection,
    account: &str,
    operation: &str,
    key: &str,
) -> Result<bool, ErrorCode> {
    db(c.query_row("SELECT EXISTS(SELECT 1 FROM imap_initial_preview_requests WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3))",params![operation,account,key],|r|r.get(0)))
}
pub(crate) fn execution_reserved(c: &Connection, id: [u8; 32]) -> Result<bool, ErrorCode> {
    db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM imap_initial_preview_requests WHERE execution_id=?1)",
        [id.as_slice()],
        |r| r.get(0),
    ))
}
pub(crate) fn validate_history(c: &Connection) -> rusqlite::Result<()> {
    let invalid:bool=c.query_row("SELECT (SELECT count(*) FROM imap_initial_preview_requests)>100000 OR (SELECT count(*) FROM imap_initial_preview_plans)>4096 OR EXISTS(SELECT 1 FROM imap_initial_preview_plans GROUP BY account_id,mailbox_key,mailbox_encoding HAVING count(*)>128) OR EXISTS(SELECT 1 FROM imap_initial_preview_plans p WHERE p.completed_pages<>(SELECT count(*) FROM imap_initial_preview_pages x WHERE x.plan_id=p.plan_id)) OR EXISTS(SELECT 1 FROM imap_initial_preview_pages p WHERE p.inserted_count<>(SELECT count(*) FROM imap_initial_preview_messages m WHERE m.plan_id=p.plan_id AND m.ordinal=p.ordinal))",[],|r|r.get(0))?;
    let collision:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM imap_initial_preview_messages p JOIN imap_message_identities b ON b.internal_message_id=p.internal_message_id WHERE b.account_id<>p.account_id OR b.mailbox_key<>p.mailbox_key OR b.mailbox_encoding<>p.mailbox_encoding OR b.uid_validity<>p.uid_validity OR b.uid<>p.uid) OR EXISTS(SELECT 1 FROM imap_initial_preview_messages p JOIN imap_message_identities b ON b.account_id=p.account_id AND b.mailbox_key=p.mailbox_key AND b.mailbox_encoding=p.mailbox_encoding AND b.uid_validity=p.uid_validity AND b.uid=p.uid WHERE b.internal_message_id<>p.internal_message_id) OR EXISTS(SELECT 1 FROM imap_initial_preview_messages p JOIN imap_initial_preview_messages b ON b.internal_message_id=p.internal_message_id WHERE b.account_id<>p.account_id OR b.mailbox_key<>p.mailbox_key OR b.mailbox_encoding<>p.mailbox_encoding OR b.uid_validity<>p.uid_validity OR b.uid<>p.uid) OR EXISTS(SELECT 1 FROM imap_initial_preview_messages p JOIN imap_initial_preview_messages b ON b.account_id=p.account_id AND b.mailbox_key=p.mailbox_key AND b.mailbox_encoding=p.mailbox_encoding AND b.uid_validity=p.uid_validity AND b.uid=p.uid WHERE b.internal_message_id<>p.internal_message_id)",[],|r|r.get(0))?;
    let namespace:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM imap_initial_preview_plans GROUP BY account_id,mailbox_key,mailbox_encoding HAVING sum(is_current)<>1 OR max(CASE WHEN is_current=1 THEN generation ELSE 0 END)<>max(generation)) OR EXISTS(SELECT 1 FROM imap_initial_preview_messages m JOIN imap_initial_preview_plans p ON p.plan_id=m.plan_id JOIN imap_initial_preview_pages x ON x.plan_id=m.plan_id AND x.ordinal=m.ordinal WHERE m.account_id<>p.account_id OR m.mailbox_key<>p.mailbox_key OR m.mailbox_encoding<>p.mailbox_encoding OR m.uid_validity<>p.uid_validity OR m.uid<=x.after_uid OR m.uid>x.covered_through_uid)",[],|r|r.get(0))?;
    if invalid || collision || namespace {
        Err(rusqlite::Error::InvalidQuery)
    } else {
        Ok(())
    }
}
