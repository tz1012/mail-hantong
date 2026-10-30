//! Account-bound Gmail cache and durable one-shot synchronization.
//! Credentials and provider I/O remain inside Secrets. Activation requires the
//! Core's native human review; these database values are not proof of approval.
use crate::Store;
use hantong_contracts::{ErrorCode, google::*};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingGoogleSync {
    pub request: GoogleSyncRequest,
    pub binding: GoogleAccountBinding,
    pub owner: [u8; 16],
    pub profile_epoch: u64,
    pub audit_id: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoogleMessageList {
    pub account_id: String,
    pub messages: Vec<GoogleMessage>,
    pub next_cursor: Option<String>,
}

macro_rules! call {
    ($store:expr, $function:ident $(, $arg:expr)*) => {{
        #[cfg(windows)] { storage::$function($store $(, $arg)*) }
        #[cfg(not(windows))] { let _ = ($store $(, $arg)*); Err(ErrorCode::CapabilityUnsupported) }
    }};
}

impl Store {
    pub fn google_client_id(&self) -> Result<Option<String>, ErrorCode> {
        call!(self, client_id)
    }
    pub fn google_account_for_subject(
        &self,
        subject: &str,
    ) -> Result<Option<GoogleAccountBinding>, ErrorCode> {
        call!(self, for_subject, subject)
    }
    pub fn google_account_binding(
        &self,
        account_id: &str,
    ) -> Result<GoogleAccountBinding, ErrorCode> {
        call!(self, account_binding, account_id)
    }
    /// Publish only after the reviewed credential generation has been stored.
    pub fn activate_google_account(
        &self,
        identity: &GoogleIdentity,
        client_id: &str,
        expected_version: u64,
        name: Option<&str>,
    ) -> Result<GoogleAccountBinding, ErrorCode> {
        call!(self, activate, identity, client_id, expected_version, name)
    }
    pub fn admit_google_sync(
        &self,
        audit_id: [u8; 32],
        owner: [u8; 16],
        epoch: u64,
        request: &GoogleSyncRequest,
    ) -> Result<GoogleSyncReceipt, ErrorCode> {
        call!(self, admit, audit_id, owner, epoch, request)
    }
    pub fn pending_google_sync(
        &self,
        execution_id: [u8; 32],
    ) -> Result<PendingGoogleSync, ErrorCode> {
        call!(self, pending, execution_id)
    }
    pub fn claim_google_sync(
        &self,
        execution_id: [u8; 32],
    ) -> Result<PendingGoogleSync, ErrorCode> {
        call!(self, claim, execution_id)
    }
    pub fn complete_google_sync(
        &self,
        execution_id: [u8; 32],
        page: &GooglePage,
    ) -> Result<GoogleSyncReceipt, ErrorCode> {
        call!(self, complete, execution_id, page)
    }
    pub fn fail_google_sync(
        &self,
        execution_id: [u8; 32],
        error: ErrorCode,
    ) -> Result<GoogleSyncReceipt, ErrorCode> {
        call!(self, fail, execution_id, error)
    }
    pub fn google_sync_status(
        &self,
        owner: [u8; 16],
        epoch: u64,
        account_id: &str,
        execution_id: [u8; 32],
    ) -> Result<GoogleSyncReceipt, ErrorCode> {
        call!(self, status, owner, epoch, account_id, execution_id)
    }
    pub fn recover_google_sync(&self) -> Result<usize, ErrorCode> {
        call!(self, recover)
    }
    pub fn google_sync_version(&self, account_id: &str) -> Result<u64, ErrorCode> {
        call!(self, sync_version, account_id)
    }
    pub fn google_labels(&self, account_id: &str) -> Result<Vec<GoogleLabel>, ErrorCode> {
        call!(self, labels, account_id)
    }
    pub fn google_messages(
        &self,
        account_id: &str,
        label: Option<&str>,
        query: Option<&str>,
        limit: u16,
        after: Option<&str>,
    ) -> Result<GoogleMessageList, ErrorCode> {
        call!(self, messages, account_id, label, query, limit, after)
    }
    pub fn google_message(&self, account_id: &str, id: &str) -> Result<GoogleMessage, ErrorCode> {
        call!(self, message, account_id, id)
    }
    pub fn mark_google_auth_required(
        &self,
        account_id: &str,
        binding_version: u64,
        error: ErrorCode,
    ) -> Result<(), ErrorCode> {
        call!(self, auth_required, account_id, binding_version, error)
    }
}

#[cfg(windows)]
mod storage {
    use super::*;
    use crate::{
        audit::{insert_audit_terminal_tx, read_audit},
        registrations::read_registration,
    };
    use hantong_contracts::audit::{AuditTerminal, AuditTool};
    use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
    use serde_json::{Value, json};

    fn db<T>(v: rusqlite::Result<T>) -> Result<T, ErrorCode> {
        v.map_err(|_| ErrorCode::InternalError)
    }
    fn integer(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
        let v = row.get::<_, i64>(index)?;
        v.try_into()
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, v))
    }
    fn one(result: rusqlite::Result<usize>) -> Result<(), ErrorCode> {
        if db(result)? != 1 {
            return Err(ErrorCode::InternalError);
        }
        Ok(())
    }
    fn run<T>(
        store: &Store,
        f: impl FnOnce(&Transaction<'_>) -> Result<T, ErrorCode>,
    ) -> Result<T, ErrorCode> {
        store.with_connection(|c| {
            let tx = Transaction::new_unchecked(c, TransactionBehavior::Immediate)?;
            match f(&tx) {
                Ok(value) => {
                    tx.commit()?;
                    Ok(Ok(value))
                }
                Err(error) => Ok(Err(error)),
            }
        })?
    }
    fn hash(bytes: &[u8]) -> Result<[u8; 32], ErrorCode> {
        hantong_platform::sha256(bytes)
    }
    fn subject_account(subject: &str) -> Result<String, ErrorCode> {
        Ok(format!(
            "google_{}",
            hantong_contracts::wire_hex::encode(&hash(subject.as_bytes())?)
        ))
    }
    fn check_subject(binding: &GoogleAccountBinding) -> Result<(), ErrorCode> {
        binding.validate().map_err(|_| ErrorCode::InternalError)?;
        if subject_account(&binding.subject)? != binding.account_id {
            return Err(ErrorCode::InternalError);
        }
        Ok(())
    }
    fn binding(c: &Connection, account: &str) -> Result<GoogleAccountBinding, ErrorCode> {
        validate_google_account_id(account)?;
        let value = db(c.query_row("SELECT g.subject,g.client_id,g.credential_version,a.provider,a.provider_subject FROM google_connections g JOIN accounts a USING(account_id) WHERE g.account_id=?1",[account],|r| Ok((GoogleAccountBinding {account_id:account.into(),subject:r.get(0)?,client_id:r.get(1)?,credential_version:integer(r,2)?},r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?))).optional())?.ok_or(ErrorCode::NotFound)?;
        check_subject(&value.0)?;
        if value.1 != "gmail" || value.2.as_deref() != Some(&value.0.subject) {
            return Err(ErrorCode::InternalError);
        }
        Ok(value.0)
    }
    pub(super) fn account_binding(
        s: &Store,
        account: &str,
    ) -> Result<GoogleAccountBinding, ErrorCode> {
        run(s, |c| binding(c, account))
    }
    pub(super) fn for_subject(
        s: &Store,
        subject: &str,
    ) -> Result<Option<GoogleAccountBinding>, ErrorCode> {
        if subject.is_empty() || subject.len() > 255 {
            return Err(ErrorCode::InvalidInput);
        }
        run(s, |c| {
            let id = db(c
                .query_row(
                    "SELECT account_id FROM google_connections WHERE subject=?1",
                    [subject],
                    |r| r.get::<_, String>(0),
                )
                .optional())?;
            id.map(|id| binding(c, &id)).transpose()
        })
    }
    pub(super) fn client_id(s: &Store) -> Result<Option<String>, ErrorCode> {
        let setting = s.get_setting("google.desktop_client_id")?;
        setting
            .map(|v| {
                let id: String =
                    serde_json::from_str(&v.value_json).map_err(|_| ErrorCode::InternalError)?;
                validate_google_client_id(&id).map_err(|_| ErrorCode::InternalError)?;
                Ok(id)
            })
            .transpose()
    }
    pub(super) fn activate(
        s: &Store,
        i: &GoogleIdentity,
        client: &str,
        expected: u64,
        name: Option<&str>,
    ) -> Result<GoogleAccountBinding, ErrorCode> {
        i.validate()?;
        validate_google_client_id(client)?;
        if subject_account(&i.subject)? != i.account_id
            || expected >= i64::MAX as u64 - 1
            || name.is_some_and(|n| {
                n.is_empty()
                    || n.chars().count() > 128
                    || n.len() > 512
                    || n.chars().any(char::is_control)
            })
        {
            return Err(ErrorCode::InvalidInput);
        }
        run(s, |c| {
            let existing = db(c
                .query_row(
                    "SELECT account_id,credential_version FROM google_connections WHERE subject=?1",
                    [&i.subject],
                    |r| Ok((r.get::<_, String>(0)?, integer(r, 1)?)),
                )
                .optional())?;
            match existing {
                Some((account, version)) if account != i.account_id || version != expected => {
                    return Err(ErrorCode::Conflict);
                }
                None if expected != 0 => return Err(ErrorCode::Conflict),
                _ => {}
            }
            let version = expected + 1;
            let scopes =
                serde_json::to_string(&GOOGLE_SCOPES).map_err(|_| ErrorCode::InternalError)?;
            let capabilities = r#"["mail.folders.list","mail.messages.get","mail.messages.search","mail.sync.page"]"#;
            if expected == 0 {
                let exists:bool=db(c.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1 OR (provider='gmail' AND provider_subject=?2))",params![i.account_id,i.subject],|r|r.get(0)))?;
                if exists {
                    return Err(ErrorCode::Conflict);
                }
                let number: i64 = db(c.query_row(
                    "SELECT COALESCE(MAX(account_number),0)+1 FROM accounts",
                    [],
                    |r| r.get(0),
                ))?;
                one(c.execute("INSERT INTO accounts(account_id,provider,provider_subject,primary_address,display_name,account_number,status,capabilities,granted_scopes,settings_version) VALUES(?1,'gmail',?2,?3,?4,?5,'ready',?6,?7,1)",params![i.account_id,i.subject,i.email,name,number,capabilities,scopes]))?;
                one(c.execute(
                    "INSERT INTO google_connections VALUES(?1,?2,?3,?4,?5)",
                    params![
                        i.account_id,
                        i.subject,
                        client,
                        version as i64,
                        db(crate::utc_millis())?
                    ],
                ))?;
                one(c.execute(
                    "INSERT INTO google_sync_state(account_id,version) VALUES(?1,0)",
                    [&i.account_id],
                ))?;
            } else {
                one(c.execute("UPDATE google_connections SET client_id=?1,credential_version=?2,updated_at_utc_ms=?3 WHERE account_id=?4 AND credential_version=?5",params![client,version as i64,db(crate::utc_millis())?,i.account_id,expected as i64]))?;
                one(c.execute("UPDATE accounts SET primary_address=?1,display_name=COALESCE(?2,display_name),status='ready',capabilities=?3,granted_scopes=?4,settings_version=settings_version+1 WHERE account_id=?5 AND provider='gmail' AND provider_subject=?6",params![i.email,name,capabilities,scopes,i.account_id,i.subject]))?;
                // Reauthorization retains cached messages and never changes client scopes.
            }
            let client_json =
                serde_json::to_string(client).map_err(|_| ErrorCode::InternalError)?;
            one(c.execute("INSERT INTO settings(setting_key,value_json,version,updated_at_utc_ms) VALUES('google.desktop_client_id',?1,1,?2) ON CONFLICT(setting_key) DO UPDATE SET value_json=excluded.value_json,version=settings.version+1,updated_at_utc_ms=excluded.updated_at_utc_ms",params![client_json,db(crate::utc_millis())?]))?;
            binding(c, &i.account_id)
        })
    }

    fn request_json(r: &GoogleSyncRequest) -> Value {
        json!({"account_id":r.account_id,"label_id":r.label_id,"page_token":r.page_token,"limit":r.limit,"expected_version":r.expected_version,"operation_id":r.operation_id,"idempotency_key":r.idempotency_key})
    }
    fn string(v: &Value, key: &str) -> Result<String, ErrorCode> {
        v.get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(ErrorCode::InternalError)
    }
    fn opt_string(v: &Value, key: &str) -> Result<Option<String>, ErrorCode> {
        match v.get(key) {
            Some(Value::Null) => Ok(None),
            Some(Value::String(v)) => Ok(Some(v.clone())),
            _ => Err(ErrorCode::InternalError),
        }
    }
    fn uint(v: &Value, key: &str) -> Result<u64, ErrorCode> {
        v.get(key)
            .and_then(Value::as_u64)
            .ok_or(ErrorCode::InternalError)
    }
    fn parse_request(raw: &str) -> Result<GoogleSyncRequest, ErrorCode> {
        let v: Value = serde_json::from_str(raw).map_err(|_| ErrorCode::InternalError)?;
        let r = GoogleSyncRequest {
            account_id: string(&v, "account_id")?,
            label_id: opt_string(&v, "label_id")?,
            page_token: opt_string(&v, "page_token")?,
            limit: uint(&v, "limit")?
                .try_into()
                .map_err(|_| ErrorCode::InternalError)?,
            expected_version: uint(&v, "expected_version")?,
            operation_id: string(&v, "operation_id")?,
            idempotency_key: string(&v, "idempotency_key")?,
        };
        r.validate().map_err(|_| ErrorCode::InternalError)?;
        Ok(r)
    }
    struct Job {
        pending: PendingGoogleSync,
        account_version: u64,
        binding_version: u64,
        receipt: GoogleSyncReceipt,
    }
    fn job(c: &Connection, id: [u8; 32]) -> Result<Job, ErrorCode> {
        let raw=db(c.query_row("SELECT owner,profile_epoch,request_json,binding_version,account_version,state,result_version,next_page_token,error_code,downloaded_count,request_hash,account_id,operation_id,idempotency_key,completed_at_utc_ms FROM google_sync_requests WHERE execution_id=?1",[id.as_slice()],|r| Ok((r.get::<_,Vec<u8>>(0)?,integer(r,1)?,r.get::<_,String>(2)?,integer(r,3)?,integer(r,4)?,r.get::<_,String>(5)?,integer(r,6)?,r.get::<_,Option<String>>(7)?,r.get::<_,Option<String>>(8)?,integer(r,9)?,r.get::<_,Vec<u8>>(10)?,r.get::<_,String>(11)?,r.get::<_,String>(12)?,r.get::<_,String>(13)?,r.get::<_,Option<i64>>(14)?))).optional())?.ok_or(ErrorCode::NotFound)?;
        let r = parse_request(&raw.2)?;
        if r.operation_id != raw.12
            || r.idempotency_key != raw.13
            || r.account_id != raw.11
            || hash(request_json(&r).to_string().as_bytes())?.as_slice() != raw.10
        {
            return Err(ErrorCode::InternalError);
        }
        let owner = raw.0.try_into().map_err(|_| ErrorCode::InternalError)?;
        let error = raw
            .8
            .map(|e| {
                ErrorCode::ALL
                    .into_iter()
                    .find(|code| code.as_str() == e)
                    .ok_or(ErrorCode::InternalError)
            })
            .transpose()?;
        let receipt = GoogleSyncReceipt {
            account_id: r.account_id.clone(),
            execution_id: id,
            state: raw.5,
            version: raw.6,
            next_page_token: raw.7,
            error_code: error,
            downloaded_count: raw.9,
        };
        receipt.validate().map_err(|_| ErrorCode::InternalError)?;
        let audit = db(read_audit(c, &id))?.ok_or(ErrorCode::InternalError)?;
        if audit.registration_id() != &owner
            || audit.profile_epoch() != raw.1
            || audit.tool() != AuditTool::SyncPage
            || audit.account_ids() != [r.account_id.clone()]
        {
            return Err(ErrorCode::InternalError);
        }
        if matches!(receipt.state.as_str(), "pending" | "running") != audit.completion().is_none() {
            return Err(ErrorCode::InternalError);
        }
        if let Some(done) = audit.completion()
            && (Some(done.completed_at_utc_ms()) != raw.14
                || done.terminal().result_count().is_some()
                || done.terminal().error() != error
                || done.terminal().is_success() != (receipt.state == "completed"))
        {
            return Err(ErrorCode::InternalError);
        }
        let expected_result = r.expected_version + u64::from(receipt.state == "completed");
        if receipt.version != expected_result
            || (receipt.state != "completed"
                && (receipt.downloaded_count != 0 || receipt.next_page_token.is_some()))
        {
            return Err(ErrorCode::InternalError);
        }
        Ok(Job {
            pending: PendingGoogleSync {
                binding: binding(c, &r.account_id)?,
                request: r,
                owner,
                profile_epoch: raw.1,
                audit_id: id,
            },
            account_version: raw.4,
            binding_version: raw.3,
            receipt,
        })
    }
    fn grants(c: &Connection, owner: [u8; 16], epoch: u64, account: &str) -> Result<(), ErrorCode> {
        let r = db(read_registration(c, &owner))?.ok_or(ErrorCode::ClientNotRegistered)?;
        if r.state != crate::RegistrationState::Active {
            return Err(ErrorCode::ClientSuspended);
        }
        if r.profile_epoch != epoch {
            return Err(ErrorCode::AuthRequired);
        }
        if !r.scope.accounts().iter().any(|a| a == account)
            || !r.scope.tools().iter().any(|t| t == "mail.sync.page")
        {
            return Err(ErrorCode::ScopeDenied);
        }
        if r.client_kind == crate::ClientKind::Mcp {
            let consent:bool=db(c.query_row("SELECT EXISTS(SELECT 1 FROM mcp_ai_consents consent JOIN client_registrations registration ON registration.registration_id=consent.registration_id WHERE consent.registration_id=?1 AND consent.account_id=?2 AND consent.profile_epoch=?3 AND consent.registration_request_id=registration.request_id AND consent.approved_at_utc_ms=registration.approved_at_utc_ms)",params![owner.as_slice(),account,epoch as i64],|row|row.get(0)))?;
            if !consent {
                return Err(ErrorCode::PolicyBlocked);
            }
        }
        Ok(())
    }
    type SyncState = (u64, Option<String>, Option<String>, u64, String);
    fn state(c: &Connection, account: &str) -> Result<SyncState, ErrorCode> {
        db(c.query_row("SELECT s.version,s.next_page_token,s.label_id,a.settings_version,a.status FROM google_sync_state s JOIN accounts a USING(account_id) WHERE s.account_id=?1",[account],|r|Ok((integer(r,0)?,r.get(1)?,r.get(2)?,integer(r,3)?,r.get(4)?))))
    }
    fn current(c: &Connection, j: &Job) -> Result<(), ErrorCode> {
        grants(
            c,
            j.pending.owner,
            j.pending.profile_epoch,
            &j.pending.request.account_id,
        )?;
        let (version, token, label, av, status) = state(c, &j.pending.request.account_id)?;
        if status != "ready" {
            return Err(ErrorCode::AuthRequired);
        }
        if av != j.account_version
            || j.pending.binding.credential_version != j.binding_version
            || version != j.pending.request.expected_version
        {
            return Err(ErrorCode::Conflict);
        }
        if j.pending.request.page_token.is_some()
            && (token != j.pending.request.page_token || label != j.pending.request.label_id)
        {
            return Err(ErrorCode::CursorExpired);
        }
        Ok(())
    }
    pub(super) fn admit(
        s: &Store,
        id: [u8; 32],
        owner: [u8; 16],
        epoch: u64,
        r: &GoogleSyncRequest,
    ) -> Result<GoogleSyncReceipt, ErrorCode> {
        r.validate()?;
        if r.expected_version >= i64::MAX as u64 - 1 {
            return Err(ErrorCode::QuotaExceeded);
        }
        if id == [0; 32] || owner == [0; 16] || epoch == 0 || epoch > i64::MAX as u64 {
            return Err(ErrorCode::InvalidInput);
        }
        run(s, |c| {
            let audit = db(read_audit(c, &id))?.ok_or(ErrorCode::NotFound)?;
            if audit.registration_id() != &owner
                || audit.profile_epoch() != epoch
                || audit.account_ids() != [r.account_id.clone()]
                || audit.tool() != AuditTool::SyncPage
                || audit.completion().is_some()
            {
                return Err(ErrorCode::Conflict);
            }
            grants(c, owner, epoch, &r.account_id)?;
            let raw = request_json(r).to_string();
            let digest = hash(raw.as_bytes())?;
            let mut st=db(c.prepare("SELECT execution_id FROM google_sync_requests WHERE account_id=?1 AND (operation_id=?2 OR idempotency_key=?3)"))?;
            let ids = db(st.query_map(
                params![r.account_id, r.operation_id, r.idempotency_key],
                |r| r.get::<_, Vec<u8>>(0),
            ))?
            .collect::<rusqlite::Result<Vec<_>>>();
            let ids = db(ids)?;
            drop(st);
            if !ids.is_empty() {
                if ids.len() != 1 {
                    return Err(ErrorCode::Conflict);
                }
                let old = job(
                    c,
                    ids[0]
                        .clone()
                        .try_into()
                        .map_err(|_| ErrorCode::InternalError)?,
                )?;
                if old.pending.owner != owner
                    || old.pending.profile_epoch != epoch
                    || old.pending.request != *r
                {
                    return Err(ErrorCode::Conflict);
                }
                if old.pending.audit_id == id {
                    return Ok(old.receipt);
                }
                insert_audit_terminal_tx(
                    c,
                    id,
                    AuditTerminal::succeeded(None)?,
                    db(crate::utc_millis())?,
                )?;
                return Ok(old.receipt);
            }
            let b = binding(c, &r.account_id)?;
            let (version, token, label, av, status) = state(c, &r.account_id)?;
            if status != "ready" {
                return Err(ErrorCode::AuthRequired);
            }
            if version != r.expected_version {
                return Err(ErrorCode::Conflict);
            }
            if r.page_token.is_some() && (r.page_token != token || r.label_id != label) {
                return Err(ErrorCode::CursorExpired);
            }
            let busy:bool=db(c.query_row("SELECT EXISTS(SELECT 1 FROM google_sync_requests WHERE account_id=?1 AND state IN ('pending','running'))",[&r.account_id],|r|r.get(0)))?;
            if busy {
                return Err(ErrorCode::Conflict);
            }
            one(c.execute("INSERT INTO google_sync_requests(execution_id,owner,profile_epoch,account_id,operation_id,idempotency_key,request_hash,request_json,binding_version,account_version,state,result_version,downloaded_count,admitted_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'pending',?11,0,?12)",params![id.as_slice(),owner.as_slice(),epoch as i64,r.account_id,r.operation_id,r.idempotency_key,digest.as_slice(),raw,b.credential_version as i64,av as i64,version as i64,db(crate::utc_millis())?]))?;
            Ok(job(c, id)?.receipt)
        })
    }
    pub(super) fn pending(s: &Store, id: [u8; 32]) -> Result<PendingGoogleSync, ErrorCode> {
        run(s, |c| {
            let j = job(c, id)?;
            if j.receipt.state != "pending" {
                return Err(ErrorCode::Conflict);
            }
            current(c, &j)?;
            Ok(j.pending)
        })
    }
    pub(super) fn claim(s: &Store, id: [u8; 32]) -> Result<PendingGoogleSync, ErrorCode> {
        run(s, |c| {
            let j = job(c, id)?;
            if j.receipt.state != "pending" {
                return Err(ErrorCode::Conflict);
            }
            current(c, &j)?;
            if db(c.execute("UPDATE google_sync_requests SET state='running' WHERE execution_id=?1 AND state='pending'",[id.as_slice()]))?!=1{return Err(ErrorCode::Conflict)}
            Ok(j.pending)
        })
    }
    fn terminal(
        c: &Connection,
        j: &Job,
        error: Option<ErrorCode>,
        interrupted: bool,
        count: u64,
        next: Option<&str>,
    ) -> Result<GoogleSyncReceipt, ErrorCode> {
        let at = db(crate::utc_millis())?;
        let version = j.pending.request.expected_version + u64::from(error.is_none());
        let state = if interrupted {
            "interrupted"
        } else if error.is_some() {
            "failed"
        } else {
            "completed"
        };
        insert_audit_terminal_tx(
            c,
            j.pending.audit_id,
            match error {
                Some(e) => AuditTerminal::failed(e),
                None => AuditTerminal::succeeded(None)?,
            },
            at,
        )?;
        one(c.execute("UPDATE google_sync_requests SET state=?1,result_version=?2,next_page_token=?3,downloaded_count=?4,error_code=?5,completed_at_utc_ms=?6 WHERE execution_id=?7 AND state IN ('pending','running')",params![state,version as i64,next,count as i64,error.map(ErrorCode::as_str),at,j.pending.audit_id.as_slice()]))?;
        Ok(job(c, j.pending.audit_id)?.receipt)
    }
    pub(super) fn fail(
        s: &Store,
        id: [u8; 32],
        error: ErrorCode,
    ) -> Result<GoogleSyncReceipt, ErrorCode> {
        run(s, |c| {
            let j = job(c, id)?;
            if !matches!(j.receipt.state.as_str(), "pending" | "running") {
                return Ok(j.receipt);
            }
            terminal(c, &j, Some(error), false, 0, None)
        })
    }
    pub(super) fn complete(
        s: &Store,
        id: [u8; 32],
        p: &GooglePage,
    ) -> Result<GoogleSyncReceipt, ErrorCode> {
        p.validate()?;
        run(s, |c| {
            let j = job(c, id)?;
            if !matches!(j.receipt.state.as_str(), "pending" | "running") {
                return Ok(j.receipt);
            }
            if j.receipt.state != "running" {
                return Err(ErrorCode::Conflict);
            }
            if let Err(error) = current(c, &j) {
                return terminal(c, &j, Some(error), false, 0, None);
            }
            if p.account_id != j.pending.binding.account_id
                || p.subject != j.pending.binding.subject
                || p.messages.len() > usize::from(j.pending.request.limit)
            {
                return terminal(c, &j, Some(ErrorCode::InvalidInput), false, 0, None);
            }
            db(c.execute(
                "DELETE FROM google_labels WHERE account_id=?1",
                [&p.account_id],
            ))?;
            for l in &p.labels {
                one(c.execute(
                    "INSERT INTO google_labels VALUES(?1,?2,?3,?4)",
                    params![p.account_id, l.id, l.name, l.kind],
                ))?;
            }
            for m in &p.messages {
                one(c.execute("INSERT INTO google_messages(account_id,message_id,internal_date_ms,message_json) VALUES(?1,?2,?3,?4) ON CONFLICT(account_id,message_id) DO UPDATE SET internal_date_ms=excluded.internal_date_ms,message_json=excluded.message_json",params![p.account_id,m.id,m.internal_date_ms,message_json(m).to_string()]))?;
            }
            one(c.execute("UPDATE google_sync_state SET version=version+1,next_page_token=?1,label_id=?2 WHERE account_id=?3 AND version=?4",params![p.next_page_token,j.pending.request.label_id,p.account_id,j.pending.request.expected_version as i64]))?;
            terminal(
                c,
                &j,
                None,
                false,
                p.messages.len() as u64,
                p.next_page_token.as_deref(),
            )
        })
    }
    pub(super) fn status(
        s: &Store,
        owner: [u8; 16],
        epoch: u64,
        account: &str,
        id: [u8; 32],
    ) -> Result<GoogleSyncReceipt, ErrorCode> {
        run(s, |c| {
            grants(c, owner, epoch, account)?;
            let j = job(c, id)?;
            if j.pending.owner != owner
                || j.pending.profile_epoch != epoch
                || j.pending.request.account_id != account
            {
                return Err(ErrorCode::ScopeDenied);
            }
            Ok(j.receipt)
        })
    }
    pub(super) fn recover(s: &Store) -> Result<usize, ErrorCode> {
        run(s, |c| {
            let mut stmt=db(c.prepare("SELECT execution_id FROM google_sync_requests WHERE state IN ('pending','running')"))?;
            let ids = db(db(stmt.query_map([], |r| r.get::<_, Vec<u8>>(0)))?
                .collect::<rusqlite::Result<Vec<_>>>())?;
            drop(stmt);
            for id in &ids {
                let j = job(
                    c,
                    id.clone()
                        .try_into()
                        .map_err(|_| ErrorCode::InternalError)?,
                )?;
                terminal(c, &j, Some(ErrorCode::Offline), true, 0, None)?;
            }
            Ok(ids.len())
        })
    }
    fn cache_account(c: &Connection, account: &str) -> Result<(), ErrorCode> {
        binding(c, account)?;
        let status: String = db(c.query_row(
            "SELECT status FROM accounts WHERE account_id=?1",
            [account],
            |r| r.get(0),
        ))?;
        if status == "disabled" {
            return Err(ErrorCode::ScopeDenied);
        }
        Ok(())
    }
    pub(super) fn sync_version(s: &Store, account: &str) -> Result<u64, ErrorCode> {
        run(s, |c| {
            cache_account(c, account)?;
            Ok(state(c, account)?.0)
        })
    }
    pub(super) fn labels(s: &Store, account: &str) -> Result<Vec<GoogleLabel>, ErrorCode> {
        run(s, |c| {
            cache_account(c, account)?;
            let mut stmt=db(c.prepare("SELECT label_id,name,kind FROM google_labels WHERE account_id=?1 ORDER BY label_id"))?;
            let items = db(db(stmt.query_map([account], |r| {
                Ok(GoogleLabel {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    kind: r.get(2)?,
                })
            }))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
            for item in &items {
                item.validate().map_err(|_| ErrorCode::InternalError)?
            }
            Ok(items)
        })
    }
    fn message_json(m: &GoogleMessage) -> Value {
        json!({"id":m.id,"thread_id":m.thread_id,"internal_date_ms":m.internal_date_ms,"label_ids":m.label_ids,"subject":m.subject,"sender":m.sender,"snippet":m.snippet,"body_text":m.body_text,"body_error":m.body_error,"body_truncated":m.body_truncated,"size":m.size})
    }
    fn parse_message(raw: &str) -> Result<GoogleMessage, ErrorCode> {
        let v: Value = serde_json::from_str(raw).map_err(|_| ErrorCode::InternalError)?;
        let m = GoogleMessage {
            id: string(&v, "id")?,
            thread_id: string(&v, "thread_id")?,
            internal_date_ms: v
                .get("internal_date_ms")
                .and_then(Value::as_i64)
                .ok_or(ErrorCode::InternalError)?,
            label_ids: v
                .get("label_ids")
                .and_then(Value::as_array)
                .ok_or(ErrorCode::InternalError)?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or(ErrorCode::InternalError)
                })
                .collect::<Result<_, _>>()?,
            subject: string(&v, "subject")?,
            sender: string(&v, "sender")?,
            snippet: string(&v, "snippet")?,
            body_text: opt_string(&v, "body_text")?,
            body_error: opt_string(&v, "body_error")?,
            body_truncated: v
                .get("body_truncated")
                .and_then(Value::as_bool)
                .ok_or(ErrorCode::InternalError)?,
            size: uint(&v, "size")?,
        };
        m.validate().map_err(|_| ErrorCode::InternalError)?;
        Ok(m)
    }
    pub(super) fn message(s: &Store, account: &str, id: &str) -> Result<GoogleMessage, ErrorCode> {
        if id.is_empty() || id.len() > 256 {
            return Err(ErrorCode::InvalidInput);
        }
        run(s, |c| {
            cache_account(c, account)?;
            let raw=db(c.query_row("SELECT message_json FROM google_messages WHERE account_id=?1 AND message_id=?2",params![account,id],|r|r.get::<_,String>(0)).optional())?.ok_or(ErrorCode::NotFound)?;
            let m = parse_message(&raw)?;
            if m.id != id {
                return Err(ErrorCode::InternalError);
            }
            Ok(m)
        })
    }
    pub(super) fn messages(
        s: &Store,
        account: &str,
        label: Option<&str>,
        query: Option<&str>,
        limit: u16,
        after: Option<&str>,
    ) -> Result<GoogleMessageList, ErrorCode> {
        if !(1..=100).contains(&limit)
            || label.is_some_and(|v| v.is_empty() || v.len() > 256)
            || query.is_some_and(|v| v.len() > 1024)
            || after.is_some_and(|v| v.is_empty() || v.len() > 256)
        {
            return Err(ErrorCode::InvalidInput);
        }
        run(s, |c| {
            cache_account(c, account)?;
            let cursor=after.map(|id|db(c.query_row("SELECT internal_date_ms FROM google_messages WHERE account_id=?1 AND message_id=?2",params![account,id],|r|r.get::<_,i64>(0)).optional())?.ok_or(ErrorCode::CursorExpired)).transpose()?;
            let mut stmt=db(c.prepare("SELECT message_id,message_json FROM google_messages m WHERE account_id=?1 AND (?2 IS NULL OR EXISTS(SELECT 1 FROM json_each(m.message_json,'$.label_ids') WHERE value=?2)) AND (?3 IS NULL OR instr(lower(json_extract(message_json,'$.subject') || ' ' || json_extract(message_json,'$.sender') || ' ' || json_extract(message_json,'$.snippet')),lower(?3))>0) AND (?4 IS NULL OR internal_date_ms<?4 OR (internal_date_ms=?4 AND message_id<?5)) ORDER BY internal_date_ms DESC,message_id DESC LIMIT ?6"))?;
            let rows = db(db(stmt.query_map(
                params![account, label, query, cursor, after, i64::from(limit) + 1],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            ))?
            .collect::<rusqlite::Result<Vec<_>>>())?;
            let has_more = rows.len() > usize::from(limit);
            let mut messages = Vec::new();
            for (id, raw) in rows.into_iter().take(usize::from(limit)) {
                let m = parse_message(&raw)?;
                if m.id != id {
                    return Err(ErrorCode::InternalError);
                }
                messages.push(m)
            }
            let next_cursor = if has_more {
                messages.last().map(|m| m.id.clone())
            } else {
                None
            };
            Ok(GoogleMessageList {
                account_id: account.into(),
                messages,
                next_cursor,
            })
        })
    }
    pub(super) fn validate_history(c: &Connection) -> rusqlite::Result<()> {
        let check = (|| -> Result<(), ErrorCode> {
            let mut connections =
                db(c.prepare("SELECT account_id FROM google_connections ORDER BY account_id"))?;
            let accounts = db(db(connections.query_map([], |r| r.get::<_, String>(0)))?
                .collect::<rusqlite::Result<Vec<_>>>())?;
            for account in accounts {
                binding(c, &account)?;
                state(c, &account)?;
            }
            let mut requests = db(
                c.prepare("SELECT execution_id FROM google_sync_requests ORDER BY execution_id")
            )?;
            let ids = db(db(requests.query_map([], |r| r.get::<_, Vec<u8>>(0)))?
                .collect::<rusqlite::Result<Vec<_>>>())?;
            for id in ids {
                job(c, id.try_into().map_err(|_| ErrorCode::InternalError)?)?;
            }
            Ok(())
        })();
        check.map_err(|_| rusqlite::Error::InvalidQuery)
    }
    pub(super) fn auth_required(
        s: &Store,
        account: &str,
        version: u64,
        error: ErrorCode,
    ) -> Result<(), ErrorCode> {
        if !matches!(error, ErrorCode::AuthRequired | ErrorCode::ScopeDenied) {
            return Err(ErrorCode::InvalidInput);
        }
        run(s, |c| {
            let b = binding(c, account)?;
            if b.credential_version != version {
                return Err(ErrorCode::Conflict);
            }
            db(c.execute("UPDATE accounts SET status='reauth_required' WHERE account_id=?1 AND status!='disabled'",[account]))?;
            Ok(())
        })
    }
}

#[cfg(all(test, windows))]
mod tests;

#[cfg(windows)]
pub(super) fn validate_history(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    storage::validate_history(c)
}
