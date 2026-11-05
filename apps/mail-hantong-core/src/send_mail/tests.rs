//! Isolated encrypted Store and synthetic SMTP futures; no provider/Hello calls.
use super::*;
use hantong_contracts::mail_send::{DraftCreateRequest, SendReviewRequest};
use std::{
    cell::Cell,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
pub(crate) struct Fixture {
    store: Option<Store>,
    root: PathBuf,
}
impl Fixture {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-send-coordinator-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let root = std::fs::canonicalize(root).unwrap();
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            crate::test_support::ensure_test_backend();
        }
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
            c.execute("INSERT INTO server_identity(singleton,key_id,state,public_key,key_epoch,version,created_at_utc_ms,updated_at_utc_ms) VALUES(1,?1,'active',?2,1,1,1,1)",([9_u8;32].as_slice(),key.as_slice()))?;
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
    pub(crate) fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }
    #[cfg(windows)]
    pub(crate) fn secrets_root(&self) -> &std::path::Path {
        &self.root
    }
    pub(crate) fn prepare(&self) -> PendingSend {
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
    pub(crate) fn prepare_personal(&self) -> PendingSend {
        use hantong_contracts::{
            auth::{RequestedScope, default_cli_send_tools},
            client::ClientKind,
            google::GoogleIdentity,
            google_write::GoogleAccessMode,
        };
        use hantong_store::DEFAULT_CLI_LABEL;
        let subject = "synthetic-personal-send-core";
        let account = format!(
            "google_{}",
            hantong_contracts::wire_hex::encode(
                &hantong_platform::sha256(subject.as_bytes()).unwrap()
            )
        );
        self.store()
            .activate_google_account(
                &GoogleIdentity {
                    access_mode: GoogleAccessMode::ReadSend,
                    account_id: account.clone(),
                    subject: subject.into(),
                    email: "sender@example.test".into(),
                    granted_scopes: GoogleAccessMode::ReadSend
                        .required_scopes()
                        .iter()
                        .map(|s| (*s).into())
                        .collect(),
                },
                "fixture.apps.googleusercontent.com",
                0,
                None,
            )
            .unwrap();
        let mut key = self
            .store()
            .load_registration([2; 16])
            .unwrap()
            .unwrap()
            .public_key;
        let server_hash = hantong_platform::sha256(&key).unwrap();
        // A distinct public P-256 point (-G) avoids reusing the synthetic
        // helper registration's key. No private key is generated or persisted.
        key[40..].copy_from_slice(
            &hantong_contracts::wire_hex::decode_fixed::<32>(
                "b01cbd1c01e58065711814b583f061e9d431cca994cea1313449bf97c840ae0a",
            )
            .unwrap(),
        );
        let request = self
            .store()
            .create_registration_request(
                [3; 32],
                ClientKind::Cli,
                DEFAULT_CLI_LABEL,
                &RequestedScope::new(vec![account.clone()], vec!["mail.accounts.list".into()])
                    .unwrap(),
            )
            .unwrap();
        let registration_id =
            hantong_platform::registration_id_for_request(request.request_id).unwrap();
        let stage = hantong_store::CliRegistrationStage::new_google_oauth(
            request,
            registration_id,
            key,
            server_hash,
        )
        .unwrap();
        let staged = self
            .store()
            .stage_google_oauth_cli_registration(&stage)
            .unwrap();
        self.store()
            .activate_cli_registration(registration_id, staged.version, server_hash)
            .unwrap();
        let refresh = self
            .store()
            .create_google_oauth_cli_scope_refresh(
                [4; 32],
                RequestedScope::new(vec![account.clone()], default_cli_send_tools()).unwrap(),
                server_hash,
            )
            .unwrap();
        let staged = self.store().stage_cli_scope_refresh(&refresh).unwrap();
        let registration = self.store().activate_cli_scope_refresh(&staged).unwrap();
        let make = |hash| {
            DraftCreateRequest::new(
                &account,
                vec!["recipient@example.test".into()],
                "Synthetic personal".into(),
                "Complete fake body".into(),
                "personal-draft",
                "personal-draft",
                0,
                hash,
            )
            .unwrap()
        };
        let d = make(
            hantong_platform::sha256_storage_payload(&make([0; 32]).canonical_payload()).unwrap(),
        );
        let id = self
            .store()
            .create_mail_draft(registration_id, registration.profile_epoch, &d)
            .unwrap();
        let request = SendReviewRequest::new(
            &account,
            id,
            1,
            "personal-send",
            "personal-send",
            *d.payload_hash(),
        )
        .unwrap();
        let receipt = self
            .store()
            .request_mail_send_review(registration_id, registration.profile_epoch, &request)
            .unwrap();
        self.store().pending_mail_send(receipt.review_id).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.store.take());
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn apr01_smtp_preview_uses_the_persisted_transport_sender() {
    #[cfg(target_os = "linux")]
    if isolated_child(
        "send_mail::integration_tests::apr01_smtp_preview_uses_the_persisted_transport_sender",
    ) {
        return;
    }
    let f = Fixture::new();
    let pending = f.prepare();
    let preview = super::preview(&pending).unwrap();
    assert_eq!(preview["from"], "sender@example.test");
    assert_eq!(preview["to"], serde_json::json!(["recipient@example.test"]));
    assert_eq!(preview["subject"], "Synthetic");
    assert_eq!(preview["body"], "Complete fake body");

    let alternate = hantong_domain::mail_connection::MailConnectionConfig::from_json(
        br#"{"schema_version":1,"account_id":"b","imap":{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"imap-login","authentication":"app_password"},"smtp":{"settings_version":1,"host":"smtp.example","port":465,"tls":"implicit_tls","login_name":"other@example.test","authentication":"app_password"}}"#,
    )
    .unwrap();
    let mut second = pending;
    second.endpoint =
        hantong_contracts::google_write::MailSendTarget::Smtp(alternate.smtp().unwrap().clone());
    assert_eq!(
        super::preview(&second).unwrap()["from"],
        "other@example.test"
    );
}

fn run<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(future)
}

