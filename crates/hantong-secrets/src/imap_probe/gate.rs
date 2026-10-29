//! One-frame, fail-closed IMAP probe parser. No I/O or authentication authority.

use std::fmt;

use io_imap::{
    codec::{
        GreetingCodec, ResponseCodec,
        fragmentizer::{FragmentInfo, Fragmentizer, LineEnding},
    },
    types::response::{
        CommandContinuationRequest, Data, GreetingKind, Response, Status, StatusKind,
    },
};

pub(crate) const MAX_FRAME_BYTES: usize = 16 * 1024;
const MAX_CAPABILITIES: usize = 256;
const MAX_CAPABILITY_BYTES: usize = 128;
const MAX_CONTINUATION_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GateMode {
    Greeting,
    Capability,
    StartTls,
    Authentication,
    Logout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GateError {
    InvalidState,
    InvalidExpectedTag,
    TooLarge,
    InvalidLineEnding,
    LiteralForbidden,
    Malformed,
    UnexpectedResponse,
    WrongTag,
    Preauthenticated,
    CapabilityLimit,
    #[cfg(test)]
    Truncated,
}
impl fmt::Display for GateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidState => "IMAP response gate is closed",
            Self::InvalidExpectedTag => "invalid expected IMAP tag",
            Self::TooLarge => "IMAP response exceeds the limit",
            Self::InvalidLineEnding => "IMAP response requires CRLF",
            Self::LiteralForbidden => "IMAP probe does not accept literals",
            Self::Malformed => "malformed IMAP probe response",
            Self::UnexpectedResponse => "unexpected IMAP probe response",
            Self::WrongTag => "IMAP response tag mismatch",
            Self::Preauthenticated => "IMAP probe requires explicit authentication",
            Self::CapabilityLimit => "IMAP capability limit exceeded",
            #[cfg(test)]
            Self::Truncated => "incomplete IMAP response",
        })
    }
}
impl std::error::Error for GateError {}

