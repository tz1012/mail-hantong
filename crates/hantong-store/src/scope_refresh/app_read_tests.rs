use super::*;

fn app_tools() -> Vec<String> {
    let mut tools = vec![
        "app.diagnostics.snapshot".into(),
        "app.settings.display.get".into(),
    ];
    tools.extend(hantong_contracts::auth::default_cli_metadata_tools());
    tools
}

#[test]
fn cli15_app_read_scope_new_history_uses_v5_domain() {
    let f = Fixture::new();
    let pending = f.create(101, &["b"]);
    assert_eq!(pending.hash_version, 5);
    assert_eq!(pending.payload_hash(), &independent_hash(&pending));
    for version in 1..=4 {
        let mut old = pending.clone();
        old.hash_version = version;
        assert_ne!(pending.payload_hash(), &independent_hash(&old));
    }
}

#[test]
fn cli15_app_read_scope_activation_preserves_metadata_until_each_app_grant_commits() {
    for tool in ["app.diagnostics.snapshot", "app.settings.display.get"] {
        for failure in ["ABORT,'synthetic'", "IGNORE"] {
            let mut f = Fixture::new();
            let metadata = f.create_scope(
                101,
                &["a"],
                hantong_contracts::auth::default_cli_metadata_tools(),
            );
            let metadata = f.store().stage_cli_scope_refresh(&metadata).unwrap();
            f.store().activate_cli_scope_refresh(&metadata).unwrap();
            let pending = f.create_scope(102, &["a"], app_tools());
            assert_eq!(pending.hash_version, 5);
            let staged = f.store().stage_cli_scope_refresh(&pending).unwrap();
            let current = f.store().load_default_cli_registration().unwrap().unwrap();
            assert_eq!(
                current.scope.tools(),
                hantong_contracts::auth::default_cli_metadata_tools()
            );
            assert_eq!(current.state, RegistrationState::Provisioning);
            let before = snapshot(&f);
            sql(
                &f,
                &format!(
                    "CREATE TRIGGER fail_app_tool BEFORE INSERT ON client_registration_tools WHEN NEW.tool_id='{tool}' BEGIN SELECT RAISE({failure}); END;"
                ),
            );
            assert_eq!(
                f.store().activate_cli_scope_refresh(&staged),
                Err(ErrorCode::InternalError)
            );
            assert_eq!(snapshot(&f), before);
            sql(&f, "DROP TRIGGER fail_app_tool");
            f.store().activate_cli_scope_refresh(&staged).unwrap();
            f.reopen();
            let active = f.store().load_default_cli_registration().unwrap().unwrap();
            assert_eq!(active.scope.tools(), app_tools());
            assert_eq!(active.registration_id, f.registration.registration_id);
            assert_eq!(active.request_id, f.registration.request_id);
            assert_eq!(active.public_key, f.registration.public_key);
            assert_eq!(
                f.store().create_cli_scope_refresh_with_scope(
                    [103; 32],
                    active.scope.clone(),
                    pin()
                ),
                Err(ErrorCode::Conflict)
            );
            for tools in [
                hantong_contracts::auth::default_cli_legacy_tools(),
                hantong_contracts::auth::default_cli_folder_tools(),
                hantong_contracts::auth::default_cli_uid_tools(),
                hantong_contracts::auth::default_cli_metadata_tools(),
            ] {
                assert_eq!(
                    f.store().create_cli_scope_refresh_with_scope(
                        [103; 32],
                        RequestedScope::new(vec!["b".into()], tools).unwrap(),
                        pin()
                    ),
                    Err(ErrorCode::Conflict)
                );
            }
        }
    }
}

#[test]
fn sec03_app_read_scope_old_versions_reject_rehashed_app_grants() {
    for version in 1..=4 {
        let f = Fixture::new();
        let mut pending = f.create_scope(104, &["b"], app_tools());
        pending.hash_version = version;
        pending.payload_hash = independent_hash(&pending);
        sql(&f, "PRAGMA ignore_check_constraints=ON");
        f.store().with_connection(|c|c.execute("UPDATE client_scope_refreshes SET hash_version=?1,payload_hash=?2 WHERE refresh_id=?3",params![version,pending.payload_hash.as_slice(),pending.refresh_id.as_slice()])).unwrap();
        assert_eq!(
            f.store().load_cli_scope_refresh(pending.refresh_id),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(
            f.store().load_default_cli_registration(),
            Err(ErrorCode::InternalError)
        );
    }
}

#[test]
fn cli15_app_read_scope_v4_pending_staged_active_preserve_exact_history() {
    for phase in [
        ScopeRefreshPhase::Pending,
        ScopeRefreshPhase::Staged,
        ScopeRefreshPhase::Active,
    ] {
        let mut f = Fixture::new();
        let fresh = f.create_scope(
            105,
            &["b"],
            hantong_contracts::auth::default_cli_metadata_tools(),
        );
        let mut old = set_historical_hash(&f, fresh, 4);
        if phase != ScopeRefreshPhase::Pending {
            old = f.store().stage_cli_scope_refresh(&old).unwrap();
        }
        if phase == ScopeRefreshPhase::Active {
            f.store().activate_cli_scope_refresh(&old).unwrap();
            old = f
                .store()
                .load_cli_scope_refresh([105; 32])
                .unwrap()
                .unwrap();
        }
        let before = snapshot(&f);
        f.store()
            .with_connection(crate::scope_refresh::validate_history_v15)
            .unwrap();
        f.reopen();
        assert_eq!(snapshot(&f), before);
        assert_eq!(
            f.store().load_cli_scope_refresh([105; 32]).unwrap(),
            Some(old.clone())
        );
        if phase == ScopeRefreshPhase::Active {
            let next = f.create_scope(106, &["b"], app_tools());
            assert!(
                f.store()
                    .with_connection(crate::scope_refresh::validate_history_v15)
                    .is_err()
            );
            let staged = f.store().stage_cli_scope_refresh(&next).unwrap();
            f.store().activate_cli_scope_refresh(&staged).unwrap();
            assert_eq!(
                f.store().load_cli_scope_refresh([105; 32]).unwrap(),
                Some(old)
            );
        }
    }
}

#[test]
fn sec03_app_read_scope_rejects_partial_grants_without_creating_history() {
    let f = Fixture::new();
    let before = snapshot(&f);
    let tools = app_tools();
    for invalid in [
        tools[1..].to_vec(),
        vec!["app.settings.display.get".into()],
        vec!["app.diagnostics.snapshot".into()],
    ] {
        assert_eq!(
            f.store().create_cli_scope_refresh_with_scope(
                [107; 32],
                RequestedScope::new(vec!["a".into()], invalid).unwrap(),
                pin()
            ),
            Err(ErrorCode::InvalidInput)
        );
    }
    assert_eq!(snapshot(&f), before);
}
