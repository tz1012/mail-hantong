//! Bounded parser for the read-only IMAP LIST exchange.
//!
//! This parser deliberately does not use the normal probe gate: mailbox names
//! may be literals, while the probe gate rejects literals before decoding.
use io_imap::{
    codec::{ResponseCodec, decode::Decoder},
    types::{
        mailbox::Mailbox,
        response::{Data, Response, StatusKind},
    },
};
use std::fmt;

pub(crate) const MAX_FRAME_BYTES: usize = 16 * 1024;
const MAX_LITERAL_BYTES: usize = 1024;
const MAX_ATTRIBUTES: usize = 32;
const MAX_ATTRIBUTE_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ListGateError {
    InvalidTag,
    TooLarge,
    Malformed,
    WrongTag,
    Unexpected,
    LiteralTooLarge,
    LiteralNotAtMailboxEnd,
    InvalidLineEnding,
    #[cfg(test)]
    Truncated,
}
impl fmt::Display for ListGateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidTag => "invalid IMAP LIST tag",
            Self::TooLarge => "IMAP LIST frame exceeds the limit",
            Self::Malformed => "malformed IMAP LIST response",
            Self::WrongTag => "IMAP LIST response tag mismatch",
            Self::Unexpected => "unexpected IMAP LIST response",
            Self::LiteralTooLarge => "IMAP LIST literal exceeds the limit",
            Self::LiteralNotAtMailboxEnd => "IMAP LIST literal is not a mailbox name",
            Self::InvalidLineEnding => "IMAP LIST response requires CRLF",
            #[cfg(test)]
            Self::Truncated => "incomplete IMAP LIST response",
        })
    }
}
impl std::error::Error for ListGateError {}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct RawMailbox {
    pub(crate) name: Vec<u8>,
    pub(crate) delimiter: Option<char>,
    pub(crate) attributes: Vec<String>,
}
impl fmt::Debug for RawMailbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RawMailbox { <redacted> }")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ListReply {
    Mailbox(RawMailbox),
    Tagged(StatusKind),
    Status(StatusKind),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ListFeed {
    pub(crate) consumed: usize,
    pub(crate) reply: Option<ListReply>,
}

pub(crate) struct ListGate {
    expected_tag: String,
    frame_limit: usize,
    frame: Vec<u8>,
    segment: Vec<u8>,
    literal_remaining: Option<usize>,
    literal_start: usize,
    literal_body: Option<(usize, usize)>,
    after_literal: bool,
    closed: bool,
}
impl fmt::Debug for ListGate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListGate")
            .field("closed", &self.closed)
            .field("literal_remaining", &self.literal_remaining)
            .finish()
    }
}

impl ListGate {
    #[cfg(test)]
    pub(crate) fn new(expected_tag: &str) -> Result<Self, ListGateError> {
        Self::with_limit(expected_tag, MAX_FRAME_BYTES)
    }

