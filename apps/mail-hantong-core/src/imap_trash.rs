//! Core coordinator for one confirmed IMAP UID MOVE.
//!
//! The request is persisted before terminal confirmation. This module only
//! consumes a confirmed job: policy admission, audit creation, and the
//! single-use Store claim all happen before Secrets opens the provider
//! connection.

use hantong_contracts::{
    ErrorCode,
    audit::{AuditBegin, AuditTool},
    uid_page::{UidMailboxEncoding, UidMailboxIdentity},
};
use hantong_policy::ReadPolicy;
use hantong_store::{
    Store,
    imap_trash::{ImapTrashOutcome, ImapTrashReceipt, PendingImapTrash},
};
use std::{
    cell::RefCell,
    path::Path,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

pub(crate) fn render(receipt: &ImapTrashReceipt) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "provider": "imap",
        "ok": true,
        "account_id": receipt.account_id,
        "review_id": hantong_contracts::wire_hex::encode(&receipt.review_id),
        "state": receipt.state,
        "error_code": receipt.error.map(ErrorCode::as_str),
    })
}

fn destination(p: &PendingImapTrash) -> Result<UidMailboxIdentity, ErrorCode> {
    let encoding = match p.destination.mailbox_encoding.as_str() {
        "utf8" => UidMailboxEncoding::Utf8,
        "modified_utf7" => UidMailboxEncoding::ModifiedUtf7,
        _ => return Err(ErrorCode::InternalError),
    };
    UidMailboxIdentity::new(&p.destination.mailbox_wire_hex, encoding)
        .map_err(|_| ErrorCode::InternalError)
}

pub(crate) async fn run_job(
    store: &Store,
    secrets_root: &Path,
    policy: &Rc<RefCell<ReadPolicy>>,
    id: [u8; 32],
) -> Result<(), ErrorCode> {
    let pending = match store.pending_imap_trash(id) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    let captured = pending.clone();
    let root = secrets_root.to_path_buf();
    let submit = async move {
        let cancel = Arc::new(AtomicBool::new(false));
        struct CancelOnDrop(Arc<AtomicBool>);
        impl Drop for CancelOnDrop {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        let _cancel = CancelOnDrop(cancel.clone());
        let target = destination(&captured)?;
        let secrets =
            hantong_secrets::SecretStore::open(&root).map_err(|_| ErrorCode::InternalError)?;
        let outcome = secrets
            .move_imap_to_trash(
                captured.request.account_id(),
                &captured.endpoint,
                &captured.request,
                &target,
                &cancel,
            )
            .await
            .map_err(|_| ErrorCode::InternalError)?;
        Ok::<_, ErrorCode>(match outcome {
            hantong_secrets::ImapTrashOutcome::Accepted => ImapTrashOutcome::Moved,
            hantong_secrets::ImapTrashOutcome::Rejected => ImapTrashOutcome::Failed,
            hantong_secrets::ImapTrashOutcome::Unknown => ImapTrashOutcome::Unknown,
        })
    };
    execute_verified(store, policy, &pending, submit).await
}

async fn execute_verified(
    store: &Store,
    policy: &Rc<RefCell<ReadPolicy>>,
    pending: &PendingImapTrash,
    submit: impl std::future::Future<Output = Result<ImapTrashOutcome, ErrorCode>>,
) -> Result<(), ErrorCode> {
    let audit_id = hantong_platform::random_nonce()?;
    let begin = AuditBegin::new(
        audit_id,
        pending.owner_registration_id,
        pending.profile_epoch,
        AuditTool::TrashExecute,
        vec![pending.request.account_id().to_owned()],
    )?;
    let admission = (|| {
        let current = store.pending_imap_trash(pending.review_id)?;
        if current != *pending {
            return Err(ErrorCode::Conflict);
        }
        let registration = store
            .load_registration(pending.owner_registration_id)?
            .ok_or(ErrorCode::ClientNotRegistered)?;
        policy.borrow_mut().check_native_trash(
            registration.registration_id,
            registration.read_only,
            registration.requests_per_minute,
            Instant::now(),
        )?;
        store.begin_and_claim_imap_trash(pending, &begin)
    })();
    admission?;
    tokio::pin!(submit);
    let outcome = match submit.await {
        Ok(value) => value,
        Err(_) => ImapTrashOutcome::Unknown,
    };
    store.finish_imap_trash(pending.review_id, outcome)
}

#[cfg(test)]
mod tests;
