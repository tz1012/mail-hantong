use super::*;
use hantong_contracts::{
    auth::{RequestedScope, default_cli_legacy_tools},
    unix_auth::UnixPlatform,
};
use hantong_secrets::KeyedConnection;
use hantong_store::{
    CliRegistrationStage, ClientKind, DEFAULT_CLI_LABEL, Store,
    account_provisioning::imap_account_stage_payload_hash, imap_setup::NewImapSetupRequest,
    mail_connection::MailMutation,
};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

fn public_key() -> [u8; 72] {
    let mut key = [0; 72];
    key[..8].copy_from_slice(b"ECS1\x20\0\0\0");
    key[8..40].copy_from_slice(
        &hantong_contracts::wire_hex::decode_fixed::<32>(
            "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
        )
        .unwrap(),
    );
    key[40..].copy_from_slice(
        &hantong_contracts::wire_hex::decode_fixed::<32>(
            "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
        )
        .unwrap(),
    );
    key
}

pub(crate) struct Fixture {
    pub(crate) root: PathBuf,
    pub(crate) store: Store,
    pub(crate) base: RegistrationRecord,
    pub(crate) request: ImapSetupRequest,
    pub(crate) peer: ObservedPeer,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
impl Fixture {
    pub(crate) fn new() -> Self {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        crate::test_support::ensure_test_backend();
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-generic-connect-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let data = root.join("data");
        let keys = root.join("keys");
        drop(Store::open(&data, &keys).unwrap());

        let public = public_key();
        let pin = hantong_platform::sha256(&public).unwrap();
        let keyed = KeyedConnection::open(&data, &keys).unwrap();
        keyed
            .with_connection(|c| {
                c.execute(
                    "INSERT INTO server_identity(singleton,key_id,state,public_key,key_epoch,version,created_at_utc_ms,updated_at_utc_ms) VALUES(1,?1,'active',?2,1,1,1,1)",
                    (pin.as_slice(), public.as_slice()),
                )?;
                c.execute(
                    "INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES('baseline-account','imap',1,'ready','[]','[]',1)",
                    [],
                )?;
                c.execute(
                    "INSERT INTO mail_connection_settings(account_id,endpoint_kind,settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms) VALUES('baseline-account','imap',1,'imap.example.test',993,'implicit_tls','synthetic-user','app_password',1)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        drop(keyed);

        let store = Store::open(&data, &keys).unwrap();
        let account = "generic-account";
        let endpoint = hantong_domain::mail_connection::MailConnectionConfig::from_json(
            br#"{"schema_version":1,"account_id":"generic-account","imap":{"settings_version":1,"host":"imap.example.test","port":993,"tls":"implicit_tls","login_name":"synthetic-user","authentication":"app_password"}}"#,
        )
        .unwrap()
        .imap()
        .clone();
        let draft = hantong_domain::account_provisioning::NewImapAccountDraft::new(
            account.to_owned(),
            Some("synthetic".into()),
            None,
        )
        .unwrap();
        let payload = imap_account_stage_payload_hash(&draft, &endpoint).unwrap();
        let mutation =
            MailMutation::new("generic-op".into(), "generic-idem".into(), 0, payload).unwrap();
        let new_request = NewImapSetupRequest::new([4; 32], draft, endpoint, mutation).unwrap();
        let request = store.create_imap_setup_request(&new_request).unwrap();
        let registration_request = store
            .create_registration_request(
                [1; 32],
                ClientKind::Cli,
                DEFAULT_CLI_LABEL,
                &RequestedScope::new(vec!["baseline-account".into()], default_cli_legacy_tools())
                    .unwrap(),
            )
            .unwrap();
        let registration_id =
            hantong_platform::registration_id_for_request(registration_request.request_id).unwrap();
        let staged = store
            .stage_terminal_cli_registration(
                &CliRegistrationStage::new_terminal_cli(
                    registration_request,
                    registration_id,
                    public,
                    pin,
                )
                .unwrap(),
            )
            .unwrap();
        // Keep synthetic approval timestamps behind the host clock; expiry
        // behavior is exercised separately without depending on clock rollback.
        drop(store);
        let keyed = KeyedConnection::open(&data, &keys).unwrap();
        keyed.with_connection(|c| {
            let current = now_ms();
            let created = current.saturating_sub(300_000);
            c.execute("UPDATE client_registration_requests SET created_at_utc_ms=?1,expires_at_utc_ms=?2,decided_at_utc_ms=?1 WHERE request_id=(SELECT request_id FROM client_registrations WHERE registration_id=?3)", (created,current,registration_id.as_slice()))?;
            c.execute("UPDATE client_registrations SET approved_at_utc_ms=?1,updated_at_utc_ms=?1 WHERE registration_id=?2", (created,registration_id.as_slice()))?;
            Ok(())
        }).unwrap();
        drop(keyed);
        let store = Store::open(&data, &keys).unwrap();
        let base = store
            .activate_cli_registration(registration_id, staged.version, pin)
            .unwrap();
        Self {
            root,
            store,
            base,
            request,
            peer: ObservedPeer::Unix(
                hantong_contracts::unix_auth::UnixPeerContextV2::new(
                    UnixPlatform::Linux,
                    1000,
                    1000,
                    42,
                    7,
                    [7; 32],
                )
                .unwrap(),
            ),
        }
    }

    fn intent(&self) -> GenericConnectIntent {
        GenericConnectIntent::new(
            &self.store,
            &self.base,
            [9; 32],
            [8; 32],
            self.peer.clone(),
            *self.request.intent().request_id(),
            now_ms(),
        )
        .unwrap()
    }
}
fn now_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

#[test]
fn sec03_generic_requires_review_pending_and_unexpired_leaf() {
    let fixture = Fixture::new();
    let intent = fixture.intent();
    assert_eq!(
        intent.account_id(),
        fixture.request.intent().draft().account_id()
    );
    assert!(matches!(
        GenericConnectIntent::new(
            &fixture.store,
            &fixture.base,
            [10; 32],
            [8; 32],
            fixture.peer.clone(),
            *fixture.request.intent().request_id(),
            fixture.request.expires_at_utc_ms()
        ),
        Err(ErrorCode::Conflict)
    ));
    let closed = fixture
        .store
        .close_imap_setup_review(&fixture.request, ErrorCode::ApprovalDenied)
        .unwrap();
    assert!(matches!(
        GenericConnectIntent::new(
            &fixture.store,
            &fixture.base,
            [10; 32],
            [8; 32],
            fixture.peer.clone(),
            *closed.intent().request_id(),
            now_ms(),
        ),
        Err(ErrorCode::Conflict)
    ));
}

#[test]
fn sec03_generic_capture_is_peer_bound_and_single_use() {
    let fixture = Fixture::new();
    let mut intent = fixture.intent();
    let request = *fixture.request.intent().request_id();
    assert_eq!(
        intent.check_capture(
            TerminalReviewKind::ImapSetup,
            request,
            &ObservedPeer::Unix(
                hantong_contracts::unix_auth::UnixPeerContextV2::new(
                    UnixPlatform::Linux,
                    1000,
                    1000,
                    42,
                    7,
                    [8; 32]
                )
                .unwrap(),
            ),
            now_ms()
        ),
        Err(ErrorCode::ScopeDenied)
    );
    intent
        .check_capture(
            TerminalReviewKind::ImapSetup,
            request,
            &fixture.peer,
            now_ms(),
        )
        .unwrap();
    intent
        .capture_submitted(
            TerminalReviewKind::ImapSetup,
            request,
            &fixture.peer,
            now_ms(),
        )
        .unwrap();
    assert_eq!(
        intent.check_capture(
            TerminalReviewKind::ImapSetup,
            request,
            &fixture.peer,
            now_ms()
        ),
        Err(ErrorCode::Conflict)
    );
}

#[test]
fn sec03_generic_ready_requires_consumed_capture_and_probe() {
    let fixture = Fixture::new();
    let intent = fixture.intent();
    assert_eq!(intent.imap_ready(&fixture.store), Err(ErrorCode::Conflict));
}

#[test]
fn sec03_generic_baseline_pin_peer_and_expiry_changes_do_not_stage() {
    let fixture = Fixture::new();
    let mut intent = fixture.intent();
    assert_eq!(
        intent.check_current(&fixture.store, [8; 32], &fixture.peer, now_ms()),
        Ok(())
    );
    assert_eq!(
        intent.check_current(&fixture.store, [9; 32], &fixture.peer, now_ms()),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        intent.check_current(&fixture.store, [8; 32], &fixture.peer, intent.expires_at),
        Err(ErrorCode::ApprovalExpired)
    );
    intent.base.version += 1;
    assert_eq!(
        intent.check_current(&fixture.store, [8; 32], &fixture.peer, now_ms()),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        fixture
            .store
            .load_registration(fixture.base.registration_id)
            .unwrap(),
        Some(fixture.base.clone())
    );
    assert!(
        fixture
            .store
            .list_staged_full_mail_decisions()
            .unwrap()
            .is_empty()
    );
}
