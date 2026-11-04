//! Strict Google setup bootstrap. Public client configuration only.
use hantong_contracts::{
    ErrorCode,
    google_setup::{GoogleLocalProfile, GoogleSetupCommand, GoogleSetupReceipt},
    wire_hex,
};
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    protocol_version: u64,
    request_id: String,
    expires_at_utc_ms: i64,
    state: String,
    account_id: Option<String>,
    login_url: Option<String>,
    error_code: serde_json::Value,
    client_ticket: Option<String>,
    #[serde(default)]
    local_profile: Option<String>,
}
pub fn request(id: &str, c: &GoogleSetupCommand) -> Result<Vec<u8>, ErrorCode> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
    {
        return Err(ErrorCode::InvalidInput);
    }
    let (method, params) = match c {
        GoogleSetupCommand::Request(i) => {
            i.validate()?;
            let mut params = serde_json::json!({"protocol_version":1,"client_id":i.client_id,"display_name":i.display_name,"reauthorize_account":i.reauthorize_account,"force_consent":i.force_consent,"access_mode":i.access_mode.as_str()});
            if i.local_profile != hantong_contracts::google_setup::GoogleLocalProfile::None {
                params["local_profile"] = i.local_profile.as_str().into();
            }
            ("setup.google.request", params)
        }
        GoogleSetupCommand::Status(r) => {
            if *r == [0; 32] {
                return Err(ErrorCode::InvalidInput);
            }
            (
                "setup.google.status",
                serde_json::json!({"protocol_version":1,"request_id":wire_hex::encode(r)}),
            )
        }
        GoogleSetupCommand::ClientTicket(r) => {
            if *r == [0; 32] {
                return Err(ErrorCode::InvalidInput);
            }
            (
                "setup.google.client_ticket",
                serde_json::json!({"protocol_version":1,"request_id":wire_hex::encode(r)}),
            )
        }
        GoogleSetupCommand::StageClient {
            request_id,
            ticket,
            staged_handle,
        } => {
            if *request_id == [0; 32] || ticket.len() != 320 || *staged_handle == [0; 32] {
                return Err(ErrorCode::InvalidInput);
            }
            (
                "setup.google.stage_client",
                serde_json::json!({"protocol_version":1,"request_id":wire_hex::encode(request_id),"ticket":wire_hex::encode(ticket),"staged_handle":wire_hex::encode(staged_handle)}),
            )
        }
    };
    serde_json::to_vec(
        &serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
    )
    .map_err(|_| ErrorCode::InternalError)
}
pub fn parse_response(
    bytes: &[u8],
    id: &str,
    c: &GoogleSetupCommand,
) -> Result<GoogleSetupReceipt, ErrorCode> {
    let w: Wire = crate::mail_send::decode(bytes, id)?;
    if w.protocol_version != 1 {
        return Err(ErrorCode::VersionMismatch);
    }
    let error_code = if w.error_code.is_null() {
        None
    } else {
        Some(
            ErrorCode::ALL
                .into_iter()
                .find(|e| Some(e.as_str()) == w.error_code.as_str())
                .ok_or(ErrorCode::InvalidInput)?,
        )
    };
    let r = GoogleSetupReceipt {
        request_id: wire_hex::decode_fixed(&w.request_id)?,
        expires_at_utc_ms: w.expires_at_utc_ms,
        state: w.state,
        account_id: w.account_id,
        login_url: w.login_url,
        error_code,
        client_ticket: w
            .client_ticket
            .map(|v| wire_hex::decode_bounded(&v, 320))
            .transpose()?,
        local_profile: w
            .local_profile
            .as_deref()
            .map(GoogleLocalProfile::parse)
            .transpose()?
            .unwrap_or_default(),
    };
    r.validate()?;
    if matches!(c,GoogleSetupCommand::Status(expected) | GoogleSetupCommand::ClientTicket(expected) if *expected!=r.request_id)
        || matches!(c, GoogleSetupCommand::StageClient { request_id, .. } if *request_id != r.request_id)
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(r)
}

