use crate::{MailEventsAction, ToolRequest};
use hantong_contracts::{ErrorCode, audit::AuditTool};
use serde_json::{Value, json};

fn envelope(tool: &str, params: Value) -> Value {
    json!({"schema_version":1,"request_id":"event-read","tool":tool,"params":params})
}

#[test]
fn mcp07_event_requests_bind_accounts_bounds_and_refuse_implicit_all() {
    let params = json!({"account_ids":["a"],"cursor":null,"limit":50,"pending_only":true});
    let request = ToolRequest::validate(&envelope("mail.events.read", params.clone())).unwrap();
    assert_eq!(request.tool(), AuditTool::EventsRead);
    assert_eq!(request.account_ids(), &["a"]);
    assert!(!format!("{request:?}").contains("event-read"));
    for accounts in [
        json!([]),
        json!(["a", "a"]),
        json!([""]),
        json!(["../other"]),
    ] {
        let mut bad = params.clone();
        bad["account_ids"] = accounts;
        assert_eq!(
            ToolRequest::validate(&envelope("mail.events.read", bad)),
            Err(ErrorCode::InvalidInput)
        );
    }
    for (key, value) in [
        ("limit", json!(0)),
        ("limit", json!(101)),
        ("cursor", json!("\u{1b}[2J")),
        ("cursor", json!("x".repeat(2049))),
        ("approved", json!(true)),
        ("pending_only", json!("true")),
    ] {
        let mut bad = params.clone();
        bad[key] = value;
        assert_eq!(
            ToolRequest::validate(&envelope("mail.events.read", bad)),
            Err(ErrorCode::InvalidInput)
        );
    }
}

#[test]
fn mcp07_configure_uses_read_capability_but_its_own_audit_and_ack_is_distinct() {
    let configured = ToolRequest::validate(&envelope(
        "mail.events.configure",
        json!({"account_ids":["a"],"enabled":true}),
    ))
    .unwrap();
    assert_eq!(configured.tool_name(), "mail.events.read");
    assert_eq!(configured.tool(), AuditTool::EventsConfigure);
    assert!(matches!(
        configured,
        ToolRequest::MailEvents {
            action: MailEventsAction::Configure { enabled: true },
            ..
        }
    ));
    let ack = ToolRequest::validate(&envelope(
        "mail.events.ack",
        json!({"account_ids":["a"],"cursor":"opaque"}),
    ))
    .unwrap();
    assert_eq!(ack.tool_name(), "mail.events.ack");
    assert_eq!(ack.tool(), AuditTool::EventsAck);
    assert!(
        ToolRequest::validate(&envelope(
            "mail.events.ack",
            json!({"account_ids":["a"],"cursor":null})
        ))
        .is_err()
    );
}
