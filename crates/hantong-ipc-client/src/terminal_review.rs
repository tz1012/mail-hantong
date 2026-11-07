//! Core foreground helper transport. Never carries credential plaintext.
use hantong_contracts::{ErrorCode, terminal_review::*, wire_hex};
use serde::Deserialize;
use serde_json::json;

const MAX_FRAME: usize = 1_048_576;

pub fn request(command: &TerminalReviewCommand) -> Result<Vec<u8>, ErrorCode> {
    let selector = |v: &TerminalReviewGet| {
        json!({
            "protocol_version":1,"kind":v.kind.as_str(),
            "request_id":wire_hex::encode(&v.request_id),
            "payload_hash":wire_hex::encode(&v.payload_hash),"account_id":v.account_id
        })
    };
    let (method, params) = match command {
        TerminalReviewCommand::Get(v) => ("terminal.review.get", selector(v)),
        TerminalReviewCommand::Confirm(v) => (
            "terminal.review.confirm",
            json!({
                "protocol_version":1,"ticket_id":wire_hex::encode(&v.ticket),
                "snapshot_hash":wire_hex::encode(&v.snapshot_hash)
            }),
        ),
        TerminalReviewCommand::Cancel(v) => {
            let mut params = selector(&v.get);
            params["ticket_id"] = wire_hex::encode(&v.ticket).into();
            ("terminal.review.cancel", params)
        }
        TerminalReviewCommand::SecretSubmit(v) => (
            "terminal.review.secret_submit",
            json!({
                "protocol_version": 1,
                "ticket_id": wire_hex::encode(&v.ticket),
                "snapshot_hash": wire_hex::encode(&v.snapshot_hash),
                "secret_ticket_opaque": wire_hex::encode(&v.secret_ticket_opaque),
                "staged_handle": wire_hex::encode(&v.staged_handle),
            }),
        ),
    };
    serde_json::to_vec(&json!({"jsonrpc":"2.0","id":"terminal-1","method":method,"params":params}))
        .map_err(|_| ErrorCode::InternalError)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    jsonrpc: String,
    id: String,
    result: Option<Receipt>,
    error: Option<StableError>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StableError {
    code: String,
    message: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Field {
    label: String,
    value: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    kind: String,
    request_id: String,
    ticket_id: Option<String>,
    snapshot_hash: String,
    owner_registration_id: String,
    profile_epoch: u64,
    expires_at_utc_ms: i64,
    state: String,
    requires_secret: bool,
    secret_ticket_opaque: Option<String>,
    fields: Vec<Field>,
}

pub fn parse_response(
    bytes: &[u8],
    command: &TerminalReviewCommand,
) -> Result<TerminalReviewReceipt, ErrorCode> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err(ErrorCode::CoreUnavailable);
    }
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|_| ErrorCode::CoreUnavailable)?;
    if envelope.jsonrpc != "2.0" || envelope.id != "terminal-1" {
        return Err(ErrorCode::CoreUnavailable);
    }
    let value = match (envelope.result, envelope.error) {
        (Some(v), None) => v,
        (None, Some(e)) if e.code == e.message => {
            return Err(ErrorCode::ALL
                .into_iter()
                .find(|c| c.as_str() == e.code)
                .ok_or(ErrorCode::CoreUnavailable)?);
        }
        _ => return Err(ErrorCode::CoreUnavailable),
    };
    if value.requires_secret != value.secret_ticket_opaque.is_some() {
        return Err(ErrorCode::CoreUnavailable);
    }
    // Secret capture is handled by a separate Secrets-owned typed ticket;
    // this flag never authorizes reading arbitrary input.
    let receipt = TerminalReviewReceipt {
        secret_ticket_opaque: value
            .secret_ticket_opaque
            .map(|v| wire_hex::decode_fixed::<320>(&v).map(Vec::from))
            .transpose()?,
        kind: TerminalReviewKind::parse(&value.kind)?,
        request_id: wire_hex::decode_fixed(&value.request_id)?,
        ticket: value
            .ticket_id
            .map(|v| wire_hex::decode_fixed(&v))
            .transpose()?,
        snapshot_hash: wire_hex::decode_fixed(&value.snapshot_hash)?,
        owner_registration_id: wire_hex::decode_fixed(&value.owner_registration_id)?,
        profile_epoch: value.profile_epoch,
        expires_at_utc_ms: value.expires_at_utc_ms,
        state: value.state,
        fields: value
            .fields
            .into_iter()
            .map(|v| TerminalReviewField::new(v.label, v.value))
            .collect::<Result<_, _>>()?,
    };
    receipt.validate()?;
    let expected = match command {
        TerminalReviewCommand::Get(v) => Some(v),
        TerminalReviewCommand::Cancel(v) => Some(&v.get),
        TerminalReviewCommand::Confirm(v) => {
            if receipt.snapshot_hash != v.snapshot_hash {
                return Err(ErrorCode::CoreUnavailable);
            }
            None
        }
        TerminalReviewCommand::SecretSubmit(v) => {
            if receipt.snapshot_hash != v.snapshot_hash {
                return Err(ErrorCode::CoreUnavailable);
            }
            None
        }
    };
    if expected.is_some_and(|v| {
        receipt.kind != v.kind
            || receipt.request_id != v.request_id
            || (v.payload_hash != [0; 32] && receipt.snapshot_hash != v.payload_hash)
    }) {
        return Err(ErrorCode::CoreUnavailable);
    }
    Ok(receipt)
}

