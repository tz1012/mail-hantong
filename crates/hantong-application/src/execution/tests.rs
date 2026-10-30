use super::*;
use serde_json::json;

#[test]
fn cli16_ops05_global_reads_keep_validation_scope_policy_audit_order() {
    for (tool, params) in [
        ("app.settings.display.get", json!({})),
        (
            "app.diagnostics.snapshot",
            json!({"from_utc_ms":0,"to_utc_ms":1000,"sections":["errors"]}),
        ),
    ] {
        let envelope = json!({"schema_version":1,"request_id":"r","tool":tool,"params":params});
        let mut fake = Fake::ok();
        fake.read = ReadResult::new(17, None);
        assert_eq!(execute_tool(&mut fake, tool, &envelope), Ok(17));
        assert_eq!(
            fake.events,
            [
                "revalidate",
                "begin",
                "scope",
                "policy",
                "app_read",
                "finish_ok"
            ]
        );
        assert_eq!(fake.scoped_accounts, vec![Vec::<String>::new()]);
        assert_eq!(fake.terminals[0].1.result_count(), None);
        for policy in [false, true] {
            let mut fake = Fake::ok();
            if policy {
                fake.fail_policy = Some(ErrorCode::PolicyBlocked)
            } else {
                fake.fail_scope = Some(ErrorCode::ScopeDenied)
            }
            assert!(execute_tool(&mut fake, tool, &envelope).is_err());
            assert!(!fake.events.contains(&"app_read"));
            assert_eq!(fake.terminals.len(), 1);
        }
        let mut fake = Fake::ok();
        fake.read = ReadResult::new(17, Some(0));
        assert_eq!(
            execute_tool(&mut fake, tool, &envelope),
            Err(ErrorCode::InternalError)
        );
        let mut invalid = envelope.clone();
        invalid["params"]["account_ids"] = json!([]);
        let mut fake = Fake::ok();
        assert_eq!(
            execute_tool(&mut fake, tool, &invalid),
            Err(ErrorCode::InvalidInput)
        );
        assert!(fake.events.is_empty());
    }
}

