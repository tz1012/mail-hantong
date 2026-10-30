//! SMTP submission kept inside the Secrets boundary.
//!
//! Credential plaintext is unsealed only in this crate and is never returned
//! to Core, the CLI, logs, or an error. The message envelope is immutable and
//! validated before a network connection is attempted.

use crate::{SecretError, SecretStore, validate_account};
use hantong_domain::mail_connection::SmtpEndpointConfig;
use std::fmt;

#[cfg(windows)]
mod windows;

#[derive(Clone, Eq, PartialEq)]
pub struct SmtpSendEnvelope {
    pub recipients: Vec<String>,
    pub subject: String,
    pub body: String,
}

impl fmt::Debug for SmtpSendEnvelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmtpSendEnvelope")
            .field("recipients", &"<redacted>")
            .field("subject", &"<redacted>")
            .field("body", &"<redacted>")
            .finish()
    }
}

impl SmtpSendEnvelope {
    fn validate(&self) -> Result<(), SmtpSendError> {
        if self.recipients.is_empty()
            || self.recipients.len() > 100
            || self.subject.len() > 998
            || self.body.len() > 1024 * 1024
            || self
                .subject
                .bytes()
                .any(|b| b == 0 || b == b'\r' || b == b'\n')
            || self.body.bytes().any(|b| b == 0)
            || self
                .recipients
                .iter()
                .any(|r| r.is_empty() || r.len() > 320 || r.bytes().any(|b| b < 0x20 || b == 0x7f))
        {
            return Err(SmtpSendError::InvalidEnvelope);
        }
        if self.recipients.iter().any(|r| {
            !r.contains('@')
                || r.starts_with('@')
                || r.ends_with('@')
                || r.matches('@').count() != 1
        }) {
            return Err(SmtpSendError::InvalidRecipient);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmtpSendResult {
    Accepted,
    Rejected,
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmtpSendError {
    InvalidAccount,
    InvalidEnvelope,
    InvalidRecipient,
    UnsupportedPlatform,
    Connection,
    Tls,
    Authentication,
    Protocol,
    Timeout,
    Uncertain,
    Credential(SecretError),
}

impl fmt::Display for SmtpSendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidAccount => "invalid mail account",
            Self::InvalidEnvelope => "invalid message envelope",
            Self::InvalidRecipient => "invalid recipient",
            Self::UnsupportedPlatform => "SMTP submission unsupported on this platform",
            Self::Connection => "SMTP connection failed",
            Self::Tls => "SMTP TLS verification failed",
            Self::Authentication => "SMTP authentication failed",
            Self::Protocol => "SMTP protocol failed",
            Self::Timeout => "SMTP submission timed out",
            Self::Uncertain => "SMTP delivery result is unknown",
            Self::Credential(_) => "SMTP credential unavailable for these settings",
        })
    }
}
impl std::error::Error for SmtpSendError {}

// Keep every wire line ASCII and bounded. Lettre owns SMTP dot stuffing;
// callers must not insert DATA terminators or reuse this as a raw command API.
#[cfg(any(windows, test))]
fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let a = chunk[0] as usize;
        let b = chunk.get(1).copied().unwrap_or(0) as usize;
        let c = chunk.get(2).copied().unwrap_or(0) as usize;
        out.push(TABLE[a >> 2] as char);
        out.push(TABLE[((a & 3) << 4) | (b >> 4)] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((b & 15) << 2) | (c >> 6)] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[c & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(any(windows, test))]
fn rfc_date(seconds: u64) -> Result<String, SmtpSendError> {
    let mut days = seconds / 86400;
    if days > 2932896 {
        return Err(SmtpSendError::InvalidEnvelope);
    }
    let weekday = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][(days % 7) as usize];
    let mut year = 1970;
    loop {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let length = if leap { 366 } else { 365 };
        if days < length {
            break;
        }
        days -= length;
        year += 1;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0;
    while days >= months[month] {
        days -= months[month];
        month += 1;
    }
    let names = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    Ok(format!(
        "{weekday}, {:02} {} {year} {:02}:{:02}:{:02} +0000",
        days + 1,
        names[month],
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60
    ))
}

