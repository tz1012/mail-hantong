use super::*;

fn ai_request(
    f: &Fixture,
    id: u8,
    provider: hantong_contracts::registration::AiProvider,
) -> RegistrationRequest {
    let label = match provider {
        hantong_contracts::registration::AiProvider::Codex => DEFAULT_AI_CODEX_LABEL,
        hantong_contracts::registration::AiProvider::Claude => DEFAULT_AI_CLAUDE_LABEL,
    };
    f.store()
        .create_registration_request(
            [id; 32],
            ClientKind::Ai,
            label,
            &RequestedScope::new(vec!["a".into()], hantong_contracts::auth::ai_mail_tools())
                .unwrap(),
        )
        .unwrap()
}
fn ai_public_key(provider: hantong_contracts::registration::AiProvider) -> [u8; 72] {
    if matches!(provider, hantong_contracts::registration::AiProvider::Codex) {
        public_key()
    } else {
        let mut key = public_key();
        key[40..].copy_from_slice(
            &wire_hex::decode_fixed::<32>(
                "b01cbd1c01e58065711814b583f061e9d431cca994cea1313449bf97c840ae0a",
            )
            .unwrap(),
        );
        key
    }
}

#[test]
fn ai09_provider_bound_stage_activation_and_consent_are_separate_from_mcp() {
    let f = Fixture::new();
    f.account("a");
    f.account("b");
    for (provider, id) in [
        (hantong_contracts::registration::AiProvider::Codex, 61),
        (hantong_contracts::registration::AiProvider::Claude, 62),
    ] {
        let request = ai_request(&f, id, provider);
        let stage = AiRegistrationStage::new(
            provider,
            request,
            rid(id),
            ai_public_key(provider),
            server_hash(),
        )
        .unwrap();
        let staged = f.store().stage_ai_registration(&stage).unwrap();
        assert_eq!(staged.client_kind, ClientKind::Ai);
        assert!(!staged.read_only);
        assert_eq!(staged.approved_channel, "ai_feature");
        assert_eq!(
            f.store()
                .check_ai_consent(provider, staged.registration_id, 1, &["a".into()]),
            Err(ErrorCode::PolicyBlocked)
        );
        let active = f
            .store()
            .activate_ai_registration(provider, staged.registration_id, 1, server_hash())
            .unwrap();
        assert!(!active.read_only);
        assert_eq!(active.profile_epoch, 1);
        assert_eq!(
            f.store()
                .check_ai_consent(provider, active.registration_id, 1, &["a".into()]),
            Ok(())
        );
        assert_eq!(
            f.store().check_ai_consent(
                match provider {
                    hantong_contracts::registration::AiProvider::Codex => {
                        hantong_contracts::registration::AiProvider::Claude
                    }
                    hantong_contracts::registration::AiProvider::Claude => {
                        hantong_contracts::registration::AiProvider::Codex
                    }
                },
                active.registration_id,
                1,
                &["a".into()],
            ),
            Err(ErrorCode::PolicyBlocked)
        );
        assert_eq!(
            f.store()
                .check_ai_consent(provider, active.registration_id, 2, &["a".into()]),
            Err(ErrorCode::AuthRequired)
        );
        assert!(matches!(
            f.store()
                .check_ai_consent(provider, active.registration_id, 1, &["b".into()]),
            Err(ErrorCode::ScopeDenied | ErrorCode::PolicyBlocked | ErrorCode::AuthRequired)
        ));
        f.store()
            .with_connection(|c| {
                c.execute(
                    "UPDATE client_registrations SET state='revoked', revoked_at_utc_ms=approved_at_utc_ms WHERE registration_id=?1",
                    [active.registration_id.as_slice()],
                )?;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            f.store()
                .check_ai_consent(provider, active.registration_id, 1, &["a".into()]),
            Err(ErrorCode::PolicyBlocked)
        );
    }
}

#[test]
fn ai09_consent_insert_failure_rolls_back_activation() {
    let f = Fixture::new();
    f.account("a");
    let request = ai_request(&f, 63, hantong_contracts::registration::AiProvider::Codex);
    let stage = AiRegistrationStage::new(
        hantong_contracts::registration::AiProvider::Codex,
        request,
        rid(63),
        public_key(),
        server_hash(),
    )
    .unwrap();
    f.store().stage_ai_registration(&stage).unwrap();
    f.store()
        .with_connection(|c| {
            c.execute_batch("CREATE TRIGGER ignore_ai_consent BEFORE INSERT ON ai_consents BEGIN SELECT RAISE(IGNORE); END;")
        })
        .unwrap();
    assert_eq!(
        f.store().activate_ai_registration(
            hantong_contracts::registration::AiProvider::Codex,
            rid(63),
            1,
            server_hash(),
        ),
        Err(ErrorCode::InternalError)
    );
    f.store()
        .with_connection(|c| {
            c.execute_batch("DROP TRIGGER ignore_ai_consent;")?;
            let state: String = c.query_row(
                "SELECT state FROM client_registrations WHERE registration_id=?1",
                [rid(63).as_slice()],
                |r| r.get(0),
            )?;
            assert_eq!(state, "provisioning");
            Ok(())
        })
        .unwrap();
}

#[test]
fn ai09_terminal_channel_cannot_activate_ai_registration() {
    let f = Fixture::new();
    f.account("a");
    let request = ai_request(&f, 64, hantong_contracts::registration::AiProvider::Codex);
    let stage = AiRegistrationStage::new(
        hantong_contracts::registration::AiProvider::Codex,
        request,
        rid(64),
        public_key(),
        server_hash(),
    )
    .unwrap();
    f.store().stage_ai_registration(&stage).unwrap();
    f.store()
        .with_connection(|c| {
            c.execute(
        "UPDATE client_registrations SET approved_channel='terminal_cli' WHERE registration_id=?1",
        [rid(64).as_slice()],
    )
        })
        .unwrap();
    assert!(matches!(
        f.store().activate_ai_registration(
            hantong_contracts::registration::AiProvider::Codex,
            rid(64),
            1,
            server_hash()
        ),
        Err(ErrorCode::InternalError | ErrorCode::Conflict)
    ));
}
