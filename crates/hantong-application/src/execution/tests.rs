use super::*;
use serde_json::json;

struct Fake {
    events: Vec<&'static str>,
    audit_id: Option<[u8; 32]>,
    terminals: Vec<([u8; 32], AuditTerminal)>,
    scoped_accounts: Vec<Vec<String>>,
    reject_multi: bool,
    fail_revalidate: Option<ErrorCode>,
    fail_start: Option<ErrorCode>,
    fail_scope: Option<ErrorCode>,
    fail_policy: Option<ErrorCode>,
    read: Result<ReadResult<u32>, ErrorCode>,
    fail_finish: bool,
}
impl Fake {
    fn ok() -> Self {
        Self {
            events: Vec::new(),
            audit_id: None,
            terminals: Vec::new(),
            scoped_accounts: Vec::new(),
            reject_multi: false,
            fail_revalidate: None,
            fail_start: None,
            fail_scope: None,
            fail_policy: None,
            read: Ok(ReadResult::new(7, Some(1)).unwrap()),
            fail_finish: false,
        }
    }
    fn send() -> serde_json::Value {
        json!({"schema_version":1,"request_id":"r","tool":"mail.send.execute","params":{"account_id":"a","draft_id":"d","operation_id":"o","expected_version":1,"payload_hash":"aa00000000000000000000000000000000000000000000000000000000000000","approval_id":"approval","idempotency_key":"i"}})
    }
    fn accounts() -> serde_json::Value {
        json!({"schema_version":1,"request_id":"r","tool":"mail.accounts.list","params":{"account_id":"a"}})
    }
    fn accounts_many() -> serde_json::Value {
        json!({"schema_version":1,"request_id":"r","tool":"mail.accounts.list","params":{"account_ids":["a","b"]}})
    }
}
impl ExecutionContext for Fake {
    type Output = u32;
    fn revalidate(&mut self) -> Result<(), ErrorCode> {
        self.events.push("revalidate");
        self.fail_revalidate.map_or(Ok(()), Err)
    }
    fn begin_audit(&mut self, _: &ToolRequest) -> Result<[u8; 32], ErrorCode> {
        self.events.push("begin");
        let id = [1; 32];
        self.audit_id = Some(id);
        self.fail_start.map_or(Ok(id), Err)
    }
    fn check_scope(&mut self, request: &ToolRequest) -> Result<(), ErrorCode> {
        self.events.push("scope");
        self.scoped_accounts.push(request.account_ids().to_vec());
        if self.reject_multi && request.account_ids().len() > 1 {
            return Err(ErrorCode::ScopeDenied);
        }
        self.fail_scope.map_or(Ok(()), Err)
    }
    fn check_policy(&mut self, _: &ToolRequest) -> Result<(), ErrorCode> {
        self.events.push("policy");
        self.fail_policy.map_or(Ok(()), Err)
    }
    fn read_accounts(&mut self, _: &AccountsListRequest) -> Result<ReadResult<u32>, ErrorCode> {
        self.events.push("read");
        std::mem::replace(&mut self.read, Err(ErrorCode::InternalError))
    }
    fn finish_audit(&mut self, id: [u8; 32], terminal: AuditTerminal) -> Result<(), ErrorCode> {
        self.events.push(if terminal.is_success() {
            "finish_ok"
        } else {
            "finish_error"
        });
        self.terminals.push((id, terminal));
        if self.fail_finish {
            Err(ErrorCode::InternalError)
        } else {
            Ok(())
        }
    }
}

