//! SMTP setup snapshot. Its technical password-control binding grants no authority.
use crate::{AccountSetupReview, registration_review::ReviewSnapshot};
use hantong_contracts::ErrorCode;
use hantong_domain::{config::Language, mail_connection::SmtpEndpointConfig};
use std::fmt;

#[derive(Clone, Eq, PartialEq)]
pub struct SmtpSetupReview {
    capture: AccountSetupReview,
    endpoint: SmtpEndpointConfig,
    account_version: u64,
}
impl fmt::Debug for SmtpSetupReview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SmtpSetupReview(<redacted>)")
    }
}
impl SmtpSetupReview {
    pub fn new(
        capture: AccountSetupReview,
        endpoint: SmtpEndpointConfig,
        account_version: u64,
    ) -> Result<Self, ErrorCode> {
        let mirror = capture.endpoint();
        if !(1..=i64::MAX as u64).contains(&account_version)
            || mirror.settings_version() != endpoint.settings_version()
            || mirror.host() != endpoint.host()
            || mirror.port() != endpoint.port()
            || mirror.tls() != endpoint.tls()
            || mirror.login_name() != endpoint.login_name()
            || mirror.authentication() != endpoint.authentication()
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            capture,
            endpoint,
            account_version,
        })
    }
    pub fn capture_review(&self) -> &AccountSetupReview {
        &self.capture
    }
    pub fn endpoint(&self) -> &SmtpEndpointConfig {
        &self.endpoint
    }
    pub fn account_version(&self) -> u64 {
        self.account_version
    }
    pub fn account_id(&self) -> &str {
        self.capture.draft().account_id()
    }
    pub fn request_id(&self) -> &[u8; 32] {
        self.capture.request_id()
    }
    pub fn payload_hash(&self) -> &[u8; 32] {
        self.capture.payload_hash()
    }
    pub fn credential_nonce(&self) -> &[u8; 32] {
        self.capture.credential_nonce()
    }
    pub fn created_at_utc_ms(&self) -> i64 {
        self.capture.created_at_utc_ms()
    }
    pub fn expires_at_utc_ms(&self) -> i64 {
        self.capture.expires_at_utc_ms()
    }
    pub fn summary(&self, language: Language) -> String {
        let text = |key| text(language, key);
        format!(
            "{}\r\n\r\n{}: {}\r\n{}: {}\r\n{}: {}\r\n{}: {}\r\nTLS: {:?}\r\n{}: {}\r\n\r\n{}",
            text("intro"),
            text("account"),
            self.account_id(),
            text("version"),
            self.account_version,
            text("server"),
            self.endpoint.host(),
            text("port"),
            self.endpoint.port(),
            self.endpoint.tls(),
            text("login"),
            self.endpoint.login_name(),
            text("footer")
        )
    }
}
impl ReviewSnapshot for SmtpSetupReview {
    fn created_at_utc_ms(&self) -> i64 {
        self.created_at_utc_ms()
    }
    fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms()
    }
}
pub fn text(language: Language, key: &str) -> &'static str {
    let source = match language {
        Language::En => include_str!("../resources/smtp-en.txt"),
        Language::Ko => include_str!("../resources/smtp-ko.txt"),
    };
    source
        .lines()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
        .expect("SMTP review resource")
}
