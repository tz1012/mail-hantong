//! Foreground account-connection orchestration.
//!
//! This module is deliberately transport-agnostic.  The caller supplies a
//! Core-authenticated exchange (the caller must keep the same Core foreground
//! process and authenticated socket for the whole operation).  No profile,
//! scope, signer, credential, or provider response is accepted from the
//! command line.  This keeps the UX state machine separate from the existing
//! ordinary Unix relay, which is intentionally not an authority for setup.

use hantong_contracts::terminal_review::{
    TerminalReviewGet, TerminalReviewKind, TerminalReviewReceipt,
};
use hantong_contracts::{ErrorCode, account_connect::AccountConnectReceipt, wire_hex};
use serde_json::{Value, json};
use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

const MAX_ACCOUNT_ID: usize = 128;
const MAX_RESPONSE_BYTES: usize = hantong_contracts::account_connect::MAX_FRAME_BYTES;
#[cfg(any(unix, test))]
const MAX_LOGIN_URL: usize = 2048;
const TOTAL_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// The only supported foreground command shape.  No profile path, key,
/// account scope, or approval boolean is accepted here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AccountConnectArgs {
    pub provider: ConnectProvider,
    pub account_id: Option<String>,
    pub imap_request: Option<[u8; 32]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectProvider {
    Google,
    ImapSmtp,
}

