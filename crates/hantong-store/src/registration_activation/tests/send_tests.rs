//! Fake local registrations and endpoints only; no native approval or SMTP.
use super::*;
use crate::mail_send::{PendingSend, SendOutcome};
use hantong_contracts::{
    audit::{AuditBegin, AuditTool},
    mail_send::{DraftCreateRequest, SendReviewRequest},
};
#[cfg(windows)]
use hantong_contracts::{google::GoogleIdentity, google_write::GoogleAccessMode};

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
    compose_for_account("a", operation, body)
}
#[cfg(windows)]
fn compose_for_account(account: &str, operation: &str, body: &str) -> DraftCreateRequest {
    let r = DraftCreateRequest::new(
        account,
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
        account,
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

#[cfg(windows)]
fn personal_identity(subject: &str) -> GoogleIdentity {
    GoogleIdentity {
        access_mode: GoogleAccessMode::ReadSend,
        account_id: format!(
            "google_{}",
            hantong_contracts::wire_hex::encode(
                &hantong_platform::sha256(subject.as_bytes()).unwrap()
            )
        ),
        subject: subject.into(),
        email: "personal@example.test".into(),
        granted_scopes: GoogleAccessMode::ReadSend
            .required_scopes()
            .iter()
            .map(|scope| (*scope).into())
            .collect(),
    }
}

#[cfg(windows)]
fn personal_fixture(body_first: bool) -> (Fixture, String, PendingSend) {
    let f = Fixture::new();
    let identity = personal_identity("personal-send-fixture");
    let account = identity.account_id.clone();
    f.store()
        .activate_google_account(&identity, "fixture.apps.googleusercontent.com", 0, None)
        .unwrap();
    let request = f.request(1, &[&account]);
    let registration_id =
        hantong_platform::registration_id_for_request(request.request_id).unwrap();
    let stage = CliRegistrationStage::new_google_oauth(
        request,
        registration_id,
        public_key(),
        server_hash(),
    )
    .unwrap();
    let record = f
        .store()
        .stage_google_oauth_cli_registration(&stage)
        .unwrap();
    f.store()
        .activate_cli_registration(registration_id, record.version, server_hash())
        .unwrap();

    if body_first {
        let body = RequestedScope::new(
            vec![account.clone()],
            hantong_contracts::auth::default_cli_body_tools(),
        )
        .unwrap();
        let refresh = f
            .store()
            .create_google_oauth_cli_scope_refresh([2; 32], body, server_hash())
            .unwrap();
        let staged = f.store().stage_cli_scope_refresh(&refresh).unwrap();
        f.store().activate_cli_scope_refresh(&staged).unwrap();
        let send = RequestedScope::new(
            vec![account.clone()],
            hantong_contracts::auth::default_cli_send_tools(),
        )
        .unwrap();
        let refresh = f
            .store()
            .create_google_oauth_cli_send_upgrade([3; 32], send, server_hash())
            .unwrap();
        let staged = f.store().stage_cli_scope_refresh(&refresh).unwrap();
        f.store().activate_cli_scope_refresh(&staged).unwrap();
    } else {
        let send = RequestedScope::new(
            vec![account.clone()],
            hantong_contracts::auth::default_cli_send_tools(),
        )
        .unwrap();
        let refresh = f
            .store()
            .create_google_oauth_cli_scope_refresh([2; 32], send, server_hash())
            .unwrap();
        let staged = f.store().stage_cli_scope_refresh(&refresh).unwrap();
        f.store().activate_cli_scope_refresh(&staged).unwrap();
    }

    let draft = DraftCreateRequest::new(
        &account,
        vec!["recipient@example.test".into()],
        "personal send".into(),
        "complete body".into(),
        if body_first {
            "personal-draft-b"
        } else {
            "personal-draft-c"
        },
        if body_first {
            "personal-draft-b"
        } else {
            "personal-draft-c"
        },
        0,
        [0; 32],
    )
    .unwrap();
    let payload_hash =
        hantong_platform::sha256_storage_payload(&draft.canonical_payload()).unwrap();
    let draft = DraftCreateRequest::new(
        &account,
        draft.recipients().to_vec(),
        draft.subject().into(),
        draft.body().into(),
        draft.operation_id(),
        draft.idempotency_key(),
        0,
        payload_hash,
    )
    .unwrap();
    let registration = f.store().load_default_cli_registration().unwrap().unwrap();
    let draft_id = f
        .store()
        .create_mail_draft(
            registration.registration_id,
            registration.profile_epoch,
            &draft,
        )
        .unwrap();
    let request = SendReviewRequest::new(
        &account,
        draft_id,
        1,
        if body_first {
            "personal-send-b"
        } else {
            "personal-send-c"
        },
        if body_first {
            "personal-send-b"
        } else {
            "personal-send-c"
        },
        *draft.payload_hash(),
    )
    .unwrap();
    let receipt = f
        .store()
        .request_mail_send_review(
            registration.registration_id,
            registration.profile_epoch,
            &request,
        )
        .unwrap();
    let pending = f.store().pending_mail_send(receipt.review_id).unwrap();
    (f, account, pending)
}

#[cfg(windows)]
fn personal_audit(f: &Fixture, p: &PendingSend, id: u8) -> [u8; 32] {
    let audit_id = [id; 32];
    f.store()
        .begin_audit(
            &AuditBegin::new(
                audit_id,
                p.owner_registration_id,
                p.profile_epoch,
                AuditTool::SendExecute,
                p.request.account_ids().to_vec(),
            )
            .unwrap(),
        )
        .unwrap();
    audit_id
}

#[cfg(windows)]
fn personal_pending_for_account(f: &Fixture, account: &str, operation: &str) -> PendingSend {
    let registration = f.store().load_default_cli_registration().unwrap().unwrap();
    let draft = compose_for_account(account, operation, "complete body");
    let draft_id = f
        .store()
        .create_mail_draft(
            registration.registration_id,
            registration.profile_epoch,
            &draft,
        )
        .unwrap();
    let request = SendReviewRequest::new(
        account,
        draft_id,
        1,
        operation,
        operation,
        *draft.payload_hash(),
    )
    .unwrap();
    let receipt = f
        .store()
        .request_mail_send_review(
            registration.registration_id,
            registration.profile_epoch,
            &request,
        )
        .unwrap();
    f.store().pending_mail_send(receipt.review_id).unwrap()
}

#[cfg(windows)]
#[test]
fn apr01_personal_google_send_allows_initial_and_upgraded_epochs() {
    for (body_first, audit_id) in [(false, 91), (true, 92)] {
        let (f, _account, pending) = personal_fixture(body_first);
        assert!(f.store().can_confirm_personal_mail_send(&pending).unwrap());
        let audit_id = personal_audit(&f, &pending, audit_id);
        f.store()
            .claim_personal_mail_send(&pending, audit_id)
            .unwrap();
        assert_eq!(count(f.store(), "mail_outbox"), 1);
    }
}

#[cfg(windows)]
#[test]
fn apr01_personal_google_send_rejects_wrong_provenance_and_scope() {
    for case in 0..7 {
        let (f, _account, pending) = personal_fixture(false);
        f.store().with_connection(|c| {
            match case {
                0 => {
                    c.execute(
                        "UPDATE client_registration_requests SET decision_channel='approval_helper' WHERE request_id=(SELECT request_id FROM client_registrations WHERE registration_id=?1)",
                        [pending.owner_registration_id.as_slice()],
                    )?;
                    c.execute(
                        "UPDATE client_registrations SET approved_channel='approval_helper' WHERE registration_id=?1",
                        [pending.owner_registration_id.as_slice()],
                    )?;
                }
                1 => {
                    c.execute(
                        "UPDATE client_registrations SET read_only=1 WHERE registration_id=?1",
                        [pending.owner_registration_id.as_slice()],
                    )?;
                }
                2 => {
                    c.execute(
                        "DELETE FROM client_registration_tools WHERE registration_id=?1",
                        [pending.owner_registration_id.as_slice()],
                    )?;
                }
                3 => {
                    c.execute(
                        "UPDATE client_registrations SET display_label='other-cli' WHERE registration_id=?1",
                        [pending.owner_registration_id.as_slice()],
                    )?;
                }
                4 => {
                    c.execute(
                        "UPDATE client_registrations SET client_kind='mcp' WHERE registration_id=?1",
                        [pending.owner_registration_id.as_slice()],
                    )?;
                }
                5 => {
                    c.execute(
                        "UPDATE client_registrations SET profile_epoch=profile_epoch+1 WHERE registration_id=?1",
                        [pending.owner_registration_id.as_slice()],
                    )?;
                }
                _ => {
                    c.execute(
                        "UPDATE accounts SET settings_version=settings_version+1 WHERE account_id=?1",
                        [pending.request.account_id()],
                    )?;
                }
            }
            Ok(())
        }).unwrap();
        assert!(matches!(
            f.store().can_confirm_personal_mail_send(&pending),
            Ok(false) | Err(_)
        ));
        assert_eq!(count(f.store(), "mail_outbox"), 0);
    }
}

#[cfg(windows)]
#[test]
fn apr01_personal_google_send_allows_each_account_in_bounded_multi_account_scope() {
    let (f, first_account, old_pending) = personal_fixture(false);
    let second = personal_identity("personal-send-second-fixture");
    let second_account = second.account_id.clone();
    f.store()
        .activate_google_account(&second, "fixture.apps.googleusercontent.com", 0, None)
        .unwrap();
    let target = RequestedScope::new(
        vec![first_account.clone(), second_account.clone()],
        hantong_contracts::auth::default_cli_send_tools(),
    )
    .unwrap();
    let refresh = f
        .store()
        .create_terminal_cli_scope_refresh([4; 32], target, server_hash())
        .unwrap();
    let staged = f.store().stage_cli_scope_refresh(&refresh).unwrap();
    f.store().activate_cli_scope_refresh(&staged).unwrap();

    // The old review is bound to the pre-expansion epoch and cannot be reused.
    assert!(
        !f.store()
            .can_confirm_personal_mail_send(&old_pending)
            .unwrap()
    );
    let first_pending = personal_pending_for_account(&f, &first_account, "personal-send-first");
    let second_pending = personal_pending_for_account(&f, &second_account, "personal-send-second");

    assert!(
        f.store()
            .can_confirm_personal_mail_send(&first_pending)
            .unwrap()
    );
    assert!(
        f.store()
            .can_confirm_personal_mail_send(&second_pending)
            .unwrap()
    );
    assert_eq!(count(f.store(), "mail_outbox"), 0);

    let first_audit = personal_audit(&f, &first_pending, 94);
    let second_audit = personal_audit(&f, &second_pending, 95);
    f.store()
        .claim_personal_mail_send(&first_pending, first_audit)
        .unwrap();
    f.store()
        .claim_personal_mail_send(&second_pending, second_audit)
        .unwrap();
    assert_eq!(count(f.store(), "mail_outbox"), 2);
}

#[cfg(windows)]
#[test]
fn apr01_personal_google_send_rechecks_provenance_before_claim() {
    let (f, _account, pending) = personal_fixture(false);
    assert!(f.store().can_confirm_personal_mail_send(&pending).unwrap());
    let audit_id = personal_audit(&f, &pending, 93);
    f.store()
        .with_connection(|c| {
            c.execute(
                "UPDATE client_registration_requests SET decision_channel='approval_helper' WHERE request_id=(SELECT request_id FROM client_registrations WHERE registration_id=?1)",
                [pending.owner_registration_id.as_slice()],
            )?;
            c.execute(
                "UPDATE client_registrations SET approved_channel='approval_helper' WHERE registration_id=?1",
                [pending.owner_registration_id.as_slice()],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.store().claim_personal_mail_send(&pending, audit_id),
        // Provenance contradicts the immutable OAuth scope history. Reject
        // corruption before it can masquerade as a valid helper registration.
        Err(ErrorCode::InternalError)
    );
    assert_eq!(count(f.store(), "mail_outbox"), 0);
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
