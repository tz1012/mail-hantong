//! Local observation summaries; no network work or synchronization side effects.
use crate::Store;
#[cfg(windows)]
use hantong_contracts::uid_inspection::UidObservationSummary;
use hantong_contracts::{
    ErrorCode, uid_inspection::UidInspectionSnapshot, uid_page::UidCursorRequest,
};
#[cfg(windows)]
use rusqlite::{TransactionBehavior, params};

impl Store {
    pub fn inspect_imap_uid(
        &self,
        request: &UidCursorRequest,
    ) -> Result<UidInspectionSnapshot, ErrorCode> {
        #[cfg(windows)]
        {
            crate::mail_connection::input_account(request.account_id())?;
            let mailbox = crate::uid_page_jobs::storage::mailbox(request.mailbox())?;
            self.mail_transaction(TransactionBehavior::Deferred, |tx| {
                let catalog = crate::imap_catalog::load_catalog(tx, request.account_id())?
                    .ok_or(ErrorCode::NotFound)?;
                if !catalog.snapshot().mailboxes().iter().any(|entry| {
                    entry.selectable()
                        && entry.name().wire_name() == mailbox.wire_name()
                        && entry.name().encoding() == mailbox.encoding()
                }) {
                    return Err(ErrorCode::NotFound);
                }
                let cursor = crate::imap_sync::storage::read_cursor(
                    tx,
                    request.account_id(),
                    &mailbox,
                )?;
                let (binding_current, observation) = if let Some(cursor) = cursor {
                    let value = cursor.value;
                    let epoch = crate::uid_epochs::read(
                        tx,
                        request.account_id(),
                        &mailbox,
                        value.uid_validity(),
                    )?
                    .ok_or(ErrorCode::InternalError)?;
                    if epoch.last.is_some() {
                        return Err(ErrorCode::InternalError);
                    }
                    let ceiling = value.uid_next_at_start().checked_sub(1)
                        .ok_or(ErrorCode::InternalError)?;
                    let observation = UidObservationSummary::new(
                        value.version(),
                        epoch.generation,
                        value.covered_through_uid(),
                        ceiling,
                        value.stored_at_utc_ms(),
                    )
                    .map_err(|_| ErrorCode::InternalError)?;
                    (catalog.binding_current() && value.binding_current(), Some(observation))
                } else {
                    // Each EXISTS uses the account/mailbox prefix of its existing index.
                    // Pending jobs alone are valid before the first observation.
                    let history: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM imap_uid_epochs WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3) OR EXISTS(SELECT 1 FROM imap_uid_page_operations WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3) OR EXISTS(SELECT 1 FROM imap_uid_messages WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3)",
                        params![request.account_id(), request.mailbox().wire_hex(), request.mailbox().encoding().as_str()],
                        |r| r.get(0),
                    ).map_err(|_| ErrorCode::InternalError)?;
                    if history {
                        return Err(ErrorCode::InternalError);
                    }
                    (catalog.binding_current(), None)
                };
                UidInspectionSnapshot::new(
                    request.account_id(),
                    request.mailbox().clone(),
                    catalog.generation(),
                    binding_current,
                    observation,
                ).map_err(|_| ErrorCode::InternalError)
            })
        }
        #[cfg(not(windows))]
        {
            let _ = request;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}

#[cfg(all(test, windows))]
mod tests;