pub fn exchange(command: GoogleSetupCommand) -> Result<GoogleSetupReceipt, ErrorCode> {
    #[cfg(windows)]
    {
        tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| ErrorCode::CoreUnavailable)?
            .block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut pipe = hantong_platform::connect().await?;
                    if matches!(
                        command,
                        GoogleSetupCommand::ClientTicket(_)
                            | GoogleSetupCommand::StageClient { .. }
                    ) {
                        hantong_platform::verify_core_helper_server(&pipe)?;
                    }
                    let bytes = request("setup-google-1", &command)?;
                    pipe.write_u32_le(bytes.len() as u32)
                        .await
                        .map_err(|_| ErrorCode::CoreUnavailable)?;
                    pipe.write_all(&bytes)
                        .await
                        .map_err(|_| ErrorCode::CoreUnavailable)?;
                    pipe.flush().await.map_err(|_| ErrorCode::CoreUnavailable)?;
                    let length = pipe
                        .read_u32_le()
                        .await
                        .map_err(|_| ErrorCode::CoreUnavailable)?
                        as usize;
                    if !(1..=16384).contains(&length) {
                        return Err(ErrorCode::CoreUnavailable);
                    }
                    let mut bytes = vec![0; length];
                    pipe.read_exact(&mut bytes)
                        .await
                        .map_err(|_| ErrorCode::CoreUnavailable)?;
                    let result = parse_response(&bytes, "setup-google-1", &command);
                    pipe.write_all(&[6])
                        .await
                        .map_err(|_| ErrorCode::CoreUnavailable)?;
                    pipe.flush().await.map_err(|_| ErrorCode::CoreUnavailable)?;
                    result
                })
                .await
                .map_err(|_| ErrorCode::CoreUnavailable)?
            })
    }
    #[cfg(all(not(windows), not(unix)))]
    {
        let _ = command;
        Err(ErrorCode::CapabilityUnsupported)
    }
    #[cfg(unix)]
    {
        let id = "setup-google-1";
        let body = request(id, &command)?;
        let envelope: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| ErrorCode::InvalidInput)?;
        let method = envelope
            .get("method")
            .and_then(serde_json::Value::as_str)
            .ok_or(ErrorCode::InvalidInput)?;
        let params = envelope.get("params").ok_or(ErrorCode::InvalidInput)?;
        let response =
            crate::unix_relay::call(crate::profile::ProfileSelector::Default, id, method, params)?;
        parse_response(&response, id, &command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acc02_personal_request_carries_only_finite_features_and_legacy_omits_them() {
        use hantong_contracts::{
            google_setup::{GoogleLocalProfile, GoogleSetupInput},
            google_write::GoogleAccessMode,
        };
        let mut input = GoogleSetupInput {
            local_profile: GoogleLocalProfile::None,
            access_mode: GoogleAccessMode::ReadOnly,
            client_id: None,
            display_name: None,
            reauthorize_account: None,
            force_consent: false,
        };
        let bytes = request("r", &GoogleSetupCommand::Request(input.clone())).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value["params"].get("local_profile").is_none());
        for profile in [
            GoogleLocalProfile::PersonalRead,
            GoogleLocalProfile::PersonalReadSend,
            GoogleLocalProfile::PersonalManage,
        ] {
            input.local_profile = profile;
            input.access_mode = profile.access_mode().unwrap();
            let bytes = request("r", &GoogleSetupCommand::Request(input.clone())).unwrap();
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["params"]["local_profile"], profile.as_str());
            assert_eq!(value["params"].as_object().unwrap().len(), 7);
        }
    }
    #[test]
    fn acc02_google_status_binds_id_and_rejects_tokens() {
        let c = GoogleSetupCommand::Status([1; 32]);
        let r = GoogleSetupReceipt {
            request_id: [1; 32],
            expires_at_utc_ms: 500,
            state: "queued".into(),
            account_id: None,
            login_url: None,
            error_code: None,
            client_ticket: None,
            local_profile: GoogleLocalProfile::None,
        };
        let mut v = serde_json::json!({"jsonrpc":"2.0","id":"r","result":r.to_json()});
        assert!(parse_response(&serde_json::to_vec(&v).unwrap(), "r", &c).is_ok());
        v["result"]["access_token"] = "forbidden".into();
        assert!(parse_response(&serde_json::to_vec(&v).unwrap(), "r", &c).is_err());
    }
}
