use super::*;
use hantong_contracts::{
    ErrorCode,
    auth::{RequestedScope, default_cli_legacy_tools, default_cli_manage_tools},
    imap_trash::ImapTrashRequest,
    terminal_review::{
        TerminalReviewCancel, TerminalReviewCommand, TerminalReviewConfirm, TerminalReviewGet,
        TerminalReviewKind,
    },
    uid_page::{UidMailboxEncoding, UidMailboxIdentity},
};
use hantong_domain::{
    capabilities::ImapCapabilities,
    imap_sync::{
        ImapEnvelope, ImapEnvelopeInput, ImapMessageMetadata, ImapUidPage, ImapUidPageRequest,
    },
    mail_connection::MailConnectionConfig,
    mailboxes::{
        ImapMailboxDescriptor, ImapMailboxName, ImapMailboxSnapshot, MailboxChildren,
        MailboxEncoding, SpecialUseRole,
    },
};
use hantong_platform::ServerSigningKey;
use hantong_secrets::KeyedConnection;
use hantong_store::{
    CliRegistrationStage, ClientKind, DEFAULT_CLI_LABEL, Store,
    imap_catalog::imap_catalog_payload_hash,
    imap_sync::imap_uid_page_payload_hash,
    imap_trash::{ImapTrashOutcome, PendingImapTrash},
    mail_connection::{MailMutation, imap_payload_hash},
};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll},
};