#[test]
fn apr01_personal_confirmation_cannot_claim_helper_origin_smtp_or_poll_provider() {
    #[cfg(target_os = "linux")]
    if isolated_child(
        "send_mail::integration_tests::apr01_personal_confirmation_cannot_claim_helper_origin_smtp_or_poll_provider",
    ) {
        return;
    }
    let f = Fixture::new();
    let p = f.prepare();
    assert!(!f.store().can_confirm_personal_mail_send(&p).unwrap());
    let policy = Rc::new(RefCell::new(ReadPolicy::new()));
    let calls = Cell::new(0);
    assert_eq!(
        run(execute_confirmed(f.store(), &policy, &p, true, async {
            calls.set(calls.get() + 1);
            SendOutcome::Sent
        })),
        Err(ErrorCode::PolicyBlocked)
    );
    assert_eq!(calls.get(), 0);
    let receipt = f
        .store()
        .mail_send_receipt([2; 16], 1, "a", p.review_id)
        .unwrap();
    assert_eq!(receipt.state, "failed");
    assert_eq!(receipt.error, Some(ErrorCode::PolicyBlocked));
}

#[test]
fn apr01_personal_send_polls_once_only_after_durable_claim_and_never_replays() {
    #[cfg(target_os = "linux")]
    if isolated_child(
        "send_mail::integration_tests::apr01_personal_send_polls_once_only_after_durable_claim_and_never_replays",
    ) {
        return;
    }
    let f = Fixture::new();
    let p = f.prepare_personal();
    assert!(f.store().can_confirm_personal_mail_send(&p).unwrap());
    let policy = Rc::new(RefCell::new(ReadPolicy::new()));
    let calls = Cell::new(0);
    run(execute_confirmed(f.store(), &policy, &p, true, async {
        calls.set(calls.get() + 1);
        assert_eq!(
            f.store()
                .mail_send_receipt(
                    p.owner_registration_id,
                    p.profile_epoch,
                    p.request.account_id(),
                    p.review_id
                )
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
            .mail_send_receipt(
                p.owner_registration_id,
                p.profile_epoch,
                p.request.account_id(),
                p.review_id
            )
            .unwrap()
            .state,
        "sent"
    );
    assert_eq!(
        run(execute_confirmed(f.store(), &policy, &p, true, async {
            calls.set(calls.get() + 1);
            SendOutcome::Sent
        })),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(calls.get(), 1);
}

#[test]
fn apr01_smtp_future_first_polls_after_durable_claim_and_replay_never_polls() {
    #[cfg(target_os = "linux")]
    if isolated_child(
        "send_mail::integration_tests::apr01_smtp_future_first_polls_after_durable_claim_and_replay_never_polls",
    ) {
        return;
    }
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
    #[cfg(target_os = "linux")]
    if isolated_child("send_mail::integration_tests::apr01_shared_rate_denial_never_polls_smtp") {
        return;
    }
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
    #[cfg(target_os = "linux")]
    if isolated_child(
        "send_mail::integration_tests::sync07_changed_endpoint_cancels_inflight_future_and_records_unknown",
    ) {
        return;
    }
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

#[cfg(target_os = "linux")]
fn isolated_child(name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    if std::env::var_os("HANTONG_SEND_TEST_CHILD").is_some() {
        return false;
    }
    let root = std::env::temp_dir().join(format!(
        "hantong-send-env-{}-{}",
        std::process::id(),
        hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
    ));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    for directory in ["data", "config", "runtime"] {
        std::fs::create_dir(root.join(directory)).unwrap();
        std::fs::set_permissions(root.join(directory), std::fs::Permissions::from_mode(0o700))
            .unwrap();
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("HANTONG_SEND_TEST_CHILD", "1")
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .status()
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(status.success(), "isolated native send test failed: {name}");
    true
}
