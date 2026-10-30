//! Strict Google setup bootstrap. Public client configuration only.
use hantong_contracts::{
    ErrorCode,
    google_setup::{GoogleSetupCommand, GoogleSetupReceipt},
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
            (
                "setup.google.request",
                serde_json::json!({"protocol_version":1,"client_id":i.client_id,"display_name":i.display_name,"reauthorize_account":i.reauthorize_account,"force_consent":i.force_consent}),
            )
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
    };
    r.validate()?;
    if matches!(c,GoogleSetupCommand::Status(expected) if *expected!=r.request_id) {
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
    #[cfg(not(windows))]
    {
        let _ = command;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        };
        let mut v = serde_json::json!({"jsonrpc":"2.0","id":"r","result":r.to_json()});
        assert!(parse_response(&serde_json::to_vec(&v).unwrap(), "r", &c).is_ok());
        v["result"]["access_token"] = "forbidden".into();
        assert!(parse_response(&serde_json::to_vec(&v).unwrap(), "r", &c).is_err());
    }
}
