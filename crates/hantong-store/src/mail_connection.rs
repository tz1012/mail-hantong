//! Typed, bounded access to encrypted IMAP/SMTP settings. No credential values.
use crate::Store;
use hantong_contracts::ErrorCode;
use hantong_domain::mail_connection::{
    Authentication, ImapEndpointConfig, MailConnectionConfig, SmtpEndpointConfig, TlsMode,
};
#[cfg(windows)]
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailEndpointKind {
    Imap,
    Smtp,
}
impl MailEndpointKind {
    #[cfg(windows)]
    fn as_str(self) -> &'static str {
        match self {
            Self::Imap => "imap",
            Self::Smtp => "smtp",
        }
    }
    fn code(self) -> u8 {
        match self {
            Self::Imap => 0,
            Self::Smtp => 1,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailConnectionState {
    Ready,
    AuthRequired,
    TlsFailed,
    Offline,
    Error,
}
#[cfg(windows)]
impl MailConnectionState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::AuthRequired => "auth_required",
            Self::TlsFailed => "tls_failed",
            Self::Offline => "offline",
            Self::Error => "error",
        }
    }
    fn parse(s: &str) -> Result<Self, ErrorCode> {
        match s {
            "ready" => Ok(Self::Ready),
            "auth_required" => Ok(Self::AuthRequired),
            "tls_failed" => Ok(Self::TlsFailed),
            "offline" => Ok(Self::Offline),
            "error" => Ok(Self::Error),
            _ => Err(ErrorCode::InternalError),
        }
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct MailMutation {
    operation_id: String,
    idempotency_key: String,
    expected_version: u64,
    payload_hash: [u8; 32],
}
impl fmt::Debug for MailMutation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MailMutation { <redacted> }")
    }
}
impl MailMutation {
    pub fn new(
        operation_id: String,
        idempotency_key: String,
        expected_version: u64,
        payload_hash: [u8; 32],
    ) -> Result<Self, ErrorCode> {
        if !valid_id(&operation_id)
            || !valid_id(&idempotency_key)
            || expected_version >= i64::MAX as u64
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            operation_id,
            idempotency_key,
            expected_version,
            payload_hash,
        })
    }
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }
    pub fn expected_version(&self) -> u64 {
        self.expected_version
    }
    pub fn payload_hash(&self) -> &[u8; 32] {
        &self.payload_hash
    }
}
/// An observation is display data, never evidence of authority to send.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailObservation {
    pub endpoint_kind: MailEndpointKind,
    pub observed_settings_version: u64,
    pub observation_version: u64,
    pub state: MailConnectionState,
    pub observed_at_utc_ms: i64,
}
#[derive(Clone, Copy)]
struct Endpoint<'a> {
    kind: MailEndpointKind,
    version: u64,
    host: &'a str,
    port: u16,
    tls: TlsMode,
    login: &'a str,
    auth: Authentication,
}
impl<'a> From<&'a ImapEndpointConfig> for Endpoint<'a> {
    fn from(e: &'a ImapEndpointConfig) -> Self {
        Self {
            kind: MailEndpointKind::Imap,
            version: e.settings_version(),
            host: e.host(),
            port: e.port(),
            tls: e.tls(),
            login: e.login_name(),
            auth: e.authentication(),
        }
    }
}
impl<'a> From<&'a SmtpEndpointConfig> for Endpoint<'a> {
    fn from(e: &'a SmtpEndpointConfig) -> Self {
        Self {
            kind: MailEndpointKind::Smtp,
            version: e.settings_version(),
            host: e.host(),
            port: e.port(),
            tls: e.tls(),
            login: e.login_name(),
            auth: e.authentication(),
        }
    }
}
pub fn imap_payload_hash(
    account: &str,
    endpoint: &ImapEndpointConfig,
    expected: u64,
) -> Result<[u8; 32], ErrorCode> {
    payload_hash(account, endpoint.into(), expected)
}
pub fn smtp_payload_hash(
    account: &str,
    endpoint: &SmtpEndpointConfig,
    expected: u64,
) -> Result<[u8; 32], ErrorCode> {
    payload_hash(account, endpoint.into(), expected)
}