/// Restricted to the verified sibling Core foreground process by both ends.
pub fn exchange(command: &TerminalReviewCommand) -> Result<TerminalReviewReceipt, ErrorCode> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let body = request(command)?;
        let envelope: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| ErrorCode::InvalidInput)?;
        let method = envelope
            .get("method")
            .and_then(serde_json::Value::as_str)
            .ok_or(ErrorCode::InvalidInput)?;
        let params = envelope.get("params").ok_or(ErrorCode::InvalidInput)?;
        #[cfg(target_os = "linux")]
        let caller = hantong_platform::linux_identity::current_asset()?
            .strip_prefix("bin/")
            .ok_or(ErrorCode::ScopeDenied)?
            .to_owned();
        #[cfg(target_os = "macos")]
        let caller = std::env::current_exe()
            .map_err(|_| ErrorCode::CoreUnavailable)?
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(ErrorCode::CoreUnavailable)?
            .to_ascii_lowercase();
        let response = if caller == "mail-hantong-core" {
            crate::unix_relay::call(
                crate::profile::ProfileSelector::Default,
                "terminal-1",
                method,
                params,
            )?
        } else {
            let profile = hantong_platform::load_default_client_profile()?;
            let parsed = crate::profile::ParsedClientProfile::from_json(profile.bytes())?;
            crate::unix_relay::call(parsed.profile(), "terminal-1", method, params)?
        };
        parse_response(&response, command)
    }
    #[cfg(all(not(windows), not(any(target_os = "linux", target_os = "macos"))))]
    {
        let _ = command;
        Err(ErrorCode::CapabilityUnsupported)
    }
    #[cfg(windows)]
    {
        tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| ErrorCode::CoreUnavailable)?
            .block_on(async {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    let mut pipe = hantong_platform::connect().await?;
                    hantong_platform::verify_core_helper_server(&pipe)?;
                    let body = request(command)?;
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
                        .map_err(|_| ErrorCode::CoreUnavailable)?
                        as usize;
                    if !(1..=MAX_FRAME).contains(&length) {
                        return Err(ErrorCode::CoreUnavailable);
                    }
                    let mut body = vec![0; length];
                    pipe.read_exact(&mut body)
                        .await
                        .map_err(|_| ErrorCode::CoreUnavailable)?;
                    let result = parse_response(&body, command);
                    pipe.write_all(&[0x06])
                        .await
                        .map_err(|_| ErrorCode::CoreUnavailable)?;
                    pipe.flush().await.map_err(|_| ErrorCode::CoreUnavailable)?;
                    result
                })
                .await
                .map_err(|_| ErrorCode::CoreUnavailable)?
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn get() -> TerminalReviewCommand {
        TerminalReviewCommand::Get(
            TerminalReviewGet::new(TerminalReviewKind::Registration, [1; 32], None, [2; 32])
                .unwrap(),
        )
    }
    #[test]
    fn terminal_review_rejects_duplicate_fields_wrong_binding_and_authority() {
        let receipt = TerminalReviewReceipt {
            secret_ticket_opaque: None,
            kind: TerminalReviewKind::Registration,
            request_id: [1; 32],
            ticket: Some([3; 32]),
            snapshot_hash: [2; 32],
            owner_registration_id: [4; 16],
            profile_epoch: 1,
            expires_at_utc_ms: 100,
            state: "pending".into(),
            fields: vec![],
        };
        let mut response = json!({"jsonrpc":"2.0","id":"terminal-1","result":receipt.to_json()});
        let bytes = serde_json::to_vec(&response).unwrap();
        assert!(parse_response(&bytes, &get()).is_ok());
        response["result"]["approved"] = true.into();
        assert!(parse_response(&serde_json::to_vec(&response).unwrap(), &get()).is_err());
        let duplicate = String::from_utf8(bytes).unwrap().replacen(
            "\"jsonrpc\":\"2.0\"",
            "\"jsonrpc\":\"2.0\",\"jsonrpc\":\"2.0\"",
            1,
        );
        assert!(parse_response(duplicate.as_bytes(), &get()).is_err());
        let mut wrong = receipt;
        wrong.request_id = [9; 32];
        let bytes = serde_json::to_vec(
            &json!({"jsonrpc":"2.0","id":"terminal-1","result":wrong.to_json()}),
        )
        .unwrap();
        assert!(parse_response(&bytes, &get()).is_err());
    }
}
