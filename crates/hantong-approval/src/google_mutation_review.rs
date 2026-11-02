//! Account-specific native review; constructing this snapshot grants no authority.
use crate::registration_review::ReviewSnapshot;
use hantong_contracts::{
    ErrorCode,
    google::GoogleAccountBinding,
    google_mutation::MutationPayload,
    google_write::{ComposeSnapshot, GoogleAccessMode},
};
use hantong_domain::config::Language;

#[derive(Clone, Eq, PartialEq)]
pub struct GoogleMutationReview {
    pub review_id: [u8; 32],
    pub owner_registration_id: [u8; 16],
    pub profile_epoch: u64,
    pub account_version: u64,
    pub binding: GoogleAccountBinding,
    pub sender: String,
    pub request: MutationPayload,
    pub compose: Option<ComposeSnapshot>,
    pub before_labels: Vec<String>,
    pub created_at_utc_ms: i64,
    pub expires_at_utc_ms: i64,
}
impl std::fmt::Debug for GoogleMutationReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GoogleMutationReview(<redacted>)")
    }
}
impl ReviewSnapshot for GoogleMutationReview {
    fn created_at_utc_ms(&self) -> i64 {
        self.created_at_utc_ms
    }
    fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }
}
impl GoogleMutationReview {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        self.binding.validate()?;
        if self.review_id == [0; 32]
            || self.owner_registration_id == [0; 16]
            || self.profile_epoch == 0
            || self.account_version == 0
            || self.binding.access_mode != GoogleAccessMode::Modify
            || self.binding.account_id != self.request.account_id()
            || self.sender.is_empty()
            || self.sender.len() > 320
            || self.sender.chars().any(char::is_control)
            || self.created_at_utc_ms <= 0
            || !matches!(
                self.expires_at_utc_ms.checked_sub(self.created_at_utc_ms),
                Some(1..=300000)
            )
            || MutationPayload::from_json(&self.request.to_json())? != self.request
        {
            return Err(ErrorCode::InvalidInput);
        }
        match (&self.request, &self.compose) {
            (MutationPayload::SaveDraft(r), Some(d))
                if d.account_id() == r.account_id()
                    && d.payload_hash() == r.payload_hash()
                    && self.before_labels.is_empty() => {}
            (MutationPayload::Labels { .. }, None)
                if self.before_labels.len() <= 100
                    && self.before_labels.iter().all(|s| {
                        !s.is_empty()
                            && s.len() <= 256
                            && s.bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                    }) => {}
            (MutationPayload::Trash { .. }, None)
                if self.before_labels.len() <= 100
                    && self.before_labels.iter().all(|s| {
                        !s.is_empty()
                            && s.len() <= 256
                            && s.bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                    }) => {}
            _ => return Err(ErrorCode::InvalidInput),
        }
        if self.summary(Language::En).encode_utf16().count() > 2 * 1024 * 1024
            || self.summary(Language::Ko).encode_utf16().count() > 2 * 1024 * 1024
        {
            return Err(ErrorCode::QuotaExceeded);
        }
        Ok(())
    }
    pub fn summary(&self, language: Language) -> String {
        let label = |key| text(language, key);
        let mut out = format!(
            "{}: {}\r\n{}: {}\r\n{}: {}\r\n{}: {}\r\n",
            label("account"),
            self.binding.account_id,
            label("from"),
            crate::send_review::visible(&self.sender),
            label("operation"),
            self.request.operation_id(),
            label("version"),
            self.request.expected_version()
        );
        match (&self.request, &self.compose) {
            (MutationPayload::SaveDraft(_), Some(d)) => {
                out.push_str(&format!(
                    "{}\r\n{}: {}\r\n{}: {}\r\n{}:\r\n{}\r\n",
                    label("draft"),
                    label("to"),
                    d.recipients().join(", "),
                    label("subject"),
                    crate::send_review::visible(d.subject()),
                    label("body"),
                    crate::send_review::visible(d.body())
                ));
                for a in d.attachments() {
                    out.push_str(&format!(
                        "{} | {} | {} bytes\r\n",
                        a.name(),
                        a.media_type(),
                        a.bytes().len()
                    ));
                    if a.media_type() == "text/plain"
                        && let Ok(value) = std::str::from_utf8(a.bytes())
                    {
                        out.push_str(&crate::send_review::visible(value));
                        out.push_str("\r\n");
                    }
                }
            }
            (MutationPayload::Labels { change, .. }, _) => out.push_str(&format!(
                "{}\r\n{}: {}\r\n{}: {}\r\n{}: {}\r\n{}: {}\r\n",
                label("labels"),
                label("message"),
                change.message_id(),
                label("before"),
                self.before_labels.join(", "),
                label("add"),
                change.add().join(", "),
                label("remove"),
                change.remove().join(", ")
            )),
            (MutationPayload::Trash { message_id, .. }, _) => out.push_str(&format!(
                "{}\r\n{}: {}\r\n{}\r\n",
                label("trash"),
                label("message"),
                message_id,
                label("trash_warning")
            )),
            _ => {}
        }
        out.push_str(&format!(
            "{}: {}\r\n\r\n{}",
            label("payload"),
            hantong_contracts::wire_hex::encode(self.request.payload_hash()),
            label("footer")
        ));
        out
    }
}
pub fn text(language: Language, key: &str) -> &'static str {
    let resource = match language {
        Language::En => include_str!("../resources/google-mutation-en.txt"),
        Language::Ko => include_str!("../resources/google-mutation-ko.txt"),
    };
    resource
        .lines()
        .filter_map(|s| s.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
        .expect("mutation resource")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn apr01_mutation_native_window_captions_and_summaries_have_both_languages() {
        let keys = |resource: &str| {
            resource
                .lines()
                .map(|l| l.split_once('=').unwrap().0.to_owned())
                .collect::<Vec<_>>()
        };
        let en = keys(include_str!("../resources/google-mutation-en.txt"));
        let ko = keys(include_str!("../resources/google-mutation-ko.txt"));
        assert_eq!(en, ko);
        for key in [
            "title",
            "intro",
            "verify",
            "verification",
            "footer",
            "grant",
            "account",
            "from",
            "operation",
            "version",
            "draft",
            "to",
            "subject",
            "body",
            "labels",
            "trash",
            "trash_warning",
            "message",
            "before",
            "add",
            "remove",
            "payload",
        ] {
            for language in [Language::En, Language::Ko] {
                assert!(!text(language, key).is_empty());
            }
        }
    }
}
