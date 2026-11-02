//! Fixed registration requests only; no caller-supplied grant or approval.
use crate::RpcRequest;
use hantong_contracts::{
    ErrorCode,
    scope_refresh::{ScopeRefreshCommand, ScopeRefreshReceipt},
    wire_hex,
};
use serde_json::{Value, json};

pub fn parse(request: &RpcRequest) -> Result<ScopeRefreshCommand, ErrorCode> {
    if matches!(
        request.method.as_str(),
        "registration.scope.google_send.request" | "registration.scope.google_changes.request"
    ) {
        let p = request.params.as_object().ok_or(ErrorCode::InvalidInput)?;
        if p.len() != 2 {
            return Err(ErrorCode::InvalidInput);
        }
        if p.get("protocol_version").and_then(Value::as_u64) != Some(1) {
            return Err(ErrorCode::VersionMismatch);
        }
        let values = p
            .get("account_ids")
            .and_then(Value::as_array)
            .ok_or(ErrorCode::InvalidInput)?;
        if values.len() > 4 {
            return Err(ErrorCode::InvalidInput);
        }
        let values = values
            .iter()
            .map(|v| v.as_str().map(str::to_owned).ok_or(ErrorCode::InvalidInput))
            .collect::<Result<Vec<_>, _>>()?;
        let selected = hantong_contracts::scope_refresh::GoogleSendAccounts::parse(&values)?;
        return Ok(
            if request.method == "registration.scope.google_changes.request" {
                ScopeRefreshCommand::RequestGoogleChanges(selected)
            } else {
                ScopeRefreshCommand::RequestGoogleSend(selected)
            },
        );
    }
    if matches!(
        request.method.as_str(),
        "registration.scope.google_readonly.request" | "registration.scope.google_readonly.add"
    ) {
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
        return Ok(if request.method.ends_with(".add") {
            ScopeRefreshCommand::AddGoogleReadOnly(account)
        } else {
            ScopeRefreshCommand::RequestGoogleReadOnly(account)
        });
    }
    if matches!(
        request.method.as_str(),
        "registration.scope.approve" | "registration.scope.complete" | "registration.scope.execute"
    ) {
        return Err(ErrorCode::HumanStepRequired);
    }
    let status = match request.method.as_str() {
        "registration.scope.request" => false,
        "registration.scope.status" => true,
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
        return Ok(ScopeRefreshCommand::Request);
    }
    let id = wire_hex::decode_fixed::<32>(
        params
            .get("refresh_id")
            .and_then(Value::as_str)
            .ok_or(ErrorCode::InvalidInput)?,
    )?;
    if id == [0; 32] {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(ScopeRefreshCommand::Status(id))
}

pub fn result(receipt: &ScopeRefreshReceipt) -> Value {
    json!({
        "protocol_version": 1,
        "refresh_id": wire_hex::encode(receipt.refresh_id()),
        "expires_at_utc_ms": receipt.expires_at_utc_ms(),
        "state": receipt.state().as_str(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::scope_refresh::ScopeRefreshProgress;
    #[test]
    fn sec03_google_changes_wire_is_distinct_from_send_and_rejects_authority() {
        let id = format!("google_{}", "1a".repeat(32));
        let selected =
            hantong_contracts::scope_refresh::GoogleSendAccounts::parse(std::slice::from_ref(&id))
                .unwrap();
        for (method, expected) in [
            (
                "registration.scope.google_send.request",
                ScopeRefreshCommand::RequestGoogleSend(selected),
            ),
            (
                "registration.scope.google_changes.request",
                ScopeRefreshCommand::RequestGoogleChanges(selected),
            ),
        ] {
            assert_eq!(
                parse(&request(
                    method,
                    json!({"protocol_version":1,"account_ids":[id]})
                )),
                Ok(expected)
            );
            for params in [
                json!({"protocol_version":1,"account_ids":[]}),
                json!({"protocol_version":1,"account_ids":[id,id]}),
                json!({"protocol_version":1,"account_ids":[id],"approved":true}),
                json!({"protocol_version":1,"account_ids":[id],"tools":["mail.google.changes.execute"]}),
            ] {
                assert_eq!(
                    parse(&request(method, params)),
                    Err(ErrorCode::InvalidInput)
                );
            }
        }
    }
    fn request(method: &str, params: Value) -> RpcRequest {
        RpcRequest {
            id: "1".into(),
            method: method.into(),
            params,
        }
    }
    #[test]
    fn sec03_google_readonly_wire_rejects_authority_other_accounts_and_duplicate_fields() {
        let id = format!("google_{}", "1a".repeat(32));
        let account = hantong_contracts::registration::GoogleReadAccount::parse(&id).unwrap();
        assert_eq!(
            parse(&request(
                "registration.scope.google_readonly.request",
                json!({"protocol_version":1,"account_id":id})
            )),
            Ok(ScopeRefreshCommand::RequestGoogleReadOnly(account))
        );
        assert!(
            !format!("{:?}", ScopeRefreshCommand::RequestGoogleReadOnly(account)).contains(&id)
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
                parse(&request(
                    "registration.scope.google_readonly.request",
                    params
                )),
                Err(ErrorCode::InvalidInput)
            );
        }
        assert_eq!(
            parse(&request(
                "registration.scope.google_readonly.request",
                json!({"protocol_version":2,"account_id":id})
            )),
            Err(ErrorCode::VersionMismatch)
        );
        let clean=serde_json::to_string(&json!({"jsonrpc":"2.0","id":"r","method":"registration.scope.google_readonly.request","params":{"protocol_version":1,"account_id":id}})).unwrap();
        let duplicated = clean.replace(
            "\"protocol_version\":1",
            "\"protocol_version\":1,\"protocol_version\":1",
        );
        let mut frame = (duplicated.len() as u32).to_le_bytes().to_vec();
        frame.extend(duplicated.as_bytes());
        assert_eq!(crate::decode_request(&frame), Err(ErrorCode::InvalidInput));
    }
    #[test]
    fn sec03_readonly_add_wire_is_single_selected_account_without_approval_fields() {
        let id = format!("google_{}", "1a".repeat(32));
        let account = hantong_contracts::registration::GoogleReadAccount::parse(&id).unwrap();
        assert_eq!(
            parse(&request(
                "registration.scope.google_readonly.add",
                json!({"protocol_version":1,"account_id":id})
            )),
            Ok(ScopeRefreshCommand::AddGoogleReadOnly(account))
        );
        for params in [
            json!({"protocol_version":1,"account_id":id,"approved":true}),
            json!({"protocol_version":1,"account_ids":[id]}),
            json!({"protocol_version":1,"account_id":id,"tools":["mail.send.execute"]}),
        ] {
            assert_eq!(
                parse(&request("registration.scope.google_readonly.add", params)),
                Err(ErrorCode::InvalidInput)
            );
        }
    }
    #[test]
    fn cli15_scope_refresh_wire_has_only_request_and_status() {
        assert_eq!(
            parse(&request(
                "registration.scope.request",
                json!({"protocol_version":1})
            )),
            Ok(ScopeRefreshCommand::Request)
        );
        assert_eq!(
            parse(&request(
                "registration.scope.status",
                json!({"protocol_version":1,"refresh_id":wire_hex::encode(&[1;32])})
            )),
            Ok(ScopeRefreshCommand::Status([1; 32]))
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
    fn sec03_scope_refresh_wire_rejects_authority_fields_and_wrong_types() {
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
                parse(&request("registration.scope.request", params)),
                Err(ErrorCode::InvalidInput)
            );
        }
        assert_eq!(
            parse(&request(
                "registration.scope.request",
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
                    "registration.scope.status",
                    json!({"protocol_version":1,"refresh_id":id})
                )),
                Err(ErrorCode::InvalidInput)
            );
        }
    }
    #[test]
    fn sec03_scope_refresh_wire_duplicate_fields_rejected_before_callback() {
        let body=br#"{"jsonrpc":"2.0","id":"1","method":"registration.scope.request","params":{"protocol_version":1,"protocol_version":1}}"#;
        let mut frame = (body.len() as u32).to_le_bytes().to_vec();
        frame.extend(body);
        assert_eq!(crate::decode_request(&frame), Err(ErrorCode::InvalidInput));
    }
    #[test]
    fn cli15_scope_refresh_receipt_has_no_identity_or_scope() {
        for state in [
            ScopeRefreshProgress::Pending,
            ScopeRefreshProgress::Denied,
            ScopeRefreshProgress::Expired,
            ScopeRefreshProgress::Provisioning,
            ScopeRefreshProgress::Ready,
            ScopeRefreshProgress::Interrupted,
        ] {
            let receipt = ScopeRefreshReceipt::new([1; 32], 1, state).unwrap();
            let value = result(&receipt);
            assert_eq!(value.as_object().unwrap().len(), 4);
            assert_eq!(value["state"], state.as_str());
            assert!(!format!("{receipt:?}").contains(&"01".repeat(32)));
        }
        assert!(ScopeRefreshReceipt::new([0; 32], 1, ScopeRefreshProgress::Pending).is_err());
        assert!(ScopeRefreshReceipt::new([1; 32], 0, ScopeRefreshProgress::Pending).is_err());
    }
    #[test]
    fn cli15_scope_refresh_never_accepts_completion_assertions() {
        for method in [
            "registration.scope.approve",
            "registration.scope.complete",
            "registration.scope.execute",
        ] {
            assert_eq!(
                parse(&request(
                    method,
                    json!({"protocol_version":1,"approved":true})
                )),
                Err(ErrorCode::HumanStepRequired)
            );
        }
    }
}
