use super::*;
use crate::{
    ClientKind, DEFAULT_CLI_LABEL, RegistrationRequestState, RegistrationState,
    registrations::{blob_column, read_registration_bare, read_request, text_column},
};
use hantong_contracts::auth::{RequestedScope, is_supported_default_cli_toolset_transition};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use std::collections::BTreeMap;

const LIMIT: usize = 1024;
const TTL: i64 = 300000;
pub(super) struct Context {
    pub(super) registration: RegistrationRecord,
    rows: Vec<CliScopeRefresh>,
    updated: i64,
}
fn db<T>(value: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    value.map_err(|_| ErrorCode::InternalError)
}
fn invalid<T>() -> rusqlite::Result<T> {
    Err(rusqlite::Error::InvalidQuery)
}
fn phase_text(phase: ScopeRefreshPhase) -> &'static str {
    match phase {
        ScopeRefreshPhase::Pending => "pending",
        ScopeRefreshPhase::Staged => "staged",
        ScopeRefreshPhase::Active => "active",
        ScopeRefreshPhase::Denied => "denied",
        ScopeRefreshPhase::Expired => "expired",
    }
}
fn phase_parse(value: &str) -> rusqlite::Result<ScopeRefreshPhase> {
    match value {
        "pending" => Ok(ScopeRefreshPhase::Pending),
        "staged" => Ok(ScopeRefreshPhase::Staged),
        "active" => Ok(ScopeRefreshPhase::Active),
        "denied" => Ok(ScopeRefreshPhase::Denied),
        "expired" => Ok(ScopeRefreshPhase::Expired),
        _ => invalid(),
    }
}
fn hash_payload(value: &CliScopeRefresh) -> Result<[u8; 32], ErrorCode> {
    let base = &value.base_registration;
    let mut bytes = match value.hash_version {
        1 => b"mail-hantong.cli-scope-refresh.v1\0".to_vec(),
        2 => b"mail-hantong.cli-scope-refresh.v2\0".to_vec(),
        3 => b"mail-hantong.cli-scope-refresh.v3\0".to_vec(),
        4 => b"mail-hantong.cli-scope-refresh.v4\0".to_vec(),
        5 => b"mail-hantong.cli-scope-refresh.v5\0".to_vec(),
        6 => b"mail-hantong.cli-scope-refresh.v6\0".to_vec(),
        _ => return Err(ErrorCode::InternalError),
    };
    bytes.extend_from_slice(&value.refresh_id);
    bytes.extend_from_slice(&base.registration_id);
    bytes.extend_from_slice(&base.request_id);
    bytes.extend_from_slice(&base.version.to_be_bytes());
    bytes.extend_from_slice(&base.profile_epoch.to_be_bytes());
    bytes.extend_from_slice(&value.client_public_key_hash);
    bytes.extend_from_slice(&value.server_public_key_hash);
    bytes.extend_from_slice(&value.created_at_utc_ms.to_be_bytes());
    bytes.extend_from_slice(&value.expires_at_utc_ms.to_be_bytes());
    for accounts in [base.scope.accounts(), value.target_accounts.as_slice()] {
        bytes.extend_from_slice(&(accounts.len() as u32).to_be_bytes());
        for account in accounts {
            bytes.extend_from_slice(&(account.len() as u32).to_be_bytes());
            bytes.extend_from_slice(account.as_bytes());
        }
    }
    if value.hash_version >= 2 {
        for tools in [base.scope.tools(), value.target_tools.as_slice()] {
            bytes.extend_from_slice(&(tools.len() as u32).to_be_bytes());
            for tool in tools {
                bytes.extend_from_slice(&(tool.len() as u32).to_be_bytes());
                bytes.extend_from_slice(tool.as_bytes());
            }
        }
    }
    hantong_platform::sha256(&bytes).map_err(|_| ErrorCode::InternalError)
}
// Frozen historical ranks must not inherit future current-toolset changes.
fn historical_rank(tools: &[String]) -> Option<u8> {
    if tools == ["mail.accounts.list"] {
        Some(0)
    } else if tools
        == [
            "mail.accounts.list",
            "mail.folders.list",
            "mail.folders.refresh",
        ]
    {
        Some(1)
    } else if tools
        == [
            "mail.accounts.list",
            "mail.folders.list",
            "mail.folders.refresh",
            "mail.sync.page",
        ]
    {
        Some(2)
    } else if tools
        == [
            "mail.accounts.list",
            "mail.folders.list",
            "mail.folders.refresh",
            "mail.messages.search",
            "mail.sync.page",
        ]
    {
        Some(3)
    } else if tools
        == [
            "app.diagnostics.snapshot",
            "app.settings.display.get",
            "mail.accounts.list",
            "mail.folders.list",
            "mail.folders.refresh",
            "mail.messages.search",
            "mail.sync.page",
        ]
    {
        Some(4)
    } else if tools
        == [
            "app.diagnostics.snapshot",
            "app.settings.display.get",
            "mail.accounts.list",
            "mail.folders.list",
            "mail.folders.refresh",
            "mail.messages.get",
            "mail.messages.search",
            "mail.sync.page",
        ]
    {
        Some(5)
    } else if tools
        == [
            "app.diagnostics.snapshot",
            "app.settings.display.get",
            "mail.accounts.list",
            "mail.drafts.create",
            "mail.folders.list",
            "mail.folders.refresh",
            "mail.messages.get",
            "mail.messages.search",
            "mail.send.execute",
            "mail.send.request",
            "mail.sync.page",
        ]
    {
        Some(6)
    } else {
        None
    }
}
fn historical_toolset(tools: &[String], hash_version: u8) -> bool {
    if hash_version == 6 {
        historical_rank(tools).is_some()
    } else {
        (1..=5).contains(&hash_version)
            && historical_rank(tools).is_some_and(|rank| rank < hash_version)
    }
}
fn historical_transition(old: &[String], target: &[String], hash_version: u8) -> bool {
    historical_toolset(old, hash_version)
        && historical_toolset(target, hash_version)
        && historical_rank(old) <= historical_rank(target)
}

