//! Strict SMTP setup bootstrap client; no password or approval fields.
use hantong_contracts::{
    ErrorCode,
    imap_setup::ImapSetupTls,
    smtp_setup::{SmtpSetupCommand, SmtpSetupReceipt, SmtpSetupState},
    wire_hex,
};
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    protocol_version: u64,
    request_id: String,
    expires_at_utc_ms: i64,
    state: String,
    error_code: serde_json::Value,
}
pub fn request(id: &str, command: &SmtpSetupCommand) -> Result<Vec<u8>, ErrorCode> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
    {
        return Err(ErrorCode::InvalidInput);
    }
    let (method, params) = match command {
        SmtpSetupCommand::Status(r) => {
            if *r == [0; 32] {
                return Err(ErrorCode::InvalidInput);
            }
            (
                "setup.smtp.status",
                serde_json::json!({"protocol_version":1,"request_id":wire_hex::encode(r)}),
            )
        }
        SmtpSetupCommand::Request(i) => (
            "setup.smtp.request",
            serde_json::json!({"protocol_version":1,"account_id":i.account_id(),"host":i.host(),"port":i.port(),"tls":match i.tls(){ImapSetupTls::ImplicitTls=>"implicit_tls",ImapSetupTls::StarttlsRequired=>"starttls_required"},"login_name":i.login_name()}),
        ),
    };
    serde_json::to_vec(
        &serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
    )
    .map_err(|_| ErrorCode::InternalError)
}
pub fn parse_response(
    bytes: &[u8],
    id: &str,
    command: &SmtpSetupCommand,
) -> Result<SmtpSetupReceipt, ErrorCode> {
    let wire: Receipt = crate::mail_send::decode(bytes, id)?;
    if wire.protocol_version != 1 {
        return Err(ErrorCode::VersionMismatch);
    }
    let error_code = if wire.error_code.is_null() {
        None
    } else {
        Some(
            ErrorCode::ALL
                .iter()
                .copied()
                .find(|e| Some(e.as_str()) == wire.error_code.as_str())
                .ok_or(ErrorCode::InvalidInput)?,
        )
    };
    let r = SmtpSetupReceipt {
        request_id: wire_hex::decode_fixed(&wire.request_id)?,
        expires_at_utc_ms: wire.expires_at_utc_ms,
        state: SmtpSetupState::parse(&wire.state)?,
        error_code,
    };
    r.validate()?;
    if matches!(command,SmtpSetupCommand::Status(id) if id!=&r.request_id) {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(r)
}
pub fn exchange(command: SmtpSetupCommand) -> Result<SmtpSetupReceipt, ErrorCode> {
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
                    let bytes = request("setup-smtp-1", &command)?;
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
                    let result = parse_response(&bytes, "setup-smtp-1", &command);
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
    fn setup01_smtp_receipt_is_strict_and_target_bound() {
        let c = SmtpSetupCommand::Status([1; 32]);
        let base = serde_json::json!({"jsonrpc":"2.0","id":"r","result":{"protocol_version":1,"request_id":"01".repeat(32),"expires_at_utc_ms":2,"state":"pending","error_code":null}});
        let raw = serde_json::to_vec(&base).unwrap();
        assert_eq!(
            parse_response(&raw, "r", &c).unwrap().state,
            SmtpSetupState::Pending
        );
        assert!(parse_response(&raw, "r", &SmtpSetupCommand::Status([2; 32])).is_err());
        let mut secret = base;
        secret["result"]["password"] = serde_json::json!("forbidden");
        assert!(parse_response(&serde_json::to_vec(&secret).unwrap(), "r", &c).is_err());
    }
}
