//! Encrypted audit starts and immutable terminal records.

use crate::Store;
use hantong_contracts::{
    ErrorCode,
    audit::{AuditBegin, AuditRecord, AuditTerminal},
};

#[cfg(windows)]
use crate::registrations::{blob_column, read_registration, text_column};
#[cfg(windows)]
use hantong_contracts::{
    audit::AuditTool,
    client::{AuditActor, ClientKind},
};
#[cfg(windows)]
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};

impl Store {
    /// Commits the local start record. This is not authorization to perform an effect.
    pub fn begin_audit(&self, begin: &AuditBegin) -> Result<u64, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = begin;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.with_connection(|connection| {
                let tx = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
                let exists: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM audit_starts WHERE execution_id=?1)",
                    [begin.execution_id().as_slice()], |row| row.get(0),
                )?;
                if exists { return Ok(Err(ErrorCode::Conflict)); }
                let Some(registration) = read_registration(&tx, begin.registration_id())? else {
                    return Ok(Err(ErrorCode::ClientNotRegistered));
                };
                if registration.state != crate::RegistrationState::Active {
                    return Ok(Err(ErrorCode::ClientSuspended));
                }
                if registration.profile_epoch != begin.profile_epoch() {
                    return Ok(Err(ErrorCode::AuthRequired));
                }
                // Audit only canonical stored account identifiers; raw input is not a log field.
                for account in begin.account_ids() {
                    let known: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1)",
                        [account], |row| row.get(0),
                    )?;
                    if !known { return Ok(Err(ErrorCode::ScopeDenied)); }
                }
                let (executions, accounts): (i64, i64) = tx.query_row(
                    "SELECT retained_executions,retained_account_refs FROM audit_capacity WHERE singleton=1",
                    [], |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                if !(0..=100_000).contains(&executions) || !(0..=1_000_000).contains(&accounts) {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                let added = begin.account_ids().len() as i64;
                if executions >= 100_000 || accounts > 1_000_000 - added {
                    return Ok(Err(ErrorCode::QuotaExceeded));
                }
                let now = crate::utc_millis()?;
                tx.execute(
                    "INSERT INTO audit_starts(execution_id,registration_id,profile_epoch,tool_code,started_at_utc_ms,client_kind,display_label)
                     VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    rusqlite::params![begin.execution_id().as_slice(), begin.registration_id().as_slice(),
                        begin.profile_epoch() as i64, begin.tool().as_str(), now,
                        registration.client_kind.as_str(), registration.display_label],
                )?;
                let sequence = tx.last_insert_rowid();
                if sequence <= 0 { return Err(rusqlite::Error::InvalidQuery); }
                for account in begin.account_ids() {
                    tx.execute(
                        "INSERT INTO audit_start_accounts(execution_id,account_id) VALUES(?1,?2)",
                        rusqlite::params![begin.execution_id().as_slice(), account],
                    )?;
                }
                if tx.execute(
                    "UPDATE audit_capacity SET retained_executions=retained_executions+1,
                     retained_account_refs=retained_account_refs+?1 WHERE singleton=1",
                    [added],
                )? != 1 { return Err(rusqlite::Error::InvalidQuery); }
                tx.commit()?;
                Ok(Ok(sequence as u64))
            })?
        }
    }

    /// Records the actual service outcome once. Failure never authorizes a retry.
    pub fn finish_audit(
        &self,
        execution_id: [u8; 32],
        terminal: AuditTerminal,
    ) -> Result<(), ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = (execution_id, terminal);
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.with_connection(|connection| {
                let tx = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
                let result =
                    insert_audit_terminal_tx(&tx, execution_id, terminal, crate::utc_millis()?);
                if result.is_ok() {
                    tx.commit()?;
                }
                Ok(result)
            })?
        }
    }

    pub fn load_audit(&self, execution_id: [u8; 32]) -> Result<Option<AuditRecord>, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = execution_id;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.with_connection(|connection| {
                let tx = Transaction::new_unchecked(connection, TransactionBehavior::Deferred)?;
                let result = read_audit(&tx, &execution_id)?;
                tx.commit()?;
                Ok(result)
            })
        }
    }

    /// Internal bounded history lookup; callers must enforce access policy before displaying it.
    pub fn list_audit_page(
        &self,
        before_sequence: Option<u64>,
        limit: u32,
    ) -> Result<Vec<AuditRecord>, ErrorCode> {
        if !(1..=100).contains(&limit)
            || before_sequence.is_some_and(|value| value == 0 || value > i64::MAX as u64)
        {
            return Err(ErrorCode::InvalidInput);
        }
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.with_connection(|connection| {
                let tx = Transaction::new_unchecked(connection, TransactionBehavior::Deferred)?;
                let mut statement = tx.prepare(
                    "SELECT execution_id FROM audit_starts WHERE (?1 IS NULL OR audit_seq<?1)
                     ORDER BY audit_seq DESC LIMIT ?2",
                )?;
                let ids = statement
                    .query_map(
                        rusqlite::params![before_sequence.map(|v| v as i64), i64::from(limit)],
                        |row| blob_column::<32>(row, 0),
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                drop(statement);
                let records = ids
                    .iter()
                    .map(|id| read_audit(&tx, id)?.ok_or(rusqlite::Error::InvalidQuery))
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                tx.commit()?;
                Ok(records)
            })
        }
    }
}

