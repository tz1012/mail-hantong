//! Pure MIME preparation; encoding never sends mail or grants approval.
use crate::{
    SensitiveBuffer,
    smtp_send::{base64, rfc_date},
};
use hantong_contracts::{ErrorCode, google_write::ComposeSnapshot};

pub struct GoogleMimeMessage(SensitiveBuffer);
impl std::fmt::Debug for GoogleMimeMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GoogleMimeMessage(<redacted>)")
    }
}
impl GoogleMimeMessage {
    /// Mail bytes only, never tokens. Later transmission must pass native review.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

pub fn encode(
    compose: &ComposeSnapshot,
    verified_sender: &str,
    review_id: &[u8; 32],
    utc_seconds: u64,
) -> Result<GoogleMimeMessage, ErrorCode> {
    if review_id == &[0; 32]
        || verified_sender.is_empty()
        || verified_sender.len() > 320
        || !verified_sender.is_ascii()
        || verified_sender.matches('@').count() != 1
        || verified_sender.starts_with('@')
        || verified_sender.ends_with('@')
        || verified_sender
            .bytes()
            .any(|b| b <= 32 || b >= 127 || b"<>(),;\\\"".contains(&b))
    {
        return Err(ErrorCode::InvalidInput);
    }
    let id = hantong_contracts::wire_hex::encode(review_id);
    let boundary = format!("=_hantong_{id}");
    let mut out = format!(
        "Date: {}\r\nMessage-ID: <{id}@hantong.local>\r\nFrom: <{verified_sender}>\r\nTo: ",
        rfc_date(utc_seconds).map_err(|_| ErrorCode::InvalidInput)?
    );
    for (index, address) in compose.draft().recipients().iter().enumerate() {
        if index > 0 {
            out.push_str(",\r\n ");
        }
        out.push('<');
        out.push_str(address);
        out.push('>');
    }
    out.push_str("\r\nSubject: ");
    let mut current = String::new();
    let mut first = true;
    fn encoded_word(out: &mut String, text: &str, first: &mut bool) {
        if !*first {
            out.push_str("\r\n ");
        }
        *first = false;
        out.push_str("=?UTF-8?B?");
        out.push_str(&base64(text.as_bytes()));
        out.push_str("?=");
    }
    for ch in compose.draft().subject().chars() {
        if current.len() + ch.len_utf8() > 45 {
            encoded_word(&mut out, &current, &mut first);
            current.clear();
        }
        current.push(ch);
    }
    if !current.is_empty() {
        encoded_word(&mut out, &current, &mut first);
    }
    out.push_str("\r\nMIME-Version: 1.0\r\n");
    let multipart = !compose.attachments().is_empty();
    if multipart {
        out.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\r\n--{boundary}\r\n"
        ));
    }
    out.push_str(
        "Content-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n",
    );
    fn body(out: &mut String, bytes: &[u8]) {
        for chunk in bytes.chunks(57) {
            out.push_str(&base64(chunk));
            out.push_str("\r\n");
        }
        if bytes.is_empty() {
            out.push_str("\r\n");
        }
    }
    body(&mut out, compose.draft().body().as_bytes());
    for attachment in compose.attachments() {
        out.push_str(&format!("--{boundary}\r\nContent-Type: {}\r\nContent-Disposition: attachment; filename=\"{}\"\r\nContent-Transfer-Encoding: base64\r\n\r\n", attachment.media_type(), attachment.name()));
        body(&mut out, attachment.bytes());
    }
    if multipart {
        out.push_str(&format!("--{boundary}--\r\n"));
    }
    Ok(GoogleMimeMessage(SensitiveBuffer(out.into_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::{google_write::AttachmentSnapshot, mail_send::DraftCreateRequest};
    fn fixture(attachments: Vec<AttachmentSnapshot>) -> ComposeSnapshot {
        let draft = DraftCreateRequest::new(
            &format!("google_{}", "a".repeat(64)),
            vec!["receiver@example.invalid".into()],
            "한통 메일 송수신 테스트".into(),
            "한통 메일 송수신 테스트".into(),
            "test-operation",
            "test-key",
            0,
            [1; 32],
        )
        .unwrap();
        ComposeSnapshot::new(draft, attachments).unwrap()
    }
    #[test]
    fn apr01_mime_preserves_synthetic_unicode_attachment_and_fixed_recipient() {
        let content = "한통 첨부 테스트".as_bytes();
        assert_eq!(content.len(), 23);
        let snapshot = fixture(vec![
            AttachmentSnapshot::new(
                "hantong-test.txt".into(),
                "text/plain".into(),
                content.to_vec(),
            )
            .unwrap(),
        ]);
        let encoded = encode(&snapshot, "sender@example.invalid", &[3; 32], 0).unwrap();
        let mime = std::str::from_utf8(encoded.as_bytes()).unwrap();
        assert!(
            mime.contains("From: <sender@example.invalid>\r\nTo: <receiver@example.invalid>\r\n")
        );
        assert!(mime.contains("Content-Disposition: attachment; filename=\"hantong-test.txt\""));
        assert!(mime.contains(&base64(content)));
        assert!(mime.contains(&base64(snapshot.draft().body().as_bytes())));
        assert_eq!(mime.matches("Content-Disposition: attachment;").count(), 1);
        assert_eq!(mime.matches("\r\nFrom:").count(), 1);
        assert!(mime.is_ascii());
        assert!(mime.split("\r\n").all(|line| line.len() <= 998));
        assert!(!mime.replace("\r\n", "").contains(['\r', '\n']));
        assert_eq!(format!("{encoded:?}"), "GoogleMimeMessage(<redacted>)");
    }
    #[test]
    fn sec02_mail_bytes_cannot_inject_headers_or_multipart_boundaries() {
        let malicious = b"\r\nBcc: hidden@example.invalid\r\n--=_hantong_x\r\n.\r\nQUIT";
        let snapshot = fixture(vec![
            AttachmentSnapshot::new("data.txt".into(), "text/plain".into(), malicious.to_vec())
                .unwrap(),
        ]);
        let encoded = encode(&snapshot, "sender@example.invalid", &[4; 32], 0).unwrap();
        let mime = std::str::from_utf8(encoded.as_bytes()).unwrap();
        assert!(!mime.contains("hidden@example.invalid"));
        assert!(!mime.contains("\r\nQUIT"));
        assert!(!mime.contains("--=_hantong_x"));
        for sender in [
            "from@example.invalid\r\nBcc: hidden@example.invalid",
            "a@example.invalid,hidden@example.invalid",
            "",
            "@example.invalid",
        ] {
            assert!(encode(&snapshot, sender, &[4; 32], 0).is_err());
        }
        assert!(encode(&snapshot, "sender@example.invalid", &[0; 32], 0).is_err());
    }
    #[test]
    fn sync07_mime_correlation_is_stable_for_one_review_and_distinct_across_reviews() {
        let snapshot = fixture(vec![]);
        let first = encode(&snapshot, "sender@example.invalid", &[1; 32], 1).unwrap();
        let replay = encode(&snapshot, "sender@example.invalid", &[1; 32], 1).unwrap();
        let other = encode(&snapshot, "sender@example.invalid", &[2; 32], 1).unwrap();
        assert_eq!(first.as_bytes(), replay.as_bytes());
        assert_ne!(first.as_bytes(), other.as_bytes());
        assert!(
            !std::str::from_utf8(first.as_bytes())
                .unwrap()
                .contains("multipart/")
        );
        // Correlation aids reconciliation; it is not provider-side idempotency.
    }
}
