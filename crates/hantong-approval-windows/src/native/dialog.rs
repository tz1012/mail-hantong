use super::presentation::review_text;
use crate::{
    VerifiedImapSetup, VerifiedRegistration, VerifiedScopeRefresh, binding::CapturedReview,
};
use hantong_approval::{
    AccountSetupReview, RegistrationReview, ScopeRefreshReview,
    registration_review::ReviewSnapshot, setup_summary,
};
use hantong_contracts::ErrorCode;
use hantong_domain::config::Language;
use hantong_secrets::PendingImapCredential;
use std::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum DialogReview {
    Registration(RegistrationReview),
    Imap(AccountSetupReview),
    ScopeRefresh(ScopeRefreshReview),
    Send(hantong_approval::SendReview),
    Smtp(hantong_approval::SmtpSetupReview),
    Google(hantong_approval::GoogleSetupReview),
}
impl ReviewSnapshot for DialogReview {
    fn created_at_utc_ms(&self) -> i64 {
        match self {
            Self::Registration(review) => review.created_at_utc_ms(),
            Self::Imap(review) => review.created_at_utc_ms(),
            Self::ScopeRefresh(review) => review.created_at_utc_ms(),
            Self::Send(review) => review.created_at_utc_ms(),
            Self::Smtp(review) => review.created_at_utc_ms(),
            Self::Google(review) => review.created_at_utc_ms,
        }
    }
    fn expires_at_utc_ms(&self) -> i64 {
        match self {
            Self::Registration(review) => review.expires_at_utc_ms(),
            Self::Imap(review) => review.expires_at_utc_ms(),
            Self::ScopeRefresh(review) => review.expires_at_utc_ms(),
            Self::Send(review) => review.expires_at_utc_ms(),
            Self::Smtp(review) => review.expires_at_utc_ms(),
            Self::Google(review) => review.expires_at_utc_ms,
        }
    }
}
impl DialogReview {
    pub(super) fn is_google(&self) -> bool {
        matches!(self, Self::Google(_))
    }
    pub(super) fn is_smtp(&self) -> bool {
        matches!(self, Self::Smtp(_))
    }
    pub(super) fn password_review(&self) -> Option<&AccountSetupReview> {
        match self {
            Self::Imap(v) => Some(v),
            Self::Smtp(v) => Some(v.capture_review()),
            _ => None,
        }
    }

    pub(super) fn is_send(&self) -> bool {
        matches!(self, Self::Send(_))
    }
    pub(super) fn is_mcp_registration(&self) -> bool {
        matches!(self, Self::Registration(review) if review.client_kind() == hantong_contracts::client::ClientKind::Mcp)
    }
    pub(super) fn is_gui_registration(&self) -> bool {
        matches!(self, Self::Registration(review) if review.client_kind() == hantong_contracts::client::ClientKind::Gui)
    }
    pub(super) fn is_setup(&self) -> bool {
        matches!(self, Self::Imap(_) | Self::Smtp(_))
    }
    pub(super) fn is_scope_refresh(&self) -> bool {
        matches!(self, Self::ScopeRefresh(_))
    }
    pub(super) fn is_resume(&self) -> bool {
        matches!(self, Self::Imap(review) if review.is_resume())
    }
    pub(super) fn summary(&self, language: Language) -> Result<String, ErrorCode> {
        match self {
            Self::Registration(review) => review_text(review, language),
            Self::Imap(review) => Ok(setup_summary(review, language)),
            Self::ScopeRefresh(review) => Ok(review.summary(language)),
            Self::Send(review) => Ok(review.summary(language)),
            Self::Smtp(review) => Ok(review.summary(language)),
            Self::Google(review) => Ok(review.summary(language)),
        }
    }
}

