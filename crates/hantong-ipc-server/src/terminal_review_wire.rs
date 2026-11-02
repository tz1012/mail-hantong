//! Strict JSON parser for the Core-owned foreground terminal review surface.
use crate::RpcRequest;
use hantong_contracts::{
    ErrorCode,
    terminal_review::{
        TerminalReviewCancel, TerminalReviewCommand, TerminalReviewConfirm, TerminalReviewGet,
        TerminalReviewKind,
    },
    wire_hex,
};
use serde_json::Value;

fn exact_keys(p: &serde_json::Map<String, Value>, allowed: &[&str]) -> Result<(), ErrorCode> {
    if p.len() == allowed.len() && p.keys().all(|k| allowed.contains(&k.as_str())) {
        Ok(())
    } else {
        Err(ErrorCode::InvalidInput)
    }
}

fn common(p: &serde_json::Map<String, Value>) -> Result<(), ErrorCode> {
    if p.get("protocol_version").and_then(Value::as_u64) != Some(1) {
        return Err(ErrorCode::VersionMismatch);
    }
    Ok(())
}
fn text<'a>(p: &'a serde_json::Map<String, Value>, key: &str) -> Result<&'a str, ErrorCode> {
    p.get(key)
        .and_then(Value::as_str)
        .ok_or(ErrorCode::InvalidInput)
}
fn optional_account(p: &serde_json::Map<String, Value>) -> Result<Option<String>, ErrorCode> {
    match p.get("account_id") {
        Some(Value::Null) | None => Ok(None),
        Some(Value::String(v)) => Ok(Some(v.clone())),
        _ => Err(ErrorCode::InvalidInput),
    }
}
pub fn parse(r: &RpcRequest) -> Result<TerminalReviewCommand, ErrorCode> {
    let p = r.params.as_object().ok_or(ErrorCode::InvalidInput)?;
    common(p)?;
    match r.method.as_str() {
        "terminal.review.get" => {
            exact_keys(
                p,
                &[
                    "protocol_version",
                    "kind",
                    "request_id",
                    "payload_hash",
                    "account_id",
                ],
            )?;
            let kind = TerminalReviewKind::parse(text(p, "kind")?);
            Ok(TerminalReviewCommand::Get(TerminalReviewGet::new(
                kind?,
                wire_hex::decode_fixed(text(p, "request_id")?)?,
                optional_account(p)?,
                wire_hex::decode_fixed(text(p, "payload_hash")?)?,
            )?))
        }
        "terminal.review.confirm" => {
            exact_keys(p, &["protocol_version", "ticket_id", "snapshot_hash"])?;
            Ok(TerminalReviewCommand::Confirm(TerminalReviewConfirm::new(
                wire_hex::decode_fixed(text(p, "ticket_id")?)?,
                wire_hex::decode_fixed(text(p, "snapshot_hash")?)?,
            )?))
        }
        "terminal.review.cancel" => {
            exact_keys(
                p,
                &[
                    "protocol_version",
                    "kind",
                    "request_id",
                    "payload_hash",
                    "account_id",
                    "ticket_id",
                ],
            )?;
            let get = TerminalReviewGet::new(
                TerminalReviewKind::parse(text(p, "kind")?)?,
                wire_hex::decode_fixed(text(p, "request_id")?)?,
                optional_account(p)?,
                wire_hex::decode_fixed(text(p, "payload_hash")?)?,
            )?;
            Ok(TerminalReviewCommand::Cancel(TerminalReviewCancel::new(
                get,
                wire_hex::decode_fixed(text(p, "ticket_id")?)?,
            )?))
        }
        "terminal.review.secret_submit" => {
            exact_keys(
                p,
                &[
                    "protocol_version",
                    "ticket_id",
                    "snapshot_hash",
                    "secret_ticket_opaque",
                    "staged_handle",
                ],
            )?;
            Ok(TerminalReviewCommand::SecretSubmit(
                hantong_contracts::terminal_review::TerminalReviewSecretSubmit::new(
                    wire_hex::decode_fixed(text(p, "ticket_id")?)?,
                    wire_hex::decode_fixed(text(p, "snapshot_hash")?)?,
                    wire_hex::decode_bounded(text(p, "secret_ticket_opaque")?, 320)?,
                    wire_hex::decode_fixed(text(p, "staged_handle")?)?,
                )?,
            ))
        }
        _ => Err(ErrorCode::InvalidInput),
    }
}
pub fn result(receipt: &hantong_contracts::terminal_review::TerminalReviewReceipt) -> Value {
    receipt.to_json()
}

/// Dispatch one typed terminal review request. The callback owns Core-side
/// peer, ticket and Store validation; this layer only validates the wire shape
/// and enforces request/receipt identity matching.
pub fn response<F>(request: &RpcRequest, frame_length: usize, callback: &mut F) -> Value
where
    F: FnMut(
        TerminalReviewCommand,
    ) -> Result<hantong_contracts::terminal_review::TerminalReviewReceipt, ErrorCode>,
{
    let value = (|| {
        if !(1..=16_384).contains(&frame_length) {
            return Err(ErrorCode::InvalidInput);
        }
        let command = parse(request)?;
        let expected = match &command {
            TerminalReviewCommand::Get(v) => Some((v.kind, v.request_id)),
            TerminalReviewCommand::Cancel(v) => Some((v.get.kind, v.get.request_id)),
            TerminalReviewCommand::Confirm(_) => None,
            TerminalReviewCommand::SecretSubmit(_) => None,
        };
        let receipt = callback(command)?;
        receipt.validate()?;
        if expected.is_some_and(|(kind, id)| receipt.kind != kind || receipt.request_id != id) {
            return Err(ErrorCode::InternalError);
        }
        Ok(result(&receipt))
    })();
    match value {
        Ok(result) => serde_json::json!({"jsonrpc":"2.0","id":request.id,"result":result}),
        Err(code) => {
            serde_json::json!({"jsonrpc":"2.0","id":request.id,"error":{"code":code.as_str(),"message":code.as_str()}})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn req(method: &str, mut p: Value) -> RpcRequest {
        p["protocol_version"] = 1.into();
        RpcRequest {
            id: "r".into(),
            method: method.into(),
            params: p,
        }
    }
    #[test]
    fn terminal_review_wire_has_no_approval_or_caller_fields() {
        let h = "01".repeat(32);
        let r = req(
            "terminal.review.get",
            json!({"kind":"personal_send","request_id":h,"payload_hash":"02".repeat(32),"account_id":null}),
        );
        assert!(parse(&r).is_ok());
        let mut forged = r.params.clone();
        forged["approved"] = true.into();
        assert!(
            parse(&RpcRequest {
                id: "r".into(),
                method: r.method,
                params: forged
            })
            .is_err()
        );
    }
}