#[cfg(test)]
#[test]
fn cli15_app_read_scope_historical_membership_and_transitions_are_closed() {
    let sets: Vec<Vec<String>> = [
        vec!["mail.accounts.list"],
        vec![
            "mail.accounts.list",
            "mail.folders.list",
            "mail.folders.refresh",
        ],
        vec![
            "mail.accounts.list",
            "mail.folders.list",
            "mail.folders.refresh",
            "mail.sync.page",
        ],
        vec![
            "mail.accounts.list",
            "mail.folders.list",
            "mail.folders.refresh",
            "mail.messages.search",
            "mail.sync.page",
        ],
        vec![
            "app.diagnostics.snapshot",
            "app.settings.display.get",
            "mail.accounts.list",
            "mail.folders.list",
            "mail.folders.refresh",
            "mail.messages.search",
            "mail.sync.page",
        ],
    ]
    .into_iter()
    .map(|set| set.into_iter().map(str::to_owned).collect())
    .collect();
    for version in 1..=5 {
        for (old_rank, old) in sets.iter().enumerate() {
            assert_eq!(
                historical_toolset(old, version),
                old_rank < usize::from(version)
            );
            for (target_rank, target) in sets.iter().enumerate() {
                assert_eq!(
                    historical_transition(old, target, version),
                    old_rank <= target_rank && target_rank < usize::from(version)
                );
            }
        }
    }
    let body = vec![
        "app.diagnostics.snapshot",
        "app.settings.display.get",
        "mail.accounts.list",
        "mail.folders.list",
        "mail.folders.refresh",
        "mail.messages.get",
        "mail.messages.search",
        "mail.sync.page",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    let send = vec![
        "app.diagnostics.snapshot",
        "app.settings.display.get",
        "mail.accounts.list",
        "mail.drafts.create",
        "mail.folders.list",
        "mail.folders.refresh",
        "mail.messages.get",
        "mail.messages.search",
        "mail.send.execute",
        "mail.send.request",
        "mail.sync.page",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    for tools in [&body, &send] {
        assert!(historical_toolset(tools, 6));
        for version in 1..=5 {
            assert!(!historical_toolset(tools, version))
        }
    }
    assert!(historical_transition(&sets[4], &body, 6));
    assert!(historical_transition(&body, &send, 6));
    assert!(!historical_transition(&send, &body, 6));
    for version in [0, 7, 255] {
        assert!(!historical_toolset(&sets[4], version));
    }
}
fn accounts_column(row: &Row<'_>, index: usize) -> rusqlite::Result<Vec<String>> {
    let values: Vec<String> = serde_json::from_str(text_column(row, index, 32768)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    validate_accounts(&values).map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(values)
}
fn read_row(
    row: &Row<'_>,
    current: &RegistrationRecord,
    client_hash: &[u8; 32],
    maximum_hash_version: u8,
) -> rusqlite::Result<CliScopeRefresh> {
    let id = blob_column::<32>(row, 0)?;
    let registration_id = blob_column::<16>(row, 1)?;
    let base_version: i64 = row.get(2)?;
    let epoch: i64 = row.get(3)?;
    let target_epoch: i64 = row.get(4)?;
    let old = accounts_column(row, 5)?;
    let target = accounts_column(row, 6)?;
    let key_hash = blob_column::<32>(row, 7)?;
    let server = blob_column::<32>(row, 8)?;
    let payload = blob_column::<32>(row, 9)?;
    let phase = phase_parse(text_column(row, 10, 8)?)?;
    let version: i64 = row.get(11)?;
    let created: i64 = row.get(12)?;
    let expires: i64 = row.get(13)?;
    let updated: i64 = row.get(14)?;
    let decided: Option<i64> = row.get(15)?;
    let activated: Option<i64> = row.get(16)?;
    let hash_version: i64 = row.get(17)?;
    let base_tools: Vec<String> = serde_json::from_str(text_column(row, 18, 1024)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let target_tools: Vec<String> = serde_json::from_str(text_column(row, 19, 1024)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    if !(1..=i64::from(maximum_hash_version)).contains(&hash_version)
        || !historical_toolset(&base_tools, hash_version as u8)
        || !historical_toolset(&target_tools, hash_version as u8)
        || !historical_transition(&base_tools, &target_tools, hash_version as u8)
        || serde_json::to_string(&base_tools).map_err(|_| rusqlite::Error::InvalidQuery)?
            != text_column(row, 18, 1024)?
        || serde_json::to_string(&target_tools).map_err(|_| rusqlite::Error::InvalidQuery)?
            != text_column(row, 19, 1024)?
    {
        return invalid();
    }
    if id == [0; 32]
        || registration_id != current.registration_id
        || !(2..=i64::MAX - 2).contains(&base_version)
        || !(1..i64::MAX).contains(&epoch)
        || target_epoch != epoch + 1
        || (old == target && base_tools == target_tools)
        || key_hash != *client_hash
        || server == [0; 32]
        || payload == [0; 32]
        || created <= 0
        || created.checked_add(TTL) != Some(expires)
        || updated < created
    {
        return invalid();
    }
    let consistent = match phase {
        ScopeRefreshPhase::Pending => {
            version == 1 && decided.is_none() && activated.is_none() && updated == created
        }
        ScopeRefreshPhase::Staged | ScopeRefreshPhase::Denied => {
            version == 2
                && decided.is_some_and(|t| t >= created && t < expires && t == updated)
                && activated.is_none()
        }
        ScopeRefreshPhase::Expired => {
            version == 2
                && decided.is_some_and(|t| t >= expires && t == updated)
                && activated.is_none()
        }
        ScopeRefreshPhase::Active => {
            version == 3
                && decided.is_some_and(|d| {
                    d >= created && d < expires && activated.is_some_and(|a| a >= d && updated == a)
                })
        }
    };
    if !consistent {
        return invalid();
    }
    let mut base = current.clone();
    base.state = RegistrationState::Active;
    base.version = base_version as u64;
    base.profile_epoch = epoch as u64;
    base.scope = RequestedScope::new(old, base_tools).map_err(|_| rusqlite::Error::InvalidQuery)?;
    base.read_only = !base.scope.tools().iter().any(|t| t == "mail.send.execute");
    let value = CliScopeRefresh {
        refresh_id: id,
        hash_version: hash_version as u8,
        base_registration: base,
        target_accounts: target,
        target_tools,
        client_public_key_hash: key_hash,
        server_public_key_hash: server,
        payload_hash: payload,
        phase,
        version: version as u64,
        created_at_utc_ms: created,
        expires_at_utc_ms: expires,
        updated_at_utc_ms: updated,
        decided_at_utc_ms: decided,
        activated_at_utc_ms: activated,
    };
    if hash_payload(&value).map_err(|_| rusqlite::Error::InvalidQuery)? != payload {
        return invalid();
    }
    Ok(value)
}
pub(super) fn context(connection: &Connection) -> Result<Option<Context>, ErrorCode> {
    context_with_layout(connection, false, 6)
}
fn context_with_layout(
    connection: &Connection,
    legacy: bool,
    maximum_hash_version: u8,
) -> Result<Option<Context>, ErrorCode> {
    let count: i64 = db(connection.query_row(
        "SELECT count(*) FROM (SELECT refresh_id FROM client_scope_refreshes LIMIT 1025)",
        [],
        |row| row.get(0),
    ))?;
    if count > LIMIT as i64 {
        return Err(ErrorCode::QuotaExceeded);
    }
    let mut stmt = db(connection.prepare(
        "SELECT registration_id FROM client_registrations WHERE client_kind='cli' AND display_label=?1 LIMIT 2",
    ))?;
    let ids: Vec<[u8; 16]> =
        db(db(stmt.query_map([DEFAULT_CLI_LABEL], |row| blob_column(row, 0)))?.collect())?;
    if ids.len() > 1 {
        return Err(ErrorCode::Conflict);
    }
    let Some(id) = ids.first() else {
        return if count == 0 {
            Ok(None)
        } else {
            Err(ErrorCode::InternalError)
        };
    };
    let registration =
        db(read_registration_bare(connection, id))?.ok_or(ErrorCode::InternalError)?;
    let original =
        db(read_request(connection, &registration.request_id))?.ok_or(ErrorCode::InternalError)?;
    if original.state != RegistrationRequestState::Approved
        || original.client_kind != ClientKind::Cli
        || original.display_label != DEFAULT_CLI_LABEL
        || !historical_toolset(original.scope.tools(), 1)
        || !historical_toolset(registration.scope.tools(), maximum_hash_version)
        || registration.read_only
            == registration
                .scope
                .tools()
                .iter()
                .any(|t| t == "mail.send.execute")
        || registration.requests_per_minute != 60
        || registration.approved_channel != "approval_helper"
        || registration.request_id == [0; 32]
        || hantong_platform::registration_id_for_request(registration.request_id)
            .map_err(|_| ErrorCode::InternalError)?
            != registration.registration_id
    {
        return Err(ErrorCode::InternalError);
    }
    hantong_platform::validate_public_key(&registration.public_key)
        .map_err(|_| ErrorCode::InternalError)?;
    let key_hash =
        hantong_platform::sha256(&registration.public_key).map_err(|_| ErrorCode::InternalError)?;
    let (approved, updated): (i64, i64) = db(connection.query_row(
        "SELECT approved_at_utc_ms,updated_at_utc_ms FROM client_registrations WHERE registration_id=?1",
        [id.as_slice()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    let mut stmt = db(connection.prepare(
        if legacy { "SELECT refresh_id,registration_id,base_registration_version,base_profile_epoch,target_profile_epoch,base_accounts_json,target_accounts_json,client_public_key_hash,server_public_key_hash,payload_hash,phase,version,created_at_utc_ms,expires_at_utc_ms,updated_at_utc_ms,decided_at_utc_ms,activated_at_utc_ms,1,'[\"mail.accounts.list\"]','[\"mail.accounts.list\"]' FROM client_scope_refreshes ORDER BY target_profile_epoch,refresh_id LIMIT 1025" } else { "SELECT refresh_id,registration_id,base_registration_version,base_profile_epoch,target_profile_epoch,base_accounts_json,target_accounts_json,client_public_key_hash,server_public_key_hash,payload_hash,phase,version,created_at_utc_ms,expires_at_utc_ms,updated_at_utc_ms,decided_at_utc_ms,activated_at_utc_ms,hash_version,base_tools_json,target_tools_json FROM client_scope_refreshes ORDER BY target_profile_epoch,refresh_id LIMIT 1025" },
    ))?;
    let rows: Vec<CliScopeRefresh> = db(db(stmt.query_map([], |row| {
        read_row(row, &registration, &key_hash, maximum_hash_version)
    }))?
    .collect())?;
    if rows.len() > LIMIT {
        return Err(ErrorCode::QuotaExceeded);
    }
    validate_provenance(&registration, &rows, &original.scope, approved, updated)?;
    Ok(Some(Context {
        registration,
        rows,
        updated,
    }))
}

/// Reads the pre-v9 ledger layout without attempting to access the v9 columns.
/// The migration validator uses this before rebuilding the table, so the old
/// payload hash and legacy tool snapshot are checked exactly as stored.
pub(super) fn validate_history_v8(connection: &Connection) -> rusqlite::Result<()> {
    context_with_layout(connection, true, 1)
        .map(|_| ())
        .map_err(|_| rusqlite::Error::InvalidQuery)
}

/// Freezes v9-v11 semantics before a v12 schema migration.
pub(super) fn validate_history_v11(connection: &Connection) -> rusqlite::Result<()> {
    context_with_layout(connection, false, 2)
        .map(|_| ())
        .map_err(|_| rusqlite::Error::InvalidQuery)
}

/// Freezes v12-v14 history before the metadata-scope schema migration.
pub(super) fn validate_history_v14(connection: &Connection) -> rusqlite::Result<()> {
    context_with_layout(connection, false, 3)
        .map(|_| ())
        .map_err(|_| rusqlite::Error::InvalidQuery)
}

/// Freezes v15 history before the app-read scope schema migration.
pub(super) fn validate_history_v15(connection: &Connection) -> rusqlite::Result<()> {
    context_with_layout(connection, false, 4)
        .map(|_| ())
        .map_err(|_| rusqlite::Error::InvalidQuery)
}

pub(super) fn validate_history_v17(connection: &Connection) -> rusqlite::Result<()> {
    context_with_layout(connection, false, 5)
        .map(|_| ())
        .map_err(|_| rusqlite::Error::InvalidQuery)
}
struct Authorized {
    scope: RequestedScope,
    version: Option<u64>,
    at: i64,
}
fn validate_provenance(
    current: &RegistrationRecord,
    rows: &[CliScopeRefresh],
    original: &RequestedScope,
    approved: i64,
    updated: i64,
) -> Result<(), ErrorCode> {
    let mut authorized = BTreeMap::from([(
        1,
        Authorized {
            scope: original.clone(),
            version: None,
            at: approved,
        },
    )]);
    let mut latest = 1;
    let mut active: Vec<_> = rows
        .iter()
        .filter(|r| r.phase == ScopeRefreshPhase::Active)
        .collect();
    active.sort_by_key(|r| r.target_profile_epoch());
    for row in active {
        if row.base_registration.profile_epoch != latest {
            return Err(ErrorCode::InternalError);
        }
        check_base(
            row,
            authorized.get(&latest).ok_or(ErrorCode::InternalError)?,
        )?;
        latest = row.target_profile_epoch();
        if authorized
            .insert(
                latest,
                Authorized {
                    scope: row.target_scope(),
                    version: Some(row.base_registration.version + 2),
                    at: row.activated_at_utc_ms.ok_or(ErrorCode::InternalError)?,
                },
            )
            .is_some()
        {
            return Err(ErrorCode::InternalError);
        }
    }
    let mut staged = None;
    let mut open = 0;
    for row in rows {
        check_base(
            row,
            authorized
                .get(&row.base_registration.profile_epoch)
                .ok_or(ErrorCode::InternalError)?,
        )?;
        if rows
            .first()
            .is_some_and(|first| first.server_public_key_hash != row.server_public_key_hash)
        {
            return Err(ErrorCode::InternalError);
        }
        if matches!(
            row.phase,
            ScopeRefreshPhase::Pending | ScopeRefreshPhase::Staged
        ) {
            open += 1;
            if row.base_registration.profile_epoch != latest {
                return Err(ErrorCode::InternalError);
            }
            if row.phase == ScopeRefreshPhase::Staged {
                staged = Some(row);
            }
        }
    }
    if open > 1 {
        return Err(ErrorCode::InternalError);
    }
    let terminal = matches!(
        current.state,
        RegistrationState::Suspended | RegistrationState::Revoked
    );
    if let Some(staged) = staged {
        let base = &staged.base_registration;
        let decision = staged.decided_at_utc_ms.ok_or(ErrorCode::InternalError)?;
        if current.profile_epoch != staged.target_profile_epoch()
            || current.scope != base.scope
            || if terminal {
                current.version < base.version + 1 || updated < decision
            } else {
                current.state != RegistrationState::Provisioning
                    || current.version != base.version + 1
                    || updated != decision
            }
        {
            return Err(ErrorCode::InternalError);
        }
    } else {
        let authority = authorized.get(&latest).ok_or(ErrorCode::InternalError)?;
        if current.profile_epoch != latest
            || current.scope != authority.scope
            || updated < authority.at
        {
            return Err(ErrorCode::InternalError);
        }
        match current.state {
            RegistrationState::Provisioning => {
                if latest != 1 || !rows.is_empty() || current.version != 1 {
                    return Err(ErrorCode::InternalError);
                }
            }
            RegistrationState::Active => {
                if current.version < 2
                    || authority
                        .version
                        .is_some_and(|v| current.version != v || updated != authority.at)
                {
                    return Err(ErrorCode::InternalError);
                }
            }
            // Suspension/revocation may advance the CAS version, but can never
            // replace the last authorized epoch or its account grant snapshot.
            RegistrationState::Suspended | RegistrationState::Revoked => {
                if current.version < authority.version.unwrap_or(1) {
                    return Err(ErrorCode::InternalError);
                }
            }
        }
    }
    Ok(())
}
fn check_base(row: &CliScopeRefresh, authorized: &Authorized) -> Result<(), ErrorCode> {
    if row.base_registration.scope != authorized.scope
        || row.created_at_utc_ms < authorized.at
        || authorized
            .version
            .is_some_and(|v| row.base_registration.version != v)
    {
        return Err(ErrorCode::InternalError);
    }
    Ok(())
}
pub(super) fn load(
    connection: &Connection,
    id: [u8; 32],
) -> Result<Option<CliScopeRefresh>, ErrorCode> {
    Ok(context(connection)?.and_then(|c| c.rows.into_iter().find(|r| r.refresh_id == id)))
}
pub(super) fn list_phase(
    connection: &Connection,
    phase: ScopeRefreshPhase,
) -> Result<Vec<CliScopeRefresh>, ErrorCode> {
    let mut statement = db(connection.prepare(
        "SELECT refresh_id FROM client_scope_refreshes WHERE phase=?1 ORDER BY refresh_id LIMIT 2",
    ))?;
    let ids: Vec<[u8; 32]> =
        db(db(statement.query_map([phase_text(phase)], |row| blob_column(row, 0)))?.collect())?;
    if ids.len() > 1 {
        return Err(ErrorCode::Conflict);
    }
    let context = context(connection)?;
    ids.into_iter()
        .map(|id| {
            context
                .as_ref()
                .and_then(|c| c.rows.iter().find(|r| r.refresh_id == id))
                .cloned()
                .ok_or(ErrorCode::InternalError)
        })
        .collect()
}

fn server_pin(connection: &Connection, expected: &[u8; 32]) -> Result<(), ErrorCode> {
    let pin = db(connection.query_row(
        "SELECT key_id,state,public_key,key_epoch,version,created_at_utc_ms,updated_at_utc_ms FROM server_identity WHERE singleton=1",
        [],
        |row| {
            let key_id = blob_column::<32>(row, 0)?;
            let state = text_column(row, 1, 8)?;
            let epoch: i64 = row.get(3)?;
            let version: i64 = row.get(4)?;
            let created: i64 = row.get(5)?;
            let updated: i64 = row.get(6)?;
            if key_id == [0; 32] || epoch <= 0 || version <= 0 || created <= 0 || updated < created {
                return invalid();
            }
            if state == "creating" && row.get_ref(2)?.data_type() == rusqlite::types::Type::Null {
                return Ok(None);
            }
            if state != "active" {
                return invalid();
            }
            Ok(Some(blob_column::<72>(row, 2)?))
        },
    ).optional())?.flatten().ok_or(ErrorCode::Conflict)?;
    hantong_platform::validate_public_key(&pin).map_err(|_| ErrorCode::InternalError)?;
    if hantong_platform::sha256(&pin).map_err(|_| ErrorCode::InternalError)? != *expected {
        return Err(ErrorCode::Conflict);
    }
    Ok(())
}
fn accounts_exist(connection: &Connection, accounts: &[String]) -> Result<(), ErrorCode> {
    for id in accounts {
        let exists: bool = db(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1)",
            [id],
            |row| row.get(0),
        ))?;
        if !exists {
            return Err(ErrorCode::NotFound);
        }
    }
    Ok(())
}
pub(super) fn create(
    tx: &Transaction<'_>,
    id: [u8; 32],
    target_scope: RequestedScope,
    pin: [u8; 32],
) -> Result<CliScopeRefresh, ErrorCode> {
    let context = context(tx)?.ok_or(ErrorCode::NotFound)?;
    let base = context.registration;
    if base.state != RegistrationState::Active
        || (target_scope.accounts() == base.scope.accounts()
            && target_scope.tools() == base.scope.tools())
        || !is_supported_default_cli_toolset_transition(base.scope.tools(), target_scope.tools())
        || base.version > i64::MAX as u64 - 2
        || base.profile_epoch >= i64::MAX as u64
        || context.rows.iter().any(|r| {
            r.refresh_id == id
                || matches!(
                    r.phase,
                    ScopeRefreshPhase::Pending | ScopeRefreshPhase::Staged
                )
        })
    {
        return Err(ErrorCode::Conflict);
    }
    server_pin(tx, &pin)?;
    if context
        .rows
        .first()
        .is_some_and(|r| r.server_public_key_hash != pin)
    {
        return Err(ErrorCode::Conflict);
    }
    accounts_exist(tx, target_scope.accounts())?;
    if context.rows.len() >= LIMIT {
        return Err(ErrorCode::QuotaExceeded);
    }
    let now = db(crate::utc_millis())?;
    if now < context.updated {
        return Err(ErrorCode::ApprovalExpired);
    }
    let mut value = CliScopeRefresh {
        refresh_id: id,
        hash_version: if target_scope
            .tools()
            .iter()
            .any(|t| t == "mail.messages.get")
        {
            6
        } else {
            5
        },
        client_public_key_hash: hantong_platform::sha256(&base.public_key)
            .map_err(|_| ErrorCode::InternalError)?,
        base_registration: base,
        target_accounts: target_scope.accounts().to_vec(),
        target_tools: target_scope.tools().to_vec(),
        server_public_key_hash: pin,
        payload_hash: [0; 32],
        phase: ScopeRefreshPhase::Pending,
        version: 1,
        created_at_utc_ms: now,
        expires_at_utc_ms: now.checked_add(TTL).ok_or(ErrorCode::InternalError)?,
        updated_at_utc_ms: now,
        decided_at_utc_ms: None,
        activated_at_utc_ms: None,
    };
    value.payload_hash = hash_payload(&value)?;
    let old = serde_json::to_string(value.base_registration.scope.accounts())
        .map_err(|_| ErrorCode::InternalError)?;
    let target =
        serde_json::to_string(&value.target_accounts).map_err(|_| ErrorCode::InternalError)?;
    let old_tools = serde_json::to_string(value.base_registration.scope.tools())
        .map_err(|_| ErrorCode::InternalError)?;
    let target_tools =
        serde_json::to_string(&value.target_tools).map_err(|_| ErrorCode::InternalError)?;
    db(tx.execute(
        "INSERT INTO client_scope_refreshes(refresh_id,registration_id,base_registration_version,base_profile_epoch,target_profile_epoch,base_accounts_json,target_accounts_json,client_public_key_hash,server_public_key_hash,payload_hash,phase,version,created_at_utc_ms,expires_at_utc_ms,updated_at_utc_ms,decided_at_utc_ms,activated_at_utc_ms,hash_version,base_tools_json,target_tools_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'pending',1,?11,?12,?11,NULL,NULL,?13,?14,?15)",
        params![
            id.as_slice(),
            value.base_registration.registration_id.as_slice(),
            value.base_registration.version as i64,
            value.base_registration.profile_epoch as i64,
            value.target_profile_epoch() as i64,
            old,
            target,
            value.client_public_key_hash.as_slice(),
            pin.as_slice(),
            value.payload_hash.as_slice(),
            now,
            value.expires_at_utc_ms,
            i64::from(value.hash_version),
            old_tools,
            target_tools,
        ],
    ))?;
    load(tx, id)?.ok_or(ErrorCode::InternalError)
}
fn exact(
    connection: &Connection,
    expected: &CliScopeRefresh,
    phase: ScopeRefreshPhase,
) -> Result<Context, ErrorCode> {
    let context = context(connection)?.ok_or(ErrorCode::NotFound)?;
    let row = context
        .rows
        .iter()
        .find(|r| r.refresh_id == expected.refresh_id)
        .ok_or(ErrorCode::NotFound)?;
    if row != expected || row.phase != phase {
        return Err(ErrorCode::Conflict);
    }
    Ok(context)
}
fn update(
    tx: &Transaction<'_>,
    expected: &CliScopeRefresh,
    phase: ScopeRefreshPhase,
    now: i64,
) -> Result<(), ErrorCode> {
    let (version, decision, activation) = if phase == ScopeRefreshPhase::Active {
        (3, expected.decided_at_utc_ms, Some(now))
    } else {
        (2, Some(now), None)
    };
    let changed = db(tx.execute(
        "UPDATE client_scope_refreshes SET phase=?2,version=?3,updated_at_utc_ms=?4,decided_at_utc_ms=?5,activated_at_utc_ms=?6 WHERE refresh_id=?1 AND phase=?7 AND version=?8",
        params![
            expected.refresh_id.as_slice(),
            phase_text(phase),
            version,
            now,
            decision,
            activation,
            phase_text(expected.phase),
            expected.version as i64,
        ],
    ))?;
    if changed != 1 {
        return Err(ErrorCode::Conflict);
    }
    Ok(())
}
pub(super) fn change(
    tx: &Transaction<'_>,
    expected: &CliScopeRefresh,
    phase: ScopeRefreshPhase,
) -> Result<CliScopeRefresh, ErrorCode> {
    let context = exact(tx, expected, ScopeRefreshPhase::Pending)?;
    let now = db(crate::utc_millis())?;
    if now < expected.updated_at_utc_ms || now >= expected.expires_at_utc_ms {
        return Err(ErrorCode::ApprovalExpired);
    }
    if phase == ScopeRefreshPhase::Staged {
        if context.registration != expected.base_registration {
            return Err(ErrorCode::Conflict);
        }
        if now < context.updated {
            return Err(ErrorCode::ApprovalExpired);
        }
        accounts_exist(tx, &expected.target_accounts)?;
        server_pin(tx, &expected.server_public_key_hash)?;
        let changed = db(tx.execute(
            "UPDATE client_registrations SET state='provisioning',profile_epoch=?2,version=version+1,updated_at_utc_ms=?3 WHERE registration_id=?1 AND state='active' AND profile_epoch=?4 AND version=?5",
            params![
                expected.base_registration.registration_id.as_slice(),
                expected.target_profile_epoch() as i64,
                now,
                expected.base_registration.profile_epoch as i64,
                expected.base_registration.version as i64,
            ],
        ))?;
        if changed != 1 {
            return Err(ErrorCode::Conflict);
        }
    }
    update(tx, expected, phase, now)?;
    load(tx, expected.refresh_id)?.ok_or(ErrorCode::InternalError)
}
pub(super) fn expire(tx: &Transaction<'_>, expected: &CliScopeRefresh) -> Result<bool, ErrorCode> {
    exact(tx, expected, ScopeRefreshPhase::Pending)?;
    let now = db(crate::utc_millis())?;
    if now < expected.updated_at_utc_ms {
        return Err(ErrorCode::ApprovalExpired);
    }
    if now < expected.expires_at_utc_ms {
        return Ok(false);
    }
    update(tx, expected, ScopeRefreshPhase::Expired, now)?;
    context(tx)?;
    Ok(true)
}
pub(super) fn activate(
    tx: &Transaction<'_>,
    expected: &CliScopeRefresh,
) -> Result<RegistrationRecord, ErrorCode> {
    let context = exact(tx, expected, ScopeRefreshPhase::Staged)?;
    let mut staged = expected.base_registration.clone();
    staged.state = RegistrationState::Provisioning;
    staged.profile_epoch = expected.target_profile_epoch();
    staged.version += 1;
    if context.registration != staged {
        return Err(ErrorCode::Conflict);
    }
    let now = db(crate::utc_millis())?;
    if now < context.updated || now < expected.updated_at_utc_ms {
        return Err(ErrorCode::ApprovalExpired);
    }
    server_pin(tx, &expected.server_public_key_hash)?;
    accounts_exist(tx, &expected.target_accounts)?;
    db(tx.execute(
        "DELETE FROM client_registration_accounts WHERE registration_id=?1",
        [staged.registration_id.as_slice()],
    ))?;
    for account in &expected.target_accounts {
        db(tx.execute(
            "INSERT INTO client_registration_accounts(registration_id,account_id) VALUES(?1,?2)",
            params![staged.registration_id.as_slice(), account],
        ))?;
    }
    db(tx.execute(
        "DELETE FROM client_registration_tools WHERE registration_id=?1",
        [staged.registration_id.as_slice()],
    ))?;
    for tool in &expected.target_tools {
        db(tx.execute(
            "INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,?2)",
            params![staged.registration_id.as_slice(), tool],
        ))?;
    }
    let changed = db(tx.execute(
        "UPDATE client_registrations SET state='active',read_only=?5,version=version+1,updated_at_utc_ms=?2 WHERE registration_id=?1 AND state='provisioning' AND profile_epoch=?3 AND version=?4",
        params![
            staged.registration_id.as_slice(),
            now,
            staged.profile_epoch as i64,
            staged.version as i64,
            !expected.target_tools.iter().any(|t|t=="mail.send.execute"),
        ],
    ))?;
    if changed != 1 {
        return Err(ErrorCode::Conflict);
    }
    update(tx, expected, ScopeRefreshPhase::Active, now)?;
    Ok(self::context(tx)?
        .ok_or(ErrorCode::InternalError)?
        .registration)
}