    pub(crate) fn with_limit(
        expected_tag: &str,
        remaining_plaintext: usize,
    ) -> Result<Self, ListGateError> {
        if expected_tag.is_empty()
            || expected_tag.len() > 64
            || !expected_tag
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(ListGateError::InvalidTag);
        }
        Ok(Self {
            expected_tag: expected_tag.to_owned(),
            frame_limit: remaining_plaintext.min(MAX_FRAME_BYTES),
            frame: Vec::new(),
            segment: Vec::new(),
            literal_remaining: None,
            literal_start: 0,
            literal_body: None,
            after_literal: false,
            closed: false,
        })
    }

    pub(crate) fn feed(&mut self, input: &[u8]) -> Result<ListFeed, ListGateError> {
        if self.closed {
            return Err(ListGateError::Unexpected);
        }
        for (index, &byte) in input.iter().enumerate() {
            if self.frame.len() >= self.frame_limit {
                return self.fail(ListGateError::TooLarge);
            }
            if let Some(remaining) = self.literal_remaining {
                if remaining > 0 {
                    self.frame.push(byte);
                    self.literal_remaining = Some(remaining - 1);
                    if remaining == 1 {
                        self.literal_body = Some((self.literal_start, self.frame.len()));
                    }
                    continue;
                }
                if !self.after_literal {
                    if byte != b'\r' {
                        return self.fail(ListGateError::InvalidLineEnding);
                    }
                    self.frame.push(byte);
                    self.after_literal = true;
                    continue;
                }
                if byte != b'\n' {
                    return self.fail(ListGateError::InvalidLineEnding);
                }
                self.frame.push(byte);
                self.closed = true;
                let reply = self.parse_frame()?;
                return Ok(ListFeed {
                    consumed: index + 1,
                    reply: Some(reply),
                });
            }

            if byte != b'\r' && byte != b'\n' && (byte < 0x20 || byte == 0x7f) {
                return self.fail(ListGateError::Malformed);
            }
            self.frame.push(byte);
            self.segment.push(byte);
            if byte == b'\n' {
                if self.segment.len() < 2 || self.segment[self.segment.len() - 2] != b'\r' {
                    return self.fail(ListGateError::InvalidLineEnding);
                }
                let content_len = self.segment.len() - 2;
                let content = self.segment[..content_len].to_vec();
                self.segment.clear();
                if let Some(length) = literal_declaration(&content)? {
                    if !is_list_prefix(&content) {
                        return self.fail(ListGateError::Unexpected);
                    }
                    validate_literal_header(&content)?;
                    if self.frame.len().saturating_add(length).saturating_add(2) > self.frame_limit
                    {
                        return self.fail(ListGateError::TooLarge);
                    }
                    self.literal_remaining = Some(length);
                    self.literal_start = self.frame.len();
                    self.after_literal = false;
                    continue;
                }
                self.closed = true;
                let reply = self.parse_frame()?;
                return Ok(ListFeed {
                    consumed: index + 1,
                    reply: Some(reply),
                });
            }
        }
        Ok(ListFeed {
            consumed: input.len(),
            reply: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn finish(&mut self) -> Result<(), ListGateError> {
        if self.closed {
            Ok(())
        } else {
            self.closed = true;
            Err(ListGateError::Truncated)
        }
    }

    fn fail<T>(&mut self, error: ListGateError) -> Result<T, ListGateError> {
        self.closed = true;
        Err(error)
    }

    fn parse_frame(&self) -> Result<ListReply, ListGateError> {
        if self.frame.len() < 2 || !self.frame.ends_with(b"\r\n") {
            return Err(ListGateError::InvalidLineEnding);
        }
        let content = &self.frame[..self.frame.len() - 2];
        if is_list_prefix(content) {
            let mailbox = parse_mailbox(content, self.literal_body)?;
            let (remaining, response) = ResponseCodec::new()
                .decode(&self.frame)
                .map_err(|_| ListGateError::Malformed)?;
            if !remaining.is_empty() {
                return Err(ListGateError::Malformed);
            }
            let Response::Data(Data::List {
                items,
                delimiter,
                mailbox: decoded,
            }) = response
            else {
                return Err(ListGateError::Unexpected);
            };
            let same_name = match &decoded {
                Mailbox::Inbox => mailbox.name.eq_ignore_ascii_case(b"INBOX"),
                Mailbox::Other(_) => mailbox_name_bytes(&decoded) == mailbox.name.as_slice(),
            };
            if !same_name {
                return Err(ListGateError::Malformed);
            }
            let mut decoded_attributes = items
                .iter()
                .map(ToString::to_string)
                .map(|item| item.trim_start_matches('\\').to_ascii_lowercase())
                .collect::<Vec<_>>();
            decoded_attributes.sort();
            decoded_attributes.dedup();
            if decoded_attributes != mailbox.attributes
                || delimiter.map(|value| value.inner()) != mailbox.delimiter
            {
                return Err(ListGateError::Malformed);
            }
            return Ok(ListReply::Mailbox(mailbox));
        }
        if self.literal_body.is_some() {
            return Err(ListGateError::LiteralNotAtMailboxEnd);
        }
        let untagged_kind = if content.starts_with(b"* ") {
            Some(parse_status(content)?)
        } else {
            None
        };
        let (tagged_tag, tagged_kind) = if untagged_kind.is_none() {
            let (tag, kind) = parse_tagged_status(content)?;
            if tag != self.expected_tag {
                return Err(ListGateError::WrongTag);
            }
            (Some(tag), Some(kind))
        } else {
            (None, None)
        };
        let (remaining, response) = ResponseCodec::new()
            .decode(&self.frame)
            .map_err(|_| ListGateError::Malformed)?;
        if !remaining.is_empty() {
            return Err(ListGateError::Malformed);
        }
        if let Some(kind) = untagged_kind {
            let Response::Status(io_imap::types::response::Status::Untagged(status)) = response
            else {
                return Err(ListGateError::Unexpected);
            };
            if status.kind != kind {
                return Err(ListGateError::Malformed);
            }
            return Ok(ListReply::Status(kind));
        }
        let tag = tagged_tag.ok_or(ListGateError::Unexpected)?;
        let kind = tagged_kind.ok_or(ListGateError::Unexpected)?;
        let Response::Status(io_imap::types::response::Status::Tagged(status)) = response else {
            return Err(ListGateError::Unexpected);
        };
        if status.body.kind != kind || status.tag.as_ref() != tag {
            return Err(ListGateError::Malformed);
        }
        Ok(ListReply::Tagged(kind))
    }
}

fn is_list_prefix(content: &[u8]) -> bool {
    content
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"* LIST "))
}

