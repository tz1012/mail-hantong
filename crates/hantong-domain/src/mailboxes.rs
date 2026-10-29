//! Bounded, provider-neutral IMAP mailbox names and snapshots.

use crate::capabilities::ImapCapabilities;
use std::fmt;

const MAX_WIRE_NAME_BYTES: usize = 1024;
const MAX_DISPLAY_NAME_BYTES: usize = 4096;
const MAX_MAILBOXES: usize = 1024;
const MAX_WIRE_TOTAL_BYTES: usize = 1024 * 1024;

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
    TooManyMailboxes,
    SnapshotTooLarge,
}

impl fmt::Display for MailboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidInput => "invalid mailbox input",
            Self::InvalidEncoding => "invalid mailbox encoding",
            Self::InvalidName => "invalid mailbox name",
            Self::InvalidDelimiter => "invalid mailbox delimiter",
            Self::DuplicateMailbox => "duplicate mailbox",
            Self::TooManyMailboxes => "too many mailboxes",
            Self::SnapshotTooLarge => "mailbox snapshot too large",
        })
    }
}
impl std::error::Error for MailboxError {}

#[derive(Clone, Eq, PartialEq)]
pub struct ImapMailboxName {
    wire: Vec<u8>,
    display: String,
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
        Ok(Self { wire, display })
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
        Ok(Self {
            account_id: account_id.to_owned(),
            settings_version,
            capabilities,
            mailboxes,
            roles_complete: false,
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
}
