use super::*;

#[test]
fn apr01_imap_trash_terminal_snapshot_binds_destination_owner_epoch_and_expiry() {
    use hantong_contracts::{
        imap_trash::ImapTrashRequest,
        uid_page::{UidMailboxEncoding, UidMailboxIdentity},
    };
    use hantong_store::imap_trash::{ImapTrashDestination, PendingImapTrash};
    let config = hantong_domain::mail_connection::MailConnectionConfig::from_json(
        br#"{"schema_version":1,"account_id":"synthetic_imap","imap":{"settings_version":1,"host":"imap.example.test","port":993,"tls":"implicit_tls","login_name":"synthetic@example.test","authentication":"app_password"}}"#,
    ).unwrap();
    let pending = PendingImapTrash {
        review_id: [7; 32],
        owner_registration_id: [8; 16],
        profile_epoch: 2,
        account_version: 1,
        endpoint: config.imap().clone(),
        request: ImapTrashRequest::new(
            "synthetic_imap",
            UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::Utf8).unwrap(),
            1,
            77,
            12,
            "op",
            "idem",
            [9; 32],
        )
        .unwrap(),
        destination: ImapTrashDestination {
            mailbox_wire_hex: "5472617368".into(),
            mailbox_encoding: "utf8".into(),
        },
        created_at_utc_ms: 1000,
        expires_at_utc_ms: 301000,
    };
    let expected = imap_trash_terminal_hash(&pending).unwrap();
    for changed in [
        {
            let mut p = pending.clone();
            p.destination.mailbox_wire_hex = "4f74686572".into();
            p
        },
        {
            let mut p = pending.clone();
            p.owner_registration_id = [6; 16];
            p
        },
        {
            let mut p = pending.clone();
            p.profile_epoch += 1;
            p
        },
        {
            let mut p = pending.clone();
            p.expires_at_utc_ms += 1;
            p
        },
        {
            let mut p = pending.clone();
            p.account_version += 1;
            p
        },
    ] {
        assert_ne!(imap_trash_terminal_hash(&changed).unwrap(), expected);
    }
}
use std::cell::RefCell;

