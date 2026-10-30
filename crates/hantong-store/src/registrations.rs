//! Bounded persistence for registration requests and read-only registrations.
//! Approval and activation are deliberately absent from this module.

use crate::Store;
pub use hantong_contracts::client::ClientKind;
use hantong_contracts::client::validate_client_label as validate_label;
use hantong_contracts::{ErrorCode, auth::RequestedScope};
use std::fmt;

#[cfg(windows)]
const REQUEST_TTL_MS: i64 = 5 * 60 * 1000;
#[cfg(windows)]
const MAX_PENDING: i64 = 64;
#[cfg(windows)]
const MAX_RETAINED: i64 = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationRequestState {
    Pending,
    Approved,
    Denied,
    Expired,
}
impl RegistrationRequestState {
    #[cfg(windows)]
    fn parse(v: &str) -> Result<Self, ErrorCode> {
        match v {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "denied" => Ok(Self::Denied),
            "expired" => Ok(Self::Expired),
            _ => Err(ErrorCode::InternalError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationState {
    Provisioning,
    Active,
    Suspended,
    Revoked,
}
impl RegistrationState {
    #[cfg(windows)]
    fn parse(v: &str) -> Result<Self, ErrorCode> {
        match v {
            "provisioning" => Ok(Self::Provisioning),
            "active" => Ok(Self::Active),
            "suspended" => Ok(Self::Suspended),
            "revoked" => Ok(Self::Revoked),
            _ => Err(ErrorCode::InternalError),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct RegistrationRequest {
    pub request_id: [u8; 32],
    pub client_kind: ClientKind,
    pub display_label: String,
    pub scope: RequestedScope,
    pub state: RegistrationRequestState,
    pub version: u64,
    pub created_at_utc_ms: i64,
    pub expires_at_utc_ms: i64,
}
impl fmt::Debug for RegistrationRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegistrationRequest")
            .field("request_id", &"<redacted>")
            .field("client_kind", &self.client_kind)
            .field("display_label", &"<redacted>")
            .field("scope", &"<redacted>")
            .field("state", &self.state)
            .field("version", &self.version)
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct RegistrationRecord {
    pub registration_id: [u8; 16],
    pub request_id: [u8; 32],
    pub client_kind: ClientKind,
    pub display_label: String,
    pub public_key: [u8; 72],
    pub profile_epoch: u64,
    pub state: RegistrationState,
    pub read_only: bool,
    pub requests_per_minute: u32,
    pub approved_channel: String,
    pub version: u64,
    pub scope: RequestedScope,
}
impl fmt::Debug for RegistrationRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegistrationRecord")
            .field("registration_id", &"<redacted>")
            .field("request_id", &"<redacted>")
            .field("client_kind", &self.client_kind)
            .field("display_label", &"<redacted>")
            .field("public_key", &"<redacted>")
            .field("profile_epoch", &self.profile_epoch)
            .field("state", &self.state)
            .field("scope", &"<redacted>")
            .finish()
    }
}

impl Store {
    /// Stores a request, never an authorization or an active registration.
    pub fn create_registration_request(
        &self,
        request_id: [u8; 32],
        client_kind: ClientKind,
        display_label: &str,
        scope: &RequestedScope,
    ) -> Result<RegistrationRequest, ErrorCode> {
        if request_id == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        validate_label(display_label)?;
        #[cfg(not(windows))]
        {
            let _ = (request_id, client_kind, scope);
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            enum InsertResult {
                Created(RegistrationRequest),
                Duplicate,
                Limited,
            }
            let accounts =
                serde_json::to_string(scope.accounts()).map_err(|_| ErrorCode::InternalError)?;
            let tools =
                serde_json::to_string(scope.tools()).map_err(|_| ErrorCode::InternalError)?;
            let outcome = self.with_connection(|connection| {
                let tx = rusqlite::Transaction::new_unchecked(
                    connection,
                    rusqlite::TransactionBehavior::Immediate,
                )?;
                let exists: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM client_registration_requests WHERE request_id=?1)",
                    [request_id.as_slice()],
                    |row| row.get(0),
                )?;
                if exists {
                    return Ok(InsertResult::Duplicate);
                }
                let now = crate::utc_millis()?;
                let pending: i64 = tx.query_row(
                    "SELECT count(*) FROM client_registration_requests
                     WHERE state='pending' AND expires_at_utc_ms>?1",
                    [now],
                    |row| row.get(0),
                )?;
                let retained: i64 = tx.query_row(
                    "SELECT count(*) FROM client_registration_requests",
                    [],
                    |row| row.get(0),
                )?;
                if pending >= MAX_PENDING || retained >= MAX_RETAINED {
                    return Ok(InsertResult::Limited);
                }
                let expires = now
                    .checked_add(REQUEST_TTL_MS)
                    .ok_or(rusqlite::Error::InvalidQuery)?;
                tx.execute(
                    "INSERT INTO client_registration_requests
                     (request_id,client_kind,display_label,requested_accounts_json,
                      requested_tools_json,state,decision_channel,version,created_at_utc_ms,
                      expires_at_utc_ms,decided_at_utc_ms)
                     VALUES(?1,?2,?3,?4,?5,'pending',NULL,1,?6,?7,NULL)",
                    rusqlite::params![
                        request_id.as_slice(),
                        client_kind.as_str(),
                        display_label,
                        accounts,
                        tools,
                        now,
                        expires
                    ],
                )?;
                tx.commit()?;
                Ok(InsertResult::Created(RegistrationRequest {
                    request_id,
                    client_kind,
                    display_label: display_label.to_owned(),
                    scope: scope.clone(),
                    state: RegistrationRequestState::Pending,
                    version: 1,
                    created_at_utc_ms: now,
                    expires_at_utc_ms: expires,
                }))
            })?;
            match outcome {
                InsertResult::Created(request) => Ok(request),
                InsertResult::Duplicate => Err(ErrorCode::Conflict),
                InsertResult::Limited => Err(ErrorCode::RateLimited),
            }
        }
    }

    /// Returns false only for a valid, current request that has not expired.
    /// The caller cannot advance the clock or approve a request through this API.
    pub fn expire_registration_request(
        &self,
        request_id: [u8; 32],
        expected_version: u64,
    ) -> Result<bool, ErrorCode> {
        if expected_version == 0 || expected_version > i64::MAX as u64 {
            return Err(ErrorCode::InvalidInput);
        }
        if expected_version == i64::MAX as u64 {
            return Err(ErrorCode::Conflict);
        }
        #[cfg(not(windows))]
        {
            let _ = request_id;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            enum ExpireOutcome {
                Expired,
                NotExpired,
                Missing,
                Conflict,
            }
            let outcome = self.with_connection(|connection| {
                let tx = rusqlite::Transaction::new_unchecked(
                    connection,
                    rusqlite::TransactionBehavior::Immediate,
                )?;
                let Some(current) = read_request(&tx, &request_id)? else {
                    return Ok(ExpireOutcome::Missing);
                };
                if current.state != RegistrationRequestState::Pending
                    || current.version != expected_version
                {
                    return Ok(ExpireOutcome::Conflict);
                }
                let now = crate::utc_millis()?;
                if current.expires_at_utc_ms > now {
                    return Ok(ExpireOutcome::NotExpired);
                }
                let changed = tx.execute(
                    "UPDATE client_registration_requests
                     SET state='expired',decided_at_utc_ms=?1,version=version+1
                     WHERE request_id=?2 AND state='pending' AND version=?3",
                    rusqlite::params![now, request_id.as_slice(), expected_version as i64],
                )?;
                if changed != 1 {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                tx.commit()?;
                Ok(ExpireOutcome::Expired)
            })?;
            match outcome {
                ExpireOutcome::Expired => Ok(true),
                ExpireOutcome::NotExpired => Ok(false),
                ExpireOutcome::Missing => Err(ErrorCode::NotFound),
                ExpireOutcome::Conflict => Err(ErrorCode::Conflict),
            }
        }
    }

    pub fn get_registration_request(
        &self,
        request_id: [u8; 32],
    ) -> Result<Option<RegistrationRequest>, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = request_id;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.with_connection(|connection| read_request(connection, &request_id))
        }
    }

    /// Returns validated stored metadata, not proof of an authenticated client.
    pub fn load_registration(
        &self,
        registration_id: [u8; 16],
    ) -> Result<Option<RegistrationRecord>, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = registration_id;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.with_connection(|connection| {
                // Read the registration, decision, and grants from one snapshot.
                let tx = rusqlite::Transaction::new_unchecked(
                    connection,
                    rusqlite::TransactionBehavior::Deferred,
                )?;
                let registration = read_registration(&tx, &registration_id)?;
                tx.commit()?;
                Ok(registration)
            })
        }
    }
}