#[cfg(windows)]
pub(super) fn read_audit(
    connection: &rusqlite::Connection,
    id: &[u8; 32],
) -> rusqlite::Result<Option<AuditRecord>> {
    let metadata = connection
        .query_row(
            "SELECT audit_seq,registration_id,profile_epoch,tool_code,started_at_utc_ms,
         EXISTS(SELECT 1 FROM client_registrations r WHERE r.registration_id=s.registration_id),client_kind,display_label
         FROM audit_starts s WHERE execution_id=?1",
            [id.as_slice()],
            |row| {
                if !row.get::<_, bool>(5)? {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                let sequence: i64 = row.get(0)?;
                let registration = blob_column::<16>(row, 1)?;
                let epoch: i64 = row.get(2)?;
                let tool = AuditTool::parse(text_column(row, 3, 96)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                let started: i64 = row.get(4)?;
                let actor = AuditActor::new(
                    ClientKind::parse(text_column(row, 6, 3)?).map_err(|_| rusqlite::Error::InvalidQuery)?,
                    text_column(row, 7, 512)?.to_owned(),
                ).map_err(|_| rusqlite::Error::InvalidQuery)?;
                if sequence <= 0 || epoch <= 0 || started <= 0 {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                Ok((sequence as u64, registration, epoch as u64, tool, actor, started))
            },
        )
        .optional()?;
    let Some((sequence, registration, epoch, tool, actor, started)) = metadata else {
        return Ok(None);
    };
    let mut statement = connection.prepare(
        "SELECT account_id FROM audit_start_accounts WHERE execution_id=?1 ORDER BY account_id LIMIT 129",
    )?;
    let accounts = statement
        .query_map([id.as_slice()], |row| {
            Ok(text_column(row, 0, 128)?.to_owned())
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let begin = AuditBegin::new(*id, registration, epoch, tool, accounts)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let record = AuditRecord::begin(sequence, &begin, actor, started)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let completion = connection
        .query_row(
            "SELECT completed_at_utc_ms,result_kind,error_code,result_count
         FROM audit_terminals WHERE execution_id=?1",
            [id.as_slice()],
            |row| {
                let completed: i64 = row.get(0)?;
                let kind = text_column(row, 1, 14)?;
                let error = match row.get_ref(2)? {
                    rusqlite::types::ValueRef::Null => None,
                    _ => Some(text_column(row, 2, 32)?),
                };
                let count: Option<i64> = row.get(3)?;
                let terminal = match (kind, error, count) {
                    ("returned_ok", None, count) => AuditTerminal::succeeded(
                        count
                            .map(u64::try_from)
                            .transpose()
                            .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    )
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    ("returned_error", Some(error), None) => AuditTerminal::failed(
                        ErrorCode::ALL
                            .into_iter()
                            .find(|value| value.as_str() == error)
                            .ok_or(rusqlite::Error::InvalidQuery)?,
                    ),
                    _ => return Err(rusqlite::Error::InvalidQuery),
                };
                Ok((completed, terminal))
            },
        )
        .optional()?;
    match completion {
        None => Ok(Some(record)),
        Some((completed, terminal)) => record
            .finish(completed, terminal)
            .map(Some)
            .map_err(|_| rusqlite::Error::InvalidQuery),
    }
}

/// Shared transaction boundary for a job result and its immutable audit terminal.
#[cfg(windows)]
pub(super) fn insert_audit_terminal_tx(
    tx: &rusqlite::Connection,
    execution_id: [u8; 32],
    terminal: AuditTerminal,
    completed: i64,
) -> Result<(), ErrorCode> {
    let record = read_audit(tx, &execution_id)
        .map_err(|_| ErrorCode::InternalError)?
        .ok_or(ErrorCode::NotFound)?;
    if record.completion().is_some() {
        return Err(ErrorCode::Conflict);
    }
    if completed < record.started_at_utc_ms() {
        return Err(ErrorCode::InternalError);
    }
    let count = terminal
        .result_count()
        .map(i64::try_from)
        .transpose()
        .map_err(|_| ErrorCode::InternalError)?;
    tx.execute("INSERT INTO audit_terminals(execution_id,completed_at_utc_ms,result_kind,error_code,result_count) VALUES(?1,?2,?3,?4,?5)",rusqlite::params![execution_id.as_slice(),completed,if terminal.is_success(){"returned_ok"}else{"returned_error"},terminal.error().map(ErrorCode::as_str),count]).map_err(|_|ErrorCode::InternalError)?;
    Ok(())
}

#[cfg(test)]
mod tests;
