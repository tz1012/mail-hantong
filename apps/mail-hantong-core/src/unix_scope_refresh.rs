//! Unix scope-refresh completion shared by OAuth and terminal callers.
//!
//! The Store row remains the authority for channel, registration epoch, and
//! target scope.  This module only provides the OAuth-origin entry point; it
//! does not create or approve a refresh.

use hantong_contracts::{
    ErrorCode,
    auth::RequestedScope,
    platform::OpaqueSigner,
    scope_refresh::{ScopeRefreshCommand, ScopeRefreshProgress, ScopeRefreshReceipt},
};
use hantong_store::{
    RegistrationRecord, RegistrationState, Store,
    scope_refresh::{CliScopeRefresh, ScopeRefreshPhase},
};
use std::path::Path;

/// Build a terminal-origin scope request. Creation only records pending intent;
/// the foreground terminal review remains the sole confirmation path.
pub(crate) fn prepare(
    store: &Store,
    server: &mut impl OpaqueSigner,
    command: ScopeRefreshCommand,
) -> Result<CliScopeRefresh, ErrorCode> {
    let pin =
        hantong_platform::sha256(&server.public_key().map_err(|_| ErrorCode::InternalError)?)?;
    let target = match command {
        ScopeRefreshCommand::Request => RequestedScope::new(
            store.list_registrable_account_ids()?,
            hantong_contracts::auth::default_cli_send_tools(),
        )?,
        ScopeRefreshCommand::RequestManage(accounts) => {
            let ids = accounts.accounts();
            validate_accounts(store, &ids)?;
            let base = store
                .load_default_cli_registration()?
                .ok_or(ErrorCode::ClientNotRegistered)?;
            if base.scope.accounts().iter().any(|id| !ids.contains(id)) {
                return Err(ErrorCode::Conflict);
            }
            let mut tools = hantong_contracts::auth::default_cli_manage_tools();
            for tool in base.scope.tools() {
                if !tools.contains(tool) {
                    tools.push(tool.clone());
                }
            }
            tools.sort();
            if !hantong_contracts::auth::is_supported_default_cli_manage_toolset_transition(
                base.scope.tools(),
                &tools,
            ) {
                return Err(ErrorCode::Conflict);
            }
            RequestedScope::new(ids, tools)?
        }
        ScopeRefreshCommand::RequestGoogleReadOnly(account) => {
            let id = account.account_id();
            validate_accounts(store, std::slice::from_ref(&id))?;
            RequestedScope::new(vec![id], hantong_contracts::auth::default_cli_body_tools())?
        }
        ScopeRefreshCommand::AddGoogleReadOnly(account) => {
            let id = account.account_id();
            validate_accounts(store, std::slice::from_ref(&id))?;
            let base = store
                .load_default_cli_registration()?
                .ok_or(ErrorCode::ClientNotRegistered)?;
            if base.scope.tools() != hantong_contracts::auth::default_cli_body_tools()
                && !(base.scope.tools() == hantong_contracts::auth::default_cli_legacy_tools()
                    && base.scope.accounts() == [id.clone()])
            {
                return Err(ErrorCode::Conflict);
            }
            let mut ids = base.scope.accounts().to_vec();
            if !ids.contains(&id) {
                ids.push(id);
            }
            ids.sort();
            RequestedScope::new(ids, hantong_contracts::auth::default_cli_body_tools())?
        }
        ScopeRefreshCommand::RequestGoogleSend(accounts) => {
            let ids = accounts.accounts();
            validate_google_operation(
                store,
                &ids,
                hantong_contracts::google_write::GoogleOperation::Send,
            )?;
            let base = store
                .load_default_cli_registration()?
                .ok_or(ErrorCode::ClientNotRegistered)?;
            if base.scope.accounts().iter().any(|id| !ids.contains(id))
                || !hantong_contracts::auth::is_supported_default_cli_toolset(base.scope.tools())
            {
                return Err(ErrorCode::Conflict);
            }
            RequestedScope::new(ids, hantong_contracts::auth::default_cli_send_tools())?
        }
        ScopeRefreshCommand::RequestGoogleChanges(accounts) => {
            let ids = accounts.accounts();
            validate_google_operation(
                store,
                &ids,
                hantong_contracts::google_write::GoogleOperation::ChangeLabels,
            )?;
            let base = store
                .load_default_cli_registration()?
                .ok_or(ErrorCode::ClientNotRegistered)?;
            if base.scope.tools() != hantong_contracts::auth::default_cli_send_tools()
                && !hantong_contracts::auth::is_google_change_toolset(base.scope.tools())
            {
                return Err(ErrorCode::Conflict);
            }
            let mut tools = base.scope.tools().to_vec();
            for id in ids {
                let grant = hantong_contracts::google_mutation::account_grant(&id)?;
                if !tools.contains(&grant) {
                    tools.push(grant);
                }
            }
            tools.sort();
            if !hantong_contracts::auth::is_google_change_toolset(&tools) {
                return Err(ErrorCode::QuotaExceeded);
            }
            RequestedScope::new(base.scope.accounts().to_vec(), tools)?
        }
        ScopeRefreshCommand::Status(_) => return Err(ErrorCode::InvalidInput),
    };
    prepare_exact(store, target, pin)
}

