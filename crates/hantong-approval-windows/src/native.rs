use super::*;
mod consent;
mod lifetime;
mod presentation;
mod window;

use lifetime::{CancelOnDrop, WindowPermit};
use std::sync::{atomic::AtomicBool, mpsc};

pub(super) async fn show(
    review: RegistrationReview,
    language: Option<Language>,
    theme: Theme,
) -> Result<VerifiedRegistration, ErrorCode> {
    static WINDOW_ACTIVE: AtomicBool = AtomicBool::new(false);
    let permit = WindowPermit::claim(&WINDOW_ACTIVE)?;
    let (cancel, cancellation) = mpsc::channel();
    let _cancel_on_drop = CancelOnDrop(cancel);
    let (sender, result) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("hantong-registration-ui".into())
        .spawn(move || {
            let _permit = permit;
            let outcome = window::run(review, language, theme, cancellation);
            let _ = sender.send(outcome);
        })
        .map_err(|_| ErrorCode::InternalError)?;
    result.await.map_err(|_| ErrorCode::InternalError)?
}
