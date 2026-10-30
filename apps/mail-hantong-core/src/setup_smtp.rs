//! Ephemeral, first-only SMTP setup; native UI owns all credential input.
use hantong_contracts::{
    ErrorCode,
    imap_setup::ImapSetupTls,
    smtp_setup::{SmtpSetupInput, SmtpSetupReceipt, SmtpSetupState},
};
use hantong_domain::mail_connection::{MailConnectionConfig, SmtpEndpointConfig};
use hantong_store::{
    Store,
    mail_connection::{MailMutation, smtp_payload_hash},
};
use std::{cell::RefCell, collections::BTreeMap, path::Path};
pub(crate) type Sessions = RefCell<BTreeMap<[u8; 32], Session>>;
#[derive(Clone)]
pub(crate) struct Session {
    input: SmtpSetupInput,
    endpoint: SmtpEndpointConfig,
    account_version: u64,
    created: i64,
    receipt: SmtpSetupReceipt,
}
fn request_nonce() -> Result<[u8; 32], ErrorCode> {
    #[cfg(windows)]
    {
        hantong_platform::random_nonce()
    }
    #[cfg(not(windows))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
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
fn secrets(root: &Path) -> Result<hantong_secrets::SecretStore, ErrorCode> {
    hantong_secrets::SecretStore::open(root).map_err(|_| ErrorCode::InternalError)
}
fn endpoint(input: &SmtpSetupInput) -> Result<SmtpEndpointConfig, ErrorCode> {
    let endpoint = serde_json::json!({"settings_version":1,"host":input.host(),"port":input.port(),"tls":match input.tls(){ImapSetupTls::ImplicitTls=>"implicit_tls",ImapSetupTls::StarttlsRequired=>"starttls_required"},"login_name":input.login_name(),"authentication":"app_password"});
    let bytes=serde_json::to_vec(&serde_json::json!({"schema_version":1,"account_id":input.account_id(),"imap":endpoint,"smtp":endpoint})).map_err(|_|ErrorCode::InternalError)?;
    MailConnectionConfig::from_json(&bytes)
        .map_err(|_| ErrorCode::InvalidInput)?
        .smtp()
        .cloned()
        .ok_or(ErrorCode::InternalError)
}
pub(crate) fn request(
    store: &Store,
    root: &Path,
    sessions: &Sessions,
    input: SmtpSetupInput,
) -> Result<SmtpSetupReceipt, ErrorCode> {
    let version = store.smtp_setup_account_version(input.account_id())?;
    let endpoint = endpoint(&input)?;
    let config = store
        .load_mail_connection(input.account_id())?
        .ok_or(ErrorCode::NotFound)?;
    let state = if let Some(existing) = config.smtp() {
        if existing.host() != endpoint.host()
            || existing.port() != endpoint.port()
            || existing.tls() != endpoint.tls()
            || existing.login_name() != endpoint.login_name()
            || existing.authentication() != endpoint.authentication()
        {
            return Err(ErrorCode::Conflict);
        }
        secrets(root)?
            .verify_smtp_credential(input.account_id(), existing)
            .map_err(|_| ErrorCode::AuthRequired)?;
        SmtpSetupState::Ready
    } else {
        SmtpSetupState::Pending
    };
    let created = now()?;
    let id = request_nonce()?;
    let receipt = SmtpSetupReceipt {
        request_id: id,
        expires_at_utc_ms: created
            .checked_add(300000)
            .ok_or(ErrorCode::InternalError)?,
        state,
        error_code: None,
    };
    let mut map = sessions.borrow_mut();
    if let Some(s) = map.values().find(|s| {
        s.input == input
            && s.receipt.state == SmtpSetupState::Pending
            && created < s.receipt.expires_at_utc_ms
    }) {
        return Ok(s.receipt);
    }
    if map.len() >= 32 {
        let old = map
            .iter()
            .find(|(_, s)| {
                s.receipt.state != SmtpSetupState::Pending || s.receipt.expires_at_utc_ms <= created
            })
            .map(|(id, _)| *id)
            .ok_or(ErrorCode::QuotaExceeded)?;
        map.remove(&old);
    }
    map.insert(
        id,
        Session {
            input,
            endpoint,
            account_version: version,
            created,
            receipt,
        },
    );
    Ok(receipt)
}
pub(crate) fn matches(sessions: &Sessions, id: [u8; 32], input: &SmtpSetupInput) -> bool {
    sessions
        .borrow()
        .get(&id)
        .is_some_and(|s| &s.input == input)
}
pub(crate) fn status(
    sessions: &Sessions,
    id: [u8; 32],
    active: bool,
) -> Result<SmtpSetupReceipt, ErrorCode> {
    let mut map = sessions.borrow_mut();
    let s = map.get_mut(&id).ok_or(ErrorCode::NotFound)?;
    if s.receipt.state == SmtpSetupState::Pending {
        if now()? >= s.receipt.expires_at_utc_ms {
            s.receipt.state = SmtpSetupState::Expired;
            s.receipt.error_code = Some(ErrorCode::ApprovalExpired)
        } else if !active {
            s.receipt.state = SmtpSetupState::Interrupted;
            s.receipt.error_code = Some(ErrorCode::ApprovalRequired)
        }
    }
    Ok(s.receipt)
}
fn validate(store: &Store, s: &Session) -> Result<(), ErrorCode> {
    if now()? >= s.receipt.expires_at_utc_ms {
        return Err(ErrorCode::ApprovalExpired);
    }
    if store.smtp_setup_account_version(s.input.account_id())? != s.account_version {
        return Err(ErrorCode::Conflict);
    }
    let current = store
        .load_mail_connection(s.input.account_id())?
        .ok_or(ErrorCode::NotFound)?;
    if current.smtp().is_some() {
        return Err(ErrorCode::Conflict);
    }
    Ok(())
}
pub(crate) async fn run(
    store: &Store,
    root: &Path,
    sessions: &Sessions,
    id: [u8; 32],
) -> Result<(), ErrorCode> {
    let snapshot = sessions
        .borrow()
        .get(&id)
        .cloned()
        .ok_or(ErrorCode::NotFound)?;
    let result = finish(store, root, &snapshot).await;
    let mut map = sessions.borrow_mut();
    let s = map.get_mut(&id).ok_or(ErrorCode::InternalError)?;
    s.receipt.state = match result {
        Ok(()) => SmtpSetupState::Ready,
        Err(ErrorCode::ApprovalDenied) => SmtpSetupState::Denied,
        Err(ErrorCode::ApprovalExpired) => SmtpSetupState::Expired,
        Err(ErrorCode::ApprovalRequired) => SmtpSetupState::Interrupted,
        Err(_) => SmtpSetupState::Failed,
    };
    s.receipt.error_code = result.err();
    Ok(())
}
async fn finish(store: &Store, root: &Path, s: &Session) -> Result<(), ErrorCode> {
    validate(store, s)?;
    let hash = smtp_payload_hash(s.input.account_id(), &s.endpoint, 0)?;
    let plan = hantong_secrets::SmtpPasswordCapturePlan::new(
        s.input.account_id(),
        &s.endpoint,
        s.receipt.request_id,
        hash,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    let draft = hantong_domain::account_provisioning::NewImapAccountDraft::new(
        s.input.account_id().to_owned(),
        None,
        None,
    )
    .map_err(|_| ErrorCode::InvalidInput)?;
    let capture = hantong_approval::AccountSetupReview::new(
        draft,
        plan.capture_endpoint().clone(),
        s.receipt.request_id,
        hash,
        *plan.nonce(),
        s.created,
        s.receipt.expires_at_utc_ms,
    )?;
    let review =
        hantong_approval::SmtpSetupReview::new(capture, s.endpoint.clone(), s.account_version)?;
    let config = store.load_config()?.ok_or(ErrorCode::InternalError)?;
    let verified = hantong_approval_windows::review_smtp_setup(
        review.clone(),
        plan,
        config.gui.language,
        config.gui.theme,
    )
    .await?;
    let pending = verified.consume(&review)?;
    validate(store, s)?;
    secrets(root)?
        .store_pending_smtp_credential(pending, s.input.account_id(), &s.endpoint)
        .map_err(|_| ErrorCode::InternalError)?;
    let key = hantong_contracts::wire_hex::encode(&s.receipt.request_id);
    let mutation = MailMutation::new(key.clone(), key, 0, hash)?;
    store.put_initial_smtp_connection(
        s.input.account_id(),
        &s.endpoint,
        &mutation,
        s.account_version,
    )?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn setup01_ephemeral_smtp_progress_expires_and_cannot_resume_after_restart() {
        let endpoint = endpoint(
            &SmtpSetupInput::new(
                "acct".into(),
                "smtp.example".into(),
                465,
                ImapSetupTls::ImplicitTls,
                "user@example.test".into(),
            )
            .unwrap(),
        )
        .unwrap();
        let input = SmtpSetupInput::new(
            "acct".into(),
            "smtp.example".into(),
            465,
            ImapSetupTls::ImplicitTls,
            "user@example.test".into(),
        )
        .unwrap();
        let now = now().unwrap();
        let sessions = RefCell::new(BTreeMap::new());
        let receipt = SmtpSetupReceipt {
            request_id: [1; 32],
            expires_at_utc_ms: now + 10000,
            state: SmtpSetupState::Pending,
            error_code: None,
        };
        sessions.borrow_mut().insert(
            [1; 32],
            Session {
                input,
                endpoint,
                account_version: 1,
                created: now,
                receipt,
            },
        );
        assert_eq!(
            status(&sessions, [1; 32], true).unwrap().state,
            SmtpSetupState::Pending
        );
        assert_eq!(
            status(&sessions, [1; 32], false).unwrap().state,
            SmtpSetupState::Interrupted
        );
        assert_eq!(
            status(&RefCell::new(BTreeMap::new()), [1; 32], false),
            Err(ErrorCode::NotFound)
        );
        sessions.borrow_mut().get_mut(&[1; 32]).unwrap().receipt = SmtpSetupReceipt {
            expires_at_utc_ms: now - 1,
            ..receipt
        };
        assert_eq!(
            status(&sessions, [1; 32], true).unwrap().state,
            SmtpSetupState::Expired
        );
    }
}
