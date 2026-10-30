use super::*;
fn full_mail_request(f: &Fixture, id: u8) -> RegistrationRequest {
    f.store()
        .create_registration_request(
            [id; 32],
            ClientKind::Mcp,
            DEFAULT_MCP_LABEL,
            &RequestedScope::new(vec!["a".into()], hantong_contracts::auth::mcp_mail_tools())
                .unwrap(),
        )
        .unwrap()
}
#[test]
fn ai07_mcp_full_mail_requires_distinct_stage_and_atomically_replaces_legacy_identity() {
    let mut f = Fixture::new();
    f.account("a");
    let legacy = mcp_request(&f, 40, &["a"]);
    f.store()
        .stage_mcp_registration(&mcp_stage(legacy))
        .unwrap();
    let old = f
        .store()
        .activate_mcp_registration(rid(40), 1, server_hash())
        .unwrap();
    let new = full_mail_request(&f, 41);
    assert!(matches!(
        McpRegistrationStage::new(new.clone(), rid(41), public_key(), server_hash()),
        Err(ErrorCode::InvalidInput)
    ));
    let stage =
        McpMailRegistrationStage::new(new, rid(41), public_key(), server_hash(), Some(old.clone()))
            .unwrap();
    // Ignored lineage insertion cannot revoke the old identity or approve the request.
    f.store().with_connection(|c|c.execute_batch("CREATE TRIGGER ignore_mcp_lineage BEFORE INSERT ON mcp_registration_replacements BEGIN SELECT RAISE(IGNORE); END;")).unwrap();
    assert_eq!(
        f.store().stage_mcp_mail_registration(&stage),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        f.store().load_mcp_registration().unwrap(),
        Some(old.clone())
    );
    f.store()
        .with_connection(|c| c.execute_batch("DROP TRIGGER ignore_mcp_lineage;"))
        .unwrap();
    let staged = f.store().stage_mcp_mail_registration(&stage).unwrap();
    assert!(!staged.read_only);
    assert_eq!(
        staged.scope.tools(),
        hantong_contracts::auth::mcp_mail_tools()
    );
    assert_eq!(
        f.store()
            .load_registration(old.registration_id)
            .unwrap()
            .unwrap()
            .state,
        RegistrationState::Revoked
    );
    assert_eq!(
        f.store()
            .check_mcp_ai_consent(old.registration_id, 1, &["a".into()]),
        Err(ErrorCode::ClientSuspended)
    );
    assert_eq!(
        f.store()
            .check_mcp_ai_consent(staged.registration_id, 1, &["a".into()]),
        Err(ErrorCode::ClientSuspended)
    );
    assert_eq!(
        f.store()
            .load_mcp_replaced_registration(staged.registration_id)
            .unwrap()
            .unwrap()
            .registration_id,
        old.registration_id
    );
    f.reopen();
    let active = f
        .store()
        .activate_mcp_registration(staged.registration_id, 1, server_hash())
        .unwrap();
    assert!(!active.read_only);
    assert_eq!(
        f.store().load_mcp_registration().unwrap(),
        Some(active.clone())
    );
    assert_eq!(
        f.store()
            .check_mcp_ai_consent(active.registration_id, 1, &["a".into()]),
        Ok(())
    );
    assert!(
        f.store()
            .list_orphan_mcp_registration_requests()
            .unwrap()
            .is_empty()
    );
    assert_eq!(count(f.store(), "client_registrations"), 2);
}
#[test]
fn ai07_mcp_fresh_full_mail_is_not_enabled_until_protected_profile_activation() {
    let f = Fixture::new();
    f.account("a");
    let request = full_mail_request(&f, 42);
    let stage =
        McpMailRegistrationStage::new(request, rid(42), mcp_public_key(), server_hash(), None)
            .unwrap();
    let staged = f.store().stage_mcp_mail_registration(&stage).unwrap();
    assert_eq!(staged.state, RegistrationState::Provisioning);
    assert!(!staged.read_only);
    assert_eq!(
        f.store().check_mcp_ai_consent(rid(42), 1, &["a".into()]),
        Err(ErrorCode::ClientSuspended)
    );
    let active = f
        .store()
        .activate_mcp_registration(rid(42), 1, server_hash())
        .unwrap();
    assert_eq!(
        active.scope.tools(),
        hantong_contracts::auth::mcp_mail_tools()
    );
    assert_eq!(
        f.store().check_mcp_ai_consent(rid(42), 1, &["a".into()]),
        Ok(())
    );
}

