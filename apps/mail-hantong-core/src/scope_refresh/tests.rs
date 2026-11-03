use super::*;
use hantong_contracts::{
    auth::RequestedScope,
    imap_setup::{ImapSetupInput, ImapSetupTls},
};
use hantong_store::{
    CliRegistrationStage, ClientKind, DEFAULT_CLI_LABEL, RegistrationState, Store,
};
use std::{
    cell::{Cell, RefCell},
    fs,
    path::PathBuf,
};

struct ReadOnlyKey(Option<ServerSigningKey>);
impl Drop for ReadOnlyKey {
    fn drop(&mut self) {
        self.0.take().unwrap().delete().unwrap();
    }
}

#[test]
fn gmail_write_scope_selects_exact_accounts_and_never_grants_before_native_approval() {
    use hantong_contracts::{google_write::GoogleAccessMode, scope_refresh::GoogleSendAccounts};
    let f = GoogleFixture::new();
    f.register_one();
    let ids = f
        .accounts
        .iter()
        .map(|a| a.account_id())
        .collect::<Vec<_>>();
    let selected = GoogleSendAccounts::parse(&ids).unwrap();
    assert!(prepare_google_send_for_pin(&f.store, f.pin(), selected.clone()).is_err());
    assert!(f.store.load_pending_cli_scope_refresh().unwrap().is_none());
    for account in &ids {
        let binding = f.store.google_account_binding(account).unwrap();
        f.store
            .activate_google_account(
                &hantong_contracts::google::GoogleIdentity {
                    access_mode: GoogleAccessMode::ReadSend,
                    account_id: account.clone(),
                    subject: binding.subject,
                    email: "synthetic@example.test".into(),
                    granted_scopes: GoogleAccessMode::ReadSend
                        .required_scopes()
                        .iter()
                        .map(|s| (*s).into())
                        .collect(),
                },
                &binding.client_id,
                binding.credential_version,
                None,
            )
            .unwrap();
    }
    let baseline = f.store.load_default_cli_registration().unwrap().unwrap();
    let request = prepare_google_send_for_pin(&f.store, f.pin(), selected.clone()).unwrap();
    assert!(matches_google_send(&request, selected.clone()));
    assert_eq!(
        f.store.load_default_cli_registration().unwrap().unwrap(),
        baseline
    );
    assert_eq!(
        prepare_google_send_for_pin(&f.store, f.pin(), selected)
            .unwrap()
            .refresh_id(),
        request.refresh_id()
    );
    let other = GoogleSendAccounts::parse(&[ids[1].clone()]).unwrap();
    assert!(matches!(
        prepare_google_send_for_pin(&f.store, f.pin(), other),
        Err(ErrorCode::Conflict)
    ));
}
#[test]
fn acc02_manage_selection_requires_modify_and_keeps_grants_pending_until_confirmation() {
    use hantong_contracts::{google_write::GoogleAccessMode, scope_refresh::ManageAccounts};
    let f = GoogleFixture::new();
    f.register_one();
    let ids = f
        .accounts
        .iter()
        .map(|a| a.account_id())
        .collect::<Vec<_>>();
    let selected = ManageAccounts::parse(&ids).unwrap();
    let base = f.store.load_default_cli_registration().unwrap().unwrap();
    assert!(matches!(
        prepare_manage_for_pin(&f.store, f.pin(), selected.clone()),
        Err(ErrorCode::ScopeDenied)
    ));
    assert!(f.store.load_pending_cli_scope_refresh().unwrap().is_none());
    for account in &ids {
        let binding = f.store.google_account_binding(account).unwrap();
        f.store
            .activate_google_account(
                &hantong_contracts::google::GoogleIdentity {
                    account_id: account.clone(),
                    subject: binding.subject,
                    email: "synthetic@example.test".into(),
                    access_mode: GoogleAccessMode::Modify,
                    granted_scopes: GoogleAccessMode::Modify
                        .required_scopes()
                        .iter()
                        .map(|s| (*s).into())
                        .collect(),
                },
                &binding.client_id,
                binding.credential_version,
                None,
            )
            .unwrap();
    }
    let pending = prepare_manage_for_pin(&f.store, f.pin(), selected.clone()).unwrap();
    assert!(matches_manage(&pending, &selected));
    assert_eq!(pending.decision_channel(), "terminal_cli");
    assert_eq!(
        f.store.load_default_cli_registration().unwrap().unwrap(),
        base
    );
    assert_eq!(
        prepare_manage_for_pin(&f.store, f.pin(), selected)
            .unwrap()
            .refresh_id(),
        pending.refresh_id()
    );
    assert!(matches!(
        prepare_manage_for_pin(
            &f.store,
            f.pin(),
            ManageAccounts::parse(&[ids[1].clone()]).unwrap()
        ),
        Err(ErrorCode::Conflict)
    ));
}

struct GoogleFixture {
    store: Store,
    key: ReadOnlyKey,
    accounts: Vec<hantong_contracts::registration::GoogleReadAccount>,
    _root: Cleanup,
}
impl GoogleFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "hantong-readonly-selection-{}",
            hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
        ));
        fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        let selector = hantong_platform::random_nonce().unwrap();
        let key = ReadOnlyKey(Some(ServerSigningKey::create(selector).unwrap()));
        store.stage_server_identity(selector).unwrap();
        store
            .activate_server_identity(selector, key.0.as_ref().unwrap().public_key().unwrap(), 1)
            .unwrap();
        let mut accounts = vec![];
        for subject in ["readonly-synthetic-one", "readonly-synthetic-two"] {
            let id = format!(
                "google_{}",
                hantong_contracts::wire_hex::encode(
                    &hantong_platform::sha256(subject.as_bytes()).unwrap()
                )
            );
            let identity = hantong_contracts::google::GoogleIdentity {
                access_mode: hantong_contracts::google_write::GoogleAccessMode::ReadOnly,
                account_id: id.clone(),
                subject: subject.into(),
                email: "synthetic@example.test".into(),
                granted_scopes: hantong_contracts::google::GOOGLE_SCOPES
                    .iter()
                    .map(|s| (*s).into())
                    .collect(),
            };
            store
                .activate_google_account(&identity, "fixture.apps.googleusercontent.com", 0, None)
                .unwrap();
            accounts.push(hantong_contracts::registration::GoogleReadAccount::parse(&id).unwrap());
        }
        Self {
            store,
            key,
            accounts,
            _root: Cleanup(root),
        }
    }
    fn pin(&self) -> [u8; 32] {
        hantong_platform::sha256(&self.key.0.as_ref().unwrap().public_key().unwrap()).unwrap()
    }
    fn register_one(&self) {
        let req = self
            .store
            .create_registration_request(
                [18; 32],
                ClientKind::Cli,
                DEFAULT_CLI_LABEL,
                &RequestedScope::new(
                    vec![self.accounts[0].account_id()],
                    hantong_contracts::auth::default_cli_legacy_tools(),
                )
                .unwrap(),
            )
            .unwrap();
        let id = hantong_platform::registration_id_for_request([18; 32]).unwrap();
        self.store
            .stage_cli_registration(
                &CliRegistrationStage::new(req, id, public(), self.pin()).unwrap(),
            )
            .unwrap();
        self.store
            .activate_cli_registration(id, 1, self.pin())
            .unwrap();
    }
}

