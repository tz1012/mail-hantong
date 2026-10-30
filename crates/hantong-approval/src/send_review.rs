//! Immutable outgoing-mail review data. Constructing it grants no authority.
use crate::registration_review::ReviewSnapshot;
use hantong_contracts::{
    ErrorCode,
    mail_send::{DraftCreateRequest, SendReviewRequest},
};
use hantong_domain::{config::Language, mail_connection::SmtpEndpointConfig};
use std::fmt;

#[derive(Clone, Eq, PartialEq)]
pub struct SendReview {
    pub review_id: [u8; 32],
    pub owner_registration_id: [u8; 16],
    pub profile_epoch: u64,
    pub account_version: u64,
    pub endpoint: SmtpEndpointConfig,
    pub draft: DraftCreateRequest,
    pub request: SendReviewRequest,
    pub created_at_utc_ms: i64,
    pub expires_at_utc_ms: i64,
}
impl fmt::Debug for SendReview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SendReview(<redacted>)")
    }
}
impl ReviewSnapshot for SendReview {
    fn created_at_utc_ms(&self) -> i64 {
        self.created_at_utc_ms
    }
    fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }
}
impl SendReview {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        if self.review_id == [0; 32]
            || self.owner_registration_id == [0; 16]
            || self.profile_epoch == 0
            || self.account_version == 0
            || self.draft.account_id() != self.request.account_id()
            || self.draft.payload_hash() != self.request.payload_hash()
            || self.created_at_utc_ms <= 0
            || !matches!(
                self.expires_at_utc_ms.checked_sub(self.created_at_utc_ms),
                Some(1..=300000)
            )
        {
            return Err(ErrorCode::InvalidInput);
        }
        if self.summary(Language::En).encode_utf16().count() > 2 * 1024 * 1024
            || self.summary(Language::Ko).encode_utf16().count() > 2 * 1024 * 1024
        {
            return Err(ErrorCode::QuotaExceeded);
        }
        Ok(())
    }
    pub fn summary(&self, lang: Language) -> String {
        let label = |key| text(lang, key);
        format!(
            "{}: {}\r\n{}: {}\r\n{}: {}:{} ({:?})\r\n{}: {}\r\n{}: {}\r\n{}: {}\r\n\r\n{}:\r\n{}\r\n\r\n{}",
            label("account"),
            visible(self.draft.account_id()),
            label("from"),
            visible(self.endpoint.login_name()),
            label("server"),
            visible(self.endpoint.host()),
            self.endpoint.port(),
            self.endpoint.tls(),
            label("to"),
            self.draft
                .recipients()
                .iter()
                .map(|s| visible(s))
                .collect::<Vec<_>>()
                .join(", "),
            label("subject"),
            visible(self.draft.subject()),
            label("operation"),
            visible(self.request.operation_id()),
            label("body"),
            visible(self.draft.body()),
            label("footer")
        )
    }
}
fn visible(value: &str) -> String {
    let mut out = String::new();
    for c in value.replace("\r\n", "\n").chars() {
        match c {
            '\n' => out.push_str("\r\n"),
            '\t' => out.push_str("    "),
            c if c.is_control()
                || matches!(c,'\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') =>
            {
                out.push_str(&format!("\\u{{{:04x}}}", c as u32))
            }
            c => out.push(c),
        }
    }
    out
}
pub fn text(language: Language, key: &str) -> &'static str {
    let resource = match language {
        Language::En => include_str!("../resources/send-en.txt"),
        Language::Ko => include_str!("../resources/send-ko.txt"),
    };
    resource
        .lines()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
        .expect("send review resource")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn apr01_send_labels_have_equal_bilingual_keys_and_escape_deceptive_controls() {
        let keys = |s: &str| {
            s.lines()
                .map(|l| l.split_once('=').unwrap().0.to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            keys(include_str!("../resources/send-en.txt")),
            keys(include_str!("../resources/send-ko.txt"))
        );
        assert_eq!(visible("x\u{1b}\u{202e}\ny"), "x\\u{001b}\\u{202e}\r\ny");
    }
}