const ACCOUNT: &str = "imap-trash-core-account";
struct Fixture {
    root: std::path::PathBuf,
    store: Option<Store>,
    owner: [u8; 16],
    epoch: u64,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-imap-trash-core-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let data = root.join("data");
        let keys = root.join("keys");
        drop(Store::open(&data, &keys).unwrap());
        let public_key = public_key();
        let server_pin = hantong_platform::sha256(&public_key).unwrap();
        let keyed = KeyedConnection::open(&data, &keys).unwrap();
        keyed
            .with_connection(|c| {
                c.execute(
                    "INSERT INTO server_identity(singleton,key_id,state,public_key,key_epoch,version,created_at_utc_ms,updated_at_utc_ms) VALUES(1,?1,'active',?2,1,1,1,1)",
                    (server_pin.as_slice(), public_key.as_slice()),
                )?;
                c.execute(
                    "INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES(?1,'imap',1,'ready','[]','[]',1)",
                    [ACCOUNT],
                )?;
                c.execute(
                    "INSERT INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES(?1,'imap',1,'imap.example.test',993,'implicit_tls','imap-user','app_password',1)",
                    [ACCOUNT],
                )?;
                Ok(())
            })
            .unwrap();
        drop(keyed);
        let store = Store::open(&data, &keys).unwrap();
        let request = store
            .create_registration_request(
                [1_u8; 32],
                ClientKind::Cli,
                DEFAULT_CLI_LABEL,
                &RequestedScope::new(vec![ACCOUNT.into()], default_cli_legacy_tools()).unwrap(),
            )
            .unwrap();
        let registration_id =
            hantong_platform::registration_id_for_request(request.request_id).unwrap();
        let staged = store
            .stage_terminal_cli_registration(
                &CliRegistrationStage::new_terminal_cli(
                    request,
                    registration_id,
                    public_key,
                    server_pin,
                )
                .unwrap(),
            )
            .unwrap();
        store
            .activate_cli_registration(registration_id, staged.version, server_pin)
            .unwrap();
        let refresh = store
            .create_terminal_cli_scope_refresh(
                [2_u8; 32],
                RequestedScope::new(vec![ACCOUNT.into()], default_cli_manage_tools()).unwrap(),
                server_pin,
            )
            .unwrap();
        let staged = store.stage_cli_scope_refresh(&refresh).unwrap();
        let active = store.activate_cli_scope_refresh(&staged).unwrap();
        let fixture = Self {
            root,
            store: Some(store),
            owner: active.registration_id,
            epoch: active.profile_epoch,
        };
        fixture.seed_catalog_and_page();
        fixture
    }

    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }

    fn seed_catalog_and_page(&self) {
        let endpoint = MailConnectionConfig::from_json(
            br#"{"schema_version":1,"account_id":"imap-trash-core-account","imap":{"settings_version":1,"host":"imap.example.test","port":993,"tls":"implicit_tls","login_name":"imap-user","authentication":"app_password"}}"#,
        )
        .unwrap();
        let snapshot = ImapMailboxSnapshot::new(
            ACCOUNT,
            1,
            ImapCapabilities::from_features([hantong_domain::capabilities::ImapFeature::Move]),
            vec![
                ImapMailboxDescriptor::new(
                    ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::Utf8).unwrap(),
                    Some('/'),
                    false,
                    false,
                    MailboxChildren::HasNoChildren,
                    vec![],
                )
                .unwrap(),
                ImapMailboxDescriptor::new(
                    ImapMailboxName::from_wire(b"Trash", MailboxEncoding::Utf8).unwrap(),
                    Some('/'),
                    false,
                    false,
                    MailboxChildren::HasNoChildren,
                    vec![SpecialUseRole::Trash],
                )
                .unwrap(),
            ],
        )
        .unwrap();
        let catalog_hash = imap_catalog_payload_hash(1, endpoint.imap(), &snapshot, 0).unwrap();
        let catalog_mutation = MailMutation::new(
            "core-trash-catalog".into(),
            "core-trash-catalog-key".into(),
            0,
            catalog_hash,
        )
        .unwrap();
        assert_eq!(
            self.store()
                .replace_imap_mailbox_snapshot(1, endpoint.imap(), &snapshot, &catalog_mutation)
                .unwrap(),
            1
        );

        let mailbox = ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::Utf8).unwrap();
        let page = ImapUidPage::new(
            ImapUidPageRequest::new(ACCOUNT, 1, mailbox, None, 0).unwrap(),
            7,
            2,
            vec![
                ImapMessageMetadata::new(
                    1,
                    vec!["\\Seen".into()],
                    0,
                    42,
                    ImapEnvelope::new(ImapEnvelopeInput {
                        subject: Some(b"synthetic trash fixture".to_vec()),
                        ..Default::default()
                    })
                    .unwrap(),
                )
                .unwrap(),
            ],
        )
        .unwrap();
        let page_hash = imap_uid_page_payload_hash(1, 1, endpoint.imap(), &page, 0).unwrap();
        let page_mutation = MailMutation::new(
            "core-trash-page".into(),
            "core-trash-page-key".into(),
            0,
            page_hash,
        )
        .unwrap();
        self.store()
            .apply_imap_uid_page(1, 1, endpoint.imap(), &page, &page_mutation)
            .unwrap();
    }

    fn request(&self, operation: &str, idempotency: &str) -> ImapTrashRequest {
        let mailbox = UidMailboxIdentity::new(
            &hantong_contracts::wire_hex::encode(b"INBOX"),
            UidMailboxEncoding::Utf8,
        )
        .unwrap();
        let initial = ImapTrashRequest::new(
            ACCOUNT,
            mailbox.clone(),
            1,
            7,
            1,
            operation,
            idempotency,
            [0; 32],
        )
        .unwrap();
        let hash = hantong_platform::sha256_storage_payload(&initial.payload_bytes()).unwrap();
        ImapTrashRequest::new(ACCOUNT, mailbox, 1, 7, 1, operation, idempotency, hash).unwrap()
    }

    fn peer() -> hantong_contracts::auth::PeerContext {
        hantong_contracts::auth::PeerContext::new("S-1-5-21-1-2-3-1001", "S-1-5-5-1-2").unwrap()
    }

    fn pending(&self, operation: &str, idempotency: &str) -> PendingImapTrash {
        let receipt = self
            .store()
            .request_imap_trash(
                self.owner,
                self.epoch,
                &self.request(operation, idempotency),
            )
            .unwrap();
        self.store().pending_imap_trash(receipt.review_id).unwrap()
    }

    fn bump_endpoint_version(&self) {
        let endpoint = MailConnectionConfig::from_json(
            br#"{"schema_version":1,"account_id":"imap-trash-core-account","imap":{"settings_version":2,"host":"changed.example.test","port":993,"tls":"implicit_tls","login_name":"imap-user","authentication":"app_password"}}"#,
        )
        .unwrap();
        let endpoint = endpoint.imap().clone();
        let hash = imap_payload_hash(ACCOUNT, &endpoint, 1).unwrap();
        let mutation = MailMutation::new(
            "binding-update".into(),
            "binding-update-key".into(),
            1,
            hash,
        )
        .unwrap();
        self.store()
            .put_imap_connection(ACCOUNT, &endpoint, &mutation)
            .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.store.take());
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn public_key() -> [u8; 72] {
    let mut key = [0_u8; 72];
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
    key
}