#[test]
fn sec03_readonly_registration_selects_one_account_without_grant_or_other_pending_reuse() {
    use hantong_contracts::registration::RegistrationCommand;
    let f = GoogleFixture::new();
    let (handle, worker) =
        crate::registration::coordinator(&f.store, f.key.0.as_ref().unwrap(), &f._root.0);
    let missing = hantong_contracts::registration::GoogleReadAccount::parse(&format!(
        "google_{}",
        "a".repeat(64)
    ))
    .unwrap();
    assert_eq!(
        handle.command(RegistrationCommand::RequestGoogleReadOnly(missing)),
        Err(ErrorCode::NotFound)
    );
    assert!(
        f.store
            .load_pending_default_cli_request()
            .unwrap()
            .is_none()
    );
    let first = handle
        .command(RegistrationCommand::RequestGoogleReadOnly(f.accounts[0]))
        .unwrap();
    let pending = f
        .store
        .get_registration_request(*first.request_id())
        .unwrap()
        .unwrap();
    assert_eq!(pending.scope.accounts(), [f.accounts[0].account_id()]);
    assert_eq!(
        pending.scope.tools(),
        hantong_contracts::auth::default_cli_legacy_tools()
    );
    assert!(f.store.load_default_cli_registration().unwrap().is_none());
    assert_eq!(
        handle.command(RegistrationCommand::RequestGoogleReadOnly(f.accounts[1])),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        handle.command(RegistrationCommand::RequestGoogleReadOnly(f.accounts[0])),
        Ok(first.clone())
    );
    drop(worker);
    drop(handle);
    let (handle, _worker) =
        crate::registration::coordinator(&f.store, f.key.0.as_ref().unwrap(), &f._root.0);
    assert_eq!(
        handle.command(RegistrationCommand::RequestGoogleReadOnly(f.accounts[1])),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store
            .get_registration_request(*first.request_id())
            .unwrap()
            .unwrap(),
        pending
    );
}

#[test]
fn sec03_readonly_refresh_requests_exact_existing_b_and_preserves_grant_until_native_review() {
    let f = GoogleFixture::new();
    f.register_one();
    let before = f.store.load_default_cli_registration().unwrap().unwrap();
    let pending = prepare_selected_for_pin(&f.store, f.pin(), Some(f.accounts[0])).unwrap();
    assert_eq!(pending.target_accounts(), [f.accounts[0].account_id()]);
    assert_eq!(
        pending.target_tools(),
        hantong_contracts::auth::default_cli_body_tools()
    );
    assert_eq!(pending.target_tools().len(), 8);
    assert!(
        !pending
            .target_tools()
            .iter()
            .any(|t| t.starts_with("mail.send.") || t.starts_with("mail.drafts."))
    );
    assert_eq!(
        f.store.load_default_cli_registration().unwrap().unwrap(),
        before
    );
    assert_eq!(
        prepare_selected_for_pin(&f.store, f.pin(), Some(f.accounts[0])).unwrap(),
        pending
    );
    assert_eq!(
        prepare_selected_for_pin(&f.store, f.pin(), Some(f.accounts[1])),
        Err(ErrorCode::Conflict)
    );
    let (handle, _worker) =
        crate::registration::coordinator(&f.store, f.key.0.as_ref().unwrap(), &f._root.0);
    assert_eq!(
        handle.command(
            hantong_contracts::registration::RegistrationCommand::RequestGoogleReadOnly(
                f.accounts[0]
            )
        ),
        Err(ErrorCode::Conflict)
    );
}

