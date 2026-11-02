//! Durable single-attempt Gmail changes. This module never issues native proof.
use hantong_contracts::{
    ErrorCode, google::GoogleAccountBinding, google_mutation::MutationPayload,
    google_write::ComposeSnapshot,
};
#[derive(Clone, Eq, PartialEq)]
pub struct PendingMutation {
    pub review_id: [u8; 32],
    pub owner_registration_id: [u8; 16],
    pub profile_epoch: u64,
    pub account_version: u64,
    pub binding: GoogleAccountBinding,
    pub sender: String,
    pub request: MutationPayload,
    pub compose: Option<ComposeSnapshot>,
    pub before_labels: Vec<String>,
    pub created_at_utc_ms: i64,
    pub expires_at_utc_ms: i64,
}
impl std::fmt::Debug for PendingMutation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PendingMutation(<redacted>)")
    }
}
#[cfg(windows)]
pub(crate) fn validate_history(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    use crate::registrations::blob_column;
    use hantong_contracts::audit::AuditTool;
    let invalid = || rusqlite::Error::InvalidQuery;
    let mut q=c.prepare("SELECT owner_registration_id,profile_epoch,account_id,operation_id,idempotency_key,request_json,before_labels_json,state,audit_id,error_code FROM google_mutation_requests LIMIT 10001")?;
    let mut rows = q.query([])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        count += 1;
        if count > 10000 {
            return Err(invalid());
        }
        let owner = blob_column::<16>(row, 0)?;
        let epoch = row.get::<_, i64>(1)?;
        let account: String = row.get(2)?;
        let op: String = row.get(3)?;
        let key: String = row.get(4)?;
        let raw: String = row.get(5)?;
        if raw.len() > 65536 || epoch < 1 {
            return Err(invalid());
        }
        let value: serde_json::Value = serde_json::from_str(&raw).map_err(|_| invalid())?;
        let request = MutationPayload::from_json(&value).map_err(|_| invalid())?;
        if request.account_id() != account
            || request.operation_id() != op
            || request.idempotency_key() != key
            || serde_json::to_string(&request.to_json()).map_err(|_| invalid())? != raw
        {
            return Err(invalid());
        }
        let before: String = row.get(6)?;
        let labels: Vec<String> = serde_json::from_str(&before).map_err(|_| invalid())?;
        if labels.len() > 100
            || labels.iter().any(|s| {
                s.is_empty()
                    || s.len() > 256
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
        {
            return Err(invalid());
        }
        if let MutationPayload::Labels {
            change,
            payload_hash,
            ..
        } = &request
            && hantong_platform::sha256_storage_payload(&change.canonical_payload())
                .map_err(|_| invalid())?
                != *payload_hash
        {
            return Err(invalid());
        }
        let state: String = row.get(7)?;
        let audit: Option<Vec<u8>> = row.get(8)?;
        let error: Option<String> = row.get(9)?;
        let error = error
            .map(|s| {
                ErrorCode::ALL
                    .into_iter()
                    .find(|e| e.as_str() == s)
                    .ok_or_else(invalid)
            })
            .transpose()?;
        let valid = match state.as_str() {
            "pending" | "claimed" | "accepted" => error.is_none(),
            "unknown" => error == Some(ErrorCode::SendUnknown),
            "expired" => error == Some(ErrorCode::ApprovalExpired),
            "denied" | "rejected" => error.is_some(),
            _ => false,
        };
        if !valid {
            return Err(invalid());
        }
        if let Some(audit) = audit {
            if matches!(state.as_str(), "pending" | "denied" | "expired") {
                return Err(invalid());
            }
            let id: [u8; 32] = audit.try_into().map_err(|_| invalid())?;
            let record = crate::audit::read_audit(c, &id)?.ok_or_else(invalid)?;
            if record.registration_id() != &owner
                || record.profile_epoch() != epoch as u64
                || record.tool() != AuditTool::GoogleMutationExecute
                || record.account_ids() != request.account_ids()
            {
                return Err(invalid());
            }
            match record.completion() {
                None if state == "claimed" => {}
                Some(done) if state != "claimed" && done.terminal().error() == error => {}
                _ => return Err(invalid()),
            }
        } else if !matches!(state.as_str(), "pending" | "denied" | "expired") {
            return Err(invalid());
        }
    }
    Ok(())
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutationReceipt {
    pub review_id: [u8; 32],
    pub state: String,
    pub error: Option<ErrorCode>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationOutcome {
    Accepted,
    Rejected,
    Unknown,
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::{
        RegistrationState, Store,
        registrations::{blob_column, read_registration},
    };
    use hantong_contracts::{
        audit::{AuditTerminal, AuditTool},
        google_mutation::{EXECUTE_TOOL, REQUEST_TOOL},
        google_write::GoogleAccessMode,
    };
    use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
    fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
        r.map_err(|_| ErrorCode::InternalError)
    }
    fn run<T>(
        s: &Store,
        f: impl FnOnce(&Transaction<'_>) -> Result<T, ErrorCode>,
    ) -> Result<T, ErrorCode> {
        s.with_connection(|c| {
            let tx = Transaction::new_unchecked(c, TransactionBehavior::Immediate)?;
            let result = f(&tx);
            if result.is_ok() {
                tx.commit()?;
            }
            Ok(result)
        })?
    }
    fn unsigned(r: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<u64> {
        u64::try_from(r.get::<_, i64>(i)?).map_err(|_| rusqlite::Error::InvalidQuery)
    }
    fn now() -> Result<i64, ErrorCode> {
        crate::utc_millis().map_err(|_| ErrorCode::InternalError)
    }
    fn grant(
        tx: &Transaction<'_>,
        owner: [u8; 16],
        epoch: u64,
        account: &str,
        tool: &str,
    ) -> Result<(), ErrorCode> {
        let r = db(read_registration(tx, &owner))?.ok_or(ErrorCode::ClientNotRegistered)?;
        if r.state != RegistrationState::Active {
            return Err(ErrorCode::ClientSuspended);
        }
        if r.profile_epoch != epoch {
            return Err(ErrorCode::AuthRequired);
        }
        if r.read_only || r.client_kind != hantong_contracts::client::ClientKind::Cli {
            return Err(ErrorCode::PolicyBlocked);
        }
        if !r.scope.permits_account(account, tool) {
            return Err(ErrorCode::ScopeDenied);
        }
        let b = crate::google::google_binding_in_transaction(tx, account)?;
        if b.access_mode != GoogleAccessMode::Modify {
            return Err(ErrorCode::ScopeDenied);
        }
        let ready: bool = db(tx.query_row(
            "SELECT status='ready' FROM accounts WHERE account_id=?1",
            [account],
            |r| r.get(0),
        ))?;
        if !ready {
            return Err(ErrorCode::PolicyBlocked);
        }
        Ok(())
    }
    fn snapshot(
        tx: &Transaction<'_>,
        owner: [u8; 16],
        r: &MutationPayload,
    ) -> Result<(Option<ComposeSnapshot>, Vec<String>), ErrorCode> {
        match r {
            MutationPayload::SaveDraft(request) => {
                let (d, o, v) = crate::mail_send::draft_in_transaction(tx, *request.draft_id())?;
                if o != owner
                    || v != request.expected_version()
                    || d.account_id() != r.account_id()
                    || d.payload_hash() != r.payload_hash()
                {
                    return Err(ErrorCode::Conflict);
                }
                Ok((Some(d), vec![]))
            }
            MutationPayload::Labels {
                change,
                expected_version,
                payload_hash,
                ..
            } => {
                let version: i64 = db(tx.query_row(
                    "SELECT version FROM google_sync_state WHERE account_id=?1",
                    [r.account_id()],
                    |q| q.get(0),
                ))?;
                if version < 0 || version as u64 != *expected_version {
                    return Err(ErrorCode::Conflict);
                }
                if hantong_platform::sha256_storage_payload(&change.canonical_payload())?
                    != *payload_hash
                {
                    return Err(ErrorCode::InvalidInput);
                }
                let raw:Option<String>=db(tx.query_row("SELECT message_json FROM google_messages WHERE account_id=?1 AND message_id=?2",params![r.account_id(),change.message_id()],|q|q.get(0)).optional())?;
                let v: serde_json::Value = serde_json::from_str(&raw.ok_or(ErrorCode::NotFound)?)
                    .map_err(|_| ErrorCode::InternalError)?;
                let labels: Vec<String> = serde_json::from_value(
                    v.get("label_ids")
                        .cloned()
                        .ok_or(ErrorCode::InternalError)?,
                )
                .map_err(|_| ErrorCode::InternalError)?;
                if labels.len() > 100 {
                    return Err(ErrorCode::QuotaExceeded);
                }
                Ok((None, labels))
            }
        }
    }
    fn receipt(tx: &Transaction<'_>, id: [u8; 32]) -> Result<MutationReceipt, ErrorCode> {
        let (mut state,error,expires):(String,Option<String>,i64)=db(tx.query_row("SELECT state,error_code,expires_at_utc_ms FROM google_mutation_requests WHERE review_id=?1",[id.as_slice()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional())?.ok_or(ErrorCode::NotFound)?;
        let mut error = error
            .map(|s| {
                ErrorCode::ALL
                    .into_iter()
                    .find(|e| e.as_str() == s)
                    .ok_or(ErrorCode::InternalError)
            })
            .transpose()?;
        if state == "pending" && now()? >= expires {
            state = "expired".into();
            error = Some(ErrorCode::ApprovalExpired);
        }
        Ok(MutationReceipt {
            review_id: id,
            state,
            error,
        })
    }
    fn pending(tx: &Transaction<'_>, id: [u8; 32]) -> Result<(PendingMutation, String), ErrorCode> {
        let row=db(tx.query_row("SELECT owner_registration_id,profile_epoch,account_id,account_version,credential_version,request_json,before_labels_json,created_at_utc_ms,expires_at_utc_ms,state FROM google_mutation_requests WHERE review_id=?1",[id.as_slice()],|r|Ok((blob_column::<16>(r,0)?,unsigned(r,1)?,r.get::<_,String>(2)?,unsigned(r,3)?,unsigned(r,4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,i64>(7)?,r.get::<_,i64>(8)?,r.get::<_,String>(9)?))).optional())?.ok_or(ErrorCode::NotFound)?;
        let request = MutationPayload::from_json(
            &serde_json::from_str(&row.5).map_err(|_| ErrorCode::InternalError)?,
        )?;
        if request.account_id() != row.2 {
            return Err(ErrorCode::InternalError);
        }
        let binding = crate::google::google_binding_in_transaction(tx, &row.2)?;
        let (version, sender): (i64, String) = db(tx.query_row(
            "SELECT settings_version,primary_address FROM accounts WHERE account_id=?1",
            [&row.2],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ))?;
        if version < 1 || version as u64 != row.3 || binding.credential_version != row.4 {
            return Err(ErrorCode::Conflict);
        }
        let (compose, before_labels) = snapshot(tx, row.0, &request)?;
        if serde_json::to_string(&before_labels).map_err(|_| ErrorCode::InternalError)? != row.6 {
            return Err(ErrorCode::Conflict);
        }
        Ok((
            PendingMutation {
                review_id: id,
                owner_registration_id: row.0,
                profile_epoch: row.1,
                account_version: version as u64,
                binding,
                sender,
                request,
                compose,
                before_labels,
                created_at_utc_ms: row.7,
                expires_at_utc_ms: row.8,
            },
            row.9,
        ))
    }
    impl Store {
        pub fn request_google_mutation(
            &self,
            owner: [u8; 16],
            epoch: u64,
            r: &MutationPayload,
        ) -> Result<MutationReceipt, ErrorCode> {
            run(self, |tx| {
                if MutationPayload::from_json(&r.to_json())? != *r {
                    return Err(ErrorCode::InvalidInput);
                }
                grant(tx, owner, epoch, r.account_id(), REQUEST_TOOL)?;
                let raw =
                    serde_json::to_string(&r.to_json()).map_err(|_| ErrorCode::InvalidInput)?;
                if raw.len() > 65536 {
                    return Err(ErrorCode::QuotaExceeded);
                }
                let existing=db(tx.prepare("SELECT review_id,profile_epoch,request_json FROM google_mutation_requests WHERE owner_registration_id=?1 AND (operation_id=?2 OR idempotency_key=?3)"))?.query_map(params![owner.as_slice(),r.operation_id(),r.idempotency_key()],|q|Ok((blob_column::<32>(q,0)?,unsigned(q,1)?,q.get::<_,String>(2)?))).map_err(|_|ErrorCode::InternalError)?.collect::<rusqlite::Result<Vec<_>>>().map_err(|_|ErrorCode::InternalError)?;
                if !existing.is_empty() {
                    if existing.len() != 1 || existing[0].1 != epoch || existing[0].2 != raw {
                        return Err(ErrorCode::Conflict);
                    }
                    return receipt(tx, existing[0].0);
                }
                let (_, labels) = snapshot(tx, owner, r)?;
                // A new operation key cannot disguise a repeat of an uncertain write.
                let target = match r {
                    MutationPayload::SaveDraft(d) => {
                        hantong_contracts::wire_hex::encode(d.draft_id())
                    }
                    MutationPayload::Labels { change, .. } => change.message_id().into(),
                };
                let field = match r {
                    MutationPayload::SaveDraft(_) => "$.draft_id",
                    _ => "$.message_id",
                };
                let blocked:bool=db(tx.query_row("SELECT EXISTS(SELECT 1 FROM google_mutation_requests WHERE account_id=?1 AND json_extract(request_json,?2)=?3 AND (state IN ('pending','claimed','unknown') OR (state='accepted' AND ?2='$.draft_id')))",params![r.account_id(),field,target],|q|q.get(0)))?;
                if blocked {
                    return Err(ErrorCode::Conflict);
                }
                let count: i64 = db(tx.query_row(
                    "SELECT count(*) FROM google_mutation_requests",
                    [],
                    |q| q.get(0),
                ))?;
                if count >= 10000 {
                    return Err(ErrorCode::QuotaExceeded);
                }
                let binding = crate::google::google_binding_in_transaction(tx, r.account_id())?;
                let version: i64 = db(tx.query_row(
                    "SELECT settings_version FROM accounts WHERE account_id=?1",
                    [r.account_id()],
                    |q| q.get(0),
                ))?;
                let id = hantong_platform::random_nonce()?;
                let created = now()?;
                let expires = created
                    .checked_add(300000)
                    .ok_or(ErrorCode::InternalError)?;
                db(tx.execute("INSERT INTO google_mutation_requests(review_id,owner_registration_id,profile_epoch,account_id,account_version,credential_version,operation_id,idempotency_key,request_json,before_labels_json,created_at_utc_ms,expires_at_utc_ms,state) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'pending')",params![id.as_slice(),owner.as_slice(),epoch as i64,r.account_id(),version,binding.credential_version as i64,r.operation_id(),r.idempotency_key(),raw,serde_json::to_string(&labels).map_err(|_|ErrorCode::InternalError)?,created,expires]))?;
                receipt(tx, id)
            })
        }
        pub fn google_mutation_receipt(
            &self,
            owner: [u8; 16],
            epoch: u64,
            account: &str,
            id: [u8; 32],
        ) -> Result<MutationReceipt, ErrorCode> {
            run(self, |tx| {
                grant(tx, owner, epoch, account, REQUEST_TOOL)?;
                let matches:bool=db(tx.query_row("SELECT EXISTS(SELECT 1 FROM google_mutation_requests WHERE review_id=?1 AND owner_registration_id=?2 AND account_id=?3 AND profile_epoch=?4)",params![id.as_slice(),owner.as_slice(),account,epoch as i64],|q|q.get(0)))?;
                if !matches {
                    return Err(ErrorCode::NotFound);
                }
                receipt(tx, id)
            })
        }
        pub fn pending_google_mutation(&self, id: [u8; 32]) -> Result<PendingMutation, ErrorCode> {
            run(self, |tx| {
                let (p, state) = pending(tx, id)?;
                if state != "pending" {
                    return Err(ErrorCode::Conflict);
                }
                grant(
                    tx,
                    p.owner_registration_id,
                    p.profile_epoch,
                    p.request.account_id(),
                    EXECUTE_TOOL,
                )?;
                if now()? >= p.expires_at_utc_ms {
                    return Err(ErrorCode::ApprovalExpired);
                }
                Ok(p)
            })
        }
        pub fn claim_google_mutation(
            &self,
            p: &PendingMutation,
            audit: [u8; 32],
        ) -> Result<(), ErrorCode> {
            run(self, |tx| {
                let (current, state) = pending(tx, p.review_id)?;
                if current != *p || state != "pending" {
                    return Err(ErrorCode::Conflict);
                }
                grant(
                    tx,
                    p.owner_registration_id,
                    p.profile_epoch,
                    p.request.account_id(),
                    EXECUTE_TOOL,
                )?;
                if now()? >= p.expires_at_utc_ms {
                    return Err(ErrorCode::ApprovalExpired);
                }
                let a =
                    db(crate::audit::read_audit(tx, &audit))?.ok_or(ErrorCode::InternalError)?;
                if a.registration_id() != &p.owner_registration_id
                    || a.profile_epoch() != p.profile_epoch
                    || a.tool() != AuditTool::GoogleMutationExecute
                    || a.account_ids() != p.request.account_ids()
                    || a.completion().is_some()
                {
                    return Err(ErrorCode::InternalError);
                }
                if db(tx.execute("UPDATE google_mutation_requests SET state='claimed',audit_id=?2 WHERE review_id=?1 AND state='pending'",params![p.review_id.as_slice(),audit.as_slice()]))?!=1{return Err(ErrorCode::Conflict)}
                Ok(())
            })
        }
        pub fn validate_claimed_google_mutation(
            &self,
            p: &PendingMutation,
        ) -> Result<(), ErrorCode> {
            run(self, |tx| {
                let (current, state) = pending(tx, p.review_id)?;
                if current != *p || state != "claimed" {
                    return Err(ErrorCode::Conflict);
                }
                grant(
                    tx,
                    p.owner_registration_id,
                    p.profile_epoch,
                    p.request.account_id(),
                    EXECUTE_TOOL,
                )
            })
        }
        pub fn deny_google_mutation(
            &self,
            id: [u8; 32],
            error: ErrorCode,
        ) -> Result<(), ErrorCode> {
            run(self, |tx| {
                db(tx.execute("UPDATE google_mutation_requests SET state=?2,error_code=?3 WHERE review_id=?1 AND state='pending'",params![id.as_slice(),if error==ErrorCode::ApprovalExpired{"expired"}else{"denied"},error.as_str()]))?;
                Ok(())
            })
        }
        pub fn finish_google_mutation(
            &self,
            id: [u8; 32],
            outcome: MutationOutcome,
        ) -> Result<(), ErrorCode> {
            run(self, |tx| finish(tx, id, outcome))
        }
        pub fn fail_google_mutation_admission(
            &self,
            id: [u8; 32],
            audit: [u8; 32],
            error: ErrorCode,
        ) -> Result<(), ErrorCode> {
            run(self, |tx| {
                let claimed:bool=db(tx.query_row("SELECT EXISTS(SELECT 1 FROM google_mutation_requests WHERE review_id=?1 AND audit_id=?2 AND state='claimed')",params![id.as_slice(),audit.as_slice()],|q|q.get(0)))?;
                if claimed {
                    return finish(tx, id, MutationOutcome::Unknown);
                }
                crate::audit::insert_audit_terminal_tx(
                    tx,
                    audit,
                    AuditTerminal::failed(error),
                    now()?,
                )?;
                db(tx.execute("UPDATE google_mutation_requests SET state='denied',error_code=?2 WHERE review_id=?1 AND state='pending'",params![id.as_slice(),error.as_str()]))?;
                Ok(())
            })
        }
        pub fn recover_google_mutations(&self) -> Result<usize, ErrorCode> {
            run(self, |tx| {
                let ids=db(tx.prepare("SELECT review_id FROM google_mutation_requests WHERE state='claimed' LIMIT 10001"))?.query_map([],|q|blob_column::<32>(q,0)).map_err(|_|ErrorCode::InternalError)?.collect::<rusqlite::Result<Vec<_>>>().map_err(|_|ErrorCode::InternalError)?;
                if ids.len() > 10000 {
                    return Err(ErrorCode::InternalError);
                }
                for id in &ids {
                    finish(tx, *id, MutationOutcome::Unknown)?;
                }
                db(tx.execute("UPDATE google_mutation_requests SET state='denied',error_code='APPROVAL_REQUIRED' WHERE state='pending'",[]))?;
                Ok(ids.len())
            })
        }
    }
    fn finish(
        tx: &Transaction<'_>,
        id: [u8; 32],
        outcome: MutationOutcome,
    ) -> Result<(), ErrorCode> {
        let (audit,raw):(Vec<u8>,String)=db(tx.query_row("SELECT audit_id,request_json FROM google_mutation_requests WHERE review_id=?1 AND state='claimed'",[id.as_slice()],|q|Ok((q.get(0)?,q.get(1)?))).optional())?.ok_or(ErrorCode::Conflict)?;
        let audit: [u8; 32] = audit.try_into().map_err(|_| ErrorCode::InternalError)?;
        let (state, error) = match outcome {
            MutationOutcome::Accepted => ("accepted", None),
            MutationOutcome::Rejected => ("rejected", Some(ErrorCode::PolicyBlocked)),
            MutationOutcome::Unknown => ("unknown", Some(ErrorCode::SendUnknown)),
        };
        db(tx.execute("UPDATE google_mutation_requests SET state=?2,error_code=?3 WHERE review_id=?1 AND state='claimed'",params![id.as_slice(),state,error.map(ErrorCode::as_str)]))?;
        if outcome == MutationOutcome::Accepted
            && let MutationPayload::Labels { change, .. } = MutationPayload::from_json(
                &serde_json::from_str(&raw).map_err(|_| ErrorCode::InternalError)?,
            )?
        {
            // Invalidate only local cached labels; fetch the provider state before the next change.
            db(tx.execute(
                "DELETE FROM google_messages WHERE account_id=?1 AND message_id=?2",
                params![change.account_id(), change.message_id()],
            ))?;
            db(tx.execute(
                "UPDATE google_sync_state SET version=version+1 WHERE account_id=?1",
                [change.account_id()],
            ))?;
        }
        let terminal = match error {
            Some(e) => AuditTerminal::failed(e),
            None => AuditTerminal::succeeded(Some(1))?,
        };
        crate::audit::insert_audit_terminal_tx(tx, audit, terminal, now()?)?;
        Ok(())
    }
}
