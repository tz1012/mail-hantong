use super::*;
fn parse(mode: Mode, bytes: &[u8]) -> Result<Reply, GateError> {
    let mut gate = UidPageGate::new(mode, "h7")?;
    let mut result = None;
    for byte in bytes {
        result = gate.push(*byte)?;
    }
    gate.finish()?;
    result.ok_or(GateError::Truncated)
}
fn fetch(envelope: &[u8]) -> Vec<u8> {
    let mut s=b"* 999 FETCH (UID 2 FLAGS (\\Seen \\Recent custom) INTERNALDATE \"01-Jan-2026 01:00:00 +0100\" RFC822.SIZE 12 ENVELOPE ".to_vec();
    s.extend(envelope);
    s.extend(b")\r\n");
    s
}
const EMPTY: &[u8] = b"(NIL NIL NIL NIL NIL NIL NIL NIL NIL NIL)";
#[test]
fn ext04_subject_protocol_words_are_data_not_fetch_attributes() {
    let result = parse(
        Mode::Fetch,
        &fetch(b"(NIL \"UID FLAGS ENVELOPE BODYSTRUCTURE\" NIL NIL NIL NIL NIL NIL NIL NIL)"),
    )
    .unwrap();
    let Reply::Message(m) = result else {
        panic!("metadata required")
    };
    assert_eq!(m.uid(), 2);
    assert_eq!(m.flags(), ["\\seen", "custom"]);
    assert_eq!(m.internal_date_utc_ms(), 1767225600000);
    assert_eq!(
        m.envelope().fields().subject.as_deref(),
        Some(b"UID FLAGS ENVELOPE BODYSTRUCTURE".as_slice())
    );
}
#[test]
fn ext04_literals_preserve_bytes_empty_and_group_markers() {
    let frame=fetch(b"(NIL {3}\r\na\xffz NIL NIL NIL ((NIL NIL \"group\" NIL)(NIL NIL NIL NIL)) NIL NIL {0}\r\n NIL)");
    let Reply::Message(m) = parse(Mode::Fetch, &frame).unwrap() else {
        panic!("metadata required")
    };
    assert_eq!(
        m.envelope().fields().subject.as_deref(),
        Some(b"a\xffz".as_slice())
    );
    assert_eq!(
        m.envelope().fields().in_reply_to.as_deref(),
        Some(b"".as_slice())
    );
    assert_eq!(m.envelope().fields().to.as_ref().unwrap().len(), 2);
    assert!(!format!("{m:?}").contains("group"));
}

#[test]
fn ext04_literal_ending_cr_is_payload_not_syntax_prefix() {
    let frame = fetch(b"(NIL {2}\r\na\r NIL NIL NIL NIL NIL NIL NIL NIL)");
    let Reply::Message(message) = parse(Mode::Fetch, &frame).unwrap() else {
        panic!("metadata required")
    };
    assert_eq!(
        message.envelope().fields().subject.as_deref(),
        Some(b"a\r".as_slice())
    );
}
#[test]
fn ext04_literal_cr_cannot_supply_following_syntax_crlf() {
    let mut gate = UidPageGate::new(Mode::Fetch, "h7").unwrap();
    for byte in b"* 1 FETCH (ENVELOPE (NIL {1}\r\n\r" {
        assert_eq!(gate.push(*byte), Ok(None));
    }
    assert_eq!(gate.push(b'\n'), Err(GateError::Invalid));
    assert_eq!(gate.push(b' '), Err(GateError::Invalid));
}