#[test]
fn sec03_readonly_refresh_never_reuses_writable_pending_or_staged_target() {
    let f = GoogleFixture::new();
    f.register_one();
    let writable = f
        .store
        .create_cli_scope_refresh_with_scope(
            [21; 32],
            RequestedScope::new(
                vec![f.accounts[0].account_id()],
                hantong_contracts::auth::default_cli_send_tools(),
            )
            .unwrap(),
            f.pin(),
        )
        .unwrap();
    assert_eq!(
        prepare_add_for_pin(&f.store, f.pin(), f.accounts[0]),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        prepare_selected_for_pin(&f.store, f.pin(), Some(f.accounts[0])),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store
            .load_cli_scope_refresh(*writable.refresh_id())
            .unwrap()
            .unwrap(),
        writable
    );
    let staged = f.store.stage_cli_scope_refresh(&writable).unwrap();
    assert_eq!(
        prepare_add_for_pin(&f.store, f.pin(), f.accounts[0]),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        prepare_selected_for_pin(&f.store, f.pin(), Some(f.accounts[0])),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store
            .load_cli_scope_refresh(*staged.refresh_id())
            .unwrap()
            .unwrap(),
        staged
    );
}
#[test]
fn sec03_readonly_refresh_resumes_only_exact_staged_b_without_activating() {
    let f = GoogleFixture::new();
    f.register_one();
    let pending = prepare_selected_for_pin(&f.store, f.pin(), Some(f.accounts[0])).unwrap();
    let staged = f.store.stage_cli_scope_refresh(&pending).unwrap();
    let before = f.store.load_default_cli_registration().unwrap().unwrap();
    assert_eq!(
        prepare_add_for_pin(&f.store, f.pin(), f.accounts[0]).unwrap(),
        staged
    );
    assert_eq!(
        prepare_selected_for_pin(&f.store, f.pin(), Some(f.accounts[0])).unwrap(),
        staged
    );
    assert_eq!(
        f.store.load_default_cli_registration().unwrap().unwrap(),
        before
    );
    assert_eq!(
        before.scope.tools(),
        hantong_contracts::auth::default_cli_legacy_tools()
    );
    assert_eq!(before.state, RegistrationState::Provisioning);
}
#[test]
fn sec03_readonly_refresh_does_not_reuse_running_writable_request() {
    use hantong_contracts::scope_refresh::ScopeRefreshCommand;
    use hantong_ipc_server::setup_dispatch::SetupCommand;
    let f = GoogleFixture::new();
    f.register_one();
    let (handle, _worker) =
        crate::registration::coordinator(&f.store, f.key.0.as_ref().unwrap(), &f._root.0);
    handle
        .setup_command(SetupCommand::ScopeRefresh(ScopeRefreshCommand::Request))
        .unwrap();
    let result = handle.setup_command(SetupCommand::ScopeRefresh(
        ScopeRefreshCommand::RequestGoogleReadOnly(f.accounts[0]),
    ));
    assert!(matches!(result, Err(ErrorCode::Conflict)));
    assert!(matches!(
        handle.setup_command(SetupCommand::ScopeRefresh(
            ScopeRefreshCommand::AddGoogleReadOnly(f.accounts[0])
        )),
        Err(ErrorCode::Conflict)
    ));
    assert_eq!(
        f.store
            .load_default_cli_registration()
            .unwrap()
            .unwrap()
            .scope
            .tools(),
        hantong_contracts::auth::default_cli_legacy_tools()
    );
}
fn public() -> [u8; 72] {
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
    key
}
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
struct Fixture {
    store: Store,
    _root: Cleanup,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "hantong-scope-core-{}",
            hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
        ));
        fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        store.stage_server_identity([7; 32]).unwrap();
        store
            .activate_server_identity([7; 32], public(), 1)
            .unwrap();
        let req = store
            .create_registration_request(
                [1; 32],
                ClientKind::Cli,
                DEFAULT_CLI_LABEL,
                &RequestedScope::new(vec![], vec!["mail.accounts.list".into()]).unwrap(),
            )
            .unwrap();
        let id = hantong_platform::registration_id_for_request([1; 32]).unwrap();
        let hash = hantong_platform::sha256(&public()).unwrap();
        store
            .stage_cli_registration(&CliRegistrationStage::new(req, id, public(), hash).unwrap())
            .unwrap();
        store.activate_cli_registration(id, 1, hash).unwrap();
        let setup = crate::setup_imap::create(
            &store,
            &ImapSetupInput::new(
                None,
                None,
                "imap.example".into(),
                993,
                ImapSetupTls::ImplicitTls,
                "synthetic".into(),
            )
            .unwrap(),
        )
        .unwrap();
        store.stage_imap_setup_request(&setup).unwrap();
        Self {
            store,
            _root: Cleanup(root),
        }
    }
    fn pending(&self) -> CliScopeRefresh {
        self.store
            .create_cli_scope_refresh(
                [9; 32],
                self.store.list_registrable_account_ids().unwrap(),
                hantong_platform::sha256(&public()).unwrap(),
            )
            .unwrap()
    }
    fn pending_with_scope(&self, tools: Vec<String>) -> CliScopeRefresh {
        self.store
            .create_cli_scope_refresh_with_scope(
                [10; 32],
                RequestedScope::new(self.store.list_registrable_account_ids().unwrap(), tools)
                    .unwrap(),
                hantong_platform::sha256(&public()).unwrap(),
            )
            .unwrap()
    }
    fn terminal_pending_with_scope(&self, tools: Vec<String>) -> CliScopeRefresh {
        self.store
            .create_terminal_cli_scope_refresh(
                [10; 32],
                RequestedScope::new(self.store.list_registrable_account_ids().unwrap(), tools)
                    .unwrap(),
                hantong_platform::sha256(&public()).unwrap(),
            )
            .unwrap()
    }
}
struct Fake<'a> {
    store: &'a Store,
    server: [u8; 72],
    events: RefCell<Vec<&'static str>>,
    fail: Option<&'static str>,
    uncertain: bool,
    wrong_key: bool,
    held: std::rc::Rc<Cell<bool>>,
    diagnostics: RefCell<Vec<ErrorCode>>,
    published: RefCell<Option<(Vec<u8>, Vec<u8>)>>,
}
struct Publication(std::rc::Rc<Cell<bool>>);

#[test]
fn acc02_personal_google_scope_publishes_exact_features_and_recovers_interrupted_stage() {
    for tools in [
        hantong_contracts::auth::default_cli_body_tools(),
        hantong_contracts::auth::default_cli_send_tools(),
    ] {
        for interrupt in [false, true] {
            let root = std::env::temp_dir().join(format!(
                "hantong-personal-scope-{}",
                hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
            ));
            let cleanup = Cleanup(root.clone());
            let store = Store::open(root.join("data"), root.join("keys")).unwrap();
            store.stage_server_identity([7; 32]).unwrap();
            store
                .activate_server_identity([7; 32], public(), 1)
                .unwrap();
            let identity = hantong_contracts::google::GoogleIdentity {
                access_mode: hantong_contracts::google_write::GoogleAccessMode::ReadSend,
                account_id: format!(
                    "google_{}",
                    hantong_contracts::wire_hex::encode(
                        &hantong_platform::sha256(b"synthetic-personal").unwrap()
                    )
                ),
                subject: "synthetic-personal".into(),
                email: "synthetic@example.test".into(),
                granted_scopes: hantong_contracts::google_write::GoogleAccessMode::ReadSend
                    .required_scopes()
                    .iter()
                    .map(|s| (*s).into())
                    .collect(),
            };
            store
                .activate_google_account(&identity, "fixture.apps.googleusercontent.com", 0, None)
                .unwrap();
            let scope = RequestedScope::new(
                vec![identity.account_id.clone()],
                hantong_contracts::auth::default_cli_legacy_tools(),
            )
            .unwrap();
            let request = store
                .create_registration_request([1; 32], ClientKind::Cli, DEFAULT_CLI_LABEL, &scope)
                .unwrap();
            let id = hantong_platform::registration_id_for_request([1; 32]).unwrap();
            let pin = hantong_platform::sha256(&public()).unwrap();
            store
                .stage_google_oauth_cli_registration(
                    &CliRegistrationStage::new_google_oauth(request, id, public(), pin).unwrap(),
                )
                .unwrap();
            store.activate_cli_registration(id, 1, pin).unwrap();
            let target = RequestedScope::new(vec![identity.account_id], tools.clone()).unwrap();
            let pending = store
                .create_google_oauth_cli_scope_refresh([9; 32], target.clone(), pin)
                .unwrap();
            let mut failed = Fake::new(&store);
            if interrupt {
                failed.fail = Some("publish");
            }
            let result = complete_google_oauth_scope(&failed, *pending.refresh_id());
            if interrupt {
                assert_eq!(result, Err(ErrorCode::InternalError));
                let staged = store.load_cli_scope_refresh([9; 32]).unwrap().unwrap();
                assert_eq!(staged.phase(), ScopeRefreshPhase::Staged);
                assert_eq!(
                    store
                        .load_default_cli_registration()
                        .unwrap()
                        .unwrap()
                        .scope,
                    scope
                );
                recover_staged(&Fake::new(&store), &staged).unwrap();
            } else {
                result.unwrap();
            }
            let current = store.load_default_cli_registration().unwrap().unwrap();
            assert_eq!(current.scope, target);
            assert_eq!(current.profile_epoch, 2);
            assert_eq!(current.approved_channel, "google_oauth");
            assert!(store.load_pending_cli_scope_refresh().unwrap().is_none());
            drop(store);
            drop(cleanup);
        }
    }
}

