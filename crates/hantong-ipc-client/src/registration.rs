//! Registration requests and untrusted progress; no approval or pin acquisition.
use hantong_contracts::{
    ErrorCode,
    registration::{RegistrationCommand, RegistrationProgress, RegistrationReceipt},
    wire_hex,
};
use serde::Deserialize;
#[cfg(test)]
use serde_json::Value;
use serde_json::json;

const MAX_FRAME: usize = 16_384;

pub fn request(id: &str, command: RegistrationCommand) -> Result<Vec<u8>, ErrorCode> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
    {
        return Err(ErrorCode::InvalidInput);
    }
    let (method, params) = match command {
        RegistrationCommand::Request => ("registration.request", json!({"protocol_version":1})),
        RegistrationCommand::McpRequest => {
            ("registration.mcp.request", json!({"protocol_version":1}))
        }
        RegistrationCommand::GuiRequest => {
            ("registration.gui.request", json!({"protocol_version":1}))
        }
        RegistrationCommand::Status(request_id)
        | RegistrationCommand::GuiStatus(request_id)
        | RegistrationCommand::McpStatus(request_id) => {
            if request_id == [0; 32] {
                return Err(ErrorCode::InvalidInput);
            }
            (
                if matches!(command, RegistrationCommand::McpStatus(_)) {
                    "registration.mcp.status"
                } else if matches!(command, RegistrationCommand::GuiStatus(_)) {
                    "registration.gui.status"
                } else {
                    "registration.status"
                },
                json!({"protocol_version":1,"request_id":wire_hex::encode(&request_id)}),
            )
        }
    };
    serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
        .map_err(|_| ErrorCode::InternalError)
}

pub fn parse_response(
    bytes: &[u8],
    id: &str,
    command: RegistrationCommand,
) -> Result<RegistrationReceipt, ErrorCode> {
    if bytes.is_empty()
        || bytes.len() > MAX_FRAME
        || bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{')
    {
        return Err(ErrorCode::CoreUnavailable);
    }
    // Deserialize directly into strict maps, preserving duplicate-key detection.
    let response: Envelope =
        serde_json::from_slice(bytes).map_err(|_| ErrorCode::CoreUnavailable)?;
    if response.jsonrpc != "2.0" || response.id != id {
        return Err(ErrorCode::CoreUnavailable);
    }
    let receipt = match response.body {
        Body::Result(value) => value,
        Body::Error(code) => return Err(code),
    };
    if let RegistrationCommand::Status(expected)
    | RegistrationCommand::GuiStatus(expected)
    | RegistrationCommand::McpStatus(expected) = command
        && receipt.request_id() != &expected
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
    Result(RegistrationReceipt),
    Error(ErrorCode),
}
impl<'de> Deserialize<'de> for Envelope {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Envelope;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("registration response object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let (mut jsonrpc, mut id, mut body) = (None, None, None);
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "jsonrpc" if jsonrpc.is_none() => {
                            jsonrpc = Some(map.next_value::<String>()?)
                        }
                        "id" if id.is_none() => id = Some(map.next_value::<String>()?),
                        "result" if body.is_none() => {
                            body = Some(Body::Result(map.next_value::<ReceiptWire>()?.0))
                        }
                        "error" if body.is_none() => {
                            body = Some(Body::Error(map.next_value::<ErrorWire>()?.0))
                        }
                        _ => return Err(A::Error::custom("unknown or duplicate response field")),
                    }
                }
                Ok(Envelope {
                    jsonrpc: jsonrpc.ok_or_else(|| A::Error::custom("missing version"))?,
                    id: id.ok_or_else(|| A::Error::custom("missing id"))?,
                    body: body.ok_or_else(|| A::Error::custom("missing body"))?,
                })
            }
        }
        deserializer.deserialize_map(V)
    }
}
struct ReceiptWire(RegistrationReceipt);
impl<'de> Deserialize<'de> for ReceiptWire {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = ReceiptWire;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("registration progress object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let (mut version, mut id, mut expires, mut state) = (None, None, None, None);
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "protocol_version" if version.is_none() => {
                            version = Some(map.next_value::<u64>()?)
                        }
                        "request_id" if id.is_none() => id = Some(map.next_value::<String>()?),
                        "expires_at_utc_ms" if expires.is_none() => {
                            expires = Some(map.next_value::<i64>()?)
                        }
                        "state" if state.is_none() => state = Some(map.next_value::<String>()?),
                        _ => return Err(A::Error::custom("unknown or duplicate progress field")),
                    }
                }
                if version != Some(1) {
                    return Err(A::Error::custom("invalid version"));
                }
                let id = wire_hex::decode_fixed::<32>(
                    &id.ok_or_else(|| A::Error::custom("missing request"))?,
                )
                .map_err(|_| A::Error::custom("invalid request"))?;
                let state = RegistrationProgress::parse(
                    &state.ok_or_else(|| A::Error::custom("missing state"))?,
                )
                .map_err(|_| A::Error::custom("invalid state"))?;
                RegistrationReceipt::new(
                    id,
                    expires.ok_or_else(|| A::Error::custom("missing expiry"))?,
                    state,
                )
                .map(ReceiptWire)
                .map_err(|_| A::Error::custom("invalid receipt"))
            }
        }
        deserializer.deserialize_map(V)
    }
}
struct ErrorWire(ErrorCode);
impl<'de> Deserialize<'de> for ErrorWire {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = ErrorWire;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("stable error object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let (mut code, mut message) = (None, None);
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "code" if code.is_none() => code = Some(map.next_value::<String>()?),
                        "message" if message.is_none() => {
                            message = Some(map.next_value::<String>()?)
                        }
                        _ => return Err(A::Error::custom("unknown or duplicate error field")),
                    }
                }
                let code = code.ok_or_else(|| A::Error::custom("missing code"))?;
                if message.as_deref() != Some(code.as_str()) {
                    return Err(A::Error::custom("invalid message"));
                }
                ErrorCode::ALL
                    .into_iter()
                    .find(|value| value.as_str() == code)
                    .map(ErrorWire)
                    .ok_or_else(|| A::Error::custom("unknown code"))
            }
        }
        deserializer.deserialize_map(V)
    }
}

