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
