use crate::{Backend, ErrorCode, TOOLS, VERSION};
use serde_json::{Value, json};
use std::path::Path;

pub fn config(executable: &Path) -> Result<Value, ErrorCode> {
    if !executable.is_absolute() {
        return Err(ErrorCode::InvalidInput);
    }
    let command = executable.to_str().ok_or(ErrorCode::InvalidInput)?;
    Ok(json!({"mcpServers":{"hantong":{"command":command,"args":[]}}}))
}

// Only readiness and granted tool names leave this diagnostic, never mail or profile bytes.
pub fn doctor(
    core: Result<hantong_ipc_client::Readiness, ErrorCode>,
    backend: &mut impl Backend,
) -> (Value, Result<(), ErrorCode>) {
    let mut report = json!({
        "schema_version":1, "transport":"stdio", "protocol_version":VERSION,
        "core":{"state":"unavailable"}, "registration":{"state":"not_checked"},
        "tools":[], "action":"start_hantong"
    });
    let status = match core {
        Ok(status) => status,
        Err(code) => {
            report["core"]["code"] = json!(code.as_str());
            return (report, Err(code));
        }
    };
    // Bootstrap intentionally reports mail_ready=false; authenticated discovery tests grants.
    if !status.transport_ready {
        report["core"]["code"] = json!(ErrorCode::CoreUnavailable.as_str());
        return (report, Err(ErrorCode::CoreUnavailable));
    }
    report["core"] = json!({"state":"ready"});
    match backend.allowed() {
        Ok(grants) => {
            let exposed: Vec<_> = TOOLS
                .iter()
                .filter(|(name, _, _)| grants.iter().any(|g| g == name))
                .map(|(name, _, _)| *name)
                .collect();
            if exposed.is_empty() {
                report["registration"] = json!({"state":"blocked","code":"SCOPE_DENIED"});
                report["action"] = json!("register_and_review");
                return (report, Err(ErrorCode::ScopeDenied));
            }
            report["registration"] = json!({"state":"ready"});
            report["tools"] = json!(exposed);
            report["action"] = Value::Null;
            (report, Ok(()))
        }
        Err(code) => {
            report["registration"] = json!({"state":"blocked","code":code.as_str()});
            report["action"] = json!(match code {
                ErrorCode::ClientNotRegistered
                | ErrorCode::AuthRequired
                | ErrorCode::ScopeDenied => "register_and_review",
                _ => "inspect_error",
            });
            (report, Err(code))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake {
        grants: Result<Vec<String>, ErrorCode>,
        probes: usize,
    }
    impl Backend for Fake {
        fn allowed(&mut self) -> Result<Vec<String>, ErrorCode> {
            self.probes += 1;
            self.grants.clone()
        }
        fn call(&mut self, _: &str, _: Value) -> Result<Value, ErrorCode> {
            panic!("Doctor must not call a mail tool")
        }
    }
    fn ready() -> Result<hantong_ipc_client::Readiness, ErrorCode> {
        Ok(hantong_ipc_client::Readiness {
            transport_ready: true,
            mail_ready: false,
            authenticated: false,
            protocol_version: 1,
        })
    }
    #[test]
    fn mcp01_config_preserves_unicode_spaces_and_json_quoting() {
        let executable = std::env::temp_dir().join("한통 space").join("hantong-mcp");
        let value = config(&executable).unwrap();
        let decoded: Value = serde_json::from_str(&value.to_string()).unwrap();
        assert_eq!(
            decoded["mcpServers"]["hantong"]["command"],
            executable.to_str().unwrap()
        );
        assert_eq!(decoded["mcpServers"]["hantong"]["args"], json!([]));
        assert_eq!(
            config(Path::new("hantong-mcp")),
            Err(ErrorCode::InvalidInput)
        );
    }
    #[test]
    fn mcp01_doctor_unavailable_never_probes_profile() {
        let mut f = Fake {
            grants: Err(ErrorCode::AuthRequired),
            probes: 0,
        };
        let (value, result) = doctor(Err(ErrorCode::CoreUnavailable), &mut f);
        assert_eq!(result, Err(ErrorCode::CoreUnavailable));
        assert_eq!(f.probes, 0);
        assert_eq!(value["registration"]["state"], "not_checked");
        assert_eq!(value["action"], "start_hantong");
    }
    #[test]
    fn mcp03_doctor_filters_execution_and_unknown_grants_without_mail_calls() {
        let mut f = Fake {
            grants: Ok(vec![
                "mail.messages.get".into(),
                "mail.send.execute".into(),
                "unknown".into(),
            ]),
            probes: 0,
        };
        let (value, result) = doctor(ready(), &mut f);
        assert_eq!(result, Ok(()));
        assert_eq!(value["tools"], json!(["mail.messages.get"]));
        assert_eq!(f.probes, 1);
        assert!(value["action"].is_null());
    }
    #[test]
    fn mcp03_doctor_preserves_blocked_error_and_does_not_widen_access() {
        for code in [
            ErrorCode::ClientNotRegistered,
            ErrorCode::PolicyBlocked,
            ErrorCode::ClientSuspended,
        ] {
            let mut f = Fake {
                grants: Err(code),
                probes: 0,
            };
            let (value, result) = doctor(ready(), &mut f);
            assert_eq!(result, Err(code));
            assert_eq!(value["registration"]["code"], code.as_str());
            assert_eq!(value["tools"], json!([]));
            assert!(!value.to_string().contains("account_id"));
        }
    }
}
