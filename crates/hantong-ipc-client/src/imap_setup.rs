//! Bounded IMAP setup bootstrap client. It only transports opaque progress.
use hantong_contracts::{
    ErrorCode,
    imap_setup::{
        ImapSetupCommand, ImapSetupNextAction, ImapSetupReceipt, ImapSetupState, ImapSetupTls,
    },
    wire_hex,
};
use serde::Deserialize;
use serde_json::json;
const MAX_FRAME: usize = 16_384;
pub fn request(id: &str, command: ImapSetupCommand) -> Result<Vec<u8>, ErrorCode> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
    {
        return Err(ErrorCode::InvalidInput);
    }
    let (method, params) = match command {
        ImapSetupCommand::Status(request_id) => {
            if request_id == [0; 32] {
                return Err(ErrorCode::InvalidInput);
            }
            (
                "setup.imap.status",
                json!({"protocol_version":1,"request_id":wire_hex::encode(&request_id)}),
            )
        }
        ImapSetupCommand::Request(input) => (
            "setup.imap.request",
            json!({"protocol_version":1,"display_name":input.display_name(),"color":input.color(),"host":input.host(),"port":input.port(),"tls":match input.tls(){ImapSetupTls::ImplicitTls=>"implicit_tls",ImapSetupTls::StarttlsRequired=>"starttls_required"},"login_name":input.login_name()}),
        ),
    };
    serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
        .map_err(|_| ErrorCode::InternalError)
}
pub fn parse_response(
    bytes: &[u8],
    id: &str,
    command: ImapSetupCommand,
) -> Result<ImapSetupReceipt, ErrorCode> {
    if bytes.is_empty()
        || bytes.len() > MAX_FRAME
        || bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{')
    {
        return Err(ErrorCode::CoreUnavailable);
    }
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|_| ErrorCode::CoreUnavailable)?;
    if envelope.jsonrpc != "2.0" || envelope.id != id {
        return Err(ErrorCode::CoreUnavailable);
    }
    let receipt = match envelope.body {
        Body::Result(x) => {
            if x.protocol_version != 1 {
                return Err(ErrorCode::CoreUnavailable);
            }
            let request_id = wire_hex::decode_fixed::<32>(&x.request_id)
                .map_err(|_| ErrorCode::CoreUnavailable)?;
            let state = ImapSetupState::parse(&x.state).map_err(|_| ErrorCode::CoreUnavailable)?;
            let action = ImapSetupNextAction::parse(&x.next_action)
                .map_err(|_| ErrorCode::CoreUnavailable)?;
            ImapSetupReceipt::new(request_id, x.expires_at_utc_ms, state, action)
                .map_err(|_| ErrorCode::CoreUnavailable)?
        }
        Body::Error(c) => return Err(c.code),
    };
    if let ImapSetupCommand::Status(request_id) = command
        && receipt.request_id() != &request_id
    {
        return Err(ErrorCode::CoreUnavailable);
    }
    Ok(receipt)
}
struct Envelope {
    jsonrpc: String,
    id: String,
    body: Body,
}
enum Body {
    Result(Receipt),
    Error(WireError),
}
impl<'de> Deserialize<'de> for Envelope {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Envelope;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("setup response")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Self::Value, A::Error> {
                let (mut jsonrpc, mut response_id, mut body) = (None, None, None);
                while let Some(field_name) = m.next_key::<String>()? {
                    match field_name.as_str() {
                        "jsonrpc" if jsonrpc.is_none() => jsonrpc = Some(m.next_value()?),
                        "id" if response_id.is_none() => response_id = Some(m.next_value()?),
                        "result" if body.is_none() => body = Some(Body::Result(m.next_value()?)),
                        "error" if body.is_none() => body = Some(Body::Error(m.next_value()?)),
                        _ => return Err(A::Error::custom("unknown or duplicate field")),
                    }
                }
                Ok(Envelope {
                    jsonrpc: jsonrpc.ok_or_else(|| A::Error::custom("missing jsonrpc"))?,
                    id: response_id.ok_or_else(|| A::Error::custom("missing id"))?,
                    body: body.ok_or_else(|| A::Error::custom("missing body"))?,
                })
            }
        }
        d.deserialize_map(V)
    }
}
struct Receipt {
    protocol_version: u64,
    request_id: String,
    expires_at_utc_ms: i64,
    state: String,
    next_action: String,
}
impl<'de> Deserialize<'de> for Receipt {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, Visitor};
        struct ReceiptVisitor;
        impl<'de> Visitor<'de> for ReceiptVisitor {
            type Value = Receipt;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("IMAP setup result object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut fields: A) -> Result<Self::Value, A::Error> {
                let (
                    mut protocol_version,
                    mut request_id,
                    mut expires_at_utc_ms,
                    mut state,
                    mut next_action,
                ) = (None, None, None, None, None);
                while let Some(field_name) = fields.next_key::<String>()? {
                    match field_name.as_str() {
                        "protocol_version" if protocol_version.is_none() => {
                            protocol_version = Some(fields.next_value()?)
                        }
                        "request_id" if request_id.is_none() => {
                            request_id = Some(fields.next_value()?)
                        }
                        "expires_at_utc_ms" if expires_at_utc_ms.is_none() => {
                            expires_at_utc_ms = Some(fields.next_value()?)
                        }
                        "state" if state.is_none() => state = Some(fields.next_value()?),
                        "next_action" if next_action.is_none() => {
                            next_action = Some(fields.next_value()?)
                        }
                        _ => return Err(A::Error::custom("unknown or duplicate result field")),
                    }
                }
                Ok(Receipt {
                    protocol_version: protocol_version
                        .ok_or_else(|| A::Error::custom("missing protocol_version"))?,
                    request_id: request_id.ok_or_else(|| A::Error::custom("missing request_id"))?,
                    expires_at_utc_ms: expires_at_utc_ms
                        .ok_or_else(|| A::Error::custom("missing expires_at_utc_ms"))?,
                    state: state.ok_or_else(|| A::Error::custom("missing state"))?,
                    next_action: next_action
                        .ok_or_else(|| A::Error::custom("missing next_action"))?,
                })
            }
        }
        deserializer.deserialize_map(ReceiptVisitor)
    }
}
struct WireError {
    code: ErrorCode,
}
impl<'de> Deserialize<'de> for WireError {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = WireError;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("error")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Self::Value, A::Error> {
                let (mut code, mut message) = (None, None);
                while let Some(k) = m.next_key::<String>()? {
                    match k.as_str() {
                        "code" if code.is_none() => code = Some(m.next_value::<String>()?),
                        "message" if message.is_none() => message = Some(m.next_value::<String>()?),
                        _ => return Err(A::Error::custom("unknown or duplicate field")),
                    }
                }
                let raw = code.ok_or_else(|| A::Error::custom("missing code"))?;
                let message = message.ok_or_else(|| A::Error::custom("missing message"))?;
                let code = ErrorCode::ALL
                    .into_iter()
                    .find(|x| x.as_str() == raw)
                    .ok_or_else(|| A::Error::custom("unknown error"))?;
                if message != code.as_str() {
                    return Err(A::Error::custom("error message mismatch"));
                }
                Ok(WireError { code })
            }
        }
        d.deserialize_map(V)
    }
}
pub fn exchange(command: ImapSetupCommand) -> Result<ImapSetupReceipt, ErrorCode> {
    #[cfg(windows)]
    {
        tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| ErrorCode::CoreUnavailable)?
            .block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    let pipe = hantong_platform::connect().await?;
                    exchange_connected(pipe, command).await
                })
                .await
                .map_err(|_| ErrorCode::CoreUnavailable)?
            })
    }
    #[cfg(not(windows))]
    {
        let _ = command;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
#[cfg(windows)]
async fn exchange_connected<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    mut pipe: S,
    command: ImapSetupCommand,
) -> Result<ImapSetupReceipt, ErrorCode> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let request_bytes = request("setup-imap-1", command.clone())?;
    pipe.write_u32_le(request_bytes.len() as u32)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    pipe.write_all(&request_bytes)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    pipe.flush().await.map_err(|_| ErrorCode::CoreUnavailable)?;
    let response_length = pipe
        .read_u32_le()
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)? as usize;
    if !(1..=MAX_FRAME).contains(&response_length) {
        return Err(ErrorCode::CoreUnavailable);
    }
    let mut response_bytes = vec![0; response_length];
    pipe.read_exact(&mut response_bytes)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let result = parse_response(&response_bytes, "setup-imap-1", command);
    pipe.write_all(&[6])
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    pipe.flush().await.map_err(|_| ErrorCode::CoreUnavailable)?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::imap_setup::ImapSetupInput;
    use serde_json::json;

    fn response(body: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":"x","result":body})).unwrap()
    }
    fn result() -> serde_json::Value {
        json!({"protocol_version":1,"request_id":wire_hex::encode(&[1;32]),"expires_at_utc_ms":1,"state":"pending","next_action":"helper_required"})
    }
    fn status() -> ImapSetupCommand {
        ImapSetupCommand::Status([1; 32])
    }

    #[test]
    fn response_decoder_rejects_shape_types_and_trailing_data() {
        for bytes in [
            b"".to_vec(),
            b"[]".to_vec(),
            response(json!([
                1,
                wire_hex::encode(&[1; 32]),
                1,
                "pending",
                "helper_required"
            ])),
            response(
                json!({"protocol_version":1,"request_id":wire_hex::encode(&[1;32]),"expires_at_utc_ms":1.0,"state":"pending","next_action":"helper_required"}),
            ),
            [response(result()), b"x".to_vec()].concat(),
            vec![0xff, 0xfe],
        ] {
            assert!(parse_response(&bytes, "x", status()).is_err());
        }
    }

    #[test]
    fn response_decoder_accepts_only_complete_valid_result() {
        let parsed = parse_response(&response(result()), "x", status()).unwrap();
        assert_eq!(parsed.request_id(), &[1; 32]);
        for field in [
            "protocol_version",
            "request_id",
            "expires_at_utc_ms",
            "state",
            "next_action",
        ] {
            let mut object = result().as_object().unwrap().clone();
            object.remove(field);
            assert!(parse_response(&response(json!(object)), "x", status()).is_err());
        }
        let valid_envelope = json!({"jsonrpc":"2.0","id":"x","result":result()});
        assert!(
            parse_response(&serde_json::to_vec(&valid_envelope).unwrap(), "x", status()).is_ok()
        );
        for envelope in [
            json!({"jsonrpc":"2.0","id":"other","result":result()}),
            json!({"jsonrpc":"1.0","id":"x","result":result()}),
        ] {
            assert!(
                parse_response(&serde_json::to_vec(&envelope).unwrap(), "x", status()).is_err()
            );
        }
    }

    #[test]
    fn response_decoder_rejects_duplicate_unknown_missing_arrays_and_wrong_state() {
        let base = r#"{"jsonrpc":"2.0","id":"x","result":{"protocol_version":1,"request_id":"0101010101010101010101010101010101010101010101010101010101010101","expires_at_utc_ms":1,"state":"pending","next_action":"helper_required"}}"#;
        for raw in [
            base.replace("\"result\":{", "\"result\":{\"extra\":1,"),
            base.replace("\"state\":\"pending\"", "\"state\":\"ready\""),
            base.replace(
                "\"next_action\":\"helper_required\"",
                "\"next_action\":\"wait\"",
            ),
            base.replace(
                "\"protocol_version\":1",
                "\"protocol_version\":1,\"protocol_version\":1",
            ),
            base.replacen("\"id\":\"x\"", "\"id\":\"x\",\"id\":\"x\"", 1),
        ] {
            assert!(
                parse_response(raw.as_bytes(), "x", status()).is_err(),
                "{raw}"
            );
        }
    }

    #[test]
    fn response_decoder_rejects_status_echo_and_error_shape_violations() {
        let wrong_id = json!({"protocol_version":1,"request_id":wire_hex::encode(&[2;32]),"expires_at_utc_ms":1,"state":"pending","next_action":"helper_required"});
        assert!(parse_response(&response(wrong_id), "x", status()).is_err());
        assert_eq!(parse_response(br#"{"jsonrpc":"2.0","id":"x","error":{"code":"INVALID_INPUT","message":"INVALID_INPUT"}}"#, "x", status()), Err(ErrorCode::InvalidInput));
        for raw in [
            r#"{"jsonrpc":"2.0","id":"x","error":{"code":"INVALID_INPUT","message":"bad"}}"#,
            r#"{"jsonrpc":"2.0","id":"x","error":{"code":"INVALID_INPUT"}}"#,
            r#"{"jsonrpc":"2.0","id":"x","result":{},"error":{"code":"INVALID_INPUT","message":"INVALID_INPUT"}}"#,
        ] {
            assert_eq!(
                parse_response(raw.as_bytes(), "x", status()),
                Err(ErrorCode::CoreUnavailable)
            );
        }
    }

    #[test]
    fn status_request_rejects_zero_id_and_preserves_input_whitespace() {
        assert_eq!(
            request("x", ImapSetupCommand::Status([0; 32])),
            Err(ErrorCode::InvalidInput)
        );
        let input = ImapSetupInput::new(
            None,
            None,
            "imap.example".into(),
            993,
            ImapSetupTls::ImplicitTls,
            " user ".into(),
        )
        .unwrap();
        let encoded = request("x", ImapSetupCommand::Request(input)).unwrap();
        assert!(String::from_utf8(encoded).unwrap().contains(" user "));
    }
}