#[derive(Eq, PartialEq)]
pub(crate) enum ReplyKind {
    GreetingOk,
    Capability(Vec<String>),
    Tagged(StatusKind),
    Continuation { empty: bool },
    Bye,
    UntaggedStatus(StatusKind),
}
impl fmt::Debug for ReplyKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GreetingOk => f.write_str("GreetingOk"),
            Self::Capability(caps) => f.debug_tuple("Capability").field(&caps.len()).finish(),
            Self::Tagged(kind) => f.debug_tuple("Tagged").field(kind).finish(),
            Self::Continuation { empty } => f
                .debug_struct("Continuation")
                .field("empty", empty)
                .finish(),
            Self::Bye => f.write_str("Bye"),
            Self::UntaggedStatus(kind) => f.debug_tuple("UntaggedStatus").field(kind).finish(),
        }
    }
}
pub(crate) struct ValidatedFrame {
    kind: ReplyKind,
}
impl ValidatedFrame {
    #[cfg(test)]
    pub(crate) fn kind(&self) -> &ReplyKind {
        &self.kind
    }
    pub(crate) fn into_kind(self) -> ReplyKind {
        self.kind
    }
}
impl fmt::Debug for ValidatedFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValidatedFrame")
            .field("kind", &self.kind)
            .finish()
    }
}
#[derive(Debug)]
pub(crate) struct FeedResult {
    pub(crate) consumed: usize,
    pub(crate) frame: Option<ValidatedFrame>,
}
pub(crate) struct StrictResponseGate {
    mode: GateMode,
    expected_tag: Option<String>,
    fragmentizer: Fragmentizer,
    closed: bool,
}
impl fmt::Debug for StrictResponseGate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StrictResponseGate")
            .field("mode", &self.mode)
            .field("closed", &self.closed)
            .finish()
    }
}
impl StrictResponseGate {
    pub(crate) fn new(mode: GateMode, expected_tag: Option<&str>) -> Result<Self, GateError> {
        let valid_tag = expected_tag.is_some_and(|tag| {
            !tag.is_empty()
                && tag.len() <= 64
                && tag
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        });
        if (mode == GateMode::Greeting && expected_tag.is_some())
            || (mode != GateMode::Greeting && !valid_tag)
        {
            return Err(GateError::InvalidExpectedTag);
        }
        Ok(Self {
            mode,
            expected_tag: expected_tag.map(str::to_owned),
            fragmentizer: Fragmentizer::new(MAX_FRAME_BYTES as u32),
            closed: false,
        })
    }
    /// Consumes at most one frame. Unconsumed input stays with the caller.
    /// Empty input is not EOF. The driver must reject a closed underlying reader.
    pub(crate) fn feed(&mut self, input: &[u8]) -> Result<FeedResult, GateError> {
        if self.closed {
            return Err(GateError::InvalidState);
        }
        for (index, &byte) in input.iter().enumerate() {
            // Do not enqueue an unbounded chunk: Fragmentizer's enqueue buffer
            // itself has no size limit. At most one unparsed byte exists here.
            if self.fragmentizer.message_bytes().len() == MAX_FRAME_BYTES {
                return self.fail(GateError::TooLarge);
            }
            let buffered = self.fragmentizer.message_bytes();
            if buffered.starts_with(b"+ ")
                && buffered.len() >= MAX_CONTINUATION_BYTES + 2
                && byte != b'\r'
                && byte != b'\n'
            {
                return self.fail(GateError::TooLarge);
            }
            if byte == 0x7f || (byte < b' ' && byte != b'\r' && byte != b'\n') {
                return self.fail(GateError::Malformed);
            }
            if self.fragmentizer.message_bytes().last() == Some(&b'\r') && byte != b'\n' {
                return self.fail(GateError::InvalidLineEnding);
            }
            self.fragmentizer.enqueue_bytes(&[byte]);
            let fragment = self.fragmentizer.progress();
            if self.fragmentizer.is_max_message_size_exceeded() {
                return self.fail(GateError::TooLarge);
            }
            if let Some(fragment) = fragment {
                match fragment {
                    FragmentInfo::Line {
                        ending: LineEnding::Lf,
                        ..
                    } => return self.fail(GateError::InvalidLineEnding),
                    FragmentInfo::Line {
                        announcement: Some(_),
                        ..
                    }
                    | FragmentInfo::Literal { .. } => {
                        return self.fail(GateError::LiteralForbidden);
                    }
                    FragmentInfo::Line { .. } => {}
                }
                // Oversized decimal announcements can exceed the codec's u32
                // literal parser. Refuse their syntactic marker as well.
                if has_literal_marker(self.fragmentizer.message_bytes()) {
                    return self.fail(GateError::LiteralForbidden);
                }
                let frame = match self.validate_frame() {
                    Ok(frame) => frame,
                    Err(error) => return self.fail(error),
                };
                self.closed = true;
                return Ok(FeedResult {
                    consumed: index + 1,
                    frame: Some(frame),
                });
            }
        }
        Ok(FeedResult {
            consumed: input.len(),
            frame: None,
        })
    }
    #[cfg(test)]
    fn eof(&mut self) -> Result<(), GateError> {
        if self.closed {
            return Err(GateError::InvalidState);
        }
        self.fail(GateError::Truncated)
    }
    fn fail<T>(&mut self, error: GateError) -> Result<T, GateError> {
        self.closed = true;
        Err(error)
    }
    fn validate_frame(&self) -> Result<ValidatedFrame, GateError> {
        let bytes = self.fragmentizer.message_bytes();
        let content = bytes
            .strip_suffix(b"\r\n")
            .ok_or(GateError::InvalidLineEnding)?;
        std::str::from_utf8(content).map_err(|_| GateError::Malformed)?;
        // Restrict the grammar BEFORE typed decode: a probe never needs FETCH,
        // BODYSTRUCTURE, STATUS or other potentially recursive data responses.
        let shape = self.precheck(content)?;
        let kind = if self.mode == GateMode::Greeting {
            let greeting = self
                .fragmentizer
                .decode_message(&GreetingCodec::new())
                .map_err(|_| GateError::Malformed)?;
            match greeting.kind {
                GreetingKind::Ok => ReplyKind::GreetingOk,
                GreetingKind::PreAuth => return Err(GateError::Preauthenticated),
                GreetingKind::Bye => return Err(GateError::UnexpectedResponse),
            }
        } else {
            let response = self
                .fragmentizer
                .decode_message(&ResponseCodec::new())
                .map_err(|_| GateError::Malformed)?;
            match (shape, response) {
                (Shape::Capability(atoms), Response::Data(Data::Capability(_))) => {
                    ReplyKind::Capability(
                        atoms
                            .split(|&b| b == b' ')
                            .filter(|s| !s.is_empty())
                            .map(|atom| {
                                // precheck_caps established ASCII and each allocation's bound.
                                std::str::from_utf8(atom)
                                    .expect("checked capability ASCII")
                                    .to_ascii_uppercase()
                            })
                            .collect(),
                    )
                }
                (Shape::Tagged, Response::Status(Status::Tagged(tagged))) => {
                    if Some(tagged.tag.as_ref()) != self.expected_tag.as_deref() {
                        return Err(GateError::WrongTag);
                    }
                    ReplyKind::Tagged(tagged.body.kind)
                }
                (Shape::Untagged, Response::Status(Status::Untagged(body))) => {
                    ReplyKind::UntaggedStatus(body.kind)
                }
                (Shape::Bye, Response::Status(Status::Bye(_))) => ReplyKind::Bye,
                (
                    Shape::Continuation { empty },
                    Response::CommandContinuationRequest(CommandContinuationRequest::Base64(_)),
                ) => ReplyKind::Continuation { empty },
                (Shape::Continuation { .. }, _) => return Err(GateError::Malformed),
                _ => return Err(GateError::UnexpectedResponse),
            }
        };
        Ok(ValidatedFrame { kind })
    }
    fn precheck<'a>(&self, content: &'a [u8]) -> Result<Shape<'a>, GateError> {
        if let Some(rest) = content.strip_prefix(b"+ ") {
            if self.mode != GateMode::Authentication {
                return Err(GateError::UnexpectedResponse);
            }
            if rest.len() > MAX_CONTINUATION_BYTES {
                return Err(GateError::TooLarge);
            }
            // Authentication challenges use SASL base64, not free-form status text.
            // The codec validates encoding; only the actual empty wire challenge
            // is classified empty so the driver can require it for PLAIN/XOAUTH2.
            return Ok(Shape::Continuation {
                empty: rest.is_empty(),
            });
        }
        let (prefix, rest) = split_word(content).ok_or(GateError::Malformed)?;
        if prefix == b"*" {
            let (word, body) = split_word(rest).ok_or(GateError::Malformed)?;
            if self.mode == GateMode::Greeting {
                if word.eq_ignore_ascii_case(b"PREAUTH") {
                    return Err(GateError::Preauthenticated);
                }
                if !word.eq_ignore_ascii_case(b"OK") {
                    return Err(GateError::UnexpectedResponse);
                }
                precheck_status_code(body)?;
                return Ok(Shape::Greeting);
            }
            if word.eq_ignore_ascii_case(b"CAPABILITY") {
                if !matches!(self.mode, GateMode::Capability | GateMode::Authentication) {
                    return Err(GateError::UnexpectedResponse);
                }
                precheck_caps(body)?;
                return Ok(Shape::Capability(body));
            }
            if word.eq_ignore_ascii_case(b"BYE") {
                if self.mode != GateMode::Logout {
                    return Err(GateError::UnexpectedResponse);
                }
                precheck_status_code(body)?;
                return Ok(Shape::Bye);
            }
            if self.mode != GateMode::StartTls && is_status_word(word) {
                precheck_status_code(body)?;
                return Ok(Shape::Untagged);
            }
            return Err(GateError::UnexpectedResponse);
        }
        if self.mode == GateMode::Greeting {
            return Err(GateError::UnexpectedResponse);
        }
        if Some(prefix) != self.expected_tag.as_deref().map(str::as_bytes) {
            return Err(GateError::WrongTag);
        }
        let (word, body) = split_word(rest).ok_or(GateError::Malformed)?;
        if !is_status_word(word) {
            return Err(GateError::UnexpectedResponse);
        }
        precheck_status_code(body)?;
        Ok(Shape::Tagged)
    }
}

