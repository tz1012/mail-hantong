use super::presentation::review_text;
use crate::{VerifiedImapSetup, VerifiedRegistration, binding::CapturedReview};
use hantong_approval::{
    AccountSetupReview, RegistrationReview, registration_review::ReviewSnapshot, setup_summary,
};
use hantong_contracts::ErrorCode;
use hantong_domain::config::Language;
use hantong_secrets::PendingImapCredential;
use std::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum DialogReview {
    Registration(RegistrationReview),
    Imap(AccountSetupReview),
}
impl ReviewSnapshot for DialogReview {
    fn created_at_utc_ms(&self) -> i64 {
        match self {
            Self::Registration(review) => review.created_at_utc_ms(),
            Self::Imap(review) => review.created_at_utc_ms(),
        }
    }
    fn expires_at_utc_ms(&self) -> i64 {
        match self {
            Self::Registration(review) => review.expires_at_utc_ms(),
            Self::Imap(review) => review.expires_at_utc_ms(),
        }
    }
}
impl DialogReview {
    pub(super) fn is_setup(&self) -> bool {
        matches!(self, Self::Imap(_))
    }
    pub(super) fn summary(&self, language: Language) -> Result<String, ErrorCode> {
        match self {
            Self::Registration(review) => review_text(review, language),
            Self::Imap(review) => Ok(setup_summary(review, language)),
        }
    }
}

pub(super) enum DialogBinding {
    Registration(CapturedReview),
    Imap(CapturedReview<AccountSetupReview>),
}
impl DialogBinding {
    pub(super) fn review(&self) -> DialogReview {
        match self {
            Self::Registration(binding) => DialogReview::Registration(binding.review.clone()),
            Self::Imap(binding) => DialogReview::Imap(binding.review.clone()),
        }
    }
    pub(super) fn started(&self) -> Instant {
        match self {
            Self::Registration(binding) => binding.started,
            Self::Imap(binding) => binding.started,
        }
    }
    pub(super) fn validate(&self, expected: &DialogReview) -> Result<(), ErrorCode> {
        match (self, expected) {
            (Self::Registration(binding), DialogReview::Registration(review)) => {
                binding.validate(review)
            }
            (Self::Imap(binding), DialogReview::Imap(review)) => binding.validate(review),
            _ => Err(ErrorCode::ScopeDenied),
        }
    }
    // Only the owned window's OS Verified branch calls this adapter.
    pub(super) fn into_verified(
        self,
        pending: Option<PendingImapCredential>,
    ) -> Result<DialogVerified, ErrorCode> {
        match (self, pending) {
            (Self::Registration(binding), None) => binding
                .into_verified()
                .map(|value| DialogVerified::Registration(Box::new(value))),
            (Self::Imap(binding), Some(pending)) => binding
                .into_verified_setup(pending)
                .map(|value| DialogVerified::Imap(Box::new(value))),
            _ => Err(ErrorCode::InternalError),
        }
    }
}
pub(super) enum DialogVerified {
    Registration(Box<VerifiedRegistration>),
    Imap(Box<VerifiedImapSetup>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_approval::{Presentation, ReviewIntent, ReviewModel, ReviewState};
    use hantong_domain::{
        account_provisioning::NewImapAccountDraft, config::Theme,
        mail_connection::MailConnectionConfig,
    };
    use std::time::Duration;
    #[test]
    fn inst05_setup_dialog_paints_exact_summary_before_verification_and_keeps_theme() {
        let endpoint = MailConnectionConfig::from_json(
            br#"{"schema_version":1,"account_id":"acct","imap":{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"user","authentication":"app_password"}}"#,
        ).unwrap().imap().clone();
        let review = AccountSetupReview::new(
            NewImapAccountDraft::new("acct".into(), Some("개인 메일".into()), None).unwrap(),
            endpoint,
            [1; 32],
            [2; 32],
            [3; 32],
            1000,
            301000,
        )
        .unwrap();
        let dialog = DialogReview::Imap(review.clone());
        for language in [Language::En, Language::Ko] {
            assert_eq!(
                dialog.summary(language).unwrap(),
                setup_summary(&review, language)
            );
            let summary = dialog.summary(language).unwrap();
            assert!(summary.contains("\r\n"));
            assert!(!summary.contains("\r\r\n"));
            let mut model = ReviewModel::new(
                dialog.clone(),
                Presentation::new(Some(language), Theme::System, false, true, false),
            );
            assert_eq!(
                model.begin_verification(2000, Duration::from_secs(1)),
                ReviewIntent::None
            );
            assert!(model.paint_completed(&dialog));
            assert_eq!(model.state(), ReviewState::Reviewing);
            model.set_theme(Theme::Light);
            model.system_theme_changed(true, true);
            assert_eq!(
                model.presentation().effective_theme(),
                hantong_approval::EffectiveTheme::HighContrast
            );
            assert_eq!(model.review(), &dialog);
            assert_eq!(
                model.begin_verification(2001, Duration::from_secs(1)),
                ReviewIntent::RequestVerification
            );
            assert_eq!(
                model.begin_verification(2002, Duration::from_secs(2)),
                ReviewIntent::None
            );
            model.cancel();
            assert_eq!(
                model.begin_verification(2003, Duration::from_secs(3)),
                ReviewIntent::None
            );
        }
    }
}