impl AccountConnectArgs {
    pub(crate) fn parse(args: &[String]) -> Result<Self, ErrorCode> {
        if args.len() < 2 || args[0] != "account-connect" {
            return Err(ErrorCode::InvalidInput);
        }
        let provider = match args[1].as_str() {
            "google" => ConnectProvider::Google,
            "imap-smtp" => ConnectProvider::ImapSmtp,
            _ => return Err(ErrorCode::CapabilityUnsupported),
        };
        let mut account_id = None;
        let mut imap_request = None;
        let mut index = 2;
        while index < args.len() {
            if index + 1 >= args.len()
                || (args[index] != "--account" && args[index] != "--imap-request")
            {
                return Err(ErrorCode::InvalidInput);
            }
            let value = &args[index + 1];
            if args[index] == "--account" {
                if account_id.is_some()
                    || imap_request.is_some()
                    || value.is_empty()
                    || value.len() > MAX_ACCOUNT_ID
                    || !value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
                    })
                {
                    return Err(ErrorCode::InvalidInput);
                }
                account_id = Some(value.clone());
            } else {
                if provider != ConnectProvider::ImapSmtp
                    || account_id.is_some()
                    || imap_request.is_some()
                {
                    return Err(ErrorCode::InvalidInput);
                }
                imap_request = Some(wire_hex::decode_fixed::<32>(value)?);
                if imap_request == Some([0; 32]) {
                    return Err(ErrorCode::InvalidInput);
                }
            }
            index += 2;
        }
        Ok(Self {
            provider,
            account_id,
            imap_request,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConnectState {
    Queued,
    AwaitingSetup,
    AwaitingBrowser,
    ProviderReady,
    Staged,
    Ready,
    Cancelled,
    Expired,
    Failed,
}

fn receipt_failure(receipt: &ConnectReceipt) -> ErrorCode {
    receipt.0.error_code.unwrap_or(ErrorCode::InternalError)
}

impl ConnectState {
    fn parse(value: &str) -> Result<Self, ErrorCode> {
        match value {
            "queued" => Ok(Self::Queued),
            "awaiting_setup" => Ok(Self::AwaitingSetup),
            "awaiting_browser" => Ok(Self::AwaitingBrowser),
            "provider_ready" => Ok(Self::ProviderReady),
            "staged" => Ok(Self::Staged),
            "ready" => Ok(Self::Ready),
            "cancelled" => Ok(Self::Cancelled),
            "expired" => Ok(Self::Expired),
            "failed" => Ok(Self::Failed),
            _ => Err(ErrorCode::InternalError),
        }
    }
}

struct ConnectReceipt(AccountConnectReceipt);

impl ConnectReceipt {
    fn decode(value: Value) -> Result<Self, ErrorCode> {
        let encoded = serde_json::to_vec(&value).map_err(|_| ErrorCode::InternalError)?;
        if encoded.is_empty() || encoded.len() > MAX_RESPONSE_BYTES {
            return Err(ErrorCode::QuotaExceeded);
        }
        Ok(Self(
            AccountConnectReceipt::from_json(&encoded).map_err(|_| ErrorCode::InternalError)?,
        ))
    }

    fn validate_binding(
        &self,
        intent: &[u8; 32],
        provider: ConnectProvider,
        selected_account: Option<&str>,
    ) -> Result<(), ErrorCode> {
        if &self.0.intent_id != intent
            || self.0.provider.as_str()
                != match provider {
                    ConnectProvider::Google => "google",
                    ConnectProvider::ImapSmtp => "imap_smtp",
                }
        {
            return Err(ErrorCode::CoreUnavailable);
        }
        if let Some(expected) = selected_account
            && self.0.account_id.as_deref() != Some(expected)
        {
            return Err(ErrorCode::Conflict);
        }
        Ok(())
    }
}

/// A narrow adapter implemented by the Core foreground loop after it has
/// authenticated the fixed Default profile.  Implementations must send these
/// methods on the same authenticated connection and must not derive scope from
/// the request parameters.
#[cfg(test)]
pub(crate) trait AuthenticatedConnectExchange {
    fn exchange(&mut self, method: &str, params: Value) -> Result<Value, ErrorCode>;
}

/// Core-owned Unix exchange.  Each request authenticates a fresh socket from
/// the fixed Default profile while this process remains the foreground owner;
/// no caller supplied profile, key, scope, or socket path is accepted.
#[cfg(unix)]
pub(crate) struct UnixCoreConnectExchange {}

#[cfg(unix)]
impl UnixCoreConnectExchange {
    pub(crate) fn load() -> Result<Self, ErrorCode> {
        let _ = hantong_platform::unix_release::verified_installed_core("mail-hantong-core")?;
        Ok(Self {})
    }

    fn load_profile() -> Result<hantong_ipc_client::profile::ParsedClientProfile, ErrorCode> {
        let loaded = hantong_platform::load_default_client_profile()?;
        let profile = hantong_ipc_client::profile::ParsedClientProfile::from_json(loaded.bytes())?;
        if profile.profile() != hantong_ipc_client::profile::ProfileSelector::Default {
            return Err(ErrorCode::ScopeDenied);
        }
        Ok(profile)
    }

    pub(crate) async fn exchange(
        &self,
        request_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, ErrorCode> {
        let profile = Self::load_profile()?;
        let key_id = wire_hex::encode(profile.client_key_id());
        let selector = hantong_secrets::unix_secret_store::UnixSecretSelector::new(
            hantong_secrets::unix_secret_store::UnixSecretKind::ClientSigningKey,
            Some(&key_id),
            1,
        )
        .map_err(|_| ErrorCode::AuthRequired)?;
        let signer = hantong_secrets::unix_signing::OpaqueSigner::load(
            &hantong_secrets::unix_secret_store::UnixSecretStore::new(),
            &selector,
            *profile.client_public_key(),
        )
        .map_err(|_| ErrorCode::AuthRequired)?;
        let connection = hantong_platform::unix_transport::connect().await?;
        let now = unix_now()?;
        let client = if profile.scope_model_version() == 2 {
            let pending =
                hantong_ipc_client::unix_auth::PendingUnixAccountAuthentication::begin_connected(
                    signer,
                    connection,
                    *profile.server_public_key(),
                    *profile.registration_id(),
                    profile.profile_epoch(),
                    profile
                        .account_scope()
                        .ok_or(ErrorCode::ScopeDenied)?
                        .clone(),
                    hantong_platform::random_nonce()?,
                    now,
                )?;
            hantong_ipc_client::unix_authenticated_wire::authenticate_account_connected(
                pending, request_id,
            )
            .await?
        } else {
            let pending =
                hantong_ipc_client::unix_auth::PendingUnixAuthentication::begin_connected(
                    signer,
                    connection,
                    *profile.server_public_key(),
                    *profile.registration_id(),
                    profile.profile_epoch(),
                    profile.scope().clone(),
                    hantong_platform::random_nonce()?,
                    now,
                )?;
            hantong_ipc_client::unix_authenticated_wire::authenticate_connected(pending, request_id)
                .await?
        };
        let raw = client.exchange(request_id, method, &params).await?;
        let envelope: Value =
            serde_json::from_slice(&raw).map_err(|_| ErrorCode::CoreUnavailable)?;
        if serde_json::to_vec(&envelope).map_err(|_| ErrorCode::CoreUnavailable)? != raw {
            return Err(ErrorCode::CoreUnavailable);
        }
        let object = envelope.as_object().ok_or(ErrorCode::CoreUnavailable)?;
        if object.len() != 3
            || object.get("jsonrpc") != Some(&Value::String("2.0".into()))
            || object.get("id") != Some(&Value::String(request_id.into()))
        {
            return Err(ErrorCode::CoreUnavailable);
        }
        if let Some(error) = object.get("error") {
            if error.as_object().is_none_or(|value| value.len() != 1) {
                return Err(ErrorCode::CoreUnavailable);
            }
            let code = error
                .get("code")
                .and_then(Value::as_str)
                .ok_or(ErrorCode::CoreUnavailable)?;
            return Err(ErrorCode::ALL
                .into_iter()
                .find(|candidate| candidate.as_str() == code)
                .ok_or(ErrorCode::CoreUnavailable)?);
        }
        object
            .get("result")
            .cloned()
            .ok_or(ErrorCode::CoreUnavailable)
    }
}

#[cfg(unix)]
fn unix_now() -> Result<i64, ErrorCode> {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| ErrorCode::InternalError)?
            .as_millis(),
    )
    .map_err(|_| ErrorCode::InternalError)
}

#[cfg(windows)]
pub(crate) struct WindowsCoreConnectExchange;

#[cfg(windows)]
impl WindowsCoreConnectExchange {
    pub(crate) fn load() -> Result<Self, ErrorCode> {
        Ok(Self)
    }

    pub(crate) async fn exchange(
        &self,
        request_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, ErrorCode> {
        let connection = hantong_ipc_client::authenticate_default().await?;
        let raw = connection.exchange(request_id, method, &params).await?;
        let envelope: Value =
            serde_json::from_slice(&raw).map_err(|_| ErrorCode::CoreUnavailable)?;
        if serde_json::to_vec(&envelope).map_err(|_| ErrorCode::CoreUnavailable)? != raw {
            return Err(ErrorCode::CoreUnavailable);
        }
        let object = envelope.as_object().ok_or(ErrorCode::CoreUnavailable)?;
        if object.len() != 3
            || object.get("jsonrpc") != Some(&Value::String("2.0".into()))
            || object.get("id") != Some(&Value::String(request_id.into()))
        {
            return Err(ErrorCode::CoreUnavailable);
        }
        if let Some(error) = object.get("error") {
            if error.as_object().is_none_or(|value| value.len() != 1) {
                return Err(ErrorCode::CoreUnavailable);
            }
            let code = error
                .get("code")
                .and_then(Value::as_str)
                .ok_or(ErrorCode::CoreUnavailable)?;
            return Err(ErrorCode::ALL
                .into_iter()
                .find(|candidate| candidate.as_str() == code)
                .ok_or(ErrorCode::CoreUnavailable)?);
        }
        object
            .get("result")
            .cloned()
            .ok_or(ErrorCode::CoreUnavailable)
    }
}

/// Unix entrypoint used by the foreground Core helper.  The authenticated
/// exchange is recreated for each bounded RPC, but every exchange is made by
/// this same verified Core process and fixed Default profile.  Once complete
/// is sent, its result is returned as-is; transport uncertainty is never
/// followed by another complete call.
#[cfg(unix)]
pub(crate) async fn run_unix<F>(
    args: &AccountConnectArgs,
    no_input: bool,
    mut open_browser: F,
) -> Result<AccountConnectReceipt, ErrorCode>
where
    F: FnMut(&str) -> Result<(), ErrorCode>,
{
    require_interactive_terminal(no_input)?;
    emit_connect_features()?;
    let exchange = UnixCoreConnectExchange::load()?;
    if let Some(request) = args.imap_request {
        return run_generic(&exchange, request).await;
    }
    if args.provider == ConnectProvider::ImapSmtp {
        return run_existing_generic(
            &exchange,
            args.account_id.as_deref().ok_or(ErrorCode::InvalidInput)?,
        )
        .await;
    }
    let provider = match args.provider {
        ConnectProvider::Google => "google",
        ConnectProvider::ImapSmtp => "imap_smtp",
    };
    let selection = match args.account_id.as_ref() {
        Some(account_id) => json!({"kind": "existing", "account_id": account_id}),
        None => match args.imap_request {
            Some(request_id) => json!({
                "kind": "imap_setup",
                "request_id": wire_hex::encode(&request_id),
            }),
            None => json!({"kind": "new"}),
        },
    };
    let prepared = ConnectReceipt::decode(
        exchange
            .exchange(
                "account-connect-prepare-1",
                hantong_contracts::account_connect::PREPARE_METHOD,
                json!({"protocol_version": 1, "provider": provider, "selection": selection}),
            )
            .await?,
    )?;
    let intent = prepared.0.intent_id;
    prepared.validate_binding(&intent, args.provider, args.account_id.as_deref())?;
    if let Some(url) = prepared.0.login_url.as_deref() {
        validate_login_url(url)?;
        if let Err(error) = open_browser(url) {
            let _ = exchange
                .exchange(
                    "account-connect-cancel-1",
                    hantong_contracts::account_connect::CANCEL_METHOD,
                    json!({"protocol_version": 1, "intent_id": wire_hex::encode(&intent)}),
                )
                .await;
            return Err(error);
        }
    }
    let started = Instant::now();
    let mut last = prepared;
    let mut complete_sent = false;
    loop {
        if started.elapsed() >= TOTAL_TIMEOUT {
            if !complete_sent {
                let _ = exchange
                    .exchange(
                        "account-connect-cancel-1",
                        hantong_contracts::account_connect::CANCEL_METHOD,
                        json!({"protocol_version": 1, "intent_id": wire_hex::encode(&intent)}),
                    )
                    .await;
            }
            return Err(ErrorCode::ApprovalExpired);
        }
        match ConnectState::parse(&last.0.state)? {
            ConnectState::ProviderReady if !complete_sent => {
                complete_sent = true;
                if let Some(complete) =
                    complete_once(&exchange, intent, started + TOTAL_TIMEOUT).await?
                {
                    complete.validate_binding(
                        &intent,
                        args.provider,
                        args.account_id.as_deref(),
                    )?;
                    match ConnectState::parse(&complete.0.state)? {
                        ConnectState::Ready => return Ok(complete.0),
                        ConnectState::Staged => {}
                        ConnectState::Expired => return Err(ErrorCode::ApprovalExpired),
                        ConnectState::Cancelled => return Err(ErrorCode::ApprovalDenied),
                        ConnectState::Failed => return Err(receipt_failure(&complete)),
                        _ => return Err(ErrorCode::Conflict),
                    }
                }
            }
            ConnectState::ProviderReady => {}
            ConnectState::Ready => return Ok(last.0),
            // Staged means provisioning is still in progress. Continue
            // polling until Core reports ready; it is never a successful
            // completion receipt by itself.
            ConnectState::Staged => {}
            ConnectState::Expired => return Err(ErrorCode::ApprovalExpired),
            ConnectState::Cancelled => return Err(ErrorCode::ApprovalDenied),
            ConnectState::Failed => return Err(receipt_failure(&last)),
            ConnectState::Queued | ConnectState::AwaitingBrowser => {}
            ConnectState::AwaitingSetup => return Err(ErrorCode::Conflict),
        }
        tokio::time::sleep(POLL_INTERVAL).await;
        last = status_receipt(&exchange, intent, complete_sent, started + TOTAL_TIMEOUT).await?;
        last.validate_binding(&intent, args.provider, args.account_id.as_deref())?;
    }
}

#[cfg(windows)]
async fn run_windows(
    args: &AccountConnectArgs,
    no_input: bool,
) -> Result<AccountConnectReceipt, ErrorCode> {
    require_interactive_terminal(no_input)?;
    emit_connect_features()?;
    let exchange = WindowsCoreConnectExchange::load()?;
    if let Some(request) = args.imap_request {
        return run_generic(&exchange, request).await;
    }
    if args.provider == ConnectProvider::ImapSmtp {
        return run_existing_generic(
            &exchange,
            args.account_id.as_deref().ok_or(ErrorCode::InvalidInput)?,
        )
        .await;
    }
    let provider = match args.provider {
        ConnectProvider::Google => "google",
        ConnectProvider::ImapSmtp => "imap_smtp",
    };
    let selection = match args.account_id.as_ref() {
        Some(account_id) => json!({"kind": "existing", "account_id": account_id}),
        None => match args.imap_request {
            Some(request_id) => json!({
                "kind": "imap_setup",
                "request_id": wire_hex::encode(&request_id),
            }),
            None => json!({"kind": "new"}),
        },
    };
    let prepared = ConnectReceipt::decode(
        exchange
            .exchange(
                "account-connect-prepare-1",
                hantong_contracts::account_connect::PREPARE_METHOD,
                json!({"protocol_version": 1, "provider": provider, "selection": selection}),
            )
            .await?,
    )?;
    let intent = prepared.0.intent_id;
    prepared.validate_binding(&intent, args.provider, args.account_id.as_deref())?;
    let started = Instant::now();
    let mut last = prepared;
    let mut complete_sent = false;
    loop {
        if started.elapsed() >= TOTAL_TIMEOUT {
            if !complete_sent {
                let _ = exchange
                    .exchange(
                        "account-connect-cancel-1",
                        hantong_contracts::account_connect::CANCEL_METHOD,
                        json!({"protocol_version": 1, "intent_id": wire_hex::encode(&intent)}),
                    )
                    .await;
            }
            return Err(ErrorCode::ApprovalExpired);
        }
        match ConnectState::parse(&last.0.state)? {
            ConnectState::ProviderReady if !complete_sent => {
                complete_sent = true;
                if let Some(complete) =
                    complete_once(&exchange, intent, started + TOTAL_TIMEOUT).await?
                {
                    complete.validate_binding(
                        &intent,
                        args.provider,
                        args.account_id.as_deref(),
                    )?;
                    match ConnectState::parse(&complete.0.state)? {
                        ConnectState::Ready => return Ok(complete.0),
                        ConnectState::Staged => {}
                        ConnectState::Expired => return Err(ErrorCode::ApprovalExpired),
                        ConnectState::Cancelled => return Err(ErrorCode::ApprovalDenied),
                        ConnectState::Failed => return Err(receipt_failure(&complete)),
                        _ => return Err(ErrorCode::Conflict),
                    }
                }
            }
            ConnectState::ProviderReady => {}
            ConnectState::Ready => return Ok(last.0),
            ConnectState::Staged => {}
            ConnectState::Expired => return Err(ErrorCode::ApprovalExpired),
            ConnectState::Cancelled => return Err(ErrorCode::ApprovalDenied),
            ConnectState::Failed => return Err(receipt_failure(&last)),
            ConnectState::Queued | ConnectState::AwaitingBrowser => {}
            ConnectState::AwaitingSetup => return Err(ErrorCode::Conflict),
        }
        tokio::time::sleep(POLL_INTERVAL).await;
        last = status_receipt(&exchange, intent, complete_sent, started + TOTAL_TIMEOUT).await?;
        last.validate_binding(&intent, args.provider, args.account_id.as_deref())?;
    }
}

pub(crate) fn require_interactive_terminal(no_input: bool) -> Result<(), ErrorCode> {
    // stdout is the private canonical receipt pipe; stderr is the human UI.
    if no_input || !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(ErrorCode::HumanStepRequired);
    }
    Ok(())
}

fn emit_connect_features() -> Result<(), ErrorCode> {
    let korean = std::env::var_os("LANG")
        .and_then(|value| value.into_string().ok())
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("ko"));
    let lang = if korean {
        crate::Lang::Ko
    } else {
        crate::Lang::En
    };
    writeln!(
        io::stderr(),
        "{}",
        crate::resource(lang, "account_connect_features")
    )
    .map_err(|_| ErrorCode::CoreUnavailable)
}

