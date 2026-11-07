//! Connection-local Unix auth-v2 client state.
//! Private keys stay behind the `OpaqueSigner` trait.

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

pub struct PendingUnixAccountAuthentication<S> {
    signer: S,
    registration_id: [u8; 16],
    profile_epoch: u64,
    requested_scope: AccountScopeV2,
    client_peer: Vec<u8>,
    client_nonce: [u8; 32],
    client_key_hash: [u8; 32],
    pinned_server_key: [u8; 72],
    pinned_server_key_hash: [u8; 32],
    started_at_utc_ms: i64,
    started: Instant,
    connection: Option<UnixLocalConnection>,
}

impl<S: OpaqueSigner> std::fmt::Debug for PendingUnixAccountAuthentication<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PendingUnixAccountAuthentication { <redacted> }")
    }
}

impl<S: OpaqueSigner> PendingUnixAccountAuthentication<S> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin(
        signer: S,
        pinned_server_key: [u8; 72],
        registration_id: [u8; 16],
        profile_epoch: u64,
        requested_scope: AccountScopeV2,
        client_context: UnixPeerContextV2,
        server_context: UnixPeerContextV2,
        client_nonce: [u8; 32],
        started_at_utc_ms: i64,
    ) -> Result<Self, ErrorCode> {
        if registration_id == [0; 16]
            || profile_epoch == 0
            || client_nonce == [0; 32]
            || started_at_utc_ms < 0
            || client_context.platform != server_context.platform
            || client_context.effective_uid != server_context.effective_uid
        {
            return Err(ErrorCode::InvalidInput);
        }
        let public = signer.public_key().map_err(|_| ErrorCode::AuthRequired)?;
        hantong_platform::validate_public_key(&pinned_server_key)?;
        Ok(Self {
            signer,
            registration_id,
            profile_epoch,
            requested_scope,
            client_peer: encode_peer(&CanonicalPeer::Unix(client_context.clone()))?,
            client_nonce,
            client_key_hash: sha256(&public)?,
            pinned_server_key_hash: sha256(&pinned_server_key)?,
            pinned_server_key,
            started_at_utc_ms,
            started: Instant::now(),
            connection: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_connected(
        signer: S,
        connection: UnixLocalConnection,
        pinned_server_key: [u8; 72],
        registration_id: [u8; 16],
        profile_epoch: u64,
        requested_scope: AccountScopeV2,
        client_nonce: [u8; 32],
        started_at_utc_ms: i64,
    ) -> Result<Self, ErrorCode> {
        let server_context = connection.peer_context().clone();
        let client_context = hantong_platform::unix_peer::current_peer_context()?;
        let mut pending = Self::begin(
            signer,
            pinned_server_key,
            registration_id,
            profile_epoch,
            requested_scope,
            client_context,
            server_context,
            client_nonce,
            started_at_utc_ms,
        )?;
        pending.connection = Some(connection);
        Ok(pending)
    }

    pub fn client_nonce(&self) -> [u8; 32] {
        self.client_nonce
    }

    pub(crate) fn registration_id(&self) -> [u8; 16] {
        self.registration_id
    }

    pub(crate) fn profile_epoch(&self) -> u64 {
        self.profile_epoch
    }

    pub fn transport(&mut self) -> Result<&mut UnixLocalConnection, ErrorCode> {
        self.connection.as_mut().ok_or(ErrorCode::AuthRequired)
    }

    pub fn requested_scope(&self) -> &AccountScopeV2 {
        &self.requested_scope
    }

    fn respond_inner(
        &mut self,
        challenge: AccountAuthTranscript,
        server_signature: &[u8; 64],
        now_utc_ms: i64,
    ) -> Result<[u8; 64], ErrorCode> {
        if challenge.revision != AccountAuthRevision::UnixV3
            || now_utc_ms < self.started_at_utc_ms
            || now_utc_ms - self.started_at_utc_ms >= AUTH_WINDOW_MS
            || self.started.elapsed() >= Duration::from_millis(AUTH_WINDOW_MS as u64)
            || challenge.registration_id != self.registration_id
            || challenge.profile_epoch != self.profile_epoch
            || challenge.client_nonce != self.client_nonce
            || challenge.scope != self.requested_scope
            || challenge.client_peer != self.client_peer
            || challenge.client_key_hash != self.client_key_hash
            || challenge.server_key_hash != self.pinned_server_key_hash
            || challenge.issued_at_utc_ms < self.started_at_utc_ms
            || challenge.issued_at_utc_ms > now_utc_ms
            || challenge.expires_at_utc_ms <= now_utc_ms
            || challenge
                .expires_at_utc_ms
                .checked_sub(challenge.issued_at_utc_ms)
                .is_none_or(|lifetime| lifetime > AUTH_WINDOW_MS)
        {
            return Err(ErrorCode::AuthRequired);
        }
        let server_bytes = challenge
            .encode(SignerRole::Server)
            .map_err(|_| ErrorCode::AuthRequired)?;
        if !verify_signature(&self.pinned_server_key, &server_bytes, server_signature)? {
            return Err(ErrorCode::AuthRequired);
        }
        let client_bytes = challenge
            .encode(SignerRole::Client)
            .map_err(|_| ErrorCode::AuthRequired)?;
        self.signer
            .sign(&client_bytes)
            .map_err(|_| ErrorCode::AuthRequired)
    }

    pub fn respond_connected(
        mut self,
        challenge: AccountAuthTranscript,
        server_signature: &[u8; 64],
        now_utc_ms: i64,
    ) -> Result<(UnixLocalConnection, [u8; 64]), ErrorCode> {
        let connection = self.connection.take().ok_or(ErrorCode::InvalidInput)?;
        let signature = self.respond_inner(challenge, server_signature, now_utc_ms)?;
        Ok((connection, signature))
    }
}

pub struct PendingUnixAuthentication<S> {
    signer: S,
    registration_id: [u8; 16],
    profile_epoch: u64,
    requested_scope: RequestedScope,
    client_context: UnixPeerContextV2,
    server_context: UnixPeerContextV2,
    client_nonce: [u8; 32],
    client_key_hash: [u8; 32],
    pinned_server_key: [u8; 72],
    pinned_server_key_hash: [u8; 32],
    started_at_utc_ms: i64,
    started: Instant,
    connection: Option<UnixLocalConnection>,
}

impl<S: OpaqueSigner> std::fmt::Debug for PendingUnixAuthentication<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PendingUnixAuthentication { <redacted> }")
    }
}