#[test]
fn sec03_metadata_search_uses_common_policy_audit_and_emitted_item_count() {
    let envelope = json!({"schema_version":1,"request_id":"r","tool":"mail.messages.search","params":{"mode":"imap_metadata","account_id":"a","wire_hex":"494e424f58","encoding":"utf8"}});
    let mut fake = Fake::ok();
    fake.read = ReadResult::new(17, Some(3));
    assert_eq!(
        execute_tool(&mut fake, "mail.messages.search", &envelope),
        Ok(17)
    );
    assert_eq!(
        fake.events,
        [
            "revalidate",
            "begin",
            "scope",
            "policy",
            "message_search",
            "finish_ok"
        ]
    );
    assert_eq!(fake.scoped_accounts, vec![vec!["a"]]);
    assert_eq!(fake.terminals.len(), 1);
    assert_eq!(fake.terminals[0].1.result_count(), Some(3));
    for error in [
        ErrorCode::NotFound,
        ErrorCode::CursorExpired,
        ErrorCode::QuotaExceeded,
        ErrorCode::InternalError,
    ] {
        let mut fake = Fake::ok();
        fake.read = Err(error);
        assert_eq!(
            execute_tool(&mut fake, "mail.messages.search", &envelope),
            Err(error)
        );
        assert_eq!(fake.terminals.len(), 1);
        assert_eq!(fake.terminals[0].1.error(), Some(error));
    }
    for policy in [false, true] {
        let mut fake = Fake::ok();
        let error = if policy {
            fake.fail_policy = Some(ErrorCode::PolicyBlocked);
            ErrorCode::PolicyBlocked
        } else {
            fake.fail_scope = Some(ErrorCode::ScopeDenied);
            ErrorCode::ScopeDenied
        };
        assert_eq!(
            execute_tool(&mut fake, "mail.messages.search", &envelope),
            Err(error)
        );
        assert!(!fake.events.contains(&"message_search"));
        assert_eq!(fake.terminals.len(), 1);
    }
    let mut fake = Fake::ok();
    fake.fail_finish = true;
    assert_eq!(
        execute_tool(&mut fake, "mail.messages.search", &envelope),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(fake.terminals.len(), 1);
    let mut invalid = envelope.clone();
    invalid["params"]["limit"] = json!(0);
    let mut fake = Fake::ok();
    assert_eq!(
        execute_tool(&mut fake, "mail.messages.search", &invalid),
        Err(ErrorCode::InvalidInput)
    );
    assert!(fake.events.is_empty());
}

#[test]
fn sec03_inspection_finishes_the_common_read_audit_without_job_handoff() {
    let envelope = json!({"schema_version":1,"request_id":"r","tool":"mail.sync.page","params":{"action":"inspect","account_id":"a","wire_hex":"494e424f58","encoding":"utf8"}});
    let mut fake = Fake::ok();
    fake.read = ReadResult::new(15, None);
    assert_eq!(execute_tool(&mut fake, "mail.sync.page", &envelope), Ok(15));
    assert_eq!(
        fake.events,
        [
            "revalidate",
            "begin",
            "scope",
            "policy",
            "uid_inspection",
            "finish_ok"
        ]
    );
    assert_eq!(fake.scoped_accounts, vec![vec!["a"]]);
    assert_eq!(fake.terminals.len(), 1);
    assert!(fake.terminals[0].1.is_success());
    assert_eq!(fake.terminals[0].1.result_count(), None);
    for error in [ErrorCode::NotFound, ErrorCode::InternalError] {
        let mut fake = Fake::ok();
        fake.read = Err(error);
        assert_eq!(
            execute_tool(&mut fake, "mail.sync.page", &envelope),
            Err(error)
        );
        assert_eq!(fake.terminals.len(), 1);
        assert_eq!(fake.terminals[0].1.error(), Some(error));
    }
    for policy in [false, true] {
        let mut fake = Fake::ok();
        let expected = if policy {
            fake.fail_policy = Some(ErrorCode::PolicyBlocked);
            ErrorCode::PolicyBlocked
        } else {
            fake.fail_scope = Some(ErrorCode::ScopeDenied);
            ErrorCode::ScopeDenied
        };
        assert_eq!(
            execute_tool(&mut fake, "mail.sync.page", &envelope),
            Err(expected)
        );
        assert!(!fake.events.contains(&"uid_inspection"));
        assert_eq!(fake.terminals.len(), 1);
    }
    let mut fake = Fake::ok();
    fake.fail_finish = true;
    assert_eq!(
        execute_tool(&mut fake, "mail.sync.page", &envelope),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(fake.terminals.len(), 1);
}

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
    folder_read: Result<ReadResult<u32>, ErrorCode>,
    folder_status: Result<ReadResult<u32>, ErrorCode>,
    handoff: Result<AuditHandoff<u32>, ErrorCode>,
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
            folder_read: Ok(ReadResult::new(8, Some(2)).unwrap()),
            folder_status: Ok(ReadResult::new(9, Some(1)).unwrap()),
            handoff: Ok(AuditHandoff::response(10)),
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
    fn folders_list() -> serde_json::Value {
        json!({"schema_version":1,"request_id":"r","tool":"mail.folders.list","params":{"account_id":"a"}})
    }
    fn folders_request() -> serde_json::Value {
        json!({"schema_version":1,"request_id":"r","tool":"mail.folders.refresh","params":{"action":"request","account_id":"a","operation_id":"o","idempotency_key":"i","expected_generation":0,"payload_hash":"00".repeat(32)}})
    }
    fn folders_status() -> serde_json::Value {
        json!({"schema_version":1,"request_id":"r","tool":"mail.folders.refresh","params":{"action":"status","account_id":"a","execution_id":"01".repeat(32)}})
    }
}
impl ExecutionContext for Fake {
    fn request_initial_sync(
        &mut self,
        _: [u8; 32],
        _: &InitialSyncRequest,
    ) -> Result<AuditHandoff<u32>, ErrorCode> {
        self.events.push("initial_handoff");
        std::mem::replace(&mut self.handoff, Err(ErrorCode::InternalError))
    }
    fn read_initial_sync_status(
        &mut self,
        _: &UidPageStatusRequest,
    ) -> Result<ReadResult<u32>, ErrorCode> {
        self.events.push("initial_status");
        ReadResult::new(20, None)
    }
    fn read_initial_sync_plan(
        &mut self,
        _: &InitialSyncPlanRequest,
    ) -> Result<ReadResult<u32>, ErrorCode> {
        self.events.push("initial_plan");
        ReadResult::new(21, None)
    }
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
    fn read_display_settings(&mut self) -> Result<ReadResult<u32>, ErrorCode> {
        self.events.push("app_read");
        std::mem::replace(&mut self.read, Err(ErrorCode::InternalError))
    }
    fn read_diagnostic_snapshot(
        &mut self,
        audit_id: [u8; 32],
        _: &hantong_contracts::diagnostic_snapshot::DiagnosticSnapshotRequest,
    ) -> Result<ReadResult<u32>, ErrorCode> {
        assert_eq!(Some(audit_id), self.audit_id);
        self.read_display_settings()
    }
    fn read_message_search(
        &mut self,
        _: &ImapMetadataSearchRequest,
    ) -> Result<ReadResult<u32>, ErrorCode> {
        self.events.push("message_search");
        std::mem::replace(&mut self.read, Err(ErrorCode::InternalError))
    }
    fn read_folders(
        &mut self,
        _: &hantong_contracts::folders::FolderListRequest,
    ) -> Result<ReadResult<u32>, ErrorCode> {
        self.events.push("folder_read");
        std::mem::replace(&mut self.folder_read, Err(ErrorCode::InternalError))
    }
    fn read_folder_refresh_status(
        &mut self,
        _: &hantong_contracts::folders::FolderRefreshStatusRequest,
    ) -> Result<ReadResult<u32>, ErrorCode> {
        self.events.push("folder_status");
        std::mem::replace(&mut self.folder_status, Err(ErrorCode::InternalError))
    }
    fn request_folder_refresh(
        &mut self,
        _: [u8; 32],
        _: &hantong_contracts::folders::FolderRefreshRequest,
    ) -> Result<AuditHandoff<u32>, ErrorCode> {
        self.events.push("handoff");
        std::mem::replace(&mut self.handoff, Err(ErrorCode::InternalError))
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
    fn read_uid_cursor(&mut self, _: &UidCursorRequest) -> Result<ReadResult<u32>, ErrorCode> {
        self.events.push("uid_cursor");
        ReadResult::new(11, None)
    }
    fn read_uid_inspection(&mut self, _: &UidCursorRequest) -> Result<ReadResult<u32>, ErrorCode> {
        self.events.push("uid_inspection");
        std::mem::replace(&mut self.read, Err(ErrorCode::InternalError))
    }
    fn read_uid_page_status(
        &mut self,
        _: &UidPageStatusRequest,
    ) -> Result<ReadResult<u32>, ErrorCode> {
        self.events.push("uid_status");
        ReadResult::new(12, None)
    }
    fn request_uid_page(
        &mut self,
        _: [u8; 32],
        _: &UidPageRequest,
    ) -> Result<AuditHandoff<u32>, ErrorCode> {
        self.events.push("uid_handoff");
        std::mem::replace(&mut self.handoff, Err(ErrorCode::InternalError))
    }
    fn request_uid_reconcile(
        &mut self,
        _: [u8; 32],
        _: &UidReconcileRequest,
    ) -> Result<AuditHandoff<u32>, ErrorCode> {
        self.events.push("reconcile_handoff");
        std::mem::replace(&mut self.handoff, Err(ErrorCode::InternalError))
    }
    fn read_uid_reconcile_status(
        &mut self,
        _: &UidPageStatusRequest,
    ) -> Result<ReadResult<u32>, ErrorCode> {
        self.events.push("reconcile_status");
        ReadResult::new(13, None)
    }
    fn request_uid_recovery(
        &mut self,
        _id: [u8; 32],
        _request: &UidRecoveryRequest,
    ) -> Result<AuditHandoff<u32>, ErrorCode> {
        self.events.push("recovery_handoff");
        std::mem::replace(&mut self.handoff, Err(ErrorCode::InternalError))
    }
    fn read_uid_recovery_status(
        &mut self,
        _request: &UidPageStatusRequest,
    ) -> Result<ReadResult<u32>, ErrorCode> {
        self.events.push("recovery_status");
        ReadResult::new(14, None)
    }
}

#[test]
fn sync01_sec03_initial_mutations_use_common_grants_and_owned_terminal_handoff() {
    let base = json!({"action":"initial-start","account_id":"a","wire_hex":"494e424f58","encoding":"utf8","operation_id":"op","idempotency_key":"key","intent_hash":"00".repeat(32),"expected_catalog_generation":1,"expected_preview_generation":0,"recent_pages":2});
    let mut resume = base.clone();
    resume["action"] = json!("initial-resume");
    resume
        .as_object_mut()
        .unwrap()
        .remove("expected_preview_generation");
    resume.as_object_mut().unwrap().remove("recent_pages");
    resume["plan_id"] = json!("01".repeat(32));
    resume["expected_plan_version"] = json!(1);
    let mut pause = resume.clone();
    pause["action"] = json!("initial-pause");
    pause
        .as_object_mut()
        .unwrap()
        .remove("expected_catalog_generation");
    for params in [base, resume, pause] {
        let envelope =
            json!({"schema_version":1,"request_id":"r","tool":"mail.sync.page","params":params});
        let mut fake = Fake::ok();
        assert_eq!(execute_tool(&mut fake, "mail.sync.page", &envelope), Ok(10));
        assert_eq!(
            fake.events,
            ["revalidate", "begin", "scope", "policy", "initial_handoff"]
        );
        assert!(fake.terminals.is_empty());
        assert_eq!(fake.scoped_accounts, vec![vec!["a"]]);
        for scope in [true, false] {
            let mut denied = Fake::ok();
            if scope {
                denied.fail_scope = Some(ErrorCode::ScopeDenied)
            } else {
                denied.fail_policy = Some(ErrorCode::PolicyBlocked)
            }
            assert!(execute_tool(&mut denied, "mail.sync.page", &envelope).is_err());
            assert!(!denied.events.contains(&"initial_handoff"));
            assert_eq!(denied.terminals.len(), 1);
        }
        let mut handed = Fake::ok();
        handed.handoff = Ok(AuditHandoff::error(ErrorCode::InternalError));
        assert_eq!(
            execute_tool(&mut handed, "mail.sync.page", &envelope),
            Err(ErrorCode::InternalError)
        );
        assert!(handed.terminals.is_empty());
    }
}

#[test]
fn sync01_initial_status_and_plan_finish_only_the_new_read_audit() {
    for (params, event, result) in [
        (
            json!({"action":"initial-status","account_id":"a","execution_id":"01".repeat(32)}),
            "initial_status",
            20,
        ),
        (
            json!({"action":"initial-plan","account_id":"a","wire_hex":"494e424f58","encoding":"utf8","plan_id":"01".repeat(32)}),
            "initial_plan",
            21,
        ),
    ] {
        let envelope =
            json!({"schema_version":1,"request_id":"r","tool":"mail.sync.page","params":params});
        let mut fake = Fake::ok();
        assert_eq!(
            execute_tool(&mut fake, "mail.sync.page", &envelope),
            Ok(result)
        );
        assert_eq!(
            fake.events,
            ["revalidate", "begin", "scope", "policy", event, "finish_ok"]
        );
        assert_eq!(fake.terminals.len(), 1);
        assert_eq!(fake.terminals[0].1.result_count(), None);
    }
}

#[test]
fn sec03_recovery_uses_common_grants_and_preserves_durable_audit_ownership() {
    for action in ["recover", "recover-status"] {
        let mut envelope = uid_envelope(if action == "recover" {
            "request"
        } else {
            "status"
        });
        envelope["params"]["action"] = json!(action);
        if action == "recover" {
            envelope["params"]["expected_cursor_version"] = json!(4);
        }
        let mut fake = Fake::ok();
        assert_eq!(
            execute_tool(&mut fake, "mail.sync.page", &envelope),
            Ok(if action == "recover" { 10 } else { 14 })
        );
        assert_eq!(
            &fake.events[..4],
            &["revalidate", "begin", "scope", "policy"]
        );
        assert_eq!(fake.scoped_accounts, vec![vec!["a"]]);
        assert_eq!(
            fake.terminals.len(),
            usize::from(action == "recover-status")
        );
        for policy in [true, false] {
            let mut fake = Fake::ok();
            if policy {
                fake.fail_policy = Some(ErrorCode::PolicyBlocked)
            } else {
                fake.fail_scope = Some(ErrorCode::ScopeDenied)
            }
            assert!(execute_tool(&mut fake, "mail.sync.page", &envelope).is_err());
            assert!(!fake.events.contains(&"recovery_handoff"));
            assert!(!fake.events.contains(&"recovery_status"));
            assert_eq!(fake.terminals.len(), 1);
        }
        if action == "recover" {
            let mut fake = Fake::ok();
            fake.handoff = Ok(AuditHandoff::error(ErrorCode::InternalError));
            assert_eq!(
                execute_tool(&mut fake, "mail.sync.page", &envelope),
                Err(ErrorCode::InternalError)
            );
            assert!(fake.terminals.is_empty());
        }
    }
}

#[test]
fn sec03_reconcile_execution_uses_scope_policy_and_distinct_audit_handoff() {
    for action in ["reconcile", "reconcile-status"] {
        let mut value = uid_envelope(if action == "reconcile" {
            "request"
        } else {
            "status"
        });
        value["params"]["action"] = json!(action);
        if action == "reconcile" {
            value["params"]["expected_cursor_version"] = json!(2);
        }
        let mut fake = Fake::ok();
        assert_eq!(
            execute_tool(&mut fake, "mail.sync.page", &value),
            Ok(if action == "reconcile" { 10 } else { 13 })
        );
        assert_eq!(
            &fake.events[..4],
            &["revalidate", "begin", "scope", "policy"]
        );
        assert_eq!(fake.scoped_accounts, vec![vec!["a"]]);
        assert_eq!(
            fake.terminals.len(),
            usize::from(action == "reconcile-status")
        );
        for policy in [false, true] {
            let mut denied = Fake::ok();
            if policy {
                denied.fail_policy = Some(ErrorCode::PolicyBlocked)
            } else {
                denied.fail_scope = Some(ErrorCode::ScopeDenied)
            }
            assert!(execute_tool(&mut denied, "mail.sync.page", &value).is_err());
            assert!(!denied.events.contains(&"reconcile_handoff"));
            assert!(!denied.events.contains(&"reconcile_status"));
            assert_eq!(denied.terminals.len(), 1);
        }
        if action == "reconcile" {
            let mut handed = Fake::ok();
            handed.handoff = Ok(AuditHandoff::error(ErrorCode::InternalError));
            assert_eq!(
                execute_tool(&mut handed, "mail.sync.page", &value),
                Err(ErrorCode::InternalError)
            );
            assert!(handed.terminals.is_empty());
        }
    }
}

#[test]
fn sec03_uncertain_durable_admission_never_finishes_the_audit_again() {
    let mut reconcile = uid_envelope("request");
    reconcile["params"]["action"] = json!("reconcile");
    reconcile["params"]["expected_cursor_version"] = json!(1);
    for (method, envelope) in [
        ("mail.folders.refresh", Fake::folders_request()),
        ("mail.sync.page", uid_envelope("request")),
        ("mail.sync.page", reconcile),
    ] {
        let mut fake = Fake::ok();
        fake.handoff = Ok(AuditHandoff::error(ErrorCode::InternalError));
        fake.fail_finish = true;
        assert_eq!(
            execute_tool(&mut fake, method, &envelope),
            Err(ErrorCode::InternalError)
        );
        assert!(fake.terminals.is_empty());
        assert!(!fake.events.contains(&"finish_error"));
        assert!(!fake.events.contains(&"finish_ok"));
    }
    // Read-only service failures retain the coordinator's audit ownership.
    let mut read = Fake::ok();
    read.read = Err(ErrorCode::InternalError);
    assert_eq!(
        execute_tool(&mut read, "mail.accounts.list", &Fake::accounts()),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(read.terminals.len(), 1);
    assert_eq!(read.events.last(), Some(&"finish_error"));
}

fn uid_envelope(action: &str) -> serde_json::Value {
    let params = match action {
        "cursor" => {
            json!({"action":"cursor","account_id":"a","wire_hex":"494e424f58","encoding":"utf8"})
        }
        "status" => json!({"action":"status","account_id":"a","execution_id":"01".repeat(32)}),
        _ => {
            json!({"action":"request","account_id":"a","wire_hex":"494e424f58","encoding":"utf8","operation_id":"o","idempotency_key":"k","expected_catalog_generation":1,"expected_cursor_version":0,"intent_hash":"00".repeat(32)})
        }
    };
    json!({"schema_version":1,"request_id":"uid","tool":"mail.sync.page","params":params})
}
#[test]
fn ext04_uid_execution_scope_policy_audit_and_handoff_are_shared() {
    for (action, result, event) in [
        ("cursor", 11, "uid_cursor"),
        ("status", 12, "uid_status"),
        ("request", 10, "uid_handoff"),
    ] {
        let mut fake = Fake::ok();
        assert_eq!(
            execute_tool(&mut fake, "mail.sync.page", &uid_envelope(action)),
            Ok(result)
        );
        assert_eq!(
            &fake.events[..5],
            &["revalidate", "begin", "scope", "policy", event]
        );
        assert_eq!(
            fake.terminals.len(),
            if action == "request" { 0 } else { 1 }
        );
        for policy in [false, true] {
            let mut fake = Fake::ok();
            if policy {
                fake.fail_policy = Some(ErrorCode::PolicyBlocked)
            } else {
                fake.fail_scope = Some(ErrorCode::ScopeDenied)
            }
            assert!(execute_tool(&mut fake, "mail.sync.page", &uid_envelope(action)).is_err());
            assert!(!fake.events.contains(&event));
            assert_eq!(fake.terminals.len(), 1);
        }
    }
}
#[test]
fn ext04_uid_admission_owned_error_does_not_finish_a_second_time() {
    let mut fake = Fake::ok();
    fake.handoff = Ok(AuditHandoff::error(ErrorCode::Conflict));
    assert_eq!(
        execute_tool(&mut fake, "mail.sync.page", &uid_envelope("request")),
        Err(ErrorCode::Conflict)
    );
    assert!(fake.terminals.is_empty());
    let mut fake = Fake::ok();
    fake.handoff = Err(ErrorCode::InternalError);
    assert_eq!(
        execute_tool(&mut fake, "mail.sync.page", &uid_envelope("request")),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(fake.terminals.len(), 1);
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

#[test]
fn m203_folder_handoff_owns_audit_and_list_status_finish_once() {
    let mut fake = Fake::ok();
    assert_eq!(
        execute_tool(&mut fake, "mail.folders.refresh", &Fake::folders_request()),
        Ok(10)
    );
    assert_eq!(
        fake.events,
        ["revalidate", "begin", "scope", "policy", "handoff"]
    );
    assert!(fake.terminals.is_empty());
    let mut fake = Fake::ok();
    assert_eq!(
        execute_tool(&mut fake, "mail.folders.list", &Fake::folders_list()),
        Ok(8)
    );
    assert_eq!(fake.terminals.len(), 1);
    let mut fake = Fake::ok();
    assert_eq!(
        execute_tool(&mut fake, "mail.folders.refresh", &Fake::folders_status()),
        Ok(9)
    );
    assert_eq!(fake.terminals.len(), 1);
}

#[test]
fn m203_folder_handoff_error_is_terminal_owned_but_outer_error_finishes() {
    let mut fake = Fake::ok();
    fake.handoff = Ok(AuditHandoff::error(ErrorCode::RateLimited));
    assert_eq!(
        execute_tool(&mut fake, "mail.folders.refresh", &Fake::folders_request()),
        Err(ErrorCode::RateLimited)
    );
    assert!(fake.terminals.is_empty());
    let mut fake = Fake::ok();
    fake.handoff = Err(ErrorCode::InternalError);
    assert_eq!(
        execute_tool(&mut fake, "mail.folders.refresh", &Fake::folders_request()),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(fake.terminals.len(), 1);
}

#[test]
fn m203_folder_scope_and_policy_denials_never_call_folder_service() {
    let mut fake = Fake::ok();
    fake.fail_scope = Some(ErrorCode::ScopeDenied);
    assert_eq!(
        execute_tool(&mut fake, "mail.folders.list", &Fake::folders_list()),
        Err(ErrorCode::ScopeDenied)
    );
    assert!(!fake.events.contains(&"folder_read"));
    let mut fake = Fake::ok();
    fake.fail_policy = Some(ErrorCode::PolicyBlocked);
    assert_eq!(
        execute_tool(&mut fake, "mail.folders.refresh", &Fake::folders_request()),
        Err(ErrorCode::PolicyBlocked)
    );
    assert!(!fake.events.contains(&"handoff"));
}
