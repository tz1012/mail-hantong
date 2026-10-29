//! New-only IMAP account storage. Inputs and receipts are data, not authority.
use crate::{
    Store,
    mail_connection::{MailMutation, MailObservation},
};
#[cfg(windows)]
use crate::{
    mail_connection::{MailEndpointKind, check_account, read_endpoint, read_observation, replay},
    registrations::text_column,
};
use hantong_contracts::ErrorCode;
#[cfg(windows)]
use hantong_domain::mail_connection::MailConnectionConfig;
use hantong_domain::{
    account_provisioning::NewImapAccountDraft,
    accounts::AccountStatus,
    mail_connection::{Authentication, ImapEndpointConfig, TlsMode},
};
#[cfg(windows)]
use rusqlite::{Transaction, TransactionBehavior, params};
use std::fmt;

#[derive(Clone, Eq, PartialEq)]
pub struct StagedImapAccount {
    pub account_id: String,
    pub account_version: u64,
    pub imap_version: u64,
}
#[derive(Clone, Eq, PartialEq)]
pub enum InitialImapProbeOutcome {
    Ready(Vec<String>),
    AuthRequired,
    TlsFailed,
    Offline,
    Error,
}
#[derive(Clone, Eq, PartialEq)]
pub struct InitialImapProbeReceipt {
    pub account_version: u64,
    pub endpoint_version: u64,
    pub observation_version: u64,
}
#[derive(Clone, Eq, PartialEq)]
pub struct ImapProvisioningState {
    pub account_id: String,
    pub status: AccountStatus,
    pub account_version: u64,
    pub endpoint: ImapEndpointConfig,
    pub capabilities: Vec<String>,
    pub observation: Option<MailObservation>,
}
macro_rules! redacted_debug {
    ($($name:ident),+ $(,)?) => {$(
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), " { <redacted> }"))
            }
        }
    )+};
}
redacted_debug!(
    StagedImapAccount,
    InitialImapProbeOutcome,
    InitialImapProbeReceipt,
    ImapProvisioningState
);