fn run<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(future)
}

#[test]
fn apr01_core_imap_trash_request_is_pending_and_confirm_enqueues_once() {
    let fixture = Fixture::new();
    let server = ServerSigningKey::create(hantong_platform::random_nonce().unwrap()).unwrap();
    let (handle, worker) =
        crate::registration::coordinator(fixture.store(), &server, &fixture.root);
    let request = fixture.request("core-request", "core-request-key");
    let receipt = handle
        .request_imap_trash(fixture.owner, fixture.epoch, &request)
        .unwrap();
    assert_eq!(receipt.state, "pending");
    assert!(!worker.has_pending_job());
    let peer = Fixture::peer();
    let preview = handle
        .terminal_review_command(
            TerminalReviewCommand::Get(
                TerminalReviewGet::new(
                    TerminalReviewKind::ImapTrash,
                    receipt.review_id,
                    Some(ACCOUNT.into()),
                    [0; 32],
                )
                .unwrap(),
            ),
            &peer,
        )
        .unwrap();
    assert_eq!(preview.state, "pending");
    assert!(!worker.has_pending_job());
    let confirm =
        TerminalReviewConfirm::new(preview.ticket.unwrap(), preview.snapshot_hash).unwrap();
    let queued = handle
        .terminal_review_command(TerminalReviewCommand::Confirm(confirm.clone()), &peer)
        .unwrap();
    assert_eq!(queued.state, "queued");
    assert!(worker.has_pending_job());
    assert_eq!(
        handle.terminal_review_command(TerminalReviewCommand::Confirm(confirm), &peer),
        Err(ErrorCode::NotFound)
    );
}

#[test]
fn apr01_core_imap_trash_cancel_expiry_and_binding_mismatch_never_enqueue() {
    let peer = Fixture::peer();
    {
        let fixture = Fixture::new();
        let server = ServerSigningKey::create(hantong_platform::random_nonce().unwrap()).unwrap();
        let (handle, worker) =
            crate::registration::coordinator(fixture.store(), &server, &fixture.root);
        let receipt = handle
            .request_imap_trash(
                fixture.owner,
                fixture.epoch,
                &fixture.request("cancel", "cancel-key"),
            )
            .unwrap();
        let get = TerminalReviewGet::new(
            TerminalReviewKind::ImapTrash,
            receipt.review_id,
            Some(ACCOUNT.into()),
            [0; 32],
        )
        .unwrap();
        let preview = handle
            .terminal_review_command(TerminalReviewCommand::Get(get.clone()), &peer)
            .unwrap();
        let cancelled = handle
            .terminal_review_command(
                TerminalReviewCommand::Cancel(
                    TerminalReviewCancel::new(get, preview.ticket.unwrap()).unwrap(),
                ),
                &peer,
            )
            .unwrap();
        assert_eq!(cancelled.state, "denied");
        assert!(!worker.has_pending_job());
    }
    {
        let fixture = Fixture::new();
        let server = ServerSigningKey::create(hantong_platform::random_nonce().unwrap()).unwrap();
        let (handle, worker) =
            crate::registration::coordinator(fixture.store(), &server, &fixture.root);
        let receipt = handle
            .request_imap_trash(
                fixture.owner,
                fixture.epoch,
                &fixture.request("expired", "expired-key"),
            )
            .unwrap();
        let get = TerminalReviewGet::new(
            TerminalReviewKind::ImapTrash,
            receipt.review_id,
            Some(ACCOUNT.into()),
            [0; 32],
        )
        .unwrap();
        let preview = handle
            .terminal_review_command(TerminalReviewCommand::Get(get), &peer)
            .unwrap();
        fixture
            .store()
            .deny_imap_trash(receipt.review_id, ErrorCode::ApprovalExpired)
            .unwrap();
        assert_eq!(
            handle.terminal_review_command(
                TerminalReviewCommand::Confirm(
                    TerminalReviewConfirm::new(preview.ticket.unwrap(), preview.snapshot_hash,)
                        .unwrap(),
                ),
                &peer,
            ),
            Err(ErrorCode::Conflict)
        );
        assert!(!worker.has_pending_job());
    }
    {
        let fixture = Fixture::new();
        let server = ServerSigningKey::create(hantong_platform::random_nonce().unwrap()).unwrap();
        let (handle, worker) =
            crate::registration::coordinator(fixture.store(), &server, &fixture.root);
        let receipt = handle
            .request_imap_trash(
                fixture.owner,
                fixture.epoch,
                &fixture.request("binding", "binding-key"),
            )
            .unwrap();
        let get = TerminalReviewGet::new(
            TerminalReviewKind::ImapTrash,
            receipt.review_id,
            Some(ACCOUNT.into()),
            [0; 32],
        )
        .unwrap();
        let preview = handle
            .terminal_review_command(TerminalReviewCommand::Get(get), &peer)
            .unwrap();
        fixture.bump_endpoint_version();
        assert_eq!(
            handle.terminal_review_command(
                TerminalReviewCommand::Confirm(
                    TerminalReviewConfirm::new(preview.ticket.unwrap(), preview.snapshot_hash,)
                        .unwrap(),
                ),
                &peer,
            ),
            Err(ErrorCode::Conflict)
        );
        assert!(!worker.has_pending_job());
    }
}

