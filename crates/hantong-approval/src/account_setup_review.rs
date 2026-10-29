//! Pure data for a human-reviewed IMAP account setup.
use crate::setup_resources::{SetupTextKey, setup_text};
use hantong_domain::config::Language;
use hantong_domain::{
    account_provisioning::NewImapAccountDraft,
    mail_connection::{Authentication, ImapEndpointConfig, TlsMode},
};
use std::{fmt, time::Duration};

const MAX_LIFETIME_MS: i64 = 300_000;

#[derive(Clone, Eq, PartialEq)]
pub struct AccountSetupReview {
    draft: NewImapAccountDraft,
    endpoint: ImapEndpointConfig,
    request_id: [u8; 32],
    payload_hash: [u8; 32],
    credential_nonce: [u8; 32],
    created_at_utc_ms: i64,
    expires_at_utc_ms: i64,
}

impl fmt::Debug for AccountSetupReview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountSetupReview { <redacted> }")
    }
}

impl AccountSetupReview {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        draft: NewImapAccountDraft,
        endpoint: ImapEndpointConfig,
        request_id: [u8; 32],
        payload_hash: [u8; 32],
        credential_nonce: [u8; 32],
        created_at_utc_ms: i64,
        expires_at_utc_ms: i64,
    ) -> Result<Self, hantong_contracts::ErrorCode> {
        draft
            .validate()
            .map_err(|_| hantong_contracts::ErrorCode::InvalidInput)?;
        if endpoint.settings_version() != 1
            || endpoint.authentication() != Authentication::AppPassword
            || request_id == [0; 32]
            || payload_hash == [0; 32]
            || credential_nonce == [0; 32]
            || created_at_utc_ms <= 0
            || !expires_at_utc_ms
                .checked_sub(created_at_utc_ms)
                .is_some_and(|value| (1..=MAX_LIFETIME_MS).contains(&value))
        {
            return Err(hantong_contracts::ErrorCode::InvalidInput);
        }
        Ok(Self {
            draft,
            endpoint,
            request_id,
            payload_hash,
            credential_nonce,
            created_at_utc_ms,
            expires_at_utc_ms,
        })
    }
    pub fn draft(&self) -> &NewImapAccountDraft {
        &self.draft
    }
    pub fn endpoint(&self) -> &ImapEndpointConfig {
        &self.endpoint
    }
    pub fn request_id(&self) -> &[u8; 32] {
        &self.request_id
    }
    pub fn payload_hash(&self) -> &[u8; 32] {
        &self.payload_hash
    }
    pub fn credential_nonce(&self) -> &[u8; 32] {
        &self.credential_nonce
    }
    pub fn created_at_utc_ms(&self) -> i64 {
        self.created_at_utc_ms
    }
    pub fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }
    pub fn lifetime(&self) -> Duration {
        Duration::from_millis((self.expires_at_utc_ms - self.created_at_utc_ms) as u64)
    }
}

fn escaped(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\u{2028}' | '\u{2029}' => out.push(' '),
            '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}' => out.push_str(&format!("\\u{{{:04X}}}", ch as u32)),
            c if c.is_control() => out.push_str(&format!("\\u{{{:02X}}}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub fn setup_summary(review: &AccountSetupReview, language: Language) -> String {
    let d = review.draft();
    let e = review.endpoint();
    let display = d
        .display_name()
        .map(escaped)
        .unwrap_or_else(|| setup_text(language, SetupTextKey::Unnamed).to_owned());
    let color = d
        .color()
        .map(escaped)
        .unwrap_or_else(|| setup_text(language, SetupTextKey::DefaultColor).to_owned());
    let tls = match e.tls() {
        TlsMode::ImplicitTls => setup_text(language, SetupTextKey::ImplicitTls),
        TlsMode::StarttlsRequired => setup_text(language, SetupTextKey::Starttls),
    };
    format!(
        "{}: {}\n{}: {}\n{}: {}:{}\n{}: {}\n{}: {}\n{}: {}",
        setup_text(language, SetupTextKey::DisplayName),
        display,
        setup_text(language, SetupTextKey::Color),
        color,
        setup_text(language, SetupTextKey::Server),
        escaped(e.host()),
        e.port(),
        setup_text(language, SetupTextKey::Tls),
        tls,
        setup_text(language, SetupTextKey::Login),
        escaped(e.login_name()),
        setup_text(language, SetupTextKey::Authentication),
        setup_text(language, SetupTextKey::AppPassword),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_domain::mail_connection::MailConnectionConfig;
    fn review(name: Option<&str>) -> AccountSetupReview {
        let d = NewImapAccountDraft::new("acct".into(), name.map(str::to_owned), None).unwrap();
        let e = MailConnectionConfig::from_json(br#"{"schema_version":1,"account_id":"acct","imap":{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"user","authentication":"app_password"}}"#).unwrap().imap().clone();
        AccountSetupReview::new(d, e, [1; 32], [2; 32], [3; 32], 1, 300001).unwrap()
    }
    #[test]
    fn setup_review_rejects_zero_ids_and_bad_lifetime() {
        let r = review(None);
        assert_eq!(
            AccountSetupReview::new(
                r.draft.clone(),
                r.endpoint.clone(),
                [0; 32],
                [2; 32],
                [3; 32],
                1,
                2
            ),
            Err(hantong_contracts::ErrorCode::InvalidInput)
        );
        assert_eq!(
            AccountSetupReview::new(
                r.draft.clone(),
                r.endpoint.clone(),
                [1; 32],
                [2; 32],
                [3; 32],
                1,
                300002
            ),
            Err(hantong_contracts::ErrorCode::InvalidInput)
        );
    }
    #[test]
    fn setup_summary_escapes_untrusted_fields_and_omits_secret_material() {
        let r = review(Some("A\"\\\u{202e}"));
        let s = setup_summary(&r, Language::En);
        assert!(s.contains("A\\\"\\\\\\u{202E}"));
        assert!(!s.contains("payload_hash") && !s.contains("nonce"));
    }
}