fn mailbox_name_bytes<'a>(mailbox: &'a Mailbox<'a>) -> &'a [u8] {
    match mailbox {
        Mailbox::Inbox => b"INBOX",
        Mailbox::Other(other) => other.as_ref(),
    }
}

fn literal_declaration(content: &[u8]) -> Result<Option<usize>, ListGateError> {
    let Some(open) = content.iter().rposition(|b| *b == b'{') else {
        return Ok(None);
    };
    if !content.ends_with(b"}") {
        return Ok(None);
    }
    let digits = &content[open + 1..content.len() - 1];
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Err(ListGateError::Malformed);
    }
    let length = std::str::from_utf8(digits)
        .map_err(|_| ListGateError::Malformed)?
        .parse::<usize>()
        .map_err(|_| ListGateError::LiteralTooLarge)?;
    if length > MAX_LITERAL_BYTES {
        return Err(ListGateError::LiteralTooLarge);
    }
    if open == 0 || content[open - 1] != b' ' {
        return Err(ListGateError::Malformed);
    }
    Ok(Some(length))
}

fn validate_literal_header(content: &[u8]) -> Result<(), ListGateError> {
    let mut p = b"* LIST ".len();
    if content.get(p) != Some(&b'(') {
        return Err(ListGateError::Malformed);
    }
    let close = content[p..]
        .iter()
        .position(|b| *b == b')')
        .ok_or(ListGateError::Malformed)?
        + p;
    parse_attributes(&content[p + 1..close])?;
    p = close + 1;
    if content.get(p) != Some(&b' ') {
        return Err(ListGateError::Malformed);
    }
    p += 1;
    let (_, used) = parse_delimiter(&content[p..])?;
    p += used;
    if content.get(p) != Some(&b' ') {
        return Err(ListGateError::Malformed);
    }
    p += 1;
    let marker = &content[p..];
    if marker.len() < 3
        || marker.first() != Some(&b'{')
        || marker.last() != Some(&b'}')
        || !marker[1..marker.len() - 1].iter().all(u8::is_ascii_digit)
    {
        return Err(ListGateError::LiteralNotAtMailboxEnd);
    }
    Ok(())
}

