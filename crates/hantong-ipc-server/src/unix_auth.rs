//! Connection-local Unix auth-v2 server state.

#![cfg(unix)]

use hantong_contracts::account_auth::{AccountAuthRevision, AccountAuthTranscript};
use hantong_contracts::account_auth_peer::{CanonicalPeer, encode as encode_peer};
use hantong_contracts::account_scope::AccountScopeV2;
use hantong_contracts::auth::{RequestedScope, SignerRole};
use hantong_contracts::unix_auth::{UnixAuthTranscriptV2, UnixPeerContextV2};
use hantong_contracts::{ErrorCode, platform::OpaqueSigner};
use hantong_platform::unix_transport::UnixLocalConnection;
use hantong_platform::{sha256, verify_signature};
use std::time::{Duration, Instant};

const AUTH_WINDOW_MS: i64 = 30_000;

#[derive(Clone)]
pub struct RegisteredUnixClient {
    id: [u8; 16],
    epoch: u64,
    public_key: [u8; 72],
    scope: RequestedScope,
    active: bool,
}

impl RegisteredUnixClient {
    pub(crate) fn id(&self) -> [u8; 16] {
        self.id
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }

    pub(crate) fn scope_model_version(&self) -> u8 {
        self.scope.scope_model_version()
    }

    pub(crate) fn account_scope(&self) -> Option<&AccountScopeV2> {
        self.scope.account_scope()
    }

    pub fn from_registration_store(
        id: [u8; 16],
        epoch: u64,
        public_key: [u8; 72],
        scope: RequestedScope,
        active: bool,
    ) -> Result<Self, ErrorCode> {
        if id == [0; 16] || epoch == 0 {
            return Err(ErrorCode::InvalidInput);
        }
        hantong_platform::validate_public_key(&public_key)?;
        Ok(Self {
            id,
            epoch,
            public_key,
            scope,
            active,
        })
    }
}

pub struct UnixServerChallenge {
    pub transcript: UnixAuthTranscriptV2,
    pub signature: [u8; 64],
}

pub struct UnixAccountServerChallenge {
    pub transcript: AccountAuthTranscript,
    pub signature: [u8; 64],
}

pub struct PendingUnixAccountAuthentication {
    transcript: AccountAuthTranscript,
    registration: RegisteredUnixClient,
    issued: Instant,
    connection: Option<UnixLocalConnection>,
}

