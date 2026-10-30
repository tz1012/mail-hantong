//! Fake local registrations and endpoints only; no native approval or SMTP.
use super::*;
use crate::mail_send::{PendingSend, SendOutcome};
use hantong_contracts::{
    audit::{AuditBegin, AuditTool},
    mail_send::{DraftCreateRequest, SendReviewRequest},
};

const OWNER: [u8; 16] = [71; 16];
fn fixture() -> Fixture {
    let f = Fixture::new();
    f.account("a");
    f.account("b");
    other_registration(&f, 70, 71, "fake-send-client", public_key());
    f.store().with_connection(|c|{
        c.execute("UPDATE client_registrations SET state='active',read_only=0,version=2 WHERE registration_id=?1",[OWNER.as_slice()])?;
        for account in ["a","b"] {c.execute("INSERT INTO client_registration_accounts(registration_id,account_id) VALUES(?1,?2)",params![OWNER.as_slice(),account])?;}
        for tool in ["mail.drafts.create","mail.send.request","mail.send.execute"] {c.execute("INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,?2)",params![OWNER.as_slice(),tool])?;}
        for kind in ["imap","smtp"] {c.execute("INSERT INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES('a',?1,1,'mail.example',465,'implicit_tls','sender@example.test','app_password',1)",[kind])?;}
        Ok(())
    }).unwrap();
    f
}
fn compose(operation: &str, body: &str) -> DraftCreateRequest {
    let r = DraftCreateRequest::new(
        "a",
        vec!["recipient@example.test".into()],
        "hello".into(),
        body.into(),
        operation,
        operation,
        0,
        [0; 32],
    )
    .unwrap();
    let hash = hantong_platform::sha256_storage_payload(&r.canonical_payload()).unwrap();
    DraftCreateRequest::new(
        "a",
        r.recipients().to_vec(),
        r.subject().into(),
        r.body().into(),
        operation,
        operation,
        0,
        hash,
    )
    .unwrap()
}
fn prepare(f: &Fixture) -> PendingSend {
    let draft = compose("draft-1", "full body");
    let id = f.store().create_mail_draft(OWNER, 1, &draft).unwrap();
    let r = SendReviewRequest::new("a", id, 1, "send-1", "send-1", *draft.payload_hash()).unwrap();
    let receipt = f.store().request_mail_send_review(OWNER, 1, &r).unwrap();
    f.store().pending_mail_send(receipt.review_id).unwrap()
}
fn audit(f: &Fixture, p: &PendingSend, id: u8, tool: AuditTool) -> [u8; 32] {
    let id = [id; 32];
    f.store()
        .begin_audit(
            &AuditBegin::new(id, OWNER, 1, tool, p.request.account_ids().to_vec()).unwrap(),
        )
        .unwrap();
    id
}

#[test]
fn apr01_send_cancellation_drift_and_policy_failures_keep_decodable_stable_receipts() {
    for (error, state) in [
        (ErrorCode::ApprovalDenied, "denied"),
        (ErrorCode::ApprovalRequired, "interrupted"),
        (ErrorCode::ApprovalExpired, "expired"),
        (ErrorCode::Conflict, "failed"),
        (ErrorCode::PolicyBlocked, "failed"),
        (ErrorCode::RateLimited, "failed"),
    ] {
        for admission in [false, true] {
            let f = fixture();
            let p = prepare(&f);
            if admission {
                let id = audit(&f, &p, 80, AuditTool::SendExecute);
                f.store()
                    .fail_mail_send_admission(p.review_id, id, error)
                    .unwrap();
            } else {
                f.store().deny_mail_send(p.review_id, error).unwrap();
            }
            let receipt = f
                .store()
                .request_mail_send_review(OWNER, 1, &p.request)
                .unwrap();
            assert_eq!(receipt.state, state);
            assert_eq!(receipt.error, Some(error));
            assert_eq!(count(f.store(), "mail_outbox"), 0);
        }
    }
}