enum Shape<'a> {
    Greeting,
    Capability(&'a [u8]),
    Tagged,
    Untagged,
    Continuation { empty: bool },
    Bye,
}
fn split_word(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let position = bytes.iter().position(|&b| b == b' ')?;
    Some((&bytes[..position], &bytes[position + 1..]))
}
fn is_status_word(word: &[u8]) -> bool {
    word.eq_ignore_ascii_case(b"OK")
        || word.eq_ignore_ascii_case(b"NO")
        || word.eq_ignore_ascii_case(b"BAD")
}
fn has_literal_marker(frame: &[u8]) -> bool {
    let Some(body) = frame
        .strip_suffix(b"\r\n")
        .and_then(|b| b.strip_suffix(b"}"))
    else {
        return false;
    };
    let Some(start) = body.iter().rposition(|&b| b == b'{') else {
        return false;
    };
    let length = &body[start + 1..];
    let length = length.strip_suffix(b"+").unwrap_or(length);
    !length.is_empty() && length.iter().all(u8::is_ascii_digit)
}
fn precheck_status_code(body: &[u8]) -> Result<(), GateError> {
    let Some(code) = body.strip_prefix(b"[") else {
        return Ok(());
    };
    let end = code
        .iter()
        .position(|&b| b == b']')
        .ok_or(GateError::Malformed)?;
    let code = &code[..end];
    let (name, args) = split_word(code).unwrap_or((code, &[]));
    if name.eq_ignore_ascii_case(b"CAPABILITY") {
        precheck_caps(args)?;
    }
    Ok(())
}
fn precheck_caps(bytes: &[u8]) -> Result<(), GateError> {
    let mut count = 0;
    let mut total = 0;
    for atom in bytes.split(|&b| b == b' ').filter(|s| !s.is_empty()) {
        count += 1;
        total += atom.len();
        if count > MAX_CAPABILITIES || atom.len() > MAX_CAPABILITY_BYTES || total > MAX_FRAME_BYTES
        {
            return Err(GateError::CapabilityLimit);
        }
        if !atom
            .iter()
            .all(|&b| (0x21..=0x7e).contains(&b) && !b"(){%*\"\\]".contains(&b))
        {
            return Err(GateError::Malformed);
        }
    }
    if count == 0 {
        return Err(GateError::Malformed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(mode: GateMode, bytes: &[u8]) -> Result<ValidatedFrame, GateError> {
        let mut gate = StrictResponseGate::new(
            mode,
            if mode == GateMode::Greeting {
                None
            } else {
                Some("a1")
            },
        )?;
        gate.feed(bytes)?.frame.ok_or(GateError::Truncated)
    }
    #[test]
    fn ext03_imap_partial_crlf_and_exact_one_frame_consumption() {
        let mut gate = StrictResponseGate::new(GateMode::StartTls, Some("a1")).unwrap();
        let first = gate.feed(b"a1 OK ready\r").unwrap();
        assert_eq!(first.consumed, 12);
        assert!(first.frame.is_none());
        let second = gate.feed(b"\n* OK forged\r\n").unwrap();
        assert_eq!(second.consumed, 1);
        let frame = second.frame.unwrap();
        assert_eq!(frame.kind(), &ReplyKind::Tagged(StatusKind::Ok));
        assert!(matches!(
            gate.feed(b"a1 OK again\r\n"),
            Err(GateError::InvalidState)
        ));
    }
    #[test]
    fn ext03_imap_wrong_embedded_and_case_changed_tags_are_rejected() {
        for bytes in [
            b"b1 OK ready\r\n".as_slice(),
            b"A1 OK ready\r\n",
            b"garbage a1 OK ready\r\n",
        ] {
            assert_eq!(
                parse(GateMode::StartTls, bytes).unwrap_err(),
                GateError::WrongTag
            );
        }
    }
    #[test]
    fn ext03_imap_starttls_no_and_bad_remain_distinct_from_ok() {
        assert_eq!(
            parse(GateMode::StartTls, b"a1 NO unavailable\r\n")
                .unwrap()
                .kind(),
            &ReplyKind::Tagged(StatusKind::No)
        );
        assert_eq!(
            parse(GateMode::StartTls, b"a1 BAD unsupported\r\n")
                .unwrap()
                .kind(),
            &ReplyKind::Tagged(StatusKind::Bad)
        );
        assert_eq!(
            parse(GateMode::StartTls, b"a1 oK ready\r\n")
                .unwrap()
                .kind(),
            &ReplyKind::Tagged(StatusKind::Ok)
        );
    }
    #[test]
    fn ext03_imap_line_endings_controls_and_invalid_utf8_fail_closed() {
        for bytes in [
            b"a1 OK ready\n".as_slice(),
            b"a1 OK x\rgarbage\r\n",
            b"a1 OK \0x\r\n",
            b"a1 OK \x7fx\r\n",
            b"a1 OK \xff\r\n",
        ] {
            assert!(parse(GateMode::StartTls, bytes).is_err());
        }
        let mut gate = StrictResponseGate::new(GateMode::StartTls, Some("a1")).unwrap();
        assert!(gate.feed(b"a1 OK bad\n").is_err());
        assert!(matches!(
            gate.feed(b"a1 OK good\r\n"),
            Err(GateError::InvalidState)
        ));
    }
    #[test]
    fn ext03_imap_exact_frame_limit_and_unterminated_limit_are_bounded() {
        let mut exact = b"a1 OK ".to_vec();
        exact.resize(MAX_FRAME_BYTES - 2, b'x');
        exact.extend_from_slice(b"\r\n");
        let mut gate = StrictResponseGate::new(GateMode::StartTls, Some("a1")).unwrap();
        let result = gate.feed(&exact).unwrap();
        assert_eq!(result.consumed, MAX_FRAME_BYTES);
        assert_eq!(
            result.frame.unwrap().into_kind(),
            ReplyKind::Tagged(StatusKind::Ok)
        );
        let mut oversized = b"a1 OK ".to_vec();
        oversized.resize(MAX_FRAME_BYTES + 50, b'x');
        let mut gate = StrictResponseGate::new(GateMode::StartTls, Some("a1")).unwrap();
        assert!(matches!(gate.feed(&oversized), Err(GateError::TooLarge)));
        assert!(gate.fragmentizer.message_bytes().len() <= MAX_FRAME_BYTES);
        assert!(matches!(
            gate.feed(b"\r\na1 OK recovered\r\n"),
            Err(GateError::InvalidState)
        ));
    }
    #[test]
    fn ext03_imap_literal_announcements_rejected_before_body_bytes() {
        for bytes in [
            b"* OK {999999999}\r\nSECRET BODY".as_slice(),
            b"* OK {12+}\r\nSECRET BODY",
            b"* OK {18446744073709551615}\r\nSECRET BODY",
            b"* OK ~{12}\r\nSECRET BODY",
        ] {
            let mut gate = StrictResponseGate::new(GateMode::Capability, Some("a1")).unwrap();
            assert_eq!(gate.feed(bytes).unwrap_err(), GateError::LiteralForbidden);
            assert!(
                !gate
                    .fragmentizer
                    .message_bytes()
                    .windows(6)
                    .any(|w| w == b"SECRET")
            );
        }
    }
    #[test]
    fn ext03_imap_greeting_preauth_and_bye_cannot_skip_authentication() {
        assert_eq!(
            parse(
                GateMode::Greeting,
                b"* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] ready\r\n"
            )
            .unwrap()
            .kind(),
            &ReplyKind::GreetingOk
        );
        assert_eq!(
            parse(GateMode::Greeting, b"* PREAUTH ready\r\n").unwrap_err(),
            GateError::Preauthenticated
        );
        assert!(parse(GateMode::Greeting, b"* BYE unavailable\r\n").is_err());
        assert!(parse(GateMode::Greeting, b"a1 OK ready\r\n").is_err());
    }
    #[test]
    fn ext05_imap_capability_is_bounded_normalized_and_mode_limited() {
        let frame = parse(
            GateMode::Capability,
            b"* CAPABILITY IMAP4rev1 auth=plain IDLE x-new\r\n",
        )
        .unwrap();
        assert_eq!(
            frame.into_kind(),
            ReplyKind::Capability(vec![
                "IMAP4REV1".into(),
                "AUTH=PLAIN".into(),
                "IDLE".into(),
                "X-NEW".into()
            ])
        );
        assert_eq!(
            parse(GateMode::StartTls, b"* CAPABILITY IMAP4rev1\r\n").unwrap_err(),
            GateError::UnexpectedResponse
        );
    }
    #[test]
    fn ext05_imap_capability_count_and_atom_size_checked_before_decode() {
        let cap_line = |count: usize| {
            format!(
                "* CAPABILITY IMAP4rev1 {}\r\n",
                (1..count)
                    .map(|n| format!("X{n}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        assert!(parse(GateMode::Capability, cap_line(MAX_CAPABILITIES).as_bytes()).is_ok());
        assert_eq!(
            parse(
                GateMode::Capability,
                cap_line(MAX_CAPABILITIES + 1).as_bytes()
            )
            .unwrap_err(),
            GateError::CapabilityLimit
        );
        let line = format!(
            "* CAPABILITY IMAP4rev1 {}\r\n",
            "x".repeat(MAX_CAPABILITY_BYTES)
        );
        assert!(parse(GateMode::Capability, line.as_bytes()).is_ok());
        let line = format!(
            "* CAPABILITY IMAP4rev1 {}\r\n",
            "x".repeat(MAX_CAPABILITY_BYTES + 1)
        );
        assert_eq!(
            parse(GateMode::Capability, line.as_bytes()).unwrap_err(),
            GateError::CapabilityLimit
        );
        let line = format!(
            "* OK [CAPABILITY {}] ready\r\n",
            "X ".repeat(MAX_CAPABILITIES + 1)
        );
        assert_eq!(
            parse(GateMode::Greeting, line.as_bytes()).unwrap_err(),
            GateError::CapabilityLimit
        );
    }
    #[test]
    fn ext03_imap_forbidden_recursive_data_never_reaches_codec() {
        let nested = format!("* 1 FETCH (BODYSTRUCTURE {}\r\n", "(".repeat(6000));
        assert_eq!(
            parse(GateMode::Capability, nested.as_bytes()).unwrap_err(),
            GateError::UnexpectedResponse
        );
        assert_eq!(
            parse(GateMode::Capability, b"* STATUS INBOX (MESSAGES 1)\r\n").unwrap_err(),
            GateError::UnexpectedResponse
        );
    }
    #[test]
    fn ext03_imap_continuation_and_bye_are_stage_specific() {
        assert_eq!(
            parse(GateMode::Authentication, b"+ VXNlcm5hbWU6\r\n")
                .unwrap()
                .kind(),
            &ReplyKind::Continuation { empty: false }
        );
        assert_eq!(
            parse(GateMode::Authentication, b"+ \r\n").unwrap().kind(),
            &ReplyKind::Continuation { empty: true }
        );
        assert!(parse(GateMode::StartTls, b"+ challenge\r\n").is_err());
        assert_eq!(
            parse(GateMode::Logout, b"* BYE closing\r\n")
                .unwrap()
                .kind(),
            &ReplyKind::Bye
        );
        assert!(parse(GateMode::Authentication, b"* BYE closed\r\n").is_err());
        assert_eq!(
            parse(GateMode::Capability, b"* OK [ALERT] info\r\n")
                .unwrap()
                .kind(),
            &ReplyKind::UntaggedStatus(StatusKind::Ok)
        );
    }
    #[test]
    fn ext03_imap_sasl_challenge_limit_is_checked_before_decode() {
        let exact = format!("+ {}\r\n", "YWFh".repeat(128));
        assert_eq!(
            parse(GateMode::Authentication, exact.as_bytes())
                .unwrap()
                .into_kind(),
            ReplyKind::Continuation { empty: false }
        );
        let mut gate = StrictResponseGate::new(GateMode::Authentication, Some("a1")).unwrap();
        let oversized = format!("+ {}", "A".repeat(513));
        assert_eq!(
            gate.feed(oversized.as_bytes()).unwrap_err(),
            GateError::TooLarge
        );
        assert_eq!(gate.fragmentizer.message_bytes().len(), 514);
        assert_eq!(gate.feed(b"\r\n").unwrap_err(), GateError::InvalidState);
        for invalid in [b"+ [ALERT] text\r\n".as_slice(), b"+ =AAA\r\n"] {
            assert_eq!(
                parse(GateMode::Authentication, invalid).unwrap_err(),
                GateError::Malformed
            );
        }
        let mut gate = StrictResponseGate::new(GateMode::Authentication, Some("a1")).unwrap();
        let result = gate.feed(b"+ \r\ntrailing").unwrap();
        assert_eq!(result.consumed, 4);
        assert_eq!(
            result.frame.unwrap().into_kind(),
            ReplyKind::Continuation { empty: true }
        );
    }
    #[test]
    fn ext05_imap_authentication_capability_is_bounded_and_separate_from_completion() {
        assert_eq!(
            parse(
                GateMode::Authentication,
                b"* CAPABILITY IMAP4rev1 auth=plain\r\n"
            )
            .unwrap()
            .into_kind(),
            ReplyKind::Capability(vec!["IMAP4REV1".into(), "AUTH=PLAIN".into()])
        );
        let flood = format!("* CAPABILITY {}\r\n", "X ".repeat(257));
        assert_eq!(
            parse(GateMode::Authentication, flood.as_bytes()).unwrap_err(),
            GateError::CapabilityLimit
        );
        assert_eq!(
            parse(GateMode::Logout, b"* CAPABILITY IMAP4rev1\r\n").unwrap_err(),
            GateError::UnexpectedResponse
        );
    }
    #[test]
    fn sec08_imap_gate_and_frames_never_debug_raw_server_content() {
        let mut gate = StrictResponseGate::new(GateMode::Capability, Some("private_tag")).unwrap();
        let frame = gate
            .feed(b"* CAPABILITY IMAP4rev1 PRIVATE-SENTINEL\r\n")
            .unwrap()
            .frame
            .unwrap();
        for debug in [
            format!("{gate:?}"),
            format!("{frame:?}"),
            format!("{:?}", frame.kind()),
        ] {
            assert!(!debug.contains("PRIVATE-SENTINEL"));
            assert!(!debug.contains("private_tag"));
        }
    }
    #[test]
    fn ext03_imap_bad_expected_tags_and_eof_cannot_reset_gate() {
        for tag in ["", "bad tag", "+", "*", "a\n"] {
            assert!(StrictResponseGate::new(GateMode::Capability, Some(tag)).is_err());
        }
        assert!(StrictResponseGate::new(GateMode::Greeting, Some("a1")).is_err());
        assert!(StrictResponseGate::new(GateMode::Authentication, None).is_err());
        let mut gate = StrictResponseGate::new(GateMode::StartTls, Some("a1")).unwrap();
        assert!(gate.feed(b"a1 OK partial\r").unwrap().frame.is_none());
        assert_eq!(gate.eof(), Err(GateError::Truncated));
        assert!(matches!(gate.feed(b"\n"), Err(GateError::InvalidState)));
    }
}
