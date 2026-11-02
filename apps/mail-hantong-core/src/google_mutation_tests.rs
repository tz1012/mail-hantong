//! Synthetic Gmail mutation coordinator tests; no provider or credential calls.
use super::*;
use hantong_contracts::{
    google::GoogleIdentity,
    google_mutation::{MutationPayload, account_grant},
    google_write::{GoogleAccessMode, MessageLabelChange},
};
use hantong_secrets::KeyedConnection;
use hantong_store::google_mutation::MutationOutcome;
use std::{
    cell::{Cell, RefCell},
    fs,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

const REGISTRATION: [u8; 16] = [9; 16];

struct Fixture {
    root: PathBuf,
    store: Option<Store>,
    account: String,
    subject: String,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-google-mutation-core-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let data = root.join("data");
        let keys = root.join("keys");
        drop(Store::open(&data, &keys).unwrap());
        let subject = "synthetic-google-mutation".to_owned();
        let account = format!(
            "google_{}",
            hantong_contracts::wire_hex::encode(
                &hantong_platform::sha256(subject.as_bytes()).unwrap()
            )
        );
        let mut public_key = [0_u8; 72];
        public_key[..8].copy_from_slice(b"ECS1\x20\0\0\0");
        public_key[8..40].copy_from_slice(&[3; 32]);
        public_key[40..].copy_from_slice(&[4; 32]);
        let store = Store::open(&data, &keys).unwrap();
        store
            .activate_google_account(
                &GoogleIdentity {
                    access_mode: GoogleAccessMode::Modify,
                    account_id: account.clone(),
                    subject: subject.clone(),
                    email: "sender@example.test".into(),
                    granted_scopes: GoogleAccessMode::Modify
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
        drop(store);
        KeyedConnection::open(&data, &keys)
            .unwrap()
            .with_connection(|c| {
                let tools = vec![
                    "mail.google.changes.request".to_owned(),
                    "mail.google.changes.execute".to_owned(),
                    account_grant(&account).unwrap(),
                ];
                let tools_json = serde_json::to_string(&tools).unwrap();
                let accounts_json = serde_json::to_string(std::slice::from_ref(&account)).unwrap();
                c.execute("INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,decision_channel,version,created_at_utc_ms,expires_at_utc_ms,decided_at_utc_ms) VALUES(?1,'cli','Synthetic mutation',?2,?3,'approved','approval_helper',2,1,300001,1)", ([1_u8; 32].as_slice(), accounts_json.as_str(), tools_json.as_str()))?;
                c.execute("INSERT INTO client_registrations(registration_id,request_id,client_kind,display_label,public_key,profile_epoch,state,read_only,requests_per_minute,approved_channel,approved_at_utc_ms,updated_at_utc_ms,version) VALUES(?1,?2,'cli','Synthetic mutation',?3,1,'active',0,60,'approval_helper',1,1,2)", (REGISTRATION.as_slice(), [1_u8; 32].as_slice(), public_key.as_slice()))?;
                c.execute("INSERT INTO client_registration_accounts(registration_id,account_id) VALUES(?1,?2)", (REGISTRATION.as_slice(), account.as_str()))?;
                for tool in &tools { c.execute("INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,?2)", (REGISTRATION.as_slice(), tool.as_str()))?; }
                Ok(())
            })
            .unwrap();
        KeyedConnection::open(&data, &keys)
            .unwrap()
            .with_connection(|c| {
                c.execute(
                    "INSERT INTO google_messages(account_id,message_id,internal_date_ms,message_json) VALUES(?1,'message1',1,?2)",
                    (account.as_str(), r#"{"label_ids":["INBOX"]}"#),
                )?;
                c.execute(
                    "UPDATE google_sync_state SET version=1 WHERE account_id=?1",
                    (account.as_str(),),
                )?;
                Ok(())
            })
            .unwrap();
        let store = Store::open(&data, &keys).unwrap();
        Self {
            root,
            store: Some(store),
            account,
            subject,
        }
    }

    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }

    fn pending(&self, operation: &str) -> hantong_store::google_mutation::PendingMutation {
        let change = MessageLabelChange::new(
            self.account.clone(),
            "message1".into(),
            vec!["STARRED".into()],
            vec!["UNREAD".into()],
        )
        .unwrap();
        let hash = hantong_platform::sha256_storage_payload(&change.canonical_payload()).unwrap();
        let request = MutationPayload::Labels {
            change,
            expected_version: 1,
            operation_id: operation.into(),
            idempotency_key: operation.into(),
            payload_hash: hash,
        };
        let receipt = self
            .store()
            .request_google_mutation(REGISTRATION, 1, &request)
            .unwrap();
        self.store()
            .pending_google_mutation(receipt.review_id)
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.store.take());
        assert!(fs::remove_dir_all(&self.root).is_ok());
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
fn apr01_google_mutation_claims_before_one_provider_poll_and_records_terminal_receipts() {
    for (outcome, state, error) in [
        (MutationOutcome::Accepted, "accepted", None),
        (
            MutationOutcome::Rejected,
            "rejected",
            Some(ErrorCode::PolicyBlocked),
        ),
        (
            MutationOutcome::Unknown,
            "unknown",
            Some(ErrorCode::SendUnknown),
        ),
    ] {
        let f = Fixture::new();
        let p = f.pending("terminal");
        let policy = Rc::new(RefCell::new(ReadPolicy::new()));
        let polls = Cell::new(0);
        run(execute_verified(f.store(), &policy, &p, async {
            assert_eq!(
                f.store()
                    .google_mutation_receipt(REGISTRATION, 1, &f.account, p.review_id)
                    .unwrap()
                    .state,
                "claimed"
            );
            polls.set(polls.get() + 1);
            outcome
        }))
        .unwrap();
        assert_eq!(polls.get(), 1);
        let receipt = f
            .store()
            .google_mutation_receipt(REGISTRATION, 1, &f.account, p.review_id)
            .unwrap();
        assert_eq!((receipt.state.as_str(), receipt.error), (state, error));
    }
}

#[test]
fn apr01_google_mutation_admission_denial_never_polls_provider() {
    let f = Fixture::new();
    let p = f.pending("rate");
    let policy = Rc::new(RefCell::new(ReadPolicy::new()));
    for _ in 0..60 {
        policy
            .borrow_mut()
            .check_native_google_mutation(REGISTRATION, false, 60, Instant::now())
            .unwrap();
    }
    let polls = Cell::new(0);
    assert_eq!(
        run(execute_verified(f.store(), &policy, &p, async {
            polls.set(1);
            MutationOutcome::Accepted
        })),
        Err(ErrorCode::RateLimited)
    );
    assert_eq!(polls.get(), 0);
    let receipt = f
        .store()
        .google_mutation_receipt(REGISTRATION, 1, &f.account, p.review_id)
        .unwrap();
    assert_eq!(
        (receipt.state.as_str(), receipt.error),
        ("denied", Some(ErrorCode::RateLimited))
    );
}

#[test]
fn apr01_google_mutation_snapshot_change_never_polls_provider() {
    let f = Fixture::new();
    let p = f.pending("snapshot-change");
    let mut changed = p.clone();
    changed.before_labels.push("STARRED".into());
    let policy = Rc::new(RefCell::new(ReadPolicy::new()));
    let polls = Cell::new(0);
    assert_eq!(
        run(execute_verified(f.store(), &policy, &changed, async {
            polls.set(1);
            MutationOutcome::Accepted
        })),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(polls.get(), 0);
    let receipt = f
        .store()
        .google_mutation_receipt(REGISTRATION, 1, &f.account, p.review_id)
        .unwrap();
    assert_eq!(
        (receipt.state.as_str(), receipt.error),
        ("denied", Some(ErrorCode::Conflict))
    );
}

#[test]
fn sync07_google_mutation_invalidation_drops_pending_provider_and_blocks_replay() {
    let f = Fixture::new();
    let p = f.pending("invalidate");
    let policy = Rc::new(RefCell::new(ReadPolicy::new()));
    let dropped = Rc::new(Cell::new(false));
    struct DropSignal(Rc<Cell<bool>>);
    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    run(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            execute_verified(f.store(), &policy, &p, async {
                let _signal = DropSignal(dropped.clone());
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                f.store()
                    .activate_google_account(
                        &GoogleIdentity {
                            access_mode: GoogleAccessMode::Modify,
                            account_id: f.account.clone(),
                            subject: f.subject.clone(),
                            email: "sender@example.test".into(),
                            granted_scopes: GoogleAccessMode::Modify
                                .required_scopes()
                                .iter()
                                .map(|s| (*s).into())
                                .collect(),
                        },
                        "fixture.apps.googleusercontent.com",
                        1,
                        None,
                    )
                    .unwrap();
                std::future::pending::<MutationOutcome>().await
            }),
        )
        .await
    })
    .unwrap()
    .unwrap();
    assert!(dropped.get());
    let receipt = f
        .store()
        .google_mutation_receipt(REGISTRATION, 1, &f.account, p.review_id)
        .unwrap();
    assert_eq!(
        (receipt.state.as_str(), receipt.error),
        ("unknown", Some(ErrorCode::SendUnknown))
    );
    let mut alternate = p.request.clone();
    if let MutationPayload::Labels {
        operation_id,
        idempotency_key,
        ..
    } = &mut alternate
    {
        *operation_id = "invalidate-new-operation".into();
        *idempotency_key = "invalidate-new-key".into();
    }
    assert_eq!(
        f.store()
            .request_google_mutation(REGISTRATION, 1, &alternate),
        Err(ErrorCode::Conflict)
    );
}
