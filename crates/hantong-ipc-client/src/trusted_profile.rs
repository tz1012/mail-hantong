//! Fixed-path profile loading and binding to the existing local CNG identity.

use crate::{
    PendingClientAuthentication,
    authenticated_wire::{self, AuthenticatedClient},
    profile::ParsedClientProfile,
};
use hantong_contracts::ErrorCode;
use hantong_platform::{
    ClientSigningKey, LoadedClientProfile, client_key_id_for_registration, validate_public_key,
};
use tokio::io::{AsyncRead, AsyncWrite};

/// One mutually authenticated connection. The pinned file/ancestor handles stay
/// alive until the one protected exchange completes or this value is dropped.
pub struct AuthenticatedDefaultProfile<S> {
    client: AuthenticatedClient<S>,
    loaded: LoadedClientProfile,
    accounts: Vec<String>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> AuthenticatedDefaultProfile<S> {
    pub fn accounts(&self) -> &[String] {
        &self.accounts
    }
    pub async fn exchange(
        self,
        id: &str,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<Vec<u8>, ErrorCode> {
        let result = self.client.exchange(id, method, params).await;
        drop(self.loaded);
        result
    }
}

/// Uses only Known Folder/mail-hantong/profiles/default.json. It never creates,
/// repairs, replaces, or accepts an alternative profile, key, or server pin.
pub async fn authenticate_default()
-> Result<AuthenticatedDefaultProfile<impl AsyncRead + AsyncWrite + Unpin>, ErrorCode> {
    let loaded = hantong_platform::load_default_client_profile()?;
    let profile = ParsedClientProfile::from_json(loaded.bytes())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ErrorCode::AuthRequired)?
        .as_millis()
        .try_into()
        .map_err(|_| ErrorCode::AuthRequired)?;
    let pending = prepare_profile(&profile, now)?;
    let client = authenticated_wire::authenticate(pending, "default-profile-auth-1").await?;
    Ok(AuthenticatedDefaultProfile {
        client,
        loaded,
        accounts: profile.scope().accounts().to_vec(),
    })
}

/// Checks the registered identity and immediately drops the authenticated
/// connection. This does not call any protected tool or issue an approval.
pub fn verify_default() -> Result<(), ErrorCode> {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .block_on(async {
            let connection = authenticate_default().await?;
            drop(connection);
            Ok(())
        })
}

fn prepare_profile(
    profile: &ParsedClientProfile,
    now: i64,
) -> Result<PendingClientAuthentication, ErrorCode> {
    let registration = *profile.registration_id();
    if client_key_id_for_registration(registration).map_err(|_| ErrorCode::AuthRequired)?
        != *profile.client_key_id()
    {
        return Err(ErrorCode::AuthRequired);
    }
    // Open only the registration-derived selector, never a name supplied by
    // JSON or a caller. Retain this exact handle through the signature exchange.
    let key = ClientSigningKey::open_for_registration(registration)
        .map_err(|_| ErrorCode::AuthRequired)?;
    let public = key.public_key().map_err(|_| ErrorCode::AuthRequired)?;
    if public != *profile.client_public_key() {
        return Err(ErrorCode::AuthRequired);
    }
    validate_public_key(&public).map_err(|_| ErrorCode::AuthRequired)?;
    validate_public_key(profile.server_public_key()).map_err(|_| ErrorCode::AuthRequired)?;
    PendingClientAuthentication::begin_existing(
        key,
        *profile.server_public_key(),
        registration,
        profile.profile_epoch(),
        profile.scope().clone(),
        now,
    )
    .map_err(|_| ErrorCode::AuthRequired)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::auth::{AuthTranscript, SignerRole};
    use hantong_platform::{current_peer_context, random_nonce, sha256, verify_signature};

    struct OwnedKey(Option<ClientSigningKey>);
    impl OwnedKey {
        fn random() -> Self {
            Self(Some(ClientSigningKey::create().unwrap()))
        }
        fn registered(id: [u8; 16]) -> Self {
            Self(Some(ClientSigningKey::create_for_registration(id).unwrap()))
        }
        fn key(&self) -> &ClientSigningKey {
            self.0.as_ref().unwrap()
        }
    }
    impl Drop for OwnedKey {
        fn drop(&mut self) {
            if let Some(key) = self.0.take() {
                let _ = key.delete();
            }
        }
    }
    fn id() -> [u8; 16] {
        random_nonce().unwrap()[..16].try_into().unwrap()
    }
    fn profile(
        id: [u8; 16],
        client: &OwnedKey,
        server: &OwnedKey,
        accounts: Vec<String>,
    ) -> ParsedClientProfile {
        ParsedClientProfile::new(
            id,
            client_key_id_for_registration(id).unwrap(),
            client.key().public_key().unwrap(),
            server.key().public_key().unwrap(),
            1,
            accounts,
        )
        .unwrap()
    }
    fn changed(
        value: &ParsedClientProfile,
        key_id: [u8; 32],
        client: [u8; 72],
        server: [u8; 72],
    ) -> ParsedClientProfile {
        ParsedClientProfile::new(
            *value.registration_id(),
            key_id,
            client,
            server,
            value.profile_epoch(),
            value.scope().accounts().to_vec(),
        )
        .unwrap()
    }

    #[test]
    fn cli07_trusted_profile_binds_existing_key_and_preserves_exact_scope() {
        let id = id();
        let client = OwnedKey::registered(id);
        let server = OwnedKey::random();
        for accounts in [vec![], vec!["account_b".into(), "account_a".into()]] {
            let profile = profile(id, &client, &server, accounts);
            let pending = prepare_profile(&profile, 1000).unwrap();
            assert_eq!(pending.requested_scope(), profile.scope());
            let context = current_peer_context().unwrap();
            let challenge = AuthTranscript {
                registration_id: id,
                profile_epoch: 1,
                issued_at_utc_ms: 1000,
                expires_at_utc_ms: 31000,
                server_instance_id: random_nonce().unwrap(),
                server_nonce: random_nonce().unwrap(),
                client_nonce: pending.client_nonce(),
                server_key_hash: sha256(profile.server_public_key()).unwrap(),
                client_key_hash: sha256(profile.client_public_key()).unwrap(),
                server: context.clone(),
                client: context,
                scope: profile.scope().clone(),
            };
            let signature = server
                .key()
                .sign(&challenge.encode(SignerRole::Server).unwrap())
                .unwrap();
            let response = pending
                .respond(challenge.clone(), &signature, 1001)
                .unwrap();
            assert!(
                verify_signature(
                    profile.client_public_key(),
                    &challenge.encode(SignerRole::Client).unwrap(),
                    &response
                )
                .unwrap()
            );
        }
    }

    #[test]
    fn cli07_trusted_profile_rejects_selector_public_key_and_curve_mismatches() {
        let id = id();
        let client = OwnedKey::registered(id);
        let server = OwnedKey::random();
        let wrong = OwnedKey::random();
        let profile = profile(id, &client, &server, vec![]);
        let mut invalid_point = [0; 72];
        invalid_point[..8].copy_from_slice(b"ECS1\x20\0\0\0");
        for altered in [
            changed(
                &profile,
                [0xaa; 32],
                *profile.client_public_key(),
                *profile.server_public_key(),
            ),
            changed(
                &profile,
                *profile.client_key_id(),
                wrong.key().public_key().unwrap(),
                *profile.server_public_key(),
            ),
            changed(
                &profile,
                *profile.client_key_id(),
                invalid_point,
                *profile.server_public_key(),
            ),
            changed(
                &profile,
                *profile.client_key_id(),
                *profile.client_public_key(),
                invalid_point,
            ),
        ] {
            assert!(matches!(
                prepare_profile(&altered, 1000),
                Err(ErrorCode::AuthRequired)
            ));
        }
        assert_eq!(
            ClientSigningKey::open_for_registration(id)
                .unwrap()
                .public_key()
                .unwrap(),
            *profile.client_public_key()
        );
    }

    #[test]
    fn cli07_trusted_profile_missing_key_is_never_created() {
        let id = id();
        let arbitrary = OwnedKey::random();
        let server = OwnedKey::random();
        let profile = profile(id, &arbitrary, &server, vec![]);
        assert!(matches!(
            ClientSigningKey::open_for_registration(id),
            Err(ErrorCode::NotFound)
        ));
        assert!(matches!(
            prepare_profile(&profile, 1000),
            Err(ErrorCode::AuthRequired)
        ));
        assert!(matches!(
            ClientSigningKey::open_for_registration(id),
            Err(ErrorCode::NotFound)
        ));
    }
}
