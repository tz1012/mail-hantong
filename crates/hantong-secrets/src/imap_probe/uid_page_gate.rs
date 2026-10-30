//! Strict framing and shape checks before invoking the recursive IMAP codec.
use hantong_domain::imap_sync::{
    ImapEnvelope, ImapEnvelopeAddress, ImapEnvelopeInput, ImapMessageMetadata,
};
use io_imap::{
    codec::{ResponseCodec, decode::Decoder},
    types::{
        core::NString,
        envelope::{Address, Envelope},
        fetch::MessageDataItem,
        response::{Code, Data, Response, Status, StatusKind},
    },
};
use std::fmt;
const MAX_FRAME: usize = 128 * 1024;
const MAX_LITERAL: usize = 8192;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Mode {
    Examine,
    Fetch,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GateError {
    Invalid,
    TooLarge,
    #[cfg(test)]
    Truncated,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Observation {
    None,
    UidValidity(u32),
    UidNext(u32),
    ReadOnly,
}
#[derive(Debug, Eq, PartialEq)]
pub(super) enum Reply {
    Data,
    Status(Observation),
    Tagged(Observation),
    Message(Box<ImapMessageMetadata>),
}

pub(super) struct UidPageGate {
    mode: Mode,
    tag: String,
    frame: Vec<u8>,
    line_start: usize,
    literal_remaining: usize,
    depth: usize,
    quoted: bool,
    escaped: bool,
    closed: bool,
}
impl fmt::Debug for UidPageGate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UidPageGate { <redacted> }")
    }
}
impl UidPageGate {
    pub(super) fn new(mode: Mode, tag: &str) -> Result<Self, GateError> {
        if tag.is_empty()
            || tag.len() > 64
            || !tag
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            return Err(GateError::Invalid);
        }
        Ok(Self {
            mode,
            tag: tag.into(),
            frame: Vec::with_capacity(4096),
            line_start: 0,
            literal_remaining: 0,
            depth: 0,
            quoted: false,
            escaped: false,
            closed: false,
        })
    }
    pub(super) fn push(&mut self, byte: u8) -> Result<Option<Reply>, GateError> {
        if self.closed {
            return Err(GateError::Invalid);
        }
        let result = self.push_inner(byte);
        if result.is_err() {
            self.closed = true;
        }
        result
    }
    fn push_inner(&mut self, byte: u8) -> Result<Option<Reply>, GateError> {
        if self.frame.len() == MAX_FRAME {
            return Err(GateError::TooLarge);
        }
        self.frame.push(byte);
        if self.literal_remaining > 0 {
            self.literal_remaining -= 1;
            if self.literal_remaining == 0 {
                self.line_start = self.frame.len();
            }
            return Ok(None);
        }
        if byte == b'\n' {
            if self.frame.len() < 2
                || self.frame.len() - 2 < self.line_start
                || self.frame[self.frame.len() - 2] != b'\r'
            {
                return Err(GateError::Invalid);
            }
            let segment = &self.frame[self.line_start..self.frame.len() - 2];
            if self.fetch_candidate()
                && !self.quoted
                && let Some(length) = literal_length(segment)?
            {
                // An incomplete preflight must stop at an ENVELOPE nstring, not
                // a status, FLAGS, BODY, or arbitrary nested structure.
                if scan_shape(&self.frame, self.mode, &self.tag, true)
                    != Err(ScanError::LiteralBoundary)
                {
                    return Err(GateError::Invalid);
                }
                self.literal_remaining = length;
                self.line_start = self.frame.len();
                return Ok(None);
            }
            self.closed = true;
            scan_shape(&self.frame, self.mode, &self.tag, false).map_err(|_| GateError::Invalid)?;
            return decode(&self.frame, self.mode, &self.tag).map(Some);
        }
        if byte == b'\r' {
            return Ok(None);
        }
        if self.frame.len() > 1
            && self.frame.len() - 2 >= self.line_start
            && self.frame[self.frame.len() - 2] == b'\r'
        {
            return Err(GateError::Invalid);
        }
        if byte < 32 || byte == 127 {
            return Err(GateError::Invalid);
        }
        if !self.fetch_candidate() {
            return Ok(None);
        }
        if self.quoted {
            if self.escaped {
                self.escaped = false;
            } else if byte == b'\\' {
                self.escaped = true;
            } else if byte == b'"' {
                self.quoted = false;
            }
        } else if byte == b'"' {
            self.quoted = true;
        } else if byte == b'(' {
            self.depth += 1;
            if self.depth > 8 {
                return Err(GateError::TooLarge);
            }
        } else if byte == b')' {
            self.depth = self.depth.checked_sub(1).ok_or(GateError::Invalid)?;
        }
        Ok(None)
    }
    fn fetch_candidate(&self) -> bool {
        self.frame.starts_with(b"* ") && self.frame.get(2).is_some_and(u8::is_ascii_digit)
    }
    #[cfg(test)]
    fn finish(&self) -> Result<(), GateError> {
        if self.closed {
            Ok(())
        } else {
            Err(GateError::Truncated)
        }
    }
}
fn literal_length(line: &[u8]) -> Result<Option<usize>, GateError> {
    if !line.ends_with(b"}") {
        return Ok(None);
    }
    let at = line
        .iter()
        .rposition(|b| *b == b'{')
        .ok_or(GateError::Invalid)?;
    let digits = &line[at + 1..line.len() - 1];
    if digits.is_empty() || digits.len() > 10 || !digits.iter().all(u8::is_ascii_digit) {
        return Err(GateError::Invalid);
    }
    let n = std::str::from_utf8(digits)
        .map_err(|_| GateError::Invalid)?
        .parse::<usize>()
        .map_err(|_| GateError::TooLarge)?;
    if n > MAX_LITERAL {
        return Err(GateError::TooLarge);
    }
    Ok(Some(n))
}
#[derive(Debug, Eq, PartialEq)]
enum ScanError {
    Invalid,
    LiteralBoundary,
}
type ScanResult<T> = Result<T, ScanError>;
struct Scan<'a> {
    bytes: &'a [u8],
    at: usize,
    boundary: bool,
    raw: usize,
    addresses: usize,
}
impl<'a> Scan<'a> {
    fn take(&mut self, byte: u8) -> ScanResult<()> {
        if self.bytes.get(self.at) != Some(&byte) {
            return Err(ScanError::Invalid);
        }
        self.at += 1;
        Ok(())
    }
    fn word(&mut self) -> ScanResult<&'a [u8]> {
        let start = self.at;
        while self.bytes.get(self.at).is_some_and(|b| {
            b.is_ascii_graphic() && !matches!(b, b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'"')
        }) {
            self.at += 1;
            if self.at - start > 128 {
                return Err(ScanError::Invalid);
            }
        }
        if start == self.at {
            return Err(ScanError::Invalid);
        }
        Ok(&self.bytes[start..self.at])
    }
    fn number(&mut self, nonzero: bool) -> ScanResult<u32> {
        let word = self.word()?;
        if !word.iter().all(u8::is_ascii_digit) {
            return Err(ScanError::Invalid);
        }
        let n = std::str::from_utf8(word)
            .map_err(|_| ScanError::Invalid)?
            .parse()
            .map_err(|_| ScanError::Invalid)?;
        if nonzero && n == 0 {
            return Err(ScanError::Invalid);
        }
        Ok(n)
    }
    fn flags(&mut self) -> ScanResult<()> {
        self.take(b'(')?;
        let mut count = 0;
        let mut total = 0;
        while self.bytes.get(self.at) != Some(&b')') {
            if count > 0 {
                self.take(b' ')?
            }
            let flag = self.word()?;
            count += 1;
            total += flag.len();
            if count > 64 || total > 4096 {
                return Err(ScanError::Invalid);
            }
        }
        self.take(b')')
    }
    fn string(&mut self, literal: bool) -> ScanResult<usize> {
        if self.bytes.get(self.at) == Some(&b'"') {
            self.at += 1;
            let mut length = 0;
            loop {
                let b = *self.bytes.get(self.at).ok_or(ScanError::Invalid)?;
                self.at += 1;
                if b == b'"' {
                    return Ok(length);
                }
                if b == b'\\' {
                    let escaped = *self.bytes.get(self.at).ok_or(ScanError::Invalid)?;
                    if !matches!(escaped, b'\\' | b'"') {
                        return Err(ScanError::Invalid);
                    }
                    self.at += 1;
                } else if !(32..=126).contains(&b) {
                    return Err(ScanError::Invalid);
                }
                length += 1;
                if length > MAX_LITERAL {
                    return Err(ScanError::Invalid);
                }
            }
        }
        if !literal {
            return Err(ScanError::Invalid);
        }
        self.take(b'{')?;
        let start = self.at;
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
            if self.at - start > 10 {
                return Err(ScanError::Invalid);
            }
        }
        if start == self.at {
            return Err(ScanError::Invalid);
        }
        let n = std::str::from_utf8(&self.bytes[start..self.at])
            .map_err(|_| ScanError::Invalid)?
            .parse::<usize>()
            .map_err(|_| ScanError::Invalid)?;
        if n > MAX_LITERAL {
            return Err(ScanError::Invalid);
        }
        self.take(b'}')?;
        self.take(b'\r')?;
        self.take(b'\n')?;
        if self.boundary && self.at == self.bytes.len() {
            return Err(ScanError::LiteralBoundary);
        }
        self.at = self
            .at
            .checked_add(n)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(ScanError::Invalid)?;
        Ok(n)
    }
    fn nstring(&mut self) -> ScanResult<()> {
        if self
            .bytes
            .get(self.at..self.at + 3)
            .is_some_and(|s| s.eq_ignore_ascii_case(b"NIL"))
        {
            self.at += 3;
            return Ok(());
        }
        self.raw += self.string(true)?;
        if self.raw > 65536 {
            return Err(ScanError::Invalid);
        }
        Ok(())
    }
    fn addresses(&mut self) -> ScanResult<()> {
        if self
            .bytes
            .get(self.at..self.at + 3)
            .is_some_and(|s| s.eq_ignore_ascii_case(b"NIL"))
        {
            self.at += 3;
            return Ok(());
        }
        self.take(b'(')?;
        let mut count = 0;
        loop {
            self.take(b'(')?;
            for n in 0..4 {
                if n > 0 {
                    self.take(b' ')?
                }
                self.nstring()?;
            }
            self.take(b')')?;
            count += 1;
            self.addresses += 1;
            if count > 128 || self.addresses > 256 {
                return Err(ScanError::Invalid);
            }
            if self.bytes.get(self.at) == Some(&b')') {
                break;
            }
            // address-list is 1*address; servers may put SP between addresses.
            if self.bytes.get(self.at) == Some(&b' ') {
                self.at += 1;
            }
        }
        self.take(b')')
    }
    fn envelope(&mut self) -> ScanResult<()> {
        self.take(b'(')?;
        for field in 0..10 {
            if field > 0 {
                self.take(b' ')?
            }
            if (2..=7).contains(&field) {
                self.addresses()?
            } else {
                self.nstring()?
            }
        }
        self.take(b')')
    }
    fn fetch(&mut self) -> ScanResult<()> {
        self.take(b'(')?;
        let mut fields = 0u8;
        loop {
            let name = self.word()?.to_ascii_uppercase();
            let bit = match name.as_slice() {
                b"UID" => 1,
                b"FLAGS" => 2,
                b"INTERNALDATE" => 4,
                b"RFC822.SIZE" => 8,
                b"ENVELOPE" => 16,
                _ => return Err(ScanError::Invalid),
            };
            if fields & bit != 0 {
                return Err(ScanError::Invalid);
            }
            fields |= bit;
            self.take(b' ')?;
            match bit {
                1 => {
                    self.number(true)?;
                }
                2 => self.flags()?,
                4 => {
                    if self.string(false)? > 128 {
                        return Err(ScanError::Invalid);
                    }
                }
                8 => {
                    self.number(false)?;
                }
                16 => self.envelope()?,
                _ => unreachable!(),
            }
            if self.bytes.get(self.at) == Some(&b')') {
                break;
            }
            self.take(b' ')?;
        }
        self.take(b')')?;
        if fields != 31 {
            return Err(ScanError::Invalid);
        }
        Ok(())
    }
    fn status(&mut self) -> ScanResult<()> {
        if self.bytes.get(self.at) == Some(&b'[') {
            self.at += 1;
            let code = self.word()?.to_ascii_uppercase();
            match code.as_slice() {
                b"UIDVALIDITY" | b"UIDNEXT" | b"UNSEEN" => {
                    self.take(b' ')?;
                    self.number(true)?;
                }
                b"PERMANENTFLAGS" => {
                    self.take(b' ')?;
                    self.flags()?;
                }
                b"READ-ONLY" | b"ALERT" => {}
                _ => return Err(ScanError::Invalid),
            }
            self.take(b']')?;
            self.take(b' ')?;
        }
        let text = &self.bytes[self.at..];
        if text.len() < 3
            || !text.ends_with(b"\r\n")
            || !text[..text.len() - 2]
                .iter()
                .all(|b| (32..=126).contains(b))
        {
            return Err(ScanError::Invalid);
        }
        self.at = self.bytes.len() - 2;
        Ok(())
    }
}
fn scan_shape(bytes: &[u8], mode: Mode, tag: &str, boundary: bool) -> ScanResult<()> {
    let mut s = Scan {
        bytes,
        at: 0,
        boundary,
        raw: 0,
        addresses: 0,
    };
    let first = s.word()?;
    s.take(b' ')?;
    if first == b"*" {
        let start = s.at;
        let word = s.word()?.to_ascii_uppercase();
        s.take(b' ')?;
        if word == b"OK" {
            s.status()?;
        } else if word == b"FLAGS" && mode == Mode::Examine {
            s.flags()?;
        } else {
            s.at = start;
            let seq = s.number(false)?;
            s.take(b' ')?;
            let kind = s.word()?.to_ascii_uppercase();
            match kind.as_slice() {
                b"EXISTS" | b"RECENT" if mode == Mode::Examine => {}
                b"FETCH" if mode == Mode::Fetch && seq > 0 => {
                    s.take(b' ')?;
                    s.fetch()?;
                }
                _ => return Err(ScanError::Invalid),
            }
        }
    } else {
        if first != tag.as_bytes() || !s.word()?.eq_ignore_ascii_case(b"OK") {
            return Err(ScanError::Invalid);
        }
        s.take(b' ')?;
        s.status()?;
    }
    s.take(b'\r')?;
    s.take(b'\n')?;
    if s.at != bytes.len() {
        return Err(ScanError::Invalid);
    }
    Ok(())
}
fn code(value: Option<Code<'_>>, mode: Mode) -> Result<Observation, GateError> {
    Ok(match value {
        None => Observation::None,
        Some(Code::Alert) => Observation::None,
        Some(Code::UidValidity(n)) if mode == Mode::Examine => Observation::UidValidity(n.get()),
        Some(Code::UidNext(n)) if mode == Mode::Examine => Observation::UidNext(n.get()),
        Some(Code::ReadOnly) if mode == Mode::Examine => Observation::ReadOnly,
        Some(Code::Unseen(_) | Code::PermanentFlags(_)) if mode == Mode::Examine => {
            Observation::None
        }
        _ => return Err(GateError::Invalid),
    })
}
fn raw(value: NString<'_>) -> Option<Vec<u8>> {
    value.into_option().map(|v| v.into_owned())
}
fn addresses(values: Vec<Address<'_>>) -> Result<Option<Vec<ImapEnvelopeAddress>>, GateError> {
    if values.is_empty() {
        return Ok(None);
    }
    values
        .into_iter()
        .map(|v| {
            ImapEnvelopeAddress::new(raw(v.name), raw(v.adl), raw(v.mailbox), raw(v.host))
                .map_err(|_| GateError::Invalid)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}
fn envelope(v: Envelope<'_>) -> Result<ImapEnvelope, GateError> {
    ImapEnvelope::new(ImapEnvelopeInput {
        date: raw(v.date),
        subject: raw(v.subject),
        from: addresses(v.from)?,
        sender: addresses(v.sender)?,
        reply_to: addresses(v.reply_to)?,
        to: addresses(v.to)?,
        cc: addresses(v.cc)?,
        bcc: addresses(v.bcc)?,
        in_reply_to: raw(v.in_reply_to),
        message_id: raw(v.message_id),
    })
    .map_err(|_| GateError::Invalid)
}
fn decode(frame: &[u8], mode: Mode, tag: &str) -> Result<Reply, GateError> {
    let (rest, response) = ResponseCodec::new()
        .decode(frame)
        .map_err(|_| GateError::Invalid)?;
    if !rest.is_empty() {
        return Err(GateError::Invalid);
    }
    match response {
        Response::Status(Status::Tagged(v))
            if v.tag.as_ref() == tag && v.body.kind == StatusKind::Ok =>
        {
            Ok(Reply::Tagged(code(v.body.code, mode)?))
        }
        Response::Status(Status::Untagged(v)) if v.kind == StatusKind::Ok => {
            Ok(Reply::Status(code(v.code, mode)?))
        }
        Response::Data(Data::Flags(_) | Data::Exists(_) | Data::Recent(_))
            if mode == Mode::Examine =>
        {
            Ok(Reply::Data)
        }
        Response::Data(Data::Fetch { items, .. }) if mode == Mode::Fetch => {
            let (mut uid, mut flags, mut date, mut size, mut env) = (None, None, None, None, None);
            for item in items.into_inner() {
                match item {
                    MessageDataItem::Uid(v) if uid.is_none() => uid = Some(v.get()),
                    MessageDataItem::Flags(v) if flags.is_none() => {
                        flags = Some(
                            v.iter()
                                .map(|flag| match flag {
                                    io_imap::types::flag::FlagFetch::Flag(flag) => flag.to_string(),
                                    io_imap::types::flag::FlagFetch::Recent => "\\Recent".into(),
                                })
                                .collect(),
                        )
                    }
                    MessageDataItem::InternalDate(v) if date.is_none() => {
                        date = Some(v.as_ref().timestamp_millis())
                    }
                    MessageDataItem::Rfc822Size(v) if size.is_none() => size = Some(v),
                    MessageDataItem::Envelope(v) if env.is_none() => env = Some(envelope(v)?),
                    _ => return Err(GateError::Invalid),
                }
            }
            ImapMessageMetadata::new(
                uid.ok_or(GateError::Invalid)?,
                flags.ok_or(GateError::Invalid)?,
                date.ok_or(GateError::Invalid)?,
                size.ok_or(GateError::Invalid)?,
                env.ok_or(GateError::Invalid)?,
            )
            .map(|message| Reply::Message(Box::new(message)))
            .map_err(|_| GateError::Invalid)
        }
        _ => Err(GateError::Invalid),
    }
}
#[cfg(test)]
#[path = "uid_page_gate_tests.rs"]
mod tests;
