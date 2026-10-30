//! Persistence for core-coordinated registration, never a user-presence issuer.
use crate::{ClientKind, RegistrationRecord, RegistrationRequest, RegistrationRequestState, Store};
use hantong_contracts::ErrorCode;
use std::fmt;

#[cfg(windows)]
use crate::{
    RegistrationState,
    registrations::{blob_column, read_registration, read_request, text_column},
};
#[cfg(windows)]
use rusqlite::{OptionalExtension, TransactionBehavior, params};

pub const DEFAULT_CLI_LABEL: &str = "hantong CLI (default)";
pub const DEFAULT_MCP_LABEL: &str = "mail-hantong MCP";
pub const DEFAULT_GUI_LABEL: &str = "mail-hantong GUI";

#[derive(Clone, Copy, Eq, PartialEq)]
enum RegistrationFlavor {
    Cli,
    Gui,
    Mcp,
}
impl RegistrationFlavor {
    fn kind(self) -> ClientKind {
        match self {
            Self::Cli => ClientKind::Cli,
            Self::Gui => ClientKind::Gui,
            Self::Mcp => ClientKind::Mcp,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Cli => DEFAULT_CLI_LABEL,
            Self::Gui => DEFAULT_GUI_LABEL,
            Self::Mcp => DEFAULT_MCP_LABEL,
        }
    }
    fn tools(self) -> Vec<String> {
        match self {
            Self::Cli => vec!["mail.accounts.list".into()],
            Self::Gui => hantong_contracts::auth::gui_read_tools(),
            Self::Mcp => hantong_contracts::auth::mcp_read_tools(),
        }
    }
    fn matches_request(self, request: &RegistrationRequest) -> bool {
        request.client_kind == self.kind()
            && request.display_label == self.label()
            && request.scope.accounts().len() <= 128
            && (request.scope.tools() == self.tools()
                || (self == Self::Mcp
                    && request.scope.tools() == hantong_contracts::auth::mcp_mail_tools()))
    }
}

