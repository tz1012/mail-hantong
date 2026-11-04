//! Durable, metadata-only receive-event tracking for external agents.

use crate::Store;
use hantong_contracts::{
    ErrorCode,
    mail_events::{MailEvent, MailEventPage, MailEventTarget},
};
#[derive(Clone, Eq, PartialEq)]
pub struct MailEventTrackingTarget {
    pub target: MailEventTarget,
    pub baseline_event_id: u64,
    pub acknowledged_event_id: u64,
}

impl std::fmt::Debug for MailEventTrackingTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MailEventTrackingTarget(<redacted>)")
    }
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod storage {
    use super::*;
    use crate::registrations::{read_registration, text_column};
    use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

    const RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;
    const MAX_PER_ACCOUNT: i64 = 10_000;
    const MAX_PAGE: u32 = 100;

    fn db<T>(result: rusqlite::Result<T>) -> Result<T, ErrorCode> {
        result.map_err(|_| ErrorCode::InternalError)
    }

    fn auth(
        tx: &Transaction<'_>,
        owner: &[u8; 16],
        epoch: u64,
    ) -> Result<crate::RegistrationRecord, ErrorCode> {
        let registration =
            db(read_registration(tx, owner))?.ok_or(ErrorCode::ClientNotRegistered)?;
        if registration.state != crate::RegistrationState::Active {
            return Err(ErrorCode::ClientSuspended);
        }
        if registration.profile_epoch != epoch {
            return Err(ErrorCode::AuthRequired);
        }
        if registration.client_kind != crate::ClientKind::Mcp {
            return Err(ErrorCode::PolicyBlocked);
        }
        if !registration
            .scope
            .tools()
            .iter()
            .any(|tool| tool == "mail.events.read")
        {
            return Err(ErrorCode::ScopeDenied);
        }
        Ok(registration)
    }

    fn consented(
        tx: &Transaction<'_>,
        registration: &crate::RegistrationRecord,
        account: &str,
        epoch: u64,
        tool: &str,
    ) -> Result<(), ErrorCode> {
        if !registration.scope.permits_account(account, tool) {
            return Err(ErrorCode::ScopeDenied);
        }
        let ok: bool = db(tx.query_row("SELECT EXISTS(SELECT 1 FROM mcp_ai_consents consent JOIN client_registrations current ON current.registration_id=consent.registration_id WHERE consent.registration_id=?1 AND consent.account_id=?2 AND consent.profile_epoch=?3 AND consent.registration_request_id=current.request_id AND consent.approved_at_utc_ms=current.approved_at_utc_ms)", params![registration.registration_id.as_slice(), account, epoch as i64], |r| r.get(0)))?;
        if !ok {
            return Err(ErrorCode::PolicyBlocked);
        }
        Ok(())
    }

    const CURSOR_RETENTION_MS: i64 = RETENTION_MS;

    fn scope_hash(targets: &[MailEventTrackingTarget]) -> Result<[u8; 32], ErrorCode> {
        let mut raw = Vec::new();
        for target in targets {
            raw.extend_from_slice(target.target.account_id.as_bytes());
            raw.push(0);
            raw.extend_from_slice(target.target.mailbox_key.as_bytes());
            raw.push(0);
            raw.extend_from_slice(target.target.mailbox_encoding.as_bytes());
            raw.push(0xff);
        }
        hantong_platform::sha256(&raw).map_err(|_| ErrorCode::InternalError)
    }

    fn issue_cursor(
        tx: &Transaction<'_>,
        owner: &[u8; 16],
        epoch: u64,
        targets: &[MailEventTrackingTarget],
        event_id: u64,
        now: i64,
    ) -> Result<String, ErrorCode> {
        if epoch == 0 || epoch > i64::MAX as u64 || event_id > i64::MAX as u64 {
            return Err(ErrorCode::InvalidInput);
        }
        let token = hantong_platform::random_nonce()?;
        let hash = scope_hash(targets)?;
        let existing: Option<Vec<u8>> = db(tx.query_row(
            "SELECT cursor_token FROM mail_event_cursors WHERE owner_registration_id=?1 AND profile_epoch=?2 AND scope_hash=?3 AND event_id=?4 AND expires_at_utc_ms>?5",
            params![owner.as_slice(), epoch as i64, hash.as_slice(), event_id as i64, now],
            |row| row.get(0),
        ).optional())?;
        if let Some(token) = existing {
            return Ok(hantong_contracts::wire_hex::encode(&token));
        }
        db(tx.execute(
            "INSERT INTO mail_event_cursors(cursor_token,owner_registration_id,profile_epoch,scope_hash,event_id,issued_at_utc_ms,expires_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![token.as_slice(), owner.as_slice(), epoch as i64, hash.as_slice(), event_id as i64, now, now + CURSOR_RETENTION_MS],
        ))?;
        db(tx.execute(
            "DELETE FROM mail_event_cursors WHERE owner_registration_id=?1 AND profile_epoch=?2 AND expires_at_utc_ms<=?3",
            params![owner.as_slice(), epoch as i64, now],
        ))?;
        db(tx.execute(
            "DELETE FROM mail_event_cursors WHERE rowid IN (SELECT rowid FROM mail_event_cursors WHERE owner_registration_id=?1 AND profile_epoch=?2 ORDER BY issued_at_utc_ms DESC LIMIT -1 OFFSET 10000)",
            params![owner.as_slice(), epoch as i64],
        ))?;
        Ok(hantong_contracts::wire_hex::encode(&token))
    }

    fn parse_cursor(
        tx: &Transaction<'_>,
        owner: &[u8; 16],
        epoch: u64,
        targets: &[MailEventTrackingTarget],
        value: &str,
        now: i64,
    ) -> Result<u64, ErrorCode> {
        let token = hantong_contracts::wire_hex::decode_fixed::<32>(value)
            .map_err(|_| ErrorCode::CursorExpired)?;
        let expected_scope = scope_hash(targets)?;
        let row: Option<(Vec<u8>, i64, i64)> = db(tx.query_row(
            "SELECT scope_hash,event_id,expires_at_utc_ms FROM mail_event_cursors WHERE cursor_token=?1 AND owner_registration_id=?2 AND profile_epoch=?3",
            params![token.as_slice(), owner, epoch as i64],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).optional())?;
        let (scope, event_id, expires) = row.ok_or(ErrorCode::CursorExpired)?;
        if scope.as_slice() != expected_scope || expires <= now || event_id < 0 {
            return Err(ErrorCode::CursorExpired);
        }
        let max: i64 = db(tx.query_row(
            "SELECT COALESCE(MAX(event_id),0) FROM mail_event_log",
            [],
            |r| r.get(0),
        ))?;
        let exists: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM mail_event_log WHERE event_id=?1)",
            [event_id],
            |r| r.get(0),
        ))?;
        if event_id > max || (event_id > 0 && !exists) {
            return Err(ErrorCode::CursorExpired);
        }
        Ok(event_id as u64)
    }

    fn tx<T>(
        store: &Store,
        f: impl FnOnce(&Transaction<'_>) -> Result<T, ErrorCode>,
    ) -> Result<T, ErrorCode> {
        store.with_connection(|connection| {
            let transaction =
                Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
            let result = f(&transaction);
            if result.is_ok() {
                transaction.commit()?;
            }
            Ok(result)
        })?
    }

    fn allowed_accounts(registration: &crate::RegistrationRecord) -> Vec<String> {
        registration.scope.accounts().to_vec()
    }

    pub(super) fn configure(
        store: &Store,
        owner: [u8; 16],
        epoch: u64,
        enabled: bool,
    ) -> Result<Vec<MailEventTrackingTarget>, ErrorCode> {
        tx(store, |tx| {
            let registration = auth(tx, &owner, epoch)?;
            let accounts = allowed_accounts(&registration);
            if accounts.is_empty() {
                return Err(ErrorCode::ScopeDenied);
            }
            let now = crate::utc_millis().map_err(|_| ErrorCode::InternalError)?;
            if enabled {
                for account in accounts.iter().take(129) {
                    consented(tx, &registration, account, epoch, "mail.events.read")?;
                    let ready: bool = db(tx.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1 AND provider IN ('gmail','imap') AND status='ready')", [account], |r| r.get(0)))?;
                    if !ready {
                        continue;
                    }
                    let max: i64 = db(tx.query_row(
                        "SELECT COALESCE(MAX(event_id),0) FROM mail_event_log WHERE account_id=?1",
                        [account],
                        |r| r.get(0),
                    ))?;
                    let already_enabled: bool = db(tx.query_row("SELECT EXISTS(SELECT 1 FROM mail_event_tracking WHERE owner_registration_id=?1 AND profile_epoch=?2 AND account_id=?3 AND enabled=1)", params![owner.as_slice(),epoch as i64,account], |r| r.get(0)))?;
                    if !already_enabled {
                        // Fresh opt-in starts at the current state; another host's repeat subscribe is harmless.
                        db(tx.execute("DELETE FROM mail_event_sources WHERE account_id=?1 AND NOT EXISTS(SELECT 1 FROM mail_event_tracking t JOIN client_registrations r ON r.registration_id=t.owner_registration_id AND r.profile_epoch=t.profile_epoch AND r.state='active' WHERE t.account_id=?1 AND t.enabled=1)", [account]))?;
                        db(tx.execute("DELETE FROM mail_event_cursors WHERE owner_registration_id=?1 AND profile_epoch=?2", params![owner.as_slice(),epoch as i64]))?;
                    }
                    db(tx.execute("INSERT INTO mail_event_tracking(owner_registration_id,profile_epoch,account_id,mailbox_key,mailbox_encoding,baseline_event_id,acknowledged_event_id,enabled,updated_at_utc_ms) VALUES(?1,?2,?3,'INBOX','utf8',?4,?4,1,?5) ON CONFLICT(owner_registration_id,profile_epoch,account_id) DO UPDATE SET baseline_event_id=CASE WHEN enabled=0 THEN excluded.baseline_event_id ELSE baseline_event_id END, acknowledged_event_id=CASE WHEN enabled=0 THEN excluded.acknowledged_event_id ELSE acknowledged_event_id END,enabled=1,updated_at_utc_ms=excluded.updated_at_utc_ms", params![owner.as_slice(), epoch as i64, account, max, now]))?;
                }
            } else {
                db(tx.execute("UPDATE mail_event_tracking SET enabled=0,updated_at_utc_ms=?3 WHERE owner_registration_id=?1 AND profile_epoch=?2", params![owner.as_slice(), epoch as i64, now]))?;
            }
            read_targets(tx, &owner, epoch)
        })
    }

    fn read_targets(
        tx: &Transaction<'_>,
        owner: &[u8; 16],
        epoch: u64,
    ) -> Result<Vec<MailEventTrackingTarget>, ErrorCode> {
        let mut statement = db(tx.prepare("SELECT account_id,mailbox_key,mailbox_encoding,baseline_event_id,acknowledged_event_id FROM mail_event_tracking WHERE owner_registration_id=?1 AND profile_epoch=?2 AND enabled=1 ORDER BY account_id"))?;
        let rows = db(
            statement.query_map(params![owner.as_slice(), epoch as i64], |r| {
                Ok(MailEventTrackingTarget {
                    target: MailEventTarget {
                        account_id: text_column(r, 0, 128)?.to_owned(),
                        mailbox_key: text_column(r, 1, 512)?.to_owned(),
                        mailbox_encoding: text_column(r, 2, 32)?.to_owned(),
                    },
                    baseline_event_id: r.get::<_, i64>(3)? as u64,
                    acknowledged_event_id: r.get::<_, i64>(4)? as u64,
                })
            }),
        )?;
        db(rows.collect()).map_err(|_| ErrorCode::InternalError)
    }

    pub(super) fn list_targets(
        store: &Store,
        owner: [u8; 16],
        epoch: u64,
    ) -> Result<Vec<MailEventTrackingTarget>, ErrorCode> {
        tx(store, |tx| {
            let registration = auth(tx, &owner, epoch)?;
            let targets = read_targets(tx, &owner, epoch)?;
            for target in &targets {
                consented(
                    tx,
                    &registration,
                    &target.target.account_id,
                    epoch,
                    "mail.events.read",
                )?;
            }
            Ok(targets)
        })
    }

    pub(super) fn read(
        store: &Store,
        owner: [u8; 16],
        epoch: u64,
        after: Option<&str>,
        limit: u32,
        pending_only: bool,
    ) -> Result<MailEventPage, ErrorCode> {
        if !(1..=MAX_PAGE).contains(&limit) {
            return Err(ErrorCode::InvalidInput);
        }
        tx(store, |tx| {
            let registration = auth(tx, &owner, epoch)?;
            let targets = read_targets(tx, &owner, epoch)?;
            if targets.is_empty() {
                return Ok(MailEventPage {
                    events: Vec::new(),
                    next_cursor: None,
                    has_more: false,
                });
            }
            for target in &targets {
                consented(
                    tx,
                    &registration,
                    &target.target.account_id,
                    epoch,
                    "mail.events.read",
                )?;
            }
            let now = crate::utc_millis().map_err(|_| ErrorCode::InternalError)?;
            let after_seq = after
                .map(|value| parse_cursor(tx, &owner, epoch, &targets, value, now))
                .transpose()?
                .unwrap_or(0);
            db(tx.execute(
                "DELETE FROM mail_event_log WHERE created_at_utc_ms<?1",
                [now - RETENTION_MS],
            ))?;
            let sql = if pending_only {
                "SELECT e.event_id,e.account_id,e.mailbox_key,e.mailbox_encoding,e.change_kind FROM mail_event_log e JOIN mail_event_tracking t ON t.account_id=e.account_id AND t.owner_registration_id=?1 AND t.profile_epoch=?2 AND t.enabled=1 WHERE e.event_id>?3 AND e.event_id>t.acknowledged_event_id ORDER BY e.event_id LIMIT ?4"
            } else {
                "SELECT e.event_id,e.account_id,e.mailbox_key,e.mailbox_encoding,e.change_kind FROM mail_event_log e JOIN mail_event_tracking t ON t.account_id=e.account_id AND t.owner_registration_id=?1 AND t.profile_epoch=?2 AND t.enabled=1 WHERE e.event_id>?3 AND e.event_id>t.baseline_event_id ORDER BY e.event_id LIMIT ?4"
            };
            let mut statement = db(tx.prepare(sql))?;
            let rows = db(statement.query_map(
                params![
                    owner.as_slice(),
                    epoch as i64,
                    after_seq as i64,
                    i64::from(limit)
                ],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        text_column(r, 1, 128)?.to_owned(),
                        text_column(r, 2, 512)?.to_owned(),
                        text_column(r, 3, 32)?.to_owned(),
                        text_column(r, 4, 32)?.to_owned(),
                    ))
                },
            ))?;
            let raw: Vec<_> = db(rows.collect())?;
            let mut events = Vec::with_capacity(raw.len());
            for (id, account, mailbox, encoding, kind) in raw {
                events.push(MailEvent {
                    event_id: id as u64,
                    account_id: account,
                    mailbox_key: mailbox,
                    mailbox_encoding: encoding,
                    change_kind: kind,
                    cursor: String::new(),
                    profile_epoch: epoch,
                });
            }
            for event in &mut events {
                event.cursor = issue_cursor(tx, &owner, epoch, &targets, event.event_id, now)?;
            }
            let next_cursor = events
                .last()
                .map(|event| event.cursor.clone())
                .or_else(|| after.map(str::to_owned));
            Ok(MailEventPage {
                has_more: events.len() == limit as usize,
                events,
                next_cursor,
            })
        })
    }

    pub(super) fn ack(
        store: &Store,
        owner: [u8; 16],
        epoch: u64,
        value: &str,
    ) -> Result<(), ErrorCode> {
        tx(store, |tx| {
            let registration = auth(tx, &owner, epoch)?;
            let targets = read_targets(tx, &owner, epoch)?;
            for target in &targets {
                consented(
                    tx,
                    &registration,
                    &target.target.account_id,
                    epoch,
                    "mail.events.ack",
                )?;
            }
            let now = crate::utc_millis().map_err(|_| ErrorCode::InternalError)?;
            let seq = parse_cursor(tx, &owner, epoch, &targets, value, now)?;
            let changed = db(tx.execute("UPDATE mail_event_tracking SET acknowledged_event_id=CASE WHEN acknowledged_event_id<?3 THEN ?3 ELSE acknowledged_event_id END,updated_at_utc_ms=?4 WHERE owner_registration_id=?1 AND profile_epoch=?2 AND enabled=1", params![owner.as_slice(), epoch as i64, seq as i64, now]))?;
            if changed == 0 {
                return Err(ErrorCode::ScopeDenied);
            }
            Ok(())
        })
    }

    pub(super) fn commit_observation(
        store: &Store,
        owner: [u8; 16],
        epoch: u64,
        account: &str,
        mailbox_key: &str,
        mailbox_encoding: &str,
        observation_hash: [u8; 32],
    ) -> Result<Option<u64>, ErrorCode> {
        if account.is_empty()
            || account.len() > 128
            || mailbox_key.is_empty()
            || mailbox_key.len() > 512
            || mailbox_encoding.is_empty()
            || mailbox_encoding.len() > 32
        {
            return Err(ErrorCode::InvalidInput);
        }
        tx(store, |tx| {
            let registration = auth(tx, &owner, epoch)?;
            consented(tx, &registration, account, epoch, "mail.events.read")?;
            let enabled: bool = db(tx.query_row("SELECT EXISTS(SELECT 1 FROM mail_event_tracking WHERE owner_registration_id=?1 AND profile_epoch=?2 AND account_id=?3 AND mailbox_key=?4 AND mailbox_encoding=?5 AND enabled=1)", params![owner.as_slice(),epoch as i64,account,mailbox_key,mailbox_encoding],|r|r.get(0)))?;
            if !enabled {
                return Err(ErrorCode::ScopeDenied);
            }
            let ready: bool = db(tx.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1 AND provider IN ('gmail','imap') AND status='ready')", [account], |r| r.get(0)))?;
            if !ready {
                return Err(ErrorCode::NotFound);
            }
            let now = crate::utc_millis().map_err(|_| ErrorCode::InternalError)?;
            db(tx.execute(
                "DELETE FROM mail_event_log WHERE created_at_utc_ms<?1",
                [now - RETENTION_MS],
            ))?;
            let previous: Option<Vec<u8>> = db(tx.query_row("SELECT observation_hash FROM mail_event_sources WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3", params![account, mailbox_key, mailbox_encoding], |r| r.get(0)).optional())?;
            if previous
                .as_ref()
                .is_some_and(|hash| hash.as_slice() == observation_hash)
            {
                return Ok(None);
            }
            let baseline = previous.is_none();
            db(tx.execute("INSERT INTO mail_event_sources(account_id,mailbox_key,mailbox_encoding,observation_hash,baseline_established,observed_at_utc_ms) VALUES(?1,?2,?3,?4,1,?5) ON CONFLICT(account_id,mailbox_key,mailbox_encoding) DO UPDATE SET observation_hash=excluded.observation_hash,baseline_established=1,observed_at_utc_ms=excluded.observed_at_utc_ms", params![account, mailbox_key, mailbox_encoding, observation_hash.as_slice(), now]))?;
            if baseline {
                return Ok(None);
            }
            db(tx.execute("INSERT INTO mail_event_log(account_id,mailbox_key,mailbox_encoding,change_kind,created_at_utc_ms) VALUES(?1,?2,?3,?4,?5)", params![account, mailbox_key, mailbox_encoding, "mailbox_changed", now]))?;
            db(tx.execute("DELETE FROM mail_event_log WHERE account_id=?1 AND event_id NOT IN (SELECT event_id FROM mail_event_log WHERE account_id=?1 ORDER BY event_id DESC LIMIT ?2)", params![account, MAX_PER_ACCOUNT]))?;
            Ok(Some(tx.last_insert_rowid() as u64))
        })
    }
}

