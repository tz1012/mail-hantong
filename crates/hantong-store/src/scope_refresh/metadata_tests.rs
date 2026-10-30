use super::*;

fn metadata_tools() -> Vec<String> {
    let mut tools = hantong_contracts::auth::default_cli_uid_tools();
    tools.insert(3, "mail.messages.search".into());
    tools
}

#[test]
fn cli15_metadata_scope_historical_v4_domain_remains_valid() {
    let f = Fixture::new();
    let pending = set_historical_hash(&f, f.create(91, &["b"]), 4);
    assert_eq!(pending.hash_version, 4);
    assert_eq!(pending.payload_hash(), &independent_hash(&pending));
    for version in 1..=3 {
        let mut old = pending.clone();
        old.hash_version = version;
        assert_ne!(pending.payload_hash(), &independent_hash(&old));
    }
}

#[test]
fn cli15_metadata_scope_activation_keeps_uid_grants_until_atomic_replace() {
    let mut f = Fixture::new();
    let uid = f.create_scope(92, &["a"], hantong_contracts::auth::default_cli_uid_tools());
    let uid = f.store().stage_cli_scope_refresh(&uid).unwrap();
    f.store().activate_cli_scope_refresh(&uid).unwrap();
    let pending = set_historical_hash(&f, f.create_scope(93, &["a"], metadata_tools()), 4);
    assert_eq!(pending.hash_version, 4);
    let staged = f.store().stage_cli_scope_refresh(&pending).unwrap();
    let current = f.store().load_default_cli_registration().unwrap().unwrap();
    assert_eq!(
        current.scope.tools(),
        hantong_contracts::auth::default_cli_uid_tools()
    );
    assert_eq!(current.state, RegistrationState::Provisioning);
    let before = snapshot(&f);
    sql(
        &f,
        "CREATE TRIGGER fail_metadata_tool BEFORE INSERT ON client_registration_tools WHEN NEW.tool_id='mail.messages.search' BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
    );
    assert_eq!(
        f.store().activate_cli_scope_refresh(&staged),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(snapshot(&f), before);
    sql(&f, "DROP TRIGGER fail_metadata_tool");
    f.store().activate_cli_scope_refresh(&staged).unwrap();
    f.reopen();
    let current = f.store().load_default_cli_registration().unwrap().unwrap();
    assert_eq!(current.scope.tools(), metadata_tools());
    assert_eq!(current.request_id, f.registration.request_id);
    assert_eq!(current.public_key, f.registration.public_key);
    assert_eq!(current.registration_id, f.registration.registration_id);
    for tools in [
        hantong_contracts::auth::default_cli_legacy_tools(),
        hantong_contracts::auth::default_cli_folder_tools(),
        hantong_contracts::auth::default_cli_uid_tools(),
    ] {
        assert_eq!(
            f.store().create_cli_scope_refresh_with_scope(
                [94; 32],
                RequestedScope::new(vec!["b".into()], tools).unwrap(),
                pin()
            ),
            Err(ErrorCode::Conflict)
        );
    }
    assert_eq!(
        f.store()
            .create_cli_scope_refresh_with_scope([94; 32], current.scope.clone(), pin()),
        Err(ErrorCode::Conflict)
    );
}

#[test]
fn sec03_metadata_scope_old_versions_reject_rehashed_metadata_grants() {
    for version in 1..=3 {
        let f = Fixture::new();
        let mut pending = f.create_scope(95, &["b"], metadata_tools());
        pending.hash_version = version;
        pending.payload_hash = independent_hash(&pending);
        sql(&f, "PRAGMA ignore_check_constraints=ON");
        f.store().with_connection(|c| c.execute("UPDATE client_scope_refreshes SET hash_version=?1,payload_hash=?2 WHERE refresh_id=?3", params![version,pending.payload_hash.as_slice(),pending.refresh_id.as_slice()])).unwrap();
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
fn cli15_metadata_scope_v3_pending_staged_active_history_and_v14_validator_stay_frozen() {
    for phase in [
        ScopeRefreshPhase::Pending,
        ScopeRefreshPhase::Staged,
        ScopeRefreshPhase::Active,
    ] {
        let mut f = Fixture::new();
        let fresh = f.create_scope(96, &["b"], hantong_contracts::auth::default_cli_uid_tools());
        let mut old = set_historical_hash(&f, fresh, 3);
        if phase != ScopeRefreshPhase::Pending {
            old = f.store().stage_cli_scope_refresh(&old).unwrap();
        }
        if phase == ScopeRefreshPhase::Active {
            f.store().activate_cli_scope_refresh(&old).unwrap();
            old = f.store().load_cli_scope_refresh([96; 32]).unwrap().unwrap();
        }
        let before = snapshot(&f);
        f.store()
            .with_connection(crate::scope_refresh::validate_history_v14)
            .unwrap();
        f.reopen();
        assert_eq!(snapshot(&f), before);
        assert_eq!(
            f.store().load_cli_scope_refresh([96; 32]).unwrap(),
            Some(old.clone())
        );
        if phase == ScopeRefreshPhase::Active {
            let next = f.create_scope(97, &["b"], metadata_tools());
            assert!(
                f.store()
                    .with_connection(crate::scope_refresh::validate_history_v14)
                    .is_err()
            );
            let staged = f.store().stage_cli_scope_refresh(&next).unwrap();
            f.store().activate_cli_scope_refresh(&staged).unwrap();
            assert_eq!(
                f.store().load_cli_scope_refresh([96; 32]).unwrap(),
                Some(old)
            );
        }
    }
}

#[test]
fn sec03_metadata_scope_historical_readers_reject_rehashed_downgrades_for_each_version() {
    for version in 2..=4 {
        let f = Fixture::new();
        let old_tools = match version {
            2 => hantong_contracts::auth::default_cli_folder_tools(),
            3 => hantong_contracts::auth::default_cli_uid_tools(),
            _ => metadata_tools(),
        };
        let pending = f.create_scope(98, &["a"], old_tools.clone());
        let pending = set_historical_hash(&f, pending, version);
        let staged = f.store().stage_cli_scope_refresh(&pending).unwrap();
        f.store().activate_cli_scope_refresh(&staged).unwrap();
        let mut next = f.create_scope(99, &["b"], old_tools);
        next.hash_version = version;
        next.target_tools = hantong_contracts::auth::default_cli_legacy_tools();
        next.payload_hash = independent_hash(&next);
        sql(&f, "PRAGMA ignore_check_constraints=ON");
        f.store().with_connection(|c| c.execute("UPDATE client_scope_refreshes SET hash_version=?1,target_tools_json=?2,payload_hash=?3 WHERE refresh_id=?4", params![version,serde_json::to_string(&next.target_tools).unwrap(),next.payload_hash.as_slice(),next.refresh_id.as_slice()])).unwrap();
        assert_eq!(
            f.store().load_cli_scope_refresh(next.refresh_id),
            Err(ErrorCode::InternalError)
        );
    }
}

#[test]
fn sec03_metadata_scope_unreviewed_search_grants_are_rejected_at_initial_and_staged_epochs() {
    for staged in [false, true] {
        let f = Fixture::new();
        if staged {
            let pending = f.create_scope(100, &["b"], metadata_tools());
            f.store().stage_cli_scope_refresh(&pending).unwrap();
        }
        f.store().with_connection(|c| {
            for tool in metadata_tools().iter().skip(1) {
                c.execute("INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,?2)", params![f.registration.registration_id.as_slice(),tool])?;
            }
            Ok(())
        }).unwrap();
        assert_eq!(
            f.store().load_registration(f.registration.registration_id),
            Err(ErrorCode::InternalError)
        );
    }
}
