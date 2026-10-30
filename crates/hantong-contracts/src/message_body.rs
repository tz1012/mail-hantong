//! Contract for reading one bounded message body.
//!
//! A body is addressed by the complete IMAP identity.  In particular, a UID
//! is never accepted without the mailbox UIDVALIDITY that created it.

use crate::{ErrorCode, uid_page::UidMailboxIdentity};
use std::fmt;

pub const MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Clone, Eq, PartialEq)]
pub struct MessageBodyRequest {
    account_id: String,
    mailbox: UidMailboxIdentity,
    uidvalidity: u32,
    uid: u32,
    max_bytes: usize,
}

impl fmt::Debug for MessageBodyRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MessageBodyRequest")
            .field("account_id", &"<redacted>")
            .field("mailbox", &"<redacted>")
            .field("uidvalidity", &self.uidvalidity)
            .field("uid", &self.uid)
            .field("max_bytes", &self.max_bytes)
            .finish()
    }
}

impl MessageBodyRequest {
    pub fn new(
        account_id: &str,
        mailbox: UidMailboxIdentity,
        uidvalidity: u32,
        uid: u32,
        max_bytes: usize,
    ) -> Result<Self, ErrorCode> {
        if account_id.is_empty()
            || account_id.len() > 128
            || !account_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || uidvalidity == 0
            || uid == 0
            || !(1..=MAX_BODY_BYTES).contains(&max_bytes)
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account_id: account_id.into(),
            mailbox,
            uidvalidity,
            uid,
            max_bytes,
        })
    }
    pub fn account_ids(&self) -> &[String] {
        std::slice::from_ref(&self.account_id)
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn mailbox(&self) -> &UidMailboxIdentity {
        &self.mailbox
    }
    pub const fn uidvalidity(&self) -> u32 {
        self.uidvalidity
    }
    pub const fn uid(&self) -> u32 {
        self.uid
    }
    pub const fn max_bytes(&self) -> usize {
        self.max_bytes
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct MessageBody {
    key: MessageBodyRequest,
    bytes: Vec<u8>,
    truncated: bool,
}

impl fmt::Debug for MessageBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MessageBody")
            .field("key", &self.key)
            .field("bytes", &self.bytes.len())
            .field("truncated", &self.truncated)
            .finish()
    }
}

impl MessageBody {
    pub fn new(
        key: MessageBodyRequest,
        bytes: Vec<u8>,
        truncated: bool,
    ) -> Result<Self, ErrorCode> {
        if bytes.len() > key.max_bytes() {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            key,
            bytes,
            truncated,
        })
    }
    pub fn key(&self) -> &MessageBodyRequest {
        &self.key
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

#[path = "message_body/mime.rs"]
mod mime;
/// Decode bounded MIME text; unsupported binary or charset is explicit.
pub fn render_message_text(bytes: &[u8]) -> Result<String, ErrorCode> {
    mime::render(bytes)
}
/// Legacy plain sanitization; never interpret terminal commands.
pub fn render_terminal_text(bytes: &[u8]) -> String {
    sanitize_terminal_text(&String::from_utf8_lossy(bytes))
}
pub fn sanitize_terminal_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(MAX_BODY_BYTES));
    let mut esc = false;
    for ch in text.chars() {
        if esc {
            if ch.is_ascii_alphabetic() || ch == '~' {
                esc = false;
            }
            continue;
        }
        if ch == '\u{1b}' {
            esc = true;
            continue;
        }
        if ch == '\r'
            || matches!(ch as u32,0x061c|0x200e..=0x200f|0x202a..=0x202e|0x2066..=0x2069)
            || ch.is_control() && ch != '\n' && ch != '\t'
        {
            continue;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uid_page::UidMailboxIdentity;

    fn key() -> MessageBodyRequest {
        MessageBodyRequest::new(
            "acct",
            UidMailboxIdentity::new("494e424f58", crate::uid_page::UidMailboxEncoding::Utf8)
                .unwrap(),
            42,
            9,
            32,
        )
        .unwrap()
    }
    #[test]
    fn body_requires_complete_identity_and_bounded_limit() {
        let mailbox =
            UidMailboxIdentity::new("494e424f58", crate::uid_page::UidMailboxEncoding::Utf8)
                .unwrap();
        assert!(MessageBodyRequest::new("acct", mailbox.clone(), 0, 9, 1).is_err());
        assert!(MessageBodyRequest::new("acct", mailbox, 42, 9, MAX_BODY_BYTES + 1).is_err());
        assert_eq!(key().uidvalidity(), 42);
    }
    #[test]
    fn terminal_rendering_never_emits_markup_or_ansi() {
        let rendered = render_message_text(
            b"Content-Type: text/html\r\n\r\nHello <script>alert(1)</script>\x1b[31mred\x1b[0m\n",
        )
        .unwrap();
        assert_eq!(rendered, "Hello red\n");
    }
}