pub fn imap_account_stage_payload_hash(
    draft: &NewImapAccountDraft,
    endpoint: &ImapEndpointConfig,
) -> Result<[u8; 32], ErrorCode> {
    draft.validate().map_err(|_| ErrorCode::InvalidInput)?;
    if endpoint.settings_version() != 1 {
        return Err(ErrorCode::InvalidInput);
    }
    // Domain values are immutable and bounded before constructing canonical bytes.
    let mut bytes = b"mail-hantong.account-provision.v1\0".to_vec();
    let push = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    };
    push(&mut bytes, draft.account_id());
    for value in [draft.display_name(), draft.color()] {
        bytes.push(u8::from(value.is_some()));
        if let Some(value) = value {
            push(&mut bytes, value);
        }
    }
    for value in [0_u64, 1, 0, 1] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    push(&mut bytes, endpoint.host());
    bytes.extend_from_slice(&endpoint.port().to_be_bytes());
    bytes.push(match endpoint.tls() {
        TlsMode::ImplicitTls => 0,
        TlsMode::StarttlsRequired => 1,
    });
    push(&mut bytes, endpoint.login_name());
    bytes.push(match endpoint.authentication() {
        Authentication::AppPassword => 0,
        Authentication::Oauth2 => 1,
    });
    #[cfg(windows)]
    {
        hantong_platform::sha256(&bytes).map_err(|_| ErrorCode::InternalError)
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
impl Store {
    /// Creates a new account only. A matching ledger replay never restores old settings.
    pub fn stage_imap_account(
        &self,
        draft: &NewImapAccountDraft,
        endpoint: &ImapEndpointConfig,
        mutation: &MailMutation,
    ) -> Result<StagedImapAccount, ErrorCode> {
        if mutation.expected_version() != 0 {
            return Err(ErrorCode::InvalidInput);
        }
        let hash = imap_account_stage_payload_hash(draft, endpoint)?;
        if &hash != mutation.payload_hash() {
            return Err(ErrorCode::InvalidInput);
        }
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(TransactionBehavior::Immediate, |tx| {
                let receipt = StagedImapAccount {
                    account_id: draft.account_id().to_owned(),
                    account_version: 1,
                    imap_version: 1,
                };
                if replay(tx, draft.account_id(), MailEndpointKind::Imap, mutation)?.is_some() {
                    read_state(tx, draft.account_id()).map_err(|_| ErrorCode::InternalError)?;
                    return Ok(receipt);
                }
                let existing: bool = db(tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1)",
                    [draft.account_id()],
                    |row| row.get(0),
                ))?;
                let duplicate = has_matching_imap_endpoint(tx, endpoint)?;
                if existing || duplicate {
                    return Err(ErrorCode::Conflict);
                }
                let operations: i64 = db(tx.query_row(
                    "SELECT count(*) FROM mail_connection_operations",
                    [],
                    |row| row.get(0),
                ))?;
                if operations >= 100000 {
                    return Err(ErrorCode::QuotaExceeded);
                }
                let maximum: Option<i64> = db(tx.query_row(
                    "SELECT max(account_number) FROM accounts",
                    [],
                    |row| row.get(0),
                ))?;
                if maximum.is_some_and(|value| value <= 0) {
                    return Err(ErrorCode::InternalError);
                }
                let number = maximum
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or(ErrorCode::Conflict)?;
                let now = db(crate::utc_millis())?;
                db(tx.execute(
                    "INSERT INTO accounts(account_id,provider,provider_subject,primary_address,display_name,color,account_number,status,capabilities,granted_scopes,settings_version) VALUES(?1,'imap',NULL,NULL,?2,?3,?4,'connecting','[]','[]',1)",
                    params![
                        draft.account_id(),
                        draft.display_name(),
                        draft.color(),
                        number,
                    ],
                ))?;
                let tls = match endpoint.tls() {
                    TlsMode::ImplicitTls => "implicit_tls",
                    TlsMode::StarttlsRequired => "starttls_required",
                };
                let auth = match endpoint.authentication() {
                    Authentication::AppPassword => "app_password",
                    Authentication::Oauth2 => "oauth2",
                };
                db(tx.execute(
                    "INSERT INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES(?1,'imap',1,?2,?3,?4,?5,?6,?7)",
                    params![
                        draft.account_id(),
                        endpoint.host(),
                        endpoint.port(),
                        tls,
                        endpoint.login_name(),
                        auth,
                        now,
                    ],
                ))?;
                db(tx.execute(
                    "INSERT INTO mail_connection_operations(operation_id,account_id,idempotency_key,endpoint_kind,payload_hash,expected_version,result_version,completed_at_utc_ms) VALUES(?1,?2,?3,'imap',?4,0,1,?5)",
                    params![
                        mutation.operation_id(),
                        draft.account_id(),
                        mutation.idempotency_key(),
                        mutation.payload_hash().as_slice(),
                        now,
                    ],
                ))?;
                Ok(receipt)
            })
        }
    }
    /// Applies a captured first probe to a still-connecting account under three CAS checks.
    pub fn finish_initial_imap_probe(
        &self,
        account_id: &str,
        expected_account_version: u64,
        expected_endpoint_version: u64,
        expected_observation_version: u64,
        outcome: &InitialImapProbeOutcome,
    ) -> Result<InitialImapProbeReceipt, ErrorCode> {
        crate::mail_connection::input_account(account_id)?;
        if !(1..i64::MAX as u64).contains(&expected_account_version)
            || !(1..=i64::MAX as u64).contains(&expected_endpoint_version)
            || expected_observation_version >= i64::MAX as u64
        {
            return Err(ErrorCode::InvalidInput);
        }
        if let InitialImapProbeOutcome::Ready(values) = outcome {
            validate_capabilities(values).map_err(|_| ErrorCode::InvalidInput)?;
        }
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(TransactionBehavior::Immediate, |tx| {
                let current = read_state(tx, account_id)?;
                if current.status != AccountStatus::Connecting
                    || current.account_version != expected_account_version
                    || current.endpoint.settings_version() != expected_endpoint_version
                    || current.observation.as_ref().map_or(0, |value| value.observation_version)
                        != expected_observation_version
                {
                    return Err(ErrorCode::Conflict);
                }
                let now = db(crate::utc_millis())?;
                if current.observation.as_ref().is_some_and(|value| now < value.observed_at_utc_ms) {
                    return Err(ErrorCode::InternalError);
                }
                let (status, observed, capabilities) = match outcome {
                    InitialImapProbeOutcome::Ready(values) => ("ready", "ready", values.as_slice()),
                    InitialImapProbeOutcome::AuthRequired => ("reauth_required", "auth_required", &[][..]),
                    InitialImapProbeOutcome::TlsFailed => ("error", "tls_failed", &[][..]),
                    InitialImapProbeOutcome::Offline => ("offline", "offline", &[][..]),
                    InitialImapProbeOutcome::Error => ("error", "error", &[][..]),
                };
                let capabilities = serde_json::to_string(capabilities)
                    .map_err(|_| ErrorCode::InternalError)?;
                let next_account = expected_account_version + 1;
                let next_observation = expected_observation_version + 1;
                let changed = db(tx.execute(
                    "UPDATE accounts SET status=?2,capabilities=?3,settings_version=?4 WHERE account_id=?1 AND provider='imap' AND status='connecting' AND settings_version=?5",
                    params![
                        account_id,
                        status,
                        capabilities,
                        next_account as i64,
                        expected_account_version as i64,
                    ],
                ))?;
                if changed != 1 {
                    return Err(ErrorCode::Conflict);
                }
                db(tx.execute(
                    "INSERT INTO mail_connection_observations(account_id,endpoint_kind,observed_settings_version,observation_version,state,observed_at_utc_ms) VALUES(?1,'imap',?2,?3,?4,?5) ON CONFLICT(account_id,endpoint_kind) DO UPDATE SET observed_settings_version=excluded.observed_settings_version,observation_version=excluded.observation_version,state=excluded.state,observed_at_utc_ms=excluded.observed_at_utc_ms",
                    params![
                        account_id,
                        expected_endpoint_version as i64,
                        next_observation as i64,
                        observed,
                        now,
                    ],
                ))?;
                Ok(InitialImapProbeReceipt {
                    account_version: next_account,
                    endpoint_version: expected_endpoint_version,
                    observation_version: next_observation,
                })
            })
        }
    }
    /// Loads one consistent, validated display snapshot, never a credential or proof.
    pub fn load_imap_provisioning_state(
        &self,
        account_id: &str,
    ) -> Result<ImapProvisioningState, ErrorCode> {
        crate::mail_connection::input_account(account_id)?;
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(TransactionBehavior::Deferred, |tx| {
                read_state(tx, account_id)
            })
        }
    }
}

