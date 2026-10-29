//! Typed bootstrap routing shared by registration and new account setup.
//! The transport and Core still own OS peer validation and the single UI queue.
use crate::RpcRequest;
use hantong_contracts::{
    ErrorCode,
    imap_setup::{ImapSetupCommand, ImapSetupReceipt},
    registration::{RegistrationCommand, RegistrationReceipt},
};
use serde_json::{Value, json};

pub enum SetupCommand {
    Registration(RegistrationCommand),
    Imap(ImapSetupCommand),
}
pub enum SetupReceipt {
    Registration(RegistrationReceipt),
    Imap(ImapSetupReceipt),
}

/// Original frame length includes JSON whitespace; reserialization is not a bound.
pub fn response<F>(request: &RpcRequest, frame_length: usize, callback: &mut F) -> Value
where
    F: FnMut(SetupCommand) -> Result<SetupReceipt, ErrorCode>,
{
    match dispatch(request, frame_length, callback) {
        Ok(result) => json!({"jsonrpc":"2.0","id":request.id,"result":result}),
        Err(code) => error(request, code),
    }
}
fn dispatch<F>(
    request: &RpcRequest,
    frame_length: usize,
    callback: &mut F,
) -> Result<Value, ErrorCode>
where
    F: FnMut(SetupCommand) -> Result<SetupReceipt, ErrorCode>,
{
    if request.method.starts_with("registration.") {
        let command = crate::registration_wire::parse(request)?;
        let expected_id = match command {
            RegistrationCommand::Request => None,
            RegistrationCommand::Status(id) => Some(id),
        };
        let SetupReceipt::Registration(receipt) = callback(SetupCommand::Registration(command))?
        else {
            return Err(ErrorCode::InternalError);
        };
        if expected_id.is_some_and(|id| receipt.request_id() != &id) {
            return Err(ErrorCode::InternalError);
        }
        Ok(crate::registration_wire::result(&receipt))
    } else if request.method.starts_with("setup.imap.") {
        if !(1..=16_384).contains(&frame_length) {
            return Err(ErrorCode::InvalidInput);
        }
        let command = crate::imap_setup_wire::parse(request)?;
        let expected_id = match &command {
            ImapSetupCommand::Request(_) => None,
            ImapSetupCommand::Status(id) => Some(*id),
        };
        let SetupReceipt::Imap(receipt) = callback(SetupCommand::Imap(command))? else {
            return Err(ErrorCode::InternalError);
        };
        if expected_id.is_some_and(|id| receipt.request_id() != &id) {
            return Err(ErrorCode::InternalError);
        }
        Ok(crate::imap_setup_wire::result(&receipt))
    } else {
        Err(ErrorCode::InvalidInput)
    }
}
fn error(request: &RpcRequest, code: ErrorCode) -> Value {
    json!({"jsonrpc":"2.0","id":request.id,"error":{"code":code.as_str(),"message":code.as_str()}})
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::{
        imap_setup::{ImapSetupNextAction, ImapSetupState},
        registration::RegistrationProgress,
        wire_hex,
    };
    use std::cell::Cell;

    fn request(method: &str, params: Value) -> RpcRequest {
        RpcRequest {
            id: "setup-1".into(),
            method: method.into(),
            params,
        }
    }
    fn input() -> Value {
        json!({"protocol_version":1,"display_name":null,"color":null,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"synthetic"})
    }
    fn imap_receipt(id: [u8; 32]) -> SetupReceipt {
        SetupReceipt::Imap(
            ImapSetupReceipt::new(
                id,
                300001,
                ImapSetupState::Pending,
                ImapSetupNextAction::HelperRequired,
            )
            .unwrap(),
        )
    }
    #[test]
    fn setup06_two_request_families_share_typed_callback_without_auth_or_mail() {
        let calls = Cell::new(0);
        let setup = response(
            &request("setup.imap.request", input()),
            200,
            &mut |command| {
                calls.set(calls.get() + 1);
                let SetupCommand::Imap(ImapSetupCommand::Request(input)) = command else {
                    panic!("wrong family")
                };
                assert_eq!(input.host(), "imap.example");
                Ok(imap_receipt([1; 32]))
            },
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(setup["id"], "setup-1");
        assert_eq!(setup["result"].as_object().unwrap().len(), 5);
        assert_eq!(setup["result"]["state"], "pending");
        let registration = response(
            &request("registration.request", json!({"protocol_version":1})),
            100,
            &mut |command| {
                calls.set(calls.get() + 1);
                assert!(matches!(
                    command,
                    SetupCommand::Registration(RegistrationCommand::Request)
                ));
                Ok(SetupReceipt::Registration(
                    RegistrationReceipt::new([2; 32], 300001, RegistrationProgress::Pending)
                        .unwrap(),
                ))
            },
        );
        assert_eq!(calls.get(), 2);
        assert_eq!(registration["result"].as_object().unwrap().len(), 4);
        for method in [
            "auth.begin",
            "auth.finish",
            "mail.accounts.list",
            "mail.send.execute",
        ] {
            let denied = response(
                &request(method, json!({"protocol_version":1})),
                100,
                &mut |_| panic!("must not call bootstrap"),
            );
            assert_eq!(denied["error"]["code"], "INVALID_INPUT");
        }
    }
    #[test]
    fn inst05_invalid_and_oversized_setup_frames_never_reach_callback() {
        let original = request("setup.imap.request", input());
        for length in [0, 16385, 131072] {
            let denied = response(&original, length, &mut |_| {
                panic!("bad frame must not cause effect")
            });
            assert_eq!(denied["error"]["code"], "INVALID_INPUT");
        }
        for forbidden in ["password", "approved", "account_id", "authentication"] {
            let mut params = input();
            params[forbidden] = json!("untrusted");
            let denied = response(&request("setup.imap.request", params), 300, &mut |_| {
                panic!("untrusted fields")
            });
            assert_eq!(denied["error"]["code"], "INVALID_INPUT");
        }
        let accepted = response(&original, 16384, &mut |_| Ok(imap_receipt([1; 32])));
        assert!(accepted.get("result").is_some());
    }
    #[test]
    fn setup06_completion_assertions_never_invoke_callback() {
        for method in [
            "setup.imap.approve",
            "setup.imap.complete",
            "setup.imap.execute",
        ] {
            let denied = response(&request(method, json!({"approved":true})), 100, &mut |_| {
                panic!("no approval callback")
            });
            assert_eq!(denied["error"]["code"], "HUMAN_STEP_REQUIRED");
        }
    }
    #[test]
    fn setup01_mismatched_callback_family_or_status_id_never_becomes_success() {
        let reg = RegistrationReceipt::new([2; 32], 300001, RegistrationProgress::Pending).unwrap();
        let mismatch = response(&request("setup.imap.request", input()), 200, &mut |_| {
            Ok(SetupReceipt::Registration(reg.clone()))
        });
        assert_eq!(mismatch["error"]["code"], "INTERNAL_ERROR");
        for method in ["registration.status", "setup.imap.status"] {
            let request = request(
                method,
                json!({"protocol_version":1,"request_id":wire_hex::encode(&[1;32])}),
            );
            let mismatch = response(&request, 200, &mut |command| {
                Ok(match command {
                    SetupCommand::Registration(_) => SetupReceipt::Registration(reg.clone()),
                    SetupCommand::Imap(_) => imap_receipt([2; 32]),
                })
            });
            assert_eq!(mismatch["error"]["code"], "INTERNAL_ERROR");
        }
    }
    #[test]
    fn setup01_stable_callback_failure_does_not_reflect_request_fields() {
        let failed = response(&request("setup.imap.request", input()), 200, &mut |_| {
            Err(ErrorCode::QuotaExceeded)
        });
        assert_eq!(
            failed["error"],
            json!({"code":"QUOTA_EXCEEDED","message":"QUOTA_EXCEEDED"})
        );
        assert!(!failed.to_string().contains("imap.example"));
        assert!(!failed.to_string().contains("synthetic"));
    }
}