#[cfg(any(windows, test))]
fn encode_message(
    sender: &str,
    envelope: &SmtpSendEnvelope,
    seconds: u64,
    nanos: u32,
) -> Result<crate::SensitiveBuffer, SmtpSendError> {
    envelope.validate()?;
    if sender.len() > 320
        || !sender.is_ascii()
        || sender.matches('@').count() != 1
        || sender.starts_with('@')
        || sender.ends_with('@')
        || sender
            .bytes()
            .any(|b| b <= 32 || b >= 127 || b"<>(),;\\\"".contains(&b))
    {
        return Err(SmtpSendError::InvalidRecipient);
    }
    let domain = sender
        .split_once('@')
        .ok_or(SmtpSendError::InvalidRecipient)?
        .1;
    let mut message = format!(
        "Date: {}\r\nMessage-ID: <{seconds:x}.{nanos:x}@{domain}>\r\nFrom: <{sender}>\r\nTo: ",
        rfc_date(seconds)?
    );
    for (index, address) in envelope.recipients.iter().enumerate() {
        if index > 0 {
            message.push_str(",\r\n ");
        }
        message.push('<');
        message.push_str(address);
        message.push('>');
    }
    message.push_str("\r\nSubject: ");
    let mut word = String::new();
    let mut words = Vec::new();
    for ch in envelope.subject.chars() {
        if word.len() + ch.len_utf8() > 45 {
            words.push(base64(word.as_bytes()));
            word.clear();
        }
        word.push(ch);
    }
    if !word.is_empty() {
        words.push(base64(word.as_bytes()));
    }
    for (index, word) in words.iter().enumerate() {
        if index > 0 {
            message.push_str("\r\n ");
        }
        message.push_str("=?UTF-8?B?");
        message.push_str(word);
        message.push_str("?=");
    }
    message.push_str("\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n");
    for chunk in envelope.body.as_bytes().chunks(57) {
        message.push_str(&base64(chunk));
        message.push_str("\r\n");
    }
    Ok(crate::SensitiveBuffer(message.into_bytes()))
}

#[cfg(test)]
mod mime_tests {
    use super::*;
    #[test]
    fn ext03_unicode_and_smtp_terminators_are_encoded_as_inert_mime_data() {
        let envelope = SmtpSendEnvelope {
            recipients: vec!["to@example.invalid".into()],
            subject: "안녕".into(),
            body: ".\nQUIT\r\n안녕".into(),
        };
        let value = encode_message("from@example.invalid", &envelope, 0, 1).unwrap();
        let text = std::str::from_utf8(value.as_slice()).unwrap();
        assert!(text.contains("Subject: =?UTF-8?B?7JWI64WV?="));
        assert!(text.contains("Date: Thu, 01 Jan 1970 00:00:00 +0000"));
        assert!(!text.contains("\r\nQUIT"));
        assert!(text.is_ascii());
        assert!(text.split("\r\n").all(|line| line.len() <= 998));
        assert!(!text.replace("\r\n", "").contains(['\r', '\n']));
    }
    #[test]
    fn ext03_header_injection_and_calendar_boundaries_are_rejected_or_normalized() {
        let envelope = SmtpSendEnvelope {
            recipients: vec!["to@example.invalid".into()],
            subject: "x".repeat(998),
            body: "hello".into(),
        };
        assert!(
            encode_message(
                "from@example.invalid\r\nBcc: hidden@example.invalid",
                &envelope,
                0,
                0
            )
            .is_err()
        );
        assert_eq!(
            rfc_date(1577836800).unwrap(),
            "Wed, 01 Jan 2020 00:00:00 +0000"
        );
        let message = encode_message("from@example.invalid", &envelope, 1577836800, 0).unwrap();
        assert!(
            std::str::from_utf8(message.as_slice())
                .unwrap()
                .split("\r\n")
                .all(|l| l.len() <= 998)
        );
    }
}

impl SecretStore {
    /// Submits one immutable envelope. No automatic retry is performed here.
    pub async fn send_smtp(
        &self,
        account: &str,
        endpoint: &SmtpEndpointConfig,
        envelope: &SmtpSendEnvelope,
    ) -> Result<SmtpSendResult, SmtpSendError> {
        validate_account(account).map_err(|_| SmtpSendError::InvalidAccount)?;
        envelope.validate()?;
        #[cfg(windows)]
        {
            windows::send(self, account, endpoint, envelope).await
        }
        #[cfg(not(windows))]
        {
            let _ = endpoint;
            Err(SmtpSendError::UnsupportedPlatform)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope() -> SmtpSendEnvelope {
        SmtpSendEnvelope {
            recipients: vec!["person@example.test".into()],
            subject: "Subject".into(),
            body: "Body".into(),
        }
    }

    #[test]
    fn send01_envelope_debug_never_contains_message_data() {
        let value = format!("{:?}", envelope());
        assert!(!value.contains("person@example.test"));
        assert!(!value.contains("Subject"));
        assert!(!value.contains("Body"));
        assert!(value.contains("<redacted>"));
    }

    #[test]
    fn send01_envelope_rejects_header_injection_and_multiple_at_signs() {
        let mut value = envelope();
        value.subject = "bad\r\nBcc: attacker@example.test".into();
        assert_eq!(value.validate(), Err(SmtpSendError::InvalidEnvelope));
        let mut value = envelope();
        value.recipients = vec!["a@@example.test".into()];
        assert_eq!(value.validate(), Err(SmtpSendError::InvalidRecipient));
    }

    #[cfg(not(windows))]
    #[test]
    fn send01_non_windows_never_attempts_submission() {
        let root = std::env::temp_dir().join("mail-hantong-send-unsupported");
        let result = match SecretStore::open(root) {
            Ok(_) => panic!("unsupported platform unexpectedly opened secrets"),
            Err(error) => error,
        };
        assert_eq!(result, SecretError::UnsupportedPlatform);
    }
}
