//! Isolated encrypted Store and synthetic SMTP futures; no provider/Hello calls.
use super::*;
use hantong_contracts::mail_send::{DraftCreateRequest, SendReviewRequest};
use std::{
    cell::Cell,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
struct Fixture {
    store: Option<Store>,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-send-coordinator-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        drop(Store::open(root.join("data"), root.join("keys")).unwrap());
        let mut key = [0; 72];
        key[..8].copy_from_slice(b"ECS1\x20\0\0\0");
        key[8..40].copy_from_slice(
            &hantong_contracts::wire_hex::decode_fixed::<32>(
                "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
            )
            .unwrap(),
        );
        key[40..].copy_from_slice(
            &hantong_contracts::wire_hex::decode_fixed::<32>(
                "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
            )
            .unwrap(),
        );
        hantong_secrets::KeyedConnection::open(root.join("data"),root.join("keys")).unwrap().with_connection(|c|{
            let now=1_i64;
            c.execute_batch("INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES('a','imap',1,'ready','[]','[]',1);")?;
            c.execute("INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,decision_channel,version,created_at_utc_ms,expires_at_utc_ms,decided_at_utc_ms) VALUES(?1,'cli','Synthetic send fixture','[\"a\"]','[\"mail.drafts.create\",\"mail.send.execute\",\"mail.send.request\"]','approved','approval_helper',2,1,300001,1)",[[1_u8;32].as_slice()])?;
            c.execute("INSERT INTO client_registrations(registration_id,request_id,client_kind,display_label,public_key,profile_epoch,state,read_only,requests_per_minute,approved_channel,approved_at_utc_ms,updated_at_utc_ms,revoked_at_utc_ms,version) VALUES(?1,?2,'cli','Synthetic send fixture',?3,1,'active',0,60,'approval_helper',?4,?4,NULL,2)",([2_u8;16].as_slice(),[1_u8;32].as_slice(),key.as_slice(),now))?;
            c.execute("INSERT INTO client_registration_accounts(registration_id,account_id) VALUES(?1,'a')",[[2_u8;16].as_slice()])?;
            for tool in ["mail.drafts.create","mail.send.execute","mail.send.request"]{c.execute("INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,?2)",([2_u8;16].as_slice(),tool))?;}
            for kind in ["imap","smtp"]{c.execute("INSERT INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES('a',?1,1,'mail.example',465,'implicit_tls','sender@example.test','app_password',1)",[kind])?;}
            Ok(())
        }).unwrap();
        let store = Some(Store::open(root.join("data"), root.join("keys")).unwrap());
        Self { root, store }
    }
    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }
    fn prepare(&self) -> PendingSend {
        let make = |hash| {
            DraftCreateRequest::new(
                "a",
                vec!["recipient@example.test".into()],
                "Synthetic".into(),
                "Complete fake body".into(),
                "draft",
                "draft",
                0,
                hash,
            )
            .unwrap()
        };
        let d = make(
            hantong_platform::sha256_storage_payload(&make([0; 32]).canonical_payload()).unwrap(),
        );
        let id = self.store().create_mail_draft([2; 16], 1, &d).unwrap();
        let r = SendReviewRequest::new("a", id, 1, "send", "send", *d.payload_hash()).unwrap();
        let r = self
            .store()
            .request_mail_send_review([2; 16], 1, &r)
            .unwrap();
        self.store().pending_mail_send(r.review_id).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.store.take());
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
fn run<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(future)
}

#[test]
fn apr01_smtp_future_first_polls_after_durable_claim_and_replay_never_polls() {
    let f = Fixture::new();
    let p = f.prepare();
    let policy = Rc::new(RefCell::new(ReadPolicy::new()));
    let calls = Cell::new(0);
    run(execute_verified(f.store(), &policy, &p, async {
        calls.set(calls.get() + 1);
        assert_eq!(
            f.store()
                .mail_send_receipt([2; 16], 1, "a", p.review_id)
                .unwrap()
                .state,
            "sending"
        );
        SendOutcome::Sent
    }))
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(
        f.store()
            .mail_send_receipt([2; 16], 1, "a", p.review_id)
            .unwrap()
            .state,
        "sent"
    );
    assert_eq!(
        run(execute_verified(f.store(), &policy, &p, async {
            calls.set(calls.get() + 1);
            SendOutcome::Sent
        })),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(calls.get(), 1);
}
#[test]
fn apr01_shared_rate_denial_never_polls_smtp() {
    let f = Fixture::new();
    let p = f.prepare();
    let policy = Rc::new(RefCell::new(ReadPolicy::new()));
    let calls = Cell::new(0);
    for _ in 0..60 {
        policy
            .borrow_mut()
            .check_native_send([2; 16], false, 60, Instant::now())
            .unwrap();
    }
    assert_eq!(
        run(execute_verified(f.store(), &policy, &p, async {
            calls.set(1);
            SendOutcome::Sent
        })),
        Err(ErrorCode::RateLimited)
    );
    assert_eq!(calls.get(), 0);
    let receipt = f
        .store()
        .mail_send_receipt([2; 16], 1, "a", p.review_id)
        .unwrap();
    assert_eq!(receipt.state, "failed");
    assert_eq!(receipt.error, Some(ErrorCode::RateLimited));
}
#[test]
fn sync07_changed_endpoint_cancels_inflight_future_and_records_unknown() {
    let f = Fixture::new();
    let p = f.prepare();
    let policy = Rc::new(RefCell::new(ReadPolicy::new()));
    let dropped = Rc::new(Cell::new(false));
    struct Signal(Rc<Cell<bool>>);
    impl Drop for Signal {
        fn drop(&mut self) {
            self.0.set(true)
        }
    }
    run(execute_verified(f.store(),&policy,&p,async{
        let _signal=Signal(dropped.clone());
        let c=hantong_domain::mail_connection::MailConnectionConfig::from_json(br#"{"schema_version":1,"account_id":"a","imap":{"settings_version":1,"host":"mail.example","port":465,"tls":"implicit_tls","login_name":"sender@example.test","authentication":"app_password"},"smtp":{"settings_version":2,"host":"changed.example","port":465,"tls":"implicit_tls","login_name":"sender@example.test","authentication":"app_password"}}"#).unwrap();
        let e=c.smtp().unwrap();let hash=hantong_store::mail_connection::smtp_payload_hash("a",e,1).unwrap();let mutation=hantong_store::mail_connection::MailMutation::new("change".into(),"change".into(),1,hash).unwrap();
        f.store().put_smtp_connection("a",e,&mutation).unwrap();
        std::future::pending::<SendOutcome>().await
    })).unwrap();
    assert!(dropped.get());
    let receipt = f
        .store()
        .mail_send_receipt([2; 16], 1, "a", p.review_id)
        .unwrap();
    assert_eq!(receipt.state, "unknown");
    assert_eq!(receipt.error, Some(ErrorCode::SendUnknown));
}
