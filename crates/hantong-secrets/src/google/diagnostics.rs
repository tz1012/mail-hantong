//! Closed OAuth diagnostics. Never accept URLs, identifiers, or free-form text.
use super::{ErrorCode, Instant, SecretJson, http};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy)]
pub(super) enum Stage {
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    CallbackWait,
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    CallbackTimeout,
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    CallbackValidation,
    TokenExchange,
    TokenResponse,
    RefreshCredential,
    Identity,
    AttachmentMetadata,
    AttachmentIdentifierLength,
    AttachmentRoute,
    AttachmentTransport,
    AttachmentResponse,
    AttachmentDecode,
}
impl Stage {
    fn name(self) -> &'static str {
        match self {
            #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
            Self::CallbackWait => "callback_wait",
            #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
            Self::CallbackTimeout => "callback_timeout",
            #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
            Self::CallbackValidation => "callback_validation",
            Self::TokenExchange => "token_exchange",
            Self::TokenResponse => "token_response",
            Self::RefreshCredential => "refresh_credential",
            Self::Identity => "identity",
            Self::AttachmentMetadata => "attachment_metadata",
            Self::AttachmentIdentifierLength => "attachment_identifier_length",
            Self::AttachmentRoute => "attachment_route",
            Self::AttachmentTransport => "attachment_transport",
            Self::AttachmentResponse => "attachment_response",
            Self::AttachmentDecode => "attachment_decode",
        }
    }
}

/// Fixed stages for the Unix HTTP boundary. These values deliberately avoid
/// carrying provider text or transport-library error messages.
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy)]
pub(super) enum TransportStage {
    Dns,
    Connect,
    Tls,
    Write,
    Read,
    Framing,
}

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
impl TransportStage {
    fn name(self) -> &'static str {
        match self {
            Self::Dns => "dns",
            Self::Connect => "connect",
            Self::Tls => "tls",
            Self::Write => "write",
            Self::Read => "read",
            Self::Framing => "framing",
        }
    }
}

/// Closed transport failure categories. The stable ErrorCode remains the
/// machine-facing category; this kind identifies the failure boundary.
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy)]
pub(super) enum TransportFailureKind {
    Timeout,
    UnexpectedEof,
    Io,
    Tls,
    InvalidFrame,
    Quota,
}

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
impl TransportFailureKind {
    fn name(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::UnexpectedEof => "unexpected_eof",
            Self::Io => "io",
            Self::Tls => "tls",
            Self::InvalidFrame => "invalid_frame",
            Self::Quota => "quota",
        }
    }
}

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
fn endpoint_name(host: http::Host) -> &'static str {
    match host {
        http::Host::Token => "token",
        http::Host::Identity => "identity",
        http::Host::Gmail => "gmail",
    }
}

/// Emits one bounded transport diagnostic. Only closed enums and stable
/// ErrorCode values cross this boundary; no URL, path, ID, or provider text.
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
pub(super) fn transport_failure(
    host: http::Host,
    stage: TransportStage,
    kind: TransportFailureKind,
    error: ErrorCode,
) {
    emit(serde_json::json!({
        "schema_version": 1,
        "event": "google_transport_stage",
        "endpoint": endpoint_name(host),
        "stage": stage.name(),
        "kind": kind.name(),
        "outcome": "failed",
        "error_code": error.as_str(),
    }));
}