impl<S: OpaqueSigner> PendingUnixAuthentication<S> {
    #[allow(clippy::too_many_arguments)] // Keep each independently bound auth input explicit.
    pub(crate) fn begin(
        signer: S,
        pinned_server_key: [u8; 72],
        registration_id: [u8; 16],
        profile_epoch: u64,
        requested_scope: RequestedScope,
        client_context: UnixPeerContextV2,
        server_context: UnixPeerContextV2,
        client_nonce: [u8; 32],
        started_at_utc_ms: i64,
    ) -> Result<Self, ErrorCode> {
        if registration_id == [0; 16]
            || profile_epoch == 0
            || client_nonce == [0; 32]
            || started_at_utc_ms < 0
            || requested_scope.scope_model_version() != 1
            || client_context.platform != server_context.platform
            || client_context.effective_uid != server_context.effective_uid
        {
            return Err(ErrorCode::InvalidInput);
        }
        let public = signer.public_key().map_err(|_| ErrorCode::AuthRequired)?;
        hantong_platform::validate_public_key(&pinned_server_key)?;
        let client_key_hash = sha256(&public)?;
        let pinned_server_key_hash = sha256(&pinned_server_key)?;
        Ok(Self {
            signer,
            registration_id,
            profile_epoch,
            requested_scope,
            client_context,
            server_context,
            client_nonce,
            client_key_hash,
            pinned_server_key,
            pinned_server_key_hash,
            started_at_utc_ms,
            started: Instant::now(),
            connection: None,
        })
    }

