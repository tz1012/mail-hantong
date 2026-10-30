use super::*;

fn reject(
    tx: &Transaction<'_>,
    id: [u8; 32],
    error: ErrorCode,
) -> Result<InitialSyncAdmission, ErrorCode> {
    if error == ErrorCode::InternalError {
        return Err(error);
    }
    insert_audit_terminal_tx(tx, id, AuditTerminal::failed(error), now()?)?;
    Ok(InitialSyncAdmission::Rejected(error))
}
fn opposite_execution(c: &Connection, id: [u8; 32]) -> Result<bool, ErrorCode> {
    Ok(crate::uid_page_jobs::execution_reserved(c, id)?
        || crate::uid_reconcile_jobs::execution_reserved(c, id)?
        || crate::uid_recovery_jobs::execution_reserved(c, id)?
        || db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM folder_refresh_requests WHERE execution_id=?1)",
            [id.as_slice()],
            |r| r.get(0),
        ))?)
}
fn opposite_operation(c: &Connection, r: &InitialSyncRequest) -> Result<bool, ErrorCode> {
    Ok(crate::uid_page_jobs::operation_reserved(c,r.account_id(),r.operation_id(),r.idempotency_key())?||crate::uid_reconcile_jobs::operation_reserved(c,r.account_id(),r.operation_id(),r.idempotency_key())?||crate::uid_recovery_jobs::operation_reserved(c,r.account_id(),r.operation_id(),r.idempotency_key())?||crate::folder_refresh::operation_reserved(c,r.account_id(),r.operation_id(),r.idempotency_key())?||db(c.query_row("SELECT EXISTS(SELECT 1 FROM imap_uid_page_operations WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3)) OR EXISTS(SELECT 1 FROM imap_mailbox_catalog_operations WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3))",params![r.operation_id(),r.account_id(),r.idempotency_key()],|r|r.get(0)))?)
}
fn interrupted_start(
    tx: &Transaction<'_>,
    request: &InitialSyncRequest,
    owner: &[u8; 16],
    instance: [u8; 32],
) -> Result<Option<Row>, ErrorCode> {
    let mut statement = db(tx.prepare("SELECT execution_id FROM imap_initial_preview_requests WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND state IN ('admitted','running') LIMIT 2"))?;
    let ids = db(db(statement.query_map(
        params![
            request.account_id(),
            request.mailbox().wire_hex(),
            request.mailbox().encoding().as_str()
        ],
        |r| blob_column::<32>(r, 0),
    ))?
    .collect::<rusqlite::Result<Vec<_>>>())?;
    if ids.len() > 1 {
        return Err(ErrorCode::InternalError);
    }
    let Some(id) = ids.first() else {
        return Ok(None);
    };
    let old = read_row(tx, *id)?.ok_or(ErrorCode::InternalError)?;
    if old.request.action() != InitialSyncAction::Start
        || old.plan.is_some()
        || old.audit.registration_id() != owner
        || old.instance == instance
    {
        return Err(ErrorCode::Conflict);
    }
    Ok(Some(old))
}
fn insert_request(
    tx: &Transaction<'_>,
    id: [u8; 32],
    r: &InitialSyncRequest,
    b: &Binding,
    instance: [u8; 32],
    step: Option<&InitialSyncStep>,
    at: i64,
) -> Result<(), ErrorCode> {
    let json = codec::request_json(r)?;
    let bj = codec::binding_json(b)?;
    change(db(tx.execute("INSERT INTO imap_initial_preview_requests(execution_id,account_id,mailbox_key,mailbox_encoding,operation_id,idempotency_key,action,request_json,request_hash,binding_json,binding_hash,instance_id,plan_id,step_version,step_ordinal,step_after_uid,step_through_uid,state,admitted_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,'admitted',?18)",params![id.as_slice(),r.account_id(),r.mailbox().wire_hex(),r.mailbox().encoding().as_str(),r.operation_id(),r.idempotency_key(),r.action().as_str(),json,hash(json.as_bytes())?.as_slice(),bj,hash(bj.as_bytes())?.as_slice(),instance.as_slice(),r.plan_id().map(|v|v.as_slice()),step.map(|s|s.plan_version() as i64),step.map(|s|i64::from(s.ordinal())),step.map(|s|i64::from(s.after_uid())),step.map(|s|i64::from(s.covered_through_uid())),at]))?)?;
    Ok(())
}
fn new_snapshot(
    p: &PreviewPlan,
    m: &InitialSyncPlan,
    at: i64,
    binding_current: bool,
) -> Result<InitialSyncPlanSnapshot, ErrorCode> {
    let progress = InitialSyncProgress::new(
        m.uid_validity(),
        p.progress().uid_next(),
        p.progress().recent_pages(),
        m.completed_page_count(),
        public_state(m.state()),
        m.version(),
    )
    .map_err(|_| ErrorCode::InternalError)?;
    InitialSyncPlanSnapshot::new(
        p.snapshot.identity().clone(),
        p.snapshot
            .version()
            .checked_add(1)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or(ErrorCode::Conflict)?,
        progress,
        p.snapshot.is_current(),
        binding_current && p.snapshot.is_current(),
        at,
    )
    .map_err(|_| ErrorCode::InternalError)
}
fn write_model(
    tx: &Transaction<'_>,
    p: &PreviewPlan,
    m: &InitialSyncPlan,
    b: &Binding,
    active: Option<[u8; 32]>,
    completion: ([u8; 32], i64),
    current: bool,
) -> Result<InitialSyncPlanSnapshot, ErrorCode> {
    let (latest, at) = completion;
    if at < p.snapshot.stored_at_utc_ms() {
        return Err(ErrorCode::InternalError);
    }
    let s = new_snapshot(p, m, at, current)?;
    let bj = codec::binding_json(b)?;
    change(db(tx.execute("UPDATE imap_initial_preview_plans SET version=?2,completed_pages=?3,state=?4,model_version=?5,binding_json=?6,binding_hash=?7,active_execution_id=?8,latest_execution_id=?9,updated_at_utc_ms=?10 WHERE plan_id=?1 AND version=?11",params![p.id().as_slice(),s.version() as i64,i64::from(m.completed_page_count()),public_state(m.state()).as_str(),m.version() as i64,bj,hash(bj.as_bytes())?.as_slice(),active.as_ref().map(|a|a.as_slice()),latest.as_slice(),at,p.snapshot.version() as i64]))?)?;
    Ok(s)
}
fn finish(
    tx: &Transaction<'_>,
    r: &Row,
    snapshot: Option<InitialSyncPlanSnapshot>,
    error: Option<ErrorCode>,
    at: i64,
) -> Result<InitialSyncReceipt, ErrorCode> {
    if r.state.is_terminal() {
        return Err(ErrorCode::Conflict);
    }
    if at < r.started.unwrap_or(r.admitted) {
        return Err(ErrorCode::InternalError);
    }
    let json = snapshot.as_ref().map(codec::snapshot_json).transpose()?;
    let digest = json.as_ref().map(|s| hash(s.as_bytes())).transpose()?;
    let state = if error.is_some() {
        InitialSyncJobState::Failed
    } else {
        InitialSyncJobState::Succeeded
    };
    change(db(tx.execute("UPDATE imap_initial_preview_requests SET state=?2,completed_at_utc_ms=?3,result_json=?4,result_hash=?5,error_code=?6 WHERE execution_id=?1 AND state IN ('admitted','running')",params![r.id.as_slice(),state.as_str(),at,json,digest.as_ref().map(|d|d.as_slice()),error.map(|e|e.as_str())]))?)?;
    insert_audit_terminal_tx(
        tx,
        r.id,
        match error {
            Some(e) => AuditTerminal::failed(e),
            None => AuditTerminal::succeeded(None).map_err(|_| ErrorCode::InternalError)?,
        },
        at,
    )?;
    let saved = read_row(tx, r.id)?.ok_or(ErrorCode::InternalError)?;
    if saved.receipt.plan() != snapshot.as_ref() || saved.receipt.error() != error {
        return Err(ErrorCode::InternalError);
    }
    Ok(saved.receipt)
}

