//! Contract for reading one bounded message body.
//!
//! A body is addressed by the complete IMAP identity.  In particular, a UID
//! is never accepted without the mailbox UIDVALIDITY that created it.

use crate::{ErrorCode, uid_page::UidMailboxIdentity};
use std::fmt;

pub const MAX_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_ENVELOPE_FIELD_BYTES: usize = 8192;
pub const MAX_ENVELOPE_BYTES: usize = 32768;

/// Untrusted RFC822 envelope fields retained for display only.
#[derive(Clone, Eq, PartialEq)]
pub struct MessageEnvelope {
    subject: Option<String>,
    from: Option<String>,
    to: Option<String>,
    cc: Option<String>,
    date: Option<String>,
}

impl std::fmt::Debug for MessageEnvelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageEnvelope")
            .field("subject", &self.subject.as_ref().map(|_| "<redacted>"))
            .field("from", &self.from.as_ref().map(|_| "<redacted>"))
            .field("to", &self.to.as_ref().map(|_| "<redacted>"))
            .field("cc", &self.cc.as_ref().map(|_| "<redacted>"))
            .field("date", &self.date.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl MessageEnvelope {
    pub fn new(
        subject: Option<String>,
        from: Option<String>,
        to: Option<String>,
        cc: Option<String>,
        date: Option<String>,
    ) -> Result<Self, ErrorCode> {
        let values = [&subject, &from, &to, &cc, &date];
        if values.iter().filter_map(|v| v.as_ref()).any(|v| {
            v.len() > MAX_ENVELOPE_FIELD_BYTES
                || v.bytes()
                    .any(|b| b == 0 || (b.is_ascii_control() && b != b'\t'))
        }) || values
            .iter()
            .filter_map(|v| v.as_ref())
            .map(|v| v.len())
            .sum::<usize>()
            > MAX_ENVELOPE_BYTES
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            subject,
            from,
            to,
            cc,
            date,
        })
    }
    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }
    pub fn from(&self) -> Option<&str> {
        self.from.as_deref()
    }
    pub fn to(&self) -> Option<&str> {
        self.to.as_deref()
    }
    pub fn cc(&self) -> Option<&str> {
        self.cc.as_deref()
    }
    pub fn date(&self) -> Option<&str> {
        self.date.as_deref()
    }
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({"subject":self.subject,"from":self.from,"to":self.to,"cc":self.cc,"date":self.date})
    }
}

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
/// Decode MIME text with a platform-provided legacy charset decoder.
/// The callback is display-only and must return `None` for invalid input.
pub fn render_message_text_with_charset<F>(bytes: &[u8], decode: F) -> Result<String, ErrorCode>
where
    F: Fn(&[u8], &str) -> Option<String>,
{
    mime::render_with_charset(bytes, &decode)
}
/// Extract bounded RFC822 envelope fields without changing body semantics.
pub fn message_envelope(bytes: &[u8]) -> Result<Option<MessageEnvelope>, ErrorCode> {
    mime::envelope(bytes)
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

    #[test]
    fn mail03_envelope_projection_is_bounded_and_display_only() {
        let raw = b"Subject: Hello\r\nFrom: sender@example.test\r\nTo: recipient@example.test\r\nCc: copy@example.test\r\nDate: Tue, 3 Nov 2026 10:00:00 +0000\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nBody";
        let envelope = message_envelope(raw).unwrap().unwrap();
        assert_eq!(envelope.subject(), Some("Hello"));
        assert_eq!(envelope.from(), Some("sender@example.test"));
        assert_eq!(envelope.to(), Some("recipient@example.test"));
        assert_eq!(envelope.cc(), Some("copy@example.test"));
        assert_eq!(envelope.date(), Some("Tue, 3 Nov 2026 10:00:00 +0000"));
        assert!(format!("{envelope:?}").contains("<redacted>"));
    }

    #[test]
    fn mail03_envelope_projection_tolerates_absent_headers() {
        assert!(message_envelope(b"plain body").unwrap().is_none());
        assert!(
            MessageEnvelope::new(
                Some("x".repeat(MAX_ENVELOPE_FIELD_BYTES + 1)),
                None,
                None,
                None,
                None
            )
            .is_err()
        );
    }
}
