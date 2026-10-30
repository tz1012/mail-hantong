//! Isolated encrypted Store fixture. Product crates may use this crate only as
//! a dev dependency. SQL operations below are fixed, synthetic test scenarios.
use hantong_contracts::{ErrorCode, auth::RequestedScope};
use hantong_platform::ServerSigningKey;
use hantong_secrets::KeyedConnection;
use hantong_store::{CliRegistrationStage, ClientKind, DEFAULT_CLI_LABEL, Store};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

pub struct PolicyStoreFixture {
    root: PathBuf,
    pub registration_id: [u8; 16],
}
pub enum PolicyFixtureMutation {
    Limit {
        read_only: bool,
        requests_per_minute: u32,
    },
    AllowSend,
    Revoke,
    AdvanceEpoch,
    AllowFolders,
    AllowUidPage,
    AllowMetadata,
    AllowAppReads,
    AppClientKind(ClientKind),
}
impl PolicyStoreFixture {
    pub fn new(server: &ServerSigningKey, client_public: [u8; 72]) -> Result<Self, ErrorCode> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-policy-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).map_err(|_| ErrorCode::InternalError)?;
        let request_id = hantong_platform::random_nonce()?;
        let value = Self {
            root,
            registration_id: hantong_platform::registration_id_for_request(request_id)?,
        };
        let store = value.open()?;
        store.stage_server_identity(server.id())?;
        store.activate_server_identity(server.id(), server.public_key()?, 1)?;
        drop(store);
        value.database()?.with_connection(|c| {
            c.execute_batch("INSERT INTO accounts(account_id,provider,primary_address,display_name,account_number,status,capabilities,granted_scopes,settings_version) VALUES('account_a','imap','fixture-a@example.invalid','Fixture A',1,'ready','[]','[]',1),('account_b','imap','fixture-b@example.invalid','Fixture B',2,'ready','[]','[]',1);")
        }).map_err(|_|ErrorCode::InternalError)?;
        let store = value.open()?;
        let scope =
            RequestedScope::new(vec!["account_a".into()], vec!["mail.accounts.list".into()])?;
        let request = store.create_registration_request(
            request_id,
            ClientKind::Cli,
            DEFAULT_CLI_LABEL,
            &scope,
        )?;
        let hash = hantong_platform::sha256(&server.public_key()?)?;
        let stage = CliRegistrationStage::new(request, value.registration_id, client_public, hash)?;
        store.stage_cli_registration(&stage)?;
        store.activate_cli_registration(value.registration_id, 1, hash)?;
        drop(store);
        // These fixtures exercise generic policy limits/tool grants/epochs.
        // The default CLI has a separate fixed policy and scope-history contract.
        value.database()?.with_connection(|c| {
            let tx = c.unchecked_transaction()?;
            tx.execute("UPDATE client_registration_requests SET display_label='Policy fixture' WHERE request_id=(SELECT request_id FROM client_registrations WHERE registration_id=?1)",[value.registration_id.as_slice()])?;
            tx.execute("UPDATE client_registrations SET display_label='Policy fixture' WHERE registration_id=?1",[value.registration_id.as_slice()])?;
            tx.commit()
        }).map_err(|_|ErrorCode::InternalError)?;
        Ok(value)
    }
    pub fn open(&self) -> Result<Store, ErrorCode> {
        Store::open(self.root.join("data"), self.root.join("keys"))
    }
    fn database(&self) -> Result<KeyedConnection, ErrorCode> {
        KeyedConnection::open(self.root.join("data"), self.root.join("keys"))
            .map_err(|_| ErrorCode::InternalError)
    }
    /// The fixture Store must be dropped before applying these named scenarios.
    pub fn mutate(&self, change: PolicyFixtureMutation) -> Result<(), ErrorCode> {
        if let PolicyFixtureMutation::Limit {
            requests_per_minute,
            ..
        } = change
            && !(1..=600).contains(&requests_per_minute)
        {
            return Err(ErrorCode::InvalidInput);
        }
        self.database()?.with_connection(|c| {
            match change {
                PolicyFixtureMutation::AllowAppReads => {
                    let tx=c.unchecked_transaction()?;
                    let tools=hantong_contracts::auth::default_cli_app_read_tools();
                    let json=r#"["app.diagnostics.snapshot","app.settings.display.get","mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]"#;
                    tx.execute("UPDATE client_registration_requests SET requested_tools_json=?1,version=version+1 WHERE request_id=(SELECT request_id FROM client_registrations WHERE registration_id=?2)",(json,self.registration_id.as_slice()))?;
                    for tool in tools { tx.execute("INSERT OR IGNORE INTO client_registration_tools(registration_id,tool_id) VALUES(?1,?2)",(self.registration_id.as_slice(),tool))?; }
                    tx.execute("UPDATE client_registrations SET profile_epoch=profile_epoch+1,version=version+1 WHERE registration_id=?1",[self.registration_id.as_slice()])?;
                    tx.commit()?;
                }
                PolicyFixtureMutation::AppClientKind(kind) => {
                    let tx=c.unchecked_transaction()?;
                    tx.execute("UPDATE client_registration_requests SET client_kind=?1 WHERE request_id=(SELECT request_id FROM client_registrations WHERE registration_id=?2)",(kind.as_str(),self.registration_id.as_slice()))?;
                    tx.execute("UPDATE client_registrations SET client_kind=?1 WHERE registration_id=?2",(kind.as_str(),self.registration_id.as_slice()))?;
                    tx.commit()?;
                }
                PolicyFixtureMutation::Limit{read_only,requests_per_minute}=> {
                    c.execute("UPDATE client_registrations SET read_only=?1,requests_per_minute=?2,version=version+1 WHERE registration_id=?3",(i64::from(read_only),i64::from(requests_per_minute),self.registration_id.as_slice()))?;
                }
                PolicyFixtureMutation::AllowSend=> {
                    c.execute("UPDATE client_registration_requests SET requested_tools_json='[\"mail.accounts.list\",\"mail.send.execute\"]',version=version+1 WHERE request_id=(SELECT request_id FROM client_registrations WHERE registration_id=?1)",[self.registration_id.as_slice()])?;
                    c.execute("INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,'mail.send.execute')",[self.registration_id.as_slice()])?;
                    c.execute("UPDATE client_registrations SET profile_epoch=profile_epoch+1,version=version+1 WHERE registration_id=?1",[self.registration_id.as_slice()])?;
                }
                PolicyFixtureMutation::Revoke=> {
                    c.execute("UPDATE client_registrations SET state='revoked',revoked_at_utc_ms=updated_at_utc_ms,version=version+1 WHERE registration_id=?1",[self.registration_id.as_slice()])?;
                }
                PolicyFixtureMutation::AdvanceEpoch=> {
                    c.execute("UPDATE client_registrations SET profile_epoch=profile_epoch+1,version=version+1 WHERE registration_id=?1",[self.registration_id.as_slice()])?;
                }
                PolicyFixtureMutation::AllowFolders=> {
                    c.execute("UPDATE client_registration_requests SET requested_tools_json='[\"mail.accounts.list\",\"mail.folders.list\",\"mail.folders.refresh\"]',version=version+1 WHERE request_id=(SELECT request_id FROM client_registrations WHERE registration_id=?1)",[self.registration_id.as_slice()])?;
                    c.execute("INSERT OR IGNORE INTO client_registration_tools(registration_id,tool_id) VALUES(?1,'mail.folders.list'),(?1,'mail.folders.refresh')",[self.registration_id.as_slice()])?;
                    c.execute("INSERT OR IGNORE INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES('account_a','imap',1,'imap.fixture.invalid',993,'implicit_tls','fixture-user','app_password',1)",[])?;
                    c.execute("UPDATE client_registrations SET profile_epoch=profile_epoch+1,version=version+1 WHERE registration_id=?1",[self.registration_id.as_slice()])?;
                }
                PolicyFixtureMutation::AllowUidPage=> {
                    c.execute("UPDATE client_registration_requests SET requested_tools_json='[\"mail.accounts.list\",\"mail.folders.list\",\"mail.folders.refresh\",\"mail.sync.page\"]',version=version+1 WHERE request_id=(SELECT request_id FROM client_registrations WHERE registration_id=?1)",[self.registration_id.as_slice()])?;
                    c.execute("INSERT OR IGNORE INTO client_registration_tools(registration_id,tool_id) VALUES(?1,'mail.folders.list'),(?1,'mail.folders.refresh'),(?1,'mail.sync.page')",[self.registration_id.as_slice()])?;
                    c.execute("INSERT OR IGNORE INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES('account_a','imap',1,'imap.fixture.invalid',993,'implicit_tls','fixture-user','app_password',1)",[])?;
                    c.execute("UPDATE client_registrations SET profile_epoch=profile_epoch+1,version=version+1 WHERE registration_id=?1",[self.registration_id.as_slice()])?;
                }
                PolicyFixtureMutation::AllowMetadata=> {
                    c.execute("UPDATE client_registration_requests SET requested_tools_json='[\"mail.accounts.list\",\"mail.folders.list\",\"mail.folders.refresh\",\"mail.messages.search\",\"mail.sync.page\"]',version=version+1 WHERE request_id=(SELECT request_id FROM client_registrations WHERE registration_id=?1)",[self.registration_id.as_slice()])?;
                    c.execute("INSERT OR IGNORE INTO client_registration_tools(registration_id,tool_id) VALUES(?1,'mail.folders.list'),(?1,'mail.folders.refresh'),(?1,'mail.messages.search'),(?1,'mail.sync.page')",[self.registration_id.as_slice()])?;
                    c.execute("INSERT OR IGNORE INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES('account_a','imap',1,'imap.fixture.invalid',993,'implicit_tls','fixture-user','app_password',1)",[])?;
                    c.execute("UPDATE client_registrations SET profile_epoch=profile_epoch+1,version=version+1 WHERE registration_id=?1",[self.registration_id.as_slice()])?;
                }

            }
            Ok(())
        }).map_err(|_|ErrorCode::InternalError)
    }
}
impl Drop for PolicyStoreFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
