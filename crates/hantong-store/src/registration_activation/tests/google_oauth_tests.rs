use super::*;

fn google_account(f: &Fixture) -> String {
    let account = format!("google_{}", "1a".repeat(32));
    f.store().with_connection(|c| {
        c.execute("INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES(?1,'gmail',1,'ready','[]','[]',1)", [&account])?;
        Ok(())
    }).unwrap();
    account
}

#[test]
fn acc02_google_registration_records_honest_source_and_first_fixed_scope() {
    for tools in [
        hantong_contracts::auth::default_cli_body_tools(),
        hantong_contracts::auth::default_cli_send_tools(),
    ] {
        let mut f = Fixture::new();
        let account = google_account(&f);
        let request = f.request(1, &[&account]);
        let id = hantong_platform::registration_id_for_request(request.request_id).unwrap();
        let stage =
            CliRegistrationStage::new_google_oauth(request, id, public_key(), server_hash())
                .unwrap();
        assert_eq!(
            f.store().stage_cli_registration(&stage),
            Err(ErrorCode::InvalidInput)
        );
        let record = f
            .store()
            .stage_google_oauth_cli_registration(&stage)
            .unwrap();
        assert_eq!(record.approved_channel, "google_oauth");
        f.store()
            .activate_cli_registration(id, record.version, server_hash())
            .unwrap();
        f.reopen();
        let target = RequestedScope::new(vec![account.clone()], tools).unwrap();
        let refresh = f
            .store()
            .create_google_oauth_cli_scope_refresh([2; 32], target.clone(), server_hash())
            .unwrap();
        assert_eq!(refresh.decision_channel(), "google_oauth");
        assert_eq!(
            refresh.base_registration().scope.tools(),
            ["mail.accounts.list"]
        );
        assert_eq!(refresh.target_scope(), target);
        assert_eq!(
            f.store()
                .create_google_oauth_cli_scope_refresh([3; 32], target, server_hash()),
            Err(ErrorCode::Conflict)
        );
        let staged = f.store().stage_cli_scope_refresh(&refresh).unwrap();
        f.store().activate_cli_scope_refresh(&staged).unwrap();
        let active = f.store().load_cli_scope_refresh([2; 32]).unwrap().unwrap();
        let registration = f.store().load_default_cli_registration().unwrap().unwrap();
        assert_eq!(registration.scope, active.target_scope());
        assert_eq!(registration.profile_epoch, 2);
        assert_eq!(
            registration.read_only,
            !registration
                .scope
                .tools()
                .iter()
                .any(|tool| tool == "mail.send.execute")
        );
        assert!(
            registration
                .scope
                .tools()
                .iter()
                .all(|tool| !tool.starts_with("mail.google.changes") && !tool.starts_with("mcp"))
        );
        f.reopen();
        assert_eq!(
            f.store().load_cli_scope_refresh([2; 32]).unwrap(),
            Some(active)
        );
        f.store().with_connection(|c| {
            c.execute("UPDATE client_scope_refreshes SET decision_channel='approval_helper' WHERE refresh_id=?1", [[2_u8;32].as_slice()])?;
            Ok(())
        }).unwrap();
        assert_eq!(
            f.store().load_cli_scope_refresh([2; 32]),
            Err(ErrorCode::InternalError)
        );
    }
}

#[test]
fn acc02_google_registration_rejects_other_accounts_tools_and_sources() {
    let f = Fixture::new();
    f.account("a");
    let native = f.request(1, &["a"]);
    assert!(
        CliRegistrationStage::new_google_oauth(native.clone(), rid(1), public_key(), server_hash())
            .is_err()
    );
    let native_stage = f.stage(native, 1);
    assert_eq!(
        f.store().stage_google_oauth_cli_registration(&native_stage),
        Err(ErrorCode::InvalidInput)
    );
    let account = google_account(&f);
    for (number, tools) in [
        (2, hantong_contracts::auth::default_cli_body_tools()),
        (3, hantong_contracts::auth::default_cli_send_tools()),
    ] {
        let mut request = f.request(number, &[&account]);
        request.scope = RequestedScope::new(vec![account.clone()], tools).unwrap();
        assert!(
            CliRegistrationStage::new_google_oauth(
                request,
                rid(number),
                public_key(),
                server_hash()
            )
            .is_err()
        );
    }
}

#[test]
fn acc02_google_send_upgrade_requires_active_v8_and_preserves_provenance() {
    let mut f = Fixture::new();
    let account = google_account(&f);
    let request = f.request(1, &[&account]);
    let id = hantong_platform::registration_id_for_request(request.request_id).unwrap();
    let stage =
        CliRegistrationStage::new_google_oauth(request, id, public_key(), server_hash()).unwrap();
    let record = f
        .store()
        .stage_google_oauth_cli_registration(&stage)
        .unwrap();
    f.store()
        .activate_cli_registration(id, record.version, server_hash())
        .unwrap();
    let body = RequestedScope::new(
        vec![account.clone()],
        hantong_contracts::auth::default_cli_body_tools(),
    )
    .unwrap();
    let first = f
        .store()
        .create_google_oauth_cli_scope_refresh([2; 32], body, server_hash())
        .unwrap();
    let staged = f.store().stage_cli_scope_refresh(&first).unwrap();
    f.store().activate_cli_scope_refresh(&staged).unwrap();
    let send = RequestedScope::new(
        vec![account.clone()],
        hantong_contracts::auth::default_cli_send_tools(),
    )
    .unwrap();
    let upgrade = f
        .store()
        .create_google_oauth_cli_send_upgrade([4; 32], send.clone(), server_hash())
        .unwrap();
    assert_eq!(upgrade.decision_channel(), "google_oauth");
    assert_eq!(upgrade.target_scope(), send);
    assert_eq!(upgrade.target_profile_epoch(), 3);
    let staged = f.store().stage_cli_scope_refresh(&upgrade).unwrap();
    f.store().activate_cli_scope_refresh(&staged).unwrap();
    f.reopen();
    let registration = f.store().load_default_cli_registration().unwrap().unwrap();
    assert_eq!(registration.profile_epoch, 3);
    assert!(!registration.read_only);
    assert_eq!(
        f.store()
            .load_cli_scope_refresh([4; 32])
            .unwrap()
            .unwrap()
            .decision_channel(),
        "google_oauth"
    );
    f.store()
        .with_connection(|c| {
            let version: i64 = c.query_row(
                "SELECT hash_version FROM client_scope_refreshes WHERE refresh_id=?1",
                [[4_u8; 32].as_slice()],
                |r| r.get(0),
            )?;
            assert_eq!(version, 9);
            Ok(())
        })
        .unwrap();
}