    /// Starts authentication over an already connected, kernel-identified Unix socket.
    /// The socket remains owned by the pending state until `respond_connected` succeeds.
    #[allow(clippy::too_many_arguments)]
    pub fn begin_connected(
        signer: S,
        connection: UnixLocalConnection,
        pinned_server_key: [u8; 72],
        registration_id: [u8; 16],
        profile_epoch: u64,
        requested_scope: RequestedScope,
        client_nonce: [u8; 32],
        started_at_utc_ms: i64,
    ) -> Result<Self, ErrorCode> {
        let server_context = connection.peer_context().clone();
        let client_context = hantong_platform::unix_peer::current_peer_context()?;
        let mut pending = Self::begin(
            signer,
            pinned_server_key,
            registration_id,
            profile_epoch,
            requested_scope,
            client_context,
            server_context,
            client_nonce,
            started_at_utc_ms,
        )?;
        pending.connection = Some(connection);
        Ok(pending)
    }

    pub fn client_nonce(&self) -> [u8; 32] {
        self.client_nonce
    }

    pub(crate) fn registration_id(&self) -> [u8; 16] {
        self.registration_id
    }

    pub(crate) fn profile_epoch(&self) -> u64 {
        self.profile_epoch
    }

    pub fn requested_scope(&self) -> &RequestedScope {
        &self.requested_scope
    }

    /// Gives the caller access to the owned socket only while the handshake is
    /// pending, for sending and receiving authentication frames.
    pub fn transport(&mut self) -> Result<&mut UnixLocalConnection, ErrorCode> {
        self.connection.as_mut().ok_or(ErrorCode::AuthRequired)
    }

    fn respond_inner(
        &mut self,
        challenge: UnixAuthTranscriptV2,
        server_signature: &[u8; 64],
        now_utc_ms: i64,
    ) -> Result<[u8; 64], ErrorCode> {
        if now_utc_ms < self.started_at_utc_ms
            || now_utc_ms - self.started_at_utc_ms >= AUTH_WINDOW_MS
            || self.started.elapsed() >= Duration::from_millis(AUTH_WINDOW_MS as u64)
            || challenge.registration_id != self.registration_id
            || challenge.profile_epoch != self.profile_epoch
            || challenge.client_nonce != self.client_nonce
            || challenge.scope != self.requested_scope
            || challenge.client != self.client_context
            || challenge.server != self.server_context
            || challenge.client_public_key_hash != self.client_key_hash
            || challenge.server_public_key_hash != self.pinned_server_key_hash
            || challenge.issued_at_utc_ms < self.started_at_utc_ms
            || challenge.issued_at_utc_ms > now_utc_ms
            || challenge.expires_at_utc_ms <= now_utc_ms
            || challenge
                .expires_at_utc_ms
                .checked_sub(challenge.issued_at_utc_ms)
                .is_none_or(|lifetime| lifetime > AUTH_WINDOW_MS)
        {
            return Err(ErrorCode::AuthRequired);
        }
        let server_bytes = challenge
            .encode(SignerRole::Server)
            .map_err(|_| ErrorCode::AuthRequired)?;
        if !verify_signature(&self.pinned_server_key, &server_bytes, server_signature)? {
            return Err(ErrorCode::AuthRequired);
        }
        let client_bytes = challenge
            .encode(SignerRole::Client)
            .map_err(|_| ErrorCode::AuthRequired)?;
        self.signer
            .sign(&client_bytes)
            .map_err(|_| ErrorCode::AuthRequired)
    }

    #[cfg(test)]
    pub(crate) fn respond(
        mut self,
        challenge: UnixAuthTranscriptV2,
        server_signature: &[u8; 64],
        now_utc_ms: i64,
    ) -> Result<[u8; 64], ErrorCode> {
        self.respond_inner(challenge, server_signature, now_utc_ms)
    }

