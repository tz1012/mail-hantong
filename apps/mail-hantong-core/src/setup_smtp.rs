//! Ephemeral, first-only SMTP setup; native UI owns all credential input.
use hantong_contracts::{
    ErrorCode,
    imap_setup::ImapSetupTls,
    smtp_setup::{SmtpSetupInput, SmtpSetupReceipt, SmtpSetupState},
};
use hantong_domain::mail_connection::{MailConnectionConfig, SmtpEndpointConfig};
use hantong_secrets::{TerminalSecretBinding, TerminalSecretHandle, TerminalSecretTicket};
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
    #[cfg(windows)]
    created: i64,
    receipt: SmtpSetupReceipt,
}

pub(crate) struct TerminalContext {
    pub account_id: String,
    pub endpoint: SmtpEndpointConfig,
    pub account_version: u64,
    #[cfg(windows)]
    pub request_id: [u8; 32],
    pub payload_hash: [u8; 32],
    pub expires_at_utc_ms: i64,
}
fn request_nonce() -> Result<[u8; 32], ErrorCode> {
    hantong_platform::random_nonce()
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
    let mut endpoint = serde_json::json!({"settings_version":1,"host":input.host(),"port":input.port(),"tls":match input.tls(){ImapSetupTls::ImplicitTls=>"implicit_tls",ImapSetupTls::StarttlsRequired=>"starttls_required"},"login_name":input.login_name(),"authentication":"app_password"});
    if let Some(sender) = input.sender_address() {
        endpoint["sender_address"] = serde_json::Value::String(sender.to_owned());
    }
    let mut imap_endpoint = endpoint.clone();
    if let Some(object) = imap_endpoint.as_object_mut() {
        object.remove("sender_address");
    }
    let bytes=serde_json::to_vec(&serde_json::json!({"schema_version":1,"account_id":input.account_id(),"imap":imap_endpoint,"smtp":endpoint})).map_err(|_|ErrorCode::InternalError)?;
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
            || existing.sender_address() != endpoint.sender_address()
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
            #[cfg(windows)]
            created,
            receipt,
        },
    );
    Ok(receipt)
}
pub(crate) fn terminal_context(
    sessions: &Sessions,
    id: [u8; 32],
) -> Result<TerminalContext, ErrorCode> {
    let session = sessions
        .borrow()
        .get(&id)
        .cloned()
        .ok_or(ErrorCode::NotFound)?;
    let payload_hash = smtp_payload_hash(session.input.account_id(), &session.endpoint, 0)?;
    Ok(TerminalContext {
        account_id: session.input.account_id().to_owned(),
        endpoint: session.endpoint,
        account_version: session.account_version,
        #[cfg(windows)]
        request_id: session.receipt.request_id,
        payload_hash,
        expires_at_utc_ms: session.receipt.expires_at_utc_ms,
    })
}

