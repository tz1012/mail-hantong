//! Strict parser for one IMAP `UID FETCH ... BODY.PEEK[]` response.
//!
//! This module is intentionally transport-independent so fake provider tests
//! exercise the same binding and bounds as the Windows transport.
use hantong_contracts::message_body::{MessageBody, MessageBodyRequest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapBodyError {
    Probe(crate::MailProbeError),
    UidValidityChanged,
    NotFound,
    TooLarge,
}
impl From<crate::MailProbeError> for ImapBodyError {
    fn from(e: crate::MailProbeError) -> Self {
        Self::Probe(e)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BodyReadError {
    Malformed,
    TooLarge,
    IdentityMismatch,
}

/// Parse one bounded BODY.PEEK literal.  The response must contain exactly
/// the requested UID and UIDVALIDITY and is never treated as a read flag
/// mutation.  No HTML or terminal interpretation occurs here.
pub fn parse_body_peek(
    request: MessageBodyRequest,
    observed_uidvalidity: u32,
    frame: &[u8],
) -> Result<MessageBody, BodyReadError> {
    if observed_uidvalidity != request.uidvalidity() {
        return Err(BodyReadError::IdentityMismatch);
    }
    if frame.len() > request.max_bytes().saturating_add(512) {
        return Err(BodyReadError::TooLarge);
    }
    let rest = frame.strip_prefix(b"* ").ok_or(BodyReadError::Malformed)?;
    let (sequence, rest) = number(rest)?;
    if sequence == 0 || sequence > u32::MAX as u64 {
        return Err(BodyReadError::Malformed);
    }
    let rest = rest
        .strip_prefix(b" FETCH (")
        .ok_or(BodyReadError::Malformed)?;
    let (before_uid, rest) = if let Some(rest) = rest.strip_prefix(b"UID ") {
        let (uid, rest) = number(rest)?;
        let rest = rest
            .strip_prefix(b" BODY[] {")
            .ok_or(BodyReadError::Malformed)?;
        (Some(uid), rest)
    } else {
        (
            None,
            rest.strip_prefix(b"BODY[] {")
                .ok_or(BodyReadError::Malformed)?,
        )
    };
    let (length, rest) = number(rest)?;
    let rest = rest
        .strip_prefix(b"}\r\n")
        .ok_or(BodyReadError::Malformed)?;
    if length > request.max_bytes() as u64 {
        return Err(BodyReadError::TooLarge);
    }
    let length = usize::try_from(length).map_err(|_| BodyReadError::TooLarge)?;
    let suffix = rest.get(length..).ok_or(BodyReadError::Malformed)?;
    let uid = if let Some(uid) = before_uid {
        if suffix != b")\r\n" {
            return Err(BodyReadError::Malformed);
        }
        uid
    } else {
        let (uid, end) = number(
            suffix
                .strip_prefix(b" UID ")
                .ok_or(BodyReadError::Malformed)?,
        )?;
        if end != b")\r\n" {
            return Err(BodyReadError::Malformed);
        }
        uid
    };
    if uid != u64::from(request.uid()) {
        return Err(BodyReadError::IdentityMismatch);
    }
    let body = rest[..length].to_vec();
    MessageBody::new(request, body, false).map_err(|_| BodyReadError::Malformed)
}

fn number(input: &[u8]) -> Result<(u64, &[u8]), BodyReadError> {
    let end = input
        .iter()
        .position(|b| !b.is_ascii_digit())
        .ok_or(BodyReadError::Malformed)?;
    if end == 0 || (end > 1 && input[0] == b'0') {
        return Err(BodyReadError::Malformed);
    }
    let n = std::str::from_utf8(&input[..end])
        .map_err(|_| BodyReadError::Malformed)?
        .parse()
        .map_err(|_| BodyReadError::Malformed)?;
    Ok((n, &input[end..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::uid_page::{UidMailboxEncoding, UidMailboxIdentity};

    fn request() -> MessageBodyRequest {
        MessageBodyRequest::new(
            "acct",
            UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::Utf8).unwrap(),
            42,
            9,
            64,
        )
        .unwrap()
    }
    #[test]
    fn body_peek_accepts_server_reordered_uid_but_rejects_duplicate_attributes() {
        assert_eq!(
            parse_body_peek(request(), 42, b"* 4 FETCH (BODY[] {5}\r\nhello UID 9)\r\n")
                .unwrap()
                .bytes(),
            b"hello"
        );
        assert_eq!(
            parse_body_peek(request(), 42, b"* 4 FETCH (BODY[] {5}\r\nhello UID 8)\r\n"),
            Err(BodyReadError::IdentityMismatch)
        );
        assert_eq!(
            parse_body_peek(
                request(),
                42,
                b"* 4 FETCH (UID 9 BODY[] {5}\r\nhello UID 9)\r\n"
            ),
            Err(BodyReadError::Malformed)
        );
    }
    #[test]
    fn body_peek_binds_exact_uid_and_validity_without_marking_read() {
        let result =
            parse_body_peek(request(), 42, b"* 77 FETCH (UID 9 BODY[] {5}\r\nhello)\r\n").unwrap();
        assert_eq!(result.bytes(), b"hello");
    }
    #[test]
    fn body_peek_rejects_wrong_identity_and_oversized_literal() {
        assert_eq!(
            parse_body_peek(request(), 42, b"* 1 FETCH (UID 8 BODY[] {5}\r\nhello)\r\n"),
            Err(BodyReadError::IdentityMismatch)
        );
        assert_eq!(
            parse_body_peek(request(), 42, b"* 1 FETCH (UID 9 BODY[] {65}\r\nhello)\r\n"),
            Err(BodyReadError::TooLarge)
        );
    }
}