/// Starts a bounded request only. The result remains untrusted progress data.
pub fn exchange(command: RegistrationCommand) -> Result<RegistrationReceipt, ErrorCode> {
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
    command: RegistrationCommand,
) -> Result<RegistrationReceipt, ErrorCode> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let body = request("registration-1", command)?;
    pipe.write_u32_le(body.len() as u32)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    pipe.write_all(&body)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    pipe.flush().await.map_err(|_| ErrorCode::CoreUnavailable)?;
    let length = pipe
        .read_u32_le()
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)? as usize;
    if !(1..=MAX_FRAME).contains(&length) {
        return Err(ErrorCode::CoreUnavailable);
    }
    let mut body = vec![0; length];
    pipe.read_exact(&mut body)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let result = parse_response(&body, "registration-1", command);
    pipe.write_all(&[0x06])
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    pipe.flush().await.map_err(|_| ErrorCode::CoreUnavailable)?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response() -> Value {
        json!({"jsonrpc":"2.0","id":"r","result":{"protocol_version":1,"request_id":wire_hex::encode(&[1;32]),"expires_at_utc_ms":123,"state":"pending"}})
    }
    fn parse(value: Value) -> Result<RegistrationReceipt, ErrorCode> {
        parse_response(
            &serde_json::to_vec(&value).unwrap(),
            "r",
            RegistrationCommand::Status([1; 32]),
        )
    }
    #[test]
    fn sec03_gui_registration_uses_distinct_methods_and_binds_status_id() {
        for (command, method) in [
            (RegistrationCommand::GuiRequest, "registration.gui.request"),
            (
                RegistrationCommand::GuiStatus([1; 32]),
                "registration.gui.status",
            ),
        ] {
            let encoded = request("r", command).unwrap();
            let value: Value = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(value["method"], method);
            assert_eq!(value["params"]["protocol_version"], 1);
            assert_eq!(
                value["params"].as_object().unwrap().len(),
                if matches!(command, RegistrationCommand::GuiRequest) {
                    1
                } else {
                    2
                }
            );
            assert_eq!(
                parse_response(&serde_json::to_vec(&response()).unwrap(), "r", command)
                    .unwrap()
                    .request_id(),
                &[1; 32]
            );
        }
        assert_eq!(
            request("r", RegistrationCommand::GuiStatus([0; 32])),
            Err(ErrorCode::InvalidInput)
        );
        let raw = serde_json::to_vec(&response()).unwrap();
        assert_eq!(
            parse_response(&raw, "r", RegistrationCommand::GuiStatus([2; 32])),
            Err(ErrorCode::CoreUnavailable)
        );
        assert_eq!(
            parse_response(&raw, "wrong", RegistrationCommand::GuiStatus([1; 32])),
            Err(ErrorCode::CoreUnavailable)
        );
        let duplicate = String::from_utf8(raw).unwrap().replace(
            "\"state\":\"pending\"",
            "\"state\":\"pending\",\"state\":\"ready\"",
        );
        assert_eq!(
            parse_response(duplicate.as_bytes(), "r", RegistrationCommand::GuiRequest),
            Err(ErrorCode::CoreUnavailable)
        );
    }
    #[test]
    fn cli15_registration_requests_only_fixed_request_or_opaque_status() {
        let value: Value =
            serde_json::from_slice(&request("r", RegistrationCommand::Request).unwrap()).unwrap();
        assert_eq!(
            value,
            json!({"jsonrpc":"2.0","id":"r","method":"registration.request","params":{"protocol_version":1}})
        );
        let value: Value =
            serde_json::from_slice(&request("r", RegistrationCommand::Status([1; 32])).unwrap())
                .unwrap();
        assert_eq!(value["params"].as_object().unwrap().len(), 2);
        assert_eq!(value["params"]["request_id"], wire_hex::encode(&[1; 32]));
        for id in ["", "bad\n", "bad id"] {
            assert_eq!(
                request(id, RegistrationCommand::Request),
                Err(ErrorCode::InvalidInput)
            );
        }
        assert_eq!(
            request("r", RegistrationCommand::Status([0; 32])),
            Err(ErrorCode::InvalidInput)
        );
    }
    #[test]
    fn cli15_registration_response_is_progress_only_and_bound_to_request() {
        for state in ["pending", "denied", "expired", "provisioning", "ready"] {
            let mut value = response();
            value["result"]["state"] = json!(state);
            let receipt = parse(value).unwrap();
            assert_eq!(receipt.state().as_str(), state);
            assert_eq!(receipt.request_id(), &[1; 32]);
            assert_eq!(receipt.expires_at_utc_ms(), 123);
        }
        assert_eq!(
            parse_response(
                &serde_json::to_vec(&response()).unwrap(),
                "r",
                RegistrationCommand::Status([2; 32])
            ),
            Err(ErrorCode::CoreUnavailable)
        );
    }
    #[test]
    fn sec03_registration_responses_reject_shape_and_authority_extensions() {
        for (field, value) in [
            ("protocol_version", json!(2)),
            ("protocol_version", json!(1.0)),
            ("request_id", json!("00".repeat(32))),
            ("request_id", json!("AA".repeat(32))),
            ("expires_at_utc_ms", json!(0)),
            ("expires_at_utc_ms", json!(1.0)),
            ("state", json!("approved")),
            ("approved", json!(true)),
            ("server_public_key", json!("forged")),
        ] {
            let mut v = response();
            v["result"][field] = value;
            assert_eq!(parse(v), Err(ErrorCode::CoreUnavailable));
        }
        for value in [
            json!(null),
            json!([]),
            json!(["2.0", "r", response()["result"]]),
            json!({"jsonrpc":"2.0","id":"r","result":[1,wire_hex::encode(&[1;32]),123,"pending"]}),
            json!({"jsonrpc":"2.0","id":"r","error":null,"result":response()["result"]}),
            json!({"jsonrpc":"2.0","id":"wrong","result":response()["result"]}),
        ] {
            assert_eq!(parse(value), Err(ErrorCode::CoreUnavailable));
        }
    }
    #[test]
    fn sec03_registration_responses_reject_duplicates_bom_and_unbounded_data() {
        let clean = serde_json::to_string(&response()).unwrap();
        for value in [
            format!("{clean}{{}}"),
            format!("\u{feff}{clean}"),
            clean.replace("\"id\":\"r\"", "\"id\":\"r\",\"id\":\"r\""),
            clean.replace(
                "\"state\":\"pending\"",
                "\"state\":\"pending\",\"state\":\"ready\"",
            ),
            " ".repeat(MAX_FRAME + 1),
        ] {
            assert_eq!(
                parse_response(value.as_bytes(), "r", RegistrationCommand::Request),
                Err(ErrorCode::CoreUnavailable)
            );
        }
    }
    #[test]
    fn cli15_registration_errors_allow_only_stable_matching_code() {
        assert_eq!(
            parse(
                json!({"jsonrpc":"2.0","id":"r","error":{"code":"CONFLICT","message":"CONFLICT"}})
            ),
            Err(ErrorCode::Conflict)
        );
        for value in [
            json!({"code":"UNKNOWN","message":"UNKNOWN"}),
            json!({"code":"CONFLICT","message":"private data"}),
            json!(["CONFLICT", "CONFLICT"]),
        ] {
            assert_eq!(
                parse(json!({"jsonrpc":"2.0","id":"r","error":value})),
                Err(ErrorCode::CoreUnavailable)
            );
        }
    }
}
