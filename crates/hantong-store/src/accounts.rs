//! Bounded, read-only account display snapshots.

use crate::Store;
use hantong_contracts::{ErrorCode, auth::RequestedScope};
use hantong_domain::accounts::AccountSnapshot;

#[cfg(windows)]
use hantong_domain::accounts::{AccountProvider, AccountStatus, AccountSummary};

#[cfg(windows)]
use crate::registrations::text_column;

impl Store {
    /// Reads only the requested canonical accounts in one deferred snapshot.
    /// This is a display read, not an authentication or authorization proof.
    pub fn list_account_summaries(
        &self,
        account_ids: &[String],
        include_capabilities: bool,
    ) -> Result<AccountSnapshot, ErrorCode> {
        if account_ids.is_empty()
            || account_ids.len() > 128
            || account_ids.iter().any(|id| {
                id.is_empty()
                    || id.len() > 128
                    || !id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            })
            || RequestedScope::new(account_ids.to_vec(), Vec::new()).is_err()
        {
            return Err(ErrorCode::InvalidInput);
        }
        #[cfg(not(windows))]
        {
            let _ = include_capabilities;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.with_connection(|connection| {
                let tx = rusqlite::Transaction::new_unchecked(
                    connection,
                    rusqlite::TransactionBehavior::Deferred,
                )?;
                let placeholders = std::iter::repeat_n("?", account_ids.len())
                    .collect::<Vec<_>>()
                    .join(",");
                let query = format!(
                    "SELECT account_id,provider,primary_address,display_name,color,account_number,status,{}
                     FROM accounts WHERE account_id IN ({}) ORDER BY account_number,account_id",
                    if include_capabilities { "capabilities" } else { "NULL" },
                    placeholders
                );
                let mut rows = tx.prepare(&query)?;
                let result = rows
                    .query_map(rusqlite::params_from_iter(account_ids.iter()), |row| {
                        read_account(row, include_capabilities)
                    })
                    .and_then(|mapped| mapped.collect::<rusqlite::Result<Vec<_>>>())?;
                drop(rows);
                if result.len() != account_ids.len() {
                    return Ok(Err(ErrorCode::NotFound));
                }
                let snapshot = AccountSnapshot {
                    items: result,
                    observed_at_utc_ms: crate::utc_millis()?,
                };
                snapshot
                    .validate()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                tx.commit()?;
                Ok(Ok(snapshot))
            })?
        }
    }
}

#[cfg(windows)]
pub(super) fn read_account(
    row: &rusqlite::Row<'_>,
    include_capabilities: bool,
) -> rusqlite::Result<AccountSummary> {
    use rusqlite::types::ValueRef;
    let account_id = text_column(row, 0, 128)?.to_owned();
    let provider = AccountProvider::parse(text_column(row, 1, 16)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let primary_address = optional_text(row, 2, 1280)?;
    let display_name = optional_text(row, 3, 512)?;
    let color = optional_text(row, 4, 7)?;
    let account_number: i64 = row.get(5)?;
    if account_number <= 0 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let status = AccountStatus::parse(text_column(row, 6, 32)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let capabilities = match row.get_ref(7)? {
        ValueRef::Null if !include_capabilities => None,
        ValueRef::Null => return Err(rusqlite::Error::InvalidQuery),
        _ => {
            let raw = text_column(row, 7, 16 * 1024)?;
            let values: Vec<String> =
                serde_json::from_str(raw).map_err(|_| rusqlite::Error::InvalidQuery)?;
            let mut values = values;
            values.sort_unstable();
            Some(values)
        }
    };
    let summary = AccountSummary {
        account_id,
        provider,
        primary_address,
        display_name,
        color,
        account_number: account_number as u64,
        status,
        capabilities,
    };
    summary
        .validate()
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(summary)
}

#[cfg(windows)]
fn optional_text(
    row: &rusqlite::Row<'_>,
    index: usize,
    max_bytes: usize,
) -> rusqlite::Result<Option<String>> {
    use rusqlite::types::ValueRef;
    match row.get_ref(index)? {
        ValueRef::Null => Ok(None),
        _ => Ok(Some(text_column(row, index, max_bytes)?.to_owned())),
    }
}

#[cfg(test)]
mod tests;
