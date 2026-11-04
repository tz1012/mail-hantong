//! Core event operations behind authenticated scope, policy and audit checks.
use hantong_application::{MailEventsAction, ReadResult};
use hantong_contracts::ErrorCode;
use hantong_store::Store;
use serde_json::{Value, json};

pub(crate) fn execute(
    store: &Store,
    owner: [u8; 16],
    epoch: u64,
    accounts: &[String],
    action: &MailEventsAction,
) -> Result<ReadResult<Value>, ErrorCode> {
    let registration = store
        .load_registration(owner)?
        .ok_or(ErrorCode::ClientNotRegistered)?;
    // A profile-wide cursor cannot be reused with a different account selection.
    if accounts.is_empty() || registration.scope.accounts() != accounts {
        return Err(ErrorCode::ScopeDenied);
    }
    match action {
        MailEventsAction::Read {
            cursor,
            limit,
            pending_only,
        } => {
            let page =
                store.read_mail_events(owner, epoch, cursor.as_deref(), *limit, *pending_only)?;
            let events: Vec<_> = page.events.into_iter().map(|event| json!({
                "event_id":event.event_id,"account_id":event.account_id,
                "mailbox_key":event.mailbox_key,"mailbox_encoding":event.mailbox_encoding,
                "change_kind":event.change_kind,"cursor":event.cursor,"profile_epoch":event.profile_epoch
            })).collect();
            let count = events.len() as u64;
            ReadResult::new(
                json!({"schema_version":1,"events":events,"next_cursor":page.next_cursor,"has_more":page.has_more}),
                Some(count),
            )
        }
        MailEventsAction::Ack { cursor } => {
            store.ack_mail_events(owner, epoch, cursor)?;
            ReadResult::new(json!({"schema_version":1,"acknowledged":true}), Some(0))
        }
        MailEventsAction::Configure { enabled } => {
            let targets = store.configure_mail_event_tracking(owner, epoch, *enabled)?;
            ReadResult::new(
                json!({"schema_version":1,"enabled":enabled,"tracked_accounts":targets.len()}),
                Some(targets.len() as u64),
            )
        }
    }
}
