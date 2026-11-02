//! Client-secret review reuses the protected input binding, never IMAP authority.
use crate::{AccountSetupReview, registration_review::ReviewSnapshot};
use hantong_contracts::ErrorCode;
use hantong_domain::{
    config::Language,
    mail_connection::{Authentication, TlsMode},
};
#[derive(Clone, Eq, PartialEq)]
pub struct GoogleClientReview {
    capture: AccountSetupReview,
}
impl std::fmt::Debug for GoogleClientReview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GoogleClientReview(<redacted>)")
    }
}
impl GoogleClientReview {
    pub fn new(capture: AccountSetupReview) -> Result<Self, ErrorCode> {
        let e = capture.endpoint();
        hantong_contracts::google::validate_google_client_id(e.login_name())?;
        if capture.draft().account_id() != "google-client"
            || e.host() != "oauth2.googleapis.com"
            || e.port() != 443
            || e.tls() != TlsMode::ImplicitTls
            || e.authentication() != Authentication::AppPassword
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self { capture })
    }
    pub fn capture_review(&self) -> &AccountSetupReview {
        &self.capture
    }
    pub fn client_id(&self) -> &str {
        self.capture.endpoint().login_name()
    }
    pub fn summary(&self, language: Language) -> String {
        format!(
            "{}\r\n\r\nClient ID: {}\r\nhttps://oauth2.googleapis.com/token\r\n\r\n{}",
            text(language, "intro"),
            self.client_id(),
            text(language, "footer")
        )
    }
}
impl ReviewSnapshot for GoogleClientReview {
    fn created_at_utc_ms(&self) -> i64 {
        self.capture.created_at_utc_ms()
    }
    fn expires_at_utc_ms(&self) -> i64 {
        self.capture.expires_at_utc_ms()
    }
}
pub fn text(language: Language, key: &str) -> &'static str {
    let source = match language {
        Language::En => include_str!("../resources/google-client-en.txt"),
        Language::Ko => include_str!("../resources/google-client-ko.txt"),
    };
    source
        .lines()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
        .expect("Google client review resource")
}
