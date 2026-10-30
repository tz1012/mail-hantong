use super::*;
mod consent;
mod dialog;
mod lifetime;
mod presentation;
mod window;

use lifetime::{CancelOnDrop, WindowPermit};
use std::sync::{atomic::AtomicBool, mpsc};

static WINDOW_ACTIVE: AtomicBool = AtomicBool::new(false);

pub(super) async fn show(
    review: RegistrationReview,
    language: Option<Language>,
    theme: Theme,
) -> Result<VerifiedRegistration, ErrorCode> {
    spawn_review(move |cancellation| window::run(review, language, theme, cancellation)).await
}

pub(super) async fn show_setup(
    review: AccountSetupReview,
    plan: hantong_secrets::ImapPasswordCapturePlan,
    language: Option<Language>,
    theme: Theme,
) -> Result<VerifiedImapSetup, ErrorCode> {
    if plan.is_smtp_capture_binding() {
        return Err(ErrorCode::ScopeDenied);
    }
    crate::setup_binding::validate_plan(&review, &plan)?;
    spawn_review(move |cancellation| window::run_setup(review, plan, language, theme, cancellation))
        .await
}

pub(super) async fn show_scope_refresh(
    review: ScopeRefreshReview,
    language: Option<Language>,
    theme: Theme,
) -> Result<VerifiedScopeRefresh, ErrorCode> {
    spawn_review(move |cancellation| {
        window::run_scope_refresh(review, language, theme, cancellation)
    })
    .await
}

pub(super) async fn show_send(
    review: SendReview,
    language: Option<Language>,
    theme: Theme,
) -> Result<VerifiedSend, ErrorCode> {
    spawn_review(move |cancellation| window::run_send(review, language, theme, cancellation)).await
}

pub(super) async fn show_smtp_setup(
    review: SmtpSetupReview,
    plan: hantong_secrets::SmtpPasswordCapturePlan,
    language: Option<Language>,
    theme: Theme,
) -> Result<VerifiedSmtpSetup, ErrorCode> {
    if plan.account_id() != review.account_id()
        || plan.endpoint() != review.endpoint()
        || plan.setup_request_id() != review.request_id()
        || plan.setup_payload_hash() != review.payload_hash()
        || plan.nonce() != review.credential_nonce()
        || plan.capture_endpoint() != review.capture_review().endpoint()
    {
        return Err(ErrorCode::ScopeDenied);
    }
    spawn_review(move |cancellation| {
        window::run_smtp_setup(
            review,
            plan.into_imap_capture_binding(),
            language,
            theme,
            cancellation,
        )
    })
    .await
}

async fn spawn_review<T: Send + 'static>(
    run: impl FnOnce(mpsc::Receiver<()>) -> Result<T, ErrorCode> + Send + 'static,
) -> Result<T, ErrorCode> {
    let permit = WindowPermit::claim(&WINDOW_ACTIVE)?;
    let (cancel, cancellation) = mpsc::channel();
    let _cancel_on_drop = CancelOnDrop(cancel);
    let (sender, result) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("hantong-review-ui".into())
        .spawn(move || {
            let _permit = permit;
            let outcome = run(cancellation);
            let _ = sender.send(outcome);
        })
        .map_err(|_| ErrorCode::InternalError)?;
    result.await.map_err(|_| ErrorCode::InternalError)?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn inst05_smtp_capture_cannot_use_an_imap_caption_or_approval_path() {
        let config=hantong_domain::mail_connection::MailConnectionConfig::from_json(br#"{"schema_version":1,"account_id":"acct","imap":{"settings_version":1,"host":"smtp.example","port":465,"tls":"implicit_tls","login_name":"test@example.test","authentication":"app_password"},"smtp":{"settings_version":1,"host":"smtp.example","port":465,"tls":"implicit_tls","login_name":"test@example.test","authentication":"app_password"}}"#).unwrap();
        let plan = hantong_secrets::SmtpPasswordCapturePlan::new(
            "acct",
            config.smtp().unwrap(),
            [1; 32],
            [2; 32],
        )
        .unwrap();
        let draft = hantong_domain::account_provisioning::NewImapAccountDraft::new(
            "acct".into(),
            None,
            None,
        )
        .unwrap();
        let review = AccountSetupReview::new(
            draft,
            plan.capture_endpoint().clone(),
            [1; 32],
            [2; 32],
            *plan.nonce(),
            1000,
            301000,
        )
        .unwrap();
        assert!(matches!(
            show_setup(
                review,
                plan.into_imap_capture_binding(),
                None,
                Theme::System
            )
            .await,
            Err(ErrorCode::ScopeDenied)
        ));
    }
    #[tokio::test(flavor = "current_thread")]
    async fn inst05_registration_and_setup_share_one_window_gate_without_opening_a_window() {
        let _occupied = WindowPermit::claim(&WINDOW_ACTIVE).unwrap();
        let registration =
            RegistrationReview::new([1; 32], 1, vec![], [2; 32], 1000, 301000).unwrap();
        assert!(matches!(
            show(registration, None, Theme::System).await,
            Err(ErrorCode::ApprovalRateLimited)
        ));
        let endpoint = hantong_domain::mail_connection::MailConnectionConfig::from_json(
            br#"{"schema_version":1,"account_id":"acct","imap":{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"user","authentication":"app_password"}}"#,
        ).unwrap().imap().clone();
        let plan =
            hantong_secrets::ImapPasswordCapturePlan::new("acct", &endpoint, [1; 32], [2; 32])
                .unwrap();
        let review = AccountSetupReview::new(
            hantong_domain::account_provisioning::NewImapAccountDraft::new(
                "acct".into(),
                None,
                None,
            )
            .unwrap(),
            endpoint,
            [1; 32],
            [2; 32],
            *plan.nonce(),
            1000,
            301000,
        )
        .unwrap();
        assert!(matches!(
            show_setup(review, plan, None, Theme::System).await,
            Err(ErrorCode::ApprovalRateLimited)
        ));
        let refresh = ScopeRefreshReview::new(
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
        assert!(matches!(
            show_scope_refresh(refresh, None, Theme::System).await,
            Err(ErrorCode::ApprovalRateLimited)
        ));
    }
}
