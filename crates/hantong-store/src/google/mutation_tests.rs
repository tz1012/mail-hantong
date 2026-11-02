// Synthetic encrypted fixtures only; no live credentials, network or user data.
use super::*;
use hantong_contracts::{google_write::{GoogleAccessMode,MessageLabelChange},google_mutation::{MutationPayload,account_grant}};
use crate::google_mutation::{MutationOutcome,PendingMutation};
fn fixture()->Fixture{
 let mut f=Fixture::new(2);
 let sync=f.request(0,"mutation-seed",0);f.admit(71,0,&sync);f.finish(71,0);
 for i in &mut f.identities{i.access_mode=GoogleAccessMode::Modify;i.granted_scopes=GoogleAccessMode::Modify.required_scopes().iter().map(|s|(*s).into()).collect();}
 for i in &f.identities{f.s().activate_google_account(i,"fixture.apps.googleusercontent.com",1,None).unwrap();}
 f.s().with_connection(|c|{
  c.execute("UPDATE client_registrations SET read_only=0 WHERE registration_id=?1",[vec![9u8;16]])?;
  for tool in ["mail.drafts.create".into(),"mail.google.changes.request".into(),"mail.google.changes.execute".into(),"mail.messages.trash.request".into(),"mail.messages.trash.execute".into(),account_grant(&f.identities[0].account_id).unwrap()]{c.execute("INSERT INTO client_registration_tools VALUES(?1,?2)",rusqlite::params![vec![9u8;16],tool])?;}Ok(())
 }).unwrap();f
}
fn labels(f:&Fixture,index:usize,op:&str)->MutationPayload{
 let change=MessageLabelChange::new(f.identities[index].account_id.clone(),"message1".into(),vec!["STARRED".into()],vec!["UNREAD".into()]).unwrap();
 let hash=hantong_platform::sha256_storage_payload(&change.canonical_payload()).unwrap();
 MutationPayload::Labels{change,expected_version:1,operation_id:op.into(),idempotency_key:op.into(),payload_hash:hash}
}
fn trash(f:&Fixture,op:&str)->MutationPayload{
 let mut request=MutationPayload::Trash{account_id:f.identities[0].account_id.clone(),message_id:"message1".into(),expected_version:1,operation_id:op.into(),idempotency_key:op.into(),payload_hash:[0;32]};
 let hash=hantong_platform::sha256_storage_payload(&request.canonical_payload()).unwrap();
 request=MutationPayload::Trash{account_id:f.identities[0].account_id.clone(),message_id:"message1".into(),expected_version:1,operation_id:op.into(),idempotency_key:op.into(),payload_hash:hash};
 request
}
fn pending(f:&Fixture)->PendingMutation{
 let receipt=f.s().request_google_mutation([9;16],1,&labels(f,0,"labels-one")).unwrap();
 f.s().pending_google_mutation(receipt.review_id).unwrap()
}
fn audit(f:&Fixture,p:&PendingMutation,id:u8){f.s().begin_audit(&AuditBegin::new([id;32],[9;16],1,AuditTool::GoogleMutationExecute,p.request.account_ids()).unwrap()).unwrap();}
#[test]
fn sec03_mutation_account_epoch_mode_and_grant_isolation(){
 let f=fixture();
 assert_eq!(f.s().request_google_mutation([9;16],1,&labels(&f,1,"cross")),Err(ErrorCode::ScopeDenied));
 assert_eq!(f.s().request_google_mutation([9;16],2,&labels(&f,0,"epoch")),Err(ErrorCode::AuthRequired));
 assert_eq!(f.count("google_mutation_requests"),0);
 let p=pending(&f);
 assert_eq!(f.s().google_mutation_receipt([9;16],1,&f.identities[1].account_id,p.review_id),Err(ErrorCode::ScopeDenied));
 assert!(f.s().google_mutation_receipt([8;16],1,p.request.account_id(),p.review_id).is_err());
 f.sql("DELETE FROM client_registration_tools WHERE tool_id LIKE 'mail.google.change.%'");
 assert_eq!(f.s().pending_google_mutation(p.review_id),Err(ErrorCode::ScopeDenied));
}
#[test]
fn sync07_mutation_claim_once_restart_unknown_and_new_key_cannot_duplicate(){
 let mut f=fixture();let p=pending(&f);audit(&f,&p,72);
 f.s().claim_google_mutation(&p,[72;32]).unwrap();
 assert_eq!(f.s().claim_google_mutation(&p,[72;32]),Err(ErrorCode::Conflict));
 f.reopen();assert_eq!(f.s().recover_google_mutations(),Ok(1));assert_eq!(f.s().recover_google_mutations(),Ok(0));
 let status=f.s().request_google_mutation([9;16],1,&p.request).unwrap();assert_eq!(status.state,"unknown");assert_eq!(status.error,Some(ErrorCode::SendUnknown));
 assert_eq!(f.s().request_google_mutation([9;16],1,&labels(&f,0,"different-key")),Err(ErrorCode::Conflict));
 assert_eq!(f.count("google_mutation_requests"),1);
}
#[test]
fn apr01_mutation_snapshot_cache_change_and_cross_operation_do_not_claim(){
 let f=fixture();let p=pending(&f);audit(&f,&p,73);
 let mut changed=p.clone();changed.before_labels.push("STARRED".into());
 assert_eq!(f.s().claim_google_mutation(&changed,[73;32]),Err(ErrorCode::Conflict));
 let mut changed=p.clone();changed.binding.credential_version+=1;
 assert_eq!(f.s().claim_google_mutation(&changed,[73;32]),Err(ErrorCode::Conflict));
 f.sql("UPDATE google_sync_state SET version=version+1");
 assert_eq!(f.s().claim_google_mutation(&p,[73;32]),Err(ErrorCode::Conflict));
 assert_eq!(f.s().google_mutation_receipt([9;16],1,p.request.account_id(),p.review_id).unwrap().state,"pending");
}
#[test]
fn apr01_mutation_denial_expiry_and_wrong_audit_never_claim(){
 let f=fixture();let p=pending(&f);
 f.s().begin_audit(&AuditBegin::new([74;32],[9;16],1,AuditTool::SendExecute,p.request.account_ids()).unwrap()).unwrap();
 assert_eq!(f.s().claim_google_mutation(&p,[74;32]),Err(ErrorCode::InternalError));
 f.s().deny_google_mutation(p.review_id,ErrorCode::ApprovalDenied).unwrap();
 assert_eq!(f.s().request_google_mutation([9;16],1,&p.request).unwrap().state,"denied");
 assert_eq!(f.s().claim_google_mutation(&p,[74;32]),Err(ErrorCode::Conflict));
 let receipt=f.s().request_google_mutation([9;16],1,&labels(&f,0,"expired")).unwrap();
 f.sql("UPDATE google_mutation_requests SET created_at_utc_ms=1,expires_at_utc_ms=2 WHERE state='pending'");
 assert_eq!(f.s().pending_google_mutation(receipt.review_id),Err(ErrorCode::ApprovalExpired));
}
#[test]
fn sync07_mutation_success_invalidates_only_selected_cached_message(){
 let f=fixture();let p=pending(&f);audit(&f,&p,75);f.s().claim_google_mutation(&p,[75;32]).unwrap();
 f.s().finish_google_mutation(p.review_id,MutationOutcome::Accepted).unwrap();
 assert_eq!(f.s().google_mutation_receipt([9;16],1,p.request.account_id(),p.review_id).unwrap().state,"accepted");
 assert_eq!(f.s().google_sync_version(p.request.account_id()).unwrap(),2);
 assert!(matches!(f.s().google_message(p.request.account_id(),"message1"),Err(ErrorCode::NotFound)));
 assert!(f.s().google_message(p.request.account_id(),"message2").is_ok());
 assert_eq!(f.s().finish_google_mutation(p.review_id,MutationOutcome::Accepted),Err(ErrorCode::Conflict));
}
#[test]
fn ai09_trash_is_modify_bound_single_message_and_invalidates_only_target(){
 let f=fixture();let req=trash(&f,"trash-one");
 let p=f.s().request_google_mutation([9;16],1,&req).unwrap();
 let pending=f.s().pending_google_mutation(p.review_id).unwrap();
 assert!(matches!(pending.request,MutationPayload::Trash{..}));
 assert_eq!(pending.before_labels,vec!["INBOX"]);
 let mut wrong=pending.request.clone();
 if let MutationPayload::Trash{expected_version,..}= &mut wrong {*expected_version=2;}
 assert_eq!(f.s().request_google_mutation([9;16],1,&wrong),Err(ErrorCode::Conflict));
 audit(&f,&pending,78);f.s().claim_google_mutation(&pending,[78;32]).unwrap();
 f.s().finish_google_mutation(p.review_id,MutationOutcome::Accepted).unwrap();
 assert!(matches!(f.s().google_message(pending.request.account_id(),"message1"),Err(ErrorCode::NotFound)));
}
#[test]
fn sync07_mutation_ambiguous_claim_failure_is_unknown_without_provider_retry(){
 let f=fixture();let p=pending(&f);audit(&f,&p,76);f.s().claim_google_mutation(&p,[76;32]).unwrap();
 f.s().fail_google_mutation_admission(p.review_id,[76;32],ErrorCode::InternalError).unwrap();
 assert_eq!(f.s().request_google_mutation([9;16],1,&p.request).unwrap().state,"unknown");
}
#[test]
fn ai09_ai_trash_registration_uses_trash_execute_grant_and_replays_as_unknown(){
 let f=fixture();
 let account=f.identities[0].account_id.clone();
 let request_id=[8u8;32];
 let registration_id=hantong_platform::registration_id_for_request(request_id).unwrap();
 f.s().with_connection(|c|{
  let tools=hantong_contracts::auth::ai_mail_tools();
  let mut key=[6u8;72]; key[..8].copy_from_slice(b"ECS1\x20\0\0\0");
  let accounts_json=serde_json::to_string(std::slice::from_ref(&account)).unwrap();
  let tools_json=serde_json::to_string(&tools).unwrap();
  c.execute("INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,decision_channel,version,created_at_utc_ms,expires_at_utc_ms,decided_at_utc_ms) VALUES(?1,'ai','mail-hantong AI Codex',?2,?3,'approved','ai_feature',2,1,300001,1)",rusqlite::params![request_id.as_slice(),accounts_json,tools_json])?;
  c.execute("INSERT INTO client_registrations(registration_id,request_id,client_kind,display_label,public_key,profile_epoch,state,read_only,requests_per_minute,approved_channel,approved_at_utc_ms,updated_at_utc_ms,version) VALUES(?1,?2,'ai','mail-hantong AI Codex',?3,1,'active',0,60,'ai_feature',1,1,2)",rusqlite::params![registration_id.as_slice(),request_id.as_slice(),key.as_slice()])?;
  c.execute("INSERT INTO client_registration_accounts(registration_id,account_id) VALUES(?1,?2)",rusqlite::params![registration_id.as_slice(),account])?;
  for tool in &tools { c.execute("INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,?2)",rusqlite::params![registration_id.as_slice(),tool])?; }
  c.execute("INSERT INTO ai_consents(provider,registration_id,account_id,profile_epoch,registration_request_id,approved_at_utc_ms) VALUES('codex',?1,?2,1,?3,1)",rusqlite::params![registration_id.as_slice(),account,request_id.as_slice()])?;
  Ok(())
 }).unwrap();
 let request=trash(&f,"ai-trash");
 let receipt=f.s().request_google_mutation(registration_id,1,&request).unwrap();
 let p=f.s().pending_google_mutation(receipt.review_id).unwrap();
 f.s().begin_audit(&AuditBegin::new([79;32],registration_id,1,AuditTool::GoogleMutationExecute,p.request.account_ids()).unwrap()).unwrap();f.s().claim_google_mutation(&p,[79;32]).unwrap();
 f.s().finish_google_mutation(receipt.review_id,MutationOutcome::Unknown).unwrap();
 let status=f.s().google_mutation_receipt(registration_id,1,&account,receipt.review_id).unwrap();
 assert_eq!(status.state,"unknown");assert_eq!(status.error,Some(ErrorCode::SendUnknown));
}
#[test]
fn apr01_google_server_draft_attachment_snapshot_and_duplicate_creation_guard(){
 use hantong_contracts::{mail_send::{DraftCreateRequest,SendReviewRequest},google_write::{AttachmentSnapshot,ComposeSnapshot}};
 let f=fixture();let account=&f.identities[0].account_id;
 let make=|hash|DraftCreateRequest::new(account,vec!["test@example.test".into()],"synthetic draft".into(),"synthetic body".into(),"local-draft","local-draft",0,hash).unwrap();
 let attachments=vec![AttachmentSnapshot::new("synthetic.txt".into(),"text/plain".into(),b"synthetic attachment".to_vec()).unwrap()];
 let raw=ComposeSnapshot::new(make([0;32]),attachments.clone()).unwrap();
 let compose=ComposeSnapshot::new(make(hantong_platform::sha256_storage_payload(&raw.canonical_payload()).unwrap()),attachments).unwrap();
 let draft=f.s().create_mail_compose([9;16],1,&compose).unwrap();
 let req=|op|MutationPayload::SaveDraft(SendReviewRequest::new(account,draft,1,op,op,*compose.payload_hash()).unwrap());
 let id=f.s().request_google_mutation([9;16],1,&req("save-one")).unwrap().review_id;
 let p=f.s().pending_google_mutation(id).unwrap();assert_eq!(p.compose,Some(compose.clone()));audit(&f,&p,77);
 f.s().claim_google_mutation(&p,[77;32]).unwrap();f.s().finish_google_mutation(id,MutationOutcome::Accepted).unwrap();
 assert_eq!(f.s().request_google_mutation([9;16],1,&req("save-two")),Err(ErrorCode::Conflict));
 assert_eq!(f.count("mail_outbox"),0);
}