pub struct CliRegistrationStage {
    expected_request: RegistrationRequest,
    registration_id: [u8; 16],
    client_public_key: [u8; 72],
    expected_server_public_key_hash: [u8; 32],
    expected_mcp_replacement: Option<RegistrationRecord>,
}
impl fmt::Debug for CliRegistrationStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CliRegistrationStage { <redacted> }")
    }
}
impl CliRegistrationStage {
    /// Validates input data, not proof that a person approved this registration.
    pub fn new(
        expected_request: RegistrationRequest,
        registration_id: [u8; 16],
        client_public_key: [u8; 72],
        expected_server_public_key_hash: [u8; 32],
    ) -> Result<Self, ErrorCode> {
        Self::new_for(
            RegistrationFlavor::Cli,
            expected_request,
            registration_id,
            client_public_key,
            expected_server_public_key_hash,
        )
    }
    fn new_for(
        flavor: RegistrationFlavor,
        expected_request: RegistrationRequest,
        registration_id: [u8; 16],
        client_public_key: [u8; 72],
        expected_server_public_key_hash: [u8; 32],
    ) -> Result<Self, ErrorCode> {
        if !flavor.matches_request(&expected_request)
            || expected_request.state != RegistrationRequestState::Pending
            || expected_request.request_id == [0; 32]
            || expected_request.version == 0
            || expected_request.version >= i64::MAX as u64
            || expected_request.created_at_utc_ms <= 0
            || expected_request
                .expires_at_utc_ms
                .checked_sub(expected_request.created_at_utc_ms)
                .is_none_or(|ttl| !(1..=300_000).contains(&ttl))
            || registration_id == [0; 16]
            || expected_server_public_key_hash == [0; 32]
            || !public_header(&client_public_key)
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            expected_request,
            registration_id,
            client_public_key,
            expected_server_public_key_hash,
            expected_mcp_replacement: None,
        })
    }
}
pub struct GuiRegistrationStage {
    inner: CliRegistrationStage,
}
impl fmt::Debug for GuiRegistrationStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GuiRegistrationStage { <redacted> }")
    }
}
impl GuiRegistrationStage {
    /// Fixed GUI registration data; this value does not prove user presence.
    pub fn new(
        expected_request: RegistrationRequest,
        registration_id: [u8; 16],
        client_public_key: [u8; 72],
        expected_server_public_key_hash: [u8; 32],
    ) -> Result<Self, ErrorCode> {
        Ok(Self {
            inner: CliRegistrationStage::new_for(
                RegistrationFlavor::Gui,
                expected_request,
                registration_id,
                client_public_key,
                expected_server_public_key_hash,
            )?,
        })
    }
}
pub struct McpRegistrationStage {
    inner: CliRegistrationStage,
}
impl fmt::Debug for McpRegistrationStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("McpRegistrationStage { <redacted> }")
    }
}
impl McpRegistrationStage {
    /// Fixed GUI registration data; this value does not prove user presence.
    pub fn new(
        expected_request: RegistrationRequest,
        registration_id: [u8; 16],
        client_public_key: [u8; 72],
        expected_server_public_key_hash: [u8; 32],
    ) -> Result<Self, ErrorCode> {
        if expected_request.scope.tools() != hantong_contracts::auth::mcp_read_tools() {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            inner: CliRegistrationStage::new_for(
                RegistrationFlavor::Mcp,
                expected_request,
                registration_id,
                client_public_key,
                expected_server_public_key_hash,
            )?,
        })
    }
}
pub struct McpMailRegistrationStage {
    inner: CliRegistrationStage,
}
impl fmt::Debug for McpMailRegistrationStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("McpMailRegistrationStage(<redacted>)")
    }
}
impl McpMailRegistrationStage {
    pub fn new(
        request: RegistrationRequest,
        id: [u8; 16],
        key: [u8; 72],
        server_hash: [u8; 32],
        replaced: Option<RegistrationRecord>,
    ) -> Result<Self, ErrorCode> {
        if request.scope.tools() != hantong_contracts::auth::mcp_mail_tools() {
            return Err(ErrorCode::InvalidInput);
        }
        if let Some(old) = &replaced
            && (old.client_kind != ClientKind::Mcp
                || old.display_label != DEFAULT_MCP_LABEL
                || old.scope.tools() != hantong_contracts::auth::mcp_read_tools()
                || old.state != crate::RegistrationState::Active
                || !old.read_only)
        {
            return Err(ErrorCode::Conflict);
        }
        let mut inner =
            CliRegistrationStage::new_for(RegistrationFlavor::Mcp, request, id, key, server_hash)?;
        inner.expected_mcp_replacement = replaced;
        Ok(Self { inner })
    }
}
impl Store {
    pub fn list_orphan_cli_registration_requests(
        &self,
    ) -> Result<Vec<RegistrationRequest>, ErrorCode> {
        self.list_orphan_registration_requests(RegistrationFlavor::Cli)
    }
    pub fn list_orphan_gui_registration_requests(
        &self,
    ) -> Result<Vec<RegistrationRequest>, ErrorCode> {
        self.list_orphan_registration_requests(RegistrationFlavor::Gui)
    }
    pub fn list_orphan_mcp_registration_requests(
        &self,
    ) -> Result<Vec<RegistrationRequest>, ErrorCode> {
        self.list_orphan_registration_requests(RegistrationFlavor::Mcp)
    }
    fn list_orphan_registration_requests(
        &self,
        flavor: RegistrationFlavor,
    ) -> Result<Vec<RegistrationRequest>, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = flavor;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.registration_transaction(TransactionBehavior::Deferred, |tx| {
                orphan_requests(tx, flavor)
            })
        }
    }
    /// Returns ownership data, never permission to delete an arbitrary selector.
    pub fn registration_key_is_linked(
        &self,
        request_id: [u8; 32],
        public_key: [u8; 72],
    ) -> Result<bool, ErrorCode> {
        if request_id == [0; 32] || !public_header(&public_key) {
            return Err(ErrorCode::InvalidInput);
        }
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            let id = hantong_platform::registration_id_for_request(request_id)?;
            self.registration_transaction(TransactionBehavior::Deferred,|tx| {
                // Validate the bounded reservation set and all corresponding
                // registration links before interpreting an absent match.
                orphan_requests(tx, RegistrationFlavor::Cli)?;
                orphan_requests(tx, RegistrationFlavor::Gui)?;
                orphan_requests(tx, RegistrationFlavor::Mcp)?;
                let request=read_request(tx,&request_id).map_err(internal)?.ok_or(ErrorCode::NotFound)?;
                if !RegistrationFlavor::Cli.matches_request(&request) && !RegistrationFlavor::Gui.matches_request(&request) && !RegistrationFlavor::Mcp.matches_request(&request) {return Err(ErrorCode::Conflict);}
                let mut statement=tx.prepare("SELECT registration_id FROM client_registrations WHERE registration_id=?1 OR request_id=?2 OR public_key=?3 LIMIT 4").map_err(internal)?;
                let ids=statement.query_map(params![id.as_slice(),request_id.as_slice(),public_key.as_slice()],|r|blob_column::<16>(r,0)).map_err(internal)?.collect::<rusqlite::Result<Vec<_>>>().map_err(internal)?;
                if ids.len()>3 {return Err(ErrorCode::InternalError);}
                for id in &ids {read_registration(tx,id).map_err(internal)?.ok_or(ErrorCode::InternalError)?;}
                Ok(!ids.is_empty())
            })
        }
    }
    pub fn load_default_cli_registration(&self) -> Result<Option<RegistrationRecord>, ErrorCode> {
        self.load_fixed_registration(RegistrationFlavor::Cli)
    }
    pub fn load_gui_registration(&self) -> Result<Option<RegistrationRecord>, ErrorCode> {
        self.load_fixed_registration(RegistrationFlavor::Gui)
    }
    /// Narrow, revocable external-AI disclosure consent; absence never means all accounts.
    pub fn check_mcp_ai_consent(
        &self,
        registration_id: [u8; 16],
        expected_epoch: u64,
        account_ids: &[String],
    ) -> Result<(), ErrorCode> {
        if registration_id == [0; 16] || !(1..=i64::MAX as u64).contains(&expected_epoch) {
            return Err(ErrorCode::InvalidInput);
        }
        if account_ids.is_empty() {
            return Err(ErrorCode::ScopeDenied);
        }
        let requested = hantong_contracts::auth::RequestedScope::new(account_ids.to_vec(), vec![])?;
        #[cfg(not(windows))]
        {
            let _ = requested;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.registration_transaction(TransactionBehavior::Deferred,|tx|{
                let record=read_registration(tx,&registration_id).map_err(internal)?.ok_or(ErrorCode::ClientNotRegistered)?;
                if record.client_kind!=ClientKind::Mcp {return Err(ErrorCode::PolicyBlocked);}
                if record.state!=RegistrationState::Active{return Err(ErrorCode::ClientSuspended);}
                if record.profile_epoch!=expected_epoch{return Err(ErrorCode::AuthRequired);}
                if requested.accounts().iter().any(|account|record.scope.accounts().binary_search(account).is_err()) {return Err(ErrorCode::ScopeDenied);}
                let approved:i64=tx.query_row("SELECT approved_at_utc_ms FROM client_registrations WHERE registration_id=?1",[registration_id.as_slice()],|row|row.get(0)).map_err(internal)?;
                let mut statement=tx.prepare("SELECT account_id,registration_request_id,approved_at_utc_ms FROM mcp_ai_consents WHERE registration_id=?1 AND profile_epoch=?2 ORDER BY account_id LIMIT 129").map_err(internal)?;
                let rows=statement.query_map(params![registration_id.as_slice(),expected_epoch as i64],|row|Ok((text_column(row,0,128)?.to_owned(),blob_column::<32>(row,1)?,row.get::<_,i64>(2)?))).map_err(internal)?.collect::<rusqlite::Result<Vec<_>>>().map_err(internal)?;
                if rows.len()>128 || rows.iter().any(|(account,request,time)|record.scope.accounts().binary_search(account).is_err() || request!=&record.request_id || *time!=approved || *time<=0){return Err(ErrorCode::InternalError);}
                if requested.accounts().iter().any(|account|!rows.iter().any(|(allowed,_,_)|allowed==account)){return Err(ErrorCode::PolicyBlocked);}
                Ok(())
            })
        }
    }

    pub fn load_mcp_registration(&self) -> Result<Option<RegistrationRecord>, ErrorCode> {
        self.load_fixed_registration(RegistrationFlavor::Mcp)
    }
    fn load_fixed_registration(
        &self,
        flavor: RegistrationFlavor,
    ) -> Result<Option<RegistrationRecord>, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = flavor;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.registration_transaction(TransactionBehavior::Deferred, |tx| {
                let mut statement=tx.prepare("SELECT registration_id FROM client_registrations WHERE ((client_kind=?1 AND display_label=?2) OR (?1 IN ('gui','mcp') AND (client_kind=?1 OR display_label=?2))) AND (?1<>'mcp' OR state<>'revoked') ORDER BY registration_id LIMIT 2").map_err(internal)?;
                let ids=statement.query_map(params![flavor.kind().as_str(),flavor.label()],|r|blob_column::<16>(r,0)).map_err(internal)?.collect::<rusqlite::Result<Vec<_>>>().map_err(internal)?;
                if ids.len()>1 {return Err(ErrorCode::Conflict);}
                ids.first().map(|id|{let record=read_registration(tx,id).map_err(internal)?.ok_or(ErrorCode::InternalError)?;if record.client_kind!=flavor.kind() || record.display_label!=flavor.label(){return Err(ErrorCode::InternalError)}Ok(record)}).transpose()
            })
        }
    }
    pub fn load_pending_default_cli_request(
        &self,
    ) -> Result<Option<RegistrationRequest>, ErrorCode> {
        self.load_pending_fixed_request(RegistrationFlavor::Cli)
    }
    pub fn load_pending_gui_request(&self) -> Result<Option<RegistrationRequest>, ErrorCode> {
        self.load_pending_fixed_request(RegistrationFlavor::Gui)
    }
    pub fn load_pending_mcp_request(&self) -> Result<Option<RegistrationRequest>, ErrorCode> {
        self.load_pending_fixed_request(RegistrationFlavor::Mcp)
    }
    fn load_pending_fixed_request(
        &self,
        flavor: RegistrationFlavor,
    ) -> Result<Option<RegistrationRequest>, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = flavor;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.registration_transaction(TransactionBehavior::Deferred, |tx| {
                let now=crate::utc_millis().map_err(internal)?;
                let mut statement=tx.prepare("SELECT request_id FROM client_registration_requests WHERE ((client_kind=?1 AND display_label=?2) OR (?1 IN ('gui','mcp') AND (client_kind=?1 OR display_label=?2))) AND state='pending' AND (expires_at_utc_ms>?3 OR created_at_utc_ms<=0 OR expires_at_utc_ms<=created_at_utc_ms OR expires_at_utc_ms-created_at_utc_ms>300000) ORDER BY request_id LIMIT 2").map_err(internal)?;
                let ids=statement.query_map(params![flavor.kind().as_str(),flavor.label(),now],|r|blob_column::<32>(r,0)).map_err(internal)?.collect::<rusqlite::Result<Vec<_>>>().map_err(internal)?;
                if ids.len()>1 {return Err(ErrorCode::Conflict);}
                ids.first().map(|id| {
                    let request=read_request(tx,id).map_err(internal)?.ok_or(ErrorCode::InternalError)?;
                    if !flavor.matches_request(&request) || request.request_id==[0;32] {return Err(ErrorCode::InternalError);}
                    request_clock(&request,now)?;
                    Ok(request)
                }).transpose()
            })
        }
    }
    /// Persists a Core-approved decision as provisioning, never directly active.
    /// The stage is input data; only the trusted coordinator may establish approval.
    pub fn stage_cli_registration(
        &self,
        stage: &CliRegistrationStage,
    ) -> Result<RegistrationRecord, ErrorCode> {
        self.stage_registration(stage, RegistrationFlavor::Cli)
    }
    pub fn stage_gui_registration(
        &self,
        stage: &GuiRegistrationStage,
    ) -> Result<RegistrationRecord, ErrorCode> {
        self.stage_registration(&stage.inner, RegistrationFlavor::Gui)
    }
    pub fn stage_mcp_registration(
        &self,
        stage: &McpRegistrationStage,
    ) -> Result<RegistrationRecord, ErrorCode> {
        self.stage_registration(&stage.inner, RegistrationFlavor::Mcp)
    }
    pub fn stage_mcp_mail_registration(
        &self,
        stage: &McpMailRegistrationStage,
    ) -> Result<RegistrationRecord, ErrorCode> {
        self.stage_registration(&stage.inner, RegistrationFlavor::Mcp)
    }
    pub fn load_mcp_replaced_registration(
        &self,
        id: [u8; 16],
    ) -> Result<Option<RegistrationRecord>, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = id;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.registration_transaction(TransactionBehavior::Deferred,|tx|{
            let old=tx.query_row("SELECT previous_registration_id FROM mcp_registration_replacements WHERE registration_id=?1",[id.as_slice()],|r|blob_column::<16>(r,0)).optional().map_err(internal)?;
            old.map(|id|read_registration(tx,&id).map_err(internal)?.ok_or(ErrorCode::InternalError)).transpose()
        })
        }
    }
    fn stage_registration(
        &self,
        stage: &CliRegistrationStage,
        flavor: RegistrationFlavor,
    ) -> Result<RegistrationRecord, ErrorCode> {
        if !flavor.matches_request(&stage.expected_request) {
            return Err(ErrorCode::InvalidInput);
        }
        #[cfg(not(windows))]
        {
            let _ = (
                &stage.expected_request,
                stage.registration_id,
                stage.client_public_key,
                stage.expected_server_public_key_hash,
            );
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            if stage.registration_id
                != hantong_platform::registration_id_for_request(stage.expected_request.request_id)?
            {
                return Err(ErrorCode::InvalidInput);
            }
            // CNG work happens before the write lock. The server snapshot is
            // checked again under the lock, including its key and CAS version.
            hantong_platform::validate_public_key(&stage.client_public_key)?;
            let server = self.registration_server_pin(stage.expected_server_public_key_hash)?;
            self.registration_transaction(TransactionBehavior::Immediate, |tx| {
                let request = read_request(tx, &stage.expected_request.request_id)
                    .map_err(internal)?.ok_or(ErrorCode::NotFound)?;
                if request != stage.expected_request {
                    return Err(ErrorCode::Conflict);
                }
                let now = crate::utc_millis().map_err(internal)?;
                request_clock(&request, now)?;
                same_server(tx, &server)?;
                for account in request.scope.accounts() {
                    let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1)", [account], |r| r.get(0)).map_err(internal)?;
                    if !exists { return Err(ErrorCode::NotFound); }
                }
                if let Some(old)=&stage.expected_mcp_replacement {
                    if flavor!=RegistrationFlavor::Mcp || request.scope.tools()!=hantong_contracts::auth::mcp_mail_tools(){return Err(ErrorCode::InvalidInput)}
                    let current=read_registration(tx,&old.registration_id).map_err(internal)?.ok_or(ErrorCode::Conflict)?;
                    if current!=*old || current.state!=RegistrationState::Active || current.scope.tools()!=hantong_contracts::auth::mcp_read_tools(){return Err(ErrorCode::Conflict)}
                    if tx.execute("UPDATE client_registrations SET state='revoked',revoked_at_utc_ms=?1,updated_at_utc_ms=?1,version=version+1 WHERE registration_id=?2 AND state='active' AND version=?3",params![now,old.registration_id.as_slice(),old.version as i64]).map_err(internal)?!=1{return Err(ErrorCode::Conflict)}
                }
                let duplicate: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM client_registrations WHERE registration_id=?1 OR request_id=?2 OR public_key=?3 OR (client_kind=?4 AND display_label=?5 AND (?4<>'mcp' OR state<>'revoked')) OR (?4 IN ('gui','mcp') AND (client_kind=?4 OR display_label=?5) AND (?4<>'mcp' OR state<>'revoked')))",
                    params![stage.registration_id.as_slice(),request.request_id.as_slice(),stage.client_public_key.as_slice(),flavor.kind().as_str(),flavor.label()], |r| r.get(0)
                ).map_err(internal)?;
                if duplicate { return Err(ErrorCode::Conflict); }
                let changed = tx.execute(
                    "UPDATE client_registration_requests SET state='approved',decision_channel='approval_helper',decided_at_utc_ms=?1,version=version+1 WHERE request_id=?2 AND state='pending' AND version=?3",
                    params![now,request.request_id.as_slice(),request.version as i64]
                ).map_err(internal)?;
                if changed != 1 { return Err(ErrorCode::Conflict); }
                tx.execute(
                    "INSERT INTO client_registrations(registration_id,request_id,client_kind,display_label,public_key,profile_epoch,state,read_only,requests_per_minute,approved_channel,approved_at_utc_ms,updated_at_utc_ms,revoked_at_utc_ms,version) VALUES(?1,?2,?3,?4,?5,1,'provisioning',?7,60,'approval_helper',?6,?6,NULL,1)",
                    params![stage.registration_id.as_slice(),request.request_id.as_slice(),flavor.kind().as_str(),flavor.label(),stage.client_public_key.as_slice(),now,i64::from(request.scope.tools()!=hantong_contracts::auth::mcp_mail_tools())]
                ).map_err(internal)?;
                for account in request.scope.accounts() {
                    tx.execute("INSERT INTO client_registration_accounts(registration_id,account_id) VALUES(?1,?2)", params![stage.registration_id.as_slice(),account]).map_err(internal)?;
                }
                for tool in request.scope.tools() {
                    tx.execute("INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,?2)", params![stage.registration_id.as_slice(),tool]).map_err(internal)?;
                }
                if let Some(old)=&stage.expected_mcp_replacement && tx.execute("INSERT INTO mcp_registration_replacements(registration_id,previous_registration_id,request_id) VALUES(?1,?2,?3)",params![stage.registration_id.as_slice(),old.registration_id.as_slice(),request.request_id.as_slice()]).map_err(internal)?!=1{return Err(ErrorCode::InternalError)}
                read_registration(tx, &stage.registration_id).map_err(internal)?.ok_or(ErrorCode::InternalError)
            })
        }
    }
    /// Core calls this only after publishing and matching the public profile.
    pub fn activate_cli_registration(
        &self,
        registration_id: [u8; 16],
        expected_version: u64,
        expected_server_public_key_hash: [u8; 32],
    ) -> Result<RegistrationRecord, ErrorCode> {
        self.activate_registration(
            RegistrationFlavor::Cli,
            registration_id,
            expected_version,
            expected_server_public_key_hash,
        )
    }
    pub fn activate_gui_registration(
        &self,
        registration_id: [u8; 16],
        expected_version: u64,
        expected_server_public_key_hash: [u8; 32],
    ) -> Result<RegistrationRecord, ErrorCode> {
        self.activate_registration(
            RegistrationFlavor::Gui,
            registration_id,
            expected_version,
            expected_server_public_key_hash,
        )
    }
    pub fn activate_mcp_registration(
        &self,
        registration_id: [u8; 16],
        expected_version: u64,
        expected_server_public_key_hash: [u8; 32],
    ) -> Result<RegistrationRecord, ErrorCode> {
        self.activate_registration(
            RegistrationFlavor::Mcp,
            registration_id,
            expected_version,
            expected_server_public_key_hash,
        )
    }
    fn activate_registration(
        &self,
        flavor: RegistrationFlavor,
        registration_id: [u8; 16],
        expected_version: u64,
        expected_server_public_key_hash: [u8; 32],
    ) -> Result<RegistrationRecord, ErrorCode> {
        if registration_id == [0; 16] || expected_server_public_key_hash == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        incrementable_version(expected_version)?;
        #[cfg(not(windows))]
        {
            let _ = flavor;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            let observed = self
                .load_registration(registration_id)?
                .ok_or(ErrorCode::NotFound)?;
            if observed.client_kind != flavor.kind() || observed.display_label != flavor.label() {
                return Err(ErrorCode::Conflict);
            }
            provisioning_version(&observed, expected_version)?;
            hantong_platform::validate_public_key(&observed.public_key)
                .map_err(|_| ErrorCode::InternalError)?;
            let server = self.registration_server_pin(expected_server_public_key_hash)?;
            self.registration_transaction(TransactionBehavior::Immediate, |tx| {
                let registration = read_registration(tx, &registration_id).map_err(internal)?.ok_or(ErrorCode::NotFound)?;
                provisioning_version(&registration, expected_version)?;
                if registration != observed { return Err(ErrorCode::Conflict); }
                let request = read_request(tx, &registration.request_id).map_err(internal)?.ok_or(ErrorCode::InternalError)?;
                if !flavor.matches_request(&request)
                    || registration.client_kind != flavor.kind()
                    || registration.display_label != flavor.label()
                    || registration.read_only != (registration.scope.tools()!=hantong_contracts::auth::mcp_mail_tools())
                    || registration.requests_per_minute != 60
                    || registration.approved_channel != "approval_helper"
                    || registration.scope != request.scope
                { return Err(ErrorCode::InternalError); }
                same_server(tx, &server)?;
                let (approved, updated): (i64,i64) = tx.query_row(
                    "SELECT approved_at_utc_ms,updated_at_utc_ms FROM client_registrations WHERE registration_id=?1",
                    [registration_id.as_slice()], |r| Ok((r.get(0)?,r.get(1)?))
                ).map_err(internal)?;
                let now = crate::utc_millis().map_err(internal)?;
                if now < approved || now < updated { return Err(ErrorCode::ApprovalExpired); }
                let changed = tx.execute(
                    "UPDATE client_registrations SET state='active',version=version+1,updated_at_utc_ms=?1 WHERE registration_id=?2 AND state='provisioning' AND version=?3",
                    params![now,registration_id.as_slice(),expected_version as i64]
                ).map_err(internal)?;
                if changed != 1 { return Err(ErrorCode::Conflict); }
                if flavor == RegistrationFlavor::Mcp {
                    for account in registration.scope.accounts() {
                        let inserted=tx.execute("INSERT INTO mcp_ai_consents(registration_id,account_id,profile_epoch,registration_request_id,approved_at_utc_ms) VALUES(?1,?2,?3,?4,?5)",
                            params![registration_id.as_slice(),account,registration.profile_epoch as i64,registration.request_id.as_slice(),approved]).map_err(internal)?;
                        if inserted!=1 { return Err(ErrorCode::InternalError); }
                    }
                }
                read_registration(tx,&registration_id).map_err(internal)?.ok_or(ErrorCode::InternalError)
            })
        }
    }
    /// Records an explicit trusted UI denial. Closing a review is not a denial.
    pub fn deny_cli_registration_request(
        &self,
        request_id: [u8; 32],
        expected_version: u64,
    ) -> Result<(), ErrorCode> {
        self.deny_registration_request(RegistrationFlavor::Cli, request_id, expected_version)
    }
    pub fn deny_gui_registration_request(
        &self,
        request_id: [u8; 32],
        expected_version: u64,
    ) -> Result<(), ErrorCode> {
        self.deny_registration_request(RegistrationFlavor::Gui, request_id, expected_version)
    }
    pub fn deny_mcp_registration_request(
        &self,
        request_id: [u8; 32],
        expected_version: u64,
    ) -> Result<(), ErrorCode> {
        self.deny_registration_request(RegistrationFlavor::Mcp, request_id, expected_version)
    }
    fn deny_registration_request(
        &self,
        flavor: RegistrationFlavor,
        request_id: [u8; 32],
        expected_version: u64,
    ) -> Result<(), ErrorCode> {
        if request_id == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        incrementable_version(expected_version)?;
        #[cfg(not(windows))]
        {
            let _ = flavor;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.registration_transaction(TransactionBehavior::Immediate, |tx| {
                let request = read_request(tx,&request_id).map_err(internal)?.ok_or(ErrorCode::NotFound)?;
                if request.state != RegistrationRequestState::Pending || request.version != expected_version || !flavor.matches_request(&request) {
                    return Err(ErrorCode::Conflict);
                }
                let now = crate::utc_millis().map_err(internal)?;
                request_clock(&request,now)?;
                let changed = tx.execute(
                    "UPDATE client_registration_requests SET state='denied',decision_channel='approval_helper',decided_at_utc_ms=?1,version=version+1 WHERE request_id=?2 AND state='pending' AND version=?3",
                    params![now,request_id.as_slice(),expected_version as i64]
                ).map_err(internal)?;
                if changed != 1 { return Err(ErrorCode::Conflict); }
                Ok(())
            })
        }
    }
    pub fn load_registration_by_request(
        &self,
        request_id: [u8; 32],
    ) -> Result<Option<RegistrationRecord>, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = request_id;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.registration_transaction(TransactionBehavior::Deferred, |tx| {
                let id = tx
                    .query_row(
                        "SELECT registration_id FROM client_registrations WHERE request_id=?1",
                        [request_id.as_slice()],
                        |r| blob_column::<16>(r, 0),
                    )
                    .optional()
                    .map_err(internal)?;
                id.map(|id| {
                    read_registration(tx, &id)
                        .map_err(internal)?
                        .ok_or(ErrorCode::InternalError)
                })
                .transpose()
            })
        }
    }
    pub fn list_provisioning_registrations(
        &self,
        limit: usize,
    ) -> Result<Vec<RegistrationRecord>, ErrorCode> {
        if !(1..=128).contains(&limit) {
            return Err(ErrorCode::InvalidInput);
        }
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.registration_transaction(TransactionBehavior::Deferred, |tx| {
                let mut statement = tx.prepare("SELECT registration_id FROM client_registrations WHERE state='provisioning' ORDER BY registration_id LIMIT ?1").map_err(internal)?;
                let ids = statement.query_map([limit as i64+1],|r| blob_column::<16>(r,0)).map_err(internal)?.collect::<rusqlite::Result<Vec<_>>>().map_err(internal)?;
                if ids.len() > limit { return Err(ErrorCode::QuotaExceeded); }
                ids.into_iter().map(|id| read_registration(tx,&id).map_err(internal)?.ok_or(ErrorCode::InternalError)).collect()
            })
        }
    }
    pub fn list_registrable_account_ids(&self) -> Result<Vec<String>, ErrorCode> {
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.registration_transaction(TransactionBehavior::Deferred, |tx| {
                let mut statement = tx
                    .prepare("SELECT account_id FROM accounts ORDER BY account_id LIMIT 129")
                    .map_err(internal)?;
                let accounts = statement
                    .query_map([], |r| Ok(text_column(r, 0, 128)?.to_owned()))
                    .map_err(internal)?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(internal)?;
                if accounts.len() > 128 {
                    return Err(ErrorCode::QuotaExceeded);
                }
                let scope = hantong_contracts::auth::RequestedScope::new(accounts, vec![])
                    .map_err(|_| ErrorCode::InternalError)?;
                Ok(scope.accounts().to_vec())
            })
        }
    }

    #[cfg(windows)]
    fn registration_transaction<T>(
        &self,
        behavior: TransactionBehavior,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, ErrorCode>,
    ) -> Result<T, ErrorCode> {
        self.with_connection(|connection| {
            let tx = rusqlite::Transaction::new_unchecked(connection, behavior)?;
            let result = operation(&tx);
            if result.is_ok() {
                tx.commit()?;
            }
            Ok(result)
        })?
    }

    #[cfg(windows)]
    fn registration_server_pin(&self, expected_hash: [u8; 32]) -> Result<ServerPin, ErrorCode> {
        let pin = self
            .with_connection(read_server_pin)?
            .ok_or(ErrorCode::Conflict)?;
        hantong_platform::validate_public_key(&pin.public_key)
            .map_err(|_| ErrorCode::InternalError)?;
        if hantong_platform::sha256(&pin.public_key)? != expected_hash {
            return Err(ErrorCode::Conflict);
        }
        Ok(pin)
    }
}

