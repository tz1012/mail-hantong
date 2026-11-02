use super::*;

fn account(c: char) -> String {
    format!("google_{}", c.to_string().repeat(64))
}
fn draft() -> DraftCreateRequest {
    DraftCreateRequest::new(
        &account('a'),
        vec!["receiver@example.invalid".into()],
        "합성 제목".into(),
        "합성 본문".into(),
        "op",
        "idem",
        0,
        [1; 32],
    )
    .unwrap()
}
fn attachment(name: &str, content: &[u8]) -> AttachmentSnapshot {
    AttachmentSnapshot::new(name.into(), "text/plain".into(), content.to_vec()).unwrap()
}

#[test]
fn acc02_selected_mode_never_escalates_from_extra_provider_scopes() {
    let selected = GoogleAccessMode::default();
    let mut granted = selected
        .required_scopes()
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    granted.extend([
        GOOGLE_SEND_SCOPE.into(),
        GOOGLE_MODIFY_SCOPE.into(),
        "https://mail.google.com/".into(),
    ]);
    selected.validate_granted(&granted).unwrap();
    assert_eq!(
        selected.check_operation(GoogleOperation::Send),
        Err(ErrorCode::ScopeDenied)
    );
    assert_eq!(
        selected.check_operation(GoogleOperation::SaveDraft),
        Err(ErrorCode::ScopeDenied)
    );
    assert_eq!(
        selected.check_operation(GoogleOperation::ChangeLabels),
        Err(ErrorCode::ScopeDenied)
    );
    assert_eq!(selected, GoogleAccessMode::ReadOnly);
}

#[test]
fn acc02_partial_or_wrong_grant_does_not_enable_requested_write_mode() {
    for mode in [
        GoogleAccessMode::ReadOnly,
        GoogleAccessMode::ReadSend,
        GoogleAccessMode::Modify,
    ] {
        let all = mode
            .required_scopes()
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        mode.validate_granted(&all).unwrap();
        for index in 0..all.len() {
            let mut partial = all.clone();
            partial.remove(index);
            assert_eq!(mode.validate_granted(&partial), Err(ErrorCode::ScopeDenied));
        }
        assert!(!all.iter().any(|s| s == "https://mail.google.com/"));
        assert_eq!(GoogleAccessMode::parse(mode.as_str()), Ok(mode));
    }
    let grant = [
        "openid",
        "https://www.googleapis.com/auth/userinfo.email",
        GOOGLE_MODIFY_SCOPE,
    ]
    .map(String::from);
    GoogleAccessMode::Modify.validate_granted(&grant).unwrap();
    assert_eq!(
        GoogleAccessMode::ReadSend.validate_granted(&grant),
        Err(ErrorCode::ScopeDenied)
    );
    assert_eq!(
        GoogleAccessMode::ReadSend.check_operation(GoogleOperation::ChangeLabels),
        Err(ErrorCode::ScopeDenied)
    );
    assert!(GoogleAccessMode::parse("all").is_err());
}

#[test]
fn acc02_granted_scope_parser_is_bounded_and_rejects_control_data() {
    let mode = GoogleAccessMode::ReadOnly;
    let base = mode
        .required_scopes()
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    for invalid in ["", "scope\r\nheader", "scope with spaces", &"x".repeat(257)] {
        let mut scopes = base.clone();
        scopes.push(invalid.into());
        assert_eq!(mode.validate_granted(&scopes), Err(ErrorCode::ScopeDenied));
    }
    let mut scopes = base;
    scopes.extend(vec!["extra".into(); 30]);
    assert_eq!(mode.validate_granted(&scopes), Err(ErrorCode::ScopeDenied));
}

#[test]
fn apr01_attachment_content_metadata_order_and_account_are_canonically_bound() {
    let base = ComposeSnapshot::new(
        draft(),
        vec![
            attachment("one.txt", b"first"),
            attachment("two.txt", b"second"),
        ],
    )
    .unwrap();
    let bytes = base.canonical_payload();
    let alternatives = [
        vec![
            attachment("renamed.txt", b"first"),
            attachment("two.txt", b"second"),
        ],
        vec![
            attachment("one.txt", b"changed"),
            attachment("two.txt", b"second"),
        ],
        vec![
            attachment("two.txt", b"second"),
            attachment("one.txt", b"first"),
        ],
        vec![
            AttachmentSnapshot::new(
                "one.txt".into(),
                "application/octet-stream".into(),
                b"first".to_vec(),
            )
            .unwrap(),
            attachment("two.txt", b"second"),
        ],
    ];
    for attachments in alternatives {
        assert_ne!(
            bytes,
            ComposeSnapshot::new(draft(), attachments)
                .unwrap()
                .canonical_payload()
        );
    }
    let other = DraftCreateRequest::new(
        &account('b'),
        draft().recipients().to_vec(),
        draft().subject().into(),
        draft().body().into(),
        "op",
        "idem",
        0,
        [1; 32],
    )
    .unwrap();
    assert_ne!(
        bytes,
        ComposeSnapshot::new(other, base.attachments().to_vec())
            .unwrap()
            .canonical_payload()
    );
    assert_eq!(
        ComposeSnapshot::new(draft(), vec![])
            .unwrap()
            .canonical_payload(),
        draft().canonical_payload()
    );
    assert_eq!(format!("{base:?}"), "ComposeSnapshot(<redacted>)");
    assert_eq!(
        format!("{:?}", base.attachments()[0]),
        "AttachmentSnapshot(<redacted>)"
    );
}