fn mcp_scope(accounts: &[&str]) -> RequestedScope {
    RequestedScope::new(
        accounts.iter().map(|account| (*account).into()).collect(),
        hantong_contracts::auth::mcp_read_tools(),
    )
    .unwrap()
}
fn mcp_request(f: &Fixture, id: u8, accounts: &[&str]) -> RegistrationRequest {
    f.store()
        .create_registration_request(
            [id; 32],
            ClientKind::Mcp,
            DEFAULT_MCP_LABEL,
            &mcp_scope(accounts),
        )
        .unwrap()
}
fn mcp_stage(request: RegistrationRequest) -> McpRegistrationStage {
    let id = hantong_platform::registration_id_for_request(request.request_id).unwrap();
    McpRegistrationStage::new(request, id, mcp_public_key(), server_hash()).unwrap()
}
fn mcp_public_key() -> [u8; 72] {
    // Negated public NIST P-256 generator, a distinct valid public point.
    let mut key = public_key();
    key[40..].copy_from_slice(
        &wire_hex::decode_fixed::<32>(
            "b01cbd1c01e58065711814b583f061e9d431cca994cea1313449bf97c840ae0a",
        )
        .unwrap(),
    );
    key
}

#[test]
fn sec03_mcp_registration_fixed_scope_activation_and_reopen() {
    let mut f = Fixture::new();
    f.account("a");
    let request = mcp_request(&f, 21, &["a"]);
    let stage = mcp_stage(request);
    assert_eq!(format!("{stage:?}"), "McpRegistrationStage { <redacted> }");
    let staged = f.store().stage_mcp_registration(&stage).unwrap();
    assert_eq!(staged.client_kind, ClientKind::Mcp);
    assert_eq!(staged.display_label, DEFAULT_MCP_LABEL);
    assert_eq!(staged.scope, mcp_scope(&["a"]));
    assert_eq!(staged.state, RegistrationState::Provisioning);
    assert!(staged.read_only);
    assert_eq!(staged.requests_per_minute, 60);
    assert_eq!(f.store().load_default_cli_registration().unwrap(), None);
    assert_eq!(
        f.store()
            .activate_cli_registration(rid(21), 1, server_hash()),
        Err(ErrorCode::Conflict)
    );
    f.reopen();
    assert_eq!(f.store().load_mcp_registration().unwrap(), Some(staged));
    let active = f
        .store()
        .activate_mcp_registration(rid(21), 1, server_hash())
        .unwrap();
    assert_eq!(active.state, RegistrationState::Active);
    assert_eq!(active.version, 2);
    f.reopen();
    assert_eq!(f.store().load_mcp_registration().unwrap(), Some(active));
    assert!(
        f.store()
            .list_orphan_mcp_registration_requests()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn sec03_mcp_registration_pending_and_denial_are_kind_specific() {
    let f = Fixture::new();
    let cli = f.request(22, &[]);
    let mcp = mcp_request(&f, 23, &[]);
    assert_eq!(
        f.store().load_pending_default_cli_request().unwrap(),
        Some(cli.clone())
    );
    assert_eq!(
        f.store().load_pending_mcp_request().unwrap(),
        Some(mcp.clone())
    );
    assert_eq!(
        f.store()
            .deny_cli_registration_request(mcp.request_id, mcp.version),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store()
            .deny_mcp_registration_request(cli.request_id, cli.version),
        Err(ErrorCode::Conflict)
    );
    assert!(
        CliRegistrationStage::new(mcp.clone(), rid(23), mcp_public_key(), server_hash()).is_err()
    );
    assert!(McpRegistrationStage::new(cli.clone(), rid(22), public_key(), server_hash()).is_err());
    f.store()
        .deny_mcp_registration_request(mcp.request_id, mcp.version)
        .unwrap();
    assert_eq!(f.store().load_pending_mcp_request().unwrap(), None);
    assert_eq!(
        f.store().load_pending_default_cli_request().unwrap(),
        Some(cli)
    );
    let denied = f
        .store()
        .get_registration_request(mcp.request_id)
        .unwrap()
        .unwrap();
    assert_eq!(denied.state, RegistrationRequestState::Denied);
    assert_eq!(
        f.store().list_orphan_mcp_registration_requests().unwrap(),
        vec![denied]
    );
}

#[test]
fn ai07_mcp_disclosure_consent_requires_native_activation_and_exact_account_epoch() {
    let f = Fixture::new();
    f.account("a");
    f.account("b");
    let request = mcp_request(&f, 31, &["a"]);
    let id = rid(31);
    assert_eq!(
        f.store().check_mcp_ai_consent(id, 1, &["a".into()]),
        Err(ErrorCode::ClientNotRegistered)
    );
    let stage = mcp_stage(request);
    f.store().stage_mcp_registration(&stage).unwrap();
    assert_eq!(
        f.store().check_mcp_ai_consent(id, 1, &["a".into()]),
        Err(ErrorCode::ClientSuspended)
    );
    let count: i64 = f
        .store()
        .with_connection(|c| c.query_row("SELECT COUNT(*) FROM mcp_ai_consents", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(count, 0);
    f.store()
        .activate_mcp_registration(id, 1, server_hash())
        .unwrap();
    assert_eq!(f.store().check_mcp_ai_consent(id, 1, &["a".into()]), Ok(()));
    assert_eq!(
        f.store().check_mcp_ai_consent(id, 1, &[]),
        Err(ErrorCode::ScopeDenied)
    );
    assert_eq!(
        f.store().check_mcp_ai_consent(id, 2, &["a".into()]),
        Err(ErrorCode::AuthRequired)
    );
    assert_eq!(
        f.store().check_mcp_ai_consent(id, 1, &["b".into()]),
        Err(ErrorCode::ScopeDenied)
    );
    f.store()
        .with_connection(|c| {
            c.execute("DELETE FROM mcp_ai_consents", [])?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.store().check_mcp_ai_consent(id, 1, &["a".into()]),
        Err(ErrorCode::PolicyBlocked)
    );
    assert_eq!(
        f.store().activate_mcp_registration(id, 2, server_hash()),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store().check_mcp_ai_consent(id, 1, &["a".into()]),
        Err(ErrorCode::PolicyBlocked)
    );
}
#[test]
fn ai07_mcp_consent_failure_rolls_back_activation_and_never_grants_by_stage() {
    let f = Fixture::new();
    f.account("a");
    let request = mcp_request(&f, 32, &["a"]);
    let id = rid(32);
    f.store()
        .stage_mcp_registration(&mcp_stage(request))
        .unwrap();
    f.store().with_connection(|c|c.execute_batch("CREATE TRIGGER deny_consent BEFORE INSERT ON mcp_ai_consents BEGIN SELECT RAISE(IGNORE); END;")).unwrap();
    assert_eq!(
        f.store().activate_mcp_registration(id, 1, server_hash()),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        f.store().load_mcp_registration().unwrap().unwrap().state,
        RegistrationState::Provisioning
    );
    assert_eq!(
        f.store().check_mcp_ai_consent(id, 1, &["a".into()]),
        Err(ErrorCode::ClientSuspended)
    );
    f.store()
        .with_connection(|c| c.execute_batch("DROP TRIGGER deny_consent;"))
        .unwrap();
    f.store()
        .activate_mcp_registration(id, 1, server_hash())
        .unwrap();
    assert_eq!(f.store().check_mcp_ai_consent(id, 1, &["a".into()]), Ok(()));
}
#[test]
fn ai07_mcp_consent_is_revoked_with_registration_and_rejects_corrupt_provenance() {
    let f = Fixture::new();
    f.account("a");
    let request = mcp_request(&f, 33, &["a"]);
    let id = rid(33);
    f.store()
        .stage_mcp_registration(&mcp_stage(request))
        .unwrap();
    f.store()
        .activate_mcp_registration(id, 1, server_hash())
        .unwrap();
    f.store()
        .with_connection(|c| {
            c.execute("UPDATE mcp_ai_consents SET approved_at_utc_ms=1", [])?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.store().check_mcp_ai_consent(id, 1, &["a".into()]),
        Err(ErrorCode::InternalError)
    );
    f.store().with_connection(|c|{c.execute("UPDATE client_registrations SET state='revoked',revoked_at_utc_ms=updated_at_utc_ms,version=version+1 WHERE registration_id=?1",[id.as_slice()])?;Ok(())}).unwrap();
    assert_eq!(
        f.store().check_mcp_ai_consent(id, 1, &["a".into()]),
        Err(ErrorCode::ClientSuspended)
    );
}
