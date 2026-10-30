#![forbid(unsafe_code)]

//! Bounded IPC framing and minimal unauthenticated readiness responses.
//! Mail actions remain unavailable until registration/authentication exists.

#[cfg(windows)]
pub mod auth;
pub mod auth_wire;
#[cfg(windows)]
pub mod authenticated_wire;
pub mod bootstrap;
pub mod imap_setup_wire;
pub mod registration_wire;
pub mod setup_dispatch;

use hantong_contracts::ErrorCode;
use serde::Deserializer;
use serde::de::{DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::{collections::BTreeMap, fmt};

pub const MAX_FRAME_SIZE: usize = 1_048_576;
const MAX_DEPTH: usize = 64;
const MAX_VALUE_NODES: usize = 16_384;
const MAX_METHOD_LENGTH: usize = 128;

#[derive(PartialEq, Eq)]
pub struct RpcRequest {
    pub id: String,
    pub method: String,
    pub params: Value,
}

impl fmt::Debug for RpcRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RpcRequest { id: <redacted>, method: <redacted>, params: <redacted> }")
    }
}

#[derive(Debug)]
enum StrictValue {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<StrictValue>),
    Object(BTreeMap<String, StrictValue>),
}

struct Budget {
    nodes: usize,
}

struct StrictSeed<'a> {
    budget: &'a mut Budget,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for StrictSeed<'_> {
    type Value = StrictValue;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        if self.depth > MAX_DEPTH {
            return Err(D::Error::custom("nesting limit"));
        }
        if self.budget.nodes >= MAX_VALUE_NODES {
            return Err(D::Error::custom("value limit"));
        }
        self.budget.nodes += 1;
        deserializer.deserialize_any(StrictVisitor {
            budget: self.budget,
            depth: self.depth,
        })
    }
}

struct StrictVisitor<'a> {
    budget: &'a mut Budget,
    depth: usize,
}

impl<'de> Visitor<'de> for StrictVisitor<'_> {
    type Value = StrictValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictValue::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictValue::Number(Number::from(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictValue::Number(Number::from(value)))
    }

    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
        Number::from_f64(value)
            .map(StrictValue::Number)
            .ok_or_else(|| E::custom("invalid number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(StrictValue::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictValue::String(value))
    }

    fn visit_seq<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = access.next_element_seed(StrictSeed {
            budget: self.budget,
            depth: self.depth + 1,
        })? {
            values.push(value);
        }
        Ok(StrictValue::Array(values))
    }

    fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = BTreeMap::new();
        while let Some(key) = access.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(A::Error::custom("duplicate object key"));
            }
            let value = access.next_value_seed(StrictSeed {
                budget: self.budget,
                depth: self.depth + 1,
            })?;
            values.insert(key, value);
        }
        Ok(StrictValue::Object(values))
    }
}

impl StrictValue {
    fn into_json(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(value),
            Self::Number(value) => Value::Number(value),
            Self::String(value) => Value::String(value),
            Self::Array(values) => Value::Array(values.into_iter().map(Self::into_json).collect()),
            Self::Object(values) => Value::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, value.into_json()))
                    .collect::<Map<_, _>>(),
            ),
        }
    }
}