fn validate_capabilities(values: &[String]) -> Result<(), ErrorCode> {
    const KNOWN: [&str; 6] = [
        "imap.condstore",
        "imap.idle",
        "imap.move",
        "imap.qresync",
        "imap.special_use",
        "imap.uidplus",
    ];
    if values.len() > KNOWN.len()
        || values.windows(2).any(|pair| pair[0] >= pair[1])
        || values.iter().any(|value| !KNOWN.contains(&value.as_str()))
    {
        return Err(ErrorCode::InternalError);
    }
    Ok(())
}

#[cfg(windows)]
fn db<T>(result: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    result.map_err(|_| ErrorCode::InternalError)
}

#[cfg(windows)]
fn has_matching_imap_endpoint(
    tx: &Transaction<'_>,
    candidate: &ImapEndpointConfig,
) -> Result<bool, ErrorCode> {
    let mut statement = db(
        tx.prepare("SELECT account_id FROM mail_connection_settings WHERE endpoint_kind='imap'")
    )?;
    let mut rows = db(statement.query([]))?;
    let mut matched = false;
    // Stream one bounded endpoint at a time. Do not turn a damaged, unrelated
    // endpoint into evidence that this target is absent, or stop at an early match.
    while let Some(row) = db(rows.next())? {
        let account_id = db(text_column(row, 0, 128))?;
        check_account(tx, account_id).map_err(|_| ErrorCode::InternalError)?;
        let existing = read_imap_endpoint(tx, account_id).map_err(|_| ErrorCode::InternalError)?;
        matched |= existing.host() == candidate.host()
            && existing.login_name().as_bytes() == candidate.login_name().as_bytes();
    }
    Ok(matched)
}

#[cfg(windows)]
fn read_imap_endpoint(
    tx: &Transaction<'_>,
    account_id: &str,
) -> Result<ImapEndpointConfig, ErrorCode> {
    let stored =
        read_endpoint(tx, account_id, MailEndpointKind::Imap)?.ok_or(ErrorCode::NotFound)?;
    let json = serde_json::json!({"schema_version":1,"account_id":account_id,"imap":stored.json});
    let bytes = serde_json::to_vec(&json).map_err(|_| ErrorCode::InternalError)?;
    Ok(MailConnectionConfig::from_json(&bytes)
        .map_err(|_| ErrorCode::InternalError)?
        .imap()
        .clone())
}

#[cfg(windows)]
fn read_state(tx: &Transaction<'_>, account_id: &str) -> Result<ImapProvisioningState, ErrorCode> {
    check_account(tx, account_id)?;
    let (status, version, capabilities) = db(tx.query_row(
        "SELECT account_id,provider,primary_address,display_name,color,account_number,status,capabilities,settings_version FROM accounts WHERE account_id=?1",
        [account_id],
        |row| {
            let summary=crate::accounts::read_account(row,true)?;
            let version: i64 = row.get(8)?;
            // Preserve stored order here: the ordinary display reader sorts its copy.
            let capabilities: Vec<String> = serde_json::from_str(text_column(row, 7, 16 * 1024)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            if version <= 0 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            Ok((summary.status, version as u64, capabilities))
        },
    ))?;
    validate_capabilities(&capabilities)?;
    let endpoint = read_imap_endpoint(tx, account_id)?;
    let observation = read_observation(
        tx,
        account_id,
        MailEndpointKind::Imap,
        endpoint.settings_version(),
    )?;
    Ok(ImapProvisioningState {
        account_id: account_id.to_owned(),
        status,
        account_version: version,
        endpoint,
        capabilities,
        observation,
    })
}
#[cfg(test)]
mod tests;