#[test]
fn acc02_personal_google_send_upgrade_v9_preserves_b_until_recovery() {
    let root = std::env::temp_dir().join(format!(
        "hantong-personal-send-upgrade-{}",
        hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
    ));
    let cleanup = Cleanup(root.clone());
    let store = Store::open(root.join("data"), root.join("keys")).unwrap();
    store.stage_server_identity([7; 32]).unwrap();
    store
        .activate_server_identity([7; 32], public(), 1)
        .unwrap();
    let identity = hantong_contracts::google::GoogleIdentity {
        access_mode: hantong_contracts::google_write::GoogleAccessMode::ReadSend,
        account_id: format!(
            "google_{}",
            hantong_contracts::wire_hex::encode(
                &hantong_platform::sha256(b"synthetic-send-upgrade").unwrap()
            )
        ),
        subject: "synthetic-send-upgrade".into(),
        email: "synthetic@example.test".into(),
        granted_scopes: hantong_contracts::google_write::GoogleAccessMode::ReadSend
            .required_scopes()
            .iter()
            .map(|s| (*s).into())
            .collect(),
    };
    store
        .activate_google_account(&identity, "fixture.apps.googleusercontent.com", 0, None)
        .unwrap();
    let account = identity.account_id.clone();
    let legacy = RequestedScope::new(
        vec![account.clone()],
        hantong_contracts::auth::default_cli_legacy_tools(),
    )
    .unwrap();
    let request = store
        .create_registration_request([1; 32], ClientKind::Cli, DEFAULT_CLI_LABEL, &legacy)
        .unwrap();
    let registration_id = hantong_platform::registration_id_for_request([1; 32]).unwrap();
    let pin = hantong_platform::sha256(&public()).unwrap();
    store
        .stage_google_oauth_cli_registration(
            &CliRegistrationStage::new_google_oauth(request, registration_id, public(), pin)
                .unwrap(),
        )
        .unwrap();
    store
        .activate_cli_registration(registration_id, 1, pin)
        .unwrap();
    let body = RequestedScope::new(
        vec![account.clone()],
        hantong_contracts::auth::default_cli_body_tools(),
    )
    .unwrap();
    let first = store
        .create_google_oauth_cli_scope_refresh([9; 32], body.clone(), pin)
        .unwrap();
    complete_google_oauth_scope(&Fake::new(&store), *first.refresh_id()).unwrap();
    let before = store.load_default_cli_registration().unwrap().unwrap();
    assert_eq!(before.scope, body);
    assert_eq!(before.profile_epoch, 2);
    assert_eq!(before.approved_channel, "google_oauth");

    let send = RequestedScope::new(
        vec![account],
        hantong_contracts::auth::default_cli_send_tools(),
    )
    .unwrap();
    let pending = store
        .create_google_oauth_cli_send_upgrade([10; 32], send.clone(), pin)
        .unwrap();
    assert_eq!(pending.decision_channel(), "google_oauth");
    let mut failed = Fake::new(&store);
    failed.fail = Some("publish");
    assert_eq!(
        complete_google_oauth_scope(&failed, *pending.refresh_id()),
        Err(ErrorCode::InternalError)
    );
    let staged = store.load_cli_scope_refresh([10; 32]).unwrap().unwrap();
    assert_eq!(staged.phase(), ScopeRefreshPhase::Staged);
    let interrupted = store.load_default_cli_registration().unwrap().unwrap();
    assert_eq!(interrupted.scope, before.scope);
    assert_eq!(interrupted.registration_id, before.registration_id);
    assert_eq!(interrupted.public_key, before.public_key);
    assert_eq!(interrupted.state, RegistrationState::Provisioning);
    assert_eq!(interrupted.profile_epoch, 3);
    let recovered = Fake::new(&store);
    recover_staged(&recovered, &staged).unwrap();
    let after = store.load_default_cli_registration().unwrap().unwrap();
    assert_eq!(after.scope, send);
    assert_eq!(after.profile_epoch, 3);
    assert_eq!(after.approved_channel, "google_oauth");
    assert_eq!(after.registration_id, before.registration_id);
    assert_eq!(after.public_key, before.public_key);
    assert!(store.load_pending_cli_scope_refresh().unwrap().is_none());
    assert_eq!(
        store
            .load_cli_scope_refresh([10; 32])
            .unwrap()
            .unwrap()
            .phase(),
        ScopeRefreshPhase::Active
    );
    drop(store);
    drop(cleanup);
}

#[test]
fn acc02_google_oauth_recovery_rejects_non_oauth_base_without_side_effects() {
    let fixture = Fixture::new();
    let pending =
        fixture.terminal_pending_with_scope(hantong_contracts::auth::default_cli_manage_tools());
    let effects = Fake::new(&fixture.store);

    assert_eq!(
        complete_google_oauth_scope(&effects, *pending.refresh_id()),
        Err(ErrorCode::Conflict)
    );
    assert!(!effects.events.borrow().contains(&"stage"));
    assert!(effects.published.borrow().is_none());
    assert_eq!(
        fixture
            .store
            .load_cli_scope_refresh(*pending.refresh_id())
            .unwrap()
            .unwrap()
            .phase(),
        ScopeRefreshPhase::Pending
    );
}

