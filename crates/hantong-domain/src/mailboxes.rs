//! Bounded, provider-neutral IMAP mailbox names and snapshots.

use crate::capabilities::ImapCapabilities;
use serde::{Deserialize, Serialize};
use std::fmt;

const MAX_WIRE_NAME_BYTES: usize = 1024;
const MAX_DISPLAY_NAME_BYTES: usize = 4096;
const MAX_MAILBOXES: usize = 1024;
const MAX_WIRE_TOTAL_BYTES: usize = 1024 * 1024;
const MAX_STORAGE_JSON_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailboxEncoding {
    ModifiedUtf7,
    Utf8,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SpecialUseRole {
    All,
    Archive,
    Drafts,
    Flagged,
    Junk,
    Sent,
    Trash,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailboxChildren {
    Unknown,
    HasChildren,
    HasNoChildren,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailboxError {
    InvalidInput,
    InvalidEncoding,
    InvalidName,
    InvalidDelimiter,
    DuplicateMailbox,
    MixedEncoding,
    TooManyMailboxes,
    SnapshotTooLarge,
    InvalidStorage,
}

impl fmt::Display for MailboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidInput => "invalid mailbox input",
            Self::InvalidEncoding => "invalid mailbox encoding",
            Self::InvalidName => "invalid mailbox name",
            Self::InvalidDelimiter => "invalid mailbox delimiter",
            Self::DuplicateMailbox => "duplicate mailbox",
            Self::MixedEncoding => "mixed mailbox encoding",
            Self::TooManyMailboxes => "too many mailboxes",
            Self::SnapshotTooLarge => "mailbox snapshot too large",
            Self::InvalidStorage => "invalid mailbox storage",
        })
    }
}
impl std::error::Error for MailboxError {}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapMailboxName {
    wire: Vec<u8>,
    display: String,
    encoding: MailboxEncoding,
}
impl fmt::Debug for ImapMailboxName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapMailboxName { <redacted> }")
    }
}
impl ImapMailboxName {
    pub fn from_wire(bytes: &[u8], encoding: MailboxEncoding) -> Result<Self, MailboxError> {
        if !(1..=MAX_WIRE_NAME_BYTES).contains(&bytes.len()) {
            return Err(MailboxError::InvalidName);
        }
        let display = match encoding {
            MailboxEncoding::Utf8 => decode_utf8(bytes)?,
            MailboxEncoding::ModifiedUtf7 => decode_modified_utf7(bytes)?,
        };
        if display.len() > MAX_DISPLAY_NAME_BYTES {
            return Err(MailboxError::InvalidName);
        }
        let wire = if display.eq_ignore_ascii_case("INBOX") {
            b"INBOX".to_vec()
        } else {
            bytes.to_vec()
        };
        Ok(Self {
            wire,
            display,
            encoding,
        })
    }
    pub fn wire_name(&self) -> &[u8] {
        &self.wire
    }
    pub fn display_name(&self) -> &str {
        &self.display
    }
    pub fn is_inbox(&self) -> bool {
        self.display.eq_ignore_ascii_case("INBOX")
    }
    pub fn encoding(&self) -> MailboxEncoding {
        self.encoding
    }

