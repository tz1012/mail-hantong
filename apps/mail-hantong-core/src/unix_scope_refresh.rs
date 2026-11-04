//! Unix scope-refresh completion shared by OAuth and terminal callers.
//!
//! The Store row remains the authority for channel, registration epoch, and
//! target scope.  This module only provides the OAuth-origin entry point; it
//! does not create or approve a refresh.

use hantong_contracts::{
    ErrorCode, auth::RequestedScope, platform::OpaqueSigner, scope_refresh::ScopeRefreshCommand,
};
use hantong_store::{RegistrationRecord, Store, scope_refresh::CliScopeRefresh};
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
        if refresh.decision_channel() != "terminal_cli"
            || refresh.target_accounts() != target.accounts()
            || refresh.target_tools() != target.tools()
            || refresh.server_public_key_hash() != &pin
            || base.as_ref() != Some(refresh.base_registration())
        {
            return Err(ErrorCode::Conflict);
        }
        if refresh.phase() != hantong_store::scope_refresh::ScopeRefreshPhase::Pending
            || !store.expire_cli_scope_refresh(&refresh)?
        {
            return Ok(refresh);
        }
    }
    store.create_terminal_cli_scope_refresh(hantong_platform::random_nonce()?, target, pin)
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
mod tests {
    use super::*;
    use hantong_contracts::{
        auth::RequestedScope,
        platform::{OpaqueSigner, PlatformError},
    };
    use hantong_store::{CliRegistrationStage, ClientKind, DEFAULT_CLI_LABEL, Store};

    struct PublicOnly;
    impl OpaqueSigner for PublicOnly {
        fn public_key(&self) -> Result<[u8; 72], PlatformError> {
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
            Ok(key)
        }
        fn sign(&mut self, _: &[u8]) -> Result<[u8; 64], PlatformError> {
            Err(PlatformError::Denied)
        }
    }

    #[test]
    fn unix_scope_request_is_pending_and_preserves_active_registration() {
        const NAME: &str = "unix_scope_refresh::tests::unix_scope_request_is_pending_and_preserves_active_registration";
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
                .args(["--exact", NAME, "--nocapture"])
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
        let vault = hantong_secrets::headless_vault::HeadlessVault::initialize(
            &paths.secrets,
            "synthetic-scope-passphrase".to_owned().into(),
        )
        .unwrap()
        .into_shared();
        hantong_secrets::unix_secret_store::UnixSecretStore::install_headless_backend(vault)
            .unwrap();
        let store = Store::open(&paths.data, &paths.secrets).unwrap();
        let mut server = PublicOnly;
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
        let account = "fixture_account".to_owned();
        let registration_request = store
            .create_registration_request(
                [1; 32],
                ClientKind::Cli,
                DEFAULT_CLI_LABEL,
                &RequestedScope::new(vec![account], vec!["mail.accounts.list".into()]).unwrap(),
            )
            .unwrap();
        let registration_id = hantong_platform::registration_id_for_request([1; 32]).unwrap();
        let staged = store
            .stage_terminal_cli_registration(
                &CliRegistrationStage::new_terminal_cli(
                    registration_request,
                    registration_id,
                    server_key,
                    server_pin,
                )
                .unwrap(),
            )
            .unwrap();
        let active = store
            .activate_cli_registration(registration_id, staged.version, server_pin)
            .unwrap();
        let refresh = prepare(&store, &mut server, ScopeRefreshCommand::Request).unwrap();
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
    }
}
