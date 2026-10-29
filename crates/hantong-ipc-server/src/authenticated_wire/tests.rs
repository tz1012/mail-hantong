use super::*;
use crate::auth::RegisteredClient;
use hantong_contracts::{
    ErrorCode,
    auth::{AuthTranscript, RequestedScope, SignerRole},
    wire_hex,
};
use hantong_platform::{
    ClientSigningKey, LocalPipeServer, ServerSigningKey, connect, random_nonce, verify_signature,
};
use serde_json::{Value, json};
use std::cell::Cell;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Keys(Option<ClientSigningKey>, Option<ServerSigningKey>);
impl Drop for Keys {
    fn drop(&mut self) {
        if let Some(key) = self.0.take() {
            let _ = key.delete();
        }
        if let Some(key) = self.1.take() {
            let _ = key.delete();
        }
    }
}

// Invoked by the single fixed-endpoint OS test, after its previous listener
// drops. This avoids competing test listeners for the production pipe name.
pub(crate) async fn run_fixture_cases() {
    let keys = Keys(
        Some(ClientSigningKey::create().unwrap()),
        Some(ServerSigningKey::create(random_nonce().unwrap()).unwrap()),
    );
    let client_key = keys.0.as_ref().unwrap();
    let server_key = keys.1.as_ref().unwrap();
    let scope =
        RequestedScope::new(vec!["account_a".into()], vec!["mail.accounts.list".into()]).unwrap();
    let registration = RegisteredClient::from_registration_store(
        [1; 16],
        1,
        client_key.public_key().unwrap(),
        scope.clone(),
        true,
    )
    .unwrap();
    let instance = random_nonce().unwrap();
    let mut listener = LocalPipeServer::bind().unwrap();
    for scenario in [
        "valid",
        "bad_signature",
        "removed_finish",
        "epoch_finish",
        "wrong_method",
        "missing_begin",
        "epoch_begin",
        "timeout",
        "begin_zero",
        "begin_oversized",
        "finish_oversized",
        "lookup_error",
        "second_request",
        "wrong_receipt",
        "missing_receipt",
        "non_mail",
        "tool_zero",
        "tool_oversized",
    ] {
        let server = async {
            let connection = listener.accept().await.unwrap();
            let mut lookups = 0;
            let mut lookup = |id: &[u8; 16]| {
                assert_eq!(id, &[1; 16]);
                lookups += 1;
                if scenario == "lookup_error" {
                    return Err(ErrorCode::InternalError);
                }
                if scenario == "missing_begin" || (scenario == "removed_finish" && lookups == 2) {
                    return Ok(None);
                }
                if scenario == "epoch_finish" && lookups == 2 {
                    return Ok(Some(
                        RegisteredClient::from_registration_store(
                            [1; 16],
                            2,
                            client_key.public_key().unwrap(),
                            scope.clone(),
                            true,
                        )
                        .unwrap(),
                    ));
                }
                Ok(Some(registration.clone()))
            };
            let deadline = if scenario == "timeout" {
                Duration::from_millis(80)
            } else {
                Duration::from_secs(5)
            };
            let result =
                authenticate_with_deadline(connection, server_key, instance, &mut lookup, deadline)
                    .await;
            if matches!(
                scenario,
                "valid"
                    | "second_request"
                    | "wrong_receipt"
                    | "missing_receipt"
                    | "non_mail"
                    | "tool_zero"
                    | "tool_oversized"
            ) {
                let mut authenticated = result.unwrap();
                assert_eq!(authenticated.registration_id(), &[1; 16]);
                assert_eq!(authenticated.profile_epoch(), 1);
                assert_eq!(
                    authenticated.check_accounts_scope(
                        Some(&registration),
                        &["account_a".into()],
                        "mail.accounts.list"
                    ),
                    Ok(())
                );
                let calls = Cell::new(0);
                let execute_result = execute_once(authenticated, |session, request| {
                    calls.set(calls.get() + 1);
                    assert_eq!(session.registration_id(), &[1; 16]);
                    if scenario == "non_mail" {
                        panic!("non-mail request reached callback")
                    }
                    assert_eq!(request.method, "mail.accounts.list");
                    assert_eq!(request.params, json!({"fixture":true}));
                    Ok(json!({"fixture_count":1}))
                })
                .await;
                match scenario {
                    "valid" => assert_eq!(execute_result, Ok(())),
                    "second_request" | "wrong_receipt" | "non_mail" | "tool_zero"
                    | "tool_oversized" => {
                        assert_eq!(execute_result, Err(ErrorCode::InvalidInput))
                    }
                    "missing_receipt" => {
                        assert_eq!(execute_result, Err(ErrorCode::CoreUnavailable))
                    }
                    _ => unreachable!(),
                }
                assert_eq!(
                    calls.get(),
                    if scenario == "non_mail"
                        || scenario == "tool_zero"
                        || scenario == "tool_oversized"
                    {
                        0
                    } else {
                        1
                    }
                );
            } else {
                let expected = match scenario {
                    "missing_begin" | "removed_finish" => ErrorCode::ClientNotRegistered,
                    "wrong_method" | "begin_zero" | "begin_oversized" | "finish_oversized" => {
                        ErrorCode::InvalidInput
                    }
                    "lookup_error" => ErrorCode::InternalError,
                    _ => ErrorCode::AuthRequired,
                };
                assert!(
                    matches!(result, Err(code) if code == expected),
                    "scenario {scenario}"
                );
            }
            assert_eq!(
                lookups,
                if matches!(scenario, "begin_zero" | "begin_oversized") {
                    0
                } else if matches!(
                    scenario,
                    "missing_begin"
                        | "epoch_begin"
                        | "wrong_method"
                        | "timeout"
                        | "finish_oversized"
                        | "lookup_error"
                ) {
                    1
                } else {
                    2
                }
            );
        };
        let client = async {
            let mut pipe = connect().await.unwrap();
            if matches!(scenario, "begin_zero" | "begin_oversized") {
                pipe.write_u32_le(if scenario == "begin_zero" { 0 } else { 131073 })
                    .await
                    .unwrap();
                assert!(pipe.read_u8().await.is_err());
                return;
            }
            let begin = json!({"jsonrpc":"2.0", "id":"begin", "method":"auth.begin", "params":{
                "protocol_version":1, "registration_id":wire_hex::encode(&[1;16]), "profile_epoch":if scenario == "epoch_begin" {2} else {1},
                "client_nonce":wire_hex::encode(&[7;32]), "scope":{"accounts":scope.accounts(), "tools":scope.tools()}
            }});
            send(&mut pipe, begin).await;
            if matches!(scenario, "missing_begin" | "epoch_begin" | "lookup_error") {
                assert!(pipe.read_u8().await.is_err());
                return;
            }
            let response = receive(&mut pipe).await;
            assert_eq!(response["id"], "begin");
            let result = &response["result"];
            assert_eq!(result["protocol_version"], 1);
            let transcript =
                wire_hex::decode_bounded(result["transcript_hex"].as_str().unwrap(), 32768)
                    .unwrap();
            let server_signature =
                wire_hex::decode_fixed::<64>(result["server_signature_hex"].as_str().unwrap())
                    .unwrap();
            assert!(
                verify_signature(
                    &server_key.public_key().unwrap(),
                    &transcript,
                    &server_signature
                )
                .unwrap()
            );
            let transcript = AuthTranscript::decode(&transcript, SignerRole::Server).unwrap();
            assert_eq!(transcript.client_nonce, [7; 32]);
            assert_eq!(transcript.server_instance_id, instance);
            assert_eq!(transcript.scope, scope);
            if scenario == "timeout" {
                assert!(pipe.read_u8().await.is_err());
                return;
            }
            if scenario == "finish_oversized" {
                pipe.write_u32_le(131073).await.unwrap();
                assert!(pipe.read_u8().await.is_err());
                return;
            }
            let mut signature = client_key
                .sign(&transcript.encode(SignerRole::Client).unwrap())
                .unwrap();
            if scenario == "bad_signature" {
                signature[0] ^= 1;
            }
            send(&mut pipe, json!({"jsonrpc":"2.0", "id":"finish", "method":if scenario == "wrong_method" {"mail.accounts.list"} else {"auth.finish"}, "params":{"signature_hex":wire_hex::encode(&signature)}})).await;
            if matches!(
                scenario,
                "valid"
                    | "second_request"
                    | "wrong_receipt"
                    | "missing_receipt"
                    | "non_mail"
                    | "tool_zero"
                    | "tool_oversized"
            ) {
                assert_eq!(
                    receive(&mut pipe).await,
                    json!({"jsonrpc":"2.0", "id":"finish", "result":{"protocol_version":1,"authenticated":true}})
                );
                if scenario == "non_mail" {
                    send(
                        &mut pipe,
                        json!({"jsonrpc":"2.0","id":"tool","method":"system.version","params":{}}),
                    )
                    .await;
                    assert!(pipe.read_u8().await.is_err());
                    return;
                }
                if scenario == "tool_zero" {
                    pipe.write_u32_le(0).await.unwrap();
                    assert!(pipe.read_u8().await.is_err());
                    return;
                }
                if scenario == "tool_oversized" {
                    pipe.write_u32_le(1_048_577).await.unwrap();
                    assert!(pipe.read_u8().await.is_err());
                    return;
                }
                send(&mut pipe, json!({"jsonrpc":"2.0","id":"tool","method":"mail.accounts.list","params":{"fixture":true}})).await;
                assert_eq!(
                    receive(&mut pipe).await,
                    json!({"jsonrpc":"2.0","id":"tool","result":{"fixture_count":1}})
                );
                match scenario {
                    "valid" => pipe.write_u8(6).await.unwrap(),
                    "second_request" => {
                        send(&mut pipe, json!({"jsonrpc":"2.0","id":"second","method":"mail.accounts.list","params":{"fixture":true}})).await;
                        assert!(pipe.read_u8().await.is_err());
                    }
                    "wrong_receipt" => {
                        pipe.write_u8(7).await.unwrap();
                        assert!(pipe.read_u8().await.is_err());
                    }
                    "missing_receipt" => drop(pipe),
                    _ => unreachable!(),
                }
            } else {
                assert!(pipe.read_u8().await.is_err());
            }
        };
        tokio::join!(server, client);
    }
}

async fn send<W: tokio::io::AsyncWrite + Unpin>(pipe: &mut W, value: Value) {
    let bytes = serde_json::to_vec(&value).unwrap();
    pipe.write_u32_le(bytes.len() as u32).await.unwrap();
    pipe.write_all(&bytes).await.unwrap();
    pipe.flush().await.unwrap();
}
async fn receive<R: tokio::io::AsyncRead + Unpin>(pipe: &mut R) -> Value {
    let length = pipe.read_u32_le().await.unwrap() as usize;
    assert!((1..=131072).contains(&length));
    let mut bytes = vec![0; length];
    pipe.read_exact(&mut bytes).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
