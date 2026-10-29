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
