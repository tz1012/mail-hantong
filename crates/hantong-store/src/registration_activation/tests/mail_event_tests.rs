use super::*;

fn v3_request(f: &Fixture, id: u8) -> RegistrationRequest {
    f.store()
        .create_registration_request(
            [id; 32],
            ClientKind::Mcp,
            DEFAULT_MCP_LABEL,
            &RequestedScope::new(
                vec!["a".into()],
                hantong_contracts::auth::mcp_mail_tools_v3(),
            )
            .unwrap(),
        )
        .unwrap()
}

fn mcp_key() -> [u8; 72] {
    let mut key = public_key();
    key[40..].copy_from_slice(
        &wire_hex::decode_fixed::<32>(
            "b01cbd1c01e58065711814b583f061e9d431cca994cea1313449bf97c840ae0a",
        )
        .unwrap(),
    );
    key
}

fn activate(f: &mut Fixture) -> RegistrationRecord {
    f.account("a");
    let stage = McpMailRegistrationStage::new_v3(
        v3_request(f, 121),
        rid(121),
        mcp_key(),
        server_hash(),
        None,
    )
    .unwrap();
    f.store().stage_mcp_v3_registration(&stage).unwrap();
    f.store()
        .activate_mcp_registration(rid(121), 1, server_hash())
        .unwrap()
}

#[test]
fn ai09_receive_events_baseline_change_ack_and_cursor_tamper() {
    let mut f = Fixture::new();
    let active = activate(&mut f);
    let owner = active.registration_id;
    assert_eq!(
        f.store()
            .configure_mail_event_tracking(owner, 1, true)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        f.store()
            .commit_mail_event_observation(owner, 1, "a", "INBOX", "utf8", [1; 32],)
            .unwrap(),
        None
    );
    assert_eq!(
        f.store()
            .commit_mail_event_observation(owner, 1, "a", "INBOX", "utf8", [2; 32],)
            .unwrap(),
        Some(1)
    );
    let page = f
        .store()
        .read_mail_events(owner, 1, None, 10, true)
        .unwrap();
    assert_eq!(page.events.len(), 1);
    let cursor = page.next_cursor.clone().unwrap();
    assert!(
        f.store()
            .read_mail_events(owner, 1, Some(&cursor), 10, true)
            .unwrap()
            .events
            .is_empty()
    );
    assert_eq!(f.store().ack_mail_events(owner, 1, &cursor), Ok(()));
    let mut tampered = cursor.into_bytes();
    tampered[0] = if tampered[0] == b'0' { b'1' } else { b'0' };
    let tampered = String::from_utf8(tampered).unwrap();
    assert_eq!(
        f.store()
            .read_mail_events(owner, 1, Some(&tampered), 10, true),
        Err(ErrorCode::CursorExpired)
    );
}

#[test]
fn ai09_receive_events_revoked_epoch_cannot_read_or_ack() {
    let mut f = Fixture::new();
    let active = activate(&mut f);
    let owner = active.registration_id;
    f.store()
        .configure_mail_event_tracking(owner, 1, true)
        .unwrap();
    f.store().with_connection(|c| {
        c.execute("UPDATE client_registrations SET state='revoked',revoked_at_utc_ms=updated_at_utc_ms,version=version+1 WHERE registration_id=?1", [owner.as_slice()])
    }).unwrap();
    assert_eq!(
        f.store().read_mail_events(owner, 1, None, 10, true),
        Err(ErrorCode::ClientSuspended)
    );
}

fn observation(f: &Fixture, owner: [u8; 16], marker: u8) -> Result<Option<u64>, ErrorCode> {
    f.store()
        .commit_mail_event_observation(owner, 1, "a", "INBOX", "utf8", [marker; 32])
}

