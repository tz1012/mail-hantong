use hantong_contracts::auth::RequestedScope;
use hantong_contracts::platform::{OpaqueSigner, PlatformError};
use hantong_ipc_client::{unix_auth as client_auth, unix_authenticated_wire as client_wire};
use hantong_ipc_server::{
    unix_auth::{AuthenticatedUnixConnection, RegisteredUnixClient},
    unix_authenticated_wire,
};
use hantong_platform::unix_transport::{self, UnixLocalServer};
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct TestSigner(EcdsaKeyPair);

impl TestSigner {
    fn new() -> Self {
        let bytes = EcdsaKeyPair::generate_pkcs8(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            &ring::rand::SystemRandom::new(),
        )
        .unwrap();
        Self(
            EcdsaKeyPair::from_pkcs8(
                &ECDSA_P256_SHA256_FIXED_SIGNING,
                bytes.as_ref(),
                &ring::rand::SystemRandom::new(),
            )
            .unwrap(),
        )
    }

    fn blob(&self) -> [u8; 72] {
        let point = self.0.public_key().as_ref();
        let mut out = [0; 72];
        out[..4].copy_from_slice(b"ECS1");
        out[4..8].copy_from_slice(&32_u32.to_le_bytes());
        out[8..].copy_from_slice(&point[1..]);
        out
    }
}

impl OpaqueSigner for TestSigner {
    fn public_key(&self) -> Result<[u8; 72], PlatformError> {
        Ok(self.blob())
    }

    fn sign(&mut self, transcript: &[u8]) -> Result<[u8; 64], PlatformError> {
        self.0
            .sign(&ring::rand::SystemRandom::new(), transcript)
            .map_err(|_| PlatformError::Denied)
            .and_then(|value| value.as_ref().try_into().map_err(|_| PlatformError::Denied))
    }
}

fn scope() -> RequestedScope {
    RequestedScope::new(vec!["account_a".into()], vec!["mail.messages.get".into()]).unwrap()
}

#[tokio::test]
async fn sec03_unix_v2_socket_handshake_uses_kernel_peer_and_fresh_lookup() {
    if std::env::var_os("HANTONG_UNIX_AUTH_CHILD").is_none() {
        let root = std::env::temp_dir().join(format!(
            "hantong-unix-auth-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("unix_authentication_tests::sec03_unix_v2_socket_handshake_uses_kernel_peer_and_fresh_lookup")
            .arg("--nocapture")
            .env("HANTONG_UNIX_AUTH_CHILD", "1")
            .env("XDG_RUNTIME_DIR", &root)
            .status()
            .unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert!(status.success());
        return;
    }

    let server = UnixLocalServer::bind().unwrap();
    let mut server_signer = TestSigner::new();
    let client_signer = TestSigner::new();
    let client_public = client_signer.blob();
    let registration =
        RegisteredUnixClient::from_registration_store([1; 16], 3, client_public, scope(), true)
            .unwrap();
    let server_public = server_signer.blob();
    let server_task = async {
        let connection = server.accept().await?;
        unix_authenticated_wire::authenticate_connected(
            connection,
            &mut server_signer,
            [2; 32],
            &mut |id| Ok((id == &[1; 16]).then_some(registration.clone())),
        )
        .await
    };
    let client_task = async {
        let connection = unix_transport::connect().await?;
        let pending = client_auth::PendingUnixAuthentication::begin_connected(
            client_signer,
            connection,
            server_public,
            [1; 16],
            3,
            scope(),
            [4; 32],
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
        )?;
        client_wire::authenticate_connected(pending, "auth-1").await
    };
    let (authenticated, _client) = tokio::try_join!(server_task, client_task).unwrap();
    assert_scope_then_revoke(authenticated, registration, client_public);
}

fn assert_scope_then_revoke(
    mut authenticated: AuthenticatedUnixConnection,
    registration: RegisteredUnixClient,
    client_public: [u8; 72],
) {
    assert!(
        authenticated
            .check_account_scope(Some(&registration), "account_a", "mail.messages.get")
            .is_ok()
    );
    let revoked =
        RegisteredUnixClient::from_registration_store([1; 16], 3, client_public, scope(), false)
            .unwrap();
    assert_eq!(
        authenticated.check_account_scope(Some(&revoked), "account_a", "mail.messages.get"),
        Err(hantong_contracts::ErrorCode::ClientSuspended)
    );
}