pub(crate) fn validate_terminal_context(
    store: &Store,
    sessions: &Sessions,
    id: [u8; 32],
) -> Result<TerminalContext, ErrorCode> {
    let context = terminal_context(sessions, id)?;
    let snapshot = sessions
        .borrow()
        .get(&id)
        .cloned()
        .ok_or(ErrorCode::NotFound)?;
    if snapshot.receipt.state != SmtpSetupState::Pending
        || snapshot.receipt.error_code.is_some()
        || now()? >= snapshot.receipt.expires_at_utc_ms
    {
        return Err(ErrorCode::Conflict);
    }
    validate(store, &snapshot)?;
    Ok(context)
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

/// Cancels a pending Unix terminal credential review before any provider I/O.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn cancel(sessions: &Sessions, id: [u8; 32]) -> Result<(), ErrorCode> {
    let mut map = sessions.borrow_mut();
    let session = map.get_mut(&id).ok_or(ErrorCode::NotFound)?;
    if session.receipt.state != SmtpSetupState::Pending {
        return Err(ErrorCode::Conflict);
    }
    session.receipt.state = SmtpSetupState::Denied;
    session.receipt.error_code = Some(ErrorCode::ApprovalDenied);
    Ok(())
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
#[cfg(windows)]
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
#[cfg(windows)]
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

/// Completes SMTP setup using a Secrets-owned staged terminal credential.
/// The native approval window is deliberately absent from this path.
pub(crate) async fn terminal_submit(
    store: &Store,
    root: &Path,
    sessions: &Sessions,
    id: [u8; 32],
    ticket: TerminalSecretTicket,
    handle: TerminalSecretHandle,
    expected_binding: TerminalSecretBinding,
) -> Result<(), ErrorCode> {
    let result =
        terminal_submit_inner(store, root, sessions, id, ticket, handle, expected_binding).await;
    if let Err(error) = result
        && let Some(session) = sessions.borrow_mut().get_mut(&id)
    {
        session.receipt.state = match error {
            ErrorCode::ApprovalExpired => SmtpSetupState::Expired,
            ErrorCode::ApprovalDenied => SmtpSetupState::Denied,
            ErrorCode::ApprovalRequired => SmtpSetupState::Interrupted,
            _ => SmtpSetupState::Failed,
        };
        session.receipt.error_code = Some(error);
    }
    result
}

async fn terminal_submit_inner(
    store: &Store,
    root: &Path,
    sessions: &Sessions,
    id: [u8; 32],
    ticket: TerminalSecretTicket,
    handle: TerminalSecretHandle,
    expected_binding: TerminalSecretBinding,
) -> Result<(), ErrorCode> {
    let snapshot = sessions
        .borrow()
        .get(&id)
        .cloned()
        .ok_or(ErrorCode::NotFound)?;
    validate(store, &snapshot)?;
    let hash = smtp_payload_hash(snapshot.input.account_id(), &snapshot.endpoint, 0)?;
    if ticket.binding().expires_at_utc_ms() <= now()? {
        return Err(ErrorCode::ApprovalExpired);
    }
    secrets(root)?
        .consume_terminal_smtp_credential(
            ticket,
            handle,
            expected_binding,
            snapshot.input.account_id(),
            &snapshot.endpoint,
        )
        .map_err(|_| ErrorCode::ScopeDenied)?;
    validate(store, &snapshot)?;
    let key = hantong_contracts::wire_hex::encode(&snapshot.receipt.request_id);
    let mutation = MailMutation::new(key.clone(), key, 0, hash)?;
    store.put_initial_smtp_connection(
        snapshot.input.account_id(),
        &snapshot.endpoint,
        &mutation,
        snapshot.account_version,
    )?;
    let mut map = sessions.borrow_mut();
    let session = map.get_mut(&id).ok_or(ErrorCode::InternalError)?;
    session.receipt.state = SmtpSetupState::Ready;
    session.receipt.error_code = None;
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
                #[cfg(windows)]
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

    #[test]
    fn ext01_smtp_endpoint_keeps_explicit_sender_on_smtp_only() {
        let with_sender = SmtpSetupInput::new_with_sender(
            "acct".into(),
            "smtp.example".into(),
            465,
            ImapSetupTls::ImplicitTls,
            "auth-user".into(),
            Some("alias@example.test".into()),
        )
        .unwrap();
        let explicit_endpoint = endpoint(&with_sender).unwrap();
        assert_eq!(explicit_endpoint.login_name(), "auth-user");
        assert_eq!(
            explicit_endpoint.sender_address(),
            Some("alias@example.test")
        );

        let without_sender = SmtpSetupInput::new(
            "acct".into(),
            "smtp.example".into(),
            465,
            ImapSetupTls::ImplicitTls,
            "auth@example.test".into(),
        )
        .unwrap();
        assert_eq!(endpoint(&without_sender).unwrap().sender_address(), None);
    }
}