static NEXT_ATTEMPT: AtomicU64 = AtomicU64::new(1);
pub(super) struct Trace {
    attempt: u64,
    started: Instant,
}
impl Trace {
    pub(super) fn new() -> Self {
        Self {
            attempt: NEXT_ATTEMPT.fetch_add(1, Ordering::Relaxed),
            started: Instant::now(),
        }
    }
    fn record(
        &self,
        stage: Stage,
        outcome: &'static str,
        error: Option<ErrorCode>,
    ) -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "event": if stage.name().starts_with("attachment_") { "google_attachment_stage" } else { "google_oauth_stage" },
            "attempt": self.attempt,
            "observed_at_utc_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(i64::MAX as u128) as u64,
            "elapsed_ms": self.started.elapsed().as_millis().min(i64::MAX as u128) as u64,
            "stage": stage.name(),
            "outcome": outcome,
            "error_code": error.map(ErrorCode::as_str),
        })
    }
    pub(super) fn start(&self, stage: Stage) {
        emit(self.record(stage, "started", None));
    }
    pub(super) fn step<T>(
        &self,
        stage: Stage,
        action: impl FnOnce() -> Result<T, ErrorCode>,
    ) -> Result<T, ErrorCode> {
        let result = action();
        self.result(stage, &result);
        result
    }
    pub(super) fn result<T>(&self, stage: Stage, result: &Result<T, ErrorCode>) {
        emit(self.record(
            stage,
            if result.is_ok() {
                "succeeded"
            } else {
                "failed"
            },
            result.as_ref().err().copied(),
        ));
    }
    pub(super) fn response(&self, response: &http::Response) {
        let mut record = self.record(Stage::TokenExchange, "response", None);
        record["http_status"] = response.status.into();
        // Only an exact match from this closed set can leave Secrets. Never
        // serialize the provider's error_description, response, or URL.
        let parsed = SecretJson::parse(response.body.as_slice()).ok();
        let provider_error = parsed
            .as_ref()
            .map(
                |json| match json.0.get("error").and_then(serde_json::Value::as_str) {
                    Some("invalid_client") => "invalid_client",
                    Some("invalid_grant") => "invalid_grant",
                    Some("invalid_request") => "invalid_request",
                    Some("access_denied") => "access_denied",
                    Some("unauthorized_client") => "unauthorized_client",
                    Some(_) => "other",
                    None => "none",
                },
            )
            .unwrap_or("unparseable");
        record["provider_error"] = provider_error.into();
        let reason = match parsed
            .as_ref()
            .and_then(|json| json.0.get("error_description"))
            .and_then(serde_json::Value::as_str)
        {
            Some("client_secret is missing.")
            | Some("Missing required parameter: client_secret") => "client_secret_missing",
            Some("Invalid code verifier.") => "code_verifier_invalid",
            _ => "unspecified",
        };
        record["provider_reason"] = reason.into();
        emit(record);
    }
}

fn emit(record: serde_json::Value) {
    // Logging failure must not change the authorization result or panic.
    if let Ok(line) = serde_json::to_string(&record) {
        let _ = writeln!(std::io::stderr().lock(), "{line}");
        #[cfg(test)]
        CAPTURE.with(|capture| {
            if let Some(lines) = capture.borrow_mut().as_mut() {
                lines.push(line);
            }
        });
    }
}

#[cfg(test)]
thread_local! {
    static CAPTURE: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(super) fn capture<T>(operation: impl FnOnce() -> T) -> (T, Vec<String>) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            CAPTURE.with(|c| *c.borrow_mut() = None);
        }
    }
    CAPTURE.with(|c| *c.borrow_mut() = Some(Vec::new()));
    let _reset = Reset;
    let result = operation();
    let lines = CAPTURE.with(|c| c.borrow_mut().take().unwrap());
    (result, lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec08_transport_diagnostic_uses_closed_allowlists() {
        let (_, lines) = capture(|| {
            for host in [http::Host::Token, http::Host::Identity, http::Host::Gmail] {
                for stage in [
                    TransportStage::Dns,
                    TransportStage::Connect,
                    TransportStage::Tls,
                    TransportStage::Write,
                    TransportStage::Read,
                    TransportStage::Framing,
                ] {
                    for kind in [
                        TransportFailureKind::Timeout,
                        TransportFailureKind::UnexpectedEof,
                        TransportFailureKind::Io,
                        TransportFailureKind::Tls,
                        TransportFailureKind::InvalidFrame,
                        TransportFailureKind::Quota,
                    ] {
                        transport_failure(host, stage, kind, ErrorCode::Offline);
                    }
                }
            }
        });
        assert_eq!(lines.len(), 3 * 6 * 6);
        for line in lines {
            let value: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(value["event"], "google_transport_stage");
            assert_eq!(value["outcome"], "failed");
            assert_eq!(value["error_code"], "OFFLINE");
            for secret in [
                "oauth2.googleapis.com",
                "authorization_code",
                "access_token",
                "refresh_token",
                "client_secret",
                "provider_error",
            ] {
                assert!(!line.contains(secret), "diagnostic leaked {secret}");
            }
        }
    }
}
