//! Native-only coordinator for draft save and single-message label changes.
use hantong_approval::GoogleMutationReview;
use hantong_contracts::{
    ErrorCode,
    audit::{AuditBegin, AuditTool},
    google_mutation::MutationPayload,
};
use hantong_policy::ReadPolicy;
use hantong_store::{
    Store,
    google_mutation::{MutationOutcome, MutationReceipt, PendingMutation},
};
use std::{cell::RefCell, path::Path, rc::Rc, time::Instant};
pub(crate) fn render(r: &MutationReceipt) -> serde_json::Value {
    serde_json::json!({"schema_version":1,"ok":true,"review_id":hantong_contracts::wire_hex::encode(&r.review_id),"state":r.state,"error_code":r.error.map(ErrorCode::as_str)})
}
fn review(p: &PendingMutation) -> GoogleMutationReview {
    GoogleMutationReview {
        review_id: p.review_id,
        owner_registration_id: p.owner_registration_id,
        profile_epoch: p.profile_epoch,
        account_version: p.account_version,
        binding: p.binding.clone(),
        sender: p.sender.clone(),
        request: p.request.clone(),
        compose: p.compose.clone(),
        before_labels: p.before_labels.clone(),
        created_at_utc_ms: p.created_at_utc_ms,
        expires_at_utc_ms: p.expires_at_utc_ms,
    }
}
pub(crate) async fn run_job(
    store: &Store,
    secrets_root: &Path,
    policy: &Rc<RefCell<ReadPolicy>>,
    id: [u8; 32],
    terminal: bool,
) -> Result<(), ErrorCode> {
    let p = match store.pending_google_mutation(id) {
        Ok(p) => p,
        Err(e) => {
            store.deny_google_mutation(id, e)?;
            return Err(e);
        }
    };
    if !terminal {
        let config = store.load_config()?.ok_or(ErrorCode::InternalError)?;
        let expected = review(&p);
        let authority = hantong_approval_windows::review_google_mutation(
            expected.clone(),
            config.gui.language,
            config.gui.theme,
        )
        .await
        .and_then(|v| v.consume(&expected));
        if let Err(e) = authority {
            store.deny_google_mutation(id, e)?;
            return Err(e);
        }
    }
    let root = secrets_root.to_path_buf();
    let captured = p.clone();
    let submit = async move {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        struct CancelOnDrop(Arc<AtomicBool>);
        impl Drop for CancelOnDrop {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let _cancel = CancelOnDrop(cancel.clone());
        match tokio::task::spawn_blocking(move || {
            let secrets =
                hantong_secrets::SecretStore::open(&root).map_err(|_| ErrorCode::InternalError)?;
            match &captured.request {
                MutationPayload::SaveDraft(_) => secrets.create_google_draft(
                    &captured.binding,
                    &captured.sender,
                    captured.compose.as_ref().ok_or(ErrorCode::InternalError)?,
                    &captured.review_id,
                    (captured.created_at_utc_ms / 1000) as u64,
                    &cancel,
                ),
                MutationPayload::Labels { change, .. } => {
                    secrets.change_google_labels(&captured.binding, change, &cancel)
                }
            }
        })
        .await
        {
            Ok(Ok(hantong_secrets::google::write::GmailWriteOutcome::Accepted)) => {
                MutationOutcome::Accepted
            }
            Ok(Ok(hantong_secrets::google::write::GmailWriteOutcome::Rejected)) | Ok(Err(_)) => {
                MutationOutcome::Rejected
            }
            _ => MutationOutcome::Unknown,
        }
    };
    execute_verified(store, policy, &p, submit).await
}
// Private: the only production caller has consumed the exact native OS proof.
async fn execute_verified(
    store: &Store,
    policy: &Rc<RefCell<ReadPolicy>>,
    p: &PendingMutation,
    submit: impl std::future::Future<Output = MutationOutcome>,
) -> Result<(), ErrorCode> {
    let audit = hantong_platform::random_nonce()?;
    store.begin_audit(&AuditBegin::new(
        audit,
        p.owner_registration_id,
        p.profile_epoch,
        AuditTool::GoogleMutationExecute,
        p.request.account_ids(),
    )?)?;
    let admission = (|| {
        if store.pending_google_mutation(p.review_id)? != *p {
            return Err(ErrorCode::Conflict);
        }
        let registration = store
            .load_registration(p.owner_registration_id)?
            .ok_or(ErrorCode::ClientNotRegistered)?;
        policy.borrow_mut().check_native_google_mutation(
            registration.registration_id,
            registration.read_only,
            registration.requests_per_minute,
            Instant::now(),
        )?;
        store.claim_google_mutation(p, audit)
    })();
    if let Err(e) = admission {
        store.fail_google_mutation_admission(p.review_id, audit, e)?;
        return Err(e);
    }
    tokio::pin!(submit);
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    let outcome = loop {
        tokio::select! {biased;
         _=interval.tick()=>{if store.validate_claimed_google_mutation(p).is_err(){break MutationOutcome::Unknown}},
         result=&mut submit=>{if store.validate_claimed_google_mutation(p).is_err(){break MutationOutcome::Unknown}break result},
        }
    };
    store.finish_google_mutation(p.review_id, outcome)
}

#[cfg(all(test, windows))]
#[path = "google_mutation_tests.rs"]
mod tests;
