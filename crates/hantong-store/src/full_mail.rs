//! Account-bound capability readiness. These observations never authorize access.
use crate::{
    Store,
    account_provisioning::ImapProvisioningState,
    imap_catalog::StoredImapMailboxCatalog,
    mail_connection::{MailConnectionState, MailEndpointKind, MailObservation},
};
use hantong_contracts::ErrorCode;
use hantong_domain::{
    accounts::AccountStatus,
    mail_connection::MailConnectionConfig,
    mailboxes::{ImapMailboxSnapshot, SpecialUseRole},
};
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use rusqlite::{OptionalExtension, TransactionBehavior, params};

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn db<T>(result: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    result.map_err(|_| ErrorCode::InternalError)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MissingMailFeature {
    Imap,
    Smtp,
    Catalog,
    Trash,
    Move,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FullMailReadiness {
    Incomplete(MissingMailFeature),
    Ready(FullMailBinding),
}

/// Public capability metadata only, never a grant or an approval bearer.
#[derive(Clone, Eq, PartialEq)]
pub struct FullMailBinding {
    account_id: String,
    account_version: u64,
    imap_settings_version: u64,
    smtp_settings_version: u64,
    catalog_generation: u64,
    trash_binding_hash: [u8; 32],
}
impl std::fmt::Debug for FullMailBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FullMailBinding { <redacted> }")
    }
}
impl FullMailBinding {
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn account_version(&self) -> u64 {
        self.account_version
    }
    pub fn imap_settings_version(&self) -> u64 {
        self.imap_settings_version
    }
    pub fn smtp_settings_version(&self) -> u64 {
        self.smtp_settings_version
    }
    pub fn catalog_generation(&self) -> u64 {
        self.catalog_generation
    }
    pub fn trash_binding_hash(&self) -> &[u8; 32] {
        &self.trash_binding_hash
    }
}

#[derive(Clone, Eq, PartialEq)]
struct Observed {
    imap: ImapProvisioningState,
    config: Option<MailConnectionConfig>,
    smtp: Option<MailObservation>,
    catalog: Option<StoredImapMailboxCatalog>,
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
impl Store {
    /// Persists a previously observed full-mail binding only after rechecking
    /// every account, endpoint, observation, and catalog generation in one
    /// immediate transaction. This cache is metadata, never authority.
    pub fn record_full_mail_readiness(&self, binding: &FullMailBinding) -> Result<(), ErrorCode> {
        crate::mail_connection::input_account(binding.account_id())?;
        if binding.account_version() == 0
            || binding.imap_settings_version() == 0
            || binding.smtp_settings_version() == 0
            || binding.catalog_generation() == 0
        {
            return Err(ErrorCode::InvalidInput);
        }
        self.with_connection(|connection| {
            let tx = rusqlite::Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
            let outcome: Result<(), ErrorCode> = (|| {
                let (status, account_version): (String, i64) = db(tx.query_row(
                    "SELECT status,settings_version FROM accounts WHERE account_id=?1",
                    [binding.account_id()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                ))?;
                if status != "ready" || account_version != binding.account_version() as i64 {
                    return Err(ErrorCode::Conflict);
                }
                for (kind, expected) in [
                    ("imap", binding.imap_settings_version()),
                    ("smtp", binding.smtp_settings_version()),
                ] {
                    let version: i64 = db(tx.query_row(
                        "SELECT settings_version FROM mail_connection_settings WHERE account_id=?1 AND endpoint_kind=?2",
                        params![binding.account_id(), kind],
                        |row| row.get(0),
                    ))?;
                    if version != expected as i64 {
                        return Err(ErrorCode::Conflict);
                    }
                    let observation: Option<(i64, i64, String)> = db(tx
                        .query_row(
                            "SELECT observed_settings_version,observation_version,state FROM mail_connection_observations WHERE account_id=?1 AND endpoint_kind=?2",
                            params![binding.account_id(), kind],
                            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                        )
                        .optional())?;
                    let Some((observed_settings, observation_version, state)) = observation else {
                        return Err(ErrorCode::Conflict);
                    };
                    if observed_settings != expected as i64
                        || observation_version <= 0
                        || state != "ready"
                    {
                        return Err(ErrorCode::Conflict);
                    }
                }
                let catalog: (i64, i64, i64, String, Vec<u8>, String) = db(tx.query_row(
                    "SELECT account_version,endpoint_version,generation,snapshot_json,snapshot_hash,last_operation_id FROM imap_mailbox_catalogs WHERE account_id=?1",
                    [binding.account_id()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
                ))?;
                if catalog.0 != binding.account_version() as i64
                    || catalog.1 != binding.imap_settings_version() as i64
                    || catalog.2 != binding.catalog_generation() as i64
                    || catalog.4.len() != 32
                    || hantong_platform::sha256(catalog.3.as_bytes())?.as_slice() != catalog.4.as_slice()
                {
                    return Err(ErrorCode::Conflict);
                }
                let snapshot = ImapMailboxSnapshot::from_storage_json(catalog.3.as_bytes())
                    .map_err(|_| ErrorCode::InternalError)?;
                if snapshot.account_id() != binding.account_id()
                    || snapshot.settings_version() != binding.imap_settings_version()
                {
                    return Err(ErrorCode::Conflict);
                }
                let trash_hash = match trash_binding(binding.account_id(), &snapshot)? {
                    Ok(hash) => hash,
                    Err(_) => return Err(ErrorCode::Conflict),
                };
                if trash_hash != *binding.trash_binding_hash() {
                    return Err(ErrorCode::Conflict);
                }
                let operation_exists: bool = db(tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM imap_mailbox_catalog_operations WHERE operation_id=?1 AND account_id=?2 AND account_version=?3 AND endpoint_version=?4 AND result_version=?5)",
                    params![catalog.5, binding.account_id(), catalog.0, catalog.1, catalog.2],
                    |row| row.get(0),
                ))?;
                if !operation_exists {
                    return Err(ErrorCode::Conflict);
                }
                let now = db(crate::utc_millis())?;
                db(tx.execute(
                    "INSERT INTO account_full_mail_readiness(account_id,account_version,imap_settings_version,smtp_settings_version,catalog_generation,trash_binding_hash,state,reason,observed_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?6,'full',NULL,?7) ON CONFLICT(account_id) DO UPDATE SET account_version=excluded.account_version,imap_settings_version=excluded.imap_settings_version,smtp_settings_version=excluded.smtp_settings_version,catalog_generation=excluded.catalog_generation,trash_binding_hash=excluded.trash_binding_hash,state='full',reason=NULL,observed_at_utc_ms=excluded.observed_at_utc_ms",
                    params![binding.account_id(), binding.account_version() as i64, binding.imap_settings_version() as i64, binding.smtp_settings_version() as i64, binding.catalog_generation() as i64, binding.trash_binding_hash().as_slice(), now],
                ))?;
                Ok(())
            })();
            if outcome.is_ok() {
                tx.commit()?;
            }
            Ok(outcome)
        })?
    }