#[test]
fn sec02_attachment_snapshot_has_no_path_header_or_unbounded_input() {
    for name in [
        "../a.txt",
        "a/b.txt",
        "a\\b.txt",
        "C:secret.txt",
        ".hidden",
        "a\r\nBcc.txt",
        "a\".txt",
        "",
        "a..txt",
    ] {
        assert!(AttachmentSnapshot::new(name.into(), "text/plain".into(), vec![]).is_err());
    }
    assert!(
        AttachmentSnapshot::new("a.txt".into(), "text/plain\r\nBcc: x".into(), vec![]).is_err()
    );
    assert!(
        AttachmentSnapshot::new(
            "a.txt".into(),
            "text/plain".into(),
            vec![0; MAX_ATTACHMENT_BYTES + 1]
        )
        .is_err()
    );
    assert!(
        ComposeSnapshot::new(
            draft(),
            vec![attachment("a.txt", b"a"), attachment("A.txt", b"b")]
        )
        .is_err()
    );
    let attachments = (0..9)
        .map(|i| attachment(&format!("{i}.txt"), b"x"))
        .collect();
    assert!(ComposeSnapshot::new(draft(), attachments).is_err());
    assert!(
        ComposeSnapshot::new(
            draft(),
            vec![
                attachment("a.txt", &vec![0; MAX_ATTACHMENT_BYTES]),
                attachment("b.txt", b"x")
            ]
        )
        .is_err()
    );
    let mut source = b"immutable".to_vec();
    let snapshot = attachment("a.txt", &source);
    source.fill(0);
    assert_eq!(snapshot.bytes(), b"immutable");
}

#[test]
fn ai09_label_change_rejects_deletion_routes_and_ambiguous_changes() {
    for label in [
        "TRASH",
        "SPAM",
        "SENT",
        "DRAFT",
        "ALL",
        "Label_",
        "Label_a/../../trash",
        "UNREAD\r\n",
    ] {
        for (add, remove) in [(vec![label.into()], vec![]), (vec![], vec![label.into()])] {
            assert!(MessageLabelChange::new(account('a'), "message1".into(), add, remove).is_err());
        }
    }
    for (add, remove) in [
        (vec![], vec![]),
        (vec!["UNREAD".into(); 2], vec![]),
        (vec!["UNREAD".into()], vec!["UNREAD".into()]),
    ] {
        assert!(MessageLabelChange::new(account('a'), "message1".into(), add, remove).is_err());
    }
    assert!(
        MessageLabelChange::new(
            account('a'),
            "message/trash".into(),
            vec!["UNREAD".into()],
            vec![]
        )
        .is_err()
    );
    assert!(
        MessageLabelChange::new("".into(), "message1".into(), vec!["UNREAD".into()], vec![])
            .is_err()
    );
}

#[test]
fn apr01_label_change_is_single_account_message_bound_and_canonical() {
    let a = MessageLabelChange::new(
        account('a'),
        "message1".into(),
        vec!["STARRED".into(), "Label_123".into()],
        vec!["UNREAD".into()],
    )
    .unwrap();
    let same = MessageLabelChange::new(
        account('a'),
        "message1".into(),
        vec!["Label_123".into(), "STARRED".into()],
        vec!["UNREAD".into()],
    )
    .unwrap();
    assert_eq!(a.canonical_payload(), same.canonical_payload());
    for other in [
        MessageLabelChange::new(
            account('b'),
            "message1".into(),
            a.add().to_vec(),
            a.remove().to_vec(),
        )
        .unwrap(),
        MessageLabelChange::new(
            account('a'),
            "message2".into(),
            a.add().to_vec(),
            a.remove().to_vec(),
        )
        .unwrap(),
    ] {
        assert_ne!(a.canonical_payload(), other.canonical_payload());
    }
    assert_eq!(format!("{a:?}"), "MessageLabelChange(<redacted>)");
}