pub(super) enum DialogBinding {
    Registration(CapturedReview),
    Imap(CapturedReview<AccountSetupReview>),
    ScopeRefresh(CapturedReview<ScopeRefreshReview>),
    Send(CapturedReview<hantong_approval::SendReview>),
    Smtp(CapturedReview<hantong_approval::SmtpSetupReview>),
    Google(CapturedReview<hantong_approval::GoogleSetupReview>),
}
impl DialogBinding {
    pub(super) fn review(&self) -> DialogReview {
        match self {
            Self::Registration(binding) => DialogReview::Registration(binding.review.clone()),
            Self::Imap(binding) => DialogReview::Imap(binding.review.clone()),
            Self::ScopeRefresh(binding) => DialogReview::ScopeRefresh(binding.review.clone()),
            Self::Send(binding) => DialogReview::Send(binding.review.clone()),
            Self::Smtp(binding) => DialogReview::Smtp(binding.review.clone()),
            Self::Google(binding) => DialogReview::Google(binding.review.clone()),
        }
    }
    pub(super) fn started(&self) -> Instant {
        match self {
            Self::Registration(binding) => binding.started,
            Self::Imap(binding) => binding.started,
            Self::ScopeRefresh(binding) => binding.started,
            Self::Send(binding) => binding.started,
            Self::Smtp(binding) => binding.started,
            Self::Google(binding) => binding.started,
        }
    }
    pub(super) fn validate(&self, expected: &DialogReview) -> Result<(), ErrorCode> {
        match (self, expected) {
            (Self::Registration(binding), DialogReview::Registration(review)) => {
                binding.validate(review)
            }
            (Self::Imap(binding), DialogReview::Imap(review)) => binding.validate(review),
            (Self::ScopeRefresh(binding), DialogReview::ScopeRefresh(review)) => {
                binding.validate(review)
            }
            (Self::Send(binding), DialogReview::Send(review)) => binding.validate(review),
            (Self::Smtp(binding), DialogReview::Smtp(review)) => binding.validate(review),
            (Self::Google(binding), DialogReview::Google(review)) => binding.validate(review),
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
            (Self::ScopeRefresh(binding), None) => binding
                .into_verified()
                .map(|value| DialogVerified::ScopeRefresh(Box::new(value))),
            (Self::Send(binding), None) => binding
                .into_verified()
                .map(|value| DialogVerified::Send(Box::new(value))),
            (Self::Smtp(binding), Some(pending)) => binding
                .into_verified_smtp(pending)
                .map(|v| DialogVerified::Smtp(Box::new(v))),
            (Self::Google(binding), None) => binding
                .into_verified_google()
                .map(|v| DialogVerified::Google(Box::new(v))),
            _ => Err(ErrorCode::InternalError),
        }
    }
}
pub(super) enum DialogVerified {
    Registration(Box<VerifiedRegistration>),
    Imap(Box<VerifiedImapSetup>),
    ScopeRefresh(Box<VerifiedScopeRefresh>),
    Send(Box<crate::VerifiedSend>),
    Smtp(Box<crate::VerifiedSmtpSetup>),
    Google(Box<crate::VerifiedGoogleSetup>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_approval::{
        Presentation, ReviewIntent, ReviewModel, ReviewState, ScopeRefreshReview,
    };
    use hantong_domain::{
        account_provisioning::NewImapAccountDraft, config::Theme,
        mail_connection::MailConnectionConfig,
    };
    use std::time::Duration;
    #[test]
    fn inst04_gui_review_requires_own_painted_snapshot_before_verification() {
        let gui = RegistrationReview::new_gui([1; 32], 1, vec!["a".into()], [2; 32], 1000, 301000)
            .unwrap();
        let cli =
            RegistrationReview::new([1; 32], 1, vec!["a".into()], [2; 32], 1000, 301000).unwrap();
        let dialog = DialogReview::Registration(gui);
        assert!(dialog.is_gui_registration());
        for language in [Language::En, Language::Ko] {
            let mut model = ReviewModel::new(
                dialog.clone(),
                Presentation::new(Some(language), Theme::System, false, true, false),
            );
            assert!(!model.paint_completed(&DialogReview::Registration(cli.clone())));
            assert_eq!(
                model.begin_verification(2000, Duration::from_secs(1)),
                ReviewIntent::None
            );
            assert!(model.paint_completed(&dialog));
            assert_eq!(
                model.begin_verification(2001, Duration::from_secs(1)),
                ReviewIntent::RequestVerification
            );
        }
    }
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

    #[test]
    fn cli15_uid_scope_dialog_paints_bilingual_uid_permission_before_verification() {
        let review = ScopeRefreshReview::new_with_tools(
            [1; 32],
            1,
            [2; 16],
            [3; 32],
            4,
            5,
            [6; 32],
            [7; 32],
            vec!["acct_a".into()],
            vec!["acct_a".into()],
            hantong_contracts::auth::default_cli_folder_tools(),
            hantong_contracts::auth::default_cli_uid_tools(),
            1000,
            301000,
        )
        .unwrap();
        for (language, explanation, recovery, preservation) in [
            (
                Language::En,
                "metadata",
                "first-page recovery",
                "preserving existing records",
            ),
            (
                Language::Ko,
                "메타데이터",
                "첫 페이지 복구",
                "기존 기록을 보존",
            ),
        ] {
            let dialog = DialogReview::ScopeRefresh(review.clone());
            let summary = dialog.summary(language).unwrap();
            assert!(summary.contains("mail.sync.page"));
            assert!(summary.contains(explanation));
            assert!(summary.contains(recovery));
            assert!(summary.contains(preservation));
            let mut model = ReviewModel::new(
                dialog.clone(),
                Presentation::new(Some(language), Theme::System, false, true, false),
            );
            assert_eq!(
                model.begin_verification(2000, Duration::from_secs(1)),
                ReviewIntent::None
            );
            assert!(model.paint_completed(&dialog));
            assert_eq!(
                model.begin_verification(2001, Duration::from_secs(1)),
                ReviewIntent::RequestVerification
            );
        }
    }

    #[test]
    fn cli15_metadata_scope_dialog_requires_exact_header_disclosure_paint() {
        let review = ScopeRefreshReview::new_with_tools(
            [1; 32],
            1,
            [2; 16],
            [3; 32],
            4,
            5,
            [6; 32],
            [7; 32],
            vec!["acct_a".into()],
            vec!["acct_a".into()],
            hantong_contracts::auth::default_cli_uid_tools(),
            hantong_contracts::auth::default_cli_metadata_tools(),
            1000,
            301000,
        )
        .unwrap();
        for (language, disclosure) in [
            (Language::En, "subject and sender headers"),
            (Language::Ko, "제목·보낸 사람 헤더"),
        ] {
            let dialog = DialogReview::ScopeRefresh(review.clone());
            assert!(dialog.summary(language).unwrap().contains(disclosure));
            let mut model = ReviewModel::new(
                dialog.clone(),
                Presentation::new(Some(language), Theme::System, false, true, false),
            );
            assert_eq!(
                model.begin_verification(2000, Duration::from_secs(1)),
                ReviewIntent::None
            );
            assert!(model.paint_completed(&dialog));
            assert_eq!(
                model.begin_verification(2001, Duration::from_secs(1)),
                ReviewIntent::RequestVerification
            );
        }
    }

    #[test]
    fn cli15_app_read_scope_dialog_requires_both_disclosures_before_verification() {
        let mut app = vec![
            "app.diagnostics.snapshot".into(),
            "app.settings.display.get".into(),
        ];
        app.extend(hantong_contracts::auth::default_cli_metadata_tools());
        let review = ScopeRefreshReview::new_with_tools(
            [1; 32],
            1,
            [2; 16],
            [3; 32],
            4,
            5,
            [6; 32],
            [7; 32],
            vec!["acct_a".into()],
            vec!["acct_a".into()],
            hantong_contracts::auth::default_cli_metadata_tools(),
            app,
            1000,
            301000,
        )
        .unwrap();
        for (language, language_text, diagnostics_text) in [
            (Language::En, "saved CLI language", "this registered client"),
            (Language::Ko, "저장된 CLI 표시 언어", "이 등록 클라이언트"),
        ] {
            let dialog = DialogReview::ScopeRefresh(review.clone());
            let summary = dialog.summary(language).unwrap();
            assert!(summary.contains(language_text) && summary.contains(diagnostics_text));
            let mut model = ReviewModel::new(
                dialog.clone(),
                Presentation::new(Some(language), Theme::System, false, true, false),
            );
            assert_eq!(
                model.begin_verification(2000, Duration::from_secs(1)),
                ReviewIntent::None
            );
            assert!(model.paint_completed(&dialog));
            assert_eq!(
                model.begin_verification(2001, Duration::from_secs(1)),
                ReviewIntent::RequestVerification
            );
        }
    }

    #[test]
    fn cli15_scope_refresh_dialog_paints_exact_bilingual_snapshot_before_verification() {
        let review = ScopeRefreshReview::new(
            [1; 32],
            1,
            [2; 16],
            [3; 32],
            4,
            5,
            [6; 32],
            [7; 32],
            vec!["acct_a".into()],
            vec!["acct_b".into()],
            1000,
            301000,
        )
        .unwrap();
        let dialog = DialogReview::ScopeRefresh(review.clone());
        for language in [Language::En, Language::Ko] {
            let summary = dialog.summary(language).unwrap();
            assert_eq!(summary, review.summary(language));
            assert!(summary.contains("acct_a\r\n"));
            assert!(summary.contains("acct_b\r\n"));
            assert!(summary.contains("5 -> 6"));
            let mut model = ReviewModel::new(
                dialog.clone(),
                Presentation::new(Some(language), Theme::System, false, true, false),
            );
            assert_eq!(
                model.begin_verification(2000, Duration::from_secs(1)),
                ReviewIntent::None
            );
            assert!(model.paint_completed(&dialog));
            model.set_theme(Theme::Light);
            model.system_theme_changed(true, true);
            assert_eq!(
                model.presentation().effective_theme(),
                hantong_approval::EffectiveTheme::HighContrast
            );
            assert_eq!(
                model.begin_verification(2001, Duration::from_secs(1)),
                ReviewIntent::RequestVerification
            );
            model.cancel();
            assert_eq!(
                model.begin_verification(2002, Duration::from_secs(2)),
                ReviewIntent::None
            );
        }
    }
}
