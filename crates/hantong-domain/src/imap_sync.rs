//! Strict, provider-neutral IMAP UID metadata page types.
//!
//! This module deliberately contains no transport or authority.  It validates
//! the bounded data crossing the Secrets/Store boundary and keeps raw envelope
//! bytes opaque to callers.

use crate::mailboxes::{ImapMailboxName, MailboxEncoding};
use serde::{Deserialize, Serialize};
use std::fmt;

const MAX_ID: usize = 128;
const PAGE_WIDTH: u32 = 64;
const MAX_RAW: usize = 8 * 1024;
const MAX_ADDRS: usize = 128;
const MAX_FLAGS: usize = 64;
const MAX_FLAG_BYTES: usize = 128;
const MAX_FLAG_TOTAL: usize = 4096;
const MAX_PAGE_MESSAGES: usize = 64;
const MAX_PAGE_JSON: usize = 8 * 1024 * 1024;
const MAX_METADATA_JSON: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapSyncError {
    InvalidInput,
    UidValidityChanged,
    CursorInvalid,
    TooLarge,
    InvalidStorage,
}
impl fmt::Display for ImapSyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidInput => "invalid IMAP sync input",
            Self::UidValidityChanged => "IMAP UID validity changed",
            Self::CursorInvalid => "invalid IMAP sync cursor",
            Self::TooLarge => "IMAP sync data too large",
            Self::InvalidStorage => "invalid IMAP sync storage",
        })
    }
}
impl std::error::Error for ImapSyncError {}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapUidPageRequest {
    account_id: String,
    endpoint_version: u64,
    mailbox: ImapMailboxName,
    expected_uid_validity: Option<u32>,
    after_uid: u32,
}
impl fmt::Debug for ImapUidPageRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapUidPageRequest { <redacted> }")
    }
}
impl ImapUidPageRequest {
    pub fn new(
        account_id: &str,
        endpoint_version: u64,
        mailbox: ImapMailboxName,
        expected_uid_validity: Option<u32>,
        after_uid: u32,
    ) -> Result<Self, ImapSyncError> {
        if !valid_id(account_id)
            || !(1..=i64::MAX as u64).contains(&endpoint_version)
            || expected_uid_validity == Some(0)
            || expected_uid_validity.is_none() && after_uid != 0
        {
            return Err(ImapSyncError::InvalidInput);
        }
        // Display spelling (notably mixed-case INBOX) is not UID identity.
        let mailbox = ImapMailboxName::from_wire(mailbox.wire_name(), mailbox.encoding())
            .map_err(|_| ImapSyncError::InvalidInput)?;
        Ok(Self {
            account_id: account_id.to_owned(),
            endpoint_version,
            mailbox,
            expected_uid_validity,
            after_uid,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn endpoint_version(&self) -> u64 {
        self.endpoint_version
    }
    pub fn mailbox(&self) -> &ImapMailboxName {
        &self.mailbox
    }
    pub fn expected_uid_validity(&self) -> Option<u32> {
        self.expected_uid_validity
    }
    pub fn after_uid(&self) -> u32 {
        self.after_uid
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapEnvelopeAddress {
    name: Option<Vec<u8>>,
    route: Option<Vec<u8>>,
    mailbox: Option<Vec<u8>>,
    host: Option<Vec<u8>>,
}
impl fmt::Debug for ImapEnvelopeAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapEnvelopeAddress { <redacted> }")
    }
}
impl ImapEnvelopeAddress {
    pub fn new(
        name: Option<Vec<u8>>,
        route: Option<Vec<u8>>,
        mailbox: Option<Vec<u8>>,
        host: Option<Vec<u8>>,
    ) -> Result<Self, ImapSyncError> {
        for x in [&name, &route, &mailbox, &host] {
            if let Some(v) = x
                && v.len() > MAX_RAW
            {
                return Err(ImapSyncError::TooLarge);
            }
        }
        Ok(Self {
            name,
            route,
            mailbox,
            host,
        })
    }
    pub fn name(&self) -> Option<&[u8]> {
        self.name.as_deref()
    }
    pub fn route(&self) -> Option<&[u8]> {
        self.route.as_deref()
    }
    pub fn mailbox(&self) -> Option<&[u8]> {
        self.mailbox.as_deref()
    }
    pub fn host(&self) -> Option<&[u8]> {
        self.host.as_deref()
    }
}

#[derive(Clone, Default, Eq, PartialEq)]
pub struct ImapEnvelopeInput {
    pub date: Option<Vec<u8>>,
    pub subject: Option<Vec<u8>>,
    pub in_reply_to: Option<Vec<u8>>,
    pub message_id: Option<Vec<u8>>,
    pub from: Option<Vec<ImapEnvelopeAddress>>,
    pub sender: Option<Vec<ImapEnvelopeAddress>>,
    pub reply_to: Option<Vec<ImapEnvelopeAddress>>,
    pub to: Option<Vec<ImapEnvelopeAddress>>,
    pub cc: Option<Vec<ImapEnvelopeAddress>>,
    pub bcc: Option<Vec<ImapEnvelopeAddress>>,
}
impl fmt::Debug for ImapEnvelopeInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapEnvelopeInput { <redacted> }")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapEnvelope {
    fields: ImapEnvelopeInput,
}
impl fmt::Debug for ImapEnvelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapEnvelope { <redacted> }")
    }
}
impl ImapEnvelope {
    pub fn new(fields: ImapEnvelopeInput) -> Result<Self, ImapSyncError> {
        let mut count = 0usize;
        let mut bytes = 0usize;
        for v in [
            &fields.date,
            &fields.subject,
            &fields.in_reply_to,
            &fields.message_id,
        ]
        .into_iter()
        .flatten()
        {
            if v.len() > MAX_RAW {
                return Err(ImapSyncError::TooLarge);
            }
            bytes += v.len();
        }
        for items in [
            &fields.from,
            &fields.sender,
            &fields.reply_to,
            &fields.to,
            &fields.cc,
            &fields.bcc,
        ]
        .into_iter()
        .flatten()
        {
            if items.is_empty() {
                return Err(ImapSyncError::InvalidInput);
            }
            if items.len() > MAX_ADDRS {
                return Err(ImapSyncError::TooLarge);
            }
            count += items.len();
            for item in items {
                for v in [item.name(), item.route(), item.mailbox(), item.host()]
                    .into_iter()
                    .flatten()
                {
                    bytes += v.len();
                }
            }
        }
        if count > MAX_ADDRS * 2 || bytes > 64 * 1024 {
            return Err(ImapSyncError::TooLarge);
        }
        Ok(Self { fields })
    }
    pub fn fields(&self) -> &ImapEnvelopeInput {
        &self.fields
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapMessageMetadata {
    uid: u32,
    flags: Vec<String>,
    internal_date_utc_ms: i64,
    size: u32,
    envelope: ImapEnvelope,
}
impl fmt::Debug for ImapMessageMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapMessageMetadata { <redacted> }")
    }
}
impl ImapMessageMetadata {
    pub fn new(
        uid: u32,
        mut flags: Vec<String>,
        internal_date_utc_ms: i64,
        size: u32,
        envelope: ImapEnvelope,
    ) -> Result<Self, ImapSyncError> {
        if uid == 0
            || internal_date_utc_ms % 1000 != 0
            || !(-62_135_596_800_000..=253_402_300_799_000).contains(&internal_date_utc_ms)
        {
            return Err(ImapSyncError::InvalidInput);
        }
        if flags.len() > MAX_FLAGS {
            return Err(ImapSyncError::TooLarge);
        }
        let mut total = 0;
        for flag in &mut flags {
            if flag.len() > MAX_FLAG_BYTES {
                return Err(ImapSyncError::TooLarge);
            }
            let canonical = flag.to_ascii_lowercase();
            if !valid_flag(&canonical) {
                return Err(ImapSyncError::InvalidInput);
            }
            *flag = canonical;
            total += flag.len();
        }
        if flags.len() > MAX_FLAGS || total > MAX_FLAG_TOTAL {
            return Err(ImapSyncError::TooLarge);
        }
        flags.retain(|flag| flag != "\\recent");
        flags.sort();
        flags.dedup();
        Ok(Self {
            uid,
            flags,
            internal_date_utc_ms,
            size,
            envelope,
        })
    }
    pub fn uid(&self) -> u32 {
        self.uid
    }
    pub fn flags(&self) -> &[String] {
        &self.flags
    }
    pub fn internal_date_utc_ms(&self) -> i64 {
        self.internal_date_utc_ms
    }
    pub fn size(&self) -> u32 {
        self.size
    }
    pub fn envelope(&self) -> &ImapEnvelope {
        &self.envelope
    }
    pub fn to_storage_json(&self) -> Result<Vec<u8>, ImapSyncError> {
        let bytes = serde_json::to_vec(&StorageMessage::from_message(self)?)
            .map_err(|_| ImapSyncError::InvalidStorage)?;
        if bytes.len() > MAX_METADATA_JSON {
            return Err(ImapSyncError::TooLarge);
        }
        Ok(bytes)
    }
    pub fn from_storage_json(bytes: &[u8]) -> Result<Self, ImapSyncError> {
        if bytes.len() > MAX_METADATA_JSON {
            return Err(ImapSyncError::TooLarge);
        }
        let wire: StorageMessage =
            serde_json::from_slice(bytes).map_err(|_| ImapSyncError::InvalidStorage)?;
        let value = wire
            .into_message()
            .map_err(|_| ImapSyncError::InvalidStorage)?;
        if value.to_storage_json()?.as_slice() != bytes {
            return Err(ImapSyncError::InvalidStorage);
        }
        Ok(value)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapUidPage {
    request: ImapUidPageRequest,
    uid_validity: u32,
    uid_next_at_start: u32,
    messages: Vec<ImapMessageMetadata>,
}
impl fmt::Debug for ImapUidPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapUidPage { <redacted> }")
    }
}
impl ImapUidPage {
    pub fn new(
        request: ImapUidPageRequest,
        uid_validity: u32,
        uid_next_at_start: u32,
        mut messages: Vec<ImapMessageMetadata>,
    ) -> Result<Self, ImapSyncError> {
        if uid_validity == 0 || uid_next_at_start == 0 {
            return Err(ImapSyncError::InvalidInput);
        }
        if let Some(expected) = request.expected_uid_validity()
            && expected != uid_validity
        {
            return Err(ImapSyncError::UidValidityChanged);
        }
        let ceiling = uid_next_at_start - 1;
        if messages.len() > MAX_PAGE_MESSAGES {
            return Err(ImapSyncError::TooLarge);
        }
        if request.after_uid() > ceiling {
            return Err(ImapSyncError::CursorInvalid);
        }
        for message in &messages {
            if message.uid() <= request.after_uid()
                || message.uid() > request.after_uid().saturating_add(PAGE_WIDTH).min(ceiling)
            {
                return Err(ImapSyncError::CursorInvalid);
            }
        }
        messages.sort_by_key(ImapMessageMetadata::uid);
        if messages.windows(2).any(|w| w[0].uid() == w[1].uid()) {
            return Err(ImapSyncError::InvalidInput);
        }
        let page = Self {
            request,
            uid_validity,
            uid_next_at_start,
            messages,
        };
        page.to_storage_json()?;
        Ok(page)
    }
    pub fn request(&self) -> &ImapUidPageRequest {
        &self.request
    }
    pub fn uid_validity(&self) -> u32 {
        self.uid_validity
    }
    pub fn uid_next_at_start(&self) -> u32 {
        self.uid_next_at_start
    }
    pub fn covered_through_uid(&self) -> u32 {
        self.request
            .after_uid()
            .saturating_add(PAGE_WIDTH)
            .min(self.uid_next_at_start - 1)
    }
    pub fn messages(&self) -> &[ImapMessageMetadata] {
        &self.messages
    }
    pub fn to_storage_json(&self) -> Result<Vec<u8>, ImapSyncError> {
        let wire = StoragePage::from_page(self)?;
        let bytes = serde_json::to_vec(&wire).map_err(|_| ImapSyncError::InvalidStorage)?;
        if bytes.len() > MAX_PAGE_JSON {
            Err(ImapSyncError::TooLarge)
        } else {
            Ok(bytes)
        }
    }
    pub fn from_storage_json(bytes: &[u8]) -> Result<Self, ImapSyncError> {
        if bytes.len() > MAX_PAGE_JSON {
            return Err(ImapSyncError::TooLarge);
        }
        let wire: StoragePage =
            serde_json::from_slice(bytes).map_err(|_| ImapSyncError::InvalidStorage)?;
        let page = wire.into_page()?;
        if page.to_storage_json()?.as_slice() != bytes {
            return Err(ImapSyncError::InvalidStorage);
        }
        Ok(page)
    }
}

fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_ID
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn valid_flag(s: &str) -> bool {
    if s.is_empty() || !s.is_ascii() {
        return false;
    }
    if s.starts_with('\\') {
        return matches!(
            s,
            "\\answered" | "\\flagged" | "\\deleted" | "\\seen" | "\\draft" | "\\recent"
        );
    }
    s.bytes().all(|b| {
        b.is_ascii_graphic()
            && !matches!(
                b,
                b'(' | b')' | b'{' | b'}' | b'%' | b'*' | b'"' | b'\\' | b']'
            )
    })
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoragePage {
    schema_version: u8,
    account_id: String,
    endpoint_version: u64,
    wire_hex: String,
    encoding: String,
    expected_uid_validity: Option<u32>,
    after_uid: u32,
    uid_validity: u32,
    uid_next_at_start: u32,
    messages: Vec<StorageMessage>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StorageMessage {
    schema_version: u8,
    uid: u32,
    flags: Vec<String>,
    internal_date_utc_ms: i64,
    size: u32,
    envelope: StorageEnvelope,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageEnvelope {
    date: Option<String>,
    subject: Option<String>,
    in_reply_to: Option<String>,
    message_id: Option<String>,
    from: Option<Vec<StorageAddress>>,
    sender: Option<Vec<StorageAddress>>,
    reply_to: Option<Vec<StorageAddress>>,
    to: Option<Vec<StorageAddress>>,
    cc: Option<Vec<StorageAddress>>,
    bcc: Option<Vec<StorageAddress>>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageAddress {
    name: Option<String>,
    route: Option<String>,
    mailbox: Option<String>,
    host: Option<String>,
}

impl StoragePage {
    fn from_page(p: &ImapUidPage) -> Result<Self, ImapSyncError> {
        Ok(Self {
            schema_version: 1,
            account_id: p.request.account_id.clone(),
            endpoint_version: p.request.endpoint_version,
            wire_hex: hex(p.request.mailbox.wire_name()),
            encoding: match p.request.mailbox.encoding() {
                MailboxEncoding::Utf8 => "utf8",
                MailboxEncoding::ModifiedUtf7 => "modified_utf7",
            }
            .into(),
            expected_uid_validity: p.request.expected_uid_validity,
            after_uid: p.request.after_uid,
            uid_validity: p.uid_validity,
            uid_next_at_start: p.uid_next_at_start,
            messages: p
                .messages
                .iter()
                .map(StorageMessage::from_message)
                .collect::<Result<_, _>>()?,
        })
    }
    fn into_page(self) -> Result<ImapUidPage, ImapSyncError> {
        if self.schema_version != 1 {
            return Err(ImapSyncError::InvalidStorage);
        };
        let wire = decode_hex(&self.wire_hex)?;
        let enc = match self.encoding.as_str() {
            "utf8" => MailboxEncoding::Utf8,
            "modified_utf7" => MailboxEncoding::ModifiedUtf7,
            _ => return Err(ImapSyncError::InvalidStorage),
        };
        let mailbox =
            ImapMailboxName::from_wire(&wire, enc).map_err(|_| ImapSyncError::InvalidStorage)?;
        let req = ImapUidPageRequest::new(
            &self.account_id,
            self.endpoint_version,
            mailbox,
            self.expected_uid_validity,
            self.after_uid,
        )
        .map_err(|_| ImapSyncError::InvalidStorage)?;
        ImapUidPage::new(
            req,
            self.uid_validity,
            self.uid_next_at_start,
            self.messages
                .into_iter()
                .map(StorageMessage::into_message)
                .collect::<Result<_, _>>()?,
        )
        .map_err(|_| ImapSyncError::InvalidStorage)
    }
}
impl StorageMessage {
    pub(crate) fn from_message(m: &ImapMessageMetadata) -> Result<Self, ImapSyncError> {
        Ok(Self {
            schema_version: 1,
            uid: m.uid,
            flags: m.flags.clone(),
            internal_date_utc_ms: m.internal_date_utc_ms,
            size: m.size,
            envelope: StorageEnvelope::from_envelope(m.envelope())?,
        })
    }
    pub(crate) fn into_message(self) -> Result<ImapMessageMetadata, ImapSyncError> {
        if self.schema_version != 1 {
            return Err(ImapSyncError::InvalidStorage);
        };
        ImapMessageMetadata::new(
            self.uid,
            self.flags,
            self.internal_date_utc_ms,
            self.size,
            self.envelope.into_envelope()?,
        )
    }
}
impl StorageEnvelope {
    fn from_envelope(e: &ImapEnvelope) -> Result<Self, ImapSyncError> {
        let f = e.fields();
        Ok(Self {
            date: enc(&f.date),
            subject: enc(&f.subject),
            in_reply_to: enc(&f.in_reply_to),
            message_id: enc(&f.message_id),
            from: addrs(&f.from)?,
            sender: addrs(&f.sender)?,
            reply_to: addrs(&f.reply_to)?,
            to: addrs(&f.to)?,
            cc: addrs(&f.cc)?,
            bcc: addrs(&f.bcc)?,
        })
    }
    fn into_envelope(self) -> Result<ImapEnvelope, ImapSyncError> {
        ImapEnvelope::new(ImapEnvelopeInput {
            date: dec(self.date)?,
            subject: dec(self.subject)?,
            in_reply_to: dec(self.in_reply_to)?,
            message_id: dec(self.message_id)?,
            from: dec_addrs(self.from)?,
            sender: dec_addrs(self.sender)?,
            reply_to: dec_addrs(self.reply_to)?,
            to: dec_addrs(self.to)?,
            cc: dec_addrs(self.cc)?,
            bcc: dec_addrs(self.bcc)?,
        })
    }
}
fn enc(v: &Option<Vec<u8>>) -> Option<String> {
    v.as_ref().map(|x| hex(x))
}
fn dec(v: Option<String>) -> Result<Option<Vec<u8>>, ImapSyncError> {
    v.map(|x| {
        if x.len() > MAX_RAW * 2 {
            return Err(ImapSyncError::TooLarge);
        }
        decode_hex(&x)
    })
    .transpose()
}
fn addrs(
    v: &Option<Vec<ImapEnvelopeAddress>>,
) -> Result<Option<Vec<StorageAddress>>, ImapSyncError> {
    v.as_ref()
        .map(|xs| {
            xs.iter()
                .map(|a| {
                    Ok(StorageAddress {
                        name: enc(&a.name),
                        route: enc(&a.route),
                        mailbox: enc(&a.mailbox),
                        host: enc(&a.host),
                    })
                })
                .collect()
        })
        .transpose()
}
fn dec_addrs(
    v: Option<Vec<StorageAddress>>,
) -> Result<Option<Vec<ImapEnvelopeAddress>>, ImapSyncError> {
    v.map(|xs| {
        xs.into_iter()
            .map(|a| {
                ImapEnvelopeAddress::new(dec(a.name)?, dec(a.route)?, dec(a.mailbox)?, dec(a.host)?)
            })
            .collect()
    })
    .transpose()
}
pub(crate) fn hex(v: &[u8]) -> String {
    v.iter().map(|b| format!("{b:02x}")).collect()
}
pub(crate) fn decode_hex(s: &str) -> Result<Vec<u8>, ImapSyncError> {
    if !s.len().is_multiple_of(2)
        || !s
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !(b'A'..=b'F').contains(&b))
    {
        return Err(ImapSyncError::InvalidStorage);
    };
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ImapSyncError::InvalidStorage))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mailboxes::MailboxEncoding;

    fn request(after: u32, validity: Option<u32>) -> ImapUidPageRequest {
        ImapUidPageRequest::new(
            "acct",
            1,
            ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::Utf8).unwrap(),
            validity,
            after,
        )
        .unwrap()
    }
    fn envelope() -> ImapEnvelope {
        ImapEnvelope::new(ImapEnvelopeInput {
            subject: Some(Vec::new()),
            to: Some(vec![
                ImapEnvelopeAddress::new(
                    Some(b"n".to_vec()),
                    None,
                    Some(b"u".to_vec()),
                    Some(b"h".to_vec()),
                )
                .unwrap(),
            ]),
            ..Default::default()
        })
        .unwrap()
    }
    fn message(uid: u32) -> ImapMessageMetadata {
        ImapMessageMetadata::new(
            uid,
            vec!["\\Seen".into(), "custom".into(), "\\Recent".into()],
            1_700_000_000_000,
            0,
            envelope(),
        )
        .unwrap()
    }

    #[test]
    fn ext04_request_rejects_invalid_account_and_preserves_cursor_fields() {
        assert_eq!(
            ImapUidPageRequest::new(
                "",
                1,
                ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::Utf8).unwrap(),
                None,
                0
            ),
            Err(ImapSyncError::InvalidInput)
        );
        let r = request(4, Some(7));
        assert_eq!(
            (
                r.account_id(),
                r.endpoint_version(),
                r.expected_uid_validity(),
                r.after_uid()
            ),
            ("acct", 1, Some(7), 4)
        );
    }

    #[test]
    fn ext04_page_uses_fixed_coverage_and_rejects_sequence_like_values() {
        let page = ImapUidPage::new(request(4, Some(7)), 7, 100, vec![message(9)]).unwrap();
        assert_eq!(page.covered_through_uid(), 68);
        assert_eq!(page.messages()[0].uid(), 9);
        assert_eq!(page.messages()[0].flags(), &["\\seen", "custom"]);
        assert!(matches!(
            ImapUidPage::new(request(68, Some(7)), 7, 69, vec![message(68)]),
            Err(ImapSyncError::CursorInvalid)
        ));
        assert!(matches!(
            ImapUidPage::new(request(4, Some(8)), 7, 100, vec![]),
            Err(ImapSyncError::UidValidityChanged)
        ));
    }

    #[test]
    fn sync02_storage_roundtrip_preserves_nil_empty_and_raw_bytes() {
        let page = ImapUidPage::new(request(0, None), 7, 2, vec![message(1)]).unwrap();
        let bytes = page.to_storage_json().unwrap();
        assert_eq!(ImapUidPage::from_storage_json(&bytes), Ok(page.clone()));
        let mut noncanonical = bytes.clone();
        noncanonical.push(b' ');
        assert_eq!(
            ImapUidPage::from_storage_json(&noncanonical),
            Err(ImapSyncError::InvalidStorage)
        );
        assert_eq!(
            page.messages()[0].envelope().fields().subject,
            Some(Vec::new())
        );
        assert_eq!(page.messages()[0].envelope().fields().date, None);
    }

    #[test]
    fn sync02_message_bounds_reject_bad_date_and_flags() {
        assert!(
            ImapMessageMetadata::new(1, vec!["bad flag".into()], 1_700_000_000_000, 0, envelope())
                .is_err()
        );
        assert!(ImapMessageMetadata::new(1, vec![], 1_700_000_000_001, 0, envelope()).is_err());
        assert!(
            ImapMessageMetadata::new(
                1,
                vec!["\\unknown".into()],
                1_700_000_000_000,
                0,
                envelope()
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    #[test]
    fn sync02_metadata_codec_preserves_nil_empty_group_and_rejects_noncanonical_inputs() {
        let group = ImapEnvelopeAddress::new(None, None, Some(b"group".to_vec()), None).unwrap();
        let end = ImapEnvelopeAddress::new(None, None, None, None).unwrap();
        let e = ImapEnvelope::new(ImapEnvelopeInput {
            subject: Some(vec![0, 255, b'z']),
            in_reply_to: Some(vec![]),
            to: Some(vec![group, end]),
            ..Default::default()
        })
        .unwrap();
        let m =
            ImapMessageMetadata::new(2, vec!["beta".into(), "ALPHA".into()], -1000, 2, e).unwrap();
        let raw = m.to_storage_json().unwrap();
        assert_eq!(ImapMessageMetadata::from_storage_json(&raw).unwrap(), m);
        let text = String::from_utf8(raw).unwrap();
        let faults = [
            format!("{text} "),
            text.replace("\"date\":null,", ""),
            text.replace("00ff7a", "00FF7a"),
            text.replace("\"alpha\",\"beta\"", "\"beta\",\"alpha\""),
            text.replace("\"subject\":", "\"subject\":null,\"subject\":"),
            text.replace(
                "\"schema_version\":1,\"uid\":2",
                "\"uid\":2,\"schema_version\":1",
            ),
            text.replace("\"uid\":2", "\"uid\":null"),
            text.replace("\"schema_version\":1", "\"schema_version\":2"),
        ];
        for invalid in faults {
            assert_eq!(
                ImapMessageMetadata::from_storage_json(invalid.as_bytes()),
                Err(ImapSyncError::InvalidStorage)
            );
        }
        assert_eq!(
            ImapMessageMetadata::from_storage_json(&vec![b' '; MAX_METADATA_JSON + 1]),
            Err(ImapSyncError::TooLarge)
        );
    }
    #[test]
    fn ext04_near_u32_limit_and_empty_existing_cursor_do_not_wrap() {
        for (after, next, covered) in [(u32::MAX - 10, u32::MAX, u32::MAX - 1), (0, 1, 0)] {
            let r = ImapUidPageRequest::new(
                "a",
                1,
                ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::Utf8).unwrap(),
                Some(1),
                after,
            )
            .unwrap();
            let p = ImapUidPage::new(r, 1, next, vec![]).unwrap();
            assert_eq!(p.covered_through_uid(), covered);
            assert_eq!(
                ImapUidPage::from_storage_json(&p.to_storage_json().unwrap()).unwrap(),
                p
            );
        }
    }

    #[test]
    fn ext04_inbox_case_is_not_part_of_cursor_identity_or_codec() {
        let name = ImapMailboxName::from_wire(b"inbox", MailboxEncoding::Utf8).unwrap();
        let r = ImapUidPageRequest::new("a", 1, name, None, 0).unwrap();
        let p = ImapUidPage::new(r, 1, 1, vec![]).unwrap();
        assert_eq!(
            ImapUidPage::from_storage_json(&p.to_storage_json().unwrap()).unwrap(),
            p
        );
    }
    #[test]
    fn sync02_all_cardinality_and_byte_ceilings_have_too_large_error() {
        let name = ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::Utf8).unwrap();
        let r = ImapUidPageRequest::new("a", 1, name, None, 0).unwrap();
        assert_eq!(
            ImapUidPage::new(r, 1, 100, vec![metadata(1, vec![]).unwrap(); 65]),
            Err(ImapSyncError::TooLarge)
        );
        let address = ImapEnvelopeAddress::new(None, None, None, None).unwrap();
        assert_eq!(
            ImapEnvelope::new(ImapEnvelopeInput {
                to: Some(vec![address; 129]),
                ..Default::default()
            }),
            Err(ImapSyncError::TooLarge)
        );
        assert_eq!(
            metadata(1, vec!["a".repeat(129)]),
            Err(ImapSyncError::TooLarge)
        );
    }

    fn metadata(uid: u32, flags: Vec<String>) -> Result<ImapMessageMetadata, ImapSyncError> {
        ImapMessageMetadata::new(
            uid,
            flags,
            0,
            0,
            ImapEnvelope::new(ImapEnvelopeInput::default()).unwrap(),
        )
    }
    #[test]
    fn ext04_page_rejects_uid_above_covered_range_even_below_snapshot_ceiling() {
        let request = ImapUidPageRequest::new(
            "a",
            1,
            ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::Utf8).unwrap(),
            None,
            0,
        )
        .unwrap();
        assert_eq!(
            ImapUidPage::new(request, 1, 1000, vec![metadata(65, vec![]).unwrap()]),
            Err(ImapSyncError::CursorInvalid)
        );
    }
    #[test]
    fn ext04_flags_input_bound_applies_before_recent_removal() {
        assert_eq!(
            metadata(1, vec!["\\Recent".into(); 65]),
            Err(ImapSyncError::TooLarge)
        );
    }
}