impl Drop for Publication {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
impl<'a> Fake<'a> {
    fn new(store: &'a Store) -> Self {
        Self {
            store,
            server: public(),
            events: RefCell::new(vec![]),
            fail: None,
            uncertain: false,
            wrong_key: false,
            held: std::rc::Rc::new(Cell::new(false)),
            diagnostics: RefCell::new(vec![]),
            published: RefCell::new(None),
        }
    }
    fn hit(&self, name: &'static str) -> Result<(), ErrorCode> {
        self.events.borrow_mut().push(name);
        if self.fail == Some(name) && !self.uncertain {
            Err(ErrorCode::InternalError)
        } else {
            Ok(())
        }
    }
}
// The guard is owned for the activation boundary; this fake records lifetime.
impl Effects for Fake<'_> {
    type Key = ();
    type Publication = Publication;
    fn load(&self, id: [u8; 32]) -> Result<CliScopeRefresh, ErrorCode> {
        self.hit("load")?;
        self.store
            .load_cli_scope_refresh(id)?
            .ok_or(ErrorCode::NotFound)
    }
    fn server_public(&self) -> Result<[u8; 72], ErrorCode> {
        self.hit("server")?;
        Ok(self.server)
    }
    fn open_key(&self, _: [u8; 16]) -> Result<(), ErrorCode> {
        self.hit("open")
    }
    fn key_public(&self, _: &()) -> Result<[u8; 72], ErrorCode> {
        self.hit("key")?;
        Ok(if self.wrong_key { [0; 72] } else { public() })
    }
    fn stage(&self, r: &CliScopeRefresh) -> Result<CliScopeRefresh, ErrorCode> {
        self.hit("stage")?;
        let r = self.store.stage_cli_scope_refresh(r)?;
        if self.fail == Some("stage") && self.uncertain {
            Err(ErrorCode::InternalError)
        } else {
            Ok(r)
        }
    }
    fn publish(&self, id: [u8; 32], old: &[u8], new: &[u8]) -> Result<Publication, ErrorCode> {
        self.hit("publish")?;
        let row = self.store.load_cli_scope_refresh(id)?.unwrap();
        assert_eq!(row.phase(), ScopeRefreshPhase::Staged);
        let a = ParsedClientProfile::from_json(old)?;
        let b = ParsedClientProfile::from_json(new)?;
        self.published
            .borrow_mut()
            .replace((old.to_vec(), new.to_vec()));
        assert_eq!(a.registration_id(), b.registration_id());
        assert_eq!(a.client_public_key(), b.client_public_key());
        assert_eq!(a.client_key_id(), b.client_key_id());
        assert_eq!(a.server_public_key(), b.server_public_key());
        assert_eq!(a.profile_epoch() + 1, b.profile_epoch());
        assert_eq!(a.scope(), &row.base_registration().scope);
        assert_eq!(b.scope(), &row.target_scope());
        self.held.set(true);
        Ok(Publication(self.held.clone()))
    }
    fn activate(&self, r: &CliScopeRefresh) -> Result<RegistrationRecord, ErrorCode> {
        self.hit("activate")?;
        assert!(self.held.get());
        let record = self.store.activate_cli_scope_refresh(r)?;
        if self.fail == Some("activate") && self.uncertain {
            Err(ErrorCode::InternalError)
        } else {
            Ok(record)
        }
    }
    fn cleanup(&self, _: &mut Publication) -> Result<(), ErrorCode> {
        assert_eq!(
            self.store.load_default_cli_registration()?.unwrap().state,
            RegistrationState::Active
        );
        self.held.set(false);
        self.hit("cleanup")
    }
    fn report(&self, e: ErrorCode) {
        self.diagnostics.borrow_mut().push(e);
    }
}

#[test]
fn cli15_body_send_scope_fresh_request_preserves_existing_targets() {
    let fixture = Fixture::new();
    let fresh =
        prepare_for_pin(&fixture.store, hantong_platform::sha256(&public()).unwrap()).unwrap();
    assert_eq!(
        fresh.target_tools(),
        hantong_contracts::auth::default_cli_send_tools()
    );
    assert_eq!(
        fixture
            .store
            .load_default_cli_registration()
            .unwrap()
            .unwrap()
            .scope
            .tools(),
        hantong_contracts::auth::default_cli_legacy_tools()
    );
    assert_eq!(
        status(&fixture.store, *fresh.refresh_id(), false)
            .unwrap()
            .state(),
        ScopeRefreshProgress::Interrupted
    );
    assert_eq!(
        fixture
            .store
            .load_cli_scope_refresh(*fresh.refresh_id())
            .unwrap(),
        Some(fresh)
    );

    for tools in [
        hantong_contracts::auth::default_cli_legacy_tools(),
        hantong_contracts::auth::default_cli_folder_tools(),
        hantong_contracts::auth::default_cli_uid_tools(),
        hantong_contracts::auth::default_cli_metadata_tools(),
    ] {
        let fixture = Fixture::new();
        let pending = fixture.terminal_pending_with_scope(tools.clone());
        let pin = hantong_platform::sha256(&public()).unwrap();
        assert_eq!(prepare_for_pin(&fixture.store, pin).unwrap(), pending);
        let (old, new) = profiles(&pending, public()).unwrap();
        assert_eq!(
            ParsedClientProfile::from_json(&new)
                .unwrap()
                .scope()
                .tools(),
            tools
        );
        let staged = fixture.store.stage_cli_scope_refresh(&pending).unwrap();
        assert_eq!(prepare_for_pin(&fixture.store, pin).unwrap(), staged);
        assert_eq!(profiles(&staged, public()).unwrap(), (old, new));
        let effects = Fake::new(&fixture.store);
        recover_staged(&effects, &staged).unwrap();
        assert_eq!(
            fixture
                .store
                .load_default_cli_registration()
                .unwrap()
                .unwrap()
                .scope
                .tools(),
            tools
        );
    }
}

#[test]
fn sec03_terminal_scope_prepare_never_reuses_legacy_channel_even_with_same_target() {
    for staged in [false, true] {
        let fixture = Fixture::new();
        let pending = fixture.pending_with_scope(hantong_contracts::auth::default_cli_send_tools());
        let expected = if staged {
            fixture.store.stage_cli_scope_refresh(&pending).unwrap()
        } else {
            pending
        };
        let baseline = fixture
            .store
            .load_default_cli_registration()
            .unwrap()
            .unwrap();
        assert_eq!(
            prepare_for_pin(&fixture.store, hantong_platform::sha256(&public()).unwrap()),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            fixture
                .store
                .load_cli_scope_refresh(*expected.refresh_id())
                .unwrap(),
            Some(expected)
        );
        assert_eq!(
            fixture
                .store
                .load_default_cli_registration()
                .unwrap()
                .unwrap(),
            baseline
        );
    }
}

