use super::*;

fn gui_scope(accounts: &[&str]) -> RequestedScope {
    RequestedScope::new(
        accounts.iter().map(|account| (*account).into()).collect(),
        hantong_contracts::auth::gui_read_tools(),
    )
    .unwrap()
}
fn gui_request(f: &Fixture, id: u8, accounts: &[&str]) -> RegistrationRequest {
    f.store()
        .create_registration_request(
            [id; 32],
            ClientKind::Gui,
            DEFAULT_GUI_LABEL,
            &gui_scope(accounts),
        )
        .unwrap()
}
fn gui_stage(request: RegistrationRequest) -> GuiRegistrationStage {
    let id = hantong_platform::registration_id_for_request(request.request_id).unwrap();
    GuiRegistrationStage::new(request, id, gui_public_key(), server_hash()).unwrap()
}
fn gui_public_key() -> [u8; 72] {
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
fn sec03_gui_registration_fixed_scope_activation_and_reopen() {
    let mut f = Fixture::new();
    f.account("a");
    let request = gui_request(&f, 21, &["a"]);
    let stage = gui_stage(request);
    assert_eq!(format!("{stage:?}"), "GuiRegistrationStage { <redacted> }");
    let staged = f.store().stage_gui_registration(&stage).unwrap();
    assert_eq!(staged.client_kind, ClientKind::Gui);
    assert_eq!(staged.display_label, DEFAULT_GUI_LABEL);
    assert_eq!(staged.scope, gui_scope(&["a"]));
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
    assert_eq!(f.store().load_gui_registration().unwrap(), Some(staged));
    let active = f
        .store()
        .activate_gui_registration(rid(21), 1, server_hash())
        .unwrap();
    assert_eq!(active.state, RegistrationState::Active);
    assert_eq!(active.version, 2);
    f.reopen();
    assert_eq!(f.store().load_gui_registration().unwrap(), Some(active));
    assert!(
        f.store()
            .list_orphan_gui_registration_requests()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn sec03_gui_registration_pending_and_denial_are_kind_specific() {
    let f = Fixture::new();
    let cli = f.request(22, &[]);
    let gui = gui_request(&f, 23, &[]);
    assert_eq!(
        f.store().load_pending_default_cli_request().unwrap(),
        Some(cli.clone())
    );
    assert_eq!(
        f.store().load_pending_gui_request().unwrap(),
        Some(gui.clone())
    );
    assert_eq!(
        f.store()
            .deny_cli_registration_request(gui.request_id, gui.version),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store()
            .deny_gui_registration_request(cli.request_id, cli.version),
        Err(ErrorCode::Conflict)
    );
    assert!(
        CliRegistrationStage::new(gui.clone(), rid(23), gui_public_key(), server_hash()).is_err()
    );
    assert!(GuiRegistrationStage::new(cli.clone(), rid(22), public_key(), server_hash()).is_err());
    f.store()
        .deny_gui_registration_request(gui.request_id, gui.version)
        .unwrap();
    assert_eq!(f.store().load_pending_gui_request().unwrap(), None);
    assert_eq!(
        f.store().load_pending_default_cli_request().unwrap(),
        Some(cli)
    );
    let denied = f
        .store()
        .get_registration_request(gui.request_id)
        .unwrap()
        .unwrap();
    assert_eq!(denied.state, RegistrationRequestState::Denied);
    assert_eq!(
        f.store().list_orphan_gui_registration_requests().unwrap(),
        vec![denied]
    );
}

#[test]
fn sec03_gui_registration_rejects_label_scope_and_pin_changes() {
    let f = Fixture::new();
    let request = gui_request(&f, 24, &[]);
    for field in ["label", "scope", "kind"] {
        let mut invalid = request.clone();
        match field {
            "label" => invalid.display_label = DEFAULT_CLI_LABEL.into(),
            "scope" => invalid.scope = scope(&[]),
            _ => invalid.client_kind = ClientKind::Cli,
        }
        assert_eq!(
            GuiRegistrationStage::new(invalid, rid(24), gui_public_key(), server_hash())
                .unwrap_err(),
            ErrorCode::InvalidInput
        );
    }
    let bad_pin =
        GuiRegistrationStage::new(request.clone(), rid(24), gui_public_key(), [7; 32]).unwrap();
    assert_eq!(
        f.store().stage_gui_registration(&bad_pin),
        Err(ErrorCode::Conflict)
    );
    unchanged(&f, &request);
}

#[test]
fn sec03_gui_and_refreshed_cli_preserve_both_linked_keys_and_scope_history() {
    let mut f = Fixture::new();
    f.account("a");
    f.account("b");
    let cli_request = f.request(25, &["a"]);
    f.store()
        .stage_cli_registration(&f.stage(cli_request, 25))
        .unwrap();
    f.store()
        .activate_cli_registration(rid(25), 1, server_hash())
        .unwrap();
    let refresh = f
        .store()
        .create_cli_scope_refresh([26; 32], vec!["a".into(), "b".into()], server_hash())
        .unwrap();
    let refresh = f.store().stage_cli_scope_refresh(&refresh).unwrap();
    let cli = f.store().activate_cli_scope_refresh(&refresh).unwrap();
    assert_eq!(cli.profile_epoch, 2);
    assert_eq!(cli.scope, scope(&["a", "b"]));
    let gui_pending = gui_request(&f, 27, &["a"]);
    f.store()
        .stage_gui_registration(&gui_stage(gui_pending))
        .unwrap();
    let gui = f
        .store()
        .activate_gui_registration(rid(27), 1, server_hash())
        .unwrap();
    f.reopen();
    assert_eq!(
        f.store().load_default_cli_registration().unwrap(),
        Some(cli)
    );
    assert_eq!(f.store().load_gui_registration().unwrap(), Some(gui));
    assert!(
        f.store()
            .list_orphan_cli_registration_requests()
            .unwrap()
            .is_empty()
    );
    assert!(
        f.store()
            .list_orphan_gui_registration_requests()
            .unwrap()
            .is_empty()
    );
    assert!(
        f.store()
            .registration_key_is_linked([25; 32], public_key())
            .unwrap()
    );
    assert!(
        f.store()
            .registration_key_is_linked([27; 32], gui_public_key())
            .unwrap()
    );
    // A pending reservation colliding with either kind's public key is still linked.
    let orphan = gui_request(&f, 28, &[]);
    assert!(
        f.store()
            .registration_key_is_linked(orphan.request_id, public_key())
            .unwrap()
    );
    assert!(
        f.store()
            .registration_key_is_linked(orphan.request_id, gui_public_key())
            .unwrap()
    );
    f.store()
        .with_connection(|c| {
            c.execute("DELETE FROM client_scope_refreshes", [])
                .map(|_| ())
        })
        .unwrap();
    assert_eq!(
        f.store().list_orphan_cli_registration_requests(),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        f.store()
            .registration_key_is_linked(orphan.request_id, gui_public_key()),
        Err(ErrorCode::InternalError)
    );
}

#[test]
fn sec03_gui_registration_grant_failure_rolls_back_all_stage_writes() {
    for failure in ["ABORT,'synthetic'", "IGNORE"] {
        let f = Fixture::new();
        f.account("a");
        let request = gui_request(&f, 29, &["a"]);
        f.store().with_connection(|c| c.execute_batch(&format!(
            "CREATE TRIGGER fail_gui_tool BEFORE INSERT ON client_registration_tools WHEN NEW.tool_id='mail.messages.search' BEGIN SELECT RAISE({failure}); END;"
        ))).unwrap();
        assert_eq!(
            f.store()
                .stage_gui_registration(&gui_stage(request.clone())),
            Err(ErrorCode::InternalError)
        );
        unchanged(&f, &request);
        f.store()
            .with_connection(|c| c.execute_batch("DROP TRIGGER fail_gui_tool"))
            .unwrap();
        f.store()
            .stage_gui_registration(&gui_stage(request))
            .unwrap();
    }
}

#[test]
fn sec03_gui_registration_corrupt_grants_and_reserved_label_fail_closed() {
    for mutation in [
        "DELETE FROM client_registration_tools WHERE tool_id='mail.messages.search'",
        "UPDATE client_registrations SET read_only=0",
        "UPDATE client_registrations SET requests_per_minute=61",
        "UPDATE client_registrations SET profile_epoch=2",
        "UPDATE client_registration_requests SET display_label='forged'; UPDATE client_registrations SET display_label='forged'",
        "UPDATE client_registration_requests SET client_kind='cli'; UPDATE client_registrations SET client_kind='cli'",
    ] {
        let f = Fixture::new();
        let request = gui_request(&f, 30, &[]);
        f.store()
            .stage_gui_registration(&gui_stage(request))
            .unwrap();
        f.store()
            .with_connection(|c| c.execute_batch(mutation))
            .unwrap();
        assert_eq!(
            f.store().load_registration(rid(30)),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(
            f.store().load_gui_registration(),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(
            f.store().list_orphan_gui_registration_requests(),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(
            f.store()
                .registration_key_is_linked([30; 32], gui_public_key()),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(count(f.store(), "client_registrations"), 1);
    }
}

#[test]
fn sec03_gui_duplicate_registration_and_reused_cli_key_are_conflicts() {
    let f = Fixture::new();
    let cli = f.request(31, &[]);
    f.store().stage_cli_registration(&f.stage(cli, 31)).unwrap();
    assert_eq!(
        f.store()
            .activate_gui_registration(rid(31), 1, server_hash()),
        Err(ErrorCode::Conflict)
    );
    let gui = gui_request(&f, 32, &[]);
    let reused =
        GuiRegistrationStage::new(gui.clone(), rid(32), public_key(), server_hash()).unwrap();
    assert_eq!(
        f.store().stage_gui_registration(&reused),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store().get_registration_request(gui.request_id).unwrap(),
        Some(gui.clone())
    );
    f.store().stage_gui_registration(&gui_stage(gui)).unwrap();
    let duplicate = gui_request(&f, 33, &[]);
    assert_eq!(
        f.store()
            .stage_gui_registration(&gui_stage(duplicate.clone())),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store()
            .get_registration_request(duplicate.request_id)
            .unwrap(),
        Some(duplicate)
    );
    assert_eq!(count(f.store(), "client_registrations"), 2);
}
