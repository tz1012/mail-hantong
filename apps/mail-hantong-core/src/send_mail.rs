//! Native-only outgoing-mail coordinator; normal RPC cannot issue approval.
use hantong_approval::SendReview;
use hantong_contracts::{
    ErrorCode,
    audit::{AuditBegin, AuditTool},
};
use hantong_policy::ReadPolicy;
use hantong_store::{
    Store,
    mail_send::{PendingSend, SendOutcome, SendReceipt},
};
use std::{cell::RefCell, path::Path, rc::Rc, time::Instant};

pub(crate) fn render(r: &SendReceipt) -> serde_json::Value {
    serde_json::json!({"schema_version":1,"ok":true,"account_id":r.account_id,"review_id":hex(&r.review_id),"state":r.state,"error_code":r.error.map(ErrorCode::as_str)})
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 15) as usize] as char);
    }
    s
}
fn review(p: &PendingSend) -> SendReview {
    SendReview {
        review_id: p.review_id,
        owner_registration_id: p.owner_registration_id,
        profile_epoch: p.profile_epoch,
        account_version: p.account_version,
        endpoint: p.endpoint.clone(),
        draft: p.draft.clone(),
        request: p.request.clone(),
        created_at_utc_ms: p.created_at_utc_ms,
        expires_at_utc_ms: p.expires_at_utc_ms,
    }
}

pub(crate) async fn run_job(
    store: &Store,
    secrets_root: &Path,
    policy: &Rc<RefCell<ReadPolicy>>,
    id: [u8; 32],
) -> Result<(), ErrorCode> {
    let p = match store.pending_mail_send(id) {
        Ok(p) => p,
        Err(e) => {
            store.deny_mail_send(id, e)?;
            return Err(e);
        }
    };
    let config = store.load_config()?.ok_or(ErrorCode::InternalError)?;
    let expected = review(&p);
    let verified = hantong_approval_windows::review_send(
        expected.clone(),
        config.gui.language,
        config.gui.theme,
    )
    .await;
    let _authority = match verified.and_then(|v| v.consume(&expected)) {
        Ok(v) => v,
        Err(e) => {
            store.deny_mail_send(id, e)?;
            return Err(e);
        }
    };
    let submit = async {
        match hantong_secrets::SecretStore::open(secrets_root) {
            Err(_) => SendOutcome::Unknown,
            Ok(secrets) => {
                let envelope = hantong_secrets::SmtpSendEnvelope {
                    recipients: p.draft.recipients().to_vec(),
                    subject: p.draft.subject().into(),
                    body: p.draft.body().into(),
                };
                classify(
                    secrets
                        .send_smtp(p.request.account_id(), &p.endpoint, &envelope)
                        .await,
                )
            }
        }
    };
    execute_verified(store, policy, &p, submit).await
}
// Kept private to this module: the only product caller has just consumed the
// opaque native proof. A test may supply a synthetic SMTP future, never a proof.
async fn execute_verified(
    store: &Store,
    policy: &Rc<RefCell<ReadPolicy>>,
    p: &PendingSend,
    submit: impl std::future::Future<Output = SendOutcome>,
) -> Result<(), ErrorCode> {
    let id = p.review_id;
    // No effect before native verification, a fresh policy check, durable audit,
    // and the atomic single-use Sending claim.
    let audit_id = hantong_platform::random_nonce()?;
    store.begin_audit(&AuditBegin::new(
        audit_id,
        p.owner_registration_id,
        p.profile_epoch,
        AuditTool::SendExecute,
        p.request.account_ids().to_vec(),
    )?)?;
    let admission = (|| {
        let current = store.pending_mail_send(id)?;
        if current.request != p.request
            || current.draft != p.draft
            || current.endpoint != p.endpoint
            || current.account_version != p.account_version
        {
            return Err(ErrorCode::Conflict);
        }
        let registration = store
            .load_registration(p.owner_registration_id)?
            .ok_or(ErrorCode::ClientNotRegistered)?;
        policy.borrow_mut().check_native_send(
            registration.registration_id,
            registration.read_only,
            registration.requests_per_minute,
            Instant::now(),
        )?;
        store.claim_mail_send(p, audit_id)
    })();
    if let Err(e) = admission {
        store.fail_mail_send_admission(id, audit_id, e)?;
        return Err(e);
    }
    tokio::pin!(submit);
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    let outcome = loop {
        tokio::select! {biased;
            _=interval.tick()=>{if store.validate_claimed_mail_send(p).is_err(){break SendOutcome::Unknown}},
            result=&mut submit=>{if store.validate_claimed_mail_send(p).is_err(){break SendOutcome::Unknown}break result},
        }
    };

    store.finish_mail_send(id, outcome)
}
fn classify(
    result: Result<hantong_secrets::SmtpSendResult, hantong_secrets::SmtpSendError>,
) -> SendOutcome {
    match result {
        Ok(hantong_secrets::SmtpSendResult::Accepted) => SendOutcome::Sent,
        Ok(hantong_secrets::SmtpSendResult::Rejected) => SendOutcome::Failed,
        _ => SendOutcome::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sync07_uncertain_errors_never_become_retryable_success() {
        use hantong_secrets::{SmtpSendError as E, SmtpSendResult as R};
        assert_eq!(classify(Ok(R::Accepted)), SendOutcome::Sent);
        assert_eq!(classify(Ok(R::Rejected)), SendOutcome::Failed);
        assert_eq!(classify(Ok(R::Uncertain)), SendOutcome::Unknown);
        for e in [
            E::Connection,
            E::Tls,
            E::Authentication,
            E::Protocol,
            E::Timeout,
        ] {
            assert_eq!(classify(Err(e)), SendOutcome::Unknown)
        }
    }
}

#[cfg(test)]
#[path = "send_mail/tests.rs"]
mod integration_tests;
