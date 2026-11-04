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
