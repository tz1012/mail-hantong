//! Client-side half of the Windows CNG challenge exchange.
//!
//! This module deliberately has no transport or registration persistence. The
//! caller must obtain the key name, pinned server key, registration id, epoch,
//! and allowed scope from a trusted profile store. A pending exchange is
//! single-use and only returns one signature.

use hantong_contracts::{
    ErrorCode,
    auth::{AuthTranscript, PeerContext, RequestedScope, SignerRole},
};
use hantong_platform::{ClientSigningKey, sha256, validate_public_key, verify_signature};
use std::time::{Duration, Instant};

const AUTH_WINDOW_MS: i64 = 30_000;

/// A connection-local client challenge state. It cannot be cloned or reused.
pub struct PendingClientAuthentication {
    key: ClientSigningKey,
    registration_id: [u8; 16],
    profile_epoch: u64,
    requested_scope: RequestedScope,
    client_context: PeerContext,
    client_nonce: [u8; 32],
    client_key_hash: [u8; 32],
    pinned_server_key_hash: [u8; 32],
    pinned_server_key: [u8; 72],
    started_at_utc_ms: i64,
    started: Instant,
}

impl std::fmt::Debug for PendingClientAuthentication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingClientAuthentication")
            .field("registration_id", &"<redacted>")
            .field("profile_epoch", &self.profile_epoch)
            .field("requested_scope", &"<redacted>")
            .field("client_nonce", &"<redacted>")
            .finish()
    }
}

impl PendingClientAuthentication {
    /// Opens an existing current-user key. This function never creates or
    /// registers a key and does not accept a path or private-key bytes.
    pub fn begin(
        key_name: &str,
        pinned_server_key: [u8; 72],
        registration_id: [u8; 16],
        profile_epoch: u64,
        requested_scope: RequestedScope,
        started_at_utc_ms: i64,
    ) -> Result<Self, ErrorCode> {
        if profile_epoch == 0 || started_at_utc_ms < 0 {
            return Err(ErrorCode::InvalidInput);
        }
        let started = Instant::now();
        let client_nonce = hantong_platform::random_nonce()?;
        // Validate the public key before retaining it. No private key material
        // enters this module.
        let key = ClientSigningKey::open(key_name).map_err(|_| ErrorCode::AuthRequired)?;
        let public = key.public_key().map_err(|_| ErrorCode::AuthRequired)?;
        let client_context = hantong_platform::current_peer_context()?;
        let client_key_hash = sha256(&public)?;
        validate_public_key(&pinned_server_key)?;
        Ok(Self {
            key,
            registration_id,
            profile_epoch,
            requested_scope,
            client_context: client_context.clone(),
            client_nonce,
            client_key_hash,
            pinned_server_key_hash: sha256(&pinned_server_key)?,
            pinned_server_key,
            started_at_utc_ms,
            started,
        })
    }

    /// Nonce to include in the server challenge request. It is not a bearer
    /// token and is only useful with this pending exchange.
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

