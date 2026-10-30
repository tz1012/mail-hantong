//! Explicit Google setup: opaque pending credentials never leave Secrets.
use hantong_contracts::{
    ErrorCode,
    google_setup::{GoogleSetupInput, GoogleSetupReceipt},
};
use hantong_store::Store;
use std::{cell::RefCell, collections::BTreeMap, path::Path};
pub(crate) type Sessions = RefCell<BTreeMap<[u8; 32], Session>>;
#[derive(Clone)]
pub(crate) struct Session {
    input: GoogleSetupInput,
    client_id: String,
    receipt: GoogleSetupReceipt,
}
fn now() -> Result<i64, ErrorCode> {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| ErrorCode::InternalError)?
            .as_millis(),
    )
    .map_err(|_| ErrorCode::InternalError)
}
pub(crate) fn request(
    store: &Store,
    sessions: &Sessions,
    input: GoogleSetupInput,
) -> Result<GoogleSetupReceipt, ErrorCode> {
    input.validate()?;
    let client_id = if let Some(account) = &input.reauthorize_account {
        let binding = store.google_account_binding(account)?;
        if input
            .client_id
            .as_ref()
            .is_some_and(|id| id != &binding.client_id)
        {
            return Err(ErrorCode::Conflict);
        }
        binding.client_id
    } else {
        input
            .client_id
            .clone()
            .or(store.google_client_id()?)
            .ok_or(ErrorCode::HumanStepRequired)?
    };
    hantong_contracts::google::validate_google_client_id(&client_id)?;
    let time = now()?;
    let mut sessions = sessions.borrow_mut();
    sessions.retain(|_, s| s.receipt.expires_at_utc_ms > time);
    if sessions.len() >= 64 {
        return Err(ErrorCode::QuotaExceeded);
    }
    let id = hantong_platform::random_nonce()?;
    let receipt = GoogleSetupReceipt {
        request_id: id,
        expires_at_utc_ms: time.checked_add(600000).ok_or(ErrorCode::InternalError)?,
        state: "queued".into(),
        account_id: None,
        login_url: None,
        error_code: None,
    };
    sessions.insert(
        id,
        Session {
            input,
            client_id,
            receipt: receipt.clone(),
        },
    );
    Ok(receipt)
}
pub(crate) fn matches(sessions: &Sessions, id: [u8; 32], input: &GoogleSetupInput) -> bool {
    sessions
        .borrow()
        .get(&id)
        .is_some_and(|s| s.input == *input)
}
pub(crate) fn status(sessions: &Sessions, id: [u8; 32]) -> Result<GoogleSetupReceipt, ErrorCode> {
    let mut sessions = sessions.borrow_mut();
    let s = sessions.get_mut(&id).ok_or(ErrorCode::NotFound)?;
    if now()? >= s.receipt.expires_at_utc_ms
        && matches!(
            s.receipt.state.as_str(),
            "queued" | "awaiting_browser" | "awaiting_review"
        )
    {
        s.receipt.state = "expired".into();
        s.receipt.login_url = None;
        s.receipt.error_code = Some(ErrorCode::ApprovalExpired)
    }
    Ok(s.receipt.clone())
}
fn update(
    sessions: &Sessions,
    id: [u8; 32],
    state: &str,
    url: Option<String>,
    account: Option<String>,
    error: Option<ErrorCode>,
) -> Result<(), ErrorCode> {
    let mut sessions = sessions.borrow_mut();
    let s = sessions.get_mut(&id).ok_or(ErrorCode::NotFound)?;
    if now()? >= s.receipt.expires_at_utc_ms {
        return Err(ErrorCode::ApprovalExpired);
    }
    s.receipt.state = state.into();
    s.receipt.login_url = url;
    s.receipt.account_id = account;
    s.receipt.error_code = error;
    s.receipt.validate()
}
pub(crate) async fn run(
    store: &Store,
    root: &Path,
    sessions: &Sessions,
    id: [u8; 32],
) -> Result<(), ErrorCode> {
    let s = sessions
        .borrow()
        .get(&id)
        .cloned()
        .ok_or(ErrorCode::NotFound)?;
    let result = connect(store, root, sessions, id, &s).await;
    if let Err(e) = result {
        let state = match e {
            ErrorCode::ApprovalExpired => "expired",
            ErrorCode::ApprovalDenied => "denied",
            ErrorCode::ApprovalRequired => "interrupted",
            _ => "failed",
        };
        if let Some(s) = sessions.borrow_mut().get_mut(&id) {
            s.receipt.state = state.into();
            s.receipt.login_url = None;
            s.receipt.error_code = Some(e)
        }
    }
    result
}
async fn connect(
    store: &Store,
    root: &Path,
    sessions: &Sessions,
    id: [u8; 32],
    s: &Session,
) -> Result<(), ErrorCode> {
    let auth =
        hantong_secrets::google::begin_google_authorization(&s.client_id, s.input.force_consent)
            .await?;
    let url = auth.authorization_url().to_owned();
    update(
        sessions,
        id,
        "awaiting_browser",
        Some(url.clone()),
        None,
        None,
    )?;
    hantong_platform::google_browser::open_google_authorization_url(&url)?;
    let pending = auth.complete().await?;
    let identity = pending.identity().clone();
    identity.validate()?;
    if s.input
        .reauthorize_account
        .as_ref()
        .is_some_and(|a| a != &identity.account_id)
    {
        return Err(ErrorCode::ScopeDenied);
    }
    let expected = store
        .google_account_for_subject(&identity.subject)?
        .map_or(0, |b| b.credential_version);
    let binding = pending.binding(expected.checked_add(1).ok_or(ErrorCode::Conflict)?)?;
    update(
        sessions,
        id,
        "awaiting_review",
        None,
        Some(binding.account_id.clone()),
        None,
    )?;
    let created = now()?;
    let review = hantong_approval::GoogleSetupReview {
        request_id: id,
        identity: identity.clone(),
        binding: binding.clone(),
        expected_version: expected,
        display_name: s.input.display_name.clone(),
        created_at_utc_ms: created,
        expires_at_utc_ms: (created + 300000).min(s.receipt.expires_at_utc_ms),
    };
    review.validate()?;
    let config = store.load_config()?.ok_or(ErrorCode::InternalError)?;
    let verified = hantong_approval_windows::review_google_setup(
        review.clone(),
        pending,
        config.gui.language,
        config.gui.theme,
    )
    .await?;
    let pending = verified.consume(&review)?;
    if store
        .google_account_for_subject(&identity.subject)?
        .map_or(0, |b| b.credential_version)
        != expected
    {
        return Err(ErrorCode::Conflict);
    }
    let secrets = hantong_secrets::SecretStore::open(root).map_err(|_| ErrorCode::InternalError)?;
    pending.store(&secrets, &binding)?;
    let actual = store.activate_google_account(
        &identity,
        &s.client_id,
        expected,
        s.input.display_name.as_deref(),
    )?;
    if actual != binding {
        return Err(ErrorCode::InternalError);
    }
    update(sessions, id, "ready", None, Some(binding.account_id), None)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acc02_google_setup_status_is_bounded_terminal_and_does_not_relaunch_browser() {
        let input = GoogleSetupInput {
            client_id: Some("fixture.apps.googleusercontent.com".into()),
            display_name: None,
            reauthorize_account: None,
            force_consent: false,
        };
        let receipt = GoogleSetupReceipt {
            request_id: [1; 32],
            expires_at_utc_ms: 1,
            state: "awaiting_browser".into(),
            account_id: None,
            login_url: Some("https://accounts.google.com/o/oauth2/v2/auth?state=synthetic".into()),
            error_code: None,
        };
        let sessions = RefCell::new(BTreeMap::from([(
            [1; 32],
            Session {
                input: input.clone(),
                client_id: input.client_id.clone().unwrap(),
                receipt,
            },
        )]));
        let expired = status(&sessions, [1; 32]).unwrap();
        assert_eq!(expired.state, "expired");
        assert_eq!(expired.error_code, Some(ErrorCode::ApprovalExpired));
        assert!(expired.login_url.is_none());
        expired.validate().unwrap();
        assert_eq!(status(&sessions, [1; 32]).unwrap(), expired);
        assert_eq!(status(&sessions, [2; 32]), Err(ErrorCode::NotFound));
        assert!(matches(&sessions, [1; 32], &input));
    }
}