    /// Rechecks current endpoint/observation/catalog bindings without network I/O.
    /// A caller must still authenticate, authorize and recheck every operation.
    pub fn observe_full_mail_readiness(
        &self,
        account: &str,
    ) -> Result<FullMailReadiness, ErrorCode> {
        let read = || -> Result<Observed, ErrorCode> {
            Ok(Observed {
                imap: self.load_imap_provisioning_state(account)?,
                config: self.load_mail_connection(account)?,
                smtp: self.load_mail_observation(account, MailEndpointKind::Smtp)?,
                catalog: self.load_imap_mailbox_snapshot(account)?,
            })
        };
        let before = read()?;
        let result = evaluate(account, &before)?;
        // The existing APIs each own their read transaction. Monotonic versions
        // and exact repeated observations reject a change between snapshots.
        if read()? != before {
            return Err(ErrorCode::Conflict);
        }
        Ok(result)
    }
}

fn ready_observation(observation: Option<&MailObservation>, version: u64) -> bool {
    observation.is_some_and(|value| {
        value.state == MailConnectionState::Ready
            && value.observed_settings_version == version
            && value.observation_version > 0
    })
}

fn evaluate(account: &str, state: &Observed) -> Result<FullMailReadiness, ErrorCode> {
    use FullMailReadiness::{Incomplete, Ready};
    use MissingMailFeature::{Catalog, Imap, Smtp};
    let imap = &state.imap;
    if imap.account_id != account {
        return Err(ErrorCode::ScopeDenied);
    }
    if imap.status != AccountStatus::Ready
        || !ready_observation(imap.observation.as_ref(), imap.endpoint.settings_version())
    {
        return Ok(Incomplete(Imap));
    }
    let Some(config) = &state.config else {
        return Ok(Incomplete(Imap));
    };
    if config.account_id() != account || config.imap() != &imap.endpoint {
        return Err(ErrorCode::Conflict);
    }
    let Some(smtp) = config.smtp() else {
        return Ok(Incomplete(Smtp));
    };
    if !ready_observation(state.smtp.as_ref(), smtp.settings_version()) {
        return Ok(Incomplete(Smtp));
    }
    let Some(catalog) = &state.catalog else {
        return Ok(Incomplete(Catalog));
    };
    if !catalog.binding_current()
        || catalog.account_version() != imap.account_version
        || catalog.endpoint_version() != imap.endpoint.settings_version()
        || catalog.snapshot().account_id() != account
    {
        return Ok(Incomplete(Catalog));
    }
    let destination = match trash_binding(account, catalog.snapshot())? {
        Ok(hash) => hash,
        Err(missing) => return Ok(Incomplete(missing)),
    };
    Ok(Ready(FullMailBinding {
        account_id: account.to_owned(),
        account_version: imap.account_version,
        imap_settings_version: imap.endpoint.settings_version(),
        smtp_settings_version: smtp.settings_version(),
        catalog_generation: catalog.generation(),
        trash_binding_hash: destination,
    }))
}

fn trash_binding(
    account: &str,
    snapshot: &ImapMailboxSnapshot,
) -> Result<Result<[u8; 32], MissingMailFeature>, ErrorCode> {
    if snapshot.account_id() != account {
        return Err(ErrorCode::ScopeDenied);
    }
    if !snapshot.capabilities().move_messages() {
        return Ok(Err(MissingMailFeature::Move));
    }
    let mut destinations = snapshot
        .mailboxes()
        .iter()
        .filter(|mailbox| mailbox.selectable() && mailbox.has_role(SpecialUseRole::Trash));
    let Some(destination) = destinations.next() else {
        return Ok(Err(MissingMailFeature::Trash));
    };
    if destinations.next().is_some() {
        return Ok(Err(MissingMailFeature::Trash));
    }
    let mut bytes = b"mail-hantong.full-mail-trash.v2\0".to_vec();
    for part in [account.as_bytes(), destination.name().wire_name()] {
        bytes.extend_from_slice(&(part.len() as u32).to_be_bytes());
        bytes.extend_from_slice(part);
    }
    bytes.push(match destination.name().encoding() {
        hantong_domain::mailboxes::MailboxEncoding::Utf8 => 1,
        hantong_domain::mailboxes::MailboxEncoding::ModifiedUtf7 => 2,
    });
    hantong_platform::sha256(&bytes).map(Ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    #[test]
    fn acc02_smtp_ready_must_match_current_settings_not_only_ready_state() {
        let observation = MailObservation {
            endpoint_kind: MailEndpointKind::Smtp,
            observed_settings_version: 3,
            observation_version: 1,
            state: MailConnectionState::Ready,
            observed_at_utc_ms: 1,
        };
        assert!(ready_observation(Some(&observation), 3));
        assert!(!ready_observation(Some(&observation), 4));
        assert!(!ready_observation(None, 3));
        let mut failed = observation;
        failed.state = MailConnectionState::AuthRequired;
        assert!(!ready_observation(Some(&failed), 3));
    }

    fn snapshot(names: &[&[u8]], supports_move: bool) -> ImapMailboxSnapshot {
        use hantong_domain::{
            capabilities::{ImapCapabilities, ImapFeature},
            mailboxes::{ImapMailboxDescriptor, ImapMailboxName, MailboxChildren, MailboxEncoding},
        };
        let features = if supports_move {
            vec![ImapFeature::Move]
        } else {
            vec![]
        };
        ImapMailboxSnapshot::new(
            "acct",
            1,
            ImapCapabilities::from_features(features),
            names
                .iter()
                .map(|name| {
                    ImapMailboxDescriptor::new(
                        ImapMailboxName::from_wire(name, MailboxEncoding::Utf8).unwrap(),
                        Some('/'),
                        false,
                        false,
                        MailboxChildren::HasNoChildren,
                        vec![SpecialUseRole::Trash],
                    )
                    .unwrap()
                })
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn acc02_full_mail_requires_move_and_one_exact_trash_destination() {
        assert_eq!(
            trash_binding("acct", &snapshot(&[], true)).unwrap(),
            Err(MissingMailFeature::Trash)
        );
        assert_eq!(
            trash_binding("acct", &snapshot(&[b"Trash", b"Deleted"], true)).unwrap(),
            Err(MissingMailFeature::Trash)
        );
        assert_eq!(
            trash_binding("acct", &snapshot(&[b"Trash"], false)).unwrap(),
            Err(MissingMailFeature::Move)
        );
        let first = trash_binding("acct", &snapshot(&[b"Trash"], true))
            .unwrap()
            .unwrap();
        let changed = trash_binding("acct", &snapshot(&[b"Deleted"], true))
            .unwrap()
            .unwrap();
        assert_ne!(first, changed);
        assert_eq!(
            trash_binding("other", &snapshot(&[b"Trash"], true)),
            Err(ErrorCode::ScopeDenied)
        );
    }

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    #[test]
    fn acc02_record_full_mail_readiness_rechecks_all_bindings_atomically() {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        crate::test_support::ensure_test_backend();
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-full-mail-readiness-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        // macOS temp_dir may contain /var, a symlink to /private/var.
        // Use the physical isolated root, as required by the product guard.
        let root = fs::canonicalize(root).unwrap();
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        let snapshot = {
            use hantong_domain::{
                capabilities::{ImapCapabilities, ImapFeature},
                mailboxes::{
                    ImapMailboxDescriptor, ImapMailboxName, MailboxChildren, MailboxEncoding,
                },
            };
            ImapMailboxSnapshot::new(
                "acct",
                2,
                ImapCapabilities::from_features(vec![ImapFeature::Move]),
                vec![
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
            .unwrap()
        };
        let bytes = snapshot.to_storage_json().unwrap();
        let snapshot_hash = hantong_platform::sha256(&bytes).unwrap();
        let trash_hash = trash_binding("acct", &snapshot).unwrap().unwrap();
        store.with_connection(|c| {
            c.execute("INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES('acct','imap',1,'ready','[]','[]',3)", [])?;
            c.execute("INSERT INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES('acct','imap',2,'imap.example',993,'implicit_tls','acct','app_password',1),('acct','smtp',4,'smtp.example',465,'implicit_tls','acct','app_password',1)", [])?;
            c.execute("INSERT INTO mail_connection_observations(account_id,endpoint_kind,observed_settings_version,observation_version,state,observed_at_utc_ms) VALUES('acct','imap',2,1,'ready',1),('acct','smtp',4,1,'ready',1)", [])?;
            c.execute("INSERT INTO imap_mailbox_catalog_operations(operation_id,account_id,idempotency_key,account_version,endpoint_version,endpoint_hash,snapshot_hash,expected_version,result_version,payload_hash,completed_at_utc_ms) VALUES('seed','acct','seed',3,2,zeroblob(32),?1,0,1,zeroblob(32),1)", [snapshot_hash.as_slice()])?;
            c.execute("INSERT INTO imap_mailbox_catalogs(account_id,account_version,endpoint_version,endpoint_hash,generation,snapshot_json,snapshot_hash,last_operation_id,stored_at_utc_ms) VALUES('acct',3,2,zeroblob(32),1,?1,?2,'seed',1)", rusqlite::params![String::from_utf8(bytes).unwrap(), snapshot_hash.as_slice()])?;
            Ok(())
        }).unwrap();
        let binding = FullMailBinding {
            account_id: "acct".into(),
            account_version: 3,
            imap_settings_version: 2,
            smtp_settings_version: 4,
            catalog_generation: 1,
            trash_binding_hash: trash_hash,
        };
        store.record_full_mail_readiness(&binding).unwrap();
        for stale in [
            FullMailBinding {
                account_version: 2,
                ..binding.clone()
            },
            FullMailBinding {
                imap_settings_version: 3,
                ..binding.clone()
            },
            FullMailBinding {
                smtp_settings_version: 5,
                ..binding.clone()
            },
            FullMailBinding {
                catalog_generation: 2,
                ..binding.clone()
            },
            FullMailBinding {
                trash_binding_hash: [0xA5; 32],
                ..binding.clone()
            },
        ] {
            assert_eq!(
                store.record_full_mail_readiness(&stale),
                Err(ErrorCode::Conflict)
            );
        }
        store
            .with_connection(|c| {
                assert_eq!(
                    c.query_row(
                        "SELECT state FROM account_full_mail_readiness WHERE account_id='acct'",
                        [],
                        |r| r.get::<_, String>(0)
                    )?,
                    "full"
                );
                assert_eq!(
                    c.query_row(
                        "SELECT account_version,imap_settings_version,smtp_settings_version,catalog_generation FROM account_full_mail_readiness WHERE account_id='acct'",
                        [],
                        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)),
                    )?,
                    (3, 2, 4, 1)
                );
                assert_eq!(
                    c.query_row(
                        "SELECT (SELECT COUNT(*) FROM account_scope_v2) + (SELECT COUNT(*) FROM account_scope_decisions_v2)",
                        [],
                        |r| r.get::<_, i64>(0),
                    )?,
                    0
                );
                Ok(())
            })
            .unwrap();
        drop(store);
        let _ = fs::remove_dir_all(root);
    }
}