#[test]
fn mcp07_receive_events_two_readers_ack_only_processed_event_and_reopen_cursor() {
    let mut f = Fixture::new();
    let owner = activate(&mut f).registration_id;
    f.store()
        .configure_mail_event_tracking(owner, 1, true)
        .unwrap();
    observation(&f, owner, 1).unwrap();
    observation(&f, owner, 2).unwrap();
    observation(&f, owner, 3).unwrap();
    let first = f.store().read_mail_events(owner, 1, None, 1, true).unwrap();
    let first_cursor = first.next_cursor.unwrap();
    let second = f
        .store()
        .read_mail_events(owner, 1, Some(&first_cursor), 1, true)
        .unwrap();
    assert_eq!(second.events[0].event_id, 2);
    let second_cursor = second.next_cursor.unwrap();
    assert_eq!(
        f.store()
            .read_mail_events(owner, 1, Some(&first_cursor), 1, true)
            .unwrap()
            .events[0]
            .event_id,
        2
    );
    f.store().ack_mail_events(owner, 1, &first_cursor).unwrap();
    assert_eq!(
        f.store()
            .read_mail_events(owner, 1, None, 100, true)
            .unwrap()
            .events
            .len(),
        1
    );
    f.reopen();
    f.store().ack_mail_events(owner, 1, &second_cursor).unwrap();
    assert!(
        f.store()
            .read_mail_events(owner, 1, None, 100, true)
            .unwrap()
            .events
            .is_empty()
    );
}

#[test]
fn mcp07_receive_events_rollback_observation_and_recheck_consent() {
    let mut f = Fixture::new();
    let owner = activate(&mut f).registration_id;
    f.store()
        .configure_mail_event_tracking(owner, 1, true)
        .unwrap();
    observation(&f, owner, 1).unwrap();
    f.store().with_connection(|c| c.execute_batch("CREATE TRIGGER fail_receive_event BEFORE INSERT ON mail_event_log BEGIN SELECT RAISE(ABORT,'synthetic'); END;")).unwrap();
    assert_eq!(observation(&f, owner, 2), Err(ErrorCode::InternalError));
    f.store()
        .with_connection(|c| c.execute_batch("DROP TRIGGER fail_receive_event;"))
        .unwrap();
    assert_eq!(observation(&f, owner, 2).unwrap(), Some(1));
    let cursor = f
        .store()
        .read_mail_events(owner, 1, None, 100, true)
        .unwrap()
        .next_cursor
        .unwrap();
    f.store()
        .with_connection(|c| {
            c.execute(
                "DELETE FROM mcp_ai_consents WHERE registration_id=?1",
                [owner.as_slice()],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(observation(&f, owner, 3), Err(ErrorCode::PolicyBlocked));
    assert_eq!(
        f.store().read_mail_events(owner, 1, None, 100, true),
        Err(ErrorCode::PolicyBlocked)
    );
    assert_eq!(
        f.store().ack_mail_events(owner, 1, &cursor),
        Err(ErrorCode::PolicyBlocked)
    );
}

#[test]
fn mcp07_receive_events_reenable_baselines_without_replaying_disabled_interval() {
    let mut f = Fixture::new();
    let owner = activate(&mut f).registration_id;
    f.store()
        .configure_mail_event_tracking(owner, 1, true)
        .unwrap();
    observation(&f, owner, 1).unwrap();
    observation(&f, owner, 2).unwrap();
    f.store()
        .configure_mail_event_tracking(owner, 1, true)
        .unwrap();
    assert_eq!(
        f.store()
            .read_mail_events(owner, 1, None, 100, true)
            .unwrap()
            .events
            .len(),
        1
    );
    f.store()
        .configure_mail_event_tracking(owner, 1, false)
        .unwrap();
    assert_eq!(observation(&f, owner, 3), Err(ErrorCode::ScopeDenied));
    f.store()
        .configure_mail_event_tracking(owner, 1, true)
        .unwrap();
    assert_eq!(observation(&f, owner, 3).unwrap(), None);
    assert!(
        f.store()
            .read_mail_events(owner, 1, None, 100, false)
            .unwrap()
            .events
            .is_empty()
    );
}

#[test]
fn mcp07_receive_events_expired_storage_and_cursor_are_not_disclosed() {
    let mut f = Fixture::new();
    let owner = activate(&mut f).registration_id;
    f.store()
        .configure_mail_event_tracking(owner, 1, true)
        .unwrap();
    observation(&f, owner, 1).unwrap();
    observation(&f, owner, 2).unwrap();
    let cursor = f
        .store()
        .read_mail_events(owner, 1, None, 100, true)
        .unwrap()
        .next_cursor
        .unwrap();
    f.store()
        .with_connection(|c| {
            c.execute("UPDATE mail_event_log SET created_at_utc_ms=1", [])?;
            Ok(())
        })
        .unwrap();
    assert!(
        f.store()
            .read_mail_events(owner, 1, None, 100, true)
            .unwrap()
            .events
            .is_empty()
    );
    assert_eq!(
        f.store().ack_mail_events(owner, 1, &cursor),
        Err(ErrorCode::CursorExpired)
    );
}
