use crate::RpcRequest;
use hantong_contracts::{
    ErrorCode,
    imap_setup::ImapSetupTls,
    smtp_setup::{SmtpSetupCommand, SmtpSetupInput, SmtpSetupReceipt},
    wire_hex,
};
use serde_json::{Value, json};
pub fn parse(request: &RpcRequest) -> Result<SmtpSetupCommand, ErrorCode> {
    let p = request.params.as_object().ok_or(ErrorCode::InvalidInput)?;
    if p.get("protocol_version").and_then(Value::as_u64) != Some(1) {
        return Err(ErrorCode::VersionMismatch);
    }
    match request.method.as_str() {
        "setup.smtp.status" => {
            if p.len() != 2 {
                return Err(ErrorCode::InvalidInput);
            }
            let id = wire_hex::decode_fixed(
                p.get("request_id")
                    .and_then(Value::as_str)
                    .ok_or(ErrorCode::InvalidInput)?,
            )?;
            if id == [0; 32] {
                return Err(ErrorCode::InvalidInput);
            }
            Ok(SmtpSetupCommand::Status(id))
        }
        "setup.smtp.request" => {
            if p.len() != 6 {
                return Err(ErrorCode::InvalidInput);
            }
            let text = |k| {
                p.get(k)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or(ErrorCode::InvalidInput)
            };
            let tls = match text("tls")?.as_str() {
                "implicit_tls" => ImapSetupTls::ImplicitTls,
                "starttls_required" => ImapSetupTls::StarttlsRequired,
                _ => return Err(ErrorCode::InvalidInput),
            };
            let port = u16::try_from(
                p.get("port")
                    .and_then(Value::as_u64)
                    .ok_or(ErrorCode::InvalidInput)?,
            )
            .map_err(|_| ErrorCode::InvalidInput)?;
            Ok(SmtpSetupCommand::Request(SmtpSetupInput::new(
                text("account_id")?,
                text("host")?,
                port,
                tls,
                text("login_name")?,
            )?))
        }
        "setup.smtp.approve" | "setup.smtp.execute" | "setup.smtp.complete" => {
            Err(ErrorCode::HumanStepRequired)
        }
        _ => Err(ErrorCode::InvalidInput),
    }
}
pub fn result(r: &SmtpSetupReceipt) -> Value {
    json!({"protocol_version":1,"request_id":wire_hex::encode(&r.request_id),"expires_at_utc_ms":r.expires_at_utc_ms,"state":r.state.as_str(),"error_code":r.error_code.map(ErrorCode::as_str)})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn setup01_smtp_rejects_cli_secrets_and_approval() {
        let mut q = RpcRequest {
            id: "1".into(),
            method: "setup.smtp.request".into(),
            params: json!({"protocol_version":1,"account_id":"acct","host":"smtp.example","port":465,"tls":"implicit_tls","login_name":"test@example.test"}),
        };
        assert!(matches!(parse(&q), Ok(SmtpSetupCommand::Request(_))));
        q.params["password"] = json!("forbidden");
        assert!(parse(&q).is_err());
        q.method = "setup.smtp.approve".into();
        assert_eq!(parse(&q), Err(ErrorCode::HumanStepRequired));
    }
}