fn parse_mailbox(
    content: &[u8],
    literal: Option<(usize, usize)>,
) -> Result<RawMailbox, ListGateError> {
    let mut p = b"* LIST ".len();
    if content.get(p) != Some(&b'(') {
        return Err(ListGateError::Malformed);
    }
    let close = content[p..]
        .iter()
        .position(|b| *b == b')')
        .ok_or(ListGateError::Malformed)?
        + p;
    let attrs = parse_attributes(&content[p + 1..close])?;
    p = close + 1;
    if content.get(p) != Some(&b' ') {
        return Err(ListGateError::Malformed);
    }
    p += 1;
    let (delimiter, used) = parse_delimiter(&content[p..])?;
    p += used;
    if content.get(p) != Some(&b' ') {
        return Err(ListGateError::Malformed);
    }
    p += 1;
    let name = if let Some((start, end)) = literal {
        if !content[p..].starts_with(b"{") || start > end || end > content.len() {
            return Err(ListGateError::LiteralNotAtMailboxEnd);
        }
        let marker_end = content[p..]
            .iter()
            .position(|b| *b == b'}')
            .ok_or(ListGateError::Malformed)?
            + p;
        if marker_end + 2 > content.len() || content[marker_end + 1] != b'\r' {
            return Err(ListGateError::LiteralNotAtMailboxEnd);
        }
        if end != content.len() {
            return Err(ListGateError::LiteralNotAtMailboxEnd);
        }
        content[start..end].to_vec()
    } else {
        let (value, used) = parse_astring(&content[p..])?;
        p += used;
        if p != content.len() {
            return Err(ListGateError::Malformed);
        }
        value
    };
    if name.is_empty()
        || name.len() > MAX_LITERAL_BYTES
        || name.iter().any(|b| *b < 0x20 || *b == 0x7f)
    {
        return Err(ListGateError::Malformed);
    }
    Ok(RawMailbox {
        name,
        delimiter,
        attributes: attrs,
    })
}

fn parse_attributes(input: &[u8]) -> Result<Vec<String>, ListGateError> {
    let mut result = Vec::new();
    for token in input.split(|b| *b == b' ').filter(|v| !v.is_empty()) {
        if token.len() < 2 || token[0] != b'\\' || token.len() > MAX_ATTRIBUTE_BYTES {
            return Err(ListGateError::Malformed);
        }
        if !token[1..]
            .iter()
            .all(|b| b.is_ascii_graphic() && *b != b'(' && *b != b')')
        {
            return Err(ListGateError::Malformed);
        }
        if result.len() == MAX_ATTRIBUTES {
            return Err(ListGateError::Malformed);
        }
        result.push(String::from_utf8_lossy(&token[1..]).to_ascii_lowercase());
    }
    result.sort();
    result.dedup();
    Ok(result)
}

fn parse_delimiter(input: &[u8]) -> Result<(Option<char>, usize), ListGateError> {
    if input.len() >= 3 && input[..3].eq_ignore_ascii_case(b"NIL") {
        return Ok((None, 3));
    }
    let (bytes, used) = parse_quoted(input)?;
    if bytes.len() != 1 || !bytes[0].is_ascii() {
        return Err(ListGateError::Malformed);
    }
    Ok((Some(bytes[0] as char), used))
}

fn parse_astring(input: &[u8]) -> Result<(Vec<u8>, usize), ListGateError> {
    if input.starts_with(b"\"") {
        return parse_quoted(input);
    }
    let end = input
        .iter()
        .position(|b| *b == b' ' || *b == b'\r' || *b == b'\n')
        .unwrap_or(input.len());
    if end == 0
        || input[..end]
            .iter()
            .any(|b| !b.is_ascii_graphic() || b"(){}\\\"".contains(b))
    {
        return Err(ListGateError::Malformed);
    }
    Ok((input[..end].to_vec(), end))
}