fn public_header(public_key: &[u8; 72]) -> bool {
    &public_key[..8] == b"ECS1\x20\0\0\0"
}
fn incrementable_version(version: u64) -> Result<(), ErrorCode> {
    if version == 0 || version > i64::MAX as u64 {
        return Err(ErrorCode::InvalidInput);
    }
    if version == i64::MAX as u64 {
        return Err(ErrorCode::Conflict);
    }
    Ok(())
}
#[cfg(windows)]
fn internal(_: rusqlite::Error) -> ErrorCode {
    ErrorCode::InternalError
}
#[cfg(windows)]
fn request_clock(request: &RegistrationRequest, now: i64) -> Result<(), ErrorCode> {
    if now < request.created_at_utc_ms || now >= request.expires_at_utc_ms {
        return Err(ErrorCode::ApprovalExpired);
    }
    Ok(())
}
#[cfg(windows)]
fn provisioning_version(registration: &RegistrationRecord, expected: u64) -> Result<(), ErrorCode> {
    if registration.state != RegistrationState::Provisioning
        || registration.version != expected
        || registration.profile_epoch != 1
    {
        return Err(ErrorCode::Conflict);
    }
    Ok(())
}

#[cfg(windows)]
#[derive(Eq, PartialEq)]
struct ServerPin {
    key_id: [u8; 32],
    public_key: [u8; 72],
    key_epoch: i64,
    version: i64,
    created: i64,
    updated: i64,
}
#[cfg(windows)]
fn read_server_pin(connection: &rusqlite::Connection) -> rusqlite::Result<Option<ServerPin>> {
    connection.query_row(
        "SELECT key_id,state,public_key,key_epoch,version,created_at_utc_ms,updated_at_utc_ms FROM server_identity WHERE singleton=1",[], |row| {
            let key_id = blob_column::<32>(row,0)?;
            let state = text_column(row,1,8)?;
            let key_epoch: i64 = row.get(3)?;
            let version: i64 = row.get(4)?;
            let created: i64 = row.get(5)?;
            let updated: i64 = row.get(6)?;
            if key_id == [0;32] || key_epoch <= 0 || version <= 0 || created <= 0 || updated < created { return Err(rusqlite::Error::InvalidQuery); }
            if state == "creating" && matches!(row.get_ref(2)?, rusqlite::types::ValueRef::Null) { return Ok(None); }
            if state != "active" { return Err(rusqlite::Error::InvalidQuery); }
            let public_key = blob_column::<72>(row,2)?;
            if !public_header(&public_key) { return Err(rusqlite::Error::InvalidQuery); }
            Ok(Some(ServerPin {key_id,public_key,key_epoch,version,created,updated}))
        }
    ).optional().map(Option::flatten)
}
#[cfg(windows)]
fn same_server(connection: &rusqlite::Connection, expected: &ServerPin) -> Result<(), ErrorCode> {
    if read_server_pin(connection).map_err(internal)?.as_ref() != Some(expected) {
        return Err(ErrorCode::Conflict);
    }
    Ok(())
}

