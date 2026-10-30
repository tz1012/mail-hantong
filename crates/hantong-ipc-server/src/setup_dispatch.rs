//! Typed bootstrap routing shared by registration and new account setup.
//! The transport and Core still own OS peer validation and the single UI queue.
use crate::RpcRequest;
use hantong_contracts::{
    ErrorCode,
    imap_setup::{ImapSetupCommand, ImapSetupReceipt},
    registration::{RegistrationCommand, RegistrationReceipt},
    scope_refresh::{ScopeRefreshCommand, ScopeRefreshReceipt},
};
use serde_json::{Value, json};

pub enum SetupCommand {
    Registration(RegistrationCommand),
    Imap(ImapSetupCommand),
    Smtp(hantong_contracts::smtp_setup::SmtpSetupCommand),
    ScopeRefresh(ScopeRefreshCommand),
}
pub enum SetupReceipt {
    Registration(RegistrationReceipt),
    Imap(ImapSetupReceipt),
    Smtp(hantong_contracts::smtp_setup::SmtpSetupReceipt),
    ScopeRefresh(ScopeRefreshReceipt),
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
    if request.method.starts_with("registration.scope.") {
        if !(1..=16_384).contains(&frame_length) {
            return Err(ErrorCode::InvalidInput);
        }
        let command = crate::scope_refresh_wire::parse(request)?;
        let expected_id = match command {
            ScopeRefreshCommand::Request => None,
            ScopeRefreshCommand::Status(id) => Some(id),
        };
        let SetupReceipt::ScopeRefresh(receipt) = callback(SetupCommand::ScopeRefresh(command))?
        else {
            return Err(ErrorCode::InternalError);
        };
        if expected_id.is_some_and(|id| receipt.refresh_id() != &id) {
            return Err(ErrorCode::InternalError);
        }
        Ok(crate::scope_refresh_wire::result(&receipt))
    } else if request.method.starts_with("registration.") {
        if !(1..=16_384).contains(&frame_length) {
            return Err(ErrorCode::InvalidInput);
        }
        let command = crate::registration_wire::parse(request)?;
        let expected_id = match command {
            RegistrationCommand::Request
            | RegistrationCommand::GuiRequest
            | RegistrationCommand::McpRequest => None,
            RegistrationCommand::Status(id)
            | RegistrationCommand::GuiStatus(id)
            | RegistrationCommand::McpStatus(id) => Some(id),
        };
        let SetupReceipt::Registration(receipt) = callback(SetupCommand::Registration(command))?
        else {
            return Err(ErrorCode::InternalError);
        };
        if expected_id.is_some_and(|id| receipt.request_id() != &id) {
            return Err(ErrorCode::InternalError);
        }
        Ok(crate::registration_wire::result(&receipt))
    } else if request.method.starts_with("setup.smtp.") {
        if !(1..=16384).contains(&frame_length) {
            return Err(ErrorCode::InvalidInput);
        }
        let command = crate::smtp_setup_wire::parse(request)?;
        let expected = match &command {
            hantong_contracts::smtp_setup::SmtpSetupCommand::Status(id) => Some(*id),
            _ => None,
        };
        let SetupReceipt::Smtp(receipt) = callback(SetupCommand::Smtp(command))? else {
            return Err(ErrorCode::InternalError);
        };
        receipt.validate().map_err(|_| ErrorCode::InternalError)?;
        if expected.is_some_and(|id| id != receipt.request_id) {
            return Err(ErrorCode::InternalError);
        }
        Ok(crate::smtp_setup_wire::result(&receipt))
    } else if request.method.starts_with("setup.imap.") {
        if !(1..=16_384).contains(&frame_length) {
            return Err(ErrorCode::InvalidInput);
        }
        let command = crate::imap_setup_wire::parse(request)?;
        let expected_id = match &command {
            ImapSetupCommand::Request(_) | ImapSetupCommand::Resume(_) => None,
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
                    SetupCommand::Smtp(_) => panic!("unexpected SMTP setup"),
                    SetupCommand::ScopeRefresh(_) => panic!("unexpected scope refresh"),
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
#[test]
fn setup01_resume_dispatch_allows_child_id_but_not_wrong_receipt_family() {
    let request = RpcRequest {
        id: "resume".into(),
        method: "setup.imap.resume".into(),
        params: json!({"protocol_version":1,"request_id":hantong_contracts::wire_hex::encode(&[1;32])}),
    };
    let calls = std::cell::Cell::new(0);
    let reply = response(&request, 200, &mut |command| {
        assert!(matches!(command,SetupCommand::Imap(ImapSetupCommand::Resume(id)) if id==[1;32]));
        calls.set(calls.get() + 1);
        Ok(SetupReceipt::Imap(
            ImapSetupReceipt::new(
                [2; 32],
                1000,
                hantong_contracts::imap_setup::ImapSetupState::Pending,
                hantong_contracts::imap_setup::ImapSetupNextAction::HelperRequired,
            )
            .unwrap(),
        ))
    });
    assert_eq!(calls.get(), 1);
    assert_eq!(
        reply["result"]["request_id"],
        hantong_contracts::wire_hex::encode(&[2; 32])
    );
    let reply = response(&request, 200, &mut |_| {
        Ok(SetupReceipt::Registration(
            RegistrationReceipt::new(
                [1; 32],
                1000,
                hantong_contracts::registration::RegistrationProgress::Pending,
            )
            .unwrap(),
        ))
    });
    assert_eq!(reply["error"]["code"], "INTERNAL_ERROR");
}
#[cfg(test)]
mod scope_refresh_tests {
    use super::*;
    use hantong_contracts::{registration::RegistrationProgress, wire_hex};
    fn request(method: &str, params: Value) -> RpcRequest {
        RpcRequest {
            id: "scope-1".into(),
            method: method.into(),
            params,
        }
    }
    #[test]
    fn cli15_scope_refresh_dispatch_is_bounded_and_distinct_from_registration() {
        use hantong_contracts::scope_refresh::ScopeRefreshProgress;
        let make =
            |id| ScopeRefreshReceipt::new(id, 300001, ScopeRefreshProgress::Pending).unwrap();
        let r = request("registration.scope.request", json!({"protocol_version":1}));
        let mut calls = 0;
        let result = response(&r, 16384, &mut |c| {
            calls += 1;
            assert_eq!(
                match c {
                    SetupCommand::ScopeRefresh(c) => c,
                    _ => panic!("wrong family"),
                },
                ScopeRefreshCommand::Request
            );
            Ok(SetupReceipt::ScopeRefresh(make([3; 32])))
        });
        assert_eq!(calls, 1);
        assert_eq!(result["result"]["refresh_id"], wire_hex::encode(&[3; 32]));
        for length in [0, 16385] {
            let result = response(&r, length, &mut |_| panic!("size before callback"));
            assert_eq!(result["error"]["code"], "INVALID_INPUT");
        }
        let result = response(&r, 100, &mut |_| {
            Ok(SetupReceipt::Registration(
                RegistrationReceipt::new([3; 32], 300001, RegistrationProgress::Pending).unwrap(),
            ))
        });
        assert_eq!(result["error"]["code"], "INTERNAL_ERROR");
        let r = request(
            "registration.scope.status",
            json!({"protocol_version":1,"refresh_id":wire_hex::encode(&[4;32])}),
        );
        let result = response(&r, 100, &mut |_| {
            Ok(SetupReceipt::ScopeRefresh(make([3; 32])))
        });
        assert_eq!(result["error"]["code"], "INTERNAL_ERROR");
        for method in [
            "registration.scope.approve",
            "registration.scope.complete",
            "registration.scope.execute",
        ] {
            let r = request(method, json!({"protocol_version":1,"approved":true}));
            let result = response(&r, 100, &mut |_| panic!("cannot issue approval"));
            assert_eq!(result["error"]["code"], "HUMAN_STEP_REQUIRED");
        }
    }
}