#[cfg(unix)]
type PlatformConnectExchange = UnixCoreConnectExchange;
#[cfg(windows)]
type PlatformConnectExchange = WindowsCoreConnectExchange;

fn connect_lang() -> crate::Lang {
    if std::env::var("LANG")
        .ok()
        .is_some_and(|v| v.starts_with("ko"))
    {
        crate::Lang::Ko
    } else {
        crate::Lang::En
    }
}

fn smtp_metadata(
    account: String,
    deadline: Instant,
) -> Result<hantong_contracts::smtp_setup::SmtpSetupInput, ErrorCode> {
    let lang = connect_lang();
    let prompt = |key| -> Result<String, ErrorCode> {
        writeln!(io::stderr(), "{}", crate::resource(lang, key))
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        io::stderr()
            .flush()
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        hantong_platform::terminal_prompt::read_line_until(512, deadline)
    };
    let host = prompt("smtp_host")?;
    let port = prompt("smtp_port")?
        .parse()
        .map_err(|_| ErrorCode::InvalidInput)?;
    let tls = match prompt("smtp_tls")?.as_str() {
        "1" | "implicit_tls" => hantong_contracts::imap_setup::ImapSetupTls::ImplicitTls,
        "2" | "starttls_required" => hantong_contracts::imap_setup::ImapSetupTls::StarttlsRequired,
        _ => return Err(ErrorCode::InvalidInput),
    };
    let login = prompt("smtp_login")?;
    let sender = prompt("smtp_sender")?;
    hantong_contracts::smtp_setup::SmtpSetupInput::new_with_sender(
        account,
        host,
        port,
        tls,
        login,
        (!sender.is_empty()).then_some(sender),
    )
}