impl PendingUnixAccountAuthentication {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin(
        server_signer: &mut impl OpaqueSigner,
        server_instance_id: [u8; 32],
        registration: &RegisteredUnixClient,
        server_context: UnixPeerContextV2,
        client_context: UnixPeerContextV2,
        client_nonce: [u8; 32],
        requested_scope: AccountScopeV2,
        now_utc_ms: i64,
        server_nonce: [u8; 32],
    ) -> Result<(Self, UnixAccountServerChallenge), ErrorCode> {
        if registration.scope_model_version() != 2 || !registration.active {
            return Err(if !registration.active {
                ErrorCode::ClientSuspended
            } else {
                ErrorCode::ScopeDenied
            });
        }
        if now_utc_ms < 0
            || server_instance_id == [0; 32]
            || server_nonce == [0; 32]
            || client_nonce == [0; 32]
            || server_context.platform != client_context.platform
            || server_context.effective_uid != client_context.effective_uid
        {
            return Err(ErrorCode::InvalidInput);
        }
        let registered_scope = registration.account_scope().ok_or(ErrorCode::ScopeDenied)?;
        if registered_scope
            .intersection(&requested_scope)
            .map_err(|_| ErrorCode::ScopeDenied)?
            != requested_scope
        {
            return Err(ErrorCode::ScopeDenied);
        }
        let public = server_signer
            .public_key()
            .map_err(|_| ErrorCode::AuthRequired)?;
        let transcript = AccountAuthTranscript {
            revision: AccountAuthRevision::UnixV3,
            registration_id: registration.id,
            profile_epoch: registration.epoch,
            issued_at_utc_ms: now_utc_ms,
            expires_at_utc_ms: now_utc_ms
                .checked_add(AUTH_WINDOW_MS)
                .ok_or(ErrorCode::InvalidInput)?,
            server_instance_id,
            server_nonce,
            client_nonce,
            server_key_hash: sha256(&public)?,
            client_key_hash: sha256(&registration.public_key)?,
            server_peer: encode_peer(&CanonicalPeer::Unix(server_context))?,
            client_peer: encode_peer(&CanonicalPeer::Unix(client_context))?,
            scope: requested_scope,
        };
        let signature = server_signer
            .sign(&transcript.encode(SignerRole::Server)?)
            .map_err(|_| ErrorCode::AuthRequired)?;
        Ok((
            Self {
                transcript: transcript.clone(),
                registration: registration.clone(),
                issued: Instant::now(),
                connection: None,
            },
            UnixAccountServerChallenge {
                transcript,
                signature,
            },
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_connected(
        server_signer: &mut impl OpaqueSigner,
        connection: UnixLocalConnection,
        server_instance_id: [u8; 32],
        registration: &RegisteredUnixClient,
        client_nonce: [u8; 32],
        requested_scope: AccountScopeV2,
        now_utc_ms: i64,
        server_nonce: [u8; 32],
    ) -> Result<(Self, UnixAccountServerChallenge), ErrorCode> {
        let client_context = connection.peer_context().clone();
        let server_context = hantong_platform::unix_peer::current_peer_context()?;
        let (mut pending, challenge) = Self::begin(
            server_signer,
            server_instance_id,
            registration,
            server_context,
            client_context,
            client_nonce,
            requested_scope,
            now_utc_ms,
            server_nonce,
        )?;
        pending.connection = Some(connection);
        Ok((pending, challenge))
    }

    pub fn transport(&mut self) -> Result<&mut UnixLocalConnection, ErrorCode> {
        self.connection.as_mut().ok_or(ErrorCode::AuthRequired)
    }

    pub fn finish(
        self,
        signature: &[u8; 64],
        current: Option<&RegisteredUnixClient>,
        now_utc_ms: i64,
    ) -> Result<AuthenticatedUnixConnection, ErrorCode> {
        if now_utc_ms < self.transcript.issued_at_utc_ms
            || now_utc_ms >= self.transcript.expires_at_utc_ms
            || self.issued.elapsed() >= Duration::from_millis(AUTH_WINDOW_MS as u64)
        {
            return Err(ErrorCode::AuthRequired);
        }
        let current = current.ok_or(ErrorCode::ClientNotRegistered)?;
        check_registration(&self.registration, current)?;
        let current_scope = current.account_scope().ok_or(ErrorCode::ScopeDenied)?;
        if current_scope
            .intersection(&self.transcript.scope)
            .map_err(|_| ErrorCode::ScopeDenied)?
            != self.transcript.scope
        {
            return Err(ErrorCode::ScopeDenied);
        }
        let bytes = self
            .transcript
            .encode(SignerRole::Client)
            .map_err(|_| ErrorCode::AuthRequired)?;
        if !verify_signature(&self.registration.public_key, &bytes, signature)? {
            return Err(ErrorCode::AuthRequired);
        }
        Ok(AuthenticatedUnixConnection {
            registration: self.registration,
            granted: RequestedScope::from_account_scope(self.transcript.scope.clone())
                .map_err(|_| ErrorCode::ScopeDenied)?,
            granted_account: Some(self.transcript.scope),
            connection: self.connection,
        })
    }
}

pub struct PendingUnixAuthentication {
    transcript: UnixAuthTranscriptV2,
    registration: RegisteredUnixClient,
    issued: Instant,
    connection: Option<UnixLocalConnection>,
}

impl PendingUnixAuthentication {
    #[allow(clippy::too_many_arguments)] // Keep each independently bound auth input explicit.
    pub(crate) fn begin(
        server_signer: &mut impl OpaqueSigner,
        server_instance_id: [u8; 32],
        registration: &RegisteredUnixClient,
        server_context: UnixPeerContextV2,
        client_context: UnixPeerContextV2,
        client_nonce: [u8; 32],
        requested_scope: RequestedScope,
        now_utc_ms: i64,
        server_nonce: [u8; 32],
    ) -> Result<(Self, UnixServerChallenge), ErrorCode> {
        if !registration.active
            || now_utc_ms < 0
            || server_instance_id == [0; 32]
            || server_nonce == [0; 32]
            || client_nonce == [0; 32]
            || server_context.platform != client_context.platform
            || server_context.effective_uid != client_context.effective_uid
        {
            return Err(if !registration.active {
                ErrorCode::ClientSuspended
            } else {
                ErrorCode::InvalidInput
            });
        }
        if registration.scope_model_version() != 1 || requested_scope.scope_model_version() != 1 {
            return Err(ErrorCode::ScopeDenied);
        }
        let public = server_signer
            .public_key()
            .map_err(|_| ErrorCode::AuthRequired)?;
        let transcript = UnixAuthTranscriptV2 {
            registration_id: registration.id,
            profile_epoch: registration.epoch,
            issued_at_utc_ms: now_utc_ms,
            expires_at_utc_ms: now_utc_ms
                .checked_add(AUTH_WINDOW_MS)
                .ok_or(ErrorCode::InvalidInput)?,
            server_instance_id,
            server_nonce,
            client_nonce,
            server_public_key_hash: sha256(&public)?,
            client_public_key_hash: sha256(&registration.public_key)?,
            server: server_context,
            client: client_context,
            scope: requested_scope,
        };
        let bytes = transcript
            .encode(SignerRole::Server)
            .map_err(|_| ErrorCode::AuthRequired)?;
        let signature = server_signer
            .sign(&bytes)
            .map_err(|_| ErrorCode::AuthRequired)?;
        Ok((
            Self {
                transcript: transcript.clone(),
                registration: registration.clone(),
                issued: Instant::now(),
                connection: None,
            },
            UnixServerChallenge {
                transcript,
                signature,
            },
        ))
    }

    /// Gives the caller access to the owned socket only while the handshake is
    /// pending, for sending and receiving authentication frames.
    pub fn transport(&mut self) -> Result<&mut UnixLocalConnection, ErrorCode> {
        self.connection.as_mut().ok_or(ErrorCode::AuthRequired)
    }

    /// Starts authentication over an already connected, kernel-identified Unix socket.
    /// The socket remains owned by the pending state until `finish` succeeds.
    #[allow(clippy::too_many_arguments)]
    pub fn begin_connected(
        server_signer: &mut impl OpaqueSigner,
        connection: UnixLocalConnection,
        server_instance_id: [u8; 32],
        registration: &RegisteredUnixClient,
        client_nonce: [u8; 32],
        requested_scope: RequestedScope,
        now_utc_ms: i64,
        server_nonce: [u8; 32],
    ) -> Result<(Self, UnixServerChallenge), ErrorCode> {
        let client_context = connection.peer_context().clone();
        let server_context = hantong_platform::unix_peer::current_peer_context()?;
        let (mut pending, challenge) = Self::begin(
            server_signer,
            server_instance_id,
            registration,
            server_context,
            client_context,
            client_nonce,
            requested_scope,
            now_utc_ms,
            server_nonce,
        )?;
        pending.connection = Some(connection);
        Ok((pending, challenge))
    }

    pub fn finish(
        self,
        signature: &[u8; 64],
        current: Option<&RegisteredUnixClient>,
        now_utc_ms: i64,
    ) -> Result<AuthenticatedUnixConnection, ErrorCode> {
        if now_utc_ms < self.transcript.issued_at_utc_ms
            || now_utc_ms >= self.transcript.expires_at_utc_ms
            || self.issued.elapsed() >= Duration::from_millis(AUTH_WINDOW_MS as u64)
        {
            return Err(ErrorCode::AuthRequired);
        }
        let current = current.ok_or(ErrorCode::ClientNotRegistered)?;
        if current.id != self.registration.id
            || !current.active
            || current.epoch != self.registration.epoch
            || current.public_key != self.registration.public_key
        {
            return Err(ErrorCode::AuthRequired);
        }
        let bytes = self
            .transcript
            .encode(SignerRole::Client)
            .map_err(|_| ErrorCode::AuthRequired)?;
        if !verify_signature(&self.registration.public_key, &bytes, signature)? {
            return Err(ErrorCode::AuthRequired);
        }
        let granted = self
            .transcript
            .scope
            .intersection(&self.registration.scope)
            .intersection(&current.scope);
        Ok(AuthenticatedUnixConnection {
            registration: self.registration,
            granted,
            granted_account: None,
            connection: self.connection,
        })
    }
}

pub struct AuthenticatedUnixConnection {
    registration: RegisteredUnixClient,
    granted: RequestedScope,
    granted_account: Option<AccountScopeV2>,
    connection: Option<UnixLocalConnection>,
}
impl AuthenticatedUnixConnection {
    pub fn registration_id(&self) -> &[u8; 16] {
        &self.registration.id
    }
    pub fn profile_epoch(&self) -> u64 {
        self.registration.epoch
    }
    pub fn transport(&mut self) -> Result<&mut UnixLocalConnection, ErrorCode> {
        self.connection.as_mut().ok_or(ErrorCode::AuthRequired)
    }

    /// Rechecks the trusted registration before each operation. A revoked,
    /// rotated, or scope-reduced registration consumes the socket.
    pub fn revalidate_registration(
        &mut self,
        current: Option<&RegisteredUnixClient>,
    ) -> Result<(), ErrorCode> {
        if self.connection.is_none() {
            return Err(ErrorCode::AuthRequired);
        }
        let result = current
            .ok_or(ErrorCode::ClientNotRegistered)
            .and_then(|value| check_registration(&self.registration, value));
        if let Err(code) = result {
            self.connection.take();
            return Err(code);
        }
        let current = current.expect("validated registration");
        if self.granted.intersection(&current.scope) != self.granted {
            self.connection.take();
            return Err(ErrorCode::ScopeDenied);
        }
        if let Some(granted) = &self.granted_account {
            if current.account_scope() != self.registration.account_scope() {
                self.connection.take();
                return Err(ErrorCode::ScopeDenied);
            }
            let typed_ok = current.account_scope().is_some_and(|scope| {
                scope
                    .intersection(granted)
                    .is_ok_and(|intersection| intersection == *granted)
            });
            if !typed_ok {
                self.connection.take();
                return Err(ErrorCode::ScopeDenied);
            }
        }
        Ok(())
    }

    pub fn check_account_scope(
        &mut self,
        current: Option<&RegisteredUnixClient>,
        account: &str,
        tool: &str,
    ) -> Result<(), ErrorCode> {
        self.revalidate_registration(current)?;
        let current = current.expect("validated registration");
        let allowed = if let Some(granted) = &self.granted_account {
            current.account_scope().is_some_and(|scope| {
                granted.permits_account(account, tool) && scope.permits_account(account, tool)
            })
        } else {
            self.granted.permits_account(account, tool)
                && current.scope.permits_account(account, tool)
        };
        if allowed {
            Ok(())
        } else {
            Err(ErrorCode::ScopeDenied)
        }
    }

    /// Checks one accountless app-read tool against the trusted principal
    /// scope. Empty account lists never imply access to these tools.
    pub fn check_app_read_scope(
        &mut self,
        current: Option<&RegisteredUnixClient>,
        tool: hantong_contracts::auth::AppReadTool,
    ) -> Result<(), ErrorCode> {
        self.revalidate_registration(current)?;
        let current = current.expect("validated registration");
        let allowed = if let Some(granted) = &self.granted_account {
            current.account_scope().is_some_and(|scope| {
                granted.permits_principal_tool(tool.as_str())
                    && scope.permits_principal_tool(tool.as_str())
            })
        } else {
            self.granted
                .tools()
                .iter()
                .any(|value| value == tool.as_str())
                && current
                    .scope
                    .tools()
                    .iter()
                    .any(|value| value == tool.as_str())
        };
        if allowed {
            Ok(())
        } else {
            Err(ErrorCode::ScopeDenied)
        }
    }
}

fn check_registration(
    original: &RegisteredUnixClient,
    current: &RegisteredUnixClient,
) -> Result<(), ErrorCode> {
    if current.id != original.id {
        return Err(ErrorCode::ClientNotRegistered);
    }
    if !current.active {
        return Err(ErrorCode::ClientSuspended);
    }
    if current.epoch != original.epoch || current.public_key != original.public_key {
        return Err(ErrorCode::AuthRequired);
    }
    if current.scope_model_version() != original.scope_model_version() {
        return Err(ErrorCode::VersionMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::platform::{OpaqueSigner, PlatformError};
    use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};

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
                .and_then(|sig| sig.as_ref().try_into().map_err(|_| PlatformError::Denied))
        }
    }
    fn scope() -> RequestedScope {
        RequestedScope::new(vec!["account_a".into()], vec!["mail.messages.get".into()]).unwrap()
    }
    fn peer(pid: u32) -> UnixPeerContextV2 {
        UnixPeerContextV2::new(
            hantong_contracts::unix_auth::UnixPlatform::Linux,
            1000,
            1000,
            pid,
            7,
            [pid as u8; 32],
        )
        .unwrap()
    }

    #[test]
    fn sec03_unix_auth_roundtrip_binds_registration_scope_and_signature() {
        let mut server = TestSigner::new();
        let mut client = TestSigner::new();
        let registration =
            RegisteredUnixClient::from_registration_store([1; 16], 3, client.blob(), scope(), true)
                .unwrap();
        let (pending, challenge) = PendingUnixAuthentication::begin(
            &mut server,
            [2; 32],
            &registration,
            peer(1),
            peer(2),
            [3; 32],
            scope(),
            1000,
            [4; 32],
        )
        .unwrap();
        let bytes = challenge.transcript.encode(SignerRole::Client).unwrap();
        let signature = client.sign(&bytes).unwrap();
        let mut authenticated = pending
            .finish(&signature, Some(&registration), 1001)
            .unwrap();
        assert_eq!(
            authenticated.check_account_scope(
                Some(&registration),
                "account_a",
                "mail.messages.get"
            ),
            Err(ErrorCode::AuthRequired)
        );
        assert!(
            authenticated
                .granted
                .permits_account("account_a", "mail.messages.get")
        );
    }

    #[test]
    fn sec03_unix_auth_rejects_bad_signature_revocation_and_expiry() {
        let mut server = TestSigner::new();
        let mut client = TestSigner::new();
        let registration =
            RegisteredUnixClient::from_registration_store([1; 16], 3, client.blob(), scope(), true)
                .unwrap();
        let (pending, _) = PendingUnixAuthentication::begin(
            &mut server,
            [2; 32],
            &registration,
            peer(1),
            peer(2),
            [3; 32],
            scope(),
            1000,
            [4; 32],
        )
        .unwrap();
        assert!(matches!(
            pending.finish(&[0; 64], Some(&registration), 1001),
            Err(ErrorCode::AuthRequired)
        ));
        let mut revoked = registration.clone();
        revoked.active = false;
        let (pending, challenge) = PendingUnixAuthentication::begin(
            &mut server,
            [2; 32],
            &registration,
            peer(1),
            peer(2),
            [5; 32],
            scope(),
            1000,
            [6; 32],
        )
        .unwrap();
        let signature = client
            .sign(&challenge.transcript.encode(SignerRole::Client).unwrap())
            .unwrap();
        assert!(matches!(
            pending.finish(&signature, Some(&revoked), 1001),
            Err(ErrorCode::AuthRequired)
        ));
        let (pending, challenge) = PendingUnixAuthentication::begin(
            &mut server,
            [2; 32],
            &registration,
            peer(1),
            peer(2),
            [7; 32],
            scope(),
            1000,
            [8; 32],
        )
        .unwrap();
        let signature = client
            .sign(&challenge.transcript.encode(SignerRole::Client).unwrap())
            .unwrap();
        assert!(matches!(
            pending.finish(&signature, Some(&registration), 31_000),
            Err(ErrorCode::AuthRequired)
        ));
    }
}

#[cfg(all(test, unix))]
mod model2_tests {
    include!("unix_model2_auth_tests.rs");
}
