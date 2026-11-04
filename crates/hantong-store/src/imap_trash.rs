//! Durable, provider-free IMAP trash admission and single-attempt ledger.
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
use crate::Store;
use hantong_contracts::{ErrorCode, imap_trash::ImapTrashRequest};
use hantong_domain::mail_connection::ImapEndpointConfig;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use hantong_domain::{
    mail_connection::MailConnectionConfig,
    mailboxes::{ImapMailboxSnapshot, SpecialUseRole},
};
use std::fmt;

#[derive(Clone, Eq, PartialEq)]
pub struct PendingImapTrash {
    pub review_id: [u8; 32],
    pub owner_registration_id: [u8; 16],
    pub profile_epoch: u64,
    pub account_version: u64,
    pub endpoint: ImapEndpointConfig,
    pub request: ImapTrashRequest,
    pub destination: ImapTrashDestination,
    pub created_at_utc_ms: i64,
    pub expires_at_utc_ms: i64,
}
impl fmt::Debug for PendingImapTrash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PendingImapTrash(<redacted>)")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapTrashDestination {
    pub mailbox_wire_hex: String,
    pub mailbox_encoding: String,
}
impl fmt::Debug for ImapTrashDestination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapTrashDestination(<redacted>)")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImapTrashReceipt {
    pub account_id: String,
    pub review_id: [u8; 32],
    pub state: &'static str,
    pub error: Option<ErrorCode>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapTrashOutcome {
    Moved,
    Failed,
    Unknown,
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod windows {
    use super::*;
    use crate::{
        ClientKind, RegistrationState, Store,
        registrations::{blob_column, read_registration, text_column},
    };
    use hantong_contracts::audit::{AuditBegin, AuditTerminal, AuditTool};
    use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

    fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
        r.map_err(|_| ErrorCode::InternalError)
    }
    fn text(r: &rusqlite::Row<'_>, i: usize, max: usize) -> rusqlite::Result<String> {
        text_column(r, i, max).map(ToOwned::to_owned)
    }
    fn tx<T>(
        store: &Store,
        f: impl FnOnce(&Transaction<'_>) -> Result<T, ErrorCode>,
    ) -> Result<T, ErrorCode> {
        store.with_connection(|c| {
            let t = Transaction::new_unchecked(c, TransactionBehavior::Immediate)?;
            let out = f(&t);
            if out.is_ok() {
                t.commit()?;
            }
            Ok(out)
        })?
    }
    fn endpoint(
        tx: &Transaction<'_>,
        account: &str,
    ) -> Result<(u64, ImapEndpointConfig), ErrorCode> {
        let version: i64 = db(tx.query_row("SELECT settings_version FROM accounts WHERE account_id=?1 AND provider='imap' AND status='ready'", [account], |r| r.get(0)).optional())?.ok_or(ErrorCode::PolicyBlocked)?;
        let row = crate::mail_connection::read_endpoint(
            tx,
            account,
            crate::mail_connection::MailEndpointKind::Imap,
        )?
        .ok_or(ErrorCode::NotFound)?;
        let json = serde_json::json!({"schema_version":1,"account_id":account,"imap":row.json});
        let bytes = serde_json::to_vec(&json).map_err(|_| ErrorCode::InternalError)?;
        let config =
            MailConnectionConfig::from_json(&bytes).map_err(|_| ErrorCode::InternalError)?;
        Ok((
            u64::try_from(version).map_err(|_| ErrorCode::InternalError)?,
            config.imap().clone(),
        ))
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
        let ai = r.client_kind == ClientKind::Ai;
        if r.read_only || (r.client_kind != ClientKind::Cli && !ai) {
            return Err(ErrorCode::PolicyBlocked);
        }
        if !r.scope.permits_account(account, tool) {
            return Err(ErrorCode::ScopeDenied);
        }
        if ai {
            let provider = match r.display_label.as_str() {
                crate::DEFAULT_AI_CODEX_LABEL => "codex",
                crate::DEFAULT_AI_CLAUDE_LABEL => "claude",
                _ => return Err(ErrorCode::PolicyBlocked),
            };
            let approved: i64 = db(tx.query_row(
                "SELECT approved_at_utc_ms FROM client_registrations WHERE registration_id=?1",
                [owner.as_slice()],
                |q| q.get(0),
            ))?;
            let ok: bool = db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM ai_consents WHERE provider=?1 AND registration_id=?2 AND account_id=?3 AND profile_epoch=?4 AND registration_request_id=?5 AND approved_at_utc_ms=?6)",
                rusqlite::params![provider, owner.as_slice(), account, epoch as i64, r.request_id.as_slice(), approved],
                |q| q.get(0),
            ))?;
            if !ok {
                return Err(ErrorCode::PolicyBlocked);
            }
        }
        Ok(())
    }
    fn unique_trash(
        snapshot: &ImapMailboxSnapshot,
    ) -> Result<&hantong_domain::mailboxes::ImapMailboxDescriptor, ErrorCode> {
        let trash_mailboxes: Vec<_> = snapshot
            .mailboxes()
            .iter()
            .filter(|m| m.selectable() && m.has_role(SpecialUseRole::Trash))
            .collect();
        let [item] = trash_mailboxes.as_slice() else {
            return Err(ErrorCode::CapabilityUnsupported);
        };
        Ok(item)
    }
    fn destination(
        tx: &Transaction<'_>,
        r: &ImapTrashRequest,
    ) -> Result<ImapTrashDestination, ErrorCode> {
        let cached: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM imap_uid_messages WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4 AND uid=?5)",
            params![r.account_id(), r.mailbox().wire_hex(), r.mailbox().encoding().as_str(), r.uidvalidity() as i64, r.uid() as i64],
            |q| q.get(0),
        ))?;
        if !cached {
            return Err(ErrorCode::Conflict);
        }
        let row: String=db(tx.query_row("SELECT snapshot_json FROM imap_mailbox_catalogs WHERE account_id=?1 AND generation=?2",params![r.account_id(),r.catalog_generation() as i64],|q|q.get(0)).optional())?.ok_or(ErrorCode::Conflict)?;
        let snapshot = ImapMailboxSnapshot::from_storage_json(row.as_bytes())
            .map_err(|_| ErrorCode::InternalError)?;
        if snapshot.account_id() != r.account_id() || !snapshot.capabilities().move_messages() {
            return Err(ErrorCode::CapabilityUnsupported);
        }
        let item = unique_trash(&snapshot)?;
        Ok(ImapTrashDestination {
            mailbox_wire_hex: hantong_contracts::wire_hex::encode(item.name().wire_name()),
            mailbox_encoding: match item.name().encoding() {
                hantong_domain::mailboxes::MailboxEncoding::Utf8 => "utf8",
                hantong_domain::mailboxes::MailboxEncoding::ModifiedUtf7 => "modified_utf7",
            }
            .into(),
        })
    }
    fn current_binding(tx: &Transaction<'_>, p: &PendingImapTrash) -> Result<bool, ErrorCode> {
        let account_version: i64 = db(tx.query_row(
            "SELECT settings_version FROM accounts WHERE account_id=?1 AND provider='imap' AND status='ready'",
            [p.request.account_id()],
            |q| q.get(0),
        ))?;
        let endpoint_version: i64 = db(tx.query_row(
            "SELECT settings_version FROM mail_connection_settings WHERE account_id=?1 AND endpoint_kind='imap'",
            [p.request.account_id()],
            |q| q.get(0),
        ))?;
        let generation: i64 = db(tx.query_row(
            "SELECT generation FROM imap_mailbox_catalogs WHERE account_id=?1",
            [p.request.account_id()],
            |q| q.get(0),
        ))?;
        let source_cached: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM imap_uid_messages WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4 AND uid=?5)",
            params![p.request.account_id(), p.request.mailbox().wire_hex(), p.request.mailbox().encoding().as_str(), p.request.uidvalidity() as i64, p.request.uid() as i64],
            |q| q.get(0),
        ))?;
        Ok(account_version == p.account_version as i64
            && endpoint_version == p.endpoint.settings_version() as i64
            && generation == p.request.catalog_generation() as i64
            && source_cached)
    }
    fn pending(
        tx: &Transaction<'_>,
        id: [u8; 32],
    ) -> Result<(PendingImapTrash, String), ErrorCode> {
        let row=db(tx.query_row("SELECT owner_registration_id,profile_epoch,account_id,account_version,endpoint_json,source_mailbox_key,source_mailbox_encoding,catalog_generation,uid_validity,uid,operation_id,idempotency_key,intent_hash,destination_mailbox_key,destination_mailbox_encoding,created_at_utc_ms,expires_at_utc_ms,state FROM imap_trash_reviews WHERE review_id=?1",[id.as_slice()],|r|Ok((blob_column::<16>(r,0)?,r.get::<_,i64>(1)?,text(r,2,128)?,r.get::<_,i64>(3)?,text(r,4,16*1024)?,text(r,5,2048)?,text(r,6,32)?,r.get::<_,i64>(7)?,r.get::<_,i64>(8)?,r.get::<_,i64>(9)?,text(r,10,128)?,text(r,11,128)?,blob_column::<32>(r,12)?,text(r,13,2048)?,text(r,14,32)?,r.get::<_,i64>(15)?,r.get::<_,i64>(16)?,text(r,17,16)?))).optional())?.ok_or(ErrorCode::NotFound)?;
        let (
            owner,
            epoch,
            account,
            av,
            endpoint_json,
            source_key,
            source_encoding,
            generation,
            validity,
            uid,
            op,
            idem,
            hash,
            dest_key,
            dest_encoding,
            created,
            expires,
            state,
        ) = row;
        let cfg = serde_json::from_str::<serde_json::Value>(&endpoint_json)
            .map_err(|_| ErrorCode::InternalError)?;
        let endpoint = MailConnectionConfig::from_json(
            &serde_json::to_vec(
                &serde_json::json!({"schema_version":1,"account_id":account,"imap":cfg}),
            )
            .map_err(|_| ErrorCode::InternalError)?,
        )
        .map_err(|_| ErrorCode::InternalError)?
        .imap()
        .clone();
        let req = ImapTrashRequest::new(
            &account,
            hantong_contracts::uid_page::UidMailboxIdentity::new(
                &source_key,
                match source_encoding.as_str() {
                    "utf8" => hantong_contracts::uid_page::UidMailboxEncoding::Utf8,
                    "modified_utf7" => {
                        hantong_contracts::uid_page::UidMailboxEncoding::ModifiedUtf7
                    }
                    _ => return Err(ErrorCode::InternalError),
                },
            )
            .map_err(|_| ErrorCode::InternalError)?,
            u64::try_from(generation).map_err(|_| ErrorCode::InternalError)?,
            u32::try_from(validity).map_err(|_| ErrorCode::InternalError)?,
            u32::try_from(uid).map_err(|_| ErrorCode::InternalError)?,
            &op,
            &idem,
            hash,
        )?;
        if hantong_platform::sha256_storage_payload(&req.payload_bytes())? != *req.payload_hash() {
            return Err(ErrorCode::InternalError);
        }
        Ok((
            PendingImapTrash {
                review_id: id,
                owner_registration_id: owner,
                profile_epoch: u64::try_from(epoch).map_err(|_| ErrorCode::InternalError)?,
                account_version: u64::try_from(av).map_err(|_| ErrorCode::InternalError)?,
                endpoint,
                request: req,
                destination: ImapTrashDestination {
                    mailbox_wire_hex: dest_key,
                    mailbox_encoding: dest_encoding,
                },
                created_at_utc_ms: created,
                expires_at_utc_ms: expires,
            },
            state,
        ))
    }
    fn receipt(
        id: [u8; 32],
        account: String,
        state: &str,
        error: Option<ErrorCode>,
    ) -> ImapTrashReceipt {
        let state = match state {
            "pending" => "pending",
            "denied" => "denied",
            "expired" => "expired",
            "consumed" => "consumed",
            "moving" => "moving",
            "moved" => "moved",
            "failed" => "failed",
            "unknown" => "unknown",
            _ => "unknown",
        };
        ImapTrashReceipt {
            account_id: account,
            review_id: id,
            state,
            error,
        }
    }
    fn stored_error(value: Option<String>) -> Option<ErrorCode> {
        value.and_then(|text| {
            ErrorCode::ALL
                .into_iter()
                .find(|code| code.as_str() == text)
        })
    }
    fn outbox_result(
        tx: &Transaction<'_>,
        id: [u8; 32],
    ) -> Result<Option<(String, Option<ErrorCode>)>, ErrorCode> {
        db(tx
            .query_row(
                "SELECT state,error_code FROM imap_trash_outbox WHERE review_id=?1",
                [id.as_slice()],
                |r| Ok((text(r, 0, 16)?, r.get::<_, Option<String>>(1)?)),
            )
            .optional())
        .map(|value| value.map(|(state, error)| (state, stored_error(error))))
    }

    fn begin_audit_tx(tx: &Transaction<'_>, begin: &AuditBegin) -> Result<(), ErrorCode> {
        let exists: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM audit_starts WHERE execution_id=?1)",
            [begin.execution_id().as_slice()],
            |row| row.get(0),
        ))?;
        if exists {
            return Err(ErrorCode::Conflict);
        }
        let registration = db(read_registration(tx, begin.registration_id()))?
            .ok_or(ErrorCode::ClientNotRegistered)?;
        if registration.state != RegistrationState::Active {
            return Err(ErrorCode::ClientSuspended);
        }
        if registration.profile_epoch != begin.profile_epoch() {
            return Err(ErrorCode::AuthRequired);
        }
        for account in begin.account_ids() {
            let known: bool = db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1)",
                [account],
                |row| row.get(0),
            ))?;
            if !known {
                return Err(ErrorCode::ScopeDenied);
            }
        }
        let (executions, accounts): (i64, i64) = db(tx.query_row(
            "SELECT retained_executions,retained_account_refs FROM audit_capacity WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ))?;
        let added = begin.account_ids().len() as i64;
        if !(0..=100_000).contains(&executions) || !(0..=1_000_000).contains(&accounts) {
            return Err(ErrorCode::InternalError);
        }
        if executions >= 100_000 || accounts > 1_000_000 - added {
            return Err(ErrorCode::QuotaExceeded);
        }
        let now = crate::utc_millis().map_err(|_| ErrorCode::InternalError)?;
        db(tx.execute(
            "INSERT INTO audit_starts(execution_id,registration_id,profile_epoch,tool_code,started_at_utc_ms,client_kind,display_label)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                begin.execution_id().as_slice(),
                begin.registration_id().as_slice(),
                begin.profile_epoch() as i64,
                begin.tool().as_str(),
                now,
                registration.client_kind.as_str(),
                registration.display_label,
            ],
        ))?;
        for account in begin.account_ids() {
            db(tx.execute(
                "INSERT INTO audit_start_accounts(execution_id,account_id) VALUES(?1,?2)",
                rusqlite::params![begin.execution_id().as_slice(), account],
            ))?;
        }
        if db(tx.execute(
            "UPDATE audit_capacity SET retained_executions=retained_executions+1,
             retained_account_refs=retained_account_refs+?1 WHERE singleton=1",
            [added],
        ))? != 1
        {
            return Err(ErrorCode::InternalError);
        }
        Ok(())
    }

    fn claim_imap_trash_tx(
        t: &Transaction<'_>,
        p: &PendingImapTrash,
        audit_id: [u8; 32],
    ) -> Result<(), ErrorCode> {
        let (c, s) = pending(t, p.review_id)?;
        if s != "pending" || c != *p {
            return Err(ErrorCode::Conflict);
        }
        grant(
            t,
            p.owner_registration_id,
            p.profile_epoch,
            p.request.account_id(),
            "mail.messages.trash.execute",
        )?;
        if !current_binding(t, p)? {
            return Err(ErrorCode::Conflict);
        }
        if crate::utc_millis().map_err(|_| ErrorCode::InternalError)? >= p.expires_at_utc_ms {
            return Err(ErrorCode::ApprovalExpired);
        }
        let duplicate: bool = db(t.query_row(
            "SELECT EXISTS(SELECT 1 FROM imap_trash_reviews r LEFT JOIN imap_trash_outbox o ON o.review_id=r.review_id WHERE r.review_id<>?1 AND r.account_id=?2 AND r.source_mailbox_key=?3 AND r.source_mailbox_encoding=?4 AND r.uid_validity=?5 AND r.uid=?6 AND (r.state='pending' OR o.state IN ('moving','moved','unknown')))",
            params![p.review_id.as_slice(), p.request.account_id(), p.request.mailbox().wire_hex(), p.request.mailbox().encoding().as_str(), p.request.uidvalidity() as i64, p.request.uid() as i64],
            |q| q.get(0),
        ))?;
        if duplicate {
            return Err(ErrorCode::Conflict);
        }
        let a = db(crate::audit::read_audit(t, &audit_id))?.ok_or(ErrorCode::InternalError)?;
        if a.registration_id() != &p.owner_registration_id
            || a.profile_epoch() != p.profile_epoch
            || a.tool() != AuditTool::TrashExecute
            || a.account_ids() != [p.request.account_id().to_owned()]
            || a.completion().is_some()
        {
            return Err(ErrorCode::InternalError);
        }
        if db(t.execute(
            "UPDATE imap_trash_reviews SET state='consumed' WHERE review_id=?1 AND state='pending'",
            [p.review_id.as_slice()],
        ))? != 1
        {
            return Err(ErrorCode::Conflict);
        }
        db(t.execute(
            "INSERT INTO imap_trash_outbox(account_id,operation_id,idempotency_key,review_id,owner_registration_id,intent_hash,state,created_at_utc_ms,completed_at_utc_ms,audit_id,error_code) VALUES(?1,?2,?3,?4,?5,?6,'moving',?7,NULL,?8,NULL)",
            params![p.request.account_id(), p.request.operation_id(), p.request.idempotency_key(), p.review_id.as_slice(), p.owner_registration_id.as_slice(), p.request.payload_hash().as_slice(), crate::utc_millis().map_err(|_| ErrorCode::InternalError)?, audit_id.as_slice()],
        ))?;
        Ok(())
    }
    fn receipt_for_review(
        tx: &Transaction<'_>,
        id: [u8; 32],
        account: String,
        state: &str,
        error: Option<ErrorCode>,
    ) -> Result<ImapTrashReceipt, ErrorCode> {
        if state == "consumed"
            && let Some((outbox_state, outbox_error)) = outbox_result(tx, id)?
        {
            return Ok(receipt(id, account, &outbox_state, outbox_error));
        }
        Ok(receipt(id, account, state, error))
    }
    impl Store {
        pub fn request_imap_trash(
            &self,
            owner: [u8; 16],
            epoch: u64,
            r: &ImapTrashRequest,
        ) -> Result<ImapTrashReceipt, ErrorCode> {
            tx(self, |t| {
                grant(
                    t,
                    owner,
                    epoch,
                    r.account_id(),
                    "mail.messages.trash.request",
                )?;
                if hantong_platform::sha256_storage_payload(&r.payload_bytes())?
                    != *r.payload_hash()
                {
                    return Err(ErrorCode::InvalidInput);
                };
                let (av, ep) = endpoint(t, r.account_id())?;
                let dest = destination(t, r)?;
                if r.mailbox().wire_hex() == dest.mailbox_wire_hex
                    && r.mailbox().encoding().as_str() == dest.mailbox_encoding
                {
                    return Err(ErrorCode::CapabilityUnsupported);
                }
                let foreign_key: bool = db(t.query_row(
                    "SELECT EXISTS(SELECT 1 FROM imap_trash_reviews WHERE account_id=?1 AND owner_registration_id<>?2 AND (operation_id=?3 OR idempotency_key=?4))",
                    params![r.account_id(), owner.as_slice(), r.operation_id(), r.idempotency_key()],
                    |q| q.get(0),
                ))?;
                if foreign_key {
                    return Err(ErrorCode::Conflict);
                }
                let existing:Option<[u8;32]>=db(t.query_row("SELECT review_id FROM imap_trash_reviews WHERE owner_registration_id=?1 AND (operation_id=?2 OR idempotency_key=?3)",params![owner.as_slice(),r.operation_id(),r.idempotency_key()],|q|blob_column::<32>(q,0)).optional())?;
                if let Some(id) = existing {
                    let (p, s) = pending(t, id)?;
                    return if p.owner_registration_id == owner
                        && p.profile_epoch == epoch
                        && p.request == *r
                    {
                        receipt_for_review(t, id, r.account_id().into(), &s, None)
                    } else {
                        Err(ErrorCode::Conflict)
                    };
                };
                let replayed: bool = db(t.query_row(
                    "SELECT EXISTS(SELECT 1 FROM imap_trash_reviews r LEFT JOIN imap_trash_outbox o ON o.review_id=r.review_id WHERE r.account_id=?1 AND r.source_mailbox_key=?2 AND r.source_mailbox_encoding=?3 AND r.uid_validity=?4 AND r.uid=?5 AND (r.state='pending' OR o.state IN ('moving','moved','unknown')))"
                    , params![r.account_id(), r.mailbox().wire_hex(), r.mailbox().encoding().as_str(), r.uidvalidity() as i64, r.uid() as i64],
                    |q| q.get(0),
                ))?;
                if replayed {
                    return Err(ErrorCode::Conflict);
                }
                let id = hantong_platform::random_nonce()?;
                let now = crate::utc_millis().map_err(|_| ErrorCode::InternalError)?;
                let exp = now.checked_add(300000).ok_or(ErrorCode::InternalError)?;
                let endpoint_json =
                    serde_json::to_string(&ep).map_err(|_| ErrorCode::InternalError)?;
                db(t.execute("INSERT INTO imap_trash_reviews(review_id,owner_registration_id,profile_epoch,account_id,account_version,endpoint_json,source_mailbox_key,source_mailbox_encoding,catalog_generation,uid_validity,uid,operation_id,idempotency_key,intent_hash,destination_mailbox_key,destination_mailbox_encoding,created_at_utc_ms,expires_at_utc_ms,state,error_code) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,'pending',NULL)",params![id.as_slice(),owner.as_slice(),epoch as i64,r.account_id(),av as i64,endpoint_json,r.mailbox().wire_hex(),r.mailbox().encoding().as_str(),r.catalog_generation() as i64,r.uidvalidity() as i64,r.uid(),r.operation_id(),r.idempotency_key(),r.payload_hash().as_slice(),dest.mailbox_wire_hex,dest.mailbox_encoding,now,exp]))?;
                Ok(receipt(id, r.account_id().into(), "pending", None))
            })
        }
        pub fn pending_imap_trash(&self, id: [u8; 32]) -> Result<PendingImapTrash, ErrorCode> {
            tx(self, |t| {
                let (p, s) = pending(t, id)?;
                if s != "pending" {
                    return Err(ErrorCode::Conflict);
                };
                grant(
                    t,
                    p.owner_registration_id,
                    p.profile_epoch,
                    p.request.account_id(),
                    "mail.messages.trash.execute",
                )?;
                if !current_binding(t, &p)? {
                    return Err(ErrorCode::Conflict);
                }
                if crate::utc_millis().map_err(|_| ErrorCode::InternalError)? >= p.expires_at_utc_ms
                {
                    return Err(ErrorCode::ApprovalExpired);
                };
                Ok(p)
            })
        }
        pub fn status_imap_trash(
            &self,
            owner: [u8; 16],
            epoch: u64,
            id: [u8; 32],
        ) -> Result<ImapTrashReceipt, ErrorCode> {
            tx(self, |t| {
                let row = db(t
                    .query_row(
                        "SELECT owner_registration_id,profile_epoch,account_id,state,error_code,expires_at_utc_ms FROM imap_trash_reviews WHERE review_id=?1",
                        [id.as_slice()],
                        |r| {
                            Ok((
                                blob_column::<16>(r, 0)?,
                                r.get::<_, i64>(1)?,
                                text(r, 2, 128)?,
                                text(r, 3, 16)?,
                                r.get::<_, Option<String>>(4)?,
                                r.get::<_, i64>(5)?,
                            ))
                        },
                    )
                    .optional())?
                    .ok_or(ErrorCode::NotFound)?;
                if row.0 != owner || u64::try_from(row.1).ok() != Some(epoch) {
                    return Err(ErrorCode::AuthRequired);
                }
                grant(t, owner, epoch, &row.2, "mail.messages.trash.request")?;
                let mut state = row.3;
                let mut error = stored_error(row.4);
                if state == "pending"
                    && crate::utc_millis().map_err(|_| ErrorCode::InternalError)? >= row.5
                {
                    db(t.execute(
                        "UPDATE imap_trash_reviews SET state='expired',error_code=?2 WHERE review_id=?1 AND state='pending'",
                        params![id.as_slice(), ErrorCode::ApprovalExpired.as_str()],
                    ))?;
                    state = "expired".into();
                    error = Some(ErrorCode::ApprovalExpired);
                }
                receipt_for_review(t, id, row.2, &state, error)
            })
        }
        pub fn deny_imap_trash(&self, id: [u8; 32], error: ErrorCode) -> Result<(), ErrorCode> {
            tx(self, |t| {
                if !matches!(
                    error,
                    ErrorCode::ApprovalDenied | ErrorCode::ApprovalExpired
                ) {
                    return Err(ErrorCode::InvalidInput);
                }
                let state = if error == ErrorCode::ApprovalExpired {
                    "expired"
                } else {
                    "denied"
                };
                let changed = db(t.execute(
                    "UPDATE imap_trash_reviews SET state=?2,error_code=?3 WHERE review_id=?1 AND state='pending'",
                    params![id.as_slice(), state, error.as_str()],
                ))?;
                if changed == 0 {
                    let exists: bool = db(t.query_row(
                        "SELECT EXISTS(SELECT 1 FROM imap_trash_reviews WHERE review_id=?1)",
                        [id.as_slice()],
                        |r| r.get(0),
                    ))?;
                    return Err(if exists {
                        ErrorCode::Conflict
                    } else {
                        ErrorCode::NotFound
                    });
                }
                Ok(())
            })
        }
        pub fn can_confirm_imap_trash(&self, p: &PendingImapTrash) -> Result<bool, ErrorCode> {
            tx(self, |t| {
                let (c, s) = pending(t, p.review_id)?;
                Ok(s == "pending"
                    && c == *p
                    && current_binding(t, &c)?
                    && crate::utc_millis().map_err(|_| ErrorCode::InternalError)?
                        < p.expires_at_utc_ms)
            })
        }
        #[cfg(test)]
        pub(crate) fn claim_imap_trash(
            &self,
            p: &PendingImapTrash,
            audit_id: [u8; 32],
        ) -> Result<(), ErrorCode> {
            tx(self, |t| claim_imap_trash_tx(t, p, audit_id))
        }

        /// Atomically starts the matching audit and claims the pending MOVE.
        /// Any validation failure rolls back both the audit start and outbox row.
        pub fn begin_and_claim_imap_trash(
            &self,
            p: &PendingImapTrash,
            begin: &AuditBegin,
        ) -> Result<(), ErrorCode> {
            tx(self, |t| {
                if begin.registration_id() != &p.owner_registration_id
                    || begin.profile_epoch() != p.profile_epoch
                    || begin.tool() != AuditTool::TrashExecute
                    || begin.account_ids() != [p.request.account_id().to_owned()]
                {
                    return Err(ErrorCode::Conflict);
                }
                begin_audit_tx(t, begin)?;
                claim_imap_trash_tx(t, p, *begin.execution_id())
            })
        }

        pub fn finish_imap_trash(
            &self,
            id: [u8; 32],
            outcome: ImapTrashOutcome,
        ) -> Result<(), ErrorCode> {
            tx(self, |t| {
                let (a,) = db(t.query_row(
                    "SELECT audit_id FROM imap_trash_outbox WHERE review_id=?1 AND state='moving'",
                    [id.as_slice()],
                    |r| Ok((blob_column::<32>(r, 0)?,)),
                ))?;
                let (now, state, error, term) = match outcome {
                    ImapTrashOutcome::Moved => (
                        crate::utc_millis().map_err(|_| ErrorCode::InternalError)?,
                        "moved",
                        None,
                        AuditTerminal::succeeded(Some(1))?,
                    ),
                    ImapTrashOutcome::Failed => (
                        crate::utc_millis().map_err(|_| ErrorCode::InternalError)?,
                        "failed",
                        Some(ErrorCode::PolicyBlocked),
                        AuditTerminal::failed(ErrorCode::PolicyBlocked),
                    ),
                    ImapTrashOutcome::Unknown => (
                        crate::utc_millis().map_err(|_| ErrorCode::InternalError)?,
                        "unknown",
                        Some(ErrorCode::SendUnknown),
                        AuditTerminal::failed(ErrorCode::SendUnknown),
                    ),
                };
                db(t.execute("UPDATE imap_trash_outbox SET state=?2,completed_at_utc_ms=?3,error_code=?4 WHERE review_id=?1 AND state='moving'",params![id.as_slice(),state,now,error.map(ErrorCode::as_str)]))?;
                crate::audit::insert_audit_terminal_tx(t, a, term, now)?;
                Ok(())
            })
        }
        pub fn recover_inflight_imap_trash(&self) -> Result<usize, ErrorCode> {
            tx(self, |t| {
                let mut statement = db(t.prepare(
                    "SELECT review_id FROM imap_trash_outbox WHERE state='moving' LIMIT 10001",
                ))?;
                let ids = statement
                    .query_map([], |r| blob_column::<32>(r, 0))
                    .map_err(|_| ErrorCode::InternalError)?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(|_| ErrorCode::InternalError)?;
                if ids.len() > 10000 {
                    return Err(ErrorCode::InternalError);
                }
                for id in &ids {
                    let audit_id = db(t.query_row(
                        "SELECT audit_id FROM imap_trash_outbox WHERE review_id=?1",
                        [id.as_slice()],
                        |r| blob_column::<32>(r, 0),
                    ))?;
                    let now = crate::utc_millis().map_err(|_| ErrorCode::InternalError)?;
                    db(t.execute(
                        "UPDATE imap_trash_outbox SET state='unknown',completed_at_utc_ms=?2,error_code=?3 WHERE review_id=?1 AND state='moving'",
                        params![id.as_slice(), now, ErrorCode::SendUnknown.as_str()],
                    ))?;
                    crate::audit::insert_audit_terminal_tx(
                        t,
                        audit_id,
                        AuditTerminal::failed(ErrorCode::SendUnknown),
                        now,
                    )?;
                }
                Ok(ids.len())
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::{CliRegistrationStage, ClientKind, DEFAULT_CLI_LABEL};
        use hantong_contracts::{
            audit::{AuditBegin, AuditTool},
            auth::{RequestedScope, default_cli_legacy_tools, default_cli_manage_tools},
            uid_page::{UidMailboxEncoding, UidMailboxIdentity},
            wire_hex,
        };
        use hantong_domain::{
            capabilities::{ImapCapabilities, ImapFeature},
            mailboxes::{ImapMailboxDescriptor, ImapMailboxName, MailboxChildren, MailboxEncoding},
        };
        use rusqlite::params;
        use std::{
            fs,
            path::PathBuf,
            sync::atomic::{AtomicU64, Ordering},
        };

        fn trash(name: &[u8]) -> ImapMailboxDescriptor {
            ImapMailboxDescriptor::new(
                ImapMailboxName::from_wire(name, MailboxEncoding::Utf8).unwrap(),
                Some('/'),
                false,
                false,
                MailboxChildren::HasNoChildren,
                vec![SpecialUseRole::Trash],
            )
            .unwrap()
        }

        const ACCOUNT: &str = "imap-trash-account";

        struct Fixture {
            store: Store,
            root: PathBuf,
            owner: [u8; 16],
            epoch: u64,
        }

        impl Fixture {
            fn new() -> Self {
                static NEXT: AtomicU64 = AtomicU64::new(0);
                let root = std::env::temp_dir().join(format!(
                    "hantong-imap-trash-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                fs::create_dir(&root).unwrap();
                let store = Store::open(root.join("data"), root.join("keys")).unwrap();
                store
                    .with_connection(|c| {
                        c.execute(
                            "INSERT INTO server_identity VALUES(1,?1,'active',?2,1,1,1,1)",
                            params![server_pin().as_slice(), public_key().as_slice()],
                        )?;
                        c.execute(
                            "INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES(?1,'imap',1,'ready','[]','[]',1)",
                            [ACCOUNT],
                        )?;
                        c.execute(
                            "INSERT INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES(?1,'imap',1,'imap.example',993,'implicit_tls','imap-user','app_password',1)",
                            [ACCOUNT],
                        )?;
                        Ok(())
                    })
                    .unwrap();
                let scope =
                    RequestedScope::new(vec![ACCOUNT.into()], default_cli_legacy_tools()).unwrap();
                let request = store
                    .create_registration_request(
                        [0x81; 32],
                        ClientKind::Cli,
                        DEFAULT_CLI_LABEL,
                        &scope,
                    )
                    .unwrap();
                let registration_id =
                    hantong_platform::registration_id_for_request(request.request_id).unwrap();
                let staged = store
                    .stage_terminal_cli_registration(
                        &CliRegistrationStage::new_terminal_cli(
                            request,
                            registration_id,
                            public_key(),
                            server_pin(),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                store
                    .activate_cli_registration(registration_id, staged.version, server_pin())
                    .unwrap();
                let pending = store
                    .create_terminal_cli_scope_refresh(
                        [0x82; 32],
                        RequestedScope::new(vec![ACCOUNT.into()], default_cli_manage_tools())
                            .unwrap(),
                        server_pin(),
                    )
                    .unwrap();
                let staged = store.stage_cli_scope_refresh(&pending).unwrap();
                let active = store.activate_cli_scope_refresh(&staged).unwrap();
                let fixture = Self {
                    store,
                    root,
                    owner: active.registration_id,
                    epoch: active.profile_epoch,
                };
                fixture.seed_catalog();
                fixture
            }

            fn seed_catalog(&self) {
                let source_key = wire_hex::encode(b"INBOX");
                let destination_key = wire_hex::encode(b"Trash");
                let snapshot = ImapMailboxSnapshot::new(
                    ACCOUNT,
                    1,
                    ImapCapabilities::from_features([ImapFeature::Move]),
                    vec![
                        ImapMailboxDescriptor::new(
                            ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::Utf8).unwrap(),
                            Some('/'),
                            false,
                            false,
                            MailboxChildren::HasNoChildren,
                            vec![],
                        )
                        .unwrap(),
                        ImapMailboxDescriptor::new(
                            ImapMailboxName::from_wire(b"Trash", MailboxEncoding::Utf8).unwrap(),
                            Some('/'),
                            false,
                            false,
                            MailboxChildren::HasNoChildren,
                            vec![SpecialUseRole::Trash],
                        )
                        .unwrap(),
                    ],
                )
                .unwrap();
                let snapshot_json = String::from_utf8(snapshot.to_storage_json().unwrap()).unwrap();
                self.store.with_connection(|c| {
                    c.execute("INSERT INTO imap_mailbox_catalog_operations(operation_id,account_id,idempotency_key,account_version,endpoint_version,endpoint_hash,snapshot_hash,expected_version,result_version,payload_hash,completed_at_utc_ms) VALUES('trash-catalog',?1,'trash-catalog-key',1,1,?2,?3,0,1,?4,1)", params![ACCOUNT, [1_u8;32].as_slice(), [2_u8;32].as_slice(), [3_u8;32].as_slice()])?;
                    c.execute("INSERT INTO imap_mailbox_catalogs(account_id,account_version,endpoint_version,endpoint_hash,generation,snapshot_json,snapshot_hash,last_operation_id,stored_at_utc_ms) VALUES(?1,1,1,?2,1,?3,?4,'trash-catalog',1)", params![ACCOUNT, [1_u8;32].as_slice(), snapshot_json, [2_u8;32].as_slice()])?;
                    c.execute("INSERT INTO imap_uid_page_operations(operation_id,account_id,idempotency_key,mailbox_key,mailbox_encoding,expected_cursor_version,result_cursor_version,account_version,catalog_generation,endpoint_version,endpoint_hash,page_json,page_hash,payload_hash,uid_validity,uid_next_at_start,after_uid,covered_through_uid,inserted_count,applied_at_utc_ms) VALUES('trash-page',?1,'trash-page-key',?2,'utf8',0,1,1,1,1,?3,'{}',?4,?5,7,2,0,1,1,1)", params![ACCOUNT, source_key, [1_u8;32].as_slice(), [2_u8;32].as_slice(), [3_u8;32].as_slice()])?;
                    c.execute("INSERT INTO imap_uid_cursors(account_id,mailbox_key,mailbox_encoding,version,account_version,catalog_generation,endpoint_version,endpoint_hash,uid_validity,uid_next_at_start,covered_through_uid,stored_at_utc_ms,last_operation_id) VALUES(?1,?2,'utf8',1,1,1,1,?3,7,2,1,1,'trash-page')", params![ACCOUNT, source_key, [1_u8;32].as_slice()])?;
                    c.execute("INSERT INTO imap_uid_messages(account_id,mailbox_key,mailbox_encoding,uid_validity,uid,metadata_json,metadata_hash,stored_at_utc_ms,operation_id) VALUES(?1,?2,'utf8',7,1,'{}',?3,1,'trash-page')", params![ACCOUNT, source_key, [3_u8;32].as_slice()])?;
                    Ok(())
                }).unwrap();
                let _ = destination_key;
            }

            fn request(&self, operation: &str, key: &str) -> ImapTrashReceipt {
                self.store
                    .request_imap_trash(self.owner, self.epoch, &request_value(operation, key))
                    .unwrap()
            }

            fn secondary_owner(&self) -> ([u8; 16], u64) {
                let owner = [0x93; 16];
                let request_id = [0x94_u8; 32];
                let now = crate::utc_millis().unwrap();
                let mut key = public_key();
                key[40..].copy_from_slice(
                    &wire_hex::decode_fixed::<32>(
                        "b01cbd1c01e58065711814b583f061e9d431cca994cea1313449bf97c840ae0a",
                    )
                    .unwrap(),
                );
                let tools = default_cli_manage_tools();
                let tools_json = serde_json::to_string(&tools).unwrap();
                self.store
                    .with_connection(|c| {
                        c.execute(
                            "INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,decision_channel,version,created_at_utc_ms,expires_at_utc_ms,decided_at_utc_ms) VALUES(?1,'cli',?2,?3,?4,'approved','terminal_cli',2,?5,?6,?5)",
                            params![
                                request_id.as_slice(),
                                "synthetic second CLI",
                                serde_json::json!([ACCOUNT]).to_string(),
                                tools_json,
                                now,
                                now + 300000,
                            ],
                        )?;
                        c.execute(
                            "INSERT INTO client_registrations(registration_id,request_id,client_kind,display_label,public_key,profile_epoch,state,read_only,requests_per_minute,approved_channel,approved_at_utc_ms,updated_at_utc_ms,revoked_at_utc_ms,version) VALUES(?1,?2,'cli',?3,?4,1,'active',0,60,'terminal_cli',?5,?5,NULL,2)",
                            params![
                                owner.as_slice(),
                                request_id.as_slice(),
                                "synthetic second CLI",
                                key.as_slice(),
                                now,
                            ],
                        )?;
                        c.execute(
                            "INSERT INTO client_registration_accounts(registration_id,account_id) VALUES(?1,?2)",
                            params![owner.as_slice(), ACCOUNT],
                        )?;
                        for tool in &tools {
                            c.execute(
                                "INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,?2)",
                                params![owner.as_slice(), tool],
                            )?;
                        }
                        Ok::<_, rusqlite::Error>(())
                    })
                    .unwrap();
                (owner, 1)
            }
        }

        fn request_value(operation: &str, key: &str) -> ImapTrashRequest {
            request_value_for_uid(operation, key, 1)
        }

        fn request_value_for_uid(operation: &str, key: &str, uid: u32) -> ImapTrashRequest {
            let initial = ImapTrashRequest::new(
                ACCOUNT,
                hantong_contracts::uid_page::UidMailboxIdentity::new(
                    &wire_hex::encode(b"INBOX"),
                    hantong_contracts::uid_page::UidMailboxEncoding::Utf8,
                )
                .unwrap(),
                1,
                7,
                uid,
                operation,
                key,
                [0; 32],
            )
            .unwrap();
            let hash = hantong_platform::sha256_storage_payload(&initial.payload_bytes()).unwrap();
            ImapTrashRequest::new(
                ACCOUNT,
                initial.mailbox().clone(),
                1,
                7,
                uid,
                operation,
                key,
                hash,
            )
            .unwrap()
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.root);
            }
        }

        fn public_key() -> [u8; 72] {
            let mut key = [0; 72];
            key[..8].copy_from_slice(b"ECS1\x20\0\0\0");
            key[8..40].copy_from_slice(
                &wire_hex::decode_fixed::<32>(
                    "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
                )
                .unwrap(),
            );
            key[40..].copy_from_slice(
                &wire_hex::decode_fixed::<32>(
                    "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
                )
                .unwrap(),
            );
            key
        }

        fn server_pin() -> [u8; 32] {
            hantong_platform::sha256(&public_key()).unwrap()
        }

        #[test]
        fn sec03_unique_trash_rejects_ambiguous_catalog_without_selection() {
            let empty = ImapMailboxSnapshot::new(
                "acct",
                1,
                ImapCapabilities::from_features([ImapFeature::Move]),
                vec![],
            )
            .unwrap();
            assert_eq!(unique_trash(&empty), Err(ErrorCode::CapabilityUnsupported));

            let ambiguous = ImapMailboxSnapshot::new(
                "acct",
                1,
                ImapCapabilities::from_features([ImapFeature::Move]),
                vec![trash(b"Trash"), trash(b"Deleted")],
            )
            .unwrap();
            assert_eq!(
                unique_trash(&ambiguous),
                Err(ErrorCode::CapabilityUnsupported)
            );
        }

        #[test]
        fn sec03_request_is_pending_and_does_not_create_provider_outbox() {
            let fixture = Fixture::new();
            let receipt = fixture.request("trash-op-1", "trash-key-1");
            assert_eq!(receipt.state, "pending");
            fixture
                .store
                .with_connection(|c| {
                    let count: i64 =
                        c.query_row("SELECT count(*) FROM imap_trash_outbox", [], |r| r.get(0))?;
                    assert_eq!(count, 0);
                    Ok::<_, rusqlite::Error>(())
                })
                .unwrap();
        }

        #[test]
        fn sec03_claim_consumes_review_and_second_claim_is_rejected() {
            let fixture = Fixture::new();
            let receipt = fixture.request("trash-op-2", "trash-key-2");
            let pending = fixture.store.pending_imap_trash(receipt.review_id).unwrap();
            let execution_id = [0x42; 32];
            fixture
                .store
                .begin_audit(
                    &AuditBegin::new(
                        execution_id,
                        fixture.owner,
                        fixture.epoch,
                        AuditTool::TrashExecute,
                        vec![ACCOUNT.into()],
                    )
                    .unwrap(),
                )
                .unwrap();
            fixture
                .store
                .claim_imap_trash(&pending, execution_id)
                .unwrap();
            assert_eq!(
                fixture.store.claim_imap_trash(&pending, [0x43; 32]),
                Err(ErrorCode::Conflict)
            );
            fixture
                .store
                .with_connection(|c| {
                    let review: String = c.query_row(
                        "SELECT state FROM imap_trash_reviews WHERE review_id=?1",
                        [receipt.review_id.as_slice()],
                        |r| r.get(0),
                    )?;
                    let outbox: (i64, String) = c.query_row(
                        "SELECT count(*),max(state) FROM imap_trash_outbox WHERE review_id=?1",
                        [receipt.review_id.as_slice()],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )?;
                    assert_eq!(review, "consumed");
                    assert_eq!(outbox, (1, "moving".into()));
                    Ok::<_, rusqlite::Error>(())
                })
                .unwrap();
        }

        #[test]
        fn sec03_atomic_begin_claim_rolls_back_audit_when_binding_changes() {
            let fixture = Fixture::new();
            let receipt = fixture.request("trash-op-atomic", "trash-key-atomic");
            let pending = fixture.store.pending_imap_trash(receipt.review_id).unwrap();
            fixture
                .store
                .with_connection(|c| {
                    c.execute(
                        "UPDATE accounts SET settings_version=2 WHERE account_id=?1",
                        [ACCOUNT],
                    )?;
                    Ok::<_, rusqlite::Error>(())
                })
                .unwrap();
            let begin = AuditBegin::new(
                [0x46; 32],
                fixture.owner,
                fixture.epoch,
                AuditTool::TrashExecute,
                vec![ACCOUNT.into()],
            )
            .unwrap();
            assert_eq!(
                fixture.store.begin_and_claim_imap_trash(&pending, &begin),
                Err(ErrorCode::Conflict)
            );
            fixture
                .store
                .with_connection(|c| {
                    let starts: i64 = c.query_row(
                        "SELECT count(*) FROM audit_starts WHERE execution_id=?1",
                        [begin.execution_id().as_slice()],
                        |r| r.get(0),
                    )?;
                    let outbox: i64 = c.query_row(
                        "SELECT count(*) FROM imap_trash_outbox WHERE review_id=?1",
                        [receipt.review_id.as_slice()],
                        |r| r.get(0),
                    )?;
                    assert_eq!(starts, 0);
                    assert_eq!(outbox, 0);
                    Ok::<_, rusqlite::Error>(())
                })
                .unwrap();
        }

        #[test]
        fn sec03_recovery_marks_moving_outbox_unknown_without_retry() {
            let fixture = Fixture::new();
            let receipt = fixture.request("trash-op-3", "trash-key-3");
            let pending = fixture.store.pending_imap_trash(receipt.review_id).unwrap();
            let execution_id = [0x44; 32];
            fixture
                .store
                .begin_audit(
                    &AuditBegin::new(
                        execution_id,
                        fixture.owner,
                        fixture.epoch,
                        AuditTool::TrashExecute,
                        vec![ACCOUNT.into()],
                    )
                    .unwrap(),
                )
                .unwrap();
            fixture
                .store
                .claim_imap_trash(&pending, execution_id)
                .unwrap();
            assert_eq!(fixture.store.recover_inflight_imap_trash().unwrap(), 1);
            assert_eq!(fixture.store.recover_inflight_imap_trash().unwrap(), 0);
            assert_eq!(
                fixture
                    .store
                    .status_imap_trash(fixture.owner, fixture.epoch, receipt.review_id)
                    .unwrap()
                    .state,
                "unknown"
            );
            fixture
                .store
                .with_connection(|c| {
                    let state: String = c.query_row(
                        "SELECT state FROM imap_trash_outbox WHERE review_id=?1",
                        [receipt.review_id.as_slice()],
                        |r| r.get(0),
                    )?;
                    assert_eq!(state, "unknown");
                    Ok::<_, rusqlite::Error>(())
                })
                .unwrap();
        }

        #[test]
        fn sec03_same_uid_with_new_operation_key_is_blocked_after_unknown() {
            let fixture = Fixture::new();
            let receipt = fixture.request("trash-op-4", "trash-key-4");
            let pending = fixture.store.pending_imap_trash(receipt.review_id).unwrap();
            let execution_id = [0x45; 32];
            fixture
                .store
                .begin_audit(
                    &AuditBegin::new(
                        execution_id,
                        fixture.owner,
                        fixture.epoch,
                        AuditTool::TrashExecute,
                        vec![ACCOUNT.into()],
                    )
                    .unwrap(),
                )
                .unwrap();
            fixture
                .store
                .claim_imap_trash(&pending, execution_id)
                .unwrap();
            fixture.store.recover_inflight_imap_trash().unwrap();
            assert_eq!(
                fixture.store.request_imap_trash(
                    fixture.owner,
                    fixture.epoch,
                    &request_value("trash-op-5", "trash-key-5")
                ),
                Err(ErrorCode::Conflict)
            );
        }

        #[test]
        fn sec03_status_rejects_a_different_registration_owner() {
            let fixture = Fixture::new();
            let receipt = fixture.request("trash-op-6", "trash-key-6");
            assert_eq!(
                fixture
                    .store
                    .status_imap_trash([0x72; 16], fixture.epoch, receipt.review_id),
                Err(ErrorCode::AuthRequired)
            );
        }

        #[test]
        fn sec03_same_account_operation_key_is_conflict_across_owners() {
            let fixture = Fixture::new();
            fixture
                .store
                .with_connection(|c| {
                    c.execute(
                        "INSERT INTO imap_uid_messages(account_id,mailbox_key,mailbox_encoding,uid_validity,uid,metadata_json,metadata_hash,stored_at_utc_ms,operation_id) VALUES(?1,?2,'utf8',7,2,'{}',?3,1,'trash-page')",
                        params![ACCOUNT, wire_hex::encode(b"INBOX"), [3_u8; 32].as_slice()],
                    )?;
                    Ok::<_, rusqlite::Error>(())
                })
                .unwrap();
            let first_request = request_value_for_uid("shared-operation", "shared-idempotency", 2);
            let first = fixture
                .store
                .request_imap_trash(fixture.owner, fixture.epoch, &first_request)
                .unwrap();
            let (other_owner, other_epoch) = fixture.secondary_owner();
            assert_eq!(
                fixture.store.request_imap_trash(
                    other_owner,
                    other_epoch,
                    &request_value_for_uid("shared-operation", "shared-idempotency", 2),
                ),
                Err(ErrorCode::Conflict)
            );
            fixture
                .store
                .with_connection(|c| {
                    let row: ([u8; 16], i64) = c.query_row(
                        "SELECT owner_registration_id,count(*) FROM imap_trash_reviews WHERE account_id=?1",
                        [ACCOUNT],
                        |r| Ok((blob_column::<16>(r, 0)?, r.get(1)?)),
                    )?;
                    assert_eq!(row.0, fixture.owner);
                    assert_eq!(row.1, 1);
                    Ok::<_, rusqlite::Error>(())
                })
                .unwrap();
            assert_eq!(first.account_id, ACCOUNT);
        }

        #[test]
        fn sec03_tampered_stored_payload_hash_is_rejected_closed() {
            let fixture = Fixture::new();
            let receipt = fixture.request("tampered-hash", "tampered-hash-key");
            fixture
                .store
                .with_connection(|c| {
                    c.execute(
                        "UPDATE imap_trash_reviews SET intent_hash=?1 WHERE review_id=?2",
                        params![[0x7f_u8; 32].as_slice(), receipt.review_id.as_slice()],
                    )?;
                    Ok::<_, rusqlite::Error>(())
                })
                .unwrap();
            assert_eq!(
                fixture.store.pending_imap_trash(receipt.review_id),
                Err(ErrorCode::InternalError)
            );
        }

        #[test]
        fn sec03_tampered_stored_uid_is_rejected_closed() {
            let fixture = Fixture::new();
            let receipt = fixture.request("tampered-uid", "tampered-uid-key");
            fixture
                .store
                .with_connection(|c| {
                    c.execute(
                        "UPDATE imap_trash_reviews SET uid=2 WHERE review_id=?1",
                        [receipt.review_id.as_slice()],
                    )?;
                    Ok::<_, rusqlite::Error>(())
                })
                .unwrap();
            assert_eq!(
                fixture.store.pending_imap_trash(receipt.review_id),
                Err(ErrorCode::InternalError)
            );
        }

        #[test]
        fn sec03_ambiguous_or_missing_trash_is_rejected_before_review_insert() {
            let fixture = Fixture::new();
            let empty = ImapMailboxSnapshot::new(
                ACCOUNT,
                1,
                ImapCapabilities::from_features([ImapFeature::Move]),
                vec![],
            )
            .unwrap();
            let json = String::from_utf8(empty.to_storage_json().unwrap()).unwrap();
            fixture
                .store
                .with_connection(|c| {
                    c.execute(
                        "UPDATE imap_mailbox_catalogs SET snapshot_json=?1 WHERE account_id=?2",
                        params![json, ACCOUNT],
                    )?;
                    Ok::<_, rusqlite::Error>(())
                })
                .unwrap();
            let initial = ImapTrashRequest::new(
                ACCOUNT,
                hantong_contracts::uid_page::UidMailboxIdentity::new(
                    &wire_hex::encode(b"INBOX"),
                    hantong_contracts::uid_page::UidMailboxEncoding::Utf8,
                )
                .unwrap(),
                1,
                7,
                1,
                "trash-op-7",
                "trash-key-7",
                [0; 32],
            )
            .unwrap();
            let hash = hantong_platform::sha256_storage_payload(&initial.payload_bytes()).unwrap();
            let request = ImapTrashRequest::new(
                ACCOUNT,
                initial.mailbox().clone(),
                1,
                7,
                1,
                "trash-op-7",
                "trash-key-7",
                hash,
            )
            .unwrap();
            assert_eq!(
                fixture
                    .store
                    .request_imap_trash(fixture.owner, fixture.epoch, &request),
                Err(ErrorCode::CapabilityUnsupported)
            );
            fixture
                .store
                .with_connection(|c| {
                    let count: i64 =
                        c.query_row("SELECT count(*) FROM imap_trash_reviews", [], |r| r.get(0))?;
                    assert_eq!(count, 0);
                    Ok::<_, rusqlite::Error>(())
                })
                .unwrap();
        }

        #[test]
        fn sec03_self_trash_destination_is_rejected_before_review_insert() {
            let fixture = Fixture::new();
            let mailbox =
                UidMailboxIdentity::new(&wire_hex::encode(b"INBOX"), UidMailboxEncoding::Utf8)
                    .unwrap();
            let initial = ImapTrashRequest::new(
                ACCOUNT,
                mailbox.clone(),
                1,
                7,
                1,
                "trash-self-op",
                "trash-self-key",
                [0; 32],
            )
            .unwrap();
            let request = ImapTrashRequest::new(
                ACCOUNT,
                mailbox,
                1,
                7,
                1,
                "trash-self-op",
                "trash-self-key",
                hantong_platform::sha256_storage_payload(&initial.payload_bytes()).unwrap(),
            )
            .unwrap();
            let self_trash_snapshot = ImapMailboxSnapshot::new(
                ACCOUNT,
                1,
                ImapCapabilities::from_features([ImapFeature::Move]),
                vec![
                    ImapMailboxDescriptor::new(
                        ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::Utf8).unwrap(),
                        Some('/'),
                        false,
                        false,
                        MailboxChildren::HasNoChildren,
                        vec![SpecialUseRole::Trash],
                    )
                    .unwrap(),
                ],
            )
            .unwrap();
            let self_trash_json =
                String::from_utf8(self_trash_snapshot.to_storage_json().unwrap()).unwrap();
            fixture
                .store
                .with_connection(|c| {
                    c.execute(
                        "UPDATE imap_mailbox_catalogs SET snapshot_json=?1 WHERE account_id=?2 AND generation=1",
                        params![self_trash_json, ACCOUNT],
                    )?;
                    Ok::<_, rusqlite::Error>(())
                })
                .unwrap();
            assert_eq!(
                fixture
                    .store
                    .request_imap_trash(fixture.owner, fixture.epoch, &request),
                Err(ErrorCode::CapabilityUnsupported)
            );
            fixture
                .store
                .with_connection(|c| {
                    let count: i64 =
                        c.query_row("SELECT count(*) FROM imap_trash_reviews", [], |r| r.get(0))?;
                    assert_eq!(count, 0);
                    Ok::<_, rusqlite::Error>(())
                })
                .unwrap();
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
impl Store {
    pub fn request_imap_trash(
        &self,
        _: [u8; 16],
        _: u64,
        _: &ImapTrashRequest,
    ) -> Result<ImapTrashReceipt, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    pub fn pending_imap_trash(&self, _: [u8; 32]) -> Result<PendingImapTrash, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    pub fn status_imap_trash(
        &self,
        _: [u8; 16],
        _: u64,
        _: [u8; 32],
    ) -> Result<ImapTrashReceipt, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    pub fn deny_imap_trash(&self, _: [u8; 32], _: ErrorCode) -> Result<(), ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    pub fn can_confirm_imap_trash(&self, _: &PendingImapTrash) -> Result<bool, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    #[cfg(test)]
    pub(crate) fn claim_imap_trash(
        &self,
        _: &PendingImapTrash,
        _: [u8; 32],
    ) -> Result<(), ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    pub fn begin_and_claim_imap_trash(
        &self,
        _: &PendingImapTrash,
        _: &hantong_contracts::audit::AuditBegin,
    ) -> Result<(), ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    pub fn finish_imap_trash(&self, _: [u8; 32], _: ImapTrashOutcome) -> Result<(), ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    pub fn recover_inflight_imap_trash(&self) -> Result<usize, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
}