    fn from_persisted(
        wire: &[u8],
        display: &str,
        encoding: MailboxEncoding,
    ) -> Result<Self, MailboxError> {
        let decoded = Self::from_wire(wire, encoding)?;
        if decoded.is_inbox() {
            if wire != b"INBOX" || !display.eq_ignore_ascii_case("INBOX") {
                return Err(MailboxError::InvalidStorage);
            }
            return Ok(Self {
                wire: b"INBOX".to_vec(),
                display: display.to_owned(),
                encoding,
            });
        }
        if decoded.display != display {
            return Err(MailboxError::InvalidStorage);
        }
        Ok(decoded)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapMailboxDescriptor {
    name: ImapMailboxName,
    delimiter: Option<char>,
    no_select: bool,
    non_existent: bool,
    children: MailboxChildren,
    roles: Vec<SpecialUseRole>,
}
impl fmt::Debug for ImapMailboxDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImapMailboxDescriptor")
            .field("name", &"<redacted>")
            .field("delimiter", &self.delimiter)
            .field("selectable", &self.selectable())
            .field("roles", &self.roles)
            .finish()
    }
}
impl ImapMailboxDescriptor {
    pub fn new(
        name: ImapMailboxName,
        delimiter: Option<char>,
        no_select: bool,
        non_existent: bool,
        children: MailboxChildren,
        mut roles: Vec<SpecialUseRole>,
    ) -> Result<Self, MailboxError> {
        if let Some(value) = delimiter
            && (!value.is_ascii() || !(0x20..=0x7e).contains(&(value as u8)))
        {
            return Err(MailboxError::InvalidDelimiter);
        }
        roles.sort_unstable();
        roles.dedup();
        Ok(Self {
            name,
            delimiter,
            no_select,
            non_existent,
            children,
            roles,
        })
    }
    pub fn name(&self) -> &ImapMailboxName {
        &self.name
    }
    pub fn delimiter(&self) -> Option<char> {
        self.delimiter
    }
    pub fn non_existent(&self) -> bool {
        self.non_existent
    }
    pub fn no_select(&self) -> bool {
        self.no_select
    }
    pub fn children(&self) -> MailboxChildren {
        self.children
    }
    pub fn roles(&self) -> &[SpecialUseRole] {
        &self.roles
    }
    pub fn selectable(&self) -> bool {
        !self.no_select && !self.non_existent
    }
    pub fn has_role(&self, role: SpecialUseRole) -> bool {
        self.roles.binary_search(&role).is_ok()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapMailboxSnapshot {
    account_id: String,
    settings_version: u64,
    capabilities: ImapCapabilities,
    mailboxes: Vec<ImapMailboxDescriptor>,
    roles_complete: bool,
    encoding: Option<MailboxEncoding>,
}
impl fmt::Debug for ImapMailboxSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImapMailboxSnapshot")
            .field("account_id", &"<redacted>")
            .field("settings_version", &self.settings_version)
            .field("mailbox_count", &self.mailboxes.len())
            .field("roles_complete", &self.roles_complete)
            .finish_non_exhaustive()
    }
}
impl ImapMailboxSnapshot {
    pub fn new(
        account_id: &str,
        settings_version: u64,
        capabilities: ImapCapabilities,
        mut mailboxes: Vec<ImapMailboxDescriptor>,
    ) -> Result<Self, MailboxError> {
        if !valid_account_id(account_id)
            || settings_version == 0
            || settings_version > i64::MAX as u64
        {
            return Err(MailboxError::InvalidInput);
        }
        if mailboxes.len() > MAX_MAILBOXES {
            return Err(MailboxError::TooManyMailboxes);
        }
        let total = mailboxes
            .iter()
            .try_fold(0usize, |sum, item| sum.checked_add(item.name.wire.len()))
            .ok_or(MailboxError::SnapshotTooLarge)?;
        if total > MAX_WIRE_TOTAL_BYTES {
            return Err(MailboxError::SnapshotTooLarge);
        }
        mailboxes.sort_by(|left, right| left.name.wire_name().cmp(right.name.wire_name()));
        if mailboxes
            .windows(2)
            .any(|pair| pair[0].name.wire_name() == pair[1].name.wire_name())
        {
            return Err(MailboxError::DuplicateMailbox);
        }
        let encoding = mailboxes.first().map(|item| item.name.encoding());
        if mailboxes
            .iter()
            .any(|item| Some(item.name.encoding()) != encoding)
        {
            return Err(MailboxError::MixedEncoding);
        }
        Ok(Self {
            account_id: account_id.to_owned(),
            settings_version,
            capabilities,
            mailboxes,
            roles_complete: false,
            encoding,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn settings_version(&self) -> u64 {
        self.settings_version
    }
    pub fn capabilities(&self) -> &ImapCapabilities {
        &self.capabilities
    }
    pub fn mailboxes(&self) -> &[ImapMailboxDescriptor] {
        &self.mailboxes
    }
    pub fn special_use_complete(&self) -> bool {
        self.roles_complete
    }
    pub fn encoding(&self) -> Option<MailboxEncoding> {
        self.encoding
    }

    pub fn to_storage_json(&self) -> Result<Vec<u8>, MailboxError> {
        let value = StorageSnapshot {
            schema_version: 1,
            account_id: self.account_id.clone(),
            settings_version: self.settings_version,
            capabilities: self.capabilities.to_ids(),
            special_use_complete: self.roles_complete,
            mailboxes: self
                .mailboxes
                .iter()
                .map(StorageMailbox::from_descriptor)
                .collect(),
        };
        let bytes = serde_json::to_vec(&value).map_err(|_| MailboxError::InvalidStorage)?;
        if bytes.len() > MAX_STORAGE_JSON_BYTES {
            return Err(MailboxError::SnapshotTooLarge);
        }
        Ok(bytes)
    }

    pub fn from_storage_json(bytes: &[u8]) -> Result<Self, MailboxError> {
        if bytes.is_empty() || bytes.len() > MAX_STORAGE_JSON_BYTES {
            return Err(MailboxError::InvalidStorage);
        }
        let value: StorageSnapshot =
            serde_json::from_slice(bytes).map_err(|_| MailboxError::InvalidStorage)?;
        if value.schema_version != 1 || value.special_use_complete {
            return Err(MailboxError::InvalidStorage);
        }
        if value.mailboxes.len() > MAX_MAILBOXES {
            return Err(MailboxError::TooManyMailboxes);
        }
        let capabilities = ImapCapabilities::from_ids(&value.capabilities)
            .map_err(|_| MailboxError::InvalidStorage)?;
        let mut descriptors = Vec::with_capacity(value.mailboxes.len());
        let mut previous_wire: Option<Vec<u8>> = None;
        for mailbox in value.mailboxes {
            let wire = decode_lower_hex(&mailbox.wire_hex)?;
            if let Some(previous) = &previous_wire
                && previous.as_slice() >= wire.as_slice()
            {
                return Err(if previous == &wire {
                    MailboxError::DuplicateMailbox
                } else {
                    MailboxError::InvalidStorage
                });
            }
            previous_wire = Some(wire.clone());
            let encoding = parse_encoding(&mailbox.encoding)?;
            let name = ImapMailboxName::from_persisted(&wire, &mailbox.display_name, encoding)?;
            let delimiter = match mailbox.delimiter {
                None => None,
                Some(value) => {
                    let mut chars = value.chars();
                    let Some(delimiter) = chars.next() else {
                        return Err(MailboxError::InvalidStorage);
                    };
                    if chars.next().is_some() {
                        return Err(MailboxError::InvalidStorage);
                    }
                    Some(delimiter)
                }
            };
            let children = parse_children(&mailbox.children)?;
            let roles = parse_roles(&mailbox.roles)?;
            let descriptor = ImapMailboxDescriptor::new(
                name,
                delimiter,
                mailbox.no_select,
                mailbox.non_existent,
                children,
                roles,
            )?;
            descriptors.push(descriptor);
        }
        let snapshot = Self::new(
            &value.account_id,
            value.settings_version,
            capabilities,
            descriptors,
        )?;
        if snapshot.to_storage_json()? != bytes {
            return Err(MailboxError::InvalidStorage);
        }
        Ok(snapshot)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageSnapshot {
    schema_version: u8,
    account_id: String,
    settings_version: u64,
    capabilities: Vec<String>,
    special_use_complete: bool,
    mailboxes: Vec<StorageMailbox>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageMailbox {
    wire_hex: String,
    display_name: String,
    encoding: String,
    delimiter: Option<String>,
    no_select: bool,
    non_existent: bool,
    children: String,
    roles: Vec<String>,
}

impl StorageMailbox {
    fn from_descriptor(value: &ImapMailboxDescriptor) -> Self {
        Self {
            wire_hex: lower_hex(value.name.wire_name()),
            display_name: value.name.display_name().to_owned(),
            encoding: encoding_name(value.name.encoding()).to_owned(),
            delimiter: value.delimiter.map(|delimiter| delimiter.to_string()),
            no_select: value.no_select,
            non_existent: value.non_existent,
            children: children_name(value.children()).to_owned(),
            roles: value
                .roles()
                .iter()
                .map(|role| role_name(*role).to_owned())
                .collect(),
        }
    }
}

fn encoding_name(value: MailboxEncoding) -> &'static str {
    match value {
        MailboxEncoding::ModifiedUtf7 => "modified_utf7",
        MailboxEncoding::Utf8 => "utf8",
    }
}
fn parse_encoding(value: &str) -> Result<MailboxEncoding, MailboxError> {
    match value {
        "modified_utf7" => Ok(MailboxEncoding::ModifiedUtf7),
        "utf8" => Ok(MailboxEncoding::Utf8),
        _ => Err(MailboxError::InvalidStorage),
    }
}
fn children_name(value: MailboxChildren) -> &'static str {
    match value {
        MailboxChildren::Unknown => "unknown",
        MailboxChildren::HasChildren => "has_children",
        MailboxChildren::HasNoChildren => "has_no_children",
    }
}
fn parse_children(value: &str) -> Result<MailboxChildren, MailboxError> {
    match value {
        "unknown" => Ok(MailboxChildren::Unknown),
        "has_children" => Ok(MailboxChildren::HasChildren),
        "has_no_children" => Ok(MailboxChildren::HasNoChildren),
        _ => Err(MailboxError::InvalidStorage),
    }
}
fn role_name(value: SpecialUseRole) -> &'static str {
    match value {
        SpecialUseRole::All => "all",
        SpecialUseRole::Archive => "archive",
        SpecialUseRole::Drafts => "drafts",
        SpecialUseRole::Flagged => "flagged",
        SpecialUseRole::Junk => "junk",
        SpecialUseRole::Sent => "sent",
        SpecialUseRole::Trash => "trash",
    }
}
fn parse_roles(values: &[String]) -> Result<Vec<SpecialUseRole>, MailboxError> {
    let mut roles = Vec::with_capacity(values.len());
    for value in values {
        roles.push(match value.as_str() {
            "all" => SpecialUseRole::All,
            "archive" => SpecialUseRole::Archive,
            "drafts" => SpecialUseRole::Drafts,
            "flagged" => SpecialUseRole::Flagged,
            "junk" => SpecialUseRole::Junk,
            "sent" => SpecialUseRole::Sent,
            "trash" => SpecialUseRole::Trash,
            _ => return Err(MailboxError::InvalidStorage),
        });
    }
    if roles.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(MailboxError::InvalidStorage);
    }
    Ok(roles)
}
fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 0x0f) as usize] as char);
    }
    result
}
fn decode_lower_hex(value: &str) -> Result<Vec<u8>, MailboxError> {
    if value.is_empty() || !value.len().is_multiple_of(2) || value.len() / 2 > MAX_WIRE_NAME_BYTES {
        return Err(MailboxError::InvalidStorage);
    }
    let bytes = value.as_bytes();
    let mut result = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = hex_value(pair[0]).ok_or(MailboxError::InvalidStorage)?;
        let low = hex_value(pair[1]).ok_or(MailboxError::InvalidStorage)?;
        result.push((high << 4) | low);
    }
    Ok(result)
}
fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn valid_account_id(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}
fn decode_utf8(bytes: &[u8]) -> Result<String, MailboxError> {
    let value = std::str::from_utf8(bytes).map_err(|_| MailboxError::InvalidEncoding)?;
    if value.chars().any(char::is_control) {
        return Err(MailboxError::InvalidName);
    }
    Ok(value.to_owned())
}

