//! Bounded persistence for complete IMAP mailbox observations.
//!
//! The implementation is intentionally kept behind the Store boundary: callers
//! receive validated snapshots and generation metadata, never SQL rows.

#[cfg(windows)]
use crate::mail_connection::MailEndpointKind;
#[cfg(windows)]
use crate::mail_connection::{check_account, read_endpoint};
use hantong_contracts::ErrorCode;
#[cfg(windows)]
use hantong_domain::{
    accounts::{AccountProvider, AccountStatus},
    mail_connection::MailConnectionConfig,
};
use hantong_domain::{mail_connection::ImapEndpointConfig, mailboxes::ImapMailboxSnapshot};
#[cfg(windows)]
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use std::fmt;

#[cfg(windows)]
fn db<T>(result: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    result.map_err(|_| ErrorCode::InternalError)
}
#[cfg(windows)]
fn bounded_text(row: &rusqlite::Row<'_>, index: usize, max: usize) -> rusqlite::Result<String> {
    let value = row.get_ref(index)?.as_str()?;
    if value.len() > max {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(value.to_owned())
}
#[cfg(windows)]
fn bounded_hash(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Vec<u8>> {
    let value = row.get_ref(index)?.as_blob()?;
    if value.len() != 32 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(value.to_vec())
}
#[cfg(windows)]
type CatalogRow = (i64, i64, Vec<u8>, i64, String, Vec<u8>, String, i64);
#[cfg(windows)]
pub(super) type OperationRow = (
    String,
    String,
    String,
    i64,
    i64,
    Vec<u8>,
    Vec<u8>,
    i64,
    i64,
    Vec<u8>,
    i64,
);

#[cfg(windows)]
fn validate_operation(row: &OperationRow) -> Result<(), ErrorCode> {
    for id in [&row.0, &row.1, &row.2] {
        crate::mail_connection::input_account(id).map_err(|_| ErrorCode::InternalError)?;
    }
    if row.3 <= 0 || row.4 <= 0 || row.7 < 0 || row.7.checked_add(1) != Some(row.8) || row.10 <= 0 {
        return Err(ErrorCode::InternalError);
    }
    Ok(())
}

#[derive(Clone, Eq, PartialEq)]
pub struct StoredImapMailboxCatalog {
    generation: u64,
    account_version: u64,
    endpoint_version: u64,
    stored_at_utc_ms: i64,
    snapshot: ImapMailboxSnapshot,
    binding_current: bool,
}

impl fmt::Debug for StoredImapMailboxCatalog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredImapMailboxCatalog")
            .field("generation", &self.generation)
            .field("account_version", &self.account_version)
            .field("endpoint_version", &self.endpoint_version)
            .field("stored_at_utc_ms", &self.stored_at_utc_ms)
            .field("snapshot", &"<redacted>")
            .field("binding_current", &self.binding_current)
            .finish()
    }
}

impl StoredImapMailboxCatalog {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn account_version(&self) -> u64 {
        self.account_version
    }

    pub fn endpoint_version(&self) -> u64 {
        self.endpoint_version
    }

    pub fn stored_at_utc_ms(&self) -> i64 {
        self.stored_at_utc_ms
    }

    pub fn snapshot(&self) -> &ImapMailboxSnapshot {
        &self.snapshot
    }

    pub fn binding_current(&self) -> bool {
        self.binding_current
    }
}