/// Validates GUI provenance without calling the registration reader recursively.
#[cfg(windows)]
pub(super) fn validate_gui_registration(
    connection: &rusqlite::Connection,
    record: &RegistrationRecord,
) -> rusqlite::Result<()> {
    if record.client_kind != ClientKind::Gui && record.display_label != DEFAULT_GUI_LABEL {
        return Ok(());
    }
    let request =
        read_request(connection, &record.request_id)?.ok_or(rusqlite::Error::InvalidQuery)?;
    if !RegistrationFlavor::Gui.matches_request(&request)
        || request.state != RegistrationRequestState::Approved
        || record.client_kind != ClientKind::Gui
        || record.display_label != DEFAULT_GUI_LABEL
        || record.scope != request.scope
        || !record.read_only
        || record.requests_per_minute != 60
        || record.approved_channel != "approval_helper"
        || record.profile_epoch != 1
        || record.registration_id
            != hantong_platform::registration_id_for_request(record.request_id)
                .map_err(|_| rusqlite::Error::InvalidQuery)?
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}
#[cfg(windows)]
pub(super) fn validate_mcp_registration(
    connection: &rusqlite::Connection,
    record: &RegistrationRecord,
) -> rusqlite::Result<()> {
    if record.client_kind != ClientKind::Mcp && record.display_label != DEFAULT_MCP_LABEL {
        return Ok(());
    }
    let request =
        read_request(connection, &record.request_id)?.ok_or(rusqlite::Error::InvalidQuery)?;
    if !RegistrationFlavor::Mcp.matches_request(&request)
        || request.state != RegistrationRequestState::Approved
        || record.client_kind != ClientKind::Mcp
        || record.display_label != DEFAULT_MCP_LABEL
        || record.scope != request.scope
        || record.read_only != (record.scope.tools() != hantong_contracts::auth::mcp_mail_tools())
        || record.requests_per_minute != 60
        || record.approved_channel != "approval_helper"
        || record.profile_epoch != 1
        || record.registration_id
            != hantong_platform::registration_id_for_request(record.request_id)
                .map_err(|_| rusqlite::Error::InvalidQuery)?
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let predecessor=connection.query_row("SELECT previous_registration_id,request_id FROM mcp_registration_replacements WHERE registration_id=?1",[record.registration_id.as_slice()],|r|Ok((blob_column::<16>(r,0)?,blob_column::<32>(r,1)?))).optional()?;
    if let Some((id, request_id)) = predecessor {
        let old = crate::registrations::read_registration_bare(connection, &id)?
            .ok_or(rusqlite::Error::InvalidQuery)?;
        if record.scope.tools() != hantong_contracts::auth::mcp_mail_tools()
            || request_id != record.request_id
            || old.client_kind != ClientKind::Mcp
            || old.display_label != DEFAULT_MCP_LABEL
            || old.scope.tools() != hantong_contracts::auth::mcp_read_tools()
            || old.state != RegistrationState::Revoked
            || !old.read_only
            || old.registration_id == record.registration_id
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
        validate_mcp_registration(connection, &old)?;
    }
    Ok(())
}