/// Decode one complete little-endian length-prefixed JSON-RPC request frame.
pub fn decode_request(frame: &[u8]) -> Result<RpcRequest, ErrorCode> {
    if frame.len() < 4 {
        return Err(ErrorCode::InvalidInput);
    }
    let declared = u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
    if !(1..=MAX_FRAME_SIZE).contains(&declared) || frame.len() != declared + 4 {
        return Err(ErrorCode::InvalidInput);
    }
    let payload = &frame[4..];
    if payload.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(ErrorCode::InvalidInput);
    }

    let mut budget = Budget { nodes: 0 };
    let mut deserializer = serde_json::Deserializer::from_slice(payload);
    let root = StrictSeed {
        budget: &mut budget,
        depth: 0,
    }
    .deserialize(&mut deserializer)
    .map_err(|_| ErrorCode::InvalidInput)?;
    deserializer.end().map_err(|_| ErrorCode::InvalidInput)?;

    let StrictValue::Object(mut envelope) = root else {
        return Err(ErrorCode::InvalidInput);
    };
    if envelope
        .keys()
        .any(|key| !matches!(key.as_str(), "jsonrpc" | "id" | "method" | "params"))
    {
        return Err(ErrorCode::InvalidInput);
    }
    let jsonrpc = envelope.remove("jsonrpc").ok_or(ErrorCode::InvalidInput)?;
    if !matches!(jsonrpc, StrictValue::String(ref value) if value == "2.0") {
        return Err(ErrorCode::InvalidInput);
    }
    let id = match envelope.remove("id") {
        Some(StrictValue::String(value)) if !value.is_empty() && value.chars().count() <= 64 => {
            value
        }
        _ => return Err(ErrorCode::InvalidInput),
    };
    let method = match envelope.remove("method") {
        Some(StrictValue::String(value))
            if !value.is_empty() && value.chars().count() <= MAX_METHOD_LENGTH =>
        {
            value
        }
        _ => return Err(ErrorCode::InvalidInput),
    };
    let params = match envelope.remove("params") {
        Some(StrictValue::Object(params)) => Value::Object(
            params
                .into_iter()
                .map(|(key, value)| (key, value.into_json()))
                .collect(),
        ),
        _ => return Err(ErrorCode::InvalidInput),
    };
    if !envelope.is_empty() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(RpcRequest { id, method, params })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(json: &str) -> Vec<u8> {
        let bytes = json.as_bytes();
        let mut frame = (bytes.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(bytes);
        frame
    }

    fn valid() -> Vec<u8> {
        frame(
            r#"{"jsonrpc":"2.0","id":"req-1","method":"mail.messages.search","params":{"q":"한글"}}"#,
        )
    }

    #[test]
    fn sec03_cli06_accepts_valid_unicode_request() {
        let request = decode_request(&valid()).unwrap();
        assert_eq!(request.id, "req-1");
        assert_eq!(request.params["q"], "한글");
    }

    #[test]
    fn sec03_rejects_duplicate_envelope_and_nested_keys() {
        assert!(
            decode_request(&frame(
                r#"{"jsonrpc":"2.0","jsonrpc":"2.0","id":"x","method":"m","params":{}}"#
            ))
            .is_err()
        );
        assert!(
            decode_request(&frame(
                r#"{"jsonrpc":"2.0","id":"x","method":"m","params":{"x":1,"x":2}}"#
            ))
            .is_err()
        );
    }

    #[test]
    fn cli06_rejects_zero_oversized_and_truncated_prefix() {
        assert!(decode_request(&[0, 0, 0, 0]).is_err());
        let mut oversized = 1_048_577u32.to_le_bytes().to_vec();
        oversized.extend([b'x'; 4]);
        assert!(decode_request(&oversized).is_err());
        assert!(decode_request(&[1, 2, 3]).is_err());
    }

    #[test]
    fn sec03_rejects_extra_bytes_and_trailing_json() {
        let mut extra = valid();
        extra.push(0);
        assert!(decode_request(&extra).is_err());
        assert!(
            decode_request(&frame(
                r#"{"jsonrpc":"2.0","id":"x","method":"m","params":{}} {}"#
            ))
            .is_err()
        );
    }

    #[test]
    fn sec03_rejects_invalid_utf8_and_bom() {
        let mut invalid = (2u32).to_le_bytes().to_vec();
        invalid.extend([0xff, 0xff]);
        assert!(decode_request(&invalid).is_err());
        let mut bom = (valid().len() as u32 - 4 + 3).to_le_bytes().to_vec();
        bom.extend([0xef, 0xbb, 0xbf]);
        bom.extend_from_slice(&valid()[4..]);
        assert!(decode_request(&bom).is_err());
    }

    #[test]
    fn sec03_rejects_deep_nesting_and_broad_arrays() {
        let deep = format!(
            r#"{{"jsonrpc":"2.0","id":"x","method":"m","params":{{"x":{}}}}}"#,
            "{".repeat(65)
        );
        assert!(decode_request(&frame(&deep)).is_err());
        let broad = format!(
            r#"{{"jsonrpc":"2.0","id":"x","method":"m","params":{{"x":[{}]}}}}"#,
            (0..16_385).map(|_| "0").collect::<Vec<_>>().join(",")
        );
        assert!(decode_request(&frame(&broad)).is_err());
    }

    #[test]
    fn cli06_rejects_non_object_params_and_forged_envelope_fields() {
        assert!(
            decode_request(&frame(
                r#"{"jsonrpc":"2.0","id":"x","method":"m","params":[]}"#
            ))
            .is_err()
        );
        assert!(
            decode_request(&frame(
                r#"{"jsonrpc":"2.0","id":"x","method":"m","params":{},"client_id":"admin"}"#
            ))
            .is_err()
        );
        assert!(
            decode_request(&frame(
                r#"{"jsonrpc":"2.0","id":"x","method":"m","params":{"approved":true}}"#
            ))
            .is_ok()
        );
    }

    #[test]
    fn sec03_rejects_notifications_and_non_string_ids() {
        assert!(decode_request(&frame(r#"{"jsonrpc":"2.0","method":"m","params":{}}"#)).is_err());
        assert!(
            decode_request(&frame(
                r#"{"jsonrpc":"2.0","id":1,"method":"m","params":{}}"#
            ))
            .is_err()
        );
        assert!(
            decode_request(&frame(
                r#"{"jsonrpc":"2.0","id":"","method":"m","params":{}}"#
            ))
            .is_err()
        );
    }
}

pub mod scope_refresh_wire;

mod google_setup_wire;
mod smtp_setup_wire;