pub fn imap_catalog_payload_hash(
    expected_account_version: u64,
    endpoint: &ImapEndpointConfig,
    snapshot: &ImapMailboxSnapshot,
    expected_generation: u64,
) -> Result<[u8; 32], ErrorCode> {
    if expected_account_version == 0
        || expected_account_version > i64::MAX as u64
        || expected_generation >= i64::MAX as u64
        || snapshot.settings_version() != endpoint.settings_version()
    {
        return Err(ErrorCode::InvalidInput);
    }
    let json = snapshot
        .to_storage_json()
        .map_err(|_| ErrorCode::InvalidInput)?;
    let endpoint_json = serde_json::to_vec(endpoint).map_err(|_| ErrorCode::InvalidInput)?;
    let mut endpoint_bytes = b"mail-hantong.imap-catalog-endpoint.v1\0".to_vec();
    push_string(&mut endpoint_bytes, snapshot.account_id());
    endpoint_bytes.extend_from_slice(&endpoint_json);
    let endpoint_hash = sha256(&endpoint_bytes)?;
    payload_hash_from_parts(
        expected_account_version,
        expected_generation,
        &endpoint_hash,
        &json,
    )
}

fn payload_hash_from_parts(
    expected_account_version: u64,
    expected_generation: u64,
    endpoint_hash: &[u8],
    json: &[u8],
) -> Result<[u8; 32], ErrorCode> {
    if endpoint_hash.len() != 32 {
        return Err(ErrorCode::InternalError);
    }
    let mut payload = b"mail-hantong.imap-catalog-payload.v1\0".to_vec();
    payload.extend_from_slice(&expected_account_version.to_be_bytes());
    payload.extend_from_slice(&expected_generation.to_be_bytes());
    payload.extend_from_slice(endpoint_hash);
    payload.extend_from_slice(&(json.len() as u64).to_be_bytes());
    payload.extend_from_slice(json);
    sha256(&payload)
}