    /// Verifies and consumes one server challenge, then signs the exact same
    /// transcript with the client role. All failures consume the state.
    pub fn respond(
        self,
        challenge: AuthTranscript,
        server_signature: &[u8],
        now_utc_ms: i64,
    ) -> Result<[u8; 64], ErrorCode> {
        if now_utc_ms < 0 || self.started.elapsed() >= Duration::from_millis(AUTH_WINDOW_MS as u64)
        {
            return Err(ErrorCode::AuthRequired);
        }
        if challenge.registration_id != self.registration_id
            || challenge.profile_epoch != self.profile_epoch
            || challenge.client_nonce != self.client_nonce
            || challenge.scope != self.requested_scope
            || challenge.client != self.client_context
            || challenge.server != self.client_context
            || challenge.client_key_hash != self.client_key_hash
            || challenge.server_key_hash != self.pinned_server_key_hash
            || challenge.issued_at_utc_ms < self.started_at_utc_ms
            || challenge.issued_at_utc_ms > now_utc_ms
            || challenge.expires_at_utc_ms <= now_utc_ms
            || challenge.expires_at_utc_ms < challenge.issued_at_utc_ms
            || challenge.expires_at_utc_ms
                > challenge
                    .issued_at_utc_ms
                    .checked_add(AUTH_WINDOW_MS)
                    .ok_or(ErrorCode::AuthRequired)?
        {
            return Err(ErrorCode::AuthRequired);
        }
        let server_bytes = challenge
            .encode(SignerRole::Server)
            .map_err(|_| ErrorCode::AuthRequired)?;
        if !verify_signature(&self.pinned_server_key, &server_bytes, server_signature)
            .map_err(|_| ErrorCode::AuthRequired)?
        {
            return Err(ErrorCode::AuthRequired);
        }
        let client_bytes = challenge
            .encode(SignerRole::Client)
            .map_err(|_| ErrorCode::AuthRequired)?;
        self.key
            .sign(&client_bytes)
            .map_err(|_| ErrorCode::AuthRequired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_platform::{ClientSigningKey, current_peer_context, random_nonce};

    struct TestKey(Option<ClientSigningKey>);
    impl TestKey {
        fn new() -> Self {
            Self(Some(ClientSigningKey::create().unwrap()))
        }
        fn get(&self) -> &ClientSigningKey {
            self.0.as_ref().unwrap()
        }
    }
    impl Drop for TestKey {
        fn drop(&mut self) {
            if let Some(key) = self.0.take() {
                let _ = key.delete();
            }
        }
    }

    fn scope() -> RequestedScope {
        RequestedScope::new(vec!["account_a".into()], vec!["mail.messages.get".into()]).unwrap()
    }

    fn challenge(
        server: &ClientSigningKey,
        client: &ClientSigningKey,
        id: [u8; 16],
        epoch: u64,
        nonce: [u8; 32],
        context: PeerContext,
    ) -> (AuthTranscript, [u8; 64]) {
        let public = server.public_key().unwrap();
        let client_public = client.public_key().unwrap();
        let transcript = AuthTranscript {
            registration_id: id,
            profile_epoch: epoch,
            issued_at_utc_ms: 1_000,
            expires_at_utc_ms: 31_000,
            server_instance_id: random_nonce().unwrap(),
            server_nonce: random_nonce().unwrap(),
            client_nonce: nonce,
            server_key_hash: sha256(&public).unwrap(),
            client_key_hash: sha256(&client_public).unwrap(),
            server: context.clone(),
            client: context,
            scope: scope(),
        };
        let signature = server
            .sign(&transcript.encode(SignerRole::Server).unwrap())
            .unwrap();
        (transcript, signature)
    }

    #[test]
    fn sec03_client_opens_existing_key_and_validates_all_challenge_bindings() {
        let server = TestKey::new();
        let client = TestKey::new();
        let context = current_peer_context().unwrap();
        let pending = PendingClientAuthentication::begin(
            client.get().name(),
            server.get().public_key().unwrap(),
            [1; 16],
            1,
            scope(),
            1_000,
        )
        .unwrap();
        let nonce = pending.client_nonce();
        let (transcript, signature) = challenge(
            server.get(),
            client.get(),
            [1; 16],
            1,
            nonce,
            context.clone(),
        );
        let result = pending
            .respond(transcript.clone(), &signature, 1_001)
            .unwrap();
        assert!(
            verify_signature(
                &client.get().public_key().unwrap(),
                &transcript.encode(SignerRole::Client).unwrap(),
                &result
            )
            .unwrap()
        );
        let pending = PendingClientAuthentication::begin(
            client.get().name(),
            server.get().public_key().unwrap(),
            [1; 16],
            1,
            scope(),
            1_000,
        )
        .unwrap();
        // A consumed challenge cannot be reused with a new pending exchange:
        // the fresh pending has a different nonce.
        let fresh = pending.respond(transcript, &signature, 1_001);
        assert_eq!(fresh, Err(ErrorCode::AuthRequired));
    }

    #[test]
    fn sec03_client_rejects_each_tampered_binding_after_server_resigns() {
        let server = TestKey::new();
        let client = TestKey::new();
        let context = current_peer_context().unwrap();

        let run = |mutate: fn(&mut AuthTranscript)| {
            let pending = PendingClientAuthentication::begin(
                client.get().name(),
                server.get().public_key().unwrap(),
                [2; 16],
                1,
                scope(),
                1_000,
            )
            .unwrap();
            let nonce = pending.client_nonce();
            let (mut transcript, _) = challenge(
                server.get(),
                client.get(),
                [2; 16],
                1,
                nonce,
                context.clone(),
            );
            mutate(&mut transcript);
            let signature = server
                .get()
                .sign(&transcript.encode(SignerRole::Server).unwrap())
                .unwrap();
            assert_eq!(
                pending.respond(transcript, &signature, 1_001),
                Err(ErrorCode::AuthRequired)
            );
        };

        run(|t| t.registration_id = [3; 16]);
        run(|t| t.profile_epoch = 2);
        run(|t| t.client_nonce = [9; 32]);
        run(|t| {
            t.scope =
                RequestedScope::new(vec!["account_b".into()], vec!["mail.messages.get".into()])
                    .unwrap()
        });
        run(|t| t.client = PeerContext::new("S-1-5-21-1", "S-1-5-5-0-2").unwrap());
        run(|t| t.server = PeerContext::new("S-1-5-21-1", "S-1-5-5-0-2").unwrap());
        run(|t| t.server_key_hash = [9; 32]);
        run(|t| t.client_key_hash = [9; 32]);
        run(|t| {
            t.issued_at_utc_ms = 999;
            t.expires_at_utc_ms = 30_999;
        });
        run(|t| t.issued_at_utc_ms = 1_002);
        run(|t| t.expires_at_utc_ms = 1_001);
        let mut too_long = challenge(
            server.get(),
            client.get(),
            [2; 16],
            1,
            [2; 32],
            context.clone(),
        )
        .0;
        too_long.expires_at_utc_ms = 31_001;
        assert_eq!(
            too_long.encode(SignerRole::Server),
            Err(ErrorCode::InvalidInput)
        );

        // Reflection: a client-role signature cannot satisfy the server-role
        // verification performed before the client signs.
        let pending = PendingClientAuthentication::begin(
            client.get().name(),
            server.get().public_key().unwrap(),
            [2; 16],
            1,
            scope(),
            1_000,
        )
        .unwrap();
        let (transcript, _) = challenge(
            server.get(),
            client.get(),
            [2; 16],
            1,
            pending.client_nonce(),
            context,
        );
        let reflected = server
            .get()
            .sign(&transcript.encode(SignerRole::Client).unwrap())
            .unwrap();
        assert_eq!(
            pending.respond(transcript, &reflected, 1_001),
            Err(ErrorCode::AuthRequired)
        );
    }

    #[test]
    fn sec03_client_rejects_monotonic_deadline_and_wrong_pinned_server() {
        let server = TestKey::new();
        let wrong = TestKey::new();
        let client = TestKey::new();
        let context = current_peer_context().unwrap();
        let pending = PendingClientAuthentication::begin(
            client.get().name(),
            server.get().public_key().unwrap(),
            [2; 16],
            1,
            scope(),
            1_000,
        )
        .unwrap();
        let nonce = pending.client_nonce();
        let (transcript, signature) = challenge(
            server.get(),
            client.get(),
            [2; 16],
            1,
            nonce,
            context.clone(),
        );
        let mut pending = pending;
        pending.started = Instant::now() - Duration::from_secs(31);
        assert_eq!(
            pending.respond(transcript, &signature, 1_001),
            Err(ErrorCode::AuthRequired)
        );

        let pending = PendingClientAuthentication::begin(
            client.get().name(),
            wrong.get().public_key().unwrap(),
            [2; 16],
            1,
            scope(),
            1_000,
        )
        .unwrap();
        let (transcript, signature) = challenge(
            server.get(),
            client.get(),
            [2; 16],
            1,
            pending.client_nonce(),
            context,
        );
        assert_eq!(
            pending.respond(transcript, &signature, 1_001),
            Err(ErrorCode::AuthRequired)
        );
    }
}