fn public() -> [u8; 72] {
    let mut key = [0; 72];
    key[..8].copy_from_slice(b"ECS1\x20\0\0\0");
    // Exact public P-256 generator; no private key or actual registration.
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
fn request() -> RegistrationRequest {
    RegistrationRequest {
        request_id: [1; 32],
        client_kind: ClientKind::Cli,
        display_label: DEFAULT_CLI_LABEL.into(),
        scope: RequestedScope::new(vec!["a".into()], vec!["mail.accounts.list".into()]).unwrap(),
        state: RegistrationRequestState::Pending,
        version: 1,
        created_at_utc_ms: 1000,
        expires_at_utc_ms: 301000,
    }
}
#[test]
fn apr01_terminal_registration_hash_binds_full_typed_snapshot() {
    let original = request();
    let expected = registration_terminal_hash(&original).unwrap();
    for changed in [
        RegistrationRequest {
            version: original.version + 1,
            ..original.clone()
        },
        RegistrationRequest {
            expires_at_utc_ms: original.expires_at_utc_ms + 1,
            ..original.clone()
        },
        RegistrationRequest {
            created_at_utc_ms: original.created_at_utc_ms + 1,
            ..original.clone()
        },
        RegistrationRequest {
            client_kind: ClientKind::Mcp,
            ..original.clone()
        },
        RegistrationRequest {
            display_label: "Different".into(),
            ..original.clone()
        },
    ] {
        assert_ne!(expected, registration_terminal_hash(&changed).unwrap());
    }
}
fn review() -> RegistrationReview {
    RegistrationReview::new(
        [1; 32],
        1,
        vec!["a".into()],
        hantong_platform::sha256(&public()).unwrap(),
        1000,
        301000,
    )
    .unwrap()
}
fn record() -> RegistrationRecord {
    RegistrationRecord {
        registration_id: hantong_platform::registration_id_for_request([1; 32]).unwrap(),
        request_id: [1; 32],
        client_kind: ClientKind::Cli,
        display_label: DEFAULT_CLI_LABEL.into(),
        public_key: public(),
        profile_epoch: 1,
        state: RegistrationState::Provisioning,
        read_only: true,
        requests_per_minute: 60,
        approved_channel: "approval_helper".into(),
        version: 1,
        scope: request().scope,
    }
}
fn gui_request() -> RegistrationRequest {
    let mut value = request();
    value.client_kind = ClientKind::Gui;
    value.display_label = DEFAULT_GUI_LABEL.into();
    value.scope =
        RequestedScope::new(vec!["a".into()], hantong_contracts::auth::gui_read_tools()).unwrap();
    value
}

fn ai_request(provider: AiProvider) -> RegistrationRequest {
    RegistrationRequest {
        request_id: [7; 32],
        client_kind: ClientKind::Ai,
        display_label: match provider {
            AiProvider::Codex => DEFAULT_AI_CODEX_LABEL.into(),
            AiProvider::Claude => DEFAULT_AI_CLAUDE_LABEL.into(),
        },
        scope: RequestedScope::new(vec!["a".into()], hantong_contracts::auth::ai_mail_tools())
            .unwrap(),
        state: RegistrationRequestState::Pending,
        version: 1,
        created_at_utc_ms: 1000,
        expires_at_utc_ms: 301000,
    }
}

fn ai_record(provider: AiProvider) -> RegistrationRecord {
    RegistrationRecord {
        registration_id: hantong_platform::registration_id_for_request([7; 32]).unwrap(),
        request_id: [7; 32],
        client_kind: ClientKind::Ai,
        display_label: match provider {
            AiProvider::Codex => DEFAULT_AI_CODEX_LABEL.into(),
            AiProvider::Claude => DEFAULT_AI_CLAUDE_LABEL.into(),
        },
        public_key: public(),
        profile_epoch: 1,
        state: RegistrationState::Provisioning,
        read_only: false,
        requests_per_minute: 60,
        approved_channel: "ai_feature".into(),
        version: 1,
        scope: ai_request(provider).scope,
    }
}

fn ai_review(provider: AiProvider) -> RegistrationReview {
    review_snapshot(
        &ai_request(provider),
        hantong_platform::sha256(&public()).unwrap(),
    )
    .unwrap()
}

#[test]
fn ai09_registration_review_is_provider_bound_and_mail_scope_explicit() {
    let codex = review_snapshot(&ai_request(AiProvider::Codex), [2; 32]).unwrap();
    let claude = review_snapshot(&ai_request(AiProvider::Claude), [2; 32]).unwrap();
    assert_eq!(codex.client_kind(), ClientKind::Ai);
    assert_eq!(codex.profile_name(), "ai-codex");
    assert_eq!(claude.profile_name(), "ai-claude");
    assert_eq!(
        codex.scope().tools(),
        hantong_contracts::auth::ai_mail_tools()
    );
    assert_ne!(codex.canonical_bytes(), claude.canonical_bytes());
}

#[test]
fn ai09_registration_commit_and_recovery_keep_ai_profile_and_channel() {
    let f = Fake::ai(AiProvider::Codex);
    let expected = ai_request(AiProvider::Codex);
    assert_eq!(
        commit_consumed_review(&f, &expected, &ai_review(AiProvider::Codex)),
        Ok(())
    );
    assert_eq!(
        f.events(),
        [
            "request",
            "server",
            "open",
            "create",
            "key_public",
            "stage",
            "publish",
            "activate",
            "release_publication"
        ]
    );
    f.events.borrow_mut().clear();
    f.current.borrow_mut().state = RegistrationRequestState::Approved;
    f.current.borrow_mut().version = 2;
    f.missing.set(false);
    assert_eq!(recover_record(&f, &ai_record(AiProvider::Codex)), Ok(()));
    assert_eq!(
        f.events(),
        [
            "request",
            "server",
            "open",
            "key_public",
            "publish",
            "activate",
            "release_publication"
        ]
    );
}

#[test]
fn ai09_expired_revoked_or_account_mismatched_review_has_no_effect() {
    for mode in 0..3 {
        let mut f = Fake::ai(AiProvider::Claude);
        let mut expected = ai_request(AiProvider::Claude);
        let mut review = ai_review(AiProvider::Claude);
        match mode {
            0 => f.now = expected.expires_at_utc_ms,
            1 => {
                expected.state = RegistrationRequestState::Denied;
                *f.current.borrow_mut() = expected.clone();
            }
            _ => {
                expected.scope = RequestedScope::new(
                    vec!["different-account".into()],
                    hantong_contracts::auth::ai_mail_tools(),
                )
                .unwrap();
                review = ai_review(AiProvider::Claude);
            }
        }
        assert!(commit_consumed_review(&f, &expected, &review).is_err());
        assert!(
            !f.events()
                .iter()
                .any(|event| ["create", "stage", "publish", "activate"].contains(event))
        );
    }
}

#[cfg(windows)]
#[test]
fn ai09_request_is_pending_only_and_never_enqueues_a_background_job() {
    let fixture = crate::send_mail::integration_tests::Fixture::new();
    let server = ServerSigningKey::create(hantong_platform::random_nonce().unwrap()).unwrap();
    let (handle, mut worker) = coordinator(fixture.store(), &server, fixture.secrets_root());
    let receipt = handle
        .command(RegistrationCommand::AiRequest(AiProvider::Codex))
        .unwrap();
    assert_eq!(receipt.state(), RegistrationProgress::Pending);
    let stored = fixture
        .store()
        .get_registration_request(*receipt.request_id())
        .unwrap()
        .unwrap();
    assert_eq!(stored.scope.accounts(), ["a"]);
    let peer =
        hantong_contracts::auth::PeerContext::new("S-1-5-21-1-2-3-4", "S-1-5-5-1-2").unwrap();
    let preview = handle
        .terminal_review_command(
            hantong_contracts::terminal_review::TerminalReviewCommand::Get(
                hantong_contracts::terminal_review::TerminalReviewGet::new(
                    hantong_contracts::terminal_review::TerminalReviewKind::Registration,
                    *receipt.request_id(),
                    None,
                    [0; 32],
                )
                .unwrap(),
            ),
            &peer,
        )
        .unwrap();
    let expected = fixture
        .store()
        .list_account_summaries(stored.scope.accounts(), false)
        .unwrap()
        .items
        .remove(0);
    let label = expected
        .primary_address
        .or(expected.display_name)
        .unwrap_or_else(|| "Account 1".into());
    assert_eq!(
        preview
            .fields
            .iter()
            .find(|field| field.label == "accounts")
            .unwrap()
            .value,
        label
    );
    assert_eq!(
        fixture
            .store()
            .get_registration_request(*receipt.request_id())
            .unwrap()
            .unwrap()
            .scope,
        stored.scope
    );
    assert!(matches!(
        handle.command(RegistrationCommand::AiStatus(
            AiProvider::Claude,
            *receipt.request_id()
        )),
        Err(ErrorCode::ScopeDenied)
    ));
    assert_eq!(
        handle
            .command(RegistrationCommand::AiStatus(
                AiProvider::Codex,
                *receipt.request_id()
            ))
            .unwrap()
            .state(),
        RegistrationProgress::Pending
    );
    assert_eq!(
        stored.scope.tools(),
        hantong_contracts::auth::ai_mail_tools()
    );
    assert!(worker.receiver.try_recv().is_err());
}

#[test]
fn apr01_terminal_personal_prepare_has_no_job_and_confirm_queues_only_terminal_job() {
    let fixture = crate::send_mail::integration_tests::Fixture::new();
    let server = ServerSigningKey::create(hantong_platform::random_nonce().unwrap()).unwrap();
    let (handle, mut worker) = coordinator(fixture.store(), &server, fixture.secrets_root());
    let pending = fixture.prepare_personal();
    let prepared = handle
        .prepare_personal_send(
            pending.owner_registration_id,
            pending.profile_epoch,
            &pending.request,
        )
        .unwrap();
    assert_eq!(prepared["state"], "pending");
    assert!(prepared["preview"].is_object());
    assert!(worker.receiver.try_recv().is_err());

    let confirm = hantong_contracts::mail_send::PersonalSendConfirmRequest::new(
        pending.request.account_id(),
        pending.review_id,
        *pending.request.payload_hash(),
    )
    .unwrap();
    let receipt = handle
        .confirm_personal_send(
            pending.owner_registration_id,
            pending.profile_epoch,
            &confirm,
        )
        .unwrap();
    assert_eq!(receipt.state, "pending");
    assert_eq!(
        worker.receiver.try_recv().unwrap(),
        Job::TerminalSend(pending.review_id)
    );
    assert!(worker.receiver.try_recv().is_err());

    // Replaying the same explicit terminal action reuses the queued result and
    // cannot enqueue a second provider attempt.
    let replay = handle
        .confirm_personal_send(
            pending.owner_registration_id,
            pending.profile_epoch,
            &confirm,
        )
        .unwrap();
    assert_eq!(replay.review_id, receipt.review_id);
    assert!(worker.receiver.try_recv().is_err());
}

#[test]
fn apr01_terminal_personal_cancel_and_binding_failures_have_no_job_or_outbox() {
    let fixture = crate::send_mail::integration_tests::Fixture::new();
    let server = ServerSigningKey::create(hantong_platform::random_nonce().unwrap()).unwrap();
    let (handle, mut worker) = coordinator(fixture.store(), &server, fixture.secrets_root());
    let pending = fixture.prepare_personal();
    let wrong_owner = [8; 16];
    let confirm = hantong_contracts::mail_send::PersonalSendConfirmRequest::new(
        pending.request.account_id(),
        pending.review_id,
        *pending.request.payload_hash(),
    )
    .unwrap();
    assert_eq!(
        handle.confirm_personal_send(wrong_owner, pending.profile_epoch, &confirm),
        Err(ErrorCode::ClientNotRegistered)
    );
    let bad_hash = hantong_contracts::mail_send::PersonalSendConfirmRequest::new(
        pending.request.account_id(),
        pending.review_id,
        [7; 32],
    )
    .unwrap();
    assert_eq!(
        handle.confirm_personal_send(
            pending.owner_registration_id,
            pending.profile_epoch,
            &bad_hash
        ),
        Err(ErrorCode::PolicyBlocked)
    );
    assert!(worker.receiver.try_recv().is_err());

    let status = hantong_contracts::mail_send::SendReviewStatusRequest {
        account_id: pending.request.account_id().into(),
        review_id: pending.review_id,
    };
    let cancelled = handle
        .cancel_personal_send(
            pending.owner_registration_id,
            pending.profile_epoch,
            &status,
        )
        .unwrap();
    assert_eq!(cancelled.state, "interrupted");
    assert_eq!(cancelled.error, Some(ErrorCode::ApprovalRequired));
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(
        fixture
            .store()
            .mail_send_receipt(
                pending.owner_registration_id,
                pending.profile_epoch,
                pending.request.account_id(),
                pending.review_id,
            )
            .unwrap()
            .state,
        "interrupted"
    );

    // The helper-origin SMTP registration is refused before any terminal job.
    let helper = fixture.prepare();
    assert_eq!(
        handle.prepare_personal_send(
            helper.owner_registration_id,
            helper.profile_epoch,
            &helper.request
        ),
        Err(ErrorCode::PolicyBlocked)
    );
    assert!(worker.receiver.try_recv().is_err());
}

#[test]
fn acc02_existing_personal_read_upgrades_only_same_account_from_exact_oauth_profile() {
    use hantong_contracts::google_setup::GoogleLocalProfile::*;
    let account = format!("google_{}", "a".repeat(64));
    let mut current = record();
    current.state = RegistrationState::Active;
    current.approved_channel = "google_oauth".into();
    current.scope = RequestedScope::new(
        vec![account.clone()],
        hantong_contracts::auth::default_cli_legacy_tools(),
    )
    .unwrap();
    assert!(existing_personal_registration_supported(
        &current,
        &account,
        PersonalRead
    ));
    current.profile_epoch = 2;
    current.scope = RequestedScope::new(
        vec![account.clone()],
        hantong_contracts::auth::default_cli_body_tools(),
    )
    .unwrap();
    assert!(existing_personal_registration_supported(
        &current,
        &account,
        PersonalReadSend
    ));
    assert!(!existing_personal_registration_supported(
        &current,
        &account,
        PersonalRead
    ));
    assert!(!existing_personal_registration_supported(
        &current,
        "different",
        PersonalReadSend
    ));
    for source in ["approval_helper", "gui"] {
        current.approved_channel = source.into();
        assert!(!existing_personal_registration_supported(
            &current,
            &account,
            PersonalReadSend
        ));
    }
    current.approved_channel = "google_oauth".into();
    current.profile_epoch = 3;
    assert!(!existing_personal_registration_supported(
        &current,
        &account,
        PersonalReadSend
    ));
    current.profile_epoch = 2;
    current.client_kind = ClientKind::Mcp;
    assert!(!existing_personal_registration_supported(
        &current,
        &account,
        PersonalReadSend
    ));
}
#[test]
fn acc02_google_reconnect_accepts_only_exact_active_oauth_grants_without_new_scope() {
    use hantong_contracts::{auth::*, google_setup::GoogleLocalProfile::*};
    let account = format!("google_{}", "a".repeat(64));
    let mut current = record();
    current.state = RegistrationState::Active;
    current.approved_channel = "google_oauth".into();
    for (profile, tools, epoch) in [
        (PersonalRead, default_cli_body_tools(), 2),
        (PersonalReadSend, default_cli_send_tools(), 2),
        (PersonalReadSend, default_cli_send_tools(), 3),
    ] {
        current.profile_epoch = epoch;
        current.scope = RequestedScope::new(vec![account.clone()], tools).unwrap();
        assert!(existing_personal_registration_is_current(
            &current, &account, profile
        ));
        assert!(!existing_personal_registration_supported(
            &current, &account, profile
        ));
        assert!(!existing_personal_registration_is_current(
            &current, "other", profile
        ));
        for channel in ["approval_helper", "terminal_cli", "ai_feature"] {
            current.approved_channel = channel.into();
            assert!(!existing_personal_registration_is_current(
                &current, &account, profile
            ));
        }
        current.approved_channel = "google_oauth".into();
        for state in [RegistrationState::Provisioning, RegistrationState::Revoked] {
            current.state = state;
            assert!(!existing_personal_registration_is_current(
                &current, &account, profile
            ));
        }
        current.state = RegistrationState::Active;
        current.profile_epoch = 4;
        assert!(!existing_personal_registration_is_current(
            &current, &account, profile
        ));
    }
    current.profile_epoch = 2;
    current.scope = RequestedScope::new(vec![account.clone()], default_cli_send_tools()).unwrap();
    assert!(!existing_personal_registration_is_current(
        &current,
        &account,
        PersonalRead
    ));
    assert!(!existing_personal_registration_is_current(
        &current, &account, None
    ));
    current.client_kind = ClientKind::Mcp;
    assert!(!existing_personal_registration_is_current(
        &current,
        &account,
        PersonalReadSend
    ));
}

#[test]
fn acc02_second_read_account_hands_off_without_broadening_personal_oauth_grants() {
    use hantong_contracts::{auth::*, google_setup::GoogleLocalProfile::*};
    let mut current = record();
    current.state = RegistrationState::Active;
    current.profile_epoch = 2;
    current.approved_channel = "google_oauth".into();
    current.scope = RequestedScope::new(vec!["first".into()], default_cli_body_tools()).unwrap();
    assert!(needs_terminal_account_access(
        &current,
        "second",
        PersonalRead
    ));
    assert!(!needs_terminal_account_access(
        &current,
        "first",
        PersonalRead
    ));
    assert!(!needs_terminal_account_access(
        &current,
        "second",
        PersonalReadSend
    ));
    for channel in ["approval_helper", "terminal_cli", "ai_feature", "unknown"] {
        current.approved_channel = channel.into();
        assert!(!needs_terminal_account_access(
            &current,
            "second",
            PersonalRead
        ));
    }
    current.approved_channel = "google_oauth".into();
    for epoch in [0, 1, 3, u64::MAX] {
        current.profile_epoch = epoch;
        assert!(!needs_terminal_account_access(
            &current,
            "second",
            PersonalRead
        ));
    }
    current.profile_epoch = 3;
    current.scope = RequestedScope::new(
        vec!["first".into(), "second".into()],
        default_cli_body_tools(),
    )
    .unwrap();
    current.profile_epoch = 2;
    assert!(!needs_terminal_account_access(
        &current,
        "third",
        PersonalRead
    ));
    current.profile_epoch = 3;
    assert!(needs_terminal_account_access(
        &current,
        "second",
        PersonalRead
    ));
    assert!(needs_terminal_account_access(
        &current,
        "third",
        PersonalRead
    ));
    for tools in [default_cli_legacy_tools(), default_cli_send_tools()] {
        current.scope = RequestedScope::new(vec!["first".into()], tools).unwrap();
        assert!(!needs_terminal_account_access(
            &current,
            "second",
            PersonalRead
        ));
    }
    current.scope = RequestedScope::new(vec!["first".into()], default_cli_body_tools()).unwrap();
    for state in [RegistrationState::Provisioning, RegistrationState::Revoked] {
        current.state = state;
        assert!(!needs_terminal_account_access(
            &current,
            "second",
            PersonalRead
        ));
    }
    current.state = RegistrationState::Active;
    current.client_kind = ClientKind::Mcp;
    assert!(!needs_terminal_account_access(
        &current,
        "second",
        PersonalRead
    ));
    current.client_kind = ClientKind::Cli;
    current.display_label = "other".into();
    assert!(!needs_terminal_account_access(
        &current,
        "second",
        PersonalRead
    ));
}

#[test]
fn acc02_second_send_account_preserves_exact_send_tools_for_terminal_selection() {
    use hantong_contracts::{auth::*, google_setup::GoogleLocalProfile::*};
    let mut current = record();
    current.state = RegistrationState::Active;
    current.approved_channel = "google_oauth".into();
    current.scope = RequestedScope::new(vec!["first".into()], default_cli_send_tools()).unwrap();
    for epoch in [2, 3] {
        current.profile_epoch = epoch;
        assert!(needs_terminal_account_access(
            &current,
            "second",
            PersonalReadSend
        ));
        assert!(!needs_terminal_account_access(
            &current,
            "second",
            PersonalRead
        ));
    }
    for epoch in [0, 1, 4, u64::MAX] {
        current.profile_epoch = epoch;
        assert!(!needs_terminal_account_access(
            &current,
            "second",
            PersonalReadSend
        ));
    }
    current.profile_epoch = 3;
    current.scope = RequestedScope::new(
        vec!["first".into(), "second".into()],
        default_cli_send_tools(),
    )
    .unwrap();
    assert!(needs_terminal_account_access(
        &current,
        "third",
        PersonalReadSend
    ));
    for channel in ["approval_helper", "terminal_cli", "ai_feature"] {
        current.approved_channel = channel.into();
        assert!(!needs_terminal_account_access(
            &current,
            "third",
            PersonalReadSend
        ));
    }
}

fn gui_record() -> RegistrationRecord {
    let mut value = record();
    value.client_kind = ClientKind::Gui;
    value.display_label = DEFAULT_GUI_LABEL.into();
    value.scope = gui_request().scope;
    value
}
fn gui_review() -> RegistrationReview {
    review_snapshot(&gui_request(), hantong_platform::sha256(&public()).unwrap()).unwrap()
}

#[test]
fn inst04_gui_and_cli_pending_requests_are_separate_and_share_the_native_queue() {
    struct Key(Option<ServerSigningKey>);
    impl Drop for Key {
        fn drop(&mut self) {
            self.0.take().unwrap().delete().unwrap();
        }
    }
    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let key = Key(Some(
        ServerSigningKey::create(hantong_platform::random_nonce().unwrap()).unwrap(),
    ));
    let root = Directory(std::env::temp_dir().join(format!(
        "hantong-gui-registration-{}",
        hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
    )));
    std::fs::create_dir(&root.0).unwrap();
    let store = Store::open(root.0.join("data"), root.0.join("keys")).unwrap();
    let secrets = root.0.join("keys");
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    let first = handle.command(RegistrationCommand::GuiRequest).unwrap();
    let id = *first.request_id();
    assert_eq!(worker.receiver.try_recv().unwrap(), Job::Registration(id));
    assert_eq!(
        handle.command(RegistrationCommand::GuiRequest),
        Ok(first.clone())
    );
    assert_eq!(
        handle.command(RegistrationCommand::GuiStatus(id)),
        Ok(first.clone())
    );
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(
        handle.command(RegistrationCommand::Status(id)),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(
        handle.command(RegistrationCommand::Request),
        Err(ErrorCode::RateLimited)
    );
    assert!(worker.receiver.try_recv().is_err());
    let stored = store.get_registration_request(id).unwrap().unwrap();
    assert_eq!(stored.client_kind, ClientKind::Gui);
    assert_eq!(stored.display_label, DEFAULT_GUI_LABEL);
    assert!(stored.scope.accounts().is_empty());
    assert_eq!(
        stored.scope.tools(),
        hantong_contracts::auth::gui_read_tools()
    );
    drop(worker);
    drop(handle);
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    assert_eq!(
        handle.command(RegistrationCommand::GuiStatus(id)),
        Ok(first.clone())
    );
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(handle.command(RegistrationCommand::GuiRequest), Ok(first));
    assert_eq!(worker.receiver.try_recv().unwrap(), Job::Registration(id));
    drop(worker);
    drop(handle);
    store
        .deny_gui_registration_request(id, stored.version)
        .unwrap();
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    let cli = handle.command(RegistrationCommand::Request).unwrap();
    assert_ne!(cli.request_id(), &id);
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(
        handle.command(RegistrationCommand::GuiStatus(*cli.request_id())),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(
        handle
            .command(RegistrationCommand::GuiStatus(id))
            .unwrap()
            .state(),
        RegistrationProgress::Denied
    );
}

#[test]
fn sec03_gui_registration_binds_own_review_profile_and_recovery_without_new_key() {
    let f = Fake::gui();
    assert_eq!(
        commit_consumed_review(&f, &gui_request(), &gui_review()),
        Ok(())
    );
    assert_eq!(
        f.events(),
        [
            "request",
            "server",
            "open",
            "create",
            "key_public",
            "stage",
            "publish",
            "activate",
            "release_publication"
        ]
    );
    f.events.borrow_mut().clear();
    f.current.borrow_mut().state = RegistrationRequestState::Approved;
    assert_eq!(recover_record(&f, &gui_record()), Ok(()));
    assert_eq!(
        f.events(),
        [
            "request",
            "server",
            "open",
            "key_public",
            "publish",
            "activate",
            "release_publication"
        ]
    );
}

#[test]
fn sec03_gui_and_cli_reviews_cannot_authorize_each_other_or_expand_tools() {
    for (f, expected, wrong) in [
        (Fake::gui(), gui_request(), review()),
        (Fake::new(), request(), gui_review()),
    ] {
        assert_eq!(
            commit_consumed_review(&f, &expected, &wrong),
            Err(ErrorCode::Conflict)
        );
        assert!(!f.events().contains(&"create"));
    }
    let mut invalid = gui_request();
    invalid.scope = RequestedScope::new(
        vec!["a".into()],
        hantong_contracts::auth::default_cli_metadata_tools(),
    )
    .unwrap();
    assert_eq!(review_snapshot(&invalid, [2; 32]), Err(ErrorCode::Conflict));
    invalid = gui_request();
    invalid.display_label = DEFAULT_CLI_LABEL.into();
    assert_eq!(review_snapshot(&invalid, [2; 32]), Err(ErrorCode::Conflict));
}

#[test]
fn sec03_gui_failure_preserves_provisioning_key_and_never_replaces_missing_key() {
    for failure in ["publish", "activate"] {
        let mut f = Fake::gui();
        f.fail = Some(failure);
        assert_eq!(
            commit_consumed_review(&f, &gui_request(), &gui_review()),
            Err(ErrorCode::Conflict)
        );
        assert!(!f.events().contains(&"delete"));
    }
    let f = Fake::gui();
    f.current.borrow_mut().state = RegistrationRequestState::Approved;
    assert_eq!(recover_record(&f, &gui_record()), Err(ErrorCode::NotFound));
    assert!(
        !f.events()
            .iter()
            .any(|event| ["create", "publish", "activate"].contains(event))
    );
}
struct Fake {
    gui: bool,
    ai: Option<AiProvider>,
    events: Rc<RefCell<Vec<&'static str>>>,
    fail: Option<&'static str>,
    current: RefCell<RegistrationRequest>,
    now: i64,
    wrong_key: bool,
    cleanup_fails: Cell<bool>,
    linked: Cell<bool>,
    missing: Cell<bool>,
    diagnostics: RefCell<Vec<ErrorCode>>,
    uncertain_commit: Cell<bool>,
}
impl Fake {
    fn new() -> Self {
        Self {
            gui: false,
            ai: None,
            events: Rc::new(RefCell::new(vec![])),
            fail: None,
            current: RefCell::new(request()),
            now: 2000,
            wrong_key: false,
            cleanup_fails: Cell::new(false),
            linked: Cell::new(false),
            missing: Cell::new(true),
            diagnostics: RefCell::new(vec![]),
            uncertain_commit: Cell::new(false),
        }
    }
    fn hit(&self, event: &'static str) -> Result<(), ErrorCode> {
        self.events.borrow_mut().push(event);
        if self.fail == Some(event) {
            Err(ErrorCode::Conflict)
        } else {
            Ok(())
        }
    }
    fn gui() -> Self {
        let mut value = Self::new();
        value.gui = true;
        *value.current.borrow_mut() = gui_request();
        value
    }
    fn ai(provider: AiProvider) -> Self {
        let mut value = Self::new();
        value.ai = Some(provider);
        *value.current.borrow_mut() = ai_request(provider);
        value
    }
    fn expected_request(&self) -> RegistrationRequest {
        if self.gui {
            gui_request()
        } else if let Some(provider) = self.ai {
            ai_request(provider)
        } else {
            request()
        }
    }
    fn expected_record(&self) -> RegistrationRecord {
        if self.gui {
            gui_record()
        } else if let Some(provider) = self.ai {
            ai_record(provider)
        } else {
            record()
        }
    }
    fn events(&self) -> Vec<&'static str> {
        self.events.borrow().clone()
    }
}
struct Publication(Rc<RefCell<Vec<&'static str>>>);
impl Drop for Publication {
    fn drop(&mut self) {
        self.0.borrow_mut().push("release_publication");
    }
}
impl Effects for Fake {
    type Key = ();
    type Publication = Publication;
    fn linked(&self, _: [u8; 32], _: [u8; 72]) -> Result<bool, ErrorCode> {
        self.hit("linked")?;
        Ok(self.linked.get())
    }
    fn orphans(&self) -> Result<Vec<RegistrationRequest>, ErrorCode> {
        self.hit("orphans")?;
        Ok(vec![self.current.borrow().clone()])
    }
    fn provisioning(&self) -> Result<Vec<RegistrationRecord>, ErrorCode> {
        self.hit("provisioning")?;
        Ok(vec![])
    }
    fn report(&self, error: ErrorCode) {
        self.diagnostics.borrow_mut().push(error);
    }
    fn request(&self, _: [u8; 32]) -> Result<RegistrationRequest, ErrorCode> {
        self.hit("request")?;
        Ok(self.current.borrow().clone())
    }
    fn server_public(&self) -> Result<[u8; 72], ErrorCode> {
        self.hit("server")?;
        Ok(public())
    }
    fn now(&self) -> Result<i64, ErrorCode> {
        Ok(self.now)
    }
    fn create_key(&self, id: [u8; 16]) -> Result<(), ErrorCode> {
        assert_eq!(
            id,
            hantong_platform::registration_id_for_request(self.expected_request().request_id)
                .unwrap()
        );
        self.hit("create")?;
        self.missing.set(false);
        Ok(())
    }
    fn open_key(&self, _: [u8; 16]) -> Result<(), ErrorCode> {
        self.hit("open")?;
        if self.missing.get() {
            Err(ErrorCode::NotFound)
        } else {
            Ok(())
        }
    }
    fn key_public(&self, _: &()) -> Result<[u8; 72], ErrorCode> {
        self.hit("key_public")?;
        let mut key = public();
        if self.wrong_key {
            key[8] ^= 1;
        }
        Ok(key)
    }
    fn delete_key(&self, _: ()) -> Result<(), ErrorCode> {
        self.hit("delete")?;
        if self.cleanup_fails.get() {
            Err(ErrorCode::InternalError)
        } else {
            self.missing.set(true);
            Ok(())
        }
    }
    fn stage(
        &self,
        r: RegistrationRequest,
        id: [u8; 16],
        key: [u8; 72],
        hash: [u8; 32],
    ) -> Result<RegistrationRecord, ErrorCode> {
        assert_eq!(r, self.expected_request());
        assert_eq!(
            id,
            hantong_platform::registration_id_for_request(self.expected_request().request_id)
                .unwrap()
        );
        assert_eq!(key, public());
        assert_eq!(hash, *review().server_public_key_hash());
        if self.uncertain_commit.get() {
            self.linked.set(true);
        }
        self.hit("stage")?;
        Ok(self.expected_record())
    }
    fn publish(&self, bytes: &[u8]) -> Result<Publication, ErrorCode> {
        let p = ParsedClientProfile::from_json(bytes).unwrap();
        assert_eq!(
            p.registration_id(),
            &hantong_platform::registration_id_for_request(self.expected_request().request_id)
                .unwrap()
        );
        assert_eq!(p.scope(), &self.expected_request().scope);
        assert_eq!(
            p.profile().as_str(),
            if self.gui {
                "gui"
            } else if self.ai == Some(AiProvider::Codex) {
                "ai-codex"
            } else if self.ai == Some(AiProvider::Claude) {
                "ai-claude"
            } else {
                "default"
            }
        );
        self.hit("publish")?;
        Ok(Publication(self.events.clone()))
    }
    fn activate(&self, r: &RegistrationRecord, hash: [u8; 32]) -> Result<(), ErrorCode> {
        assert_eq!(r, &self.expected_record());
        assert_eq!(hash, *review().server_public_key_hash());
        self.hit("activate")
    }
}
#[test]
fn cli15_consumed_review_rechecks_then_stages_publishes_and_activates_with_guard() {
    let f = Fake::new();
    assert_eq!(commit_consumed_review(&f, &request(), &review()), Ok(()));
    assert_eq!(
        f.events(),
        vec![
            "request",
            "server",
            "open",
            "create",
            "key_public",
            "stage",
            "publish",
            "activate",
            "release_publication"
        ]
    );
}
#[test]
fn cli15_no_key_is_created_for_stale_expired_or_wrong_review() {
    for mode in 0..4 {
        let mut f = Fake::new();
        let mut review = review();
        match mode {
            0 => f.current.borrow_mut().version += 1,
            1 => f.now = 301000,
            2 => f.now = 999,
            _ => {
                review =
                    RegistrationReview::new([1; 32], 1, vec!["b".into()], [9; 32], 1000, 301000)
                        .unwrap()
            }
        };
        assert!(commit_consumed_review(&f, &request(), &review).is_err());
        assert!(!f.events().contains(&"create"));
    }
}
#[test]
fn cli15_only_new_key_before_successful_stage_is_deleted() {
    for failure in ["create", "key_public", "stage", "publish", "activate"] {
        let mut f = Fake::new();
        f.fail = Some(failure);
        assert_eq!(
            commit_consumed_review(&f, &request(), &review()),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(f.events().contains(&"delete"), matches!(failure, "stage"));
        if matches!(failure, "publish" | "activate") {
            assert!(f.events().contains(&"stage"));
        }
    }
}
#[test]
fn cli15_recovery_opens_existing_key_and_reuses_profile_without_create_or_delete() {
    let f = Fake::new();
    f.current.borrow_mut().state = RegistrationRequestState::Approved;
    f.missing.set(false);
    f.current.borrow_mut().version = 2;
    assert_eq!(recover_record(&f, &record()), Ok(()));
    assert_eq!(
        f.events(),
        vec![
            "request",
            "server",
            "open",
            "key_public",
            "publish",
            "activate",
            "release_publication"
        ]
    );
}
#[test]
fn cli15_recovery_failures_preserve_provisioning_key_and_do_not_activate() {
    for failure in ["open", "publish"] {
        let mut f = Fake::new();
        f.current.borrow_mut().state = RegistrationRequestState::Approved;
        f.missing.set(false);
        f.current.borrow_mut().version = 2;
        f.fail = Some(failure);
        assert_eq!(recover_record(&f, &record()), Err(ErrorCode::Conflict));
        assert!(!f.events().contains(&"create"));
        assert!(!f.events().contains(&"delete"));
        assert!(!f.events().contains(&"activate"));
    }
    let mut f = Fake::new();
    f.current.borrow_mut().state = RegistrationRequestState::Approved;
    f.missing.set(false);
    f.wrong_key = true;
    assert!(recover_record(&f, &record()).is_err());
    assert!(!f.events().contains(&"publish"));
}
#[test]
fn cli15_recovery_rejects_nondefault_or_changed_grants() {
    for mode in 0..3 {
        let f = Fake::new();
        f.current.borrow_mut().state = RegistrationRequestState::Approved;
        f.missing.set(false);
        let mut r = record();
        match mode {
            0 => r.display_label = "other".into(),
            1 => r.state = RegistrationState::Active,
            _ => r.scope = RequestedScope::new(vec![], vec!["mail.accounts.list".into()]).unwrap(),
        };
        assert!(recover_record(&f, &r).is_err());
        assert!(!f.events().contains(&"open"));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn cli15_pending_window_deduplicates_and_cancel_releases_for_explicit_retry() {
    let scheduled = Cell::new(None);
    let (sender, mut receiver) = mpsc::channel(1);
    enqueue(&scheduled, &sender, [1; 32]).unwrap();
    enqueue(&scheduled, &sender, [1; 32]).unwrap();
    assert_eq!(
        enqueue(&scheduled, &sender, [2; 32]),
        Err(ErrorCode::RateLimited)
    );
    assert_eq!(receiver.recv().await, Some([1; 32]));
    assert!(receiver.try_recv().is_err());
    let window = ScheduledWindow(&scheduled);
    // A slow/unfinished review does not hold a database borrow or block IPC.
    let responsive = tokio::time::timeout(std::time::Duration::from_millis(100), async {
        tokio::task::yield_now().await;
        enqueue(&scheduled, &sender, [1; 32])
    })
    .await
    .unwrap();
    assert_eq!(responsive, Ok(()));
    assert!(receiver.try_recv().is_err());
    drop(window); // cancellation or dropping the review clears only local UI state
    assert_eq!(scheduled.get(), None);
    assert!(receiver.try_recv().is_err()); // no automatic retry after cancellation
    enqueue(&scheduled, &sender, [1; 32]).unwrap(); // an explicit request can reopen
    assert_eq!(receiver.recv().await, Some([1; 32]));
}

#[test]
fn cli15_closed_worker_never_marks_a_review_as_scheduled() {
    let scheduled = Cell::new(None);
    let (sender, receiver) = mpsc::channel(1);
    drop(receiver);
    assert_eq!(
        enqueue(&scheduled, &sender, [1; 32]),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(scheduled.get(), None);
}

#[test]
fn cli15_crash_before_stage_cleanup_reopens_only_persisted_selector() {
    let f = Fake::new();
    f.missing.set(false);
    assert_eq!(cleanup_orphan(&f, &request()), Ok(()));
    assert_eq!(f.events(), vec!["open", "key_public", "linked", "delete"]);
    assert_eq!(cleanup_orphan(&f, &request()), Ok(()));
    assert!(!f.events().contains(&"create"));
}
#[test]
fn cli15_failed_cleanup_retries_and_never_deletes_linked_or_uncertain_key() {
    let f = Fake::new();
    f.missing.set(false);
    f.cleanup_fails.set(true);
    assert_eq!(
        cleanup_orphan(&f, &request()),
        Err(ErrorCode::InternalError)
    );
    f.cleanup_fails.set(false);
    assert_eq!(cleanup_orphan(&f, &request()), Ok(()));
    let f = Fake::new();
    f.missing.set(false);
    f.linked.set(true);
    assert_eq!(cleanup_orphan(&f, &request()), Err(ErrorCode::Conflict));
    assert!(!f.events().contains(&"delete"));
    let mut f = Fake::new();
    f.missing.set(false);
    f.fail = Some("linked");
    assert_eq!(cleanup_orphan(&f, &request()), Err(ErrorCode::Conflict));
    assert!(!f.events().contains(&"delete"));
}
#[test]
fn cli15_startup_list_failure_is_fatal_but_item_failure_is_diagnosed() {
    let mut f = Fake::new();
    f.fail = Some("orphans");
    assert_eq!(recover_startup(&f), Err(ErrorCode::Conflict));
    assert!(!f.events().contains(&"open"));
    let f = Fake::new();
    f.missing.set(false);
    f.cleanup_fails.set(true);
    assert_eq!(recover_startup(&f), Ok(()));
    assert_eq!(*f.diagnostics.borrow(), vec![ErrorCode::InternalError]);
}
#[test]
fn cli15_stage_error_keeps_original_code_when_cleanup_also_fails() {
    let mut f = Fake::new();
    f.fail = Some("stage");
    f.cleanup_fails.set(true);
    f.missing.set(true);
    assert_eq!(
        commit_consumed_review(&f, &request(), &review()),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        *f.diagnostics.borrow(),
        vec![ErrorCode::Conflict, ErrorCode::InternalError]
    );
}

#[test]
fn cli15_uncertain_stage_commit_preserves_linked_key() {
    let mut f = Fake::new();
    f.fail = Some("stage");
    f.uncertain_commit.set(true);
    assert_eq!(
        commit_consumed_review(&f, &request(), &review()),
        Err(ErrorCode::Conflict)
    );
    assert!(f.events().contains(&"linked"));
    assert!(!f.events().contains(&"delete"));
    assert!(!f.events().contains(&"publish"));
}
#[test]
fn cli15_cleanup_failure_blocks_new_creation_until_explicit_retry_succeeds() {
    let f = Fake::new();
    f.missing.set(false);
    f.cleanup_fails.set(true);
    assert_eq!(
        commit_consumed_review(&f, &request(), &review()),
        Err(ErrorCode::InternalError)
    );
    assert!(!f.events().contains(&"create"));
    f.cleanup_fails.set(false);
    assert_eq!(commit_consumed_review(&f, &request(), &review()), Ok(()));
    let events = f.events();
    let created = events.iter().position(|s| *s == "create").unwrap();
    assert_eq!(
        events[..created].iter().filter(|s| **s == "delete").count(),
        2
    );
}

#[test]
fn setup01_terminal_binding_is_bound_to_typed_endpoint_peer_and_kind() {
    let key = ServerSigningKey::create(hantong_platform::random_nonce().unwrap()).unwrap();
    let peer =
        hantong_contracts::auth::PeerContext::new("S-1-5-21-1-2-3-4", "S-1-5-5-1-2").unwrap();
    let endpoint = serde_json::json!({
        "settings_version": 1,
        "host": "imap.example",
        "port": 993,
        "tls": "implicit_tls",
        "login_name": "user",
        "authentication": "app_password"
    });
    let expiry = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
        + 60_000;
    let first = terminal_binding(&TerminalBindingInput {
        kind: *b"terminal-imap-v1",
        request_id: [1; 32],
        payload_hash: [2; 32],
        account_id: "acct",
        endpoint: &endpoint,
        version: 1,
        expires_at_utc_ms: expiry,
        peer: &peer,
        server: &key,
    })
    .unwrap();
    let mut changed = endpoint.clone();
    changed["port"] = 143.into();
    let second = terminal_binding(&TerminalBindingInput {
        kind: *b"terminal-imap-v1",
        request_id: [1; 32],
        payload_hash: [2; 32],
        account_id: "acct",
        endpoint: &changed,
        version: 1,
        expires_at_utc_ms: expiry,
        peer: &peer,
        server: &key,
    })
    .unwrap();
    let other_peer =
        hantong_contracts::auth::PeerContext::new("S-1-5-21-9-8-7-6", "S-1-5-5-9-8").unwrap();
    let third = terminal_binding(&TerminalBindingInput {
        kind: *b"terminal-smtp-v1",
        request_id: [1; 32],
        payload_hash: [2; 32],
        account_id: "acct",
        endpoint: &endpoint,
        version: 1,
        expires_at_utc_ms: expiry,
        peer: &other_peer,
        server: &key,
    })
    .unwrap();
    assert_ne!(first, second);
    assert_ne!(first, third);
    key.delete().unwrap();
}
#[test]
fn cli15_orphan_cleanup_preserves_explicit_denial_and_diagnostics_have_only_stable_fields() {
    let f = Fake::new();
    f.missing.set(false);
    let mut denied = request();
    denied.state = RegistrationRequestState::Denied;
    assert_eq!(cleanup_orphan(&f, &denied), Ok(()));
    assert_eq!(denied.state, RegistrationRequestState::Denied);
    let line = registration_failure_line(ErrorCode::InternalError, 1234).unwrap();
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    let object = value.as_object().unwrap();
    assert_eq!(object.len(), 5);
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["event"], "operation_failed");
    assert_eq!(value["error_code"], "INTERNAL_ERROR");
    assert!(!line.contains("request"));
    assert!(!line.contains("key"));
    assert!(!line.contains("path"));
}

#[test]
fn setup01_registration_and_imap_share_one_gate_and_status_never_schedules() {
    use hantong_contracts::imap_setup::{ImapSetupInput, ImapSetupState, ImapSetupTls};
    struct Key(Option<ServerSigningKey>);
    impl Drop for Key {
        fn drop(&mut self) {
            self.0.take().unwrap().delete().unwrap();
        }
    }
    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let key = Key(Some(
        ServerSigningKey::create(hantong_platform::random_nonce().unwrap()).unwrap(),
    ));
    let root = Directory(std::env::temp_dir().join(format!(
        "hantong-core-queue-{}",
        hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
    )));
    std::fs::create_dir(&root.0).unwrap();
    let store = Store::open(root.0.join("data"), root.0.join("keys")).unwrap();
    let secrets = root.0.join("keys");
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    let input = ImapSetupInput::new(
        None,
        None,
        "IMAP.EXAMPLE".into(),
        993,
        ImapSetupTls::ImplicitTls,
        "user".into(),
    )
    .unwrap();
    let cmd = || SetupCommand::Imap(ImapSetupCommand::Request(input.clone()));
    let SetupReceipt::Imap(receipt) = handle.setup_command(cmd()).unwrap() else {
        panic!("wrong family");
    };
    assert_eq!(receipt.state(), ImapSetupState::Pending);
    let id = *receipt.request_id();
    assert!(worker.receiver.try_recv().is_err());
    assert!(
        matches!(handle.setup_command(cmd()).unwrap(),SetupReceipt::Imap(value) if value==receipt)
    );
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(store.list_imap_setup_requests().unwrap().len(), 1);
    assert!(handle.command(RegistrationCommand::Request).is_ok());
    assert!(
        matches!(handle.setup_command(SetupCommand::Imap(ImapSetupCommand::Status(id))).unwrap(),SetupReceipt::Imap(value) if value==receipt)
    );
    assert!(worker.receiver.try_recv().is_err());
    drop(worker);
    let SetupReceipt::Imap(interrupted) = handle
        .setup_command(SetupCommand::Imap(ImapSetupCommand::Status(id)))
        .unwrap()
    else {
        panic!("wrong family");
    };
    assert_eq!(interrupted.state(), ImapSetupState::Pending);
    assert!(
        matches!(handle.setup_command(cmd()), Ok(SetupReceipt::Imap(value)) if value == interrupted)
    );
    assert_eq!(store.list_imap_setup_requests().unwrap().len(), 1);
    assert!(store.list_registrable_account_ids().unwrap().is_empty());
    drop(handle);
    let resume = || SetupCommand::Imap(ImapSetupCommand::Resume(id));
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    let SetupReceipt::Imap(child) = handle.setup_command(resume()).unwrap() else {
        panic!("wrong family");
    };
    assert_ne!(child.request_id(), &id);
    assert!(worker.receiver.try_recv().is_err());
    assert!(
        matches!(handle.setup_command(resume()).unwrap(),SetupReceipt::Imap(value) if value==child)
    );
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(store.list_imap_setup_requests().unwrap().len(), 2);
    drop(worker);
    drop(handle);
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    let SetupReceipt::Imap(grandchild) = handle.setup_command(resume()).unwrap() else {
        panic!("wrong family");
    };
    assert_eq!(grandchild.request_id(), child.request_id());
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(store.list_imap_setup_requests().unwrap().len(), 2);
    assert!(store.list_registrable_account_ids().unwrap().is_empty());
    drop(worker);
    drop(handle);
    let pending = store
        .load_imap_setup_request(*child.request_id())
        .unwrap()
        .unwrap();
    let staged = store.stage_imap_setup_request(&pending).unwrap();
    let started = store.begin_imap_credential_store(&staged).unwrap();
    let returned = store
        .finish_imap_credential_store(&started, Ok(()))
        .unwrap();
    let probing = store.begin_initial_imap_probe(&returned).unwrap();
    store
        .finish_imap_setup_probe(
            &probing,
            &hantong_store::account_provisioning::InitialImapProbeOutcome::Ready(vec![]),
        )
        .unwrap();
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    let SetupReceipt::Imap(completed) = handle.setup_command(resume()).unwrap() else {
        panic!("wrong family");
    };
    assert_eq!(completed.request_id(), child.request_id());
    assert_eq!(completed.state(), ImapSetupState::Ready);
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(store.list_imap_setup_requests().unwrap().len(), 2);
    drop(worker);
    drop(handle);
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    enqueue(
        &handle.state.scheduled,
        &handle.sender,
        Job::Registration([9; 32]),
    )
    .unwrap();
    assert!(matches!(
        handle.setup_command(cmd()),
        Err(ErrorCode::RateLimited)
    ));
    assert_eq!(
        worker.receiver.try_recv().unwrap(),
        Job::Registration([9; 32])
    );
    assert_eq!(store.list_imap_setup_requests().unwrap().len(), 2);
    assert!(matches!(
        handle.setup_command(resume()),
        Err(ErrorCode::RateLimited)
    ));
}

#[cfg(windows)]
#[test]
fn setup01_expired_matching_imap_request_is_replaced_without_job() {
    use hantong_contracts::imap_setup::{ImapSetupInput, ImapSetupState, ImapSetupTls};
    struct Key(Option<ServerSigningKey>);
    impl Drop for Key {
        fn drop(&mut self) {
            self.0.take().unwrap().delete().unwrap();
        }
    }
    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    let key = Key(Some(
        ServerSigningKey::create(hantong_platform::random_nonce().unwrap()).unwrap(),
    ));
    let root = Directory(std::env::temp_dir().join(format!(
        "hantong-core-imap-expired-{}",
        hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
    )));
    std::fs::create_dir(&root.0).unwrap();
    let store = Store::open(root.0.join("data"), root.0.join("keys")).unwrap();
    let secrets = root.0.join("keys");
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    let input = ImapSetupInput::new(
        None,
        None,
        "IMAP.EXAMPLE".into(),
        993,
        ImapSetupTls::ImplicitTls,
        "user".into(),
    )
    .unwrap();
    let request = || SetupCommand::Imap(ImapSetupCommand::Request(input.clone()));

    let SetupReceipt::Imap(first) = handle.setup_command(request()).unwrap() else {
        panic!("wrong family");
    };
    let first_id = *first.request_id();
    assert_eq!(first.state(), ImapSetupState::Pending);
    assert!(worker.receiver.try_recv().is_err());

    drop(worker);
    drop(handle);
    drop(store);
    let keyed =
        hantong_secrets::KeyedConnection::open(root.0.join("data"), root.0.join("keys")).unwrap();
    keyed
        .with_connection(|c| {
            c.execute(
                "UPDATE imap_setup_requests SET created_at_utc_ms=created_at_utc_ms-3600000, expires_at_utc_ms=expires_at_utc_ms-3600000, updated_at_utc_ms=updated_at_utc_ms-3600000",
                [],
            )?;
            c.execute(
                "UPDATE imap_setup_events SET occurred_at_utc_ms=occurred_at_utc_ms-3600000",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    drop(keyed);
    let store = Store::open(root.0.join("data"), root.0.join("keys")).unwrap();
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);

    let SetupReceipt::Imap(replacement) = handle.setup_command(request()).unwrap() else {
        panic!("wrong family");
    };
    assert_eq!(replacement.state(), ImapSetupState::Pending);
    assert_ne!(replacement.request_id(), &first_id);
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(store.list_imap_setup_requests().unwrap().len(), 2);
    let SetupReceipt::Imap(expired) = handle
        .setup_command(SetupCommand::Imap(ImapSetupCommand::Status(first_id)))
        .unwrap()
    else {
        panic!("wrong family");
    };
    assert_eq!(expired.state(), ImapSetupState::Expired);
}

#[test]
fn cli15_scope_refresh_deduplicates_without_native_job_and_resumes_only_explicit_requests() {
    use hantong_contracts::scope_refresh::{ScopeRefreshCommand as C, ScopeRefreshProgress as P};
    struct Key(Option<ServerSigningKey>);
    impl Drop for Key {
        fn drop(&mut self) {
            self.0.take().unwrap().delete().unwrap();
        }
    }
    struct Dir(std::path::PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let key = Key(Some(
        ServerSigningKey::create(hantong_platform::random_nonce().unwrap()).unwrap(),
    ));
    let server = key.0.as_ref().unwrap();
    let root = Dir(std::env::temp_dir().join(format!(
        "hantong-scope-queue-{}",
        hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
    )));
    std::fs::create_dir(&root.0).unwrap();
    let store = Store::open(root.0.join("data"), root.0.join("keys")).unwrap();
    store.stage_server_identity(server.id()).unwrap();
    store
        .activate_server_identity(server.id(), server.public_key().unwrap(), 1)
        .unwrap();
    let pin = hantong_platform::sha256(&server.public_key().unwrap()).unwrap();
    let req = store
        .create_registration_request(
            [1; 32],
            ClientKind::Cli,
            DEFAULT_CLI_LABEL,
            &RequestedScope::new(vec![], vec!["mail.accounts.list".into()]).unwrap(),
        )
        .unwrap();
    let id = hantong_platform::registration_id_for_request([1; 32]).unwrap();
    store
        .stage_cli_registration(&CliRegistrationStage::new(req, id, public(), pin).unwrap())
        .unwrap();
    store.activate_cli_registration(id, 1, pin).unwrap();
    let input = hantong_contracts::imap_setup::ImapSetupInput::new(
        None,
        None,
        "imap.example".into(),
        993,
        hantong_contracts::imap_setup::ImapSetupTls::ImplicitTls,
        "synthetic".into(),
    )
    .unwrap();
    let setup = crate::setup_imap::create(&store, &input).unwrap();
    store.stage_imap_setup_request(&setup).unwrap();
    let secrets = root.0.join("keys");
    let (handle, mut worker) = coordinator(&store, server, &secrets);
    let command = || SetupCommand::ScopeRefresh(C::Request);
    let SetupReceipt::ScopeRefresh(first) = handle.setup_command(command()).unwrap() else {
        panic!("family")
    };
    assert_eq!(first.state(), P::Pending);
    let rid = *first.refresh_id();
    assert_eq!(
        store
            .load_cli_scope_refresh(rid)
            .unwrap()
            .unwrap()
            .target_tools(),
        hantong_contracts::auth::default_cli_send_tools()
    );
    assert!(worker.receiver.try_recv().is_err());
    assert!(
        matches!(handle.setup_command(command()).unwrap(),SetupReceipt::ScopeRefresh(r)if r==first)
    );
    assert!(worker.receiver.try_recv().is_err());
    assert!(matches!(
        handle.setup_command(SetupCommand::Imap(ImapSetupCommand::Request(input.clone()))),
        Ok(SetupReceipt::Imap(_))
    ));
    assert!(matches!(
        handle.setup_command(SetupCommand::Imap(ImapSetupCommand::Resume(
            *setup.intent().request_id()
        ))),
        Ok(SetupReceipt::Imap(_))
    ));
    assert!(
        matches!(handle.setup_command(SetupCommand::ScopeRefresh(C::Status(rid))).unwrap(),SetupReceipt::ScopeRefresh(r)if r.state()==P::Pending)
    );
    assert!(worker.receiver.try_recv().is_err());
    drop(worker);
    assert!(
        matches!(handle.setup_command(SetupCommand::ScopeRefresh(C::Status(rid))).unwrap(),SetupReceipt::ScopeRefresh(r)if r.state()==P::Interrupted)
    );
    assert!(matches!(
        handle.setup_command(command()),
        Err(ErrorCode::InternalError)
    ));
    let (handle, mut worker) = coordinator(&store, server, &secrets);
    assert!(
        matches!(handle.setup_command(command()).unwrap(),SetupReceipt::ScopeRefresh(r)if r.refresh_id()==&rid)
    );
    assert!(worker.receiver.try_recv().is_err());
    drop(worker);
    let pending = store.load_cli_scope_refresh(rid).unwrap().unwrap();
    store.stage_cli_scope_refresh(&pending).unwrap();
    assert_eq!(store.list_provisioning_registrations(128).unwrap().len(), 1);
    assert!(
        NativeEffects {
            store: &store,
            server
        }
        .provisioning()
        .unwrap()
        .is_empty()
    );
    let (handle, mut worker) = coordinator(&store, server, &secrets);
    assert!(
        matches!(handle.setup_command(command()).unwrap(),SetupReceipt::ScopeRefresh(r)if r.refresh_id()==&rid&&r.state()==P::Provisioning)
    );
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(
        store
            .load_default_cli_registration()
            .unwrap()
            .unwrap()
            .state,
        RegistrationState::Provisioning
    );
}

#[test]
fn inst04_mcp_and_cli_pending_requests_are_separate_and_do_not_schedule_native_work() {
    struct Key(Option<ServerSigningKey>);
    impl Drop for Key {
        fn drop(&mut self) {
            self.0.take().unwrap().delete().unwrap();
        }
    }
    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let key = Key(Some(
        ServerSigningKey::create(hantong_platform::random_nonce().unwrap()).unwrap(),
    ));
    let root = Directory(std::env::temp_dir().join(format!(
        "hantong-mcp-registration-{}",
        hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
    )));
    std::fs::create_dir(&root.0).unwrap();
    let store = Store::open(root.0.join("data"), root.0.join("keys")).unwrap();
    let secrets = root.0.join("keys");
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    let first = handle.command(RegistrationCommand::McpRequest).unwrap();
    let id = *first.request_id();
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(
        handle.command(RegistrationCommand::McpRequest),
        Ok(first.clone())
    );
    assert_eq!(
        handle.command(RegistrationCommand::McpStatus(id)),
        Ok(first.clone())
    );
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(
        handle.command(RegistrationCommand::Status(id)),
        Err(ErrorCode::NotFound)
    );
    let cli = handle.command(RegistrationCommand::Request).unwrap();
    assert_ne!(cli.request_id(), &id);
    assert!(worker.receiver.try_recv().is_err());
    let stored = store.get_registration_request(id).unwrap().unwrap();
    assert_eq!(stored.client_kind, ClientKind::Mcp);
    assert_eq!(stored.display_label, DEFAULT_MCP_LABEL);
    assert!(stored.scope.accounts().is_empty());
    assert_eq!(
        stored.scope.tools(),
        hantong_contracts::auth::mcp_mail_tools()
    );
    drop(worker);
    drop(handle);
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    assert_eq!(
        handle.command(RegistrationCommand::McpStatus(id)),
        Ok(first.clone())
    );
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(handle.command(RegistrationCommand::McpRequest), Ok(first));
    assert!(worker.receiver.try_recv().is_err());
    drop(worker);
    drop(handle);
    store
        .deny_mcp_registration_request(id, stored.version)
        .unwrap();
    let (handle, mut worker) = coordinator(&store, key.0.as_ref().unwrap(), &secrets);
    let cli_after_deny = handle.command(RegistrationCommand::Request).unwrap();
    assert_ne!(cli_after_deny.request_id(), &id);
    assert!(worker.receiver.try_recv().is_err());
    assert_eq!(
        handle.command(RegistrationCommand::McpStatus(*cli_after_deny.request_id())),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(
        handle
            .command(RegistrationCommand::McpStatus(id))
            .unwrap()
            .state(),
        RegistrationProgress::Denied
    );
}