#[test]
fn ext04_examine_whitelist_and_tagged_code_are_typed() {
    for (wire, want) in [
        (
            b"* OK [UIDVALIDITY 3] selected\r\n".as_slice(),
            Reply::Status(Observation::UidValidity(3)),
        ),
        (
            b"* OK [UIDNEXT 4] selected\r\n",
            Reply::Status(Observation::UidNext(4)),
        ),
        (
            b"h7 OK [READ-ONLY] selected\r\n",
            Reply::Tagged(Observation::ReadOnly),
        ),
        (b"* FLAGS (\\Seen)\r\n", Reply::Data),
        (b"* 0 EXISTS\r\n", Reply::Data),
        (b"* 0 RECENT\r\n", Reply::Data),
        (
            b"* OK [PERMANENTFLAGS (\\Seen \\*)] ignored\r\n",
            Reply::Status(Observation::None),
        ),
    ] {
        assert_eq!(parse(Mode::Examine, wire).unwrap(), want)
    }
}
#[test]
fn ext04_missing_duplicate_body_and_nested_payloads_fail_before_codec() {
    let normal = String::from_utf8(fetch(EMPTY)).unwrap();
    for wire in [
        normal.replace("UID 2 ", ""),
        normal.replace("UID 2 ", "UID 2 UID 3 "),
        normal.replace(
            "FLAGS (\\Seen \\Recent custom)",
            "BODY (((((((((((((NIL)))))))))))))",
        ),
        normal.replace("RFC822.SIZE 12", "RFC822 {9999999999}\r\n"),
        normal.replace("ENVELOPE (", "ENVELOPE (("),
        normal.replace("ENVELOPE (NIL", "ENVELOPE ({8193}\r\n"),
        normal.replace("FLAGS (\\Seen \\Recent custom)", "FLAGS ({1}\r\nx)"),
        normal.replace(
            "INTERNALDATE \"01-Jan-2026 01:00:00 +0100\"",
            "INTERNALDATE {1}\r\nx",
        ),
    ] {
        assert!(parse(Mode::Fetch, wire.as_bytes()).is_err());
    }
}
#[test]
fn ext04_wrong_tag_status_or_unsolicited_response_is_rejected() {
    for wire in [
        b"other OK done\r\n".as_slice(),
        b"h7 NO failed\r\n",
        b"* BYE gone\r\n",
        b"* 2 EXPUNGE\r\n",
        b"* SEARCH 1\r\n",
        b"* OK [READ-WRITE] no\r\n",
        b"* OK [UIDVALIDITY 0] no\r\n",
        b"* OK {1}\r\nx\r\n",
    ] {
        assert!(parse(Mode::Examine, wire).is_err());
    }
}
#[test]
fn ext04_truncation_lf_and_appended_second_frame_cannot_be_accepted() {
    let wire = fetch(EMPTY);
    assert_eq!(
        parse(Mode::Fetch, &wire[..wire.len() - 1]),
        Err(GateError::Truncated)
    );
    assert!(
        parse(
            Mode::Fetch,
            &wire
                .iter()
                .copied()
                .filter(|b| *b != b'\r')
                .collect::<Vec<_>>()
        )
        .is_err()
    );
    let mut two = wire;
    two.extend(b"h7 OK done\r\n");
    assert!(parse(Mode::Fetch, &two).is_err());
}
#[test]
fn ext04_frame_and_envelope_address_limits_are_enforced() {
    let mut frame = b"* OK ".to_vec();
    frame.extend(vec![b'x'; MAX_FRAME]);
    frame.extend(b"\r\n");
    assert_eq!(parse(Mode::Examine, &frame), Err(GateError::TooLarge));
    let addresses = format!(
        "(NIL NIL ({}) NIL NIL NIL NIL NIL NIL NIL)",
        "(NIL NIL NIL NIL)".repeat(129)
    );
    assert!(parse(Mode::Fetch, &fetch(addresses.as_bytes())).is_err());
}
#[test]
fn ext04_status_text_punctuation_is_not_parsed_as_structure() {
    assert_eq!(
        parse(Mode::Examine, b"* OK harmless (unbalanced) {{{ text }\r\n").unwrap(),
        Reply::Status(Observation::None)
    );
}
#[test]
fn sec08_gate_and_metadata_debug_never_include_raw_mail() {
    let mut g = UidPageGate::new(Mode::Fetch, "private-tag").unwrap();
    for b in b"* 1 FETCH (UID 2 ENVELOPE (NIL \"secret-subject" {
        g.push(*b).unwrap();
    }
    assert!(!format!("{g:?}").contains("secret"));
    assert!(!format!("{g:?}").contains("private-tag"));
}

#[test]
fn ext04_every_fetch_field_is_required_exactly_once() {
    let frame = String::from_utf8(fetch(EMPTY)).unwrap();
    for field in [
        "UID 2",
        "FLAGS (\\Seen \\Recent custom)",
        "INTERNALDATE \"01-Jan-2026 01:00:00 +0100\"",
        "RFC822.SIZE 12",
        "ENVELOPE (NIL NIL NIL NIL NIL NIL NIL NIL NIL NIL)",
    ] {
        assert!(parse(Mode::Fetch, frame.replace(field, "").as_bytes()).is_err());
        assert!(
            parse(
                Mode::Fetch,
                frame.replace(field, &format!("{field} {field}")).as_bytes()
            )
            .is_err()
        );
    }
}

#[test]
fn ext04_envelope_literal_limit_is_exact_and_invalid_date_is_not_repaired() {
    let mut envelope = b"(NIL {8192}\r\n".to_vec();
    envelope.extend(vec![b'x'; 8192]);
    envelope.extend(b" NIL NIL NIL NIL NIL NIL NIL NIL)");
    let Reply::Message(message) = parse(Mode::Fetch, &fetch(&envelope)).unwrap() else {
        panic!("message required")
    };
    assert_eq!(
        message.envelope().fields().subject.as_ref().unwrap().len(),
        8192
    );
    let valid = String::from_utf8(fetch(EMPTY)).unwrap();
    for date in [
        "31-Feb-2026 01:00:00 +0100",
        "01-Jan-2026 25:00:00 +0100",
        "01-Jan-2026 01:00:00 +2500",
    ] {
        assert!(
            parse(
                Mode::Fetch,
                valid.replace("01-Jan-2026 01:00:00 +0100", date).as_bytes()
            )
            .is_err()
        );
    }
}

#[test]
fn ext04_flags_and_aggregate_envelope_address_limits_precede_codec() {
    let valid = String::from_utf8(fetch(EMPTY)).unwrap();
    assert!(
        parse(
            Mode::Fetch,
            valid
                .replace(
                    "\\Seen \\Recent custom",
                    &(0..65)
                        .map(|n| format!("flag{n}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
                .as_bytes()
        )
        .is_err()
    );
    assert!(
        parse(
            Mode::Fetch,
            valid
                .replace("\\Seen \\Recent custom", &"x".repeat(129))
                .as_bytes()
        )
        .is_err()
    );
    let list = format!("({})", "(NIL NIL NIL NIL)".repeat(128));
    let good = format!("(NIL NIL {list} {list} NIL NIL NIL NIL NIL NIL)");
    assert!(parse(Mode::Fetch, &fetch(good.as_bytes())).is_ok());
    let too_many = format!("(NIL NIL {list} {list} ((NIL NIL NIL NIL)) NIL NIL NIL NIL NIL)");
    assert!(parse(Mode::Fetch, &fetch(too_many.as_bytes())).is_err());
}
