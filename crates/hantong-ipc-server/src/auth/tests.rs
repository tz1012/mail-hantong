use super::*;
use hantong_platform::{ClientSigningKey, LocalPipeServer, ServerSigningKey, connect};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

struct TestServerKey(Option<ServerSigningKey>);
impl TestServerKey {
    fn new() -> Self {
        Self(Some(
            ServerSigningKey::create(random_nonce().unwrap()).unwrap(),
        ))
    }
    fn get(&self) -> &ServerSigningKey {
        self.0.as_ref().unwrap()
    }
}
impl Drop for TestServerKey {
    fn drop(&mut self) {
        if let Some(key) = self.0.take() {
            let _ = key.delete();
        }
    }
}
fn scope(accounts: &[&str]) -> RequestedScope {
    RequestedScope::new(
        accounts.iter().map(|s| (*s).into()).collect(),
        vec!["mail.messages.get".into()],
    )
    .unwrap()
}

// One OS integration test owns the fixed product endpoint exclusively. The
// binary exchange below is a test fixture, not the future JSON handshake API.
#[tokio::test]
async fn sec03_cng_authentication_is_connection_bound_and_rechecks_revocation() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let server_key = TestServerKey::new();
        let client_key = TestKey::new();
        let server_public = server_key.get().public_key().unwrap();
        let registration = RegisteredClient::from_registration_store(
            [1; 16],
            1,
            client_key.get().public_key().unwrap(),
            scope(&["account_a"]),
            true,
        )
        .unwrap();
        let mut listener = LocalPipeServer::bind().unwrap();
        let instance = random_nonce().unwrap();
        let mut previous_signature = None;
        let mut previous_nonce = None;
        for scenario in [
            "valid",
            "replay",
            "reflection",
            "tampered",
            "utc_expired",
            "clock_rollback",
            "monotonic_expired",
            "epoch",
            "revoked",
            "removed",
            "key_changed",
            "empty_scope",
        ] {
            let registration = &registration;
            let server_public = &server_public;
            let client_key = &client_key;
            let server_key = &server_key;
            let client_nonce = random_nonce().unwrap();
            let server = async {
                let connection = listener.accept().await.unwrap();
                let requested = if scenario == "empty_scope" {
                    scope(&[])
                } else {
                    scope(&["account_a", "account_b"])
                };
                let (mut pending, challenge) = PendingAuthentication::begin(
                    connection,
                    server_key.get(),
                    instance,
                    registration,
                    client_nonce,
                    requested,
                    1000,
                )
                .unwrap();
                assert_eq!(challenge.transcript.server, current_peer_context().unwrap());
                assert_eq!(challenge.transcript.client, current_peer_context().unwrap());
                assert_eq!(
                    challenge.transcript.server_key_hash,
                    sha256(server_public).unwrap()
                );
                if let Some(old) = previous_nonce {
                    assert_ne!(old, challenge.transcript.server_nonce);
                }
                previous_nonce = Some(challenge.transcript.server_nonce);
                let bytes = challenge.transcript.encode(SignerRole::Server).unwrap();
                pending
                    .transport()
                    .write_u32_le(bytes.len() as u32)
                    .await
                    .unwrap();
                pending.transport().write_all(&bytes).await.unwrap();
                pending
                    .transport()
                    .write_all(&challenge.signature)
                    .await
                    .unwrap();
                let mut signature = [0; 64];
                pending
                    .transport()
                    .read_exact(&mut signature)
                    .await
                    .unwrap();
                let mut current = registration.clone();
                let now = match scenario {
                    "utc_expired" => 31000,
                    "clock_rollback" => 999,
                    _ => 1001,
                };
                if scenario == "monotonic_expired" {
                    pending.issued = Instant::now() - Duration::from_secs(31);
                }
                if scenario == "epoch" {
                    current.epoch += 1;
                }
                if scenario == "revoked" {
                    current.active = false;
                }
                if scenario == "key_changed" {
                    current.public_key = *server_public;
                }
                let current_arg = if scenario == "removed" {
                    None
                } else {
                    Some(&current)
                };
                let result = pending.finish(&signature, current_arg, now);
                if scenario == "valid" || scenario == "empty_scope" {
                    let mut authenticated = result.unwrap_or_else(|e| panic!("{scenario}: {e:?}"));
                    assert_eq!(authenticated.registration_id(), &[1; 16]);
                    assert_eq!(authenticated.profile_epoch(), 1);
                    let all_accounts = vec!["account_a".to_owned()];
                    assert_eq!(
                        authenticated.check_accounts_scope(
                            Some(&current),
                            &all_accounts,
                            "mail.messages.get",
                        ),
                        if scenario == "valid" {
                            Ok(())
                        } else {
                            Err(ErrorCode::ScopeDenied)
                        }
                    );
                    assert_eq!(
                        authenticated.check_accounts_scope(
                            Some(&current),
                            &[],
                            "mail.messages.get",
                        ),
                        Err(ErrorCode::InvalidInput)
                    );
                    let mixed_accounts = vec!["account_a".to_owned(), "account_b".to_owned()];
                    assert_eq!(
                        authenticated.check_accounts_scope(
                            Some(&current),
                            &mixed_accounts,
                            "mail.messages.get",
                        ),
                        Err(ErrorCode::ScopeDenied)
                    );
                    let allowed = authenticated.check_account_scope(
                        Some(&current),
                        "account_a",
                        "mail.messages.get",
                    );
                    assert_eq!(
                        allowed,
                        if scenario == "valid" {
                            Ok(())
                        } else {
                            Err(ErrorCode::ScopeDenied)
                        }
                    );
                    assert_eq!(
                        authenticated.check_account_scope(
                            Some(&current),
                            "account_b",
                            "mail.messages.get"
                        ),
                        Err(ErrorCode::ScopeDenied)
                    );
                    assert_eq!(
                        authenticated.check_account_scope(
                            Some(&current),
                            "account_a",
                            "mail.send.execute"
                        ),
                        Err(ErrorCode::ScopeDenied)
                    );
                    let mut removed_account = current.clone();
                    removed_account.scope = scope(&[]);
                    assert_eq!(
                        authenticated.check_account_scope(
                            Some(&removed_account),
                            "account_a",
                            "mail.messages.get"
                        ),
                        Err(ErrorCode::ScopeDenied)
                    );
                    current.epoch += 1;
                    assert_eq!(
                        authenticated.check_account_scope(
                            Some(&current),
                            "account_a",
                            "mail.messages.get"
                        ),
                        Err(ErrorCode::AuthRequired)
                    );
                    assert!(authenticated.transport().is_err());
                    assert_eq!(
                        authenticated.check_account_scope(
                            Some(registration),
                            "account_a",
                            "mail.messages.get"
                        ),
                        Err(ErrorCode::AuthRequired)
                    );
                } else {
                    let expected = match scenario {
                        "revoked" => ErrorCode::ClientSuspended,
                        "removed" => ErrorCode::ClientNotRegistered,
                        _ => ErrorCode::AuthRequired,
                    };
                    assert!(
                        matches!(result,Err(code) if code==expected),
                        "unexpected result for {scenario}"
                    );
                }
            };
            let client = async {
                let mut pipe = connect().await.unwrap();
                let size = pipe.read_u32_le().await.unwrap() as usize;
                assert!(size <= hantong_contracts::auth::MAX_TRANSCRIPT_BYTES);
                let mut transcript = vec![0; size];
                pipe.read_exact(&mut transcript).await.unwrap();
                let mut server_signature = [0; 64];
                pipe.read_exact(&mut server_signature).await.unwrap();
                assert!(verify_signature(server_public, &transcript, &server_signature).unwrap());
                // Fixture knows the fixed layout; a real client must also
                // validate all challenge fields against its request and pin.
                transcript[25] = SignerRole::Client as u8;
                let honest = client_key.get().sign(&transcript).unwrap();
                let response = match scenario {
                    "replay" => previous_signature.unwrap(),
                    "reflection" => server_signature,
                    "tampered" => {
                        let mut bad = honest;
                        bad[0] ^= 1;
                        bad
                    }
                    _ => honest,
                };
                previous_signature = Some(honest);
                pipe.write_all(&response).await.unwrap();
                let mut end = [0; 1];
                assert!(
                    matches!(pipe.read(&mut end).await, Ok(0) | Err(_)),
                    "rejected/revoked connection must close"
                );
            };
            tokio::join!(server, client);
        }
        drop(listener);
        crate::authenticated_wire::tests::run_fixture_cases().await;
    })
    .await
    .unwrap();
}