fn parse_quoted(input: &[u8]) -> Result<(Vec<u8>, usize), ListGateError> {
    if input.first() != Some(&b'\"') {
        return Err(ListGateError::Malformed);
    }
    let mut value = Vec::new();
    let mut escaped = false;
    for (i, byte) in input[1..].iter().copied().enumerate() {
        if escaped {
            if byte != b'\"' && byte != b'\\' {
                return Err(ListGateError::Malformed);
            }
            value.push(byte);
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'\"' {
            return Ok((value, i + 2));
        } else if byte < 0x20 || byte == 0x7f {
            return Err(ListGateError::Malformed);
        } else {
            value.push(byte);
        }
    }
    Err(ListGateError::Malformed)
}

fn parse_status(content: &[u8]) -> Result<StatusKind, ListGateError> {
    let mut parts = content.splitn(3, |b| *b == b' ');
    if parts.next() != Some(b"*") {
        return Err(ListGateError::Unexpected);
    }
    match parts.next() {
        Some(token) if token.eq_ignore_ascii_case(b"OK") => Ok(StatusKind::Ok),
        Some(token) if token.eq_ignore_ascii_case(b"NO") => Ok(StatusKind::No),
        Some(token) if token.eq_ignore_ascii_case(b"BAD") => Ok(StatusKind::Bad),
        _ => Err(ListGateError::Unexpected),
    }
}

fn parse_tagged_status(content: &[u8]) -> Result<(String, StatusKind), ListGateError> {
    let mut parts = content.splitn(3, |b| *b == b' ');
    let tag = parts.next().ok_or(ListGateError::Malformed)?;
    let kind = match parts.next() {
        Some(token) if token.eq_ignore_ascii_case(b"OK") => StatusKind::Ok,
        Some(token) if token.eq_ignore_ascii_case(b"NO") => StatusKind::No,
        Some(token) if token.eq_ignore_ascii_case(b"BAD") => StatusKind::Bad,
        _ => return Err(ListGateError::Unexpected),
    };
    let tag = std::str::from_utf8(tag).map_err(|_| ListGateError::Malformed)?;
    Ok((tag.to_owned(), kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext06_list_gate_parses_quoted_mailbox_and_stops_at_one_frame() {
        let mut gate = ListGate::new("l1").unwrap();
        let input = b"* LIST (\\HasNoChildren \\Sent) \"/\" \"Sent\"\r\nl1 OK done\r\n";
        let result = gate.feed(input).unwrap();
        assert_eq!(result.consumed, input.len() - b"l1 OK done\r\n".len());
        let Some(ListReply::Mailbox(mailbox)) = result.reply else {
            panic!()
        };
        assert_eq!(mailbox.name, b"Sent");
        assert_eq!(mailbox.delimiter, Some('/'));
        assert_eq!(mailbox.attributes, ["hasnochildren", "sent"]);
    }

    #[test]
    fn ext06_list_gate_accepts_bounded_literal_name() {
        let mut gate = ListGate::new("l1").unwrap();
        let input = b"* LIST (\\Noselect) NIL {5}\r\nInbox\r\n";
        let result = gate.feed(input).unwrap();
        let Some(ListReply::Mailbox(mailbox)) = result.reply else {
            panic!()
        };
        assert_eq!(mailbox.name, b"Inbox");
    }

    #[test]
    fn ext06_list_gate_rejects_literal_before_body_allocation() {
        let mut gate = ListGate::new("l1").unwrap();
        assert_eq!(
            gate.feed(b"* LIST () NIL {1025}\r\n"),
            Err(ListGateError::LiteralTooLarge)
        );
    }

    #[test]
    fn ext06_list_gate_requires_exact_completion_tag() {
        let mut gate = ListGate::new("l1").unwrap();
        assert_eq!(
            gate.feed(b"wrong OK done\r\n"),
            Err(ListGateError::WrongTag)
        );
    }

    #[test]
    fn ext06_list_gate_accepts_mixed_case_inbox_and_braces_in_quoted_name() {
        let mut gate = ListGate::new("l1").unwrap();
        let result = gate.feed(b"* LIST () \"/\" iNbOx\r\n").unwrap();
        let Some(ListReply::Mailbox(mailbox)) = result.reply else {
            panic!()
        };
        assert_eq!(mailbox.name, b"iNbOx");
        let mut gate = ListGate::new("l1").unwrap();
        let result = gate.feed(b"* LIST () \"/\" \"{5}\"\r\n").unwrap();
        let Some(ListReply::Mailbox(mailbox)) = result.reply else {
            panic!()
        };
        assert_eq!(mailbox.name, b"{5}");
    }

    #[test]
    fn ext06_list_gate_checks_completion_and_status_with_typed_codec() {
        let mut gate = ListGate::new("l1").unwrap();
        assert_eq!(
            gate.feed(b"* oK notice\r\n").unwrap().reply,
            Some(ListReply::Status(StatusKind::Ok))
        );
        let mut gate = ListGate::new("l1").unwrap();
        assert_eq!(
            gate.feed(b"l1 oK done\r\n").unwrap().reply,
            Some(ListReply::Tagged(StatusKind::Ok))
        );
        let mut gate = ListGate::new("l1").unwrap();
        let result = gate.feed(b"* lIsT (\\Noselect) nIl \"Inbox\"\r\n").unwrap();
        assert!(matches!(result.reply, Some(ListReply::Mailbox(_))));
    }

    #[test]
    fn ext06_list_gate_rejects_aggregate_literal_before_body_and_eof() {
        let mut gate = ListGate::with_limit("l1", 8).unwrap();
        assert_eq!(
            gate.feed(b"* LIST () NIL {5}\r\n"),
            Err(ListGateError::TooLarge)
        );
        let mut gate = ListGate::new("l1").unwrap();
        assert!(matches!(
            gate.feed(b"* LIST () NIL \"x"),
            Ok(ListFeed { reply: None, .. })
        ));
        assert_eq!(gate.finish(), Err(ListGateError::Truncated));
    }

    #[test]
    fn ext06_list_gate_rejects_nested_fetch_and_attribute_overflow() {
        let mut gate = ListGate::new("l1").unwrap();
        assert!(gate.feed(b"* 1 FETCH (BODY[])\r\n").is_err());
        let mut gate = ListGate::new("l1").unwrap();
        assert!(gate.feed(b"* 1 FETCH (BODY[] {4}\r\n").is_err());
        let attrs = (0..33).map(|i| format!(" \\X{i}")).collect::<String>();
        let line = format!("* LIST ({} ) NIL \"x\"\r\n", attrs);
        let mut gate = ListGate::new("l1").unwrap();
        assert!(gate.feed(line.as_bytes()).is_err());
    }

    #[test]
    fn ext06_list_gate_accepts_every_literal_chunk_boundary() {
        let input = b"* LIST () NIL {5}\r\nInbox\r\n";
        for split in 0..input.len() {
            let mut gate = ListGate::new("l1").unwrap();
            let first = gate.feed(&input[..split]).unwrap();
            let second = gate.feed(&input[split..]).unwrap();
            assert!(first.reply.is_none());
            assert!(
                matches!(second.reply, Some(ListReply::Mailbox(_))),
                "split={split}"
            );
            assert_eq!(second.consumed, input.len() - split);
        }
    }

    #[test]
    fn ext06_list_gate_rejects_literal_overflow_and_illegal_position_before_body() {
        let mut gate = ListGate::new("l1").unwrap();
        assert_eq!(
            gate.feed(b"* LIST () NIL {184467440737095516160}\r\n"),
            Err(ListGateError::LiteralTooLarge)
        );
        let mut gate = ListGate::new("l1").unwrap();
        assert!(gate.feed(b"* LIST () {3}\r\n").is_err());
    }

    #[test]
    fn ext06_list_gate_rejects_entire_malformed_literal_header_before_body() {
        for header in [
            b"* LIST () NIL {4}x {4}\r\n".as_slice(),
            b"* LIST () NIL {4} {4}\r\n",
            b"* OK notice {4}\r\n",
            b"* 1 FETCH (BODY[] {4}\r\n",
        ] {
            let mut gate = ListGate::new("l1").unwrap();
            assert!(gate.feed(header).is_err());
            assert!(gate.literal_remaining.is_none());
        }
    }

    #[test]
    fn ext06_list_gate_rejects_attribute_atom_and_count_limits() {
        let oversized = format!("\\{}", "x".repeat(127));
        let line = format!("* LIST ({oversized}) NIL \"x\"\r\n");
        let mut gate = ListGate::new("l1").unwrap();
        assert!(gate.feed(line.as_bytes()).is_ok());
        let oversized = format!("\\{}", "x".repeat(129));
        let line = format!("* LIST ({oversized}) NIL \"x\"\r\n");
        let mut gate = ListGate::new("l1").unwrap();
        assert!(gate.feed(line.as_bytes()).is_err());
        let attrs = (0..33).map(|i| format!(" \\X{i}")).collect::<String>();
        let line = format!("* LIST ({attrs}) NIL \"x\"\r\n");
        let mut gate = ListGate::new("l1").unwrap();
        assert!(gate.feed(line.as_bytes()).is_err());
    }

    #[test]
    fn ext06_list_gate_rejects_controls_and_frame_overflow() {
        let mut gate = ListGate::new("l1").unwrap();
        assert!(gate.feed(b"* LIST () NIL \"A\x01\"\r\n").is_err());
        let mut gate = ListGate::new("l1").unwrap();
        let oversized = vec![b'a'; MAX_FRAME_BYTES + 1];
        assert_eq!(gate.feed(&oversized), Err(ListGateError::TooLarge));
    }
}