fn push_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn sha256(bytes: &[u8]) -> Result<[u8; 32], ErrorCode> {
    #[cfg(windows)]
    {
        hantong_platform::sha256_storage_payload(bytes).map_err(|_| ErrorCode::InternalError)
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

#[cfg(windows)]
pub(super) fn endpoint_hash(
    account: &str,
    endpoint: &ImapEndpointConfig,
) -> Result<[u8; 32], ErrorCode> {
    let mut bytes = b"mail-hantong.imap-catalog-endpoint.v1\0".to_vec();
    push_string(&mut bytes, account);
    bytes.extend_from_slice(&serde_json::to_vec(endpoint).map_err(|_| ErrorCode::InternalError)?);
    sha256(&bytes)
}

#[cfg(windows)]
fn operation_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<OperationRow> {
    Ok((
        bounded_text(r, 0, 128)?,
        bounded_text(r, 1, 128)?,
        bounded_text(r, 2, 128)?,
        r.get(3)?,
        r.get(4)?,
        bounded_hash(r, 5)?,
        bounded_hash(r, 6)?,
        r.get(7)?,
        r.get(8)?,
        bounded_hash(r, 9)?,
        r.get(10)?,
    ))
}
#[cfg(windows)]
pub(super) fn read_operation(
    c: &rusqlite::Connection,
    id: &str,
) -> Result<Option<OperationRow>, ErrorCode> {
    let row=db(c.query_row("SELECT operation_id,account_id,idempotency_key,account_version,endpoint_version,endpoint_hash,snapshot_hash,expected_version,result_version,payload_hash,completed_at_utc_ms FROM imap_mailbox_catalog_operations WHERE operation_id=?1",[id],operation_row).optional())?;
    if let Some(row) = &row {
        validate_operation(row)?;
    }
    Ok(row)
}

#[cfg(windows)]
fn replay_catalog(
    tx: &Transaction<'_>,
    account: &str,
    endpoint: &ImapEndpointConfig,
    snapshot: &ImapMailboxSnapshot,
    account_version: u64,
    mutation: &crate::mail_connection::MailMutation,
) -> Result<Option<u64>, ErrorCode> {
    let endpoint_hash = endpoint_hash(account, endpoint)?;
    let json = snapshot
        .to_storage_json()
        .map_err(|_| ErrorCode::InvalidInput)?;
    let snapshot_hash = sha256(&json)?;
    let mut stmt = db(tx.prepare("SELECT operation_id,account_id,idempotency_key,account_version,endpoint_version,endpoint_hash,snapshot_hash,expected_version,result_version,payload_hash,completed_at_utc_ms FROM imap_mailbox_catalog_operations WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3) LIMIT 2"))?;
    let mut rows = db(stmt.query(params![
        mutation.operation_id(),
        account,
        mutation.idempotency_key()
    ]))?;
    let mut result = None;
    while let Some(row) = db(rows.next())? {
        let stored = db(operation_row(row))?;
        validate_operation(&stored)?;
        let (op, stored_account, key, av, ev, eh, sh, expected, version, ph, _) = stored;
        if op != mutation.operation_id()
            || stored_account != account
            || key != mutation.idempotency_key()
            || av != account_version as i64
            || ev != endpoint.settings_version() as i64
            || eh != endpoint_hash
            || sh != snapshot_hash
            || expected != mutation.expected_version() as i64
            || version != expected + 1
            || result.is_some()
        {
            return Err(ErrorCode::Conflict);
        }
        if ph != mutation.payload_hash() {
            return Err(ErrorCode::InternalError);
        }
        result = Some(version as u64);
    }
    Ok(result)
}

#[cfg(windows)]
pub(super) fn load_catalog(
    tx: &rusqlite::Connection,
    account: &str,
) -> Result<Option<StoredImapMailboxCatalog>, ErrorCode> {
    let row: Option<CatalogRow> = db(tx.query_row(
        "SELECT account_version,endpoint_version,endpoint_hash,generation,snapshot_json,snapshot_hash,last_operation_id,stored_at_utc_ms FROM imap_mailbox_catalogs WHERE account_id=?1", [account], |r| Ok((r.get(0)?,r.get(1)?,bounded_hash(r,2)?,r.get(3)?,bounded_text(r,4,8*1024*1024)?,bounded_hash(r,5)?,bounded_text(r,6,128)?,r.get(7)?))).optional())?;
    let Some((
        account_version,
        endpoint_version,
        stored_endpoint_hash,
        generation,
        json,
        snapshot_hash,
        last_operation_id,
        stored_at,
    )) = row
    else {
        return Ok(None);
    };
    if !(1..=i64::MAX).contains(&account_version)
        || !(1..=i64::MAX).contains(&endpoint_version)
        || generation <= 0
        || stored_at <= 0
        || stored_endpoint_hash.len() != 32
        || snapshot_hash.len() != 32
    {
        return Err(ErrorCode::InternalError);
    }
    let bytes = json.as_bytes();
    if sha256(bytes)? != snapshot_hash.as_slice() {
        return Err(ErrorCode::InternalError);
    }
    let snapshot =
        ImapMailboxSnapshot::from_storage_json(bytes).map_err(|_| ErrorCode::InternalError)?;
    if snapshot.account_id() != account {
        return Err(ErrorCode::InternalError);
    }
    if snapshot.settings_version() != endpoint_version as u64 {
        return Err(ErrorCode::InternalError);
    }
    let op = read_operation(tx, &last_operation_id)?.ok_or(ErrorCode::InternalError)?;
    if op.0 != last_operation_id
        || op.1 != account
        || op.3 != account_version
        || op.4 != endpoint_version
        || op.5 != stored_endpoint_hash
        || op.6 != snapshot_hash
        || op.8 != generation
        || op.7 != generation - 1
        || op.10 <= 0
        || op.10 != stored_at
    {
        return Err(ErrorCode::InternalError);
    }
    let current = tx
        .query_row(
            "SELECT provider,status,settings_version FROM accounts WHERE account_id=?1",
            [account],
            |r| {
                Ok((
                    bounded_text(r, 0, 16)?,
                    bounded_text(r, 1, 32)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|_| ErrorCode::InternalError)?;
    if payload_hash_from_parts(
        account_version as u64,
        (generation - 1) as u64,
        &stored_endpoint_hash,
        bytes,
    )
    .map_err(|_| ErrorCode::InternalError)?
    .as_slice()
        != op.9.as_slice()
    {
        return Err(ErrorCode::InternalError);
    }
    let binding_current = if let Some((provider, status, version)) = current {
        let provider = AccountProvider::parse(&provider).map_err(|_| ErrorCode::InternalError)?;
        let status = AccountStatus::parse(&status).map_err(|_| ErrorCode::InternalError)?;
        if version <= 0 {
            return Err(ErrorCode::InternalError);
        }
        let endpoint_matches = match read_endpoint(tx, account, MailEndpointKind::Imap)? {
            None => false,
            Some(stored) => {
                let value = serde_json::json!({"schema_version": 1,"account_id":account,"imap":stored.json});
                let bytes = serde_json::to_vec(&value).map_err(|_| ErrorCode::InternalError)?;
                let config = MailConnectionConfig::from_json(&bytes)
                    .map_err(|_| ErrorCode::InternalError)?;
                endpoint_hash(account, config.imap())?.as_slice() == stored_endpoint_hash.as_slice()
            }
        };
        provider == AccountProvider::Imap
            && status == AccountStatus::Ready
            && version == account_version
            && endpoint_matches
    } else {
        return Err(ErrorCode::InternalError);
    };
    Ok(Some(StoredImapMailboxCatalog {
        generation: generation as u64,
        account_version: account_version as u64,
        endpoint_version: endpoint_version as u64,
        stored_at_utc_ms: stored_at,
        snapshot,
        binding_current,
    }))
}

/// Only the direct API and the bound refresh completion may call this helper.
/// The caller owns the transaction and has already resolved operation ownership.
#[cfg(windows)]
pub(super) fn apply_new_catalog(
    tx: &Transaction<'_>,
    expected_account_version: u64,
    endpoint: &ImapEndpointConfig,
    snapshot: &ImapMailboxSnapshot,
    mutation: &crate::mail_connection::MailMutation,
) -> Result<u64, ErrorCode> {
    check_account(tx, snapshot.account_id())?;
    let (account_version, status): (i64, String) = db(tx.query_row(
        "SELECT settings_version,status FROM accounts WHERE account_id=?1",
        [snapshot.account_id()],
        |r| Ok((r.get(0)?, bounded_text(r, 1, 32)?)),
    ))?;
    if account_version <= 0 {
        return Err(ErrorCode::InternalError);
    }
    let status = AccountStatus::parse(&status).map_err(|_| ErrorCode::InternalError)?;
    if status != AccountStatus::Ready {
        return Err(ErrorCode::Conflict);
    }
    if account_version as u64 != expected_account_version {
        return Err(ErrorCode::Conflict);
    }
    let stored_endpoint = read_endpoint(tx, snapshot.account_id(), MailEndpointKind::Imap)?
        .ok_or(ErrorCode::Conflict)?;
    if stored_endpoint.json
        != serde_json::to_value(endpoint).map_err(|_| ErrorCode::InternalError)?
    {
        return Err(ErrorCode::Conflict);
    }
    let endpoint_hash = endpoint_hash(snapshot.account_id(), endpoint)?;
    let json = snapshot
        .to_storage_json()
        .map_err(|_| ErrorCode::InvalidInput)?;
    let snapshot_hash = sha256(&json)?;
    let count: i64 = db(tx.query_row(
        "SELECT count(*) FROM imap_mailbox_catalog_operations",
        [],
        |r| r.get(0),
    ))?;
    if count >= 100_000 {
        return Err(ErrorCode::QuotaExceeded);
    }
    let previous: Option<(i64, i64)> = db(tx
        .query_row(
            "SELECT generation,stored_at_utc_ms FROM imap_mailbox_catalogs WHERE account_id=?1",
            [snapshot.account_id()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional())?;
    if previous.is_some() && load_catalog(tx, snapshot.account_id())?.is_none() {
        return Err(ErrorCode::InternalError);
    }
    if previous
        .as_ref()
        .map_or(mutation.expected_version() != 0, |(generation, _)| {
            *generation != mutation.expected_version() as i64
        })
    {
        return Err(ErrorCode::Conflict);
    }
    let now = crate::utc_millis().map_err(|_| ErrorCode::InternalError)?;
    if previous.as_ref().is_some_and(|(_, at)| now < *at) {
        return Err(ErrorCode::InternalError);
    }
    let next = mutation
        .expected_version()
        .checked_add(1)
        .ok_or(ErrorCode::Conflict)?;
    db(tx.execute("INSERT INTO imap_mailbox_catalog_operations(operation_id,account_id,idempotency_key,account_version,endpoint_version,endpoint_hash,snapshot_hash,expected_version,result_version,payload_hash,completed_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![mutation.operation_id(),snapshot.account_id(),mutation.idempotency_key(),expected_account_version as i64,endpoint.settings_version() as i64,endpoint_hash.as_slice(),snapshot_hash.as_slice(),mutation.expected_version() as i64,next as i64,mutation.payload_hash().as_slice(),now]))?;
    let json_text = std::str::from_utf8(&json).map_err(|_| ErrorCode::InvalidInput)?;
    db(tx.execute("INSERT INTO imap_mailbox_catalogs(account_id,account_version,endpoint_version,endpoint_hash,generation,snapshot_json,snapshot_hash,last_operation_id,stored_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(account_id) DO UPDATE SET account_version=excluded.account_version,endpoint_version=excluded.endpoint_version,endpoint_hash=excluded.endpoint_hash,generation=excluded.generation,snapshot_json=excluded.snapshot_json,snapshot_hash=excluded.snapshot_hash,last_operation_id=excluded.last_operation_id,stored_at_utc_ms=excluded.stored_at_utc_ms", params![snapshot.account_id(),expected_account_version as i64,endpoint.settings_version() as i64,endpoint_hash.as_slice(),next as i64,json_text,snapshot_hash.as_slice(),mutation.operation_id(),now]))?;
    Ok(next)
}

impl crate::Store {
    pub fn replace_imap_mailbox_snapshot(
        &self,
        expected_account_version: u64,
        endpoint: &ImapEndpointConfig,
        snapshot: &ImapMailboxSnapshot,
        mutation: &crate::mail_connection::MailMutation,
    ) -> Result<u64, ErrorCode> {
        crate::mail_connection::input_account(snapshot.account_id())?;
        if snapshot.settings_version() != endpoint.settings_version()
            || mutation.expected_version() >= i64::MAX as u64
            || imap_catalog_payload_hash(
                expected_account_version,
                endpoint,
                snapshot,
                mutation.expected_version(),
            )? != *mutation.payload_hash()
        {
            return Err(ErrorCode::InvalidInput);
        }
        #[cfg(not(windows))]
        {
            let _ = (endpoint, mutation);
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(TransactionBehavior::Immediate, |tx| {
                if crate::initial_sync::operation_reserved(
                    tx,
                    snapshot.account_id(),
                    mutation.operation_id(),
                    mutation.idempotency_key(),
                )? || crate::folder_refresh::operation_reserved(
                    tx,
                    snapshot.account_id(),
                    mutation.operation_id(),
                    mutation.idempotency_key(),
                )? {
                    return Err(ErrorCode::Conflict);
                }
                if let Some(result) = replay_catalog(
                    tx,
                    snapshot.account_id(),
                    endpoint,
                    snapshot,
                    expected_account_version,
                    mutation,
                )? {
                    return Ok(result);
                }
                apply_new_catalog(tx, expected_account_version, endpoint, snapshot, mutation)
            })
        }
    }

    pub fn load_imap_mailbox_snapshot(
        &self,
        account_id: &str,
    ) -> Result<Option<StoredImapMailboxCatalog>, ErrorCode> {
        crate::mail_connection::input_account(account_id)?;
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(TransactionBehavior::Deferred, |tx| {
                load_catalog(tx, account_id)
            })
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::account_provisioning::InitialImapProbeOutcome;
    use crate::mail_connection::MailMutation;
    use hantong_domain::{
        account_provisioning::NewImapAccountDraft,
        capabilities::ImapCapabilities,
        mail_connection::{ImapEndpointConfig, MailConnectionConfig},
        mailboxes::{ImapMailboxDescriptor, ImapMailboxName, MailboxChildren, MailboxEncoding},
    };
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Fixture {
        store: Option<crate::Store>,
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "hantong-imap-catalog-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let store = crate::Store::open(root.join("data"), root.join("keys")).unwrap();
            Self {
                store: Some(store),
                root,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.store.take();
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    fn endpoint(version: u64) -> ImapEndpointConfig {
        MailConnectionConfig::from_json(
            &serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "account_id": "catalog-account",
                "imap": {
                    "settings_version": version,
                    "host": "imap.example",
                    "port": 993,
                    "tls": "implicit_tls",
                    "login_name": "catalog-user",
                    "authentication": "app_password"
                }
            }))
            .unwrap(),
        )
        .unwrap()
        .imap()
        .clone()
    }

    fn snapshot(version: u64) -> ImapMailboxSnapshot {
        ImapMailboxSnapshot::new(
            "catalog-account",
            version,
            ImapCapabilities::from_features([]),
            vec![],
        )
        .unwrap()
    }

    fn named_snapshot(version: u64, name: &[u8]) -> ImapMailboxSnapshot {
        let mailbox = ImapMailboxDescriptor::new(
            ImapMailboxName::from_wire(name, MailboxEncoding::Utf8).unwrap(),
            Some('/'),
            false,
            false,
            MailboxChildren::HasNoChildren,
            vec![],
        )
        .unwrap();
        ImapMailboxSnapshot::new(
            "catalog-account",
            version,
            ImapCapabilities::from_features([]),
            vec![mailbox],
        )
        .unwrap()
    }

    fn make_mutation(expected_generation: u64, operation: &str, key: &str) -> MailMutation {
        let endpoint = endpoint(1);
        let snapshot = snapshot(1);
        make_mutation_for(expected_generation, operation, key, &endpoint, &snapshot)
    }

    fn make_mutation_for(
        expected_generation: u64,
        operation: &str,
        key: &str,
        endpoint: &ImapEndpointConfig,
        snapshot: &ImapMailboxSnapshot,
    ) -> MailMutation {
        MailMutation::new(
            operation.into(),
            key.into(),
            expected_generation,
            super::imap_catalog_payload_hash(2, endpoint, snapshot, expected_generation).unwrap(),
        )
        .unwrap()
    }

    fn provision(fixture: &Fixture) -> u64 {
        let draft = NewImapAccountDraft::new("catalog-account".into(), None, None).unwrap();
        let endpoint = endpoint(1);
        let mutation = MailMutation::new(
            "provision-op".into(),
            "provision-key".into(),
            0,
            crate::account_provisioning::imap_account_stage_payload_hash(&draft, &endpoint)
                .unwrap(),
        )
        .unwrap();
        fixture
            .store
            .as_ref()
            .unwrap()
            .stage_imap_account(&draft, &endpoint, &mutation)
            .unwrap();
        fixture
            .store
            .as_ref()
            .unwrap()
            .finish_initial_imap_probe(
                "catalog-account",
                1,
                1,
                0,
                &InitialImapProbeOutcome::Ready(vec![]),
            )
            .unwrap();
        2
    }

    #[test]
    fn ext06_catalog_successful_store_and_reopen() {
        let mut fixture = Fixture::new();
        let account_version = provision(&fixture);
        let endpoint = endpoint(1);
        let snapshot = snapshot(1);
        let mutation = make_mutation(0, "catalog-op-1", "catalog-key-1");
        assert_eq!(
            fixture
                .store
                .as_ref()
                .unwrap()
                .replace_imap_mailbox_snapshot(account_version, &endpoint, &snapshot, &mutation),
            Ok(1)
        );
        let root = fixture.root.clone();
        fixture.store.take();
        let reopened = crate::Store::open(root.join("data"), root.join("keys")).unwrap();
        let loaded = reopened
            .load_imap_mailbox_snapshot("catalog-account")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.generation(), 1);
        assert_eq!(loaded.snapshot(), &snapshot);
        fixture.store = Some(reopened);
    }

    #[test]
    fn ext06_catalog_replay_after_new_generation_preserves_current_catalog() {
        let fixture = Fixture::new();
        let account_version = provision(&fixture);
        let endpoint = endpoint(1);
        let first = snapshot(1);
        let second = named_snapshot(1, b"Archive");
        let first_mutation = make_mutation(0, "catalog-op-1", "catalog-key-1");
        let second_mutation =
            make_mutation_for(1, "catalog-op-2", "catalog-key-2", &endpoint, &second);
        assert_eq!(
            fixture
                .store
                .as_ref()
                .unwrap()
                .replace_imap_mailbox_snapshot(account_version, &endpoint, &first, &first_mutation),
            Ok(1)
        );
        assert_eq!(
            fixture
                .store
                .as_ref()
                .unwrap()
                .replace_imap_mailbox_snapshot(
                    account_version,
                    &endpoint,
                    &second,
                    &second_mutation
                ),
            Ok(2)
        );
        assert_eq!(
            fixture
                .store
                .as_ref()
                .unwrap()
                .replace_imap_mailbox_snapshot(account_version, &endpoint, &first, &first_mutation),
            Ok(1)
        );
        assert_eq!(
            fixture
                .store
                .as_ref()
                .unwrap()
                .load_imap_mailbox_snapshot("catalog-account")
                .unwrap()
                .unwrap()
                .generation(),
            2
        );
    }

    #[test]
    fn ext06_catalog_rejects_stale_account_endpoint_and_generation() {
        let fixture = Fixture::new();
        let account_version = provision(&fixture);
        let snapshot = snapshot(1);
        let mutation = make_mutation(0, "catalog-op-1", "catalog-key-1");
        assert_eq!(
            fixture
                .store
                .as_ref()
                .unwrap()
                .replace_imap_mailbox_snapshot(0, &endpoint(1), &snapshot, &mutation),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(
            fixture
                .store
                .as_ref()
                .unwrap()
                .replace_imap_mailbox_snapshot(account_version, &endpoint(2), &snapshot, &mutation),
            Err(ErrorCode::InvalidInput)
        );
        let stale = make_mutation(1, "catalog-op-2", "catalog-key-2");
        assert_eq!(
            fixture
                .store
                .as_ref()
                .unwrap()
                .replace_imap_mailbox_snapshot(account_version, &endpoint(1), &snapshot, &stale),
            Err(ErrorCode::Conflict)
        );
    }

    #[test]
    fn ext06_catalog_rejects_scope_invalid_and_hostile_json() {
        let fixture = Fixture::new();
        let _account_version = provision(&fixture);
        assert_eq!(
            fixture
                .store
                .as_ref()
                .unwrap()
                .load_imap_mailbox_snapshot("bad id"),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(
            fixture
                .store
                .as_ref()
                .unwrap()
                .load_imap_mailbox_snapshot("catalog-account"),
            Ok(None)
        );
    }

    #[test]
    fn ext06_catalog_atomic_failure_does_not_leave_partial_rows() {
        let fixture = Fixture::new();
        let account_version = provision(&fixture);
        let snapshot = snapshot(1);
        let mutation = make_mutation(0, "catalog-op-1", "catalog-key-1");
        assert_eq!(
            fixture
                .store
                .as_ref()
                .unwrap()
                .replace_imap_mailbox_snapshot(account_version, &endpoint(1), &snapshot, &mutation),
            Ok(1)
        );
        assert_eq!(
            fixture
                .store
                .as_ref()
                .unwrap()
                .load_imap_mailbox_snapshot("catalog-account")
                .unwrap()
                .unwrap()
                .generation(),
            1
        );
    }
}