pub(in crate::initial_sync) fn admit(
    store: &Store,
    id: [u8; 32],
    r: &InitialSyncRequest,
    queue: UidQueueAdmission,
    instance: [u8; 32],
) -> Result<InitialSyncAdmission, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate,|tx|{
 let audit=db(read_audit(tx,&id))?.ok_or(ErrorCode::NotFound)?;
 if audit.completion().is_some()||execution_reserved(tx,id)?||opposite_execution(tx,id)?{return Err(ErrorCode::Conflict)}
 if id==[0;32]||instance==[0;32]||audit.tool()!=AuditTool::SyncPage||audit.account_ids()!=r.account_ids(){return Err(ErrorCode::InvalidInput)}
 if let Err(e)=current_grants(tx,&audit){return reject(tx,id,e)}
 if let Err(e)=mailbox(r.mailbox()){return reject(tx,id,e)}
 if hex(&hash(&r.intent_bytes())?)!=r.intent_hash(){return reject(tx,id,ErrorCode::InvalidInput)}
 if opposite_operation(tx,r)?{return reject(tx,id,ErrorCode::Conflict)}
 let existing:Option<[u8;32]>=db(tx.query_row("SELECT execution_id FROM imap_initial_preview_requests WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3) LIMIT 1",params![r.operation_id(),r.account_id(),r.idempotency_key()],|row|blob_column(row,0)).optional())?;
 if let Some(old)=existing{let old=read_row(tx,old)?.ok_or(ErrorCode::InternalError)?;if old.request!=*r||old.audit.registration_id()!=audit.registration_id(){return reject(tx,id,ErrorCode::Conflict)}validate_receipt(tx,&old)?;insert_audit_terminal_tx(tx,id,AuditTerminal::succeeded(None).map_err(|_|ErrorCode::InternalError)?,now()?)?;return Ok(InitialSyncAdmission::Existing(old.receipt))}
 let requests:i64=db(tx.query_row("SELECT count(*) FROM imap_initial_preview_requests",[],|row|row.get(0)))?;if requests>=100000{return reject(tx,id,ErrorCode::QuotaExceeded)}
 let(b,_)=match binding(tx,r.account_id(),r.mailbox(),r.expected_catalog_generation()){Ok(v)=>v,Err(e)=>return reject(tx,id,e)};
 let at=now()?;if at<audit.started_at_utc_ms(){return Err(ErrorCode::InternalError)}
 let mut plan=if let Some(plan_id)=r.plan_id(){let p=match read_plan(tx,*plan_id)?{Some(p)if p.owner==*audit.registration_id()&&p.snapshot.account_id()==r.account_id()&&p.snapshot.mailbox()==r.mailbox()=>p,_=>return reject(tx,id,ErrorCode::NotFound)};if !p.snapshot.is_current()||Some(p.snapshot.version())!=r.expected_plan_version(){return reject(tx,id,ErrorCode::Conflict)}Some(p)}else{None};
 let mut abandoned_start=None;
 if r.action()==InitialSyncAction::Start{
  let previous=current_preview(tx,r.account_id(),r.mailbox())?;if previous.as_ref().map_or(0,|p|p.snapshot.generation())!=r.expected_preview_generation().unwrap(){return reject(tx,id,ErrorCode::Conflict)}
  if previous.as_ref().is_some_and(|p|p.active.is_some()){return reject(tx,id,ErrorCode::Conflict)}
  abandoned_start=match interrupted_start(tx,r,audit.registration_id(),instance){Ok(old)=>old,Err(e)=>return reject(tx,id,e)};
  let(total,scoped):(i64,i64)=db(tx.query_row("SELECT (SELECT count(*) FROM imap_initial_preview_plans),(SELECT count(*) FROM imap_initial_preview_plans WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3)",params![r.account_id(),r.mailbox().wire_hex(),r.mailbox().encoding().as_str()],|row|Ok((row.get(0)?,row.get(1)?))))?;if total>=4096||scoped>=128{return reject(tx,id,ErrorCode::QuotaExceeded)}
 }else{
  let p=plan.as_ref().ok_or(ErrorCode::InternalError)?;
  if b.ev!=p.binding.ev||b.endpoint_hash!=p.binding.endpoint_hash||b.validity.is_some_and(|v|v!=p.progress().uid_validity())||(p.binding.epoch.is_some()&&p.binding.epoch!=b.epoch){return reject(tx,id,ErrorCode::Conflict)}
  if r.action()==InitialSyncAction::Resume && let Some(active)=p.active{let old=read_row(tx,active)?.ok_or(ErrorCode::InternalError)?;if old.instance==instance{return reject(tx,id,ErrorCode::Conflict)}}
  if r.action()==InitialSyncAction::Pause && let Some(active)=p.active{let old=read_row(tx,active)?.ok_or(ErrorCode::InternalError)?;if old.instance!=instance{return reject(tx,id,ErrorCode::Conflict)}}
 }
 if r.action()!=InitialSyncAction::Pause{match queue{UidQueueAdmission::Full=>return reject(tx,id,ErrorCode::RateLimited),UidQueueAdmission::Closed=>return reject(tx,id,ErrorCode::CoreUnavailable),UidQueueAdmission::Reserved=>{}}}
 if r.action()==InitialSyncAction::Resume{
  if let Some(p)=plan.as_ref() && let Some(active)=p.active{let old=read_row(tx,active)?.ok_or(ErrorCode::InternalError)?;let mut m=p.model()?;m.interrupt(m.version()).map_err(|_|ErrorCode::Conflict)?;let snap=write_model(tx,p,&m,&p.binding,None,(active,at),false)?;finish(tx,&old,Some(snap),Some(ErrorCode::CoreUnavailable),at)?;plan=Some(read_plan(tx,*p.id())?.ok_or(ErrorCode::InternalError)?);}
  let p=plan.as_ref().ok_or(ErrorCode::InternalError)?;let mut m=p.model()?;let step=match m.begin(m.version()){Ok(v)=>v,Err(_)=>return reject(tx,id,ErrorCode::Conflict)};
  insert_request(tx,id,r,&b,instance,Some(&step),at)?;write_model(tx,p,&m,&b,Some(id),(id,at),true)?;
 }else if r.action()==InitialSyncAction::Pause{
  let p=plan.as_ref().ok_or(ErrorCode::InternalError)?;let mut m=p.model()?;if m.pause(m.version()).is_err(){return reject(tx,id,ErrorCode::Conflict)}insert_request(tx,id,r,&p.binding,instance,None,at)?;let snap=write_model(tx,p,&m,&p.binding,p.active,(id,at),b==p.binding)?;let row=read_row(tx,id)?.ok_or(ErrorCode::InternalError)?;let receipt=finish(tx,&row,Some(snap),None,at)?;read_plan(tx,*p.id())?.ok_or(ErrorCode::InternalError)?;return Ok(InitialSyncAdmission::Admitted(receipt))
 }else{if let Some(old)=abandoned_start{finish(tx,&old,None,Some(ErrorCode::CoreUnavailable),at)?;}insert_request(tx,id,r,&b,instance,None,at)?;}
 let row=read_row(tx,id)?.ok_or(ErrorCode::InternalError)?;if let Some(p)=r.plan_id(){read_plan(tx,*p)?.ok_or(ErrorCode::InternalError)?;}Ok(InitialSyncAdmission::Admitted(row.receipt))
})
}