struct ProbeFuture<'a> {
    store: &'a Store,
    owner: [u8; 16],
    epoch: u64,
    review_id: [u8; 32],
    polls: Arc<AtomicUsize>,
    saw_moving: Arc<AtomicBool>,
    outcome: ImapTrashOutcome,
}

impl Future for ProbeFuture<'_> {
    type Output = Result<ImapTrashOutcome, ErrorCode>;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        let state = self
            .store
            .status_imap_trash(self.owner, self.epoch, self.review_id)
            .unwrap()
            .state;
        self.saw_moving.store(state == "moving", Ordering::SeqCst);
        Poll::Ready(Ok(self.outcome))
    }
}

#[test]
fn apr01_core_imap_trash_claims_before_provider_poll_and_unknown_is_not_retried() {
    let fixture = Fixture::new();
    let pending = fixture.pending("execute", "execute-key");
    let policy = std::rc::Rc::new(std::cell::RefCell::new(hantong_policy::ReadPolicy::new()));
    let polls = Arc::new(AtomicUsize::new(0));
    let saw_moving = Arc::new(AtomicBool::new(false));
    let result = run(execute_verified(
        fixture.store(),
        &policy,
        &pending,
        ProbeFuture {
            store: fixture.store(),
            owner: fixture.owner,
            epoch: fixture.epoch,
            review_id: pending.review_id,
            polls: polls.clone(),
            saw_moving: saw_moving.clone(),
            outcome: ImapTrashOutcome::Unknown,
        },
    ));
    assert_eq!(result, Ok(()));
    assert_eq!(polls.load(Ordering::SeqCst), 1);
    assert!(saw_moving.load(Ordering::SeqCst));
    assert_eq!(
        fixture
            .store()
            .status_imap_trash(fixture.owner, fixture.epoch, pending.review_id)
            .unwrap()
            .state,
        "unknown"
    );
    let second_polls = Arc::new(AtomicUsize::new(0));
    let second = run(execute_verified(
        fixture.store(),
        &policy,
        &pending,
        ProbeFuture {
            store: fixture.store(),
            owner: fixture.owner,
            epoch: fixture.epoch,
            review_id: pending.review_id,
            polls: second_polls.clone(),
            saw_moving: Arc::new(AtomicBool::new(false)),
            outcome: ImapTrashOutcome::Moved,
        },
    ));
    assert_eq!(second, Err(ErrorCode::Conflict));
    assert_eq!(second_polls.load(Ordering::SeqCst), 0);
}

#[test]
fn apr01_core_imap_trash_invalid_atomic_claim_never_polls_provider() {
    let fixture = Fixture::new();
    let pending = fixture.pending("invalid-claim", "invalid-claim-key");
    fixture.bump_endpoint_version();
    let policy = std::rc::Rc::new(std::cell::RefCell::new(hantong_policy::ReadPolicy::new()));
    let polls = Arc::new(AtomicUsize::new(0));
    let result = run(execute_verified(
        fixture.store(),
        &policy,
        &pending,
        ProbeFuture {
            store: fixture.store(),
            owner: fixture.owner,
            epoch: fixture.epoch,
            review_id: pending.review_id,
            polls: polls.clone(),
            saw_moving: Arc::new(AtomicBool::new(false)),
            outcome: ImapTrashOutcome::Moved,
        },
    ));
    assert_eq!(result, Err(ErrorCode::Conflict));
    assert_eq!(polls.load(Ordering::SeqCst), 0);
}
