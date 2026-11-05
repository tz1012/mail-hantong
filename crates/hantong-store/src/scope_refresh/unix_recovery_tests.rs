use super::*;
use crate::{CliRegistrationStage, ClientKind, DEFAULT_CLI_LABEL, RegistrationRecord, Store};
use hantong_contracts::{ErrorCode, auth::RequestedScope, wire_hex};
use rusqlite::params;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

fn ensure_headless_backend() {
    static READY: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    READY.get_or_init(|| {
        let root = std::env::temp_dir().join(format!(
            "hantong-scope-refresh-vault-{}-{}",
            std::process::id(),
            AtomicU64::new(0).fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
        let vault = hantong_secrets::headless_vault::HeadlessVault::initialize(
            &root,
            "synthetic-scope-refresh-passphrase".to_owned().into(),
        )
        .unwrap();
        match hantong_secrets::unix_secret_store::UnixSecretStore::install_headless_backend(
            vault.into_shared(),
        ) {
            Ok(()) => {}
            Err(hantong_secrets::unix_secret_store::UnixSecretError::Conflict) => {
                let _ = fs::remove_dir_all(&root);
            }
            Err(error) => panic!("install synthetic scope refresh vault: {error}"),
        }
    });
}

struct Fixture {
    store: Option<Store>,
    root: PathBuf,
    registration: RegistrationRecord,
    credential_vault: hantong_secrets::headless_vault::SharedHeadlessVault,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-scope-refresh-unix-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        ensure_headless_backend();
        let credential_root = root.join("credential-vault");
        fs::create_dir(&credential_root).unwrap();
        std::fs::set_permissions(
            &credential_root,
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        let credential_vault = hantong_secrets::headless_vault::HeadlessVault::initialize(
            &credential_root,
            "synthetic-credential-passphrase".to_owned().into(),
        )
        .unwrap()
        .into_shared();
        {
            let vault = credential_vault.lock().unwrap();
            vault
                .replace(
                    "model-synthetic",
                    b"synthetic-model-key",
                    vault.generation().unwrap(),
                )
                .unwrap();
        }
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        store
            .with_connection(|c| {
                c.execute(
                    "INSERT INTO server_identity VALUES(1,?1,'active',?2,1,1,1,1)",
                    params![[9_u8; 32].as_slice(), public_key().as_slice()],
                )?;
                c.execute(
                    "INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES('a','imap',1,'ready','[]','[]',1)",
                    [],
                )?;
                c.execute(
                    "INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES('b','imap',2,'ready','[]','[]',1)",
                    [],
                )?;
                c.execute(
                    "INSERT INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES('a','imap',1,'imap.example',993,'implicit_tls','fixture-user','app_password',1)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let request = store
            .create_registration_request(
                [1; 32],
                ClientKind::Cli,
                DEFAULT_CLI_LABEL,
                &scope(&["a"]),
            )
            .unwrap();
        let registration_id =
            hantong_platform::registration_id_for_request(request.request_id).unwrap();
        let staged = store
            .stage_terminal_cli_registration(
                &CliRegistrationStage::new_terminal_cli(
                    request,
                    registration_id,
                    public_key(),
                    pin(),
                )
                .unwrap(),
            )
            .unwrap();
        let registration = store
            .activate_cli_registration(registration_id, staged.version, pin())
            .unwrap();
        Self {
            store: Some(store),
            root,
            registration,
            credential_vault,
        }
    }

    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.store.take());
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn public_key() -> [u8; 72] {
    let mut key = [0; 72];
    key[..8].copy_from_slice(b"ECS1\x20\0\0\0");
    key[8..40].copy_from_slice(
        &wire_hex::decode_fixed::<32>(
            "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
        )
        .unwrap(),
    );
    key[40..].copy_from_slice(
        &wire_hex::decode_fixed::<32>(
            "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
        )
        .unwrap(),
    );
    key
}

fn pin() -> [u8; 32] {
    hantong_platform::sha256(&public_key()).unwrap()
}

fn scope(accounts: &[&str]) -> RequestedScope {
    RequestedScope::new(
        accounts.iter().map(|value| (*value).into()).collect(),
        vec!["mail.accounts.list".into()],
    )
    .unwrap()
}

fn snapshot(f: &Fixture) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
    f.store()
        .with_connection(|c| {
            [
                "accounts",
                "mail_connection_settings",
                "server_identity",
                "client_registrations",
                "client_registration_accounts",
                "client_registration_tools",
            ]
            .iter()
            .map(|table| {
                let mut stmt = c.prepare(&format!("SELECT * FROM {table} ORDER BY 1,2"))?;
                let len = stmt.column_count();
                stmt.query_map([], |row| (0..len).map(|i| row.get(i)).collect())?
                    .collect()
            })
            .collect()
        })
        .unwrap()
}

fn shift_expired(f: &Fixture, expected: &CliScopeRefresh) -> CliScopeRefresh {
    let mut changed = expected.clone();
    let delta = -400_000;
    changed.created_at_utc_ms += delta;
    changed.expires_at_utc_ms += delta;
    changed.updated_at_utc_ms += delta;
    changed.payload_hash = hash_payload(&changed).unwrap();
    f.store()
        .with_connection(|c| {
            c.execute(
                "UPDATE client_scope_refreshes SET created_at_utc_ms=?2,expires_at_utc_ms=?3,updated_at_utc_ms=?4,payload_hash=?5 WHERE refresh_id=?1",
                params![
                    changed.refresh_id.as_slice(),
                    changed.created_at_utc_ms,
                    changed.expires_at_utc_ms,
                    changed.updated_at_utc_ms,
                    changed.payload_hash.as_slice(),
                ],
            )?;
            c.execute(
                "UPDATE client_registrations SET approved_at_utc_ms=approved_at_utc_ms+?1,updated_at_utc_ms=updated_at_utc_ms+?1",
                [delta],
            )?;
            c.execute(
                "UPDATE client_registration_requests SET created_at_utc_ms=created_at_utc_ms+?1,expires_at_utc_ms=expires_at_utc_ms+?1,decided_at_utc_ms=decided_at_utc_ms+?1",
                [delta],
            )?;
            Ok(())
        })
        .unwrap();
    changed
}

#[test]
fn unix_scope_expiry_closes_expired_pending_without_granting_or_deleting_state() {
    let f = Fixture::new();
    let target = RequestedScope::new(
        vec!["a".into(), "b".into()],
        hantong_contracts::auth::default_cli_folder_tools(),
    )
    .unwrap();
    let pending = f
        .store()
        .create_terminal_cli_scope_refresh([71; 32], target, pin())
        .unwrap();
    let before = snapshot(&f);

    assert!(!f.store().expire_cli_scope_refresh(&pending).unwrap());
    assert_eq!(snapshot(&f), before);

    let expired = shift_expired(&f, &pending);
    let shifted_state = snapshot(&f);
    let shifted_registration = f
        .store()
        .load_registration(f.registration.registration_id)
        .unwrap()
        .unwrap();
    let credential_generation = f.credential_vault.lock().unwrap().generation().unwrap();
    assert!(f.store().expire_cli_scope_refresh(&expired).unwrap());
    let retained = f.store().load_cli_scope_refresh([71; 32]).unwrap().unwrap();
    assert_eq!(retained.phase(), ScopeRefreshPhase::Expired);
    assert_eq!(
        retained.target_profile_epoch(),
        f.registration.profile_epoch + 1
    );
    assert_eq!(snapshot(&f), shifted_state);
    {
        let vault = f.credential_vault.lock().unwrap();
        assert_eq!(vault.generation().unwrap(), credential_generation);
        assert!(vault.read("model-synthetic").is_ok());
    }
    assert_eq!(
        f.store().expire_cli_scope_refresh(&expired),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store().expire_cli_scope_refresh(&pending),
        Err(ErrorCode::Conflict)
    );

    let fresh = f
        .store()
        .create_terminal_cli_scope_refresh(
            [72; 32],
            RequestedScope::new(
                vec!["a".into(), "b".into()],
                hantong_contracts::auth::default_cli_folder_tools(),
            )
            .unwrap(),
            pin(),
        )
        .unwrap();
    assert_eq!(
        fresh.target_profile_epoch(),
        f.registration.profile_epoch + 1
    );
    assert_eq!(
        f.store()
            .load_registration(f.registration.registration_id)
            .unwrap()
            .unwrap(),
        shifted_registration
    );
    assert_eq!(snapshot(&f), shifted_state);
}