fn fetch(r: &Row, p: Option<&PreviewPlan>) -> Result<InitialSyncFetch, ErrorCode> {
    let name = mailbox(r.request.mailbox())?;
    match r.request.action() {
        InitialSyncAction::Start => Ok(InitialSyncFetch::Observe(
            ImapUidObservationRequest::new(r.request.account_id(), r.binding.ev, name)
                .map_err(|_| ErrorCode::InternalError)?,
        )),
        InitialSyncAction::Resume => {
            let p = p.ok_or(ErrorCode::InternalError)?;
            let (_, _, after, through) = r.step.ok_or(ErrorCode::InternalError)?;
            Ok(InitialSyncFetch::Page(
                ImapUidReconcileRequest::new(
                    r.request.account_id(),
                    r.binding.ev,
                    name,
                    p.progress().uid_validity(),
                    after,
                    through,
                    p.progress().uid_next(),
                )
                .map_err(|_| ErrorCode::InternalError)?,
            ))
        }
        InitialSyncAction::Pause => Err(ErrorCode::InternalError),
    }
}
pub(in crate::initial_sync) fn start(
    store: &Store,
    id: [u8; 32],
) -> Result<InitialSyncStart, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate,|tx|{let row=read_row(tx,id)?.ok_or(ErrorCode::NotFound)?;if row.state.is_terminal(){validate_receipt(tx,&row)?;return Ok(InitialSyncStart::Finished(row.receipt))}
 if row.state!=InitialSyncJobState::Admitted{return Err(ErrorCode::Conflict)}let(_,endpoint)=match current_binding(tx,&row){Ok(v)=>v,Err(ErrorCode::InternalError)=>return Err(ErrorCode::InternalError),Err(e)=>return failure(tx,&row,e).map(InitialSyncStart::Finished)};let p=row.plan.map(|p|read_plan(tx,p)?.ok_or(ErrorCode::InternalError)).transpose()?;let fetch=fetch(&row,p.as_ref())?;let at=now()?;if at<row.admitted{return Err(ErrorCode::InternalError)}change(db(tx.execute("UPDATE imap_initial_preview_requests SET state='running',started_at_utc_ms=?2 WHERE execution_id=?1 AND state='admitted'",params![id.as_slice(),at]))?)?;read_row(tx,id)?.ok_or(ErrorCode::InternalError)?;Ok(InitialSyncStart::Ready(Box::new(InitialSyncJob{execution_id:id,request:row.request,fetch,endpoint})))})
}
pub(in crate::initial_sync) fn check(store: &Store, id: [u8; 32]) -> Result<(), ErrorCode> {
    store.mail_transaction(TransactionBehavior::Deferred, |tx| {
        let row = read_row(tx, id)?.ok_or(ErrorCode::NotFound)?;
        if row.state != InitialSyncJobState::Running {
            return Err(ErrorCode::Conflict);
        }
        current_binding(tx, &row).map(|_| ())
    })
}
pub(in crate::initial_sync) fn observe(
    store: &Store,
    id: [u8; 32],
    observation: &ImapUidObservation,
) -> Result<InitialSyncReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate,|tx|{
 let row=read_row(tx,id)?.ok_or(ErrorCode::NotFound)?;if row.state!=InitialSyncJobState::Running||row.request.action()!=InitialSyncAction::Start{return Err(ErrorCode::Conflict)}
 if let Err(e)=current_binding(tx,&row){if e==ErrorCode::InternalError{return Err(e)}return failure(tx,&row,e)}
 if fetch(&row,None)?!=InitialSyncFetch::Observe(observation.request().clone()){return failure(tx,&row,ErrorCode::InvalidInput)}
 if row.binding.validity.is_some_and(|v|v!=observation.uid_validity()){return failure(tx,&row,ErrorCode::Conflict)}
 let previous=current_preview(tx,row.request.account_id(),row.request.mailbox())?;if previous.as_ref().map_or(0,|p|p.snapshot.generation())!=row.request.expected_preview_generation().unwrap()||previous.as_ref().is_some_and(|p|p.active.is_some()){return failure(tx,&row,ErrorCode::Conflict)}
 let reused:bool=db(tx.query_row("SELECT EXISTS(SELECT 1 FROM imap_initial_preview_plans WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4 AND is_current=0)",params![row.request.account_id(),row.request.mailbox().wire_hex(),row.request.mailbox().encoding().as_str(),i64::from(observation.uid_validity())],|r|r.get(0)))?;
 if reused&&previous.as_ref().is_none_or(|p|p.progress().uid_validity()!=observation.uid_validity()){return failure(tx,&row,ErrorCode::Conflict)}
 let m=InitialSyncPlan::new(observation.uid_validity(),observation.uid_next(),row.request.recent_pages().unwrap()).map_err(|_|ErrorCode::InternalError)?;let at=now()?;if at<row.started.ok_or(ErrorCode::InternalError)?{return Err(ErrorCode::InternalError)}
 if let Some(p)=previous{change(db(tx.execute("UPDATE imap_initial_preview_plans SET is_current=0 WHERE plan_id=?1 AND is_current=1",[p.id().as_slice()]))?)?;}
 let progress=InitialSyncProgress::new(observation.uid_validity(),observation.uid_next(),row.request.recent_pages().unwrap(),0,public_state(m.state()),1).map_err(|_|ErrorCode::InternalError)?;
 let snap=InitialSyncPlanSnapshot::new(InitialSyncPlanIdentity::new(id,row.request.account_id(),row.request.mailbox().clone(),row.request.expected_preview_generation().unwrap()+1).map_err(|_|ErrorCode::InternalError)?,1,progress,true,true,at).map_err(|_|ErrorCode::InternalError)?;let bj=codec::binding_json(&row.binding)?;
 change(db(tx.execute("INSERT INTO imap_initial_preview_plans(plan_id,owner_registration_id,account_id,mailbox_key,mailbox_encoding,generation,is_current,version,uid_validity,uid_next,recent_pages,completed_pages,state,model_version,binding_json,binding_hash,active_execution_id,latest_execution_id,created_at_utc_ms,updated_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?6,1,1,?7,?8,?9,0,?10,1,?11,?12,NULL,?1,?13,?13)",params![id.as_slice(),row.audit.registration_id().as_slice(),row.request.account_id(),row.request.mailbox().wire_hex(),row.request.mailbox().encoding().as_str(),snap.generation() as i64,i64::from(observation.uid_validity()),i64::from(observation.uid_next()),i64::from(row.request.recent_pages().unwrap()),public_state(m.state()).as_str(),bj,hash(bj.as_bytes())?.as_slice(),at]))?)?;
 change(db(tx.execute("UPDATE imap_initial_preview_requests SET plan_id=?2 WHERE execution_id=?1 AND plan_id IS NULL",params![id.as_slice(),id.as_slice()]))?)?;let result=finish(tx,&row,Some(snap),None,at)?;read_plan(tx,id)?.ok_or(ErrorCode::InternalError)?;Ok(result)
})
}
pub(in crate::initial_sync) fn page(
    store: &Store,
    id: [u8; 32],
    page: &ImapUidReconcilePage,
) -> Result<InitialSyncReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate, |tx| {
        let row = read_row(tx, id)?.ok_or(ErrorCode::NotFound)?;
        if row.state != InitialSyncJobState::Running
            || row.request.action() != InitialSyncAction::Resume
        {
            return Err(ErrorCode::Conflict);
        }
        if let Err(e) = current_binding(tx, &row) {
            if e == ErrorCode::InternalError {
                return Err(e);
            }
            return failure(tx, &row, e);
        }
        let p = read_plan(tx, row.plan.ok_or(ErrorCode::InternalError)?)?
            .ok_or(ErrorCode::InternalError)?;
        if fetch(&row, Some(&p))? != InitialSyncFetch::Page(page.request().clone()) {
            return failure(tx, &row, ErrorCode::InvalidInput);
        }
        let mut m = p.model()?;
        let step = m.active_step().ok_or(ErrorCode::InternalError)?;
        m.complete_step(&step, page.request().uid_validity())
            .map_err(|_| ErrorCode::Conflict)?;
        let at = now()?;
        if at < row.started.ok_or(ErrorCode::InternalError)? || at < p.snapshot.stored_at_utc_ms() {
            return Err(ErrorCode::InternalError);
        }
        pages::insert(tx, &p, &row, page, at)?;
        let snap = write_model(tx, &p, &m, &row.binding, None, (id, at), true)?;
        let result = finish(tx, &row, Some(snap), None, at)?;
        let p = read_plan(tx, *p.id())?.ok_or(ErrorCode::InternalError)?;
        let source = preview_source(tx, &p, step.ordinal())?.ok_or(ErrorCode::InternalError)?;
        read_preview_page(tx, &p, &source)?;
        Ok(result)
    })
}
fn failure(
    tx: &Transaction<'_>,
    r: &Row,
    error: ErrorCode,
) -> Result<InitialSyncReceipt, ErrorCode> {
    if r.state.is_terminal() {
        return Err(ErrorCode::Conflict);
    }
    let at = now()?;
    let mut result = None;
    if r.request.action() == InitialSyncAction::Resume {
        let p = read_plan(tx, r.plan.ok_or(ErrorCode::InternalError)?)?
            .ok_or(ErrorCode::InternalError)?;
        if p.active != Some(r.id) {
            return Err(ErrorCode::Conflict);
        }
        let mut m = p.model()?;
        if m.state() == ModelState::Pausing {
            m.acknowledge_pause(m.version())
                .map_err(|_| ErrorCode::InternalError)?;
        } else {
            let endpoint = current_endpoint(tx, r.request.account_id())?;
            let endpoint_changed = match endpoint {
                Some(endpoint) => {
                    endpoint.settings_version() != p.binding.ev
                        || endpoint_hash(r.request.account_id(), &endpoint)?
                            != p.binding.endpoint_hash
                }
                None => true,
            };
            let current = binding(tx, r.request.account_id(), r.request.mailbox(), None);
            if matches!(current, Err(ErrorCode::InternalError)) {
                return Err(ErrorCode::InternalError);
            }
            if endpoint_changed {
                m.invalidate(m.version(), false)
            } else {
                match current {
                    Ok((b, _)) if b.validity.is_some_and(|v| v != m.uid_validity()) => {
                        m.invalidate(m.version(), true)
                    }
                    Ok((b, _))
                        if b.ev != p.binding.ev || b.endpoint_hash != p.binding.endpoint_hash =>
                    {
                        m.invalidate(m.version(), false)
                    }
                    Err(ErrorCode::InternalError) => return Err(ErrorCode::InternalError),
                    _ => m.fail(m.version()),
                }
            }
            .map_err(|_| ErrorCode::InternalError)?;
        }
        result = Some(write_model(
            tx,
            &p,
            &m,
            &p.binding,
            None,
            (r.id, at),
            false,
        )?);
    }
    let receipt = finish(tx, r, result, Some(error), at)?;
    if let Some(p) = r.plan {
        read_plan(tx, p)?.ok_or(ErrorCode::InternalError)?;
    }
    Ok(receipt)
}
pub(in crate::initial_sync) fn fail(
    store: &Store,
    id: [u8; 32],
    error: ErrorCode,
) -> Result<InitialSyncReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate, |tx| {
        let row = read_row(tx, id)?.ok_or(ErrorCode::NotFound)?;
        failure(tx, &row, error)
    })
}
fn validate_receipt(c: &Connection, r: &Row) -> Result<(), ErrorCode> {
    if r.state == InitialSyncJobState::Succeeded {
        let s = r.receipt.plan().ok_or(ErrorCode::InternalError)?;
        let p = read_plan(c, *s.plan_id())?.ok_or(ErrorCode::InternalError)?;
        if r.request.action() == InitialSyncAction::Resume {
            let source = preview_source(c, &p, r.step.ok_or(ErrorCode::InternalError)?.1)?
                .ok_or(ErrorCode::InternalError)?;
            if source.execution_id != r.id {
                return Err(ErrorCode::InternalError);
            }
            read_preview_page(c, &p, &source)?;
        }
    }
    Ok(())
}
pub(in crate::initial_sync) fn receipt(
    store: &Store,
    owner: [u8; 16],
    request: &UidPageStatusRequest,
) -> Result<InitialSyncReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Deferred, |tx| {
        let r = read_row(tx, *request.execution_id())?.ok_or(ErrorCode::NotFound)?;
        if r.audit.registration_id() != &owner || r.request.account_id() != request.account_id() {
            return Err(ErrorCode::NotFound);
        }
        validate_receipt(tx, &r)?;
        Ok(r.receipt)
    })
}
pub(in crate::initial_sync) fn plan_snapshot(
    store: &Store,
    owner: [u8; 16],
    request: &InitialSyncPlanRequest,
) -> Result<InitialSyncPlanSnapshot, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Deferred, |tx| {
        let p = read_plan(tx, *request.plan_id())?.ok_or(ErrorCode::NotFound)?;
        if p.owner != owner
            || p.snapshot.account_id() != request.account_id()
            || p.snapshot.mailbox() != request.mailbox()
        {
            return Err(ErrorCode::NotFound);
        }
        let current = match binding(
            tx,
            request.account_id(),
            request.mailbox(),
            Some(p.binding.cg),
        ) {
            Ok((b, _)) => b == p.binding && p.snapshot.is_current(),
            Err(ErrorCode::InternalError) => return Err(ErrorCode::InternalError),
            Err(_) => false,
        };
        InitialSyncPlanSnapshot::new(
            p.snapshot.identity().clone(),
            p.snapshot.version(),
            p.progress().clone(),
            p.snapshot.is_current(),
            current,
            p.snapshot.stored_at_utc_ms(),
        )
        .map_err(|_| ErrorCode::InternalError)
    })
}
