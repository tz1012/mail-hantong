// Synthetic encrypted account/attachment fixtures. No network or live credentials.
use super::*;
use hantong_contracts::{google_write::{AttachmentSnapshot, ComposeSnapshot, GoogleAccessMode, MailSendTarget}, mail_send::{DraftCreateRequest, SendReviewRequest}};

fn writable() -> Fixture {
    let mut f = Fixture::new(2);
    f.identities[0].access_mode = GoogleAccessMode::ReadSend;
    f.identities[0].granted_scopes = GoogleAccessMode::ReadSend.required_scopes().iter().map(|s|(*s).into()).collect();
    f.s().activate_google_account(&f.identities[0], "fixture.apps.googleusercontent.com", 1, None).unwrap();
    f.s().with_connection(|c| {
        c.execute("UPDATE client_registrations SET read_only=0 WHERE registration_id=?1", [vec![9u8;16]])?;
        for tool in ["mail.drafts.create","mail.send.request","mail.send.execute"] {
            c.execute("INSERT INTO client_registration_tools VALUES(?1,?2)",rusqlite::params![vec![9u8;16],tool])?;
        }
        Ok(())
    }).unwrap();
    f
}
fn compose(f: &Fixture, index: usize, content: &[u8]) -> ComposeSnapshot {
    let make = |hash| DraftCreateRequest::new(&f.identities[index].account_id, vec!["receiver@example.test".into()], "synthetic".into(), "synthetic body".into(), "draft-attachment", "draft-attachment",0,hash).unwrap();
    let attachments = vec![AttachmentSnapshot::new("fixture.txt".into(),"text/plain".into(),content.to_vec()).unwrap()];
    let raw = ComposeSnapshot::new(make([0;32]),attachments.clone()).unwrap();
    ComposeSnapshot::new(make(hantong_platform::sha256_storage_payload(&raw.canonical_payload()).unwrap()),attachments).unwrap()
}
fn pending(f: &Fixture) -> crate::mail_send::PendingSend {
    let d=compose(f,0,b"synthetic attachment");
    let id=f.s().create_mail_compose([9;16],1,&d).unwrap();
    let request=SendReviewRequest::new(d.account_id(),id,1,"send","send",*d.payload_hash()).unwrap();
    let receipt=f.s().request_mail_send_review([9;16],1,&request).unwrap();
    f.s().pending_mail_send(receipt.review_id).unwrap()
}
#[test]
fn gmail_write_ledger_attachment_roundtrip_and_readonly_account_isolation() {
    let f=writable();
    let d=compose(&f,0,b"synthetic attachment");
    let id=f.s().create_mail_compose([9;16],1,&d).unwrap();
    assert_eq!(f.s().create_mail_compose([9;16],1,&d),Ok(id));
    assert_eq!(f.s().create_mail_compose([9;16],1,&compose(&f,0,b"changed")),Err(ErrorCode::Conflict));
    assert!(f.s().create_mail_compose([9;16],1,&compose(&f,1,b"synthetic attachment")).is_err());
    let p=pending(&f);
    assert_eq!(p.draft,d);
    assert!(matches!(p.endpoint, MailSendTarget::Gmail{..}));
    let wrong=SendReviewRequest::new(&f.identities[1].account_id,id,1,"cross","cross",*d.payload_hash()).unwrap();
    assert!(f.s().request_mail_send_review([9;16],1,&wrong).is_err());
    assert_eq!(f.count("mail_outbox"),0);
}
#[test]
fn gmail_write_ledger_attachment_tampering_blocks_claim() {
    let f=writable(); let p=pending(&f);
    f.s().with_connection(|c| {c.execute("UPDATE mail_draft_attachments SET content=?1",[b"changed".as_slice()])?;Ok(())}).unwrap();
    assert!(f.s().pending_mail_send(p.review_id).is_err());
    f.s().begin_audit(&AuditBegin::new([87;32],[9;16],1,AuditTool::SendExecute,vec![p.draft.account_id().into()]).unwrap()).unwrap();
    assert!(f.s().claim_mail_send(&p,[87;32]).is_err());
    assert_eq!(f.count("mail_outbox"),0);
}
#[test]
fn gmail_write_ledger_version_drift_and_unknown_replay_do_not_send() {
    let f=writable(); let p=pending(&f);
    f.s().begin_audit(&AuditBegin::new([88;32],[9;16],1,AuditTool::SendExecute,vec![p.draft.account_id().into()]).unwrap()).unwrap();
    let mut changed=p.clone();
    if let MailSendTarget::Gmail{ref mut binding,..}=changed.endpoint {binding.credential_version+=1;}
    assert_eq!(f.s().claim_mail_send(&changed,[88;32]),Err(ErrorCode::Conflict));
    f.s().claim_mail_send(&p,[88;32]).unwrap();
    assert_eq!(f.s().claim_mail_send(&p,[88;32]),Err(ErrorCode::Conflict));
    assert_eq!(f.s().recover_mail_sends(),Ok(1));
    assert_eq!(f.s().mail_send_receipt([9;16],1,p.draft.account_id(),p.review_id).unwrap().state,"unknown");
    assert!(f.s().pending_mail_send(p.review_id).is_err());
    assert_eq!(f.count("mail_outbox"),1);
}