fn decode_modified_utf7(bytes: &[u8]) -> Result<String, MailboxError> {
    let mut output = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'&' {
            if !(0x20..=0x7e).contains(&bytes[index]) {
                return Err(MailboxError::InvalidEncoding);
            }
            output.push(bytes[index] as char);
            index += 1;
            continue;
        }
        if index + 1 < bytes.len() && bytes[index + 1] == b'-' {
            output.push('&');
            index += 2;
            continue;
        }
        index += 1;
        let start = index;
        while index < bytes.len() && bytes[index] != b'-' {
            index += 1;
        }
        if index == bytes.len() {
            return Err(MailboxError::InvalidEncoding);
        }
        if start == index {
            return Err(MailboxError::InvalidEncoding);
        }
        let encoded = &bytes[start..index];
        index += 1;
        if encoded.contains(&b'=') {
            return Err(MailboxError::InvalidEncoding);
        }
        let mut bits = 0u32;
        let mut bit_count = 0u8;
        let mut utf16 = Vec::new();
        for &byte in encoded {
            let value = modified_base64_value(byte).ok_or(MailboxError::InvalidEncoding)?;
            bits = (bits << 6) | u32::from(value);
            bit_count += 6;
            while bit_count >= 16 {
                bit_count -= 16;
                utf16.push(((bits >> bit_count) & 0xffff) as u16);
            }
        }
        if bit_count != 0 && (bits & ((1u32 << bit_count) - 1)) != 0 {
            return Err(MailboxError::InvalidEncoding);
        }
        if utf16.is_empty() {
            return Err(MailboxError::InvalidEncoding);
        }
        if encoded.len() != (utf16.len() * 16).div_ceil(6) {
            return Err(MailboxError::InvalidEncoding);
        }
        let decoded = String::from_utf16(&utf16).map_err(|_| MailboxError::InvalidEncoding)?;
        if decoded
            .chars()
            .any(|c| c.is_control() || c.is_ascii_graphic() || c == ' ')
        {
            return Err(MailboxError::InvalidEncoding);
        }
        output.push_str(&decoded);
    }
    if output.is_empty() || output.len() > MAX_DISPLAY_NAME_BYTES {
        return Err(MailboxError::InvalidName);
    }
    Ok(output)
}
fn modified_base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b',' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ext06_long_modified_utf7_segments_and_surrogate_pairs_preserve_text() {
        for (wire, expected) in [
            ("&1VyuANg93gA-", "한글😀"),
            ("&ZeVnLIqeThZ1TA-", "日本語世界"),
            (
                "&byJvIm8ibyJvIm8ibyJvIm8ibyJvIm8ibyJvIm8ibyJvIm8ibyJvIm8ibyJvIm8i-",
                "漢漢漢漢漢漢漢漢漢漢漢漢漢漢漢漢漢漢漢漢漢漢漢漢",
            ),
            ("A&AOQ-B/&-x/&ZeVnLIqe-", "AäB/&x/日本語"),
        ] {
            let value = name(wire.as_bytes(), MailboxEncoding::ModifiedUtf7);
            assert_eq!(value.display_name(), expected);
            assert_eq!(value.wire_name(), wire.as_bytes());
        }
        for wire in ["&zzzzzz-", "&++++++-", "&1VyuANg93gB-", "&ZeVnLIqeA-"] {
            assert!(
                ImapMailboxName::from_wire(wire.as_bytes(), MailboxEncoding::ModifiedUtf7).is_err()
            );
        }
    }
    #[test]
    fn ext06_snapshot_name_and_binding_limits_are_enforced_without_truncation() {
        let caps = ImapCapabilities::from_features([]);
        for bytes in [
            b"".as_slice(),
            b"line\r\nname",
            b"nul\0name",
            b"\x7f",
            &[0xff],
        ] {
            for encoding in [MailboxEncoding::ModifiedUtf7, MailboxEncoding::Utf8] {
                assert!(ImapMailboxName::from_wire(bytes, encoding).is_err());
            }
        }
        assert!(ImapMailboxName::from_wire(&vec![b'x'; 1025], MailboxEncoding::Utf8).is_err());
        let long = name(&vec![b'x'; 1024], MailboxEncoding::Utf8);
        assert_eq!(long.wire_name().len(), 1024);
        assert!(!format!("{long:?}").contains("xxxx"));
        for delimiter in [Some('\0'), Some('\n'), Some('한')] {
            assert_eq!(
                ImapMailboxDescriptor::new(
                    long.clone(),
                    delimiter,
                    false,
                    false,
                    MailboxChildren::Unknown,
                    vec![]
                ),
                Err(MailboxError::InvalidDelimiter)
            );
        }
        assert!(
            ImapMailboxDescriptor::new(
                long,
                Some(' '),
                false,
                true,
                MailboxChildren::Unknown,
                vec![]
            )
            .is_ok_and(|m| !m.selectable())
        );
        for (id, version) in [
            ("", 1),
            ("acct/escape", 1),
            ("acct", 0),
            ("acct", i64::MAX as u64 + 1),
        ] {
            assert_eq!(
                ImapMailboxSnapshot::new(id, version, caps, vec![]),
                Err(MailboxError::InvalidInput)
            );
        }
        assert!(ImapMailboxSnapshot::new("acct", i64::MAX as u64, caps, vec![]).is_ok());
        let rows = (0..1024)
            .map(|i| {
                ImapMailboxDescriptor::new(
                    name(format!("box-{i:04}").as_bytes(), MailboxEncoding::Utf8),
                    None,
                    false,
                    false,
                    MailboxChildren::Unknown,
                    vec![],
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ImapMailboxSnapshot::new("acct", 1, caps, rows.clone())
                .unwrap()
                .mailboxes()
                .len(),
            1024
        );
        let mut too_many = rows;
        too_many.push(too_many[0].clone());
        assert_eq!(
            ImapMailboxSnapshot::new("acct", 1, caps, too_many),
            Err(MailboxError::TooManyMailboxes)
        );
    }
    fn name(value: &[u8], encoding: MailboxEncoding) -> ImapMailboxName {
        ImapMailboxName::from_wire(value, encoding)
            .unwrap_or_else(|error| panic!("{value:?}: {error:?}"))
    }

    #[test]
    fn ext06_modified_utf7_decodes_rfc_examples_and_ampersand() {
        assert_eq!(
            name(b"INBOX", MailboxEncoding::ModifiedUtf7).display_name(),
            "INBOX"
        );
        assert_eq!(
            name(b"&AOQ-", MailboxEncoding::ModifiedUtf7).display_name(),
            "ä",
            "single shifted segment"
        );
        assert_eq!(
            name(b"&-", MailboxEncoding::ModifiedUtf7).display_name(),
            "&"
        );
        assert_eq!(
            name(b"&AOQ-B", MailboxEncoding::ModifiedUtf7).display_name(),
            "äB",
            "shifted then atom"
        );
    }
    #[test]
    fn sec08_modified_utf7_rejects_bad_padding_surrogate_and_encoded_ascii() {
        for value in [
            b"&A-".as_slice(),
            b"&A==",
            b"&2AA-",
            b"&AGE-",
            b"&A",
            b"&AOQA-",
        ] {
            assert!(
                ImapMailboxName::from_wire(value, MailboxEncoding::ModifiedUtf7).is_err(),
                "accepted {:?}",
                value
            );
        }
    }
    #[test]
    fn ext06_utf8_preserves_exact_text_and_inbox_identity_only() {
        let inbox = name("inbox".as_bytes(), MailboxEncoding::Utf8);
        assert_eq!(inbox.display_name(), "inbox");
        assert!(inbox.is_inbox());
        assert_eq!(inbox.wire_name(), b"INBOX");
        let composed = name("e\u{301}".as_bytes(), MailboxEncoding::Utf8);
        assert_eq!(composed.display_name(), "e\u{301}");
    }
    #[test]
    fn ext06_descriptor_normalizes_roles_and_selectability() {
        let descriptor = ImapMailboxDescriptor::new(
            name(b"Archive", MailboxEncoding::Utf8),
            Some('/'),
            true,
            false,
            MailboxChildren::Unknown,
            vec![SpecialUseRole::Archive, SpecialUseRole::Archive],
        )
        .unwrap();
        assert!(!descriptor.selectable());
        assert_eq!(descriptor.roles(), &[SpecialUseRole::Archive]);
    }
    #[test]
    fn ext06_snapshot_sorts_and_rejects_duplicate_inbox_identity() {
        let caps = ImapCapabilities::from_features(std::iter::empty());
        let descriptor = |value| {
            ImapMailboxDescriptor::new(
                name(value, MailboxEncoding::Utf8),
                None,
                false,
                false,
                MailboxChildren::Unknown,
                vec![],
            )
            .unwrap()
        };
        let inbox = descriptor(b"INBOX");
        let lower = descriptor(b"inbox");
        assert_eq!(
            ImapMailboxSnapshot::new("acct-1", 1, caps, vec![inbox.clone(), lower]),
            Err(MailboxError::DuplicateMailbox)
        );
        let snapshot =
            ImapMailboxSnapshot::new("acct-1", 1, caps, vec![descriptor(b"z"), inbox]).unwrap();
        assert_eq!(snapshot.mailboxes()[0].name().display_name(), "INBOX");
        assert_eq!(snapshot.mailboxes()[1].name().display_name(), "z");
    }

    #[test]
    fn ext06_storage_roundtrip_preserves_encoding_and_mixed_case_inbox() {
        let inbox = ImapMailboxDescriptor::new(
            name(b"inbox", MailboxEncoding::Utf8),
            Some('/'),
            false,
            false,
            MailboxChildren::HasNoChildren,
            vec![],
        )
        .unwrap();
        let snapshot =
            ImapMailboxSnapshot::new("acct", 1, ImapCapabilities::from_features([]), vec![inbox])
                .unwrap();
        let restored =
            ImapMailboxSnapshot::from_storage_json(&snapshot.to_storage_json().unwrap()).unwrap();
        assert_eq!(restored.mailboxes()[0].name().display_name(), "inbox");
        assert_eq!(restored.mailboxes()[0].name().wire_name(), b"INBOX");
        assert_eq!(restored.encoding(), Some(MailboxEncoding::Utf8));

        for (name_value, no_select, non_existent) in [
            (b"nil".as_slice(), false, false),
            (b"noselect".as_slice(), true, false),
            (b"nonexistent".as_slice(), false, true),
            (b"both".as_slice(), true, true),
        ] {
            let descriptor = ImapMailboxDescriptor::new(
                name(name_value, MailboxEncoding::Utf8),
                None,
                no_select,
                non_existent,
                MailboxChildren::Unknown,
                vec![],
            )
            .unwrap();
            let snapshot = ImapMailboxSnapshot::new(
                "acct",
                1,
                ImapCapabilities::from_features([]),
                vec![descriptor],
            )
            .unwrap();
            let restored =
                ImapMailboxSnapshot::from_storage_json(&snapshot.to_storage_json().unwrap())
                    .unwrap();
            let restored_descriptor = &restored.mailboxes()[0];
            assert_eq!(restored_descriptor.no_select(), no_select);
            assert_eq!(restored_descriptor.non_existent(), non_existent);
            assert_eq!(restored_descriptor.delimiter(), None);
        }
    }

    #[test]
    fn ext06_storage_codec_rejects_noncanonical_and_hostile_json() {
        let snapshot =
            ImapMailboxSnapshot::new("acct", 1, ImapCapabilities::from_features([]), vec![])
                .unwrap();
        let bytes = snapshot.to_storage_json().unwrap();
        assert_eq!(ImapMailboxSnapshot::from_storage_json(&bytes), Ok(snapshot));
        assert!(ImapMailboxSnapshot::from_storage_json(
            br#"{"schema_version":1,"account_id":"acct","settings_version":1,"capabilities":[],"special_use_complete":false,"mailboxes":[],"mailboxes":[]}"#
        )
        .is_err());
        let mut with_space = bytes.clone();
        with_space.push(b' ');
        assert!(ImapMailboxSnapshot::from_storage_json(&with_space).is_err());
        let missing_delimiter = br#"{"schema_version":1,"account_id":"acct","settings_version":1,"capabilities":[],"special_use_complete":false,"mailboxes":[{"wire_hex":"6e696c","display_name":"nil","encoding":"utf8","no_select":false,"non_existent":false,"children":"unknown","roles":[]}]}"#;
        assert!(ImapMailboxSnapshot::from_storage_json(missing_delimiter).is_err());
        let nested_unknown = bytes
            .strip_suffix(b"[]}")
            .map(|prefix| {
                let mut value = prefix.to_vec();
                value.extend_from_slice(br#"[{"wire_hex":"6e696c","display_name":"nil","encoding":"utf8","delimiter":null,"no_select":false,"non_existent":false,"children":"unknown","roles":["unknown"]}]}"#);
                value
            })
            .unwrap();
        assert!(ImapMailboxSnapshot::from_storage_json(&nested_unknown).is_err());
        assert_eq!(
            ImapMailboxSnapshot::from_storage_json(&vec![b' '; MAX_STORAGE_JSON_BYTES + 1]),
            Err(MailboxError::InvalidStorage)
        );
    }

    #[test]
    fn ext06_storage_codec_rejects_mixed_name_encodings() {
        let utf8 = ImapMailboxDescriptor::new(
            name(b"utf8", MailboxEncoding::Utf8),
            None,
            false,
            false,
            MailboxChildren::Unknown,
            vec![],
        )
        .unwrap();
        let utf7 = ImapMailboxDescriptor::new(
            name(b"utf7", MailboxEncoding::ModifiedUtf7),
            None,
            false,
            false,
            MailboxChildren::Unknown,
            vec![],
        )
        .unwrap();
        assert_eq!(
            ImapMailboxSnapshot::new(
                "acct",
                1,
                ImapCapabilities::from_features([]),
                vec![utf8, utf7],
            ),
            Err(MailboxError::MixedEncoding)
        );
    }
    #[test]
    fn ext06_storage_exact_roundtrip_handles_unicode_escapes_and_full_mailbox_limit() {
        for encoding in [MailboxEncoding::ModifiedUtf7, MailboxEncoding::Utf8] {
            let unicode = match encoding {
                MailboxEncoding::ModifiedUtf7 => b"&1VyuANg93gA-".as_slice(),
                MailboxEncoding::Utf8 => "한글😀".as_bytes(),
            };
            let mut boxes = Vec::new();
            for wire in [unicode, b"inBoX", b"quote\"and\\slash"] {
                boxes.push(
                    ImapMailboxDescriptor::new(
                        name(wire, encoding),
                        Some('\\'),
                        true,
                        true,
                        MailboxChildren::HasChildren,
                        vec![SpecialUseRole::Sent, SpecialUseRole::All],
                    )
                    .unwrap(),
                );
            }
            for i in boxes.len()..MAX_MAILBOXES {
                boxes.push(
                    ImapMailboxDescriptor::new(
                        name(format!("folder-{i:04}").as_bytes(), encoding),
                        None,
                        false,
                        false,
                        MailboxChildren::Unknown,
                        vec![],
                    )
                    .unwrap(),
                );
            }
            let snapshot = ImapMailboxSnapshot::new(
                "acct",
                7,
                ImapCapabilities::from_features([crate::capabilities::ImapFeature::Idle]),
                boxes,
            )
            .unwrap();
            let bytes = snapshot.to_storage_json().unwrap();
            assert_eq!(
                ImapMailboxSnapshot::from_storage_json(&bytes).unwrap(),
                snapshot
            );
        }
    }

    #[test]
    fn ext06_storage_structurally_valid_json_mutations_fail_closed() {
        let boxes = [b"a".as_slice(), b"z"]
            .into_iter()
            .map(|wire| {
                ImapMailboxDescriptor::new(
                    name(wire, MailboxEncoding::Utf8),
                    None,
                    false,
                    false,
                    MailboxChildren::Unknown,
                    vec![SpecialUseRole::All, SpecialUseRole::Sent],
                )
                .unwrap()
            })
            .collect();
        let snapshot = ImapMailboxSnapshot::new(
            "acct",
            1,
            ImapCapabilities::from_features([
                crate::capabilities::ImapFeature::Idle,
                crate::capabilities::ImapFeature::Move,
            ]),
            boxes,
        )
        .unwrap();
        let bytes = snapshot.to_storage_json().unwrap();
        let original = String::from_utf8(bytes.clone()).unwrap();
        let reordered = original.replacen(
            "\"schema_version\":1,\"account_id\":\"acct\"",
            "\"account_id\":\"acct\",\"schema_version\":1",
            1,
        );
        assert_ne!(original, reordered);
        assert!(ImapMailboxSnapshot::from_storage_json(reordered.as_bytes()).is_err());
        for case in 0..14 {
            let mut value: StorageSnapshot = serde_json::from_slice(&bytes).unwrap();
            match case {
                0 => value.mailboxes.swap(0, 1),
                1 => {
                    value.mailboxes[0].wire_hex = "6A".into();
                    value.mailboxes[0].display_name = "j".into();
                }
                2 => value.mailboxes[0].display_name = "wrong".into(),
                3 => value.mailboxes[0].encoding = "unknown".into(),
                4 => value.mailboxes[0].roles.reverse(),
                5 => value.mailboxes[0].roles.push("sent".into()),
                6 => value.mailboxes[0].children = "unknown_child".into(),
                7 => value.mailboxes[0].delimiter = Some("ab".into()),
                8 => value.capabilities.reverse(),
                9 => value.capabilities.push("mail.read".into()),
                10 => value.special_use_complete = true,
                11 => value.mailboxes[1].encoding = "modified_utf7".into(),
                12 => {
                    value.mailboxes[1].wire_hex = value.mailboxes[0].wire_hex.clone();
                    value.mailboxes[1].display_name = value.mailboxes[0].display_name.clone();
                }
                13 => value.settings_version = i64::MAX as u64 + 1,
                _ => unreachable!(),
            }
            let corrupt = serde_json::to_vec(&value).unwrap();
            assert_ne!(bytes, corrupt, "case {case}");
            assert!(
                ImapMailboxSnapshot::from_storage_json(&corrupt).is_err(),
                "case {case}"
            );
        }
        for corrupt in [
            original.replacen(
                "\"schema_version\":1",
                "\"schema_version\":1,\"unknown\":0",
                1,
            ),
            original.replacen(
                "\"wire_hex\":\"61\"",
                "\"wire_hex\":\"61\",\"wire_hex\":\"61\"",
                1,
            ),
            original.replacen(
                "\"no_select\":false",
                "\"no_select\":false,\"unknown\":0",
                1,
            ),
        ] {
            assert!(ImapMailboxSnapshot::from_storage_json(corrupt.as_bytes()).is_err());
        }
    }
}