fn payload_hash(account: &str, e: Endpoint<'_>, expected: u64) -> Result<[u8; 32], ErrorCode> {
    if !valid_id(account) || expected >= i64::MAX as u64 || e.version != expected + 1 {
        return Err(ErrorCode::InvalidInput);
    }
    let mut bytes = b"mail-hantong.mail-connection.v1\0".to_vec();
    let push = |b: &mut Vec<u8>, v: &str| {
        b.extend_from_slice(&(v.len() as u32).to_be_bytes());
        b.extend_from_slice(v.as_bytes());
    };
    // All strings are bounded before allocation: account here, endpoint in Domain.
    push(&mut bytes, account);
    bytes.push(e.kind.code());
    bytes.extend_from_slice(&expected.to_be_bytes());
    bytes.extend_from_slice(&e.version.to_be_bytes());
    push(&mut bytes, e.host);
    bytes.extend_from_slice(&e.port.to_be_bytes());
    bytes.push(match e.tls {
        TlsMode::ImplicitTls => 0,
        TlsMode::StarttlsRequired => 1,
    });
    push(&mut bytes, e.login);
    bytes.push(match e.auth {
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
fn valid_id(s: &str) -> bool {
    (1..=128).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
pub(super) fn input_account(s: &str) -> Result<(), ErrorCode> {
    if valid_id(s) {
        Ok(())
    } else {
        Err(ErrorCode::InvalidInput)
    }
}

impl Store {
    pub fn put_imap_connection(
        &self,
        account: &str,
        endpoint: &ImapEndpointConfig,
        mutation: &MailMutation,
    ) -> Result<u64, ErrorCode> {
        self.put_connection(account, endpoint.into(), mutation)
    }
    pub fn put_smtp_connection(
        &self,
        account: &str,
        endpoint: &SmtpEndpointConfig,
        mutation: &MailMutation,
    ) -> Result<u64, ErrorCode> {
        self.put_connection(account, endpoint.into(), mutation)
    }

    pub fn load_mail_connection(
        &self,
        account: &str,
    ) -> Result<Option<MailConnectionConfig>, ErrorCode> {
        input_account(account)?;
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(TransactionBehavior::Deferred,|tx| {
                check_account(tx,account)?;
                let imap=read_endpoint(tx,account,MailEndpointKind::Imap)?;
                let smtp=read_endpoint(tx,account,MailEndpointKind::Smtp)?;
                let Some(imap)=imap else { return if smtp.is_some() { Err(ErrorCode::InternalError) } else { Ok(None) }; };
                let value=serde_json::json!({"schema_version":1,"account_id":account,"imap":imap.json,"smtp":smtp.map(|v|v.json)});
                let bytes=serde_json::to_vec(&value).map_err(|_|ErrorCode::InternalError)?;
                MailConnectionConfig::from_json(&bytes).map(Some).map_err(|_|ErrorCode::InternalError)
            })
        }
    }

    /// Internal core observation persistence; not exposed as an IPC tool.
    pub fn record_mail_observation(
        &self,
        account: &str,
        kind: MailEndpointKind,
        settings: u64,
        expected: u64,
        state: MailConnectionState,
    ) -> Result<u64, ErrorCode> {
        input_account(account)?;
        if settings == 0 || settings > i64::MAX as u64 || expected > i64::MAX as u64 {
            return Err(ErrorCode::InvalidInput);
        }
        #[cfg(not(windows))]
        {
            let _ = (kind, state);
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(TransactionBehavior::Immediate,|tx| {
                check_account(tx,account)?;
                let endpoint=read_endpoint(tx,account,kind)?.ok_or(ErrorCode::NotFound)?;
                let previous=read_observation(tx,account,kind,endpoint.version)?;
                if settings!=endpoint.version||previous.as_ref().map_or(0,|v|v.observation_version)!=expected||expected==i64::MAX as u64 { return Err(ErrorCode::Conflict); }
                let now=db(crate::utc_millis())?;
                if previous.as_ref().is_some_and(|v|now<v.observed_at_utc_ms) { return Err(ErrorCode::InternalError); }
                let next=expected+1;
                db(tx.execute("INSERT INTO mail_connection_observations(account_id,endpoint_kind,observed_settings_version,observation_version,state,observed_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(account_id,endpoint_kind) DO UPDATE SET observed_settings_version=excluded.observed_settings_version,observation_version=excluded.observation_version,state=excluded.state,observed_at_utc_ms=excluded.observed_at_utc_ms",params![account,kind.as_str(),settings as i64,next as i64,state.as_str(),now]))?;
                Ok(next)
            })
        }
    }
    pub fn load_mail_observation(
        &self,
        account: &str,
        kind: MailEndpointKind,
    ) -> Result<Option<MailObservation>, ErrorCode> {
        input_account(account)?;
        #[cfg(not(windows))]
        {
            let _ = kind;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(TransactionBehavior::Deferred, |tx| {
                check_account(tx, account)?;
                let Some(endpoint) = read_endpoint(tx, account, kind)? else {
                    return Ok(None);
                };
                read_observation(tx, account, kind, endpoint.version)
            })
        }
    }
    fn put_connection(
        &self,
        account: &str,
        e: Endpoint<'_>,
        m: &MailMutation,
    ) -> Result<u64, ErrorCode> {
        let hash = payload_hash(account, e, m.expected_version)?;
        if hash != m.payload_hash {
            return Err(ErrorCode::InvalidInput);
        }
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(TransactionBehavior::Immediate,|tx| {
                if let Some(version)=replay(tx,account,e.kind,m)? { return Ok(version); }
                check_account(tx,account)?;
                if e.kind==MailEndpointKind::Smtp&&read_endpoint(tx,account,MailEndpointKind::Imap)?.is_none() { return Err(ErrorCode::Conflict); }
                let previous=read_endpoint(tx,account,e.kind)?;
                if previous.as_ref().map_or(0,|v|v.version)!=m.expected_version { return Err(ErrorCode::Conflict); }
                let count:i64=db(tx.query_row("SELECT count(*) FROM mail_connection_operations",[],|r|r.get(0)))?;
                if count>=100000 { return Err(ErrorCode::QuotaExceeded); }
                let now=db(crate::utc_millis())?;
                if previous.as_ref().is_some_and(|v|now<v.updated) { return Err(ErrorCode::InternalError); }
                let tls=match e.tls { TlsMode::ImplicitTls=>"implicit_tls",TlsMode::StarttlsRequired=>"starttls_required" };
                let auth=match e.auth { Authentication::AppPassword=>"app_password",Authentication::Oauth2=>"oauth2" };
                let changed=if previous.is_none() {
                    db(tx.execute("INSERT INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![account,e.kind.as_str(),e.version as i64,e.host,e.port,tls,e.login,auth,now]))?
                } else {
                    db(tx.execute("UPDATE mail_connection_settings SET settings_version=?3,host=?4,port=?5,tls=?6,login_name=?7,authentication=?8,updated_at_utc_ms=?9 WHERE account_id=?1 AND endpoint_kind=?2 AND settings_version=?10",params![account,e.kind.as_str(),e.version as i64,e.host,e.port,tls,e.login,auth,now,m.expected_version as i64]))?
                };
                if changed!=1 { return Err(ErrorCode::Conflict); }
                db(tx.execute("DELETE FROM mail_connection_observations WHERE account_id=?1 AND endpoint_kind=?2",params![account,e.kind.as_str()]))?;
                db(tx.execute("INSERT INTO mail_connection_operations(operation_id,account_id,idempotency_key,endpoint_kind,payload_hash,expected_version,result_version,completed_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![m.operation_id,account,m.idempotency_key,e.kind.as_str(),m.payload_hash.as_slice(),m.expected_version as i64,e.version as i64,now]))?;
                Ok(e.version)
            })
        }
    }
    #[cfg(windows)]
    pub(super) fn mail_transaction<T>(
        &self,
        behavior: TransactionBehavior,
        operation: impl FnOnce(&Transaction<'_>) -> Result<T, ErrorCode>,
    ) -> Result<T, ErrorCode> {
        self.with_connection(|c| {
            let tx = Transaction::new_unchecked(c, behavior)?;
            let result = operation(&tx);
            if result.is_ok() {
                tx.commit()?;
            }
            Ok(result)
        })?
    }
}
#[cfg(windows)]
fn db<T>(result: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    result.map_err(|_| ErrorCode::InternalError)
}
#[cfg(windows)]
fn text(row: &rusqlite::Row<'_>, index: usize, max: usize) -> rusqlite::Result<String> {
    let value = row.get_ref(index)?.as_str()?;
    if value.len() > max {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(value.to_owned())
}
#[cfg(windows)]
pub(super) fn check_account(tx: &Transaction<'_>, account: &str) -> Result<(), ErrorCode> {
    match db(tx
        .query_row(
            "SELECT provider FROM accounts WHERE account_id=?1",
            [account],
            |r| text(r, 0, 16),
        )
        .optional())?
    .as_deref()
    {
        None => Err(ErrorCode::NotFound),
        Some("imap") => Ok(()),
        Some("gmail" | "graph" | "archive") => Err(ErrorCode::CapabilityUnsupported),
        _ => Err(ErrorCode::InternalError),
    }
}
#[cfg(windows)]
pub(super) struct StoredEndpoint {
    version: u64,
    updated: i64,
    pub(super) json: serde_json::Value,
}
#[cfg(windows)]
pub(super) fn read_endpoint(
    tx: &Transaction<'_>,
    account: &str,
    kind: MailEndpointKind,
) -> Result<Option<StoredEndpoint>, ErrorCode> {
    let row=db(tx.query_row("SELECT settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms FROM mail_connection_settings WHERE account_id=?1 AND endpoint_kind=?2",params![account,kind.as_str()],|r| {
        Ok((serde_json::json!({"settings_version":r.get::<_,i64>(0)?,"host":text(r,1,253)?,"port":r.get::<_,i64>(2)?,"tls":text(r,3,32)?,"login_name":text(r,4,1280)?,"authentication":text(r,5,32)?}),r.get::<_,i64>(6)?))
    }).optional())?;
    let Some((json, updated)) = row else {
        return Ok(None);
    };
    if updated <= 0 {
        return Err(ErrorCode::InternalError);
    }
    // Both endpoint roles have identical field validation. Do not normalize corrupt storage.
    let wrapped = serde_json::json!({"schema_version":1,"account_id":account,"imap":json});
    let bytes = serde_json::to_vec(&wrapped).map_err(|_| ErrorCode::InternalError)?;
    let valid = MailConnectionConfig::from_json(&bytes).map_err(|_| ErrorCode::InternalError)?;
    if serde_json::to_value(valid.imap()).map_err(|_| ErrorCode::InternalError)? != json {
        return Err(ErrorCode::InternalError);
    }
    Ok(Some(StoredEndpoint {
        version: valid.imap().settings_version(),
        updated,
        json,
    }))
}
#[cfg(windows)]
pub(super) fn read_observation(
    tx: &Transaction<'_>,
    account: &str,
    kind: MailEndpointKind,
    current: u64,
) -> Result<Option<MailObservation>, ErrorCode> {
    let row=db(tx.query_row("SELECT observed_settings_version,observation_version,state,observed_at_utc_ms FROM mail_connection_observations WHERE account_id=?1 AND endpoint_kind=?2",params![account,kind.as_str()],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,text(r,2,32)?,r.get::<_,i64>(3)?))).optional())?;
    let Some((settings, version, state, time)) = row else {
        return Ok(None);
    };
    if settings <= 0 || version <= 0 || time <= 0 || settings as u64 != current {
        return Err(ErrorCode::InternalError);
    }
    Ok(Some(MailObservation {
        endpoint_kind: kind,
        observed_settings_version: settings as u64,
        observation_version: version as u64,
        state: MailConnectionState::parse(&state)?,
        observed_at_utc_ms: time,
    }))
}
#[cfg(windows)]
pub(super) fn replay(
    tx: &Transaction<'_>,
    account: &str,
    kind: MailEndpointKind,
    m: &MailMutation,
) -> Result<Option<u64>, ErrorCode> {
    let mut stmt=db(tx.prepare("SELECT operation_id,account_id,idempotency_key,endpoint_kind,payload_hash,expected_version,result_version,completed_at_utc_ms FROM mail_connection_operations WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3)"))?;
    let mut rows = db(stmt.query(params![m.operation_id, account, m.idempotency_key]))?;
    let mut result = None;
    while let Some(row) = db(rows.next())? {
        let op = db(text(row, 0, 128))?;
        let a = db(text(row, 1, 128))?;
        let key = db(text(row, 2, 128))?;
        let endpoint = db(text(row, 3, 4))?;
        let hash = db(row.get_ref(4))?
            .as_blob()
            .map_err(|_| ErrorCode::InternalError)?;
        let expected: i64 = db(row.get(5))?;
        let version: i64 = db(row.get(6))?;
        let time: i64 = db(row.get(7))?;
        if !valid_id(&op)
            || !valid_id(&a)
            || !valid_id(&key)
            || !matches!(endpoint.as_str(), "imap" | "smtp")
            || hash.len() != 32
            || !(0..i64::MAX).contains(&expected)
            || version != expected + 1
            || time <= 0
        {
            return Err(ErrorCode::InternalError);
        }
        if result.is_some()
            || op != m.operation_id
            || a != account
            || key != m.idempotency_key
            || endpoint != kind.as_str()
            || hash != m.payload_hash
            || expected as u64 != m.expected_version
        {
            return Err(ErrorCode::Conflict);
        }
        result = Some(version as u64);
    }
    Ok(result)
}
#[cfg(test)]
mod tests;