async fn protected_setup(
    kind: hantong_contracts::terminal_review::TerminalReviewKind,
    id: [u8; 32],
    deadline: Instant,
) -> Result<(), ErrorCode> {
    tokio::task::spawn_blocking(move || {
        let receipt = review_setup_request(
            kind,
            id,
            matches!(connect_lang(), crate::Lang::Ko),
            deadline,
        )?;
        if matches!(receipt.state.as_str(), "ready" | "provisioning") {
            Ok(())
        } else {
            Err(ErrorCode::ApprovalDenied)
        }
    })
    .await
    .map_err(|_| ErrorCode::InternalError)?
}

async fn wait_imap(id: [u8; 32], account: &str, deadline: Instant) -> Result<(), ErrorCode> {
    use hantong_contracts::imap_setup::{ImapSetupCommand, ImapSetupState};
    loop {
        let receipt = tokio::task::spawn_blocking(move || {
            hantong_ipc_client::imap_setup::exchange(ImapSetupCommand::Status(id))
        })
        .await
        .map_err(|_| ErrorCode::InternalError)??;
        if receipt.request_id() != &id || receipt.account_id() != Some(account) {
            return Err(ErrorCode::Conflict);
        }
        match receipt.state() {
            ImapSetupState::Ready => return Ok(()),
            ImapSetupState::Pending | ImapSetupState::Connecting => {}
            ImapSetupState::AuthRequired => return Err(ErrorCode::AuthRequired),
            ImapSetupState::Expired => return Err(ErrorCode::ApprovalExpired),
            ImapSetupState::Denied | ImapSetupState::Cancelled => {
                return Err(ErrorCode::ApprovalDenied);
            }
            _ => return Err(ErrorCode::Conflict),
        }
        if Instant::now() >= deadline {
            return Err(ErrorCode::ApprovalExpired);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn wait_smtp(id: [u8; 32], deadline: Instant) -> Result<(), ErrorCode> {
    use hantong_contracts::smtp_setup::{SmtpSetupCommand, SmtpSetupState};
    loop {
        let receipt = tokio::task::spawn_blocking(move || {
            hantong_ipc_client::smtp_setup::exchange(SmtpSetupCommand::Status(id))
        })
        .await
        .map_err(|_| ErrorCode::InternalError)??;
        if receipt.request_id != id {
            return Err(ErrorCode::Conflict);
        }
        if receipt.state == SmtpSetupState::Ready {
            return Ok(());
        }
        if receipt.state != SmtpSetupState::Pending {
            return Err(receipt.error_code.unwrap_or(ErrorCode::Conflict));
        }
        if Instant::now() >= deadline {
            return Err(ErrorCode::ApprovalExpired);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn run_generic(
    exchange: &PlatformConnectExchange,
    request: [u8; 32],
) -> Result<AccountConnectReceipt, ErrorCode> {
    use hantong_contracts::{
        account_connect as wire,
        smtp_setup::{SmtpSetupCommand, SmtpSetupState},
        terminal_review::TerminalReviewKind,
    };
    let prepared=ConnectReceipt::decode(exchange.exchange("account-connect-prepare-1",wire::PREPARE_METHOD,
        json!({"protocol_version":1,"provider":"imap_smtp","selection":{"kind":"imap_setup","request_id":wire_hex::encode(&request)}})).await?)?;
    let intent = prepared.0.intent_id;
    prepared.validate_binding(&intent, ConnectProvider::ImapSmtp, None)?;
    if prepared.0.state != "awaiting_setup" {
        return Err(receipt_failure(&prepared));
    }
    let account = prepared.0.account_id.clone().ok_or(ErrorCode::Conflict)?;
    let deadline = Instant::now() + TOTAL_TIMEOUT;
    let before_complete=async {
        protected_setup(TerminalReviewKind::ImapSetup,request,deadline).await?;
        wait_imap(request,&account,deadline).await?;
        let selected=account.clone();
        let input=tokio::task::spawn_blocking(move||smtp_metadata(selected,deadline)).await.map_err(|_|ErrorCode::InternalError)??;
        let smtp=tokio::task::spawn_blocking(move||hantong_ipc_client::smtp_setup::exchange(SmtpSetupCommand::Request(input))).await.map_err(|_|ErrorCode::InternalError)??;
        if !matches!(smtp.state,SmtpSetupState::Pending|SmtpSetupState::Ready) {return Err(ErrorCode::Conflict);}
        let bound=ConnectReceipt::decode(exchange.exchange("account-connect-bind-smtp-1",wire::BIND_SMTP_METHOD,
            json!({"protocol_version":1,"intent_id":wire_hex::encode(&intent),"request_id":wire_hex::encode(&smtp.request_id)})).await?)?;
        bound.validate_binding(&intent,ConnectProvider::ImapSmtp,Some(&account))?;
        match bound.0.state.as_str() {
            "awaiting_setup"=>{
                protected_setup(TerminalReviewKind::SmtpSetup,smtp.request_id,deadline).await?;
                wait_smtp(smtp.request_id,deadline).await
            },
            "provider_ready"=>Ok(()),
            _=>Err(ErrorCode::Conflict),
        }
    }.await;
    if let Err(error) = before_complete {
        let _ = exchange
            .exchange(
                "account-connect-cancel-1",
                wire::CANCEL_METHOD,
                json!({"protocol_version":1,"intent_id":wire_hex::encode(&intent)}),
            )
            .await;
        return Err(error);
    }
    // Exactly one completion; uncertainty must never repeat capture or staging.
    let mut receipt = complete_once(exchange, intent, deadline).await?;
    loop {
        if let Some(receipt) = receipt {
            receipt.validate_binding(&intent, ConnectProvider::ImapSmtp, Some(&account))?;
            match ConnectState::parse(&receipt.0.state)? {
                ConnectState::Ready => return Ok(receipt.0),
                ConnectState::Queued | ConnectState::Staged => {}
                ConnectState::Failed => return Err(receipt_failure(&receipt)),
                ConnectState::Expired => return Err(ErrorCode::ApprovalExpired),
                ConnectState::Cancelled => return Err(ErrorCode::ApprovalDenied),
                _ => return Err(ErrorCode::Conflict),
            }
        }
        if Instant::now() >= deadline {
            return Err(ErrorCode::ApprovalExpired);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
        receipt = Some(status_receipt(exchange, intent, true, deadline).await?);
    }
}

async fn run_existing_generic(
    exchange: &PlatformConnectExchange,
    account: &str,
) -> Result<AccountConnectReceipt, ErrorCode> {
    use hantong_contracts::account_connect as wire;
    let prepared = ConnectReceipt::decode(exchange.exchange("account-connect-prepare-1", wire::PREPARE_METHOD,
        json!({"protocol_version":1,"provider":"imap_smtp","selection":{"kind":"existing","account_id":account}})).await?)?;
    let id = prepared.0.intent_id;
    prepared.validate_binding(&id, ConnectProvider::ImapSmtp, Some(account))?;
    if prepared.0.state != "awaiting_setup" {
        return Err(receipt_failure(&prepared));
    }
    let deadline = Instant::now() + TOTAL_TIMEOUT;
    let setup=async {
        use hantong_contracts::smtp_setup::{SmtpSetupCommand,SmtpSetupState};
        let selected=account.to_owned();
        let input=tokio::task::spawn_blocking(move||smtp_metadata(selected,deadline)).await.map_err(|_|ErrorCode::InternalError)??;
        let smtp=tokio::task::spawn_blocking(move||hantong_ipc_client::smtp_setup::exchange(SmtpSetupCommand::Request(input))).await.map_err(|_|ErrorCode::InternalError)??;
        if !matches!(smtp.state,SmtpSetupState::Pending|SmtpSetupState::Ready){return Err(ErrorCode::Conflict);}
        let bound=ConnectReceipt::decode(exchange.exchange("account-connect-bind-smtp-1",wire::BIND_SMTP_METHOD,json!({"protocol_version":1,"intent_id":wire_hex::encode(&id),"request_id":wire_hex::encode(&smtp.request_id)})).await?)?;
        bound.validate_binding(&id,ConnectProvider::ImapSmtp,Some(account))?;
        match bound.0.state.as_str() {
            "awaiting_setup"=>{
                protected_setup(TerminalReviewKind::SmtpSetup,smtp.request_id,deadline).await?;
                wait_smtp(smtp.request_id,deadline).await?;
            },
            "provider_ready"=>{},
            _=>return Err(ErrorCode::Conflict),
        }
        protected_setup(TerminalReviewKind::AccountConnectSetup,id,deadline).await
    }.await;
    if let Err(error) = setup {
        let _ = exchange
            .exchange(
                "account-connect-cancel-1",
                wire::CANCEL_METHOD,
                json!({"protocol_version":1,"intent_id":wire_hex::encode(&id)}),
            )
            .await;
        return Err(error);
    }
    let mut complete_sent = false;
    loop {
        let status = status_receipt(exchange, id, complete_sent, deadline).await?;
        status.validate_binding(&id, ConnectProvider::ImapSmtp, Some(account))?;
        match ConnectState::parse(&status.0.state)? {
            ConnectState::Ready => return Ok(status.0),
            ConnectState::ProviderReady if !complete_sent => {
                complete_sent = true;
                if let Some(result) = complete_once(exchange, id, deadline).await? {
                    result.validate_binding(&id, ConnectProvider::ImapSmtp, Some(account))?;
                    if result.0.state == "ready" {
                        return Ok(result.0);
                    }
                }
            }
            ConnectState::Queued | ConnectState::Staged => {}
            ConnectState::Failed => return Err(receipt_failure(&status)),
            ConnectState::Expired => return Err(ErrorCode::ApprovalExpired),
            ConnectState::Cancelled => return Err(ErrorCode::ApprovalDenied),
            _ => return Err(ErrorCode::Conflict),
        }
        if Instant::now() >= deadline {
            return Err(ErrorCode::ApprovalExpired);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

fn status_retry_allowed(error: ErrorCode, completion_sent: bool, deadline: Instant) -> bool {
    completion_sent && complete_fallback_allowed(error, deadline)
}

fn complete_fallback_allowed(error: ErrorCode, deadline: Instant) -> bool {
    Instant::now() < deadline
        && matches!(
            error,
            ErrorCode::AuthRequired
                | ErrorCode::ClientSuspended
                | ErrorCode::CoreUnavailable
                | ErrorCode::VersionMismatch
        )
}

async fn complete_once(
    exchange: &PlatformConnectExchange,
    intent: [u8; 32],
    deadline: Instant,
) -> Result<Option<ConnectReceipt>, ErrorCode> {
    match exchange
        .exchange(
            "account-connect-complete-1",
            hantong_contracts::account_connect::COMPLETE_METHOD,
            json!({"protocol_version": 1, "intent_id": wire_hex::encode(&intent)}),
        )
        .await
    {
        Ok(value) => ConnectReceipt::decode(value).map(Some),
        Err(error) if complete_fallback_allowed(error, deadline) => Ok(None),
        Err(error) => Err(error),
    }
}

async fn status_receipt(
    exchange: &PlatformConnectExchange,
    intent: [u8; 32],
    completion_sent: bool,
    deadline: Instant,
) -> Result<ConnectReceipt, ErrorCode> {
    loop {
        if Instant::now() >= deadline {
            return Err(ErrorCode::ApprovalExpired);
        }
        let response = exchange
            .exchange(
                "account-connect-status-1",
                hantong_contracts::account_connect::STATUS_METHOD,
                json!({"protocol_version": 1, "intent_id": wire_hex::encode(&intent)}),
            )
            .await;
        let value = match response {
            Ok(value) => value,
            Err(error) if status_retry_allowed(error, completion_sent, deadline) => {
                tokio::time::sleep(POLL_INTERVAL).await;
                continue;
            }
            Err(error) => return Err(error),
        };
        return ConnectReceipt::decode(value);
    }
}

pub(crate) fn review_setup_request(
    kind: TerminalReviewKind,
    request_id: [u8; 32],
    korean: bool,
    deadline: Instant,
) -> Result<TerminalReviewReceipt, ErrorCode> {
    let request = TerminalReviewGet::new(kind, request_id, None, [0; 32])?;
    crate::unix_terminal_review_frontend::review_in_account_connect(
        request,
        if korean {
            crate::Lang::Ko
        } else {
            crate::Lang::En
        },
        deadline,
    )
}

/// Dedicated foreground command entrypoint. The daemon/Core owns browser
/// launching on Unix; this helper never executes an untrusted URL or shell.
pub(crate) fn run(args: &[std::ffi::OsString]) -> Result<(), ErrorCode> {
    let mut words = Vec::with_capacity(args.len());
    let mut no_input = false;
    for arg in args {
        let word = arg.to_str().ok_or(ErrorCode::InvalidInput)?;
        if word == "--no-input" {
            no_input = true;
        } else {
            words.push(word.to_owned());
        }
    }
    let parsed = AccountConnectArgs::parse(&words)?;
    #[cfg(unix)]
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .enable_io()
            .build()
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        let result = runtime.block_on(run_unix(&parsed, no_input, |_| Ok(())))?;
        if result.state != "ready" {
            return Err(ErrorCode::CoreUnavailable);
        }
        let bytes = result.to_json()?;
        io::stdout()
            .write_all(&bytes)
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        Ok(())
    }
    #[cfg(windows)]
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .enable_io()
            .build()
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        let result = runtime.block_on(run_windows(&parsed, no_input))?;
        if result.state != "ready" {
            return Err(ErrorCode::CoreUnavailable);
        }
        let bytes = result.to_json()?;
        io::stdout()
            .write_all(&bytes)
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        Ok(())
    }
}

/// Best-effort cancellation for an intent that has not reached staging.  The
/// Core endpoint remains the authority: a staged or ready intent is rejected
/// there and cannot be rolled back by this helper.
#[cfg(test)]
pub(crate) fn cancel_intent<E: AuthenticatedConnectExchange>(
    exchange: &mut E,
    intent_id: &[u8; 32],
) -> Result<(), ErrorCode> {
    let receipt = ConnectReceipt::decode(exchange.exchange(
        "app.accounts.connect.cancel",
        json!({"protocol_version": 1, "intent_id": wire_hex::encode(intent_id)}),
    )?)?;
    if receipt.0.intent_id != *intent_id {
        return Err(ErrorCode::CoreUnavailable);
    }
    match ConnectState::parse(&receipt.0.state)? {
        ConnectState::Cancelled | ConnectState::Expired => Ok(()),
        ConnectState::Staged | ConnectState::Ready => Err(ErrorCode::Conflict),
        _ => Err(ErrorCode::Conflict),
    }
}

#[cfg(any(unix, test))]
fn validate_login_url(value: &str) -> Result<(), ErrorCode> {
    if value.len() > MAX_LOGIN_URL
        || !value.starts_with("https://")
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
    {
        return Err(ErrorCode::InvalidInput);
    }
    let authority = value
        .strip_prefix("https://")
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .ok_or(ErrorCode::InvalidInput)?;
    if authority != "accounts.google.com" {
        return Err(ErrorCode::ScopeDenied);
    }
    if !value.starts_with("https://accounts.google.com/o/oauth2/v2/auth?") {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockExchange {
        response: Option<Value>,
        methods: Vec<String>,
    }
    impl AuthenticatedConnectExchange for MockExchange {
        fn exchange(&mut self, method: &str, _params: Value) -> Result<Value, ErrorCode> {
            self.methods.push(method.to_owned());
            self.response.take().ok_or(ErrorCode::InternalError)
        }
    }

    #[test]
    fn sec03_args_reject_profile_scope_and_unknown_flags() {
        let base = vec!["account-connect".into(), "google".into()];
        assert!(AccountConnectArgs::parse(&base).is_ok());
        assert!(
            AccountConnectArgs::parse(&[
                "account-connect".into(),
                "google".into(),
                "--profile".into(),
                "default.json".into(),
            ])
            .is_err()
        );
        let request = "0707070707070707070707070707070707070707070707070707070707070707";
        let parsed = AccountConnectArgs::parse(&[
            "account-connect".into(),
            "imap-smtp".into(),
            "--imap-request".into(),
            request.into(),
        ])
        .unwrap();
        assert_eq!(parsed.imap_request, Some([7; 32]));
        assert!(
            AccountConnectArgs::parse(&[
                "account-connect".into(),
                "google".into(),
                "--imap-request".into(),
                request.into(),
            ])
            .is_err()
        );
        assert!(
            AccountConnectArgs::parse(&[
                "account-connect".into(),
                "google".into(),
                "--account".into(),
                "bad/id".into(),
            ])
            .is_err()
        );
    }

    #[test]
    fn sec03_login_url_is_allowlisted_and_receipt_is_strict() {
        assert!(validate_login_url("https://accounts.google.com/o/oauth2/v2/auth?x=1").is_ok());
        assert!(matches!(
            validate_login_url("https://evil.example.test/"),
            Err(ErrorCode::ScopeDenied)
        ));
        let value = serde_json::json!({
            "protocol_version": 1,
            "intent_id": "i",
            "state": "awaiting_browser",
            "expires_at_utc_ms": 1,
            "provider": "google",
            "account_id": null,
            "login_url": "https://accounts.google.com/x",
            "error_code": null,
            "unexpected": true
        });
        assert!(ConnectReceipt::decode(value).is_err());
    }

    #[test]
    fn sec03_complete_is_only_after_provider_ready() {
        assert_eq!(
            ConnectState::parse("provider_ready"),
            Ok(ConnectState::ProviderReady)
        );
        assert!(ConnectState::parse("approved").is_err());
    }

    #[test]
    fn sec03_status_auth_retry_requires_completed_intent_and_live_deadline() {
        let live = Instant::now() + Duration::from_secs(1);
        let expired = Instant::now() - Duration::from_secs(1);
        assert!(status_retry_allowed(ErrorCode::ClientSuspended, true, live));
        assert!(status_retry_allowed(ErrorCode::AuthRequired, true, live));
        assert!(status_retry_allowed(ErrorCode::CoreUnavailable, true, live));
        assert!(status_retry_allowed(ErrorCode::VersionMismatch, true, live));
        assert!(!status_retry_allowed(
            ErrorCode::ClientSuspended,
            false,
            live
        ));
        assert!(!status_retry_allowed(
            ErrorCode::AuthRequired,
            true,
            expired
        ));
        assert!(!status_retry_allowed(ErrorCode::Conflict, true, live));
        assert!(!status_retry_allowed(ErrorCode::Offline, true, live));
    }

    #[test]
    fn sec03_complete_uncertainty_allows_only_finite_live_transport_errors() {
        let live = Instant::now() + Duration::from_secs(1);
        let expired = Instant::now() - Duration::from_secs(1);
        assert!(complete_fallback_allowed(ErrorCode::AuthRequired, live));
        assert!(complete_fallback_allowed(ErrorCode::ClientSuspended, live));
        assert!(complete_fallback_allowed(ErrorCode::CoreUnavailable, live));
        assert!(complete_fallback_allowed(ErrorCode::VersionMismatch, live));
        assert!(!complete_fallback_allowed(ErrorCode::Conflict, live));
        assert!(!complete_fallback_allowed(ErrorCode::ApprovalDenied, live));
        assert!(!complete_fallback_allowed(ErrorCode::AuthRequired, expired));
    }

    #[test]
    fn sec03_failed_receipt_preserves_stable_error_code() {
        let receipt = ConnectReceipt(AccountConnectReceipt {
            intent_id: [9; 32],
            expires_at_utc_ms: 1,
            state: "failed".into(),
            provider: hantong_contracts::account_connect::AccountConnectProvider::ImapSmtp,
            account_id: None,
            login_url: None,
            error_code: Some(ErrorCode::ScopeDenied),
        });
        assert_eq!(receipt_failure(&receipt), ErrorCode::ScopeDenied);
    }

    #[test]
    fn sec03_cancel_is_single_bound_method_and_staged_is_not_rolled_back() {
        let mut exchange = MockExchange {
            response: Some(json!({
                "protocol_version": 1,
                "intent_id": "0101010101010101010101010101010101010101010101010101010101010101",
                "state": "cancelled",
                "expires_at_utc_ms": 1,
                "provider": "google",
                "account_id": "google_0101010101010101010101010101010101010101010101010101010101010101",
                "login_url": null,
                "error_code": "APPROVAL_DENIED"
            })),
            methods: Vec::new(),
        };
        let intent = [1_u8; 32];
        assert!(cancel_intent(&mut exchange, &intent).is_ok());
        assert_eq!(exchange.methods, ["app.accounts.connect.cancel"]);

        exchange.response = Some(json!({
            "protocol_version": 1,
            "intent_id": "0101010101010101010101010101010101010101010101010101010101010101",
            "state": "staged",
            "expires_at_utc_ms": 1,
            "provider": "google",
            "account_id": "google_0101010101010101010101010101010101010101010101010101010101010101",
            "login_url": null,
            "error_code": null
        }));
        assert_eq!(
            cancel_intent(&mut exchange, &intent),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            exchange.methods,
            ["app.accounts.connect.cancel", "app.accounts.connect.cancel"]
        );
    }
}