#[test]
fn cli15_body_send_scope_metadata_upgrade_requires_separate_activation() {
    let fixture = Fixture::new();
    let metadata =
        fixture.pending_with_scope(hantong_contracts::auth::default_cli_metadata_tools());
    let effects = Fake::new(&fixture.store);
    commit_consumed_review(&effects, &metadata, &review_snapshot(&metadata).unwrap()).unwrap();
    let metadata_history = fixture
        .store
        .load_cli_scope_refresh(*metadata.refresh_id())
        .unwrap()
        .unwrap();
    let before = fixture
        .store
        .load_default_cli_registration()
        .unwrap()
        .unwrap();
    let fresh =
        prepare_for_pin(&fixture.store, hantong_platform::sha256(&public()).unwrap()).unwrap();
    assert_eq!(fresh.base_registration(), &before);
    assert_eq!(fresh.target_accounts(), before.scope.accounts());
    assert_eq!(
        fresh.target_tools(),
        hantong_contracts::auth::default_cli_send_tools()
    );
    assert_eq!(
        fixture.store.load_default_cli_registration().unwrap(),
        Some(before.clone()),
        "requesting body/send access must preserve the active metadata-only grant"
    );
    let effects = Fake::new(&fixture.store);
    commit_consumed_review(&effects, &fresh, &review_snapshot(&fresh).unwrap()).unwrap();
    let active = fixture
        .store
        .load_default_cli_registration()
        .unwrap()
        .unwrap();
    assert_eq!(active.scope, fresh.target_scope());
    assert_eq!(active.registration_id, before.registration_id);
    assert_eq!(active.public_key, before.public_key);
    assert_eq!(active.profile_epoch, before.profile_epoch + 1);
    assert_eq!(
        fixture
            .store
            .load_cli_scope_refresh(*metadata.refresh_id())
            .unwrap(),
        Some(metadata_history)
    );
}