    /// Completes authentication and returns the verified socket with the client proof.
    /// A failed authentication drops the socket without exposing it to the caller.
    pub fn respond_connected(
        mut self,
        challenge: UnixAuthTranscriptV2,
        server_signature: &[u8; 64],
        now_utc_ms: i64,
    ) -> Result<(UnixLocalConnection, [u8; 64]), ErrorCode> {
        let connection = self.connection.take().ok_or(ErrorCode::InvalidInput)?;
        let signature = self.respond_inner(challenge, server_signature, now_utc_ms)?;
        Ok((connection, signature))
    }
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
        fn sign(&mut self, message: &[u8]) -> Result<[u8; 64], PlatformError> {
            self.0
                .sign(&ring::rand::SystemRandom::new(), message)
                .map_err(|_| PlatformError::Denied)
                .and_then(|s| s.as_ref().try_into().map_err(|_| PlatformError::Denied))
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
    fn sec03_unix_client_roundtrip_verifies_server_and_signs_bound_transcript() {
        let mut server = TestSigner::new();
        let client = TestSigner::new();
        let client_public = client.blob();
        let server_public = server.blob();
        let pending = PendingUnixAuthentication::begin(
            client,
            server_public,
            [1; 16],
            3,
            scope(),
            peer(2),
            peer(1),
            [4; 32],
            1000,
        )
        .unwrap();
        let transcript = UnixAuthTranscriptV2 {
            registration_id: [1; 16],
            profile_epoch: 3,
            issued_at_utc_ms: 1000,
            expires_at_utc_ms: 31000,
            server_instance_id: [2; 32],
            server_nonce: [3; 32],
            client_nonce: [4; 32],
            server_public_key_hash: sha256(&server_public).unwrap(),
            client_public_key_hash: sha256(&client_public).unwrap(),
            server: peer(1),
            client: peer(2),
            scope: scope(),
        };
        let server_signature = server
            .sign(&transcript.encode(SignerRole::Server).unwrap())
            .unwrap();
        let client_signature = pending
            .respond(transcript.clone(), &server_signature, 1001)
            .unwrap();
        assert!(
            hantong_platform::verify_signature(
                &client_public,
                &transcript.encode(SignerRole::Client).unwrap(),
                &client_signature
            )
            .unwrap()
        );
    }

    #[test]
    fn sec03_unix_client_rejects_nonce_scope_peer_and_expiry_tampering() {
        let mut server = TestSigner::new();
        let client = TestSigner::new();
        let client_public = client.blob();
        let server_public = server.blob();
        let make = |nonce, scope, client_peer, expires| UnixAuthTranscriptV2 {
            registration_id: [1; 16],
            profile_epoch: 3,
            issued_at_utc_ms: 1000,
            expires_at_utc_ms: expires,
            server_instance_id: [2; 32],
            server_nonce: [3; 32],
            client_nonce: nonce,
            server_public_key_hash: sha256(&server_public).unwrap(),
            client_public_key_hash: sha256(&client_public).unwrap(),
            server: peer(1),
            client: client_peer,
            scope,
        };
        let mut pending = PendingUnixAuthentication::begin(
            client,
            server_public,
            [1; 16],
            3,
            scope(),
            peer(2),
            peer(1),
            [4; 32],
            1000,
        )
        .unwrap();
        let valid = make([4; 32], scope(), peer(2), 31000);
        let signature = server
            .sign(&valid.encode(SignerRole::Server).unwrap())
            .unwrap();
        let mut expired = valid.clone();
        expired.expires_at_utc_ms = 900;
        for transcript in [
            make([5; 32], scope(), peer(2), 31000),
            make(
                [4; 32],
                RequestedScope::new(vec!["other".into()], vec!["mail.messages.get".into()])
                    .unwrap(),
                peer(2),
                31000,
            ),
            make([4; 32], scope(), peer(9), 31000),
            expired,
        ] {
            assert!(pending.respond(transcript, &signature, 1001).is_err());
            pending = PendingUnixAuthentication::begin(
                TestSigner::new(),
                server_public,
                [1; 16],
                3,
                scope(),
                peer(2),
                peer(1),
                [4; 32],
                1000,
            )
            .unwrap();
        }
    }
}