#[cfg(windows)]
fn orphan_requests(
    tx: &rusqlite::Transaction<'_>,
    flavor: RegistrationFlavor,
) -> Result<Vec<RegistrationRequest>, ErrorCode> {
    let mut defaults=tx.prepare("SELECT registration_id FROM client_registrations WHERE ((client_kind=?1 AND display_label=?2) OR (?1 IN ('gui','mcp') AND (client_kind=?1 OR display_label=?2))) AND (?1<>'mcp' OR state<>'revoked') LIMIT 2").map_err(internal)?;
    let default_ids = defaults
        .query_map(params![flavor.kind().as_str(), flavor.label()], |r| {
            blob_column::<16>(r, 0)
        })
        .map_err(internal)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(internal)?;
    if default_ids.len() > 1 {
        return Err(ErrorCode::Conflict);
    }
    for id in default_ids {
        read_registration(tx, &id)
            .map_err(internal)?
            .ok_or(ErrorCode::InternalError)?;
    }
    let mut statement=tx.prepare("SELECT request_id FROM client_registration_requests WHERE (client_kind=?1 AND display_label=?2) OR (?1 IN ('gui','mcp') AND (client_kind=?1 OR display_label=?2)) ORDER BY request_id LIMIT 1025").map_err(internal)?;
    let ids = statement
        .query_map(params![flavor.kind().as_str(), flavor.label()], |r| {
            blob_column::<32>(r, 0)
        })
        .map_err(internal)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(internal)?;
    if ids.len() > 1024 {
        return Err(ErrorCode::QuotaExceeded);
    }
    let mut derived = std::collections::HashSet::new();
    let mut orphans = Vec::new();
    let now = crate::utc_millis().map_err(internal)?;
    for id in ids {
        let request = read_request(tx, &id)
            .map_err(internal)?
            .ok_or(ErrorCode::InternalError)?;
        if id == [0; 32] || !flavor.matches_request(&request) {
            return Err(ErrorCode::InternalError);
        }
        if request.created_at_utc_ms > now {
            return Err(ErrorCode::ApprovalExpired);
        }
        let registration_id = hantong_platform::registration_id_for_request(id)
            .map_err(|_| ErrorCode::InternalError)?;
        if !derived.insert(registration_id) {
            return Err(ErrorCode::Conflict);
        }
        let linked = tx
            .query_row(
                "SELECT registration_id FROM client_registrations WHERE request_id=?1",
                [id.as_slice()],
                |r| blob_column::<16>(r, 0),
            )
            .optional()
            .map_err(internal)?;
        match linked {
            Some(linked) => {
                let record = read_registration(tx, &linked)
                    .map_err(internal)?
                    .ok_or(ErrorCode::InternalError)?;
                if record.registration_id != registration_id
                    || record.client_kind != flavor.kind()
                    || record.display_label != flavor.label()
                    // read_registration already validates every CLI scope-refresh
                    // transition. Only its initial epoch still equals this request.
                    || ((flavor != RegistrationFlavor::Cli || record.profile_epoch == 1)
                        && record.scope != request.scope)
                {
                    return Err(ErrorCode::InternalError);
                }
            }
            None if request.state == RegistrationRequestState::Approved => {
                return Err(ErrorCode::InternalError);
            }
            None => orphans.push(request),
        }
    }
    Ok(orphans)
}

#[cfg(all(test, windows))]
mod tests;