#[test]
fn cli15_body_send_scope_noop_preserves_registration_and_history() {
    let fixture = Fixture::new();
    let pending = fixture.pending_with_scope(hantong_contracts::auth::default_cli_send_tools());
    let effects = Fake::new(&fixture.store);
    commit_consumed_review(&effects, &pending, &review_snapshot(&pending).unwrap()).unwrap();
    let before = fixture
        .store
        .load_default_cli_registration()
        .unwrap()
        .unwrap();
    let history = fixture
        .store
        .load_cli_scope_refresh(*pending.refresh_id())
        .unwrap();
    assert_eq!(
        before.scope.accounts(),
        fixture.store.list_registrable_account_ids().unwrap()
    );
    assert_eq!(
        prepare_for_pin(&fixture.store, hantong_platform::sha256(&public()).unwrap()),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        fixture.store.load_default_cli_registration().unwrap(),
        Some(before)
    );
    assert_eq!(
        fixture
            .store
            .load_cli_scope_refresh(*pending.refresh_id())
            .unwrap(),
        history
    );
    assert!(
        fixture
            .store
            .load_pending_cli_scope_refresh()
            .unwrap()
            .is_none()
    );
    assert!(
        fixture
            .store
            .list_staged_cli_scope_refreshes()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn cli15_metadata_scope_staged_consent_recovers_without_fresh_account_snapshot() {
    let fixture = Fixture::new();
    let pending =
        fixture.terminal_pending_with_scope(hantong_contracts::auth::default_cli_metadata_tools());
    let original_profiles = profiles(&pending, public()).unwrap();
    let staged = fixture.store.stage_cli_scope_refresh(&pending).unwrap();
    let setup = crate::setup_imap::create(
        &fixture.store,
        &ImapSetupInput::new(
            None,
            None,
            "later.example".into(),
            993,
            ImapSetupTls::ImplicitTls,
            "synthetic-later".into(),
        )
        .unwrap(),
    )
    .unwrap();
    fixture.store.stage_imap_setup_request(&setup).unwrap();
    assert_ne!(
        fixture.store.list_registrable_account_ids().unwrap(),
        staged.target_accounts()
    );
    assert_eq!(
        prepare_for_pin(&fixture.store, hantong_platform::sha256(&public()).unwrap()).unwrap(),
        staged
    );
    let effects = Fake::new(&fixture.store);
    recover_staged(&effects, &staged).unwrap();
    assert_eq!(
        effects.published.borrow().as_ref(),
        Some(&original_profiles)
    );
    assert_eq!(
        fixture
            .store
            .load_default_cli_registration()
            .unwrap()
            .unwrap()
            .scope,
        pending.target_scope()
    );
}

#[test]
fn sec03_metadata_scope_cannot_consume_uid_review_as_metadata_consent() {
    let fixture = Fixture::new();
    let pending = fixture.pending_with_scope(hantong_contracts::auth::default_cli_metadata_tools());
    let b = pending.base_registration();
    let substituted = ScopeRefreshReview::new_with_tools(
        *pending.refresh_id(),
        pending.version(),
        b.registration_id,
        b.request_id,
        b.version,
        b.profile_epoch,
        *pending.client_public_key_hash(),
        *pending.server_public_key_hash(),
        b.scope.accounts().to_vec(),
        pending.target_accounts().to_vec(),
        b.scope.tools().to_vec(),
        hantong_contracts::auth::default_cli_uid_tools(),
        pending.created_at_utc_ms(),
        pending.expires_at_utc_ms(),
    )
    .unwrap();
    let effects = Fake::new(&fixture.store);
    assert_eq!(
        commit_consumed_review(&effects, &pending, &substituted),
        Err(ErrorCode::Conflict)
    );
    assert!(!effects.events.borrow().contains(&"stage"));
    assert!(effects.published.borrow().is_none());
}

#[test]
fn cli15_uid_scope_recovery_preserves_exact_profile_and_rejects_tool_substitution() {
    let fixture = Fixture::new();
    let pending = fixture.pending_with_scope(hantong_contracts::auth::default_cli_uid_tools());
    let b = pending.base_registration();
    let altered = ScopeRefreshReview::new_with_tools(
        *pending.refresh_id(),
        pending.version(),
        b.registration_id,
        b.request_id,
        b.version,
        b.profile_epoch,
        *pending.client_public_key_hash(),
        *pending.server_public_key_hash(),
        b.scope.accounts().to_vec(),
        pending.target_accounts().to_vec(),
        b.scope.tools().to_vec(),
        hantong_contracts::auth::default_cli_folder_tools(),
        pending.created_at_utc_ms(),
        pending.expires_at_utc_ms(),
    )
    .unwrap();
    let mut effects = Fake::new(&fixture.store);
    assert_eq!(
        commit_consumed_review(&effects, &pending, &altered),
        Err(ErrorCode::Conflict)
    );
    assert!(!effects.events.borrow().contains(&"stage"));
    effects.fail = Some("activate");
    assert_eq!(
        commit_consumed_review(&effects, &pending, &review_snapshot(&pending).unwrap()),
        Err(ErrorCode::InternalError)
    );
    let original_bytes = effects.published.borrow().clone().unwrap();
    let staged = fixture
        .store
        .load_cli_scope_refresh(*pending.refresh_id())
        .unwrap()
        .unwrap();
    let effects = Fake::new(&fixture.store);
    recover_staged(&effects, &staged).unwrap();
    assert_eq!(
        effects.published.borrow().as_ref().unwrap(),
        &original_bytes
    );
    assert_eq!(
        fixture
            .store
            .load_default_cli_registration()
            .unwrap()
            .unwrap()
            .scope
            .tools(),
        hantong_contracts::auth::default_cli_uid_tools()
    );
}

#[test]
fn cli15_folder_scope_refresh_activates_full_scope_and_recovery_reuses_same_profile_bytes() {
    let fixture = Fixture::new();
    let tools = hantong_contracts::auth::default_cli_folder_tools();
    let r = fixture.pending_with_scope(tools.clone());
    assert_eq!(r.target_scope().tools(), tools.as_slice());
    let mut f = Fake::new(&fixture.store);
    f.fail = Some("activate");
    assert_eq!(
        commit_consumed_review(&f, &r, &review_snapshot(&r).unwrap()),
        Err(ErrorCode::InternalError)
    );
    let bytes = f.published.borrow().clone().expect("publish bytes");
    let staged = fixture
        .store
        .load_cli_scope_refresh(*r.refresh_id())
        .unwrap()
        .unwrap();
    assert_eq!(staged.phase(), ScopeRefreshPhase::Staged);
    assert_eq!(
        fixture
            .store
            .load_default_cli_registration()
            .unwrap()
            .unwrap()
            .scope,
        r.base_registration().scope
    );
    let f = Fake::new(&fixture.store);
    assert_eq!(recover_staged(&f, &staged), Ok(()));
    let recovered = f
        .published
        .borrow()
        .clone()
        .expect("recovery publish bytes");
    assert_eq!(recovered, bytes);
    let active = fixture
        .store
        .load_default_cli_registration()
        .unwrap()
        .unwrap();
    assert_eq!(active.scope.tools(), tools.as_slice());
    assert_eq!(active.scope.accounts(), r.target_scope().accounts());
}
#[test]
fn cli15_scope_refresh_orders_stage_publish_activate_then_cleanup_without_new_identity() {
    let fixture = Fixture::new();
    let r = fixture.pending();
    let f = Fake::new(&fixture.store);
    assert_eq!(
        commit_consumed_review(&f, &r, &review_snapshot(&r).unwrap()),
        Ok(())
    );
    assert_eq!(
        fixture
            .store
            .load_cli_scope_refresh(*r.refresh_id())
            .unwrap()
            .unwrap()
            .phase(),
        ScopeRefreshPhase::Active
    );
    let events = f.events.borrow();
    for pair in [
        ("stage", "open"),
        ("open", "publish"),
        ("publish", "activate"),
        ("activate", "cleanup"),
    ] {
        assert!(
            events.iter().position(|x| *x == pair.0).unwrap()
                < events.iter().position(|x| *x == pair.1).unwrap()
        );
    }
    assert_eq!(
        fixture
            .store
            .load_default_cli_registration()
            .unwrap()
            .unwrap()
            .profile_epoch,
        2
    );
}
#[test]
fn cli15_scope_refresh_failures_never_advance_to_next_effect() {
    for (failed, next) in [
        ("stage", "open"),
        ("open", "publish"),
        ("key", "publish"),
        ("publish", "activate"),
        ("activate", "cleanup"),
    ] {
        for uncertain in [false, true] {
            if uncertain && !["stage", "activate"].contains(&failed) {
                continue;
            }
            let fixture = Fixture::new();
            let r = fixture.pending();
            let mut f = Fake::new(&fixture.store);
            f.fail = Some(failed);
            f.uncertain = uncertain;
            assert_eq!(
                commit_consumed_review(&f, &r, &review_snapshot(&r).unwrap()),
                Err(ErrorCode::InternalError)
            );
            assert!(f.events.borrow().contains(&failed));
            assert!(!f.events.borrow().contains(&next));
            assert!(!f.held.get());
        }
    }
}
#[test]
fn cli15_scope_refresh_recovery_uses_staged_decision_and_reopens_same_key() {
    let fixture = Fixture::new();
    let r = fixture.pending();
    let f = Fake::new(&fixture.store);
    assert_eq!(recover_staged(&f, &r), Err(ErrorCode::Conflict));
    assert!(!f.events.borrow().contains(&"publish"));
    let staged = fixture.store.stage_cli_scope_refresh(&r).unwrap();
    assert_eq!(recover_staged(&f, &staged), Ok(()));
    assert!(!f.events.borrow().contains(&"stage"));
    assert_eq!(recover_staged(&f, &staged), Err(ErrorCode::Conflict));
}
#[test]
fn cli15_scope_refresh_stale_or_changed_review_and_wrong_key_cannot_publish() {
    let fixture = Fixture::new();
    let r = fixture.pending();
    let f = Fake::new(&fixture.store);
    let b = r.base_registration();
    let wrong = ScopeRefreshReview::new(
        [8; 32],
        1,
        b.registration_id,
        b.request_id,
        b.version,
        b.profile_epoch,
        *r.client_public_key_hash(),
        *r.server_public_key_hash(),
        b.scope.accounts().to_vec(),
        r.target_accounts().to_vec(),
        r.created_at_utc_ms(),
        r.expires_at_utc_ms(),
    )
    .unwrap();
    assert_eq!(
        commit_consumed_review(&f, &r, &wrong),
        Err(ErrorCode::Conflict)
    );
    assert!(!f.events.borrow().contains(&"stage"));
    let mut f = Fake::new(&fixture.store);
    f.wrong_key = true;
    assert_eq!(
        commit_consumed_review(&f, &r, &review_snapshot(&r).unwrap()),
        Err(ErrorCode::Conflict)
    );
    assert!(!f.events.borrow().contains(&"publish"));
    assert_eq!(
        commit_consumed_review(&f, &r, &review_snapshot(&r).unwrap()),
        Err(ErrorCode::Conflict)
    );
}
#[test]
fn cli15_scope_refresh_cleanup_failure_does_not_revoke_successful_activation() {
    let fixture = Fixture::new();
    let r = fixture.pending();
    let mut f = Fake::new(&fixture.store);
    f.fail = Some("cleanup");
    assert_eq!(
        commit_consumed_review(&f, &r, &review_snapshot(&r).unwrap()),
        Ok(())
    );
    assert_eq!(
        f.diagnostics.borrow().as_slice(),
        [ErrorCode::InternalError]
    );
    assert_eq!(
        fixture
            .store
            .load_default_cli_registration()
            .unwrap()
            .unwrap()
            .state,
        RegistrationState::Active
    );
}

#[test]
fn sec03_readonly_add_two_accounts_keeps_old_grant_and_rejects_other_pending_target() {
    let f = GoogleFixture::new();
    f.register_one();
    assert_eq!(
        prepare_add_for_pin(&f.store, f.pin(), f.accounts[1]).err(),
        Some(ErrorCode::Conflict)
    );
    let first = prepare_add_for_pin(&f.store, f.pin(), f.accounts[0]).unwrap();
    let mut effects = Fake::new(&f.store);
    effects.server = f.key.0.as_ref().unwrap().public_key().unwrap();
    commit_consumed_review(&effects, &first, &review_snapshot(&first).unwrap()).unwrap();
    let before = f.store.load_default_cli_registration().unwrap().unwrap();
    let add = prepare_add_for_pin(&f.store, f.pin(), f.accounts[1]).unwrap();
    assert_eq!(add.target_accounts().len(), 2);
    assert!(add.target_accounts().contains(&f.accounts[0].account_id()));
    assert!(add.target_accounts().contains(&f.accounts[1].account_id()));
    assert_eq!(
        add.target_tools(),
        hantong_contracts::auth::default_cli_body_tools()
    );
    assert_eq!(
        prepare_add_for_pin(&f.store, f.pin(), f.accounts[1])
            .unwrap()
            .refresh_id(),
        add.refresh_id()
    );
    assert_eq!(
        prepare_add_for_pin(&f.store, f.pin(), f.accounts[0]).err(),
        Some(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store.load_default_cli_registration().unwrap().unwrap(),
        before
    );
    let mut effects = Fake::new(&f.store);
    effects.server = f.key.0.as_ref().unwrap().public_key().unwrap();
    commit_consumed_review(&effects, &add, &review_snapshot(&add).unwrap()).unwrap();
    let after = f.store.load_default_cli_registration().unwrap().unwrap();
    assert_eq!(after.scope.accounts(), add.target_accounts());
    assert_eq!(
        after.scope.tools(),
        hantong_contracts::auth::default_cli_body_tools()
    );
}
#[test]
fn sec03_readonly_add_rejects_write_scope_and_multiaccount_legacy_upgrade() {
    let a = hantong_contracts::registration::GoogleReadAccount::parse(&format!(
        "google_{}",
        "a".repeat(64)
    ))
    .unwrap();
    let b = hantong_contracts::registration::GoogleReadAccount::parse(&format!(
        "google_{}",
        "b".repeat(64)
    ))
    .unwrap();
    for tools in [
        hantong_contracts::auth::default_cli_send_tools(),
        hantong_contracts::auth::default_cli_legacy_tools(),
    ] {
        let base = RequestedScope::new(vec![a.account_id(), b.account_id()], tools).unwrap();
        assert_eq!(readonly_add_scope(&base, b), Err(ErrorCode::Conflict));
    }
}

#[test]
fn sec03_google_mutation_scope_v7_only_changes_selected_account_after_native_activation() {
    use hantong_contracts::{
        google_mutation::REQUEST_TOOL, google_write::GoogleAccessMode,
        scope_refresh::GoogleSendAccounts,
    };
    let f = GoogleFixture::new();
    f.register_one();
    let ids = f
        .accounts
        .iter()
        .map(|a| a.account_id())
        .collect::<Vec<_>>();
    for (index, account) in ids.iter().enumerate() {
        let binding = f.store.google_account_binding(account).unwrap();
        let mode = if index == 0 {
            GoogleAccessMode::Modify
        } else {
            GoogleAccessMode::ReadSend
        };
        f.store
            .activate_google_account(
                &hantong_contracts::google::GoogleIdentity {
                    access_mode: mode,
                    account_id: account.clone(),
                    subject: binding.subject,
                    email: "synthetic@example.test".into(),
                    granted_scopes: mode.required_scopes().iter().map(|s| (*s).into()).collect(),
                },
                &binding.client_id,
                binding.credential_version,
                None,
            )
            .unwrap();
    }
    let send =
        prepare_google_send_for_pin(&f.store, f.pin(), GoogleSendAccounts::parse(&ids).unwrap())
            .unwrap();
    let staged = f.store.stage_cli_scope_refresh(&send).unwrap();
    f.store.activate_cli_scope_refresh(&staged).unwrap();
    let before = f.store.load_default_cli_registration().unwrap().unwrap();
    let selected = GoogleSendAccounts::parse(&[ids[0].clone()]).unwrap();
    let change = prepare_google_changes_for_pin(&f.store, f.pin(), selected.clone()).unwrap();
    assert!(matches_google_changes(&change, selected));
    assert_eq!(
        f.store.load_default_cli_registration().unwrap().unwrap(),
        before
    );
    assert!(change.target_scope().permits_account(&ids[0], REQUEST_TOOL));
    assert!(!change.target_scope().permits_account(&ids[1], REQUEST_TOOL));
    assert!(
        prepare_google_changes_for_pin(
            &f.store,
            f.pin(),
            GoogleSendAccounts::parse(&[ids[1].clone()]).unwrap()
        )
        .is_err()
    );
    let staged = f.store.stage_cli_scope_refresh(&change).unwrap();
    let active = f.store.activate_cli_scope_refresh(&staged).unwrap();
    assert!(active.scope.permits_account(&ids[0], REQUEST_TOOL));
    assert!(!active.scope.permits_account(&ids[1], REQUEST_TOOL));
    assert!(active.scope.permits_account(&ids[1], "mail.send.execute"));
}