#[test]
fn m109_success_orders_and_returns_service_count() {
    let mut fake = Fake::ok();
    assert_eq!(
        execute_tool(&mut fake, "mail.accounts.list", &Fake::accounts()),
        Ok(7)
    );
    assert_eq!(fake.audit_id, Some([1; 32]));
    assert_eq!(fake.terminals.len(), 1);
    assert_eq!(fake.terminals[0].0, [1; 32]);
    assert!(fake.terminals[0].1.is_success());
    assert_eq!(fake.terminals[0].1.result_count(), Some(1));
    assert_eq!(fake.scoped_accounts, vec![vec!["a".to_owned()]]);
    assert_eq!(
        fake.events,
        [
            "revalidate",
            "begin",
            "scope",
            "policy",
            "read",
            "finish_ok"
        ]
    );
}
#[test]
fn m109_malformed_and_method_mismatch_stop_before_context() {
    let mut fake = Fake::ok();
    assert_eq!(
        execute_tool(&mut fake, "mail.send.execute", &Fake::accounts()),
        Err(ErrorCode::InvalidInput)
    );
    assert!(fake.events.is_empty());
    assert_eq!(
        execute_tool(&mut fake, "mail.accounts.list", &json!({})),
        Err(ErrorCode::InvalidInput)
    );
    assert!(fake.events.is_empty());
    let sentinel = json!({"schema_version":1,"request_id":"secret","tool":"mail.accounts.list","params":{"account_id":"a","approved":true}});
    assert_eq!(
        execute_tool(&mut fake, "mail.accounts.list", &sentinel),
        Err(ErrorCode::InvalidInput)
    );
    assert!(fake.events.is_empty());
}
#[test]
fn m109_each_gate_is_fail_closed() {
    for (field, expected) in [
        ("fail_revalidate", ErrorCode::AuthRequired),
        ("fail_start", ErrorCode::Conflict),
        ("fail_scope", ErrorCode::ScopeDenied),
        ("fail_policy", ErrorCode::PolicyBlocked),
    ] {
        let mut fake = Fake::ok();
        match field {
            "fail_revalidate" => fake.fail_revalidate = Some(expected),
            "fail_start" => fake.fail_start = Some(expected),
            "fail_scope" => fake.fail_scope = Some(expected),
            _ => fake.fail_policy = Some(expected),
        }
        assert_eq!(
            execute_tool(&mut fake, "mail.accounts.list", &Fake::accounts()),
            Err(expected)
        );
        if field == "fail_revalidate" {
            assert_eq!(fake.events, ["revalidate"]);
        }
        if field == "fail_start" {
            assert_eq!(fake.events, ["revalidate", "begin"]);
        }
        if field == "fail_scope" {
            assert_eq!(
                fake.events,
                ["revalidate", "begin", "scope", "finish_error"]
            );
        }
        if field == "fail_policy" {
            assert_eq!(
                fake.events,
                ["revalidate", "begin", "scope", "policy", "finish_error"]
            );
        }
    }
}
#[test]
fn m109_send_is_blocked_even_with_approval_field() {
    let mut fake = Fake::ok();
    assert_eq!(
        execute_tool(&mut fake, "mail.send.execute", &Fake::send()),
        Err(ErrorCode::PolicyBlocked)
    );
    assert_eq!(
        fake.events,
        ["revalidate", "begin", "scope", "policy", "finish_error"]
    );
}
#[test]
fn m109_service_error_is_terminal_and_not_retried() {
    let mut fake = Fake::ok();
    fake.read = Err(ErrorCode::Offline);
    assert_eq!(
        execute_tool(&mut fake, "mail.accounts.list", &Fake::accounts()),
        Err(ErrorCode::Offline)
    );
    assert_eq!(
        fake.events,
        [
            "revalidate",
            "begin",
            "scope",
            "policy",
            "read",
            "finish_error"
        ]
    );
    assert_eq!(fake.terminals[0].1.error(), Some(ErrorCode::Offline));
}

#[test]
fn m109_multi_account_scope_is_checked_as_one_batch() {
    let mut fake = Fake::ok();
    fake.reject_multi = true;
    assert_eq!(
        execute_tool(&mut fake, "mail.accounts.list", &Fake::accounts_many()),
        Err(ErrorCode::ScopeDenied)
    );
    assert_eq!(
        fake.scoped_accounts,
        vec![vec!["a".to_owned(), "b".to_owned()]]
    );
    assert_eq!(
        fake.events,
        ["revalidate", "begin", "scope", "finish_error"]
    );
    assert_eq!(fake.terminals[0].1.error(), Some(ErrorCode::ScopeDenied));
}

#[test]
fn m109_finish_failure_after_denial_is_internal_and_not_retried() {
    let mut fake = Fake::ok();
    fake.fail_scope = Some(ErrorCode::ScopeDenied);
    fake.fail_finish = true;
    assert_eq!(
        execute_tool(&mut fake, "mail.accounts.list", &Fake::accounts()),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        fake.events,
        ["revalidate", "begin", "scope", "finish_error"]
    );
    assert_eq!(fake.terminals.len(), 1);
    assert_eq!(fake.terminals[0].1.error(), Some(ErrorCode::ScopeDenied));
}
#[test]
fn m109_terminal_failure_returns_internal_without_retry() {
    let mut fake = Fake::ok();
    fake.fail_finish = true;
    assert_eq!(
        execute_tool(&mut fake, "mail.accounts.list", &Fake::accounts()),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        fake.events,
        [
            "revalidate",
            "begin",
            "scope",
            "policy",
            "read",
            "finish_ok"
        ]
    );
}
#[test]
fn m109_count_bound_is_checked() {
    assert!(ReadResult::<u32>::new(1, Some(i64::MAX as u64)).is_ok());
    assert!(ReadResult::<u32>::new(1, Some(i64::MAX as u64 + 1)).is_err());
}

#[test]
fn m109_unknown_canonical_account_stops_before_scope_policy_and_service() {
    let mut fake = Fake::ok();
    fake.fail_start = Some(ErrorCode::ScopeDenied);
    assert_eq!(
        execute_tool(&mut fake, "mail.accounts.list", &Fake::accounts()),
        Err(ErrorCode::ScopeDenied)
    );
    assert_eq!(fake.events, ["revalidate", "begin"]);
    assert!(fake.terminals.is_empty());
}
