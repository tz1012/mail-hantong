//! Public account snapshot only. Constructing this value grants no authority.
use crate::registration_review::ReviewSnapshot;
use hantong_contracts::{
    ErrorCode,
    google::{GoogleAccountBinding, GoogleIdentity},
};
use hantong_domain::config::Language;

#[derive(Clone, Eq, PartialEq)]
pub struct GoogleSetupReview {
    pub request_id: [u8; 32],
    pub identity: GoogleIdentity,
    pub binding: GoogleAccountBinding,
    pub expected_version: u64,
    pub display_name: Option<String>,
    pub created_at_utc_ms: i64,
    pub expires_at_utc_ms: i64,
}
impl std::fmt::Debug for GoogleSetupReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GoogleSetupReview(<redacted>)")
    }
}
impl ReviewSnapshot for GoogleSetupReview {
    fn created_at_utc_ms(&self) -> i64 {
        self.created_at_utc_ms
    }
    fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }
}
impl GoogleSetupReview {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        self.identity.validate()?;
        self.binding.validate()?;
        if self.request_id == [0; 32]
            || self.identity.account_id != self.binding.account_id
            || self.identity.subject != self.binding.subject
            || self.expected_version.checked_add(1) != Some(self.binding.credential_version)
            || self.created_at_utc_ms <= 0
            || !matches!(
                self.expires_at_utc_ms.checked_sub(self.created_at_utc_ms),
                Some(1..=300000)
            )
            || self.display_name.as_ref().is_some_and(|s| {
                s.is_empty()
                    || s.len() > 512
                    || s.chars().count() > 128
                    || s.chars().any(char::is_control)
            })
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(())
    }
    pub fn summary(&self, language: Language) -> String {
        let visible = |s: &str| hantong_contracts::message_body::sanitize_terminal_text(s);
        format!(
            "{}\r\n\r\n{}: {}\r\n{}: {}\r\n{}: {}\r\n{}: {}\r\n{}: {}\r\n{}: {}\r\n\r\n{}\r\n\r\n{}\r\n\r\n{}",
            text(language, "intro"),
            text(language, "account"),
            visible(&self.binding.account_id),
            text(language, "email"),
            visible(&self.identity.email),
            text(language, "subject"),
            visible(&self.identity.subject),
            text(language, "client"),
            visible(&self.binding.client_id),
            text(language, "name"),
            visible(self.display_name.as_deref().unwrap_or(&self.identity.email)),
            text(language, "scopes"),
            visible(&self.identity.granted_scopes.join(" ")),
            text(language, "readonly"),
            if self.expected_version > 0 {
                text(language, "reconnect")
            } else {
                text(language, "new_account")
            },
            text(language, "footer")
        )
    }
}
pub fn text(language: Language, key: &str) -> &'static str {
    let source = match language {
        Language::En => include_str!("../resources/google-en.txt"),
        Language::Ko => include_str!("../resources/google-ko.txt"),
    };
    source
        .lines()
        .find_map(|line| {
            line.split_once('=')
                .filter(|(k, _)| *k == key)
                .map(|(_, v)| v)
        })
        .expect("Google review resource")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acc02_google_review_resources_have_equal_keys_and_explicit_readonly_scope() {
        let keys = |s: &str| {
            s.lines()
                .map(|l| l.split_once('=').unwrap().0.to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            keys(include_str!("../resources/google-en.txt")),
            keys(include_str!("../resources/google-ko.txt"))
        );
        assert!(text(Language::En, "readonly").contains("read-only"));
        assert!(text(Language::Ko, "readonly").contains("읽기 전용"));
    }
}
