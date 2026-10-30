//! Immutable drafts and native-review-bound, single-attempt SMTP ledger.
use hantong_contracts::{
    ErrorCode,
    mail_send::{DraftCreateRequest, SendReviewRequest},
};
use hantong_domain::mail_connection::SmtpEndpointConfig;

#[derive(Clone)]
pub struct PendingSend {
    pub review_id: [u8; 32],
    pub owner_registration_id: [u8; 16],
    pub profile_epoch: u64,
    pub account_version: u64,
    pub endpoint: SmtpEndpointConfig,
    pub draft: DraftCreateRequest,
    pub request: SendReviewRequest,
    pub created_at_utc_ms: i64,
    pub expires_at_utc_ms: i64,
}
impl std::fmt::Debug for PendingSend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PendingSend(<redacted>)")
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendReceipt {
    pub account_id: String,
    pub review_id: [u8; 32],
    pub state: &'static str,
    pub error: Option<ErrorCode>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendOutcome {
    Sent,
    Failed,
    Unknown,
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::{
        RegistrationState, Store,
        registrations::{blob_column, read_registration},
    };
    use hantong_contracts::audit::{AuditTerminal, AuditTool};
    use hantong_domain::mail_connection::MailConnectionConfig;
    use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

    fn text_column(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<String> {
        Ok(crate::registrations::text_column(row, index, 128)?.to_owned())
    }
    fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
        r.map_err(|_| ErrorCode::InternalError)
    }
    fn transaction<T>(
        store: &Store,
        f: impl FnOnce(&Transaction<'_>) -> Result<T, ErrorCode>,
    ) -> Result<T, ErrorCode> {
        store.with_connection(|c| {
            let tx = Transaction::new_unchecked(c, TransactionBehavior::Immediate)?;
            let r = f(&tx);
            if r.is_ok() {
                tx.commit()?;
            }
            Ok(r)
        })?
    }
    fn grant(
        tx: &Transaction<'_>,
        owner: [u8; 16],
        epoch: u64,
        account: &str,
        tool: &str,
    ) -> Result<crate::RegistrationRecord, ErrorCode> {
        let r = db(read_registration(tx, &owner))?.ok_or(ErrorCode::ClientNotRegistered)?;
        if r.state != RegistrationState::Active {
            return Err(ErrorCode::ClientSuspended);
        }
        if r.profile_epoch != epoch {
            return Err(ErrorCode::AuthRequired);
        }
        if r.read_only {
            return Err(ErrorCode::PolicyBlocked);
        }
        if !r.scope.permits_account(account, tool) {
            return Err(ErrorCode::ScopeDenied);
        }
        crate::mail_connection::check_account(tx, account)?;
        let status: String = db(tx.query_row(
            "SELECT status FROM accounts WHERE account_id=?1",
            [account],
            |r| r.get(0),
        ))?;
        if status != "ready" {
            return Err(ErrorCode::PolicyBlocked);
        }
        Ok(r)
    }
    fn endpoint(
        tx: &Transaction<'_>,
        account: &str,
    ) -> Result<(u64, SmtpEndpointConfig), ErrorCode> {
        crate::mail_connection::check_account(tx, account)?;
        let version: i64 = db(tx.query_row(
            "SELECT settings_version FROM accounts WHERE account_id=?1",
            [account],
            |r| r.get(0),
        ))?;
        if version <= 0 {
            return Err(ErrorCode::InternalError);
        }
        let imap = crate::mail_connection::read_endpoint(
            tx,
            account,
            crate::mail_connection::MailEndpointKind::Imap,
        )?
        .ok_or(ErrorCode::NotFound)?;
        let smtp = crate::mail_connection::read_endpoint(
            tx,
            account,
            crate::mail_connection::MailEndpointKind::Smtp,
        )?
        .ok_or(ErrorCode::NotFound)?;
        let bytes=serde_json::to_vec(&serde_json::json!({"schema_version":1,"account_id":account,"imap":imap.json,"smtp":smtp.json})).map_err(|_|ErrorCode::InternalError)?;
        let c = MailConnectionConfig::from_json(&bytes).map_err(|_| ErrorCode::InternalError)?;
        Ok((
            version as u64,
            c.smtp().cloned().ok_or(ErrorCode::InternalError)?,
        ))
    }
    fn draft(
        tx: &Transaction<'_>,
        id: [u8; 32],
    ) -> Result<(DraftCreateRequest, [u8; 16], u64), ErrorCode> {
        let row=db(tx.query_row("SELECT account_id,owner_registration_id,version,operation_id,idempotency_key,recipients_json,subject_text,body_text,payload_hash FROM mail_drafts WHERE draft_id=?1",[id.as_slice()],|r|Ok((text_column(r,0)?,blob_column::<16>(r,1)?,r.get::<_,i64>(2)?,text_column(r,3)?,text_column(r,4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,blob_column::<32>(r,8)?))).optional())?.ok_or(ErrorCode::NotFound)?;
        if row.2 != 1 || row.5.len() > 65536 || row.6.len() > 998 || row.7.len() > 1048576 {
            return Err(ErrorCode::InternalError);
        }
        let recipients = serde_json::from_str(&row.5).map_err(|_| ErrorCode::InternalError)?;
        let value =
            DraftCreateRequest::new(&row.0, recipients, row.6, row.7, &row.3, &row.4, 0, row.8)
                .map_err(|_| ErrorCode::InternalError)?;
        if hantong_platform::sha256_storage_payload(&value.canonical_payload())?
            != *value.payload_hash()
        {
            return Err(ErrorCode::InternalError);
        }
        Ok((value, row.1, row.2 as u64))
    }
    fn pending(
        tx: &Transaction<'_>,
        id: [u8; 32],
    ) -> Result<(PendingSend, String, Option<ErrorCode>), ErrorCode> {
        let r=db(tx.query_row("SELECT draft_id,owner_registration_id,draft_version,owner_profile_epoch,account_version,smtp_endpoint_version,payload_hash,operation_id,idempotency_key,created_at_utc_ms,expires_at_utc_ms,state,error_code FROM mail_send_reviews WHERE review_id=?1",[id.as_slice()],|r|Ok((blob_column::<32>(r,0)?,blob_column::<16>(r,1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?,blob_column::<32>(r,6)?,text_column(r,7)?,text_column(r,8)?,r.get::<_,i64>(9)?,r.get::<_,i64>(10)?,text_column(r,11)?,r.get::<_,Option<String>>(12)?))).optional())?.ok_or(ErrorCode::NotFound)?;
        if id == [0; 32]
            || r.2 <= 0
            || r.3 <= 0
            || r.4 <= 0
            || r.5 <= 0
            || r.9 <= 0
            || r.10 <= r.9
            || r.10 - r.9 > 300000
        {
            return Err(ErrorCode::InternalError);
        }
        let (d, owner, v) = draft(tx, r.0)?;
        if owner != r.1 || v != r.2 as u64 || d.payload_hash() != &r.6 {
            return Err(ErrorCode::InternalError);
        }
        let req = SendReviewRequest::new(d.account_id(), r.0, v, &r.7, &r.8, r.6)
            .map_err(|_| ErrorCode::InternalError)?;
        let error =
            r.12.map(|v| {
                ErrorCode::ALL
                    .into_iter()
                    .find(|e| e.as_str() == v)
                    .ok_or(ErrorCode::InternalError)
            })
            .transpose()?;
        let (current_version, e) = endpoint(tx, d.account_id())?;
        // The current endpoint is never substituted into an old approval.
        if current_version != r.4 as u64 || e.settings_version() != r.5 as u64 {
            return Err(ErrorCode::Conflict);
        }
        Ok((
            PendingSend {
                review_id: id,
                owner_registration_id: owner,
                profile_epoch: r.3 as u64,
                account_version: r.4 as u64,
                endpoint: e,
                draft: d,
                request: req,
                created_at_utc_ms: r.9,
                expires_at_utc_ms: r.10,
            },
            r.11,
            error,
        ))
    }
    fn receipt(
        tx: &Transaction<'_>,
        id: [u8; 32],
        account: String,
        state: &str,
        error: Option<ErrorCode>,
        expires: i64,
    ) -> Result<SendReceipt, ErrorCode> {
        let out = db(tx
            .query_row(
                "SELECT state,error_code FROM mail_outbox WHERE review_id=?1",
                [id.as_slice()],
                |r| Ok((text_column(r, 0)?, r.get::<_, Option<String>>(1)?)),
            )
            .optional())?;
        let (state, error) = match out {
            Some((s, e)) => {
                let state = match s.as_str() {
                    "sending" => "sending",
                    "sent" => "sent",
                    "failed" => "failed",
                    "unknown" => "unknown",
                    _ => return Err(ErrorCode::InternalError),
                };
                let e = e
                    .map(|v| {
                        ErrorCode::ALL
                            .into_iter()
                            .find(|e| e.as_str() == v)
                            .ok_or(ErrorCode::InternalError)
                    })
                    .transpose()?;
                (state, e)
            }
            None => match state {
                "pending"
                    if crate::utc_millis().map_err(|_| ErrorCode::InternalError)? >= expires =>
                {
                    ("expired", Some(ErrorCode::ApprovalExpired))
                }
                "pending" => ("pending", None),
                "denied" => match error {
                    Some(ErrorCode::ApprovalDenied) => ("denied", error),
                    Some(ErrorCode::ApprovalRequired) => ("interrupted", error),
                    Some(_) => ("failed", error),
                    None => return Err(ErrorCode::InternalError),
                },
                "expired" => ("expired", Some(ErrorCode::ApprovalExpired)),
                _ => return Err(ErrorCode::InternalError),
            },
        };
        Ok(SendReceipt {
            account_id: account,
            review_id: id,
            state,
            error,
        })
    }
    impl Store {
        pub fn create_mail_draft(
            &self,
            owner: [u8; 16],
            epoch: u64,
            r: &DraftCreateRequest,
        ) -> Result<[u8; 32], ErrorCode> {
            transaction(self, |tx| {
                grant(tx, owner, epoch, r.account_id(), "mail.drafts.create")?;
                if hantong_platform::sha256_storage_payload(&r.canonical_payload())?
                    != *r.payload_hash()
                {
                    return Err(ErrorCode::InvalidInput);
                }
                let existing=db(tx.prepare("SELECT draft_id FROM mail_drafts WHERE account_id=?1 AND (operation_id=?2 OR idempotency_key=?3)"))?.query_map(params![r.account_id(),r.operation_id(),r.idempotency_key()],|row|blob_column::<32>(row,0)).map_err(|_|ErrorCode::InternalError)?.collect::<rusqlite::Result<Vec<_>>>().map_err(|_|ErrorCode::InternalError)?;
                if !existing.is_empty() {
                    if existing.len() != 1 {
                        return Err(ErrorCode::Conflict);
                    }
                    let (d, o, _) = draft(tx, existing[0])?;
                    return if o == owner && d == *r {
                        Ok(existing[0])
                    } else {
                        Err(ErrorCode::Conflict)
                    };
                }
                let count: i64 =
                    db(tx.query_row("SELECT count(*) FROM mail_drafts", [], |r| r.get(0)))?;
                if count >= 10000 {
                    return Err(ErrorCode::QuotaExceeded);
                }
                let id = hantong_platform::random_nonce()?;
                let now = crate::utc_millis().map_err(|_| ErrorCode::InternalError)?;
                db(tx.execute("INSERT INTO mail_drafts(draft_id,account_id,owner_registration_id,version,operation_id,idempotency_key,recipients_json,subject_text,body_text,payload_hash,created_at_utc_ms) VALUES(?1,?2,?3,1,?4,?5,?6,?7,?8,?9,?10)",params![id.as_slice(),r.account_id(),owner.as_slice(),r.operation_id(),r.idempotency_key(),serde_json::to_string(r.recipients()).map_err(|_|ErrorCode::InternalError)?,r.subject(),r.body(),r.payload_hash().as_slice(),now]))?;
                Ok(id)
            })
        }
        pub fn request_mail_send_review(
            &self,
            owner: [u8; 16],
            epoch: u64,
            r: &SendReviewRequest,
        ) -> Result<SendReceipt, ErrorCode> {
            transaction(self, |tx| {
                grant(tx, owner, epoch, r.account_id(), "mail.send.request")?;
                let existing=db(tx.prepare("SELECT review_id FROM mail_send_reviews WHERE owner_registration_id=?1 AND (operation_id=?2 OR idempotency_key=?3)"))?.query_map(params![owner.as_slice(),r.operation_id(),r.idempotency_key()],|row|blob_column::<32>(row,0)).map_err(|_|ErrorCode::InternalError)?.collect::<rusqlite::Result<Vec<_>>>().map_err(|_|ErrorCode::InternalError)?;
                if !existing.is_empty() {
                    if existing.len() != 1 {
                        return Err(ErrorCode::Conflict);
                    }
                    let id = existing[0];
                    let row=db(tx.query_row("SELECT r.draft_id,r.draft_version,r.payload_hash,r.owner_profile_epoch,r.operation_id,r.idempotency_key,d.account_id,r.state,r.error_code,r.expires_at_utc_ms FROM mail_send_reviews r JOIN mail_drafts d USING(draft_id) WHERE r.review_id=?1",[id.as_slice()],|q|Ok((blob_column::<32>(q,0)?,q.get::<_,i64>(1)?,blob_column::<32>(q,2)?,q.get::<_,i64>(3)?,text_column(q,4)?,text_column(q,5)?,text_column(q,6)?,text_column(q,7)?,q.get::<_,Option<String>>(8)?,q.get::<_,i64>(9)?))))?;
                    if row.0 != *r.draft_id()
                        || row.1 != r.expected_version() as i64
                        || row.2 != *r.payload_hash()
                        || row.3 != epoch as i64
                        || row.4 != r.operation_id()
                        || row.5 != r.idempotency_key()
                        || row.6 != r.account_id()
                    {
                        return Err(ErrorCode::Conflict);
                    }
                    let error = row
                        .8
                        .map(|v| {
                            ErrorCode::ALL
                                .into_iter()
                                .find(|e| e.as_str() == v)
                                .ok_or(ErrorCode::InternalError)
                        })
                        .transpose()?;
                    return receipt(tx, id, r.account_id().into(), &row.7, error, row.9);
                }
                let (d, o, v) = draft(tx, *r.draft_id())?;
                if o != owner
                    || d.account_id() != r.account_id()
                    || v != r.expected_version()
                    || d.payload_hash() != r.payload_hash()
                {
                    return Err(ErrorCode::Conflict);
                }
                let (account_version, endpoint) = endpoint(tx, r.account_id())?;
                let count: i64 =
                    db(tx.query_row("SELECT count(*) FROM mail_send_reviews", [], |r| r.get(0)))?;
                if count >= 10000 {
                    return Err(ErrorCode::QuotaExceeded);
                }
                let id = hantong_platform::random_nonce()?;
                let now = crate::utc_millis().map_err(|_| ErrorCode::InternalError)?;
                let expires = now.checked_add(300000).ok_or(ErrorCode::InternalError)?;
                db(tx.execute("INSERT INTO mail_send_reviews(review_id,draft_id,owner_registration_id,draft_version,owner_profile_epoch,account_version,smtp_endpoint_version,payload_hash,operation_id,idempotency_key,created_at_utc_ms,expires_at_utc_ms,state,error_code) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'pending',NULL)",params![id.as_slice(),r.draft_id().as_slice(),owner.as_slice(),v as i64,epoch as i64,account_version as i64,endpoint.settings_version() as i64,r.payload_hash().as_slice(),r.operation_id(),r.idempotency_key(),now,expires]))?;
                Ok(SendReceipt {
                    account_id: r.account_id().into(),
                    review_id: id,
                    state: "pending",
                    error: None,
                })
            })
        }
        pub fn mail_send_receipt(
            &self,
            owner: [u8; 16],
            epoch: u64,
            account: &str,
            id: [u8; 32],
        ) -> Result<SendReceipt, ErrorCode> {
            transaction(self, |tx| {
                grant(tx, owner, epoch, account, "mail.send.request")?;
                // Status survives endpoint changes; no provider payload is reconstructed.
                let row=db(tx.query_row("SELECT d.account_id,r.owner_registration_id,r.owner_profile_epoch,r.state,r.error_code,r.expires_at_utc_ms FROM mail_send_reviews r JOIN mail_drafts d USING(draft_id) WHERE r.review_id=?1",[id.as_slice()],|r|Ok((text_column(r,0)?,blob_column::<16>(r,1)?,r.get::<_,i64>(2)?,text_column(r,3)?,r.get::<_,Option<String>>(4)?,r.get::<_,i64>(5)?))).optional())?.ok_or(ErrorCode::NotFound)?;
                if row.0 != account || row.1 != owner {
                    return Err(ErrorCode::NotFound);
                }
                if row.2 != epoch as i64 {
                    return Err(ErrorCode::AuthRequired);
                }
                let error = row
                    .4
                    .map(|v| {
                        ErrorCode::ALL
                            .into_iter()
                            .find(|e| e.as_str() == v)
                            .ok_or(ErrorCode::InternalError)
                    })
                    .transpose()?;
                receipt(tx, id, account.into(), &row.3, error, row.5)
            })
        }
        pub fn pending_mail_send(&self, id: [u8; 32]) -> Result<PendingSend, ErrorCode> {
            transaction(self, |tx| {
                let (p, s, _) = pending(tx, id)?;
                if s != "pending" {
                    return Err(ErrorCode::Conflict);
                }
                grant(
                    tx,
                    p.owner_registration_id,
                    p.profile_epoch,
                    p.request.account_id(),
                    "mail.send.execute",
                )?;
                if crate::utc_millis().map_err(|_| ErrorCode::InternalError)? >= p.expires_at_utc_ms
                {
                    return Err(ErrorCode::ApprovalExpired);
                }
                Ok(p)
            })
        }
        /// Native coordinator only: consumes an existing pending review in the same
        /// transaction that commits the send claim. This operation issues no approval.
        pub fn claim_mail_send(
            &self,
            p: &PendingSend,
            audit_id: [u8; 32],
        ) -> Result<(), ErrorCode> {
            transaction(self, |tx| {
                let (current, state, _) = pending(tx, p.review_id)?;
                if state != "pending"
                    || current.owner_registration_id != p.owner_registration_id
                    || current.profile_epoch != p.profile_epoch
                    || current.request != p.request
                    || current.draft != p.draft
                    || current.endpoint != p.endpoint
                    || current.account_version != p.account_version
                {
                    return Err(ErrorCode::Conflict);
                }
                grant(
                    tx,
                    p.owner_registration_id,
                    p.profile_epoch,
                    p.request.account_id(),
                    "mail.send.execute",
                )?;
                let now = crate::utc_millis().map_err(|_| ErrorCode::InternalError)?;
                if now >= p.expires_at_utc_ms {
                    return Err(ErrorCode::ApprovalExpired);
                }
                let audit =
                    db(crate::audit::read_audit(tx, &audit_id))?.ok_or(ErrorCode::InternalError)?;
                if audit.registration_id() != &p.owner_registration_id
                    || audit.profile_epoch() != p.profile_epoch
                    || audit.tool() != AuditTool::SendExecute
                    || audit.account_ids() != p.request.account_ids()
                    || audit.completion().is_some()
                {
                    return Err(ErrorCode::InternalError);
                }
                let occupied:bool=db(tx.query_row("SELECT EXISTS(SELECT 1 FROM mail_outbox WHERE account_id=?1 AND (operation_id=?2 OR idempotency_key=?3))",params![p.request.account_id(),p.request.operation_id(),p.request.idempotency_key()],|r|r.get(0)))?;
                if occupied {
                    return Err(ErrorCode::Conflict);
                }
                if db(tx.execute("UPDATE mail_send_reviews SET state='consumed' WHERE review_id=?1 AND state='pending'",[p.review_id.as_slice()]))?!=1{return Err(ErrorCode::Conflict)}
                if db(tx.execute("INSERT INTO mail_outbox(account_id,operation_id,idempotency_key,review_id,draft_id,owner_registration_id,payload_hash,state,created_at_utc_ms,completed_at_utc_ms,audit_id,error_code) VALUES(?1,?2,?3,?4,?5,?6,?7,'sending',?8,NULL,?9,NULL)",params![p.request.account_id(),p.request.operation_id(),p.request.idempotency_key(),p.review_id.as_slice(),p.request.draft_id().as_slice(),p.owner_registration_id.as_slice(),p.request.payload_hash().as_slice(),now,audit_id.as_slice()]))?!=1{return Err(ErrorCode::InternalError)}
                Ok(())
            })
        }
        pub fn deny_mail_send(&self, id: [u8; 32], error: ErrorCode) -> Result<(), ErrorCode> {
            transaction(self, |tx| {
                let state = if error == ErrorCode::ApprovalExpired {
                    "expired"
                } else {
                    "denied"
                };
                db(tx.execute("UPDATE mail_send_reviews SET state=?2,error_code=?3 WHERE review_id=?1 AND state='pending'",params![id.as_slice(),state,error.as_str()]))?;
                Ok(())
            })
        }
        pub fn validate_claimed_mail_send(&self, p: &PendingSend) -> Result<(), ErrorCode> {
            transaction(self, |tx| {
                let (current, state, _) = pending(tx, p.review_id)?;
                if state != "consumed"
                    || current.owner_registration_id != p.owner_registration_id
                    || current.profile_epoch != p.profile_epoch
                    || current.endpoint != p.endpoint
                    || current.request != p.request
                    || current.account_version != p.account_version
                {
                    return Err(ErrorCode::Conflict);
                }
                grant(
                    tx,
                    p.owner_registration_id,
                    p.profile_epoch,
                    p.request.account_id(),
                    "mail.send.execute",
                )?;
                let active:bool=db(tx.query_row("SELECT EXISTS(SELECT 1 FROM mail_outbox WHERE review_id=?1 AND state='sending')",[p.review_id.as_slice()],|r|r.get(0)))?;
                if !active {
                    return Err(ErrorCode::Conflict);
                }
                Ok(())
            })
        }
        pub fn finish_mail_send(
            &self,
            id: [u8; 32],
            outcome: SendOutcome,
        ) -> Result<(), ErrorCode> {
            transaction(self, |tx| finish(tx, id, outcome))
        }
        /// A claim error may follow a committed write. Resolve that claim as
        /// unknown with its audit in the same transaction, without SMTP.
        pub fn fail_mail_send_admission(
            &self,
            id: [u8; 32],
            audit_id: [u8; 32],
            error: ErrorCode,
        ) -> Result<(), ErrorCode> {
            transaction(self, |tx| {
                let claimed = db(tx
                    .query_row(
                        "SELECT audit_id FROM mail_outbox WHERE review_id=?1",
                        [id.as_slice()],
                        |r| blob_column::<32>(r, 0),
                    )
                    .optional())?;
                if claimed == Some(audit_id) {
                    return finish(tx, id, SendOutcome::Unknown);
                }
                let now = crate::utc_millis().map_err(|_| ErrorCode::InternalError)?;
                crate::audit::insert_audit_terminal_tx(
                    tx,
                    audit_id,
                    AuditTerminal::failed(error),
                    now,
                )?;
                let state = if error == ErrorCode::ApprovalExpired {
                    "expired"
                } else {
                    "denied"
                };
                db(tx.execute("UPDATE mail_send_reviews SET state=?2,error_code=?3 WHERE review_id=?1 AND state='pending'",params![id.as_slice(),state,error.as_str()]))?;
                Ok(())
            })
        }
        /// Crashed SMTP attempts become unknown without any network operation.
        pub fn recover_mail_sends(&self) -> Result<usize, ErrorCode> {
            transaction(self, |tx| {
                let ids = db(tx.prepare(
                    "SELECT review_id FROM mail_outbox WHERE state='sending' LIMIT 10001",
                ))?
                .query_map([], |r| blob_column::<32>(r, 0))
                .map_err(|_| ErrorCode::InternalError)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|_| ErrorCode::InternalError)?;
                if ids.len() > 10000 {
                    return Err(ErrorCode::InternalError);
                }
                for id in &ids {
                    finish(tx, *id, SendOutcome::Unknown)?;
                }
                Ok(ids.len())
            })
        }
    }
    fn finish(tx: &Transaction<'_>, id: [u8; 32], outcome: SendOutcome) -> Result<(), ErrorCode> {
        let row = db(tx
            .query_row(
                "SELECT state,audit_id FROM mail_outbox WHERE review_id=?1",
                [id.as_slice()],
                |r| Ok((text_column(r, 0)?, blob_column::<32>(r, 1)?)),
            )
            .optional())?
        .ok_or(ErrorCode::NotFound)?;
        if row.0 != "sending" {
            return Err(ErrorCode::Conflict);
        }
        let (state, error, terminal) = match outcome {
            SendOutcome::Sent => ("sent", None, AuditTerminal::succeeded(Some(1))?),
            SendOutcome::Failed => (
                "failed",
                Some(ErrorCode::PolicyBlocked),
                AuditTerminal::failed(ErrorCode::PolicyBlocked),
            ),
            SendOutcome::Unknown => (
                "unknown",
                Some(ErrorCode::SendUnknown),
                AuditTerminal::failed(ErrorCode::SendUnknown),
            ),
        };
        let now = crate::utc_millis().map_err(|_| ErrorCode::InternalError)?;
        if db(tx.execute("UPDATE mail_outbox SET state=?2,completed_at_utc_ms=?3,error_code=?4 WHERE review_id=?1 AND state='sending'",params![id.as_slice(),state,now,error.map(ErrorCode::as_str)]))?!=1{return Err(ErrorCode::Conflict)}
        crate::audit::insert_audit_terminal_tx(tx, row.1, terminal, now)?;
        Ok(())
    }
}