#[test]
fn apr01_draft_hash_scope_and_idempotency_are_bound() {
    let f = fixture();
    let d = compose("draft-1", "body");
    let id = f.store().create_mail_draft(OWNER, 1, &d).unwrap();
    assert_eq!(f.store().create_mail_draft(OWNER, 1, &d), Ok(id));
    assert_eq!(
        f.store()
            .create_mail_draft(OWNER, 1, &compose("draft-1", "changed")),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store().create_mail_draft(OWNER, 2, &d),
        Err(ErrorCode::AuthRequired)
    );
    let forged = DraftCreateRequest::new(
        "a",
        d.recipients().to_vec(),
        d.subject().into(),
        d.body().into(),
        "new",
        "new",
        0,
        [7; 32],
    )
    .unwrap();
    assert_eq!(
        f.store().create_mail_draft(OWNER, 1, &forged),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(count(f.store(), "mail_drafts"), 1);
}
#[test]
fn sync07_claim_commits_once_and_replay_after_recovery_never_resubmits() {
    let mut f = fixture();
    let p = prepare(&f);
    let a = audit(&f, &p, 1, AuditTool::SendExecute);
    f.store().claim_mail_send(&p, a).unwrap();
    assert_eq!(f.store().claim_mail_send(&p, a), Err(ErrorCode::Conflict));
    assert_eq!(
        f.store()
            .request_mail_send_review(OWNER, 1, &p.request)
            .unwrap()
            .state,
        "sending"
    );
    f.reopen();
    assert_eq!(f.store().recover_mail_sends(), Ok(1));
    assert_eq!(f.store().recover_mail_sends(), Ok(0));
    let receipt = f
        .store()
        .request_mail_send_review(OWNER, 1, &p.request)
        .unwrap();
    assert_eq!(receipt.state, "unknown");
    assert_eq!(receipt.error, Some(ErrorCode::SendUnknown));
    assert_eq!(
        f.store()
            .load_audit(a)
            .unwrap()
            .unwrap()
            .completion()
            .unwrap()
            .terminal()
            .error(),
        Some(ErrorCode::SendUnknown)
    );
    assert!(f.store().pending_mail_send(p.review_id).is_err());
    assert_eq!(count(f.store(), "mail_outbox"), 1);
}
#[test]
fn apr01_claim_requires_exact_execute_audit_and_current_endpoint() {
    let f = fixture();
    let p = prepare(&f);
    let wrong = audit(&f, &p, 2, AuditTool::SendRequest);
    assert_eq!(
        f.store().claim_mail_send(&p, wrong),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(count(f.store(), "mail_outbox"), 0);
    let a = audit(&f, &p, 3, AuditTool::SendExecute);
    f.store()
        .with_connection(|c| {
            c.execute(
                "UPDATE mail_connection_settings SET settings_version=2 WHERE endpoint_kind='smtp'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(f.store().claim_mail_send(&p, a), Err(ErrorCode::Conflict));
    assert_eq!(count(f.store(), "mail_outbox"), 0);
    // An old review can still be inspected, without substituting settings.
    assert_eq!(
        f.store()
            .mail_send_receipt(OWNER, 1, "a", p.review_id)
            .unwrap()
            .state,
        "pending"
    );
}
#[test]
fn apr01_revocation_and_expiration_prevent_submission() {
    let f = fixture();
    let p = prepare(&f);
    f.store().with_connection(|c|{c.execute("UPDATE client_registrations SET state='revoked',revoked_at_utc_ms=updated_at_utc_ms,version=3 WHERE registration_id=?1",[OWNER.as_slice()])?;Ok(())}).unwrap();
    assert!(matches!(
        f.store().pending_mail_send(p.review_id),
        Err(ErrorCode::ClientSuspended)
    ));
    assert_eq!(count(f.store(), "mail_outbox"), 0);
    let g = fixture();
    let p = prepare(&g);
    g.store().with_connection(|c|{let now=crate::utc_millis()?;c.execute("UPDATE mail_send_reviews SET created_at_utc_ms=?1,expires_at_utc_ms=?2 WHERE review_id=?3",params![now-300001,now-1,p.review_id.as_slice()])?;Ok(())}).unwrap();
    assert!(matches!(
        g.store().pending_mail_send(p.review_id),
        Err(ErrorCode::ApprovalExpired)
    ));
    assert_eq!(
        g.store()
            .request_mail_send_review(OWNER, 1, &p.request)
            .unwrap()
            .state,
        "expired"
    );
}
#[test]
fn sync07_outbox_terminal_and_audit_terminal_commit_atomically() {
    let f = fixture();
    let p = prepare(&f);
    let a = audit(&f, &p, 4, AuditTool::SendExecute);
    f.store().claim_mail_send(&p, a).unwrap();
    f.store().with_connection(|c|c.execute_batch("CREATE TRIGGER fake_ignore_terminal BEFORE INSERT ON audit_terminals BEGIN SELECT RAISE(IGNORE); END;")).unwrap();
    assert!(
        f.store()
            .finish_mail_send(p.review_id, SendOutcome::Sent)
            .is_err()
    );
    assert_eq!(
        f.store()
            .mail_send_receipt(OWNER, 1, "a", p.review_id)
            .unwrap()
            .state,
        "sending"
    );
    assert!(
        f.store()
            .load_audit(a)
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
    f.store()
        .with_connection(|c| c.execute_batch("DROP TRIGGER fake_ignore_terminal;"))
        .unwrap();
    f.store()
        .finish_mail_send(p.review_id, SendOutcome::Sent)
        .unwrap();
    assert_eq!(
        f.store()
            .mail_send_receipt(OWNER, 1, "a", p.review_id)
            .unwrap()
            .state,
        "sent"
    );
    assert!(
        f.store()
            .load_audit(a)
            .unwrap()
            .unwrap()
            .completion()
            .unwrap()
            .terminal()
            .is_success()
    );
    assert_eq!(
        f.store()
            .finish_mail_send(p.review_id, SendOutcome::Unknown),
        Err(ErrorCode::Conflict)
    );
}

#[test]
fn sync07_ambiguous_committed_claim_cannot_detach_its_audit() {
    let f = fixture();
    let p = prepare(&f);
    let a = audit(&f, &p, 5, AuditTool::SendExecute);
    f.store().claim_mail_send(&p, a).unwrap();
    // Model an error observed by the caller after the claim actually committed.
    f.store()
        .fail_mail_send_admission(p.review_id, a, ErrorCode::InternalError)
        .unwrap();
    let receipt = f
        .store()
        .mail_send_receipt(OWNER, 1, "a", p.review_id)
        .unwrap();
    assert_eq!(receipt.state, "unknown");
    assert_eq!(receipt.error, Some(ErrorCode::SendUnknown));
    assert_eq!(
        f.store()
            .load_audit(a)
            .unwrap()
            .unwrap()
            .completion()
            .unwrap()
            .terminal()
            .error(),
        Some(ErrorCode::SendUnknown)
    );
    assert_eq!(f.store().recover_mail_sends(), Ok(0));
}