impl Store {
    pub fn configure_mail_event_tracking(
        &self,
        owner: [u8; 16],
        epoch: u64,
        enabled: bool,
    ) -> Result<Vec<MailEventTrackingTarget>, ErrorCode> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            storage::configure(self, owner, epoch, enabled)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = (owner, epoch, enabled);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn list_mail_event_tracking_targets(
        &self,
        owner: [u8; 16],
        epoch: u64,
    ) -> Result<Vec<MailEventTrackingTarget>, ErrorCode> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            storage::list_targets(self, owner, epoch)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = (owner, epoch);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn read_mail_events(
        &self,
        owner: [u8; 16],
        epoch: u64,
        cursor: Option<&str>,
        limit: u32,
        pending_only: bool,
    ) -> Result<MailEventPage, ErrorCode> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            let _ = pending_only;
            storage::read(self, owner, epoch, cursor, limit, pending_only)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = (owner, epoch, cursor, limit, pending_only);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn ack_mail_events(
        &self,
        owner: [u8; 16],
        epoch: u64,
        cursor: &str,
    ) -> Result<(), ErrorCode> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            storage::ack(self, owner, epoch, cursor)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = (owner, epoch, cursor);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn commit_mail_event_observation(
        &self,
        owner: [u8; 16],
        epoch: u64,
        account: &str,
        mailbox_key: &str,
        mailbox_encoding: &str,
        observation_hash: [u8; 32],
    ) -> Result<Option<u64>, ErrorCode> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            storage::commit_observation(
                self,
                owner,
                epoch,
                account,
                mailbox_key,
                mailbox_encoding,
                observation_hash,
            )
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = (
                owner,
                epoch,
                account,
                mailbox_key,
                mailbox_encoding,
                observation_hash,
            );
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}
