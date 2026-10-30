//! Fixed-path profile loading and binding to the existing local CNG identity.

use crate::{
    PendingClientAuthentication,
    authenticated_wire::{self, AuthenticatedClient},
    profile::ParsedClientProfile,
};
use hantong_contracts::ErrorCode;
use hantong_contracts::auth::{AppReadTool, RequestedScope};
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
    scope: RequestedScope,
}

impl<S: AsyncRead + AsyncWrite + Unpin> AuthenticatedDefaultProfile<S> {
    pub fn accounts(&self) -> &[String] {
        &self.accounts
    }
    pub fn scope(&self) -> &RequestedScope {
        &self.scope
    }
    pub fn tools(&self) -> &[String] {
        self.scope.tools()
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
    authenticate_default_for(None).await
}

/// Requests only one explicitly granted app read and no account scope.
/// Existing registration, profile bytes, key, and server pin remain unchanged.
pub async fn authenticate_default_app_read(
    tool: AppReadTool,
) -> Result<AuthenticatedDefaultProfile<impl AsyncRead + AsyncWrite + Unpin>, ErrorCode> {
    authenticate_default_for(Some(tool)).await
}

async fn authenticate_default_for(
    app_read: Option<AppReadTool>,
) -> Result<AuthenticatedDefaultProfile<impl AsyncRead + AsyncWrite + Unpin>, ErrorCode> {
    let loaded = hantong_platform::load_default_client_profile()?;
    let profile = ParsedClientProfile::from_json(loaded.bytes())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ErrorCode::AuthRequired)?
        .as_millis()
        .try_into()
        .map_err(|_| ErrorCode::AuthRequired)?;
    let pending = match app_read {
        Some(tool) => prepare_app_profile(&profile, tool, now)?,
        None => prepare_profile(&profile, now)?,
    };
    let scope = pending.requested_scope().clone();
    let client = authenticated_wire::authenticate(pending, "default-profile-auth-1").await?;
    Ok(AuthenticatedDefaultProfile {
        client,
        loaded,
        accounts: scope.accounts().to_vec(),
        scope,
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
    prepare_profile_with_scope(profile, profile.scope().clone(), now)
}

fn prepare_app_profile(
    profile: &ParsedClientProfile,
    tool: AppReadTool,
    now: i64,
) -> Result<PendingClientAuthentication, ErrorCode> {
    if !profile
        .scope()
        .tools()
        .iter()
        .any(|allowed| allowed == tool.as_str())
    {
        return Err(ErrorCode::ScopeDenied);
    }
    let scope = RequestedScope::new(vec![], vec![tool.as_str().into()])?;
    prepare_profile_with_scope(profile, scope, now)
}

fn prepare_profile_with_scope(
    profile: &ParsedClientProfile,
    scope: RequestedScope,
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
        scope,
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
    fn cli15_trusted_profile_preserves_folder_scope_in_auth_transcript() {
        let id = id();
        let client = OwnedKey::registered(id);
        let server = OwnedKey::random();
        let scope = RequestedScope::new(
            vec!["account_a".into()],
            hantong_contracts::auth::default_cli_folder_tools(),
        )
        .unwrap();
        let profile = ParsedClientProfile::new_with_scope(
            id,
            client_key_id_for_registration(id).unwrap(),
            client.key().public_key().unwrap(),
            server.key().public_key().unwrap(),
            3,
            scope.clone(),
        )
        .unwrap();
        let pending = prepare_profile(&profile, 1000).unwrap();
        assert_eq!(pending.requested_scope(), &scope);
        assert_eq!(pending.requested_scope().tools(), scope.tools());
        let context = current_peer_context().unwrap();
        let challenge = AuthTranscript {
            registration_id: id,
            profile_epoch: 3,
            issued_at_utc_ms: 1000,
            expires_at_utc_ms: 31000,
            server_instance_id: random_nonce().unwrap(),
            server_nonce: random_nonce().unwrap(),
            client_nonce: pending.client_nonce(),
            server_key_hash: sha256(profile.server_public_key()).unwrap(),
            client_key_hash: sha256(profile.client_public_key()).unwrap(),
            server: context.clone(),
            client: context,
            scope: scope.clone(),
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
                &response,
            )
            .unwrap()
        );
    }

    #[test]
    fn cli15_metadata_scope_trusted_profile_preserves_exact_requested_scope() {
        let id = id();
        let client = OwnedKey::registered(id);
        let server = OwnedKey::random();
        let scope = RequestedScope::new(
            vec!["account_a".into()],
            hantong_contracts::auth::default_cli_metadata_tools(),
        )
        .unwrap();
        let profile = ParsedClientProfile::new_with_scope(
            id,
            client_key_id_for_registration(id).unwrap(),
            client.key().public_key().unwrap(),
            server.key().public_key().unwrap(),
            4,
            scope.clone(),
        )
        .unwrap();
        let pending = prepare_profile(&profile, 1000).unwrap();
        assert_eq!(pending.requested_scope(), &scope);
        assert!(
            pending
                .requested_scope()
                .permits_account("account_a", "mail.messages.search")
        );
        assert!(
            !pending
                .requested_scope()
                .permits_account("account_b", "mail.messages.search")
        );
    }

    #[test]
    fn cli15_app_read_scope_minimal_auth_binds_exact_tool_and_preserves_profile() {
        let id = id();
        let client = OwnedKey::registered(id);
        let server = OwnedKey::random();
        let profile = ParsedClientProfile::new_with_scope(
            id,
            client_key_id_for_registration(id).unwrap(),
            client.key().public_key().unwrap(),
            server.key().public_key().unwrap(),
            5,
            RequestedScope::new(
                vec!["account_a".into()],
                hantong_contracts::auth::default_cli_app_read_tools(),
            )
            .unwrap(),
        )
        .unwrap();
        let before = profile.to_json().unwrap();
        for tool in [
            AppReadTool::DisplaySettingsGet,
            AppReadTool::DiagnosticsSnapshot,
        ] {
            let pending = prepare_app_profile(&profile, tool, 1000).unwrap();
            let requested = RequestedScope::new(vec![], vec![tool.as_str().into()]).unwrap();
            assert_eq!(pending.requested_scope(), &requested);
            assert!(
                !pending
                    .requested_scope()
                    .permits_account("account_a", tool.as_str())
            );
            let context = current_peer_context().unwrap();
            let challenge = AuthTranscript {
                registration_id: id,
                profile_epoch: 5,
                issued_at_utc_ms: 1000,
                expires_at_utc_ms: 31000,
                server_instance_id: random_nonce().unwrap(),
                server_nonce: random_nonce().unwrap(),
                client_nonce: pending.client_nonce(),
                server_key_hash: sha256(profile.server_public_key()).unwrap(),
                client_key_hash: sha256(profile.client_public_key()).unwrap(),
                server: context.clone(),
                client: context,
                scope: requested,
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
            let pending = prepare_app_profile(&profile, tool, 1000).unwrap();
            let mut widened = challenge;
            widened.client_nonce = pending.client_nonce();
            widened.scope = profile.scope().clone();
            let signature = server
                .key()
                .sign(&widened.encode(SignerRole::Server).unwrap())
                .unwrap();
            assert_eq!(
                pending.respond(widened, &signature, 1001),
                Err(ErrorCode::AuthRequired)
            );
        }
        assert_eq!(
            prepare_profile(&profile, 1000).unwrap().requested_scope(),
            profile.scope()
        );
        assert_eq!(profile.to_json().unwrap(), before);
    }

    #[test]
    fn cli15_app_read_scope_without_grant_stops_before_existing_key_access() {
        let id = id();
        let arbitrary = OwnedKey::random();
        let server = OwnedKey::random();
        assert!(matches!(
            ClientSigningKey::open_for_registration(id),
            Err(ErrorCode::NotFound)
        ));
        for tools in [
            hantong_contracts::auth::default_cli_legacy_tools(),
            hantong_contracts::auth::default_cli_folder_tools(),
            hantong_contracts::auth::default_cli_uid_tools(),
            hantong_contracts::auth::default_cli_metadata_tools(),
        ] {
            let profile = ParsedClientProfile::new_with_scope(
                id,
                client_key_id_for_registration(id).unwrap(),
                arbitrary.key().public_key().unwrap(),
                server.key().public_key().unwrap(),
                1,
                RequestedScope::new(vec![], tools).unwrap(),
            )
            .unwrap();
            for tool in [
                AppReadTool::DisplaySettingsGet,
                AppReadTool::DiagnosticsSnapshot,
            ] {
                assert!(matches!(
                    prepare_app_profile(&profile, tool, 1000),
                    Err(ErrorCode::ScopeDenied)
                ));
            }
        }
        assert!(matches!(
            ClientSigningKey::open_for_registration(id),
            Err(ErrorCode::NotFound)
        ));
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
