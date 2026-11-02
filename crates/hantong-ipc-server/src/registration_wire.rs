//! Fixed registration requests only; no caller-supplied grant or approval.
use crate::RpcRequest;
use hantong_contracts::{
    ErrorCode,
    registration::{RegistrationCommand, RegistrationReceipt},
    wire_hex,
};
use serde_json::{Value, json};

pub fn parse(request: &RpcRequest) -> Result<RegistrationCommand, ErrorCode> {
    if matches!(
        request.method.as_str(),
        "registration.ai.request" | "registration.ai.status"
    ) {
        let status = request.method == "registration.ai.status";
        let params = request.params.as_object().ok_or(ErrorCode::InvalidInput)?;
        if params.len() != if status { 3 } else { 2 } {
            return Err(ErrorCode::InvalidInput);
        }
        let version = params
            .get("protocol_version")
            .and_then(Value::as_u64)
            .ok_or(ErrorCode::InvalidInput)?;
        if version != 1 {
            return Err(ErrorCode::VersionMismatch);
        }
        let provider = hantong_contracts::registration::AiProvider::parse(
            params
                .get("provider")
                .and_then(Value::as_str)
                .ok_or(ErrorCode::InvalidInput)?,
        )?;
        if !status {
            return Ok(RegistrationCommand::AiRequest(provider));
        }
        let id = wire_hex::decode_fixed::<32>(
            params
                .get("request_id")
                .and_then(Value::as_str)
                .ok_or(ErrorCode::InvalidInput)?,
        )?;
        if id == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        return Ok(RegistrationCommand::AiStatus(provider, id));
    }
    if request.method == "registration.google_readonly.request" {
        let p = request.params.as_object().ok_or(ErrorCode::InvalidInput)?;
        if p.len() != 2 {
            return Err(ErrorCode::InvalidInput);
        }
        let version = p
            .get("protocol_version")
            .and_then(Value::as_u64)
            .ok_or(ErrorCode::InvalidInput)?;
        if version != 1 {
            return Err(ErrorCode::VersionMismatch);
        }
        let account = hantong_contracts::registration::GoogleReadAccount::parse(
            p.get("account_id")
                .and_then(Value::as_str)
                .ok_or(ErrorCode::InvalidInput)?,
        )?;
        return Ok(RegistrationCommand::RequestGoogleReadOnly(account));
    }
    let mcp = request.method.starts_with("registration.mcp.");
    let (status, gui) = match request.method.as_str() {
        "registration.mcp.request" => (false, false),
        "registration.mcp.status" => (true, false),
        "registration.request" => (false, false),
        "registration.status" => (true, false),
        "registration.gui.request" => (false, true),
        "registration.gui.status" => (true, true),
        _ => return Err(ErrorCode::InvalidInput),
    };
    let params = request.params.as_object().ok_or(ErrorCode::InvalidInput)?;
    if params.len() != if status { 2 } else { 1 } {
        return Err(ErrorCode::InvalidInput);
    }
    let version = params
        .get("protocol_version")
        .and_then(Value::as_u64)
        .ok_or(ErrorCode::InvalidInput)?;
    if version != 1 {
        return Err(ErrorCode::VersionMismatch);
    }
    if !status {
        return Ok(if mcp {
            RegistrationCommand::McpRequest
        } else if gui {
            RegistrationCommand::GuiRequest
        } else {
            RegistrationCommand::Request
        });
    }
    let id = wire_hex::decode_fixed::<32>(
        params
            .get("request_id")
            .and_then(Value::as_str)
            .ok_or(ErrorCode::InvalidInput)?,
    )?;
    if id == [0; 32] {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(if mcp {
        RegistrationCommand::McpStatus(id)
    } else if gui {
        RegistrationCommand::GuiStatus(id)
    } else {
        RegistrationCommand::Status(id)
    })
}

pub fn result(receipt: &RegistrationReceipt) -> Value {
    json!({
        "protocol_version": 1,
        "request_id": wire_hex::encode(receipt.request_id()),
        "expires_at_utc_ms": receipt.expires_at_utc_ms(),
        "state": receipt.state().as_str(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::registration::RegistrationProgress;
    fn request(method: &str, params: Value) -> RpcRequest {
        RpcRequest {
            id: "1".into(),
            method: method.into(),
            params,
        }
    }
    #[test]
    fn ai07_registration_wire_routes_provider_bound_requests_without_authority_fields() {
        use hantong_contracts::registration::AiProvider;
        for provider in [AiProvider::Codex, AiProvider::Claude] {
            assert_eq!(
                parse(&request(
                    "registration.ai.request",
                    json!({"protocol_version":1,"provider":provider.as_str()})
                )),
                Ok(RegistrationCommand::AiRequest(provider))
            );
            assert_eq!(
                parse(&request(
                    "registration.ai.status",
                    json!({"protocol_version":1,"provider":provider.as_str(),"request_id":"01".repeat(32)})
                )),
                Ok(RegistrationCommand::AiStatus(provider, [1; 32]))
            );
        }
        for params in [
            json!({"protocol_version":1}),
            json!({"protocol_version":1,"provider":"unknown"}),
            json!({"protocol_version":1,"provider":"codex","approved":true}),
            json!({"protocol_version":1,"provider":"codex","accounts":["other"]}),
            json!({"protocol_version":1.0,"provider":"codex"}),
        ] {
            assert_eq!(
                parse(&request("registration.ai.request", params)),
                Err(ErrorCode::InvalidInput)
            );
        }
        assert_eq!(
            parse(&request(
                "registration.ai.request",
                json!({"protocol_version":2,"provider":"codex"})
            )),
            Err(ErrorCode::VersionMismatch)
        );
        assert_eq!(
            parse(&request(
                "registration.ai.status",
                json!({"protocol_version":1,"provider":"codex","request_id":"00".repeat(32)})
            )),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(
            parse(&request(
                "registration.ai.activate",
                json!({"protocol_version":1,"provider":"codex"})
            )),
            Err(ErrorCode::InvalidInput)
        );
        let rpc = request(
            "registration.ai.request",
            json!({"protocol_version":1,"provider":"codex"}),
        );
        let mut calls = 0;
        let mut callback = |command| {
            calls += 1;
            assert!(matches!(
                command,
                crate::setup_dispatch::SetupCommand::Registration(RegistrationCommand::AiRequest(
                    AiProvider::Codex
                ))
            ));
            Ok(crate::setup_dispatch::SetupReceipt::Registration(
                RegistrationReceipt::new([9; 32], 300001, RegistrationProgress::Pending).unwrap(),
            ))
        };
        let result = crate::setup_dispatch::response(&rpc, 180, &mut callback);
        assert_eq!(result["result"]["state"], "pending");
        assert_eq!(result["result"]["request_id"], "09".repeat(32));
        let oversized = crate::setup_dispatch::response(&rpc, 16385, &mut callback);
        assert!(oversized.get("error").is_some());
        assert_eq!(calls, 1);
    }
    #[test]
    fn sec03_google_readonly_wire_rejects_authority_other_accounts_and_duplicate_fields() {
        let id = format!("google_{}", "1a".repeat(32));
        let account = hantong_contracts::registration::GoogleReadAccount::parse(&id).unwrap();
        assert_eq!(
            parse(&request(
                "registration.google_readonly.request",
                json!({"protocol_version":1,"account_id":id})
            )),
            Ok(RegistrationCommand::RequestGoogleReadOnly(account))
        );
        assert!(
            !format!("{:?}", RegistrationCommand::RequestGoogleReadOnly(account)).contains(&id)
        );
        for params in [
            json!({"protocol_version":1}),
            json!({"protocol_version":1.0,"account_id":id}),
            json!({"protocol_version":1,"account_id":[id]}),
            json!({"protocol_version":1,"account_id":"../default"}),
            json!({"protocol_version":1,"account_id":id,"tools":["mail.send.execute"]}),
            json!({"protocol_version":1,"account_id":id,"approved":true}),
        ] {
            assert_eq!(
                parse(&request("registration.google_readonly.request", params)),
                Err(ErrorCode::InvalidInput)
            );
        }
        assert_eq!(
            parse(&request(
                "registration.google_readonly.request",
                json!({"protocol_version":2,"account_id":id})
            )),
            Err(ErrorCode::VersionMismatch)
        );
        let clean=serde_json::to_string(&json!({"jsonrpc":"2.0","id":"r","method":"registration.google_readonly.request","params":{"protocol_version":1,"account_id":id}})).unwrap();
        let duplicated = clean.replace(
            "\"protocol_version\":1",
            "\"protocol_version\":1,\"protocol_version\":1",
        );
        let mut frame = (duplicated.len() as u32).to_le_bytes().to_vec();
        frame.extend(duplicated.as_bytes());
        assert_eq!(crate::decode_request(&frame), Err(ErrorCode::InvalidInput));
    }
    #[test]
    fn sec03_gui_registration_routes_fixed_kind_and_rejects_authority_fields() {
        assert_eq!(
            parse(&request(
                "registration.gui.request",
                json!({"protocol_version":1})
            )),
            Ok(RegistrationCommand::GuiRequest)
        );
        assert_eq!(
            parse(&request(
                "registration.gui.status",
                json!({"protocol_version":1,"request_id":wire_hex::encode(&[1;32])})
            )),
            Ok(RegistrationCommand::GuiStatus([1; 32]))
        );
        for params in [
            json!(null),
            json!([]),
            json!({}),
            json!({"protocol_version":1,"client_kind":"gui"}),
            json!({"protocol_version":1,"approved":true}),
            json!({"protocol_version":1,"scope":[]}),
        ] {
            assert_eq!(
                parse(&request("registration.gui.request", params)),
                Err(ErrorCode::InvalidInput)
            );
        }
        for method in ["registration.gui.approve", "registration.gui.activate"] {
            assert_eq!(
                parse(&request(method, json!({"protocol_version":1}))),
                Err(ErrorCode::InvalidInput)
            );
        }
        let body = br#"{"jsonrpc":"2.0","id":"g","method":"registration.gui.status","params":{"protocol_version":1,"protocol_version":1}}"#;
        let mut frame = (body.len() as u32).to_le_bytes().to_vec();
        frame.extend(body);
        assert_eq!(crate::decode_request(&frame), Err(ErrorCode::InvalidInput));
    }
    #[test]
    fn sec03_mcp_registration_routes_fixed_kind_and_rejects_authority_fields() {
        assert_eq!(
            parse(&request(
                "registration.mcp.request",
                json!({"protocol_version":1})
            )),
            Ok(RegistrationCommand::McpRequest)
        );
        assert_eq!(
            parse(&request(
                "registration.mcp.status",
                json!({"protocol_version":1,"request_id":wire_hex::encode(&[1;32])})
            )),
            Ok(RegistrationCommand::McpStatus([1; 32]))
        );
        for params in [
            json!(null),
            json!([]),
            json!({}),
            json!({"protocol_version":1,"client_kind":"mcp"}),
            json!({"protocol_version":1,"approved":true}),
            json!({"protocol_version":1,"scope":[]}),
        ] {
            assert_eq!(
                parse(&request("registration.mcp.request", params)),
                Err(ErrorCode::InvalidInput)
            );
        }
        for method in ["registration.mcp.approve", "registration.mcp.activate"] {
            assert_eq!(
                parse(&request(method, json!({"protocol_version":1}))),
                Err(ErrorCode::InvalidInput)
            );
        }
        let body = br#"{"jsonrpc":"2.0","id":"g","method":"registration.mcp.status","params":{"protocol_version":1,"protocol_version":1}}"#;
        let mut frame = (body.len() as u32).to_le_bytes().to_vec();
        frame.extend(body);
        assert_eq!(crate::decode_request(&frame), Err(ErrorCode::InvalidInput));
    }
    #[test]
    fn cli15_registration_wire_has_only_request_and_status() {
        assert_eq!(
            parse(&request(
                "registration.request",
                json!({"protocol_version":1})
            )),
            Ok(RegistrationCommand::Request)
        );
        assert_eq!(
            parse(&request(
                "registration.status",
                json!({"protocol_version":1,"request_id":wire_hex::encode(&[1;32])})
            )),
            Ok(RegistrationCommand::Status([1; 32]))
        );
        for method in [
            "registration.approve",
            "registration.deny",
            "registration.activate",
            "auth.begin",
            "mail.accounts.list",
        ] {
            assert_eq!(
                parse(&request(method, json!({"protocol_version":1}))),
                Err(ErrorCode::InvalidInput)
            );
        }
    }
    #[test]
    fn sec03_registration_wire_rejects_authority_fields_and_wrong_types() {
        for params in [
            json!(null),
            json!([]),
            json!({}),
            json!({"protocol_version":1.0}),
            json!({"protocol_version":true}),
            json!({"protocol_version":1,"approved":true}),
            json!({"protocol_version":1,"scope":[]}),
            json!({"protocol_version":1,"client_key":"x"}),
        ] {
            assert_eq!(
                parse(&request("registration.request", params)),
                Err(ErrorCode::InvalidInput)
            );
        }
        assert_eq!(
            parse(&request(
                "registration.request",
                json!({"protocol_version":2})
            )),
            Err(ErrorCode::VersionMismatch)
        );
        for id in [
            "00".repeat(32),
            "AA".repeat(32),
            "01".repeat(31),
            "../profile".into(),
        ] {
            assert_eq!(
                parse(&request(
                    "registration.status",
                    json!({"protocol_version":1,"request_id":id})
                )),
                Err(ErrorCode::InvalidInput)
            );
        }
    }
    #[test]
    fn sec03_registration_wire_duplicate_fields_rejected_before_callback() {
        let body=br#"{"jsonrpc":"2.0","id":"1","method":"registration.request","params":{"protocol_version":1,"protocol_version":1}}"#;
        let mut frame = (body.len() as u32).to_le_bytes().to_vec();
        frame.extend(body);
        assert_eq!(crate::decode_request(&frame), Err(ErrorCode::InvalidInput));
    }
    #[test]
    fn cli15_registration_receipt_has_no_identity_or_scope() {
        for state in [
            RegistrationProgress::Pending,
            RegistrationProgress::Denied,
            RegistrationProgress::Expired,
            RegistrationProgress::Provisioning,
            RegistrationProgress::Ready,
        ] {
            let receipt = RegistrationReceipt::new([1; 32], 1, state).unwrap();
            let value = result(&receipt);
            assert_eq!(value.as_object().unwrap().len(), 4);
            assert_eq!(value["state"], state.as_str());
            assert!(!format!("{receipt:?}").contains(&"01".repeat(32)));
        }
        assert!(RegistrationReceipt::new([0; 32], 1, RegistrationProgress::Pending).is_err());
        assert!(RegistrationReceipt::new([1; 32], 0, RegistrationProgress::Pending).is_err());
    }
}