fn validate_accounts(store: &Store, ids: &[String]) -> Result<(), ErrorCode> {
    let available = store.list_registrable_account_ids()?;
    if ids.iter().any(|id| !available.contains(id)) {
        return Err(ErrorCode::NotFound);
    }
    Ok(())
}

fn validate_google_operation(
    store: &Store,
    ids: &[String],
    operation: hantong_contracts::google_write::GoogleOperation,
) -> Result<(), ErrorCode> {
    validate_accounts(store, ids)?;
    for id in ids {
        store
            .google_account_binding(id)?
            .access_mode
            .check_operation(operation)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConflictField {
    Accounts,
    Tools,
    BaseRegistration,
    ServerBinding,
    DecisionChannel,
}
impl ConflictField {
    fn as_str(self) -> &'static str {
        match self {
            Self::Accounts => "accounts",
            Self::Tools => "tools",
            Self::BaseRegistration => "base_registration",
            Self::ServerBinding => "server_binding",
            Self::DecisionChannel => "decision_channel",
        }
    }
    fn diagnostic(self) -> String {
        format!(
            "hantong_scope_refresh stage=request_conflict field={} code=CONFLICT",
            self.as_str()
        )
    }
}

fn conflict_fields(
    refresh: &CliScopeRefresh,
    target: &RequestedScope,
    pin: &[u8; 32],
    current: Option<&RegistrationRecord>,
) -> Vec<ConflictField> {
    let mut expected = refresh.base_registration().clone();
    if refresh.phase() == ScopeRefreshPhase::Staged {
        expected.state = RegistrationState::Provisioning;
        expected.profile_epoch = refresh.target_profile_epoch();
        expected.version += 1;
    }
    [
        (
            ConflictField::Accounts,
            refresh.target_accounts() != target.accounts(),
        ),
        (
            ConflictField::Tools,
            refresh.target_tools() != target.tools(),
        ),
        (ConflictField::BaseRegistration, current != Some(&expected)),
        (
            ConflictField::ServerBinding,
            refresh.server_public_key_hash() != pin,
        ),
        (
            ConflictField::DecisionChannel,
            refresh.decision_channel() != "terminal_cli",
        ),
    ]
    .into_iter()
    .filter_map(|(field, differs)| differs.then_some(field))
    .collect()
}

fn can_reuse(
    refresh: &CliScopeRefresh,
    target: &RequestedScope,
    pin: &[u8; 32],
    current: Option<&RegistrationRecord>,
    expire: impl FnOnce() -> Result<bool, ErrorCode>,
) -> Result<bool, ErrorCode> {
    // Expiration closes only pending intent; a prior staged decision is never replaced.
    if refresh.phase() == ScopeRefreshPhase::Pending && expire()? {
        return Ok(false);
    }
    let fields = conflict_fields(refresh, target, pin, current);
    if !fields.is_empty() {
        use std::io::Write;
        for field in fields {
            let _ = writeln!(std::io::stderr().lock(), "{}", field.diagnostic());
        }
        return Err(ErrorCode::Conflict);
    }
    Ok(true)
}

fn prepare_exact(
    store: &Store,
    target: RequestedScope,
    pin: [u8; 32],
) -> Result<CliScopeRefresh, ErrorCode> {
    let base = store.load_default_cli_registration()?;
    for refresh in store
        .list_staged_cli_scope_refreshes()?
        .into_iter()
        .chain(store.load_pending_cli_scope_refresh()?)
    {
        if can_reuse(&refresh, &target, &pin, base.as_ref(), || {
            store.expire_cli_scope_refresh(&refresh)
        })? {
            return Ok(refresh);
        }
    }
    store.create_terminal_cli_scope_refresh(hantong_platform::random_nonce()?, target, pin)
}

pub(crate) fn status(store: &Store, id: [u8; 32]) -> Result<ScopeRefreshReceipt, ErrorCode> {
    let mut refresh = store
        .load_cli_scope_refresh(id)?
        .ok_or(ErrorCode::NotFound)?;
    if refresh.phase() == ScopeRefreshPhase::Pending && store.expire_cli_scope_refresh(&refresh)? {
        refresh = store
            .load_cli_scope_refresh(id)?
            .ok_or(ErrorCode::InternalError)?;
    }
    let state = match refresh.phase() {
        ScopeRefreshPhase::Pending => ScopeRefreshProgress::Pending,
        ScopeRefreshPhase::Staged => ScopeRefreshProgress::Provisioning,
        ScopeRefreshPhase::Active => ScopeRefreshProgress::Ready,
        ScopeRefreshPhase::Denied => ScopeRefreshProgress::Denied,
        ScopeRefreshPhase::Expired => ScopeRefreshProgress::Expired,
    };
    ScopeRefreshReceipt::new(*refresh.refresh_id(), refresh.expires_at_utc_ms(), state)
}

/// Complete a previously-created Google OAuth scope refresh.
///
/// The caller must pass the exact pending row it received from Store.  The
/// coordinator reloads it by id and rechecks the OAuth decision channel and
/// source registration before staging, publishing, and activating.
pub(crate) fn complete_google(
    store: &Store,
    secrets_root: &Path,
    server: &mut impl OpaqueSigner,
    refresh: &CliScopeRefresh,
) -> Result<RegistrationRecord, ErrorCode> {
    crate::unix_registration::activate_google_scope_refresh(
        store,
        secrets_root,
        server,
        *refresh.refresh_id(),
    )
}

#[cfg(all(test, target_os = "linux"))]
pub(crate) mod tests {
    use super::*;
    use hantong_contracts::auth::RequestedScope;
    use hantong_store::{CliRegistrationStage, ClientKind, DEFAULT_CLI_LABEL, Store};

    pub(crate) fn with_fixture(
        name: &str,
        test: impl FnOnce(
            &Store,
            &hantong_platform::AppDirectories,
            &mut hantong_secrets::unix_signing::OpaqueSigner,
            &RegistrationRecord,
        ),
    ) {
        with_fixture_origin(name, "terminal_cli", test);
    }

    pub(crate) fn with_fixture_origin(
        name: &str,
        origin: &str,
        test: impl FnOnce(
            &Store,
            &hantong_platform::AppDirectories,
            &mut hantong_secrets::unix_signing::OpaqueSigner,
            &RegistrationRecord,
        ),
    ) {
        if std::env::var_os("HANTONG_UNIX_SCOPE_TEST_CHILD").is_none() {
            let root = std::env::temp_dir().join(format!(
                "hantong-unix-scope-{}",
                hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
            ));
            std::fs::create_dir(&root).unwrap();
            for name in ["data-home", "config-home", "runtime"] {
                std::fs::create_dir(root.join(name)).unwrap();
                std::fs::set_permissions(
                    root.join(name),
                    std::os::unix::fs::PermissionsExt::from_mode(0o700),
                )
                .unwrap();
            }
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", name, "--nocapture"])
                .env("HANTONG_UNIX_SCOPE_TEST_CHILD", "1")
                .env("XDG_DATA_HOME", root.join("data-home"))
                .env("XDG_CONFIG_HOME", root.join("config-home"))
                .env("XDG_RUNTIME_DIR", root.join("runtime"))
                .status()
                .unwrap();
            std::fs::remove_dir_all(root).unwrap();
            assert!(status.success());
            return;
        }
        let paths = hantong_platform::app_directories().unwrap();
        let _secrets_guard =
            hantong_platform::unix_paths::UnixDirectoryGuard::prepare(&paths.secrets).unwrap();
        let _data_guard =
            hantong_platform::unix_paths::UnixDirectoryGuard::prepare(&paths.data).unwrap();
        crate::test_support::ensure_test_backend();
        let store = Store::open(&paths.data, &paths.secrets).unwrap();
        let mut server =
            hantong_secrets::unix_signing::create_server_signing_key(&paths.secrets, [7; 32])
                .unwrap();
        let server_key = server.public_key().unwrap();
        let server_pin = hantong_platform::sha256(&server_key).unwrap();
        store.stage_server_identity([7; 32]).unwrap();
        store
            .activate_server_identity([7; 32], server_key, 1)
            .unwrap();
        let draft = hantong_domain::account_provisioning::NewImapAccountDraft::new(
            "fixture_account".into(),
            Some("Synthetic account".into()),
            None,
        )
        .unwrap();
        let config = hantong_domain::mail_connection::MailConnectionConfig::from_json(
            &serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "account_id": "fixture_account",
                "imap": {
                    "settings_version": 1,
                    "host": "imap.example.invalid",
                    "port": 993,
                    "tls": "implicit_tls",
                    "login_name": "fixture@example.invalid",
                    "authentication": "app_password"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let mutation = hantong_store::mail_connection::MailMutation::new(
            "scope-fixture-operation".into(),
            "scope-fixture-idempotency".into(),
            0,
            hantong_store::account_provisioning::imap_account_stage_payload_hash(
                &draft,
                config.imap(),
            )
            .unwrap(),
        )
        .unwrap();
        let staged = store
            .stage_imap_account(&draft, config.imap(), &mutation)
            .unwrap();
        store
            .finish_initial_imap_probe(
                "fixture_account",
                staged.account_version,
                staged.imap_version,
                0,
                &hantong_store::account_provisioning::InitialImapProbeOutcome::Ready(vec![
                    "imap.move".into(),
                ]),
            )
            .unwrap();
        let account = if origin == "google_oauth" {
            let subject = "synthetic-scope-subject";
            let id = format!(
                "google_{}",
                hantong_contracts::wire_hex::encode(
                    &hantong_platform::sha256(subject.as_bytes()).unwrap()
                )
            );
            store
                .activate_google_account(
                    &hantong_contracts::google::GoogleIdentity {
                        access_mode: hantong_contracts::google_write::GoogleAccessMode::ReadOnly,
                        account_id: id.clone(),
                        subject: subject.into(),
                        email: "fixture@example.invalid".into(),
                        granted_scopes: hantong_contracts::google::GOOGLE_SCOPES
                            .iter()
                            .map(|scope| (*scope).into())
                            .collect(),
                    },
                    "fixture.apps.googleusercontent.com",
                    0,
                    None,
                )
                .unwrap();
            id
        } else {
            "fixture_account".to_owned()
        };
        let registration_request = store
            .create_registration_request(
                [1; 32],
                ClientKind::Cli,
                DEFAULT_CLI_LABEL,
                &RequestedScope::new(vec![account], vec!["mail.accounts.list".into()]).unwrap(),
            )
            .unwrap();
        let registration_id = hantong_platform::registration_id_for_request([1; 32]).unwrap();
        let client = hantong_secrets::unix_signing::load_client_signing_key(
            &paths.secrets,
            registration_id,
            None,
        )
        .unwrap();
        let stage = match origin {
            "google_oauth" => CliRegistrationStage::new_google_oauth(
                registration_request,
                registration_id,
                client.public_key().unwrap(),
                server_pin,
            ),
            "approval_helper" => CliRegistrationStage::new(
                registration_request,
                registration_id,
                client.public_key().unwrap(),
                server_pin,
            ),
            "terminal_cli" => CliRegistrationStage::new_terminal_cli(
                registration_request,
                registration_id,
                client.public_key().unwrap(),
                server_pin,
            ),
            _ => panic!("unsupported synthetic origin"),
        }
        .unwrap();
        let staged = match origin {
            "google_oauth" => store.stage_google_oauth_cli_registration(&stage),
            "approval_helper" => store.stage_cli_registration(&stage),
            _ => store.stage_terminal_cli_registration(&stage),
        }
        .unwrap();
        let active = store
            .activate_cli_registration(registration_id, staged.version, server_pin)
            .unwrap();
        let profile = hantong_ipc_client::profile::ParsedClientProfile::new_with_scope(
            registration_id,
            hantong_platform::client_key_id_for_registration(registration_id).unwrap(),
            active.public_key,
            server_key,
            active.profile_epoch,
            active.scope.clone(),
        )
        .unwrap();
        hantong_platform::unix_profile_publication::publish_default_profile(
            &profile.to_json().unwrap(),
        )
        .unwrap();
        test(&store, &paths, &mut server, &active);
    }

    #[test]
    fn unix_scope_request_is_pending_and_preserves_active_registration() {
        with_fixture(
            "unix_scope_refresh::tests::unix_scope_request_is_pending_and_preserves_active_registration",
            |store, _, server, active| {
                let refresh = prepare(store, server, ScopeRefreshCommand::Request).unwrap();

                assert_eq!(refresh.decision_channel(), "terminal_cli");
                assert_eq!(
                    refresh.phase(),
                    hantong_store::scope_refresh::ScopeRefreshPhase::Pending
                );
                let current = store.load_default_cli_registration().unwrap().unwrap();
                assert_eq!(current.profile_epoch, active.profile_epoch);
                assert_eq!(current.scope, active.scope);
                assert!(
                    store
                        .load_cli_scope_refresh(*refresh.refresh_id())
                        .unwrap()
                        .is_some()
                );
            },
        );
    }
    #[test]
    fn cli15_conflict_diagnostics_are_closed_field_names_and_never_change_access() {
        const NAME: &str = "unix_scope_refresh::tests::cli15_conflict_diagnostics_are_closed_field_names_and_never_change_access";
        with_fixture(NAME, |store, _, server, active| {
            let pending = prepare(store, server, ScopeRefreshCommand::Request).unwrap();
            let pin = *pending.server_public_key_hash();
            let target = pending.target_scope();
            assert!(conflict_fields(&pending, &target, &pin, Some(active)).is_empty());
            let other_accounts =
                RequestedScope::new(vec!["other_fixture".into()], target.tools().to_vec()).unwrap();
            assert_eq!(
                conflict_fields(&pending, &other_accounts, &pin, Some(active)),
                [ConflictField::Accounts]
            );
            let other_tools = RequestedScope::new(
                target.accounts().to_vec(),
                hantong_contracts::auth::default_cli_body_tools(),
            )
            .unwrap();
            assert_eq!(
                conflict_fields(&pending, &other_tools, &pin, Some(active)),
                [ConflictField::Tools]
            );
            let mut stale_base = active.clone();
            stale_base.version += 1;
            assert_eq!(
                conflict_fields(&pending, &target, &pin, Some(&stale_base)),
                [ConflictField::BaseRegistration]
            );
            assert_eq!(
                conflict_fields(&pending, &target, &[99; 32], Some(active)),
                [ConflictField::ServerBinding]
            );
            let all_changed =
                RequestedScope::new(vec!["other_fixture".into()], other_tools.tools().to_vec())
                    .unwrap();
            assert_eq!(
                conflict_fields(&pending, &all_changed, &[99; 32], None),
                [
                    ConflictField::Accounts,
                    ConflictField::Tools,
                    ConflictField::BaseRegistration,
                    ConflictField::ServerBinding
                ]
            );
            assert!(matches!(
                can_reuse(&pending, &all_changed, &[99; 32], None, || Ok(false)),
                Err(ErrorCode::Conflict)
            ));
            // A normal Store expiry wins over stale target/base comparison. Store CAS is tested separately.
            assert_eq!(
                can_reuse(&pending, &all_changed, &[99; 32], None, || Ok(true)),
                Ok(false)
            );
            assert_eq!(
                store.load_default_cli_registration().unwrap().as_ref(),
                Some(active)
            );
            assert_eq!(
                store
                    .load_cli_scope_refresh(*pending.refresh_id())
                    .unwrap()
                    .unwrap(),
                pending
            );
            let receipt = status(store, *pending.refresh_id()).unwrap();
            assert_eq!(receipt.state(), ScopeRefreshProgress::Pending);
            for field in [
                ConflictField::Accounts,
                ConflictField::Tools,
                ConflictField::BaseRegistration,
                ConflictField::ServerBinding,
                ConflictField::DecisionChannel,
            ] {
                let line = field.diagnostic();
                assert_eq!(
                    line,
                    format!(
                        "hantong_scope_refresh stage=request_conflict field={} code=CONFLICT",
                        field.as_str()
                    )
                );
                assert!(
                    !line.contains("fixture")
                        && !line.contains('@')
                        && !line.contains('/')
                        && !line.contains("99")
                );
            }
        });
    }

    #[test]
    fn cli15_legacy_pending_channel_conflict_preserves_registration_and_request() {
        const NAME: &str = "unix_scope_refresh::tests::cli15_legacy_pending_channel_conflict_preserves_registration_and_request";
        with_fixture_origin(NAME, "approval_helper", |store, _, server, active| {
            let pin = hantong_platform::sha256(&server.public_key().unwrap()).unwrap();
            let target = RequestedScope::new(
                active.scope.accounts().to_vec(),
                hantong_contracts::auth::default_cli_send_tools(),
            )
            .unwrap();
            let pending = store
                .create_cli_scope_refresh_with_scope([33; 32], target.clone(), pin)
                .unwrap();
            assert_eq!(
                conflict_fields(&pending, &target, &pin, Some(active)),
                [ConflictField::DecisionChannel]
            );
            assert!(matches!(
                prepare(store, server, ScopeRefreshCommand::Request),
                Err(ErrorCode::Conflict)
            ));
            assert_eq!(
                store.load_default_cli_registration().unwrap().as_ref(),
                Some(active)
            );
            assert_eq!(
                store
                    .load_cli_scope_refresh(*pending.refresh_id())
                    .unwrap()
                    .unwrap(),
                pending
            );
        });
    }
}
