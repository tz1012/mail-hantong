//! Strict pre-auth IMAP setup bootstrap wire. It never starts setup itself.
use crate::RpcRequest;
use hantong_contracts::{
    ErrorCode,
    imap_setup::{ImapSetupCommand, ImapSetupInput, ImapSetupReceipt, ImapSetupTls},
    wire_hex,
};
use serde_json::{Value, json};

pub fn parse(request: &RpcRequest) -> Result<ImapSetupCommand, ErrorCode> {
    let status = match request.method.as_str() {
        "setup.imap.request" => false,
        "setup.imap.status" => true,
        "setup.imap.complete" | "setup.imap.approve" | "setup.imap.execute" => {
            return Err(ErrorCode::HumanStepRequired);
        }
        _ => return Err(ErrorCode::InvalidInput),
    };
    let params = request.params.as_object().ok_or(ErrorCode::InvalidInput)?;
    if (!status && params.len() != 7) || (status && params.len() != 2) {
        return Err(ErrorCode::InvalidInput);
    }
    let protocol = params
        .get("protocol_version")
        .and_then(Value::as_u64)
        .ok_or(ErrorCode::InvalidInput)?;
    if protocol != 1 {
        return Err(ErrorCode::VersionMismatch);
    }
    if status {
        let id = wire_hex::decode_fixed::<32>(
            params
                .get("request_id")
                .and_then(Value::as_str)
                .ok_or(ErrorCode::InvalidInput)?,
        )?;
        return if id == [0; 32] {
            Err(ErrorCode::InvalidInput)
        } else {
            Ok(ImapSetupCommand::Status(id))
        };
    }
    let name = opt_string(params, "display_name")?;
    let color = opt_string(params, "color")?;
    let host = params
        .get("host")
        .and_then(Value::as_str)
        .ok_or(ErrorCode::InvalidInput)?
        .to_owned();
    let port = u16::try_from(
        params
            .get("port")
            .and_then(Value::as_u64)
            .ok_or(ErrorCode::InvalidInput)?,
    )
    .map_err(|_| ErrorCode::InvalidInput)?;
    let tls = match params
        .get("tls")
        .and_then(Value::as_str)
        .ok_or(ErrorCode::InvalidInput)?
    {
        "implicit_tls" => ImapSetupTls::ImplicitTls,
        "starttls_required" => ImapSetupTls::StarttlsRequired,
        _ => return Err(ErrorCode::InvalidInput),
    };
    let login = params
        .get("login_name")
        .and_then(Value::as_str)
        .ok_or(ErrorCode::InvalidInput)?
        .to_owned();
    Ok(ImapSetupCommand::Request(ImapSetupInput::new(
        name, color, host, port, tls, login,
    )?))
}
fn opt_string(p: &serde_json::Map<String, Value>, key: &str) -> Result<Option<String>, ErrorCode> {
    match p.get(key).ok_or(ErrorCode::InvalidInput)? {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        _ => Err(ErrorCode::InvalidInput),
    }
}
pub fn result(receipt: &ImapSetupReceipt) -> Value {
    json!({"protocol_version":1,"request_id":wire_hex::encode(receipt.request_id()),"expires_at_utc_ms":receipt.expires_at_utc_ms(),"state":receipt.state().as_str(),"next_action":receipt.next_action().as_str()})
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::imap_setup::{ImapSetupNextAction, ImapSetupState};
    fn q(m: &str, p: Value) -> RpcRequest {
        RpcRequest {
            id: "1".into(),
            method: m.into(),
            params: p,
        }
    }
    #[test]
    fn setup_request_is_strict_and_privacy_bound() {
        let p = json!({"protocol_version":1,"display_name":null,"color":null,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"user"});
        assert!(matches!(
            parse(&q("setup.imap.request", p)),
            Ok(ImapSetupCommand::Request(_))
        ));
        for extra in [
            json!({"protocol_version":1,"display_name":null,"color":null,"host":"x","port":1,"tls":"implicit_tls","login_name":"u","password":"x"}),
            json!({"protocol_version":1,"display_name":null,"color":null,"host":"x","port":1,"tls":"implicit_tls"}),
            json!({"protocol_version":1,"display_name":[],"color":null,"host":"x","port":1,"tls":"implicit_tls","login_name":"u"}),
        ] {
            assert_eq!(
                parse(&q("setup.imap.request", extra)),
                Err(ErrorCode::InvalidInput)
            );
        }
    }
    #[test]
    fn status_and_human_steps_are_separated() {
        assert!(matches!(
            parse(&q(
                "setup.imap.status",
                json!({"protocol_version":1,"request_id":wire_hex::encode(&[1;32])})
            )),
            Ok(ImapSetupCommand::Status(_))
        ));
        assert_eq!(
            parse(&q("setup.imap.complete", json!({"protocol_version":1}))),
            Err(ErrorCode::HumanStepRequired)
        );
    }
    #[test]
    fn receipt_rejects_inconsistent_action_and_hides_fields() {
        assert!(
            ImapSetupReceipt::new(
                [1; 32],
                1,
                ImapSetupState::Pending,
                ImapSetupNextAction::Wait
            )
            .is_err()
        );
        let r = ImapSetupReceipt::new(
            [1; 32],
            1,
            ImapSetupState::Pending,
            ImapSetupNextAction::HelperRequired,
        )
        .unwrap();
        assert_eq!(result(&r).as_object().unwrap().len(), 5);
        assert!(!format!("{r:?}").contains("01".repeat(32).as_str()));
    }

    #[test]
    fn setup_request_requires_every_nullable_field_and_integer_types() {
        let valid = json!({"protocol_version":1,"display_name":null,"color":null,"host":"x","port":1,"tls":"implicit_tls","login_name":"u"});
        for key in ["display_name", "color", "host", "port", "tls", "login_name"] {
            let mut missing = valid.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert_eq!(
                parse(&q("setup.imap.request", missing)),
                Err(ErrorCode::InvalidInput)
            );
        }
        for value in [json!(1.0), json!(true), json!("1"), json!([1])] {
            let mut wrong = valid.clone();
            wrong["port"] = value;
            assert_eq!(
                parse(&q("setup.imap.request", wrong)),
                Err(ErrorCode::InvalidInput)
            );
        }
        for value in [json!(0), json!(65536)] {
            let mut wrong = valid.clone();
            wrong["port"] = value;
            assert_eq!(
                parse(&q("setup.imap.request", wrong)),
                Err(ErrorCode::InvalidInput)
            );
        }
    }

    #[test]
    fn setup_status_rejects_zero_uppercase_and_wrong_types() {
        for id in [
            "0000000000000000000000000000000000000000000000000000000000000000",
            "01010101010101010101010101010101010101010101010101010101010101AA",
        ] {
            assert_eq!(
                parse(&q(
                    "setup.imap.status",
                    json!({"protocol_version":1,"request_id":id})
                )),
                Err(ErrorCode::InvalidInput)
            );
        }
        for id in [json!(1), json!(true), json!([])] {
            assert_eq!(
                parse(&q(
                    "setup.imap.status",
                    json!({"protocol_version":1,"request_id":id})
                )),
                Err(ErrorCode::InvalidInput)
            );
        }
    }

    #[test]
    fn setup01_protocol_type_errors_are_distinct_from_unsupported_versions() {
        for protocol in [json!(null), json!(true), json!(1.0), json!("1"), json!([])] {
            assert_eq!(
                parse(&q(
                    "setup.imap.status",
                    json!({"protocol_version":protocol,"request_id":wire_hex::encode(&[1;32])})
                )),
                Err(ErrorCode::InvalidInput)
            );
        }
        for protocol in [0, 2] {
            assert_eq!(
                parse(&q(
                    "setup.imap.status",
                    json!({"protocol_version":protocol,"request_id":wire_hex::encode(&[1;32])})
                )),
                Err(ErrorCode::VersionMismatch)
            );
        }
    }
}