#[cfg(windows)]
pub(super) fn text_column<'row>(
    row: &'row rusqlite::Row<'_>,
    index: usize,
    max_bytes: usize,
) -> rusqlite::Result<&'row str> {
    let value = row
        .get_ref(index)?
        .as_str()
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    if value.len() > max_bytes {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(value)
}

#[cfg(windows)]
pub(super) fn blob_column<const N: usize>(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<[u8; N]> {
    row.get_ref(index)?
        .as_blob()
        .map_err(|_| rusqlite::Error::InvalidQuery)?
        .try_into()
        .map_err(|_| rusqlite::Error::InvalidQuery)
}

#[cfg(windows)]
fn parse_scope(accounts: &str, tools: &str) -> rusqlite::Result<RequestedScope> {
    let accounts: Vec<String> =
        serde_json::from_str(accounts).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let tools: Vec<String> =
        serde_json::from_str(tools).map_err(|_| rusqlite::Error::InvalidQuery)?;
    RequestedScope::new(accounts, tools).map_err(|_| rusqlite::Error::InvalidQuery)
}

#[cfg(windows)]
pub(super) fn read_request(
    connection: &rusqlite::Connection,
    id: &[u8; 32],
) -> rusqlite::Result<Option<RegistrationRequest>> {
    use rusqlite::{OptionalExtension, types::ValueRef};
    connection
        .query_row(
            "SELECT client_kind,display_label,requested_accounts_json,requested_tools_json,
         state,decision_channel,decided_at_utc_ms,version,created_at_utc_ms,expires_at_utc_ms
         FROM client_registration_requests WHERE request_id=?1",
            [id.as_slice()],
            |row| {
                let kind = ClientKind::parse(text_column(row, 0, 3)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                let label = text_column(row, 1, 512)?;
                validate_label(label).map_err(|_| rusqlite::Error::InvalidQuery)?;
                let scope = parse_scope(text_column(row, 2, 32768)?, text_column(row, 3, 32768)?)?;
                let state = RegistrationRequestState::parse(text_column(row, 4, 8)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                let channel = match row.get_ref(5)? {
                    ValueRef::Null => None,
                    _ => Some(text_column(row, 5, 15)?),
                };
                let decided: Option<i64> = row.get(6)?;
                let version: i64 = row.get(7)?;
                let created: i64 = row.get(8)?;
                let expires: i64 = row.get(9)?;
                if version <= 0
                    || created <= 0
                    || expires <= created
                    || expires
                        .checked_sub(created)
                        .is_none_or(|ttl| ttl > REQUEST_TTL_MS)
                {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                let consistent = match state {
                    RegistrationRequestState::Pending => channel.is_none() && decided.is_none(),
                    RegistrationRequestState::Expired => {
                        channel.is_none() && decided.is_some_and(|time| time >= expires)
                    }
                    RegistrationRequestState::Approved | RegistrationRequestState::Denied => {
                        matches!(channel, Some("approval_helper" | "gui"))
                            && decided.is_some_and(|time| time >= created && time < expires)
                    }
                };
                if !consistent {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                Ok(RegistrationRequest {
                    request_id: *id,
                    client_kind: kind,
                    display_label: label.to_owned(),
                    scope,
                    state,
                    version: version as u64,
                    created_at_utc_ms: created,
                    expires_at_utc_ms: expires,
                })
            },
        )
        .optional()
}

#[cfg(windows)]
pub(super) fn read_registration(
    connection: &rusqlite::Connection,
    id: &[u8; 16],
) -> rusqlite::Result<Option<RegistrationRecord>> {
    let record = read_registration_bare(connection, id)?;
    if let Some(value) = &record {
        crate::scope_refresh::validate_registration(connection, value)?;
        crate::registration_activation::validate_gui_registration(connection, value)?;
    }
    Ok(record)
}

// Scope provenance reads this structural snapshot directly; calling the public
// adapter from history validation would recurse through the refresh reader.
#[cfg(windows)]
pub(super) fn read_registration_bare(
    connection: &rusqlite::Connection,
    id: &[u8; 16],
) -> rusqlite::Result<Option<RegistrationRecord>> {
    use rusqlite::OptionalExtension;
    connection
        .query_row(
            "SELECT request_id,client_kind,display_label,public_key,profile_epoch,
         state,read_only,requests_per_minute,approved_channel,version,
         approved_at_utc_ms,updated_at_utc_ms,revoked_at_utc_ms
         FROM client_registrations WHERE registration_id=?1",
            [id.as_slice()],
            |row| {
                let request_id = blob_column::<32>(row, 0)?;
                let kind = ClientKind::parse(text_column(row, 1, 3)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                let label = text_column(row, 2, 512)?;
                validate_label(label).map_err(|_| rusqlite::Error::InvalidQuery)?;
                let public_key = blob_column::<72>(row, 3)?;
                if &public_key[..4] != b"ECS1" || public_key[4..8] != 32u32.to_le_bytes() {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                let epoch: i64 = row.get(4)?;
                let state = RegistrationState::parse(text_column(row, 5, 12)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                let read_only: i64 = row.get(6)?;
                let rate: i64 = row.get(7)?;
                let channel = text_column(row, 8, 15)?;
                let version: i64 = row.get(9)?;
                let approved: i64 = row.get(10)?;
                let updated: i64 = row.get(11)?;
                let revoked: Option<i64> = row.get(12)?;
                if epoch <= 0
                    || version <= 0
                    || !(0..=1).contains(&read_only)
                    || !(1..=600).contains(&rate)
                    || !matches!(channel, "approval_helper" | "gui")
                    || approved <= 0
                    || updated < approved
                    || match state {
                        RegistrationState::Revoked => {
                            revoked.is_none_or(|time| time < approved || time > updated)
                        }
                        _ => revoked.is_some(),
                    }
                {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                let request =
                    read_request(connection, &request_id)?.ok_or(rusqlite::Error::InvalidQuery)?;
                if request.state != RegistrationRequestState::Approved
                    || request.client_kind != kind
                    || request.display_label != label
                {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                let decision_matches: bool = connection.query_row(
                    "SELECT decision_channel=?1 AND decided_at_utc_ms=?2
                 FROM client_registration_requests WHERE request_id=?3",
                    rusqlite::params![channel, approved, request_id.as_slice()],
                    |decision| decision.get(0),
                )?;
                if !decision_matches {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                let scope = read_granted_scope(connection, id)?;
                Ok(RegistrationRecord {
                    registration_id: *id,
                    request_id,
                    client_kind: kind,
                    display_label: label.to_owned(),
                    public_key,
                    profile_epoch: epoch as u64,
                    state,
                    read_only: read_only == 1,
                    requests_per_minute: rate as u32,
                    approved_channel: channel.to_owned(),
                    version: version as u64,
                    scope,
                })
            },
        )
        .optional()
}

#[cfg(windows)]
fn read_granted_scope(
    connection: &rusqlite::Connection,
    registration_id: &[u8; 16],
) -> rusqlite::Result<RequestedScope> {
    let mut statement = connection.prepare(
        "SELECT g.account_id, a.account_id IS NOT NULL
         FROM client_registration_accounts g LEFT JOIN accounts a ON a.account_id=g.account_id
         WHERE registration_id=?1 LIMIT 129",
    )?;
    let values = statement.query_map([registration_id.as_slice()], |row| {
        if !row.get::<_, bool>(1)? {
            return Err(rusqlite::Error::InvalidQuery);
        }
        Ok(text_column(row, 0, 128)?.to_owned())
    })?;
    let accounts: Vec<String> = values.collect::<rusqlite::Result<_>>()?;
    if accounts.len() > 128 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let mut statement = connection.prepare(
        "SELECT tool_id FROM client_registration_tools WHERE registration_id=?1 LIMIT 129",
    )?;
    let values = statement.query_map([registration_id.as_slice()], |row| {
        Ok(text_column(row, 0, 96)?.to_owned())
    })?;
    let tools: Vec<String> = values.collect::<rusqlite::Result<_>>()?;
    RequestedScope::new(accounts, tools).map_err(|_| rusqlite::Error::InvalidQuery)
}

#[cfg(test)]
mod tests;
