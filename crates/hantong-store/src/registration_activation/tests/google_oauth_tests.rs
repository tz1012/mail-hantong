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
fn sec03_google_manage_pending_rejects_changed_registration_source_without_grant() {
    for channel in ["approval_helper", "terminal_cli"] {
        let f = Fixture::new();
        let account = google_account(&f);
        let request = f.request(1, &[&account]);
        let id = hantong_platform::registration_id_for_request(request.request_id).unwrap();
        let stage =
            CliRegistrationStage::new_google_oauth(request, id, public_key(), server_hash())
                .unwrap();
        let record = f
            .store()
            .stage_google_oauth_cli_registration(&stage)
            .unwrap();
        f.store()
            .activate_cli_registration(id, record.version, server_hash())
            .unwrap();
        let base = f.store().load_default_cli_registration().unwrap().unwrap();
        let target = RequestedScope::new(
            vec![account],
            hantong_contracts::auth::default_cli_manage_tools(),
        )
        .unwrap();
        let pending = f
            .store()
            .create_google_oauth_cli_manage_scope_refresh([27; 32], target, server_hash())
            .unwrap();
        assert_eq!(pending.decision_channel(), "google_oauth");
        f.store()
            .with_connection(|c| {
                c.execute(
                    "UPDATE client_registrations SET approved_channel=?1 WHERE registration_id=?2",
                    params![channel, id.as_slice()],
                )?;
                c.execute(
                    "UPDATE client_registration_requests SET decision_channel=?1 WHERE request_id=?2",
                    params![channel, base.request_id.as_slice()],
                )?;
                // Keep the registration internally consistent so the refresh
                // reader itself must reject the non-OAuth source.
                assert_eq!(crate::registrations::read_registration_bare(c, &id)?.unwrap().approved_channel, channel);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            f.store().load_pending_cli_scope_refresh(),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(
            f.store().stage_cli_scope_refresh(&pending),
            Err(ErrorCode::InternalError)
        );
        f.store()
            .with_connection(|c| {
                let (epoch, version): (i64, i64) = c.query_row(
                    "SELECT profile_epoch,version FROM client_registrations WHERE registration_id=?1",
                    [id.as_slice()], |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                assert_eq!(epoch, base.profile_epoch as i64);
                assert_eq!(version, base.version as i64);
                for (table, column, expected) in [
                    ("client_registration_accounts", "account_id", base.scope.accounts()),
                    ("client_registration_tools", "tool_id", base.scope.tools()),
                ] {
                    let mut statement = c.prepare(&format!(
                        "SELECT {column} FROM {table} WHERE registration_id=?1 ORDER BY {column}"
                    ))?;
                    let actual = statement.query_map([id.as_slice()], |r| r.get::<_, String>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    assert_eq!(actual, expected);
                }
                let phase: String = c.query_row(
                    "SELECT phase FROM client_scope_refreshes WHERE refresh_id=?1",
                    [[27_u8; 32].as_slice()],
                    |r| r.get(0),
                )?;
                assert_eq!(phase, "pending");
                Ok(())
            })
            .unwrap();
    }
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
