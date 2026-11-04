use super::*;
#[cfg(not(any(windows, test)))]
use crate::SensitiveBuffer;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use ring::rand::SecureRandom;
#[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub struct GoogleAuthorization {
    url: SensitiveBuffer,
    #[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
    listener: tokio::net::TcpListener,
    #[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
    state: SecretText,
    #[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
    verifier: SecretText,
    #[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
    client_id: String,
    #[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
    access_mode: GoogleAccessMode,
    #[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
    client_secret: Option<GoogleClientSecret>,
    #[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
    redirect: String,
    #[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
    deadline: Instant,
}
impl std::fmt::Debug for GoogleAuthorization {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GoogleAuthorization(<redacted>)")
    }
}
pub async fn begin_google_authorization(
    client_id: &str,
    force_consent: bool,
) -> Result<GoogleAuthorization, ErrorCode> {
    begin_google_authorization_for_mode(client_id, force_consent, GoogleAccessMode::ReadOnly).await
}
pub async fn begin_google_authorization_for_mode(
    client_id: &str,
    force_consent: bool,
    access_mode: GoogleAccessMode,
) -> Result<GoogleAuthorization, ErrorCode> {
    validate_google_client_id(client_id)?;
    #[cfg(not(any(windows, test, target_os = "linux", target_os = "macos")))]
    {
        let _ = (force_consent, access_mode);
        Err(ErrorCode::CapabilityUnsupported)
    }
    #[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
    {
        let mut random = SensitiveBuffer(vec![0; 64]);
        #[cfg(windows)]
        crate::windows_impl::random_bytes(&mut random.0).map_err(|_| ErrorCode::InternalError)?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        ring::rand::SystemRandom::new()
            .fill(&mut random.0)
            .map_err(|_| ErrorCode::InternalError)?;
        let state = SecretText::from_string(crypto::base64url(&random.0[..32]))?;
        let verifier = SecretText::from_string(crypto::base64url(&random.0[32..]))?;
        let challenge = crypto::base64url(&crypto::sha256(verifier.0.as_slice())?);
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| ErrorCode::Offline)?;
        let redirect = format!(
            "http://127.0.0.1:{}/google/oauth/callback",
            listener
                .local_addr()
                .map_err(|_| ErrorCode::Offline)?
                .port()
        );
        let url = format!(
            "https://accounts.google.com/o/oauth2/v2/auth?{}",
            std::str::from_utf8(
                form(&[
                    ("client_id", client_id),
                    ("redirect_uri", &redirect),
                    ("response_type", "code"),
                    ("scope", &access_mode.required_scopes().join(" ")),
                    ("access_type", "offline"),
                    (
                        "prompt",
                        if force_consent {
                            "consent select_account"
                        } else {
                            "select_account"
                        }
                    ),
                    ("state", state.text()),
                    ("code_challenge", &challenge),
                    ("code_challenge_method", "S256")
                ])
                .as_slice()
            )
            .map_err(|_| ErrorCode::InternalError)?
        );
        Ok(GoogleAuthorization {
            url: SensitiveBuffer(url.into_bytes()),
            listener,
            state,
            verifier,
            client_id: client_id.into(),
            access_mode,
            client_secret: None,
            redirect,
            deadline: Instant::now() + Duration::from_secs(300),
        })
    }
}
impl GoogleAuthorization {
    /// Only an opaque, client-bound native capture can supply this credential.
    pub fn with_client_secret(mut self, secret: GoogleClientSecret) -> Result<Self, ErrorCode> {
        #[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
        {
            if !secret.matches(&self.client_id) {
                return Err(ErrorCode::ScopeDenied);
            }
            self.client_secret = Some(secret);
            Ok(self)
        }
        #[cfg(not(any(windows, test, target_os = "linux", target_os = "macos")))]
        {
            let _ = (&mut self, secret);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }

    pub fn authorization_url(&self) -> &str {
        std::str::from_utf8(self.url.as_slice()).unwrap_or("")
    }
    pub async fn complete(self) -> Result<PendingGoogleCredential, ErrorCode> {
        #[cfg(not(any(windows, test, target_os = "linux", target_os = "macos")))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
        {
            let trace = diagnostics::Trace::new();
            trace.start(diagnostics::Stage::CallbackWait);
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                trace.result::<()>(
                    diagnostics::Stage::CallbackTimeout,
                    &Err(ErrorCode::AuthRequired),
                );
                trace.result::<()>(
                    diagnostics::Stage::CallbackWait,
                    &Err(ErrorCode::AuthRequired),
                );
                return Err(ErrorCode::AuthRequired);
            }
            let code = tokio::time::timeout(remaining, async {
                let (mut stream, peer) = self
                    .listener
                    .accept()
                    .await
                    .map_err(|_| ErrorCode::AuthRequired)?;
                if !peer.ip().is_loopback() {
                    return Err(ErrorCode::AuthRequired);
                }
                let mut request = SensitiveBuffer(vec![0; 8193]);
                let mut used = 0;
                loop {
                    let n = stream
                        .read(&mut request.0[used..])
                        .await
                        .map_err(|_| ErrorCode::AuthRequired)?;
                    if n == 0 {
                        return Err(ErrorCode::AuthRequired);
                    }
                    used += n;
                    if used > 8192 {
                        return Err(ErrorCode::AuthRequired);
                    }
                    if request.0[..used].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let host = format!(
                    "127.0.0.1:{}",
                    self.listener
                        .local_addr()
                        .map_err(|_| ErrorCode::AuthRequired)?
                        .port()
                );
                let result = callback(&request.0[..used], &host, self.state.text());
                trace.result(diagnostics::Stage::CallbackValidation, &result);
                let reply = callback_reply(result.is_ok());
                let _ = stream.write_all(&reply).await;
                let _ = stream.shutdown().await;
                result
            })
            .await
            .map_err(|_| {
                trace.result::<()>(
                    diagnostics::Stage::CallbackTimeout,
                    &Err(ErrorCode::AuthRequired),
                );
                ErrorCode::AuthRequired
            })
            .and_then(|result| result);
            trace.result(diagnostics::Stage::CallbackWait, &code);
            let code = code?;
            // Closing the only listener before exchange makes replay impossible.
            drop(self.listener);
            let deadline = self.deadline;
            tokio::task::spawn_blocking(move || {
                complete_code_traced_for_mode(
                    &http::NativeTransport,
                    (&self.client_id, self.client_secret),
                    &self.redirect,
                    &self.verifier,
                    code,
                    deadline,
                    &trace,
                    self.access_mode,
                )
            })
            .await
            .map_err(|_| ErrorCode::InternalError)?
        }
    }
}
#[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
fn callback_reply(valid: bool) -> Vec<u8> {
    let (status, body) = if valid {
        (
            "200 OK",
            include_str!("../../resources/google-callback-ok.html"),
        )
    } else {
        (
            "400 Bad Request",
            include_str!("../../resources/google-callback-error.html"),
        )
    };
    format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",body.len()).into_bytes()
}

#[cfg(test)]
#[test]
fn sec08_google_callback_page_is_static_bilingual_no_reflection_or_external_content() {
    for valid in [true, false] {
        let response = String::from_utf8(callback_reply(valid)).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(headers.contains(&format!("Content-Length: {}", body.len())));
        assert!(headers.contains("no-store") && headers.contains("no-referrer"));
        assert!(headers.contains("default-src 'none'"));
        assert!(body.contains("한통") && body.contains("HanTong"));
        for forbidden in [
            "<script",
            "http://",
            "https://",
            "state=",
            "code=",
            "access_token",
            "refresh_token",
        ] {
            assert!(!body.contains(forbidden));
        }
        if valid {
            assert!(body.contains("not complete yet"));
        }
    }
}

#[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
pub(super) fn form(values: &[(&str, &str)]) -> SensitiveBuffer {
    const HEX: &[u8] = b"0123456789ABCDEF";
    let size = values.iter().map(|(k, v)| k.len() + v.len() * 3 + 2).sum();
    let mut out = SensitiveBuffer(Vec::with_capacity(size));
    for (i, (k, v)) in values.iter().enumerate() {
        if i > 0 {
            out.0.push(b'&')
        }
        out.0.extend_from_slice(k.as_bytes());
        out.0.push(b'=');
        for b in v.bytes() {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                out.0.push(b)
            } else {
                out.0.push(b'%');
                out.0.push(HEX[(b >> 4) as usize]);
                out.0.push(HEX[(b & 15) as usize]);
            }
        }
    }
    out
}
#[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
pub(super) fn encode(value: &str) -> String {
    const HEX: &[u8] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(value.len() * 3);
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char)
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 15) as usize] as char)
        }
    }
    out
}
#[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
fn decode(value: &str) -> Result<String, ErrorCode> {
    let mut out = SensitiveBuffer(Vec::with_capacity(value.len()));
    let mut chars = value.bytes();
    while let Some(b) = chars.next() {
        match b {
            b'%' => {
                let a = chars
                    .next()
                    .and_then(|b| (b as char).to_digit(16))
                    .ok_or(ErrorCode::AuthRequired)?;
                let b = chars
                    .next()
                    .and_then(|b| (b as char).to_digit(16))
                    .ok_or(ErrorCode::AuthRequired)?;
                out.0.push((a * 16 + b) as u8)
            }
            b'+' => out.0.push(b' '),
            _ => out.0.push(b),
        }
    }
    String::from_utf8(std::mem::take(&mut out.0)).map_err(|e| {
        let mut bytes = e.into_bytes();
        wipe(&mut bytes);
        ErrorCode::AuthRequired
    })
}
#[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
pub(super) fn callback(request: &[u8], host: &str, state: &str) -> Result<SecretText, ErrorCode> {
    let request = std::str::from_utf8(request).map_err(|_| ErrorCode::AuthRequired)?;
    let (head, end) = request
        .split_once("\r\n\r\n")
        .ok_or(ErrorCode::AuthRequired)?;
    if !end.is_empty() {
        return Err(ErrorCode::AuthRequired);
    }
    let mut lines = head.split("\r\n");
    let line = lines.next().ok_or(ErrorCode::AuthRequired)?;
    let mut fields = line.split(' ');
    if fields.next() != Some("GET") {
        return Err(ErrorCode::AuthRequired);
    }
    let target = fields.next().ok_or(ErrorCode::AuthRequired)?;
    if fields.next() != Some("HTTP/1.1") || fields.next().is_some() {
        return Err(ErrorCode::AuthRequired);
    }
    let mut hosts = 0;
    for line in lines {
        let (k, v) = line.split_once(':').ok_or(ErrorCode::AuthRequired)?;
        if k.eq_ignore_ascii_case("host") {
            hosts += 1;
            if v.trim() != host {
                return Err(ErrorCode::AuthRequired);
            }
        }
        if k.eq_ignore_ascii_case("content-length") && v.trim() != "0"
            || k.eq_ignore_ascii_case("transfer-encoding")
        {
            return Err(ErrorCode::AuthRequired);
        }
    }
    if hosts != 1 {
        return Err(ErrorCode::AuthRequired);
    }
    let (path, query) = target.split_once('?').ok_or(ErrorCode::AuthRequired)?;
    if path != "/google/oauth/callback" || query.contains('#') {
        return Err(ErrorCode::AuthRequired);
    }
    let mut query_json = SecretJson(Value::Object(Default::default()));
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').ok_or(ErrorCode::AuthRequired)?;
        let k = decode(k)?;
        let v = decode(v)?;
        if query_json.0.get(&k).is_some() {
            let mut bytes = v.into_bytes();
            wipe(&mut bytes);
            return Err(ErrorCode::AuthRequired);
        }
        query_json
            .0
            .as_object_mut()
            .unwrap()
            .insert(k, Value::String(v));
    }
    if query_json.0.get("state").and_then(Value::as_str) != Some(state)
        || query_json.0.get("error").is_some()
    {
        return Err(ErrorCode::AuthRequired);
    }
    SecretText::from_string(query_json.take_text("code")?)
}
#[cfg(test)]
pub(super) fn complete_code(
    transport: &impl http::Transport,
    client_id: &str,
    redirect: &str,
    verifier: &SecretText,
    code: SecretText,
    deadline: Instant,
) -> Result<PendingGoogleCredential, ErrorCode> {
    complete_code_traced(
        transport,
        (client_id, None),
        redirect,
        verifier,
        code,
        deadline,
        &diagnostics::Trace::new(),
    )
}

#[cfg(test)]
pub(super) fn complete_code_traced(
    transport: &impl http::Transport,
    client: (&str, Option<GoogleClientSecret>),
    redirect: &str,
    verifier: &SecretText,
    code: SecretText,
    deadline: Instant,
    trace: &diagnostics::Trace,
) -> Result<PendingGoogleCredential, ErrorCode> {
    complete_code_traced_for_mode(
        transport,
        client,
        redirect,
        verifier,
        code,
        deadline,
        trace,
        GoogleAccessMode::ReadOnly,
    )
}
#[cfg(any(windows, test, target_os = "linux", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn complete_code_traced_for_mode(
    transport: &impl http::Transport,
    client: (&str, Option<GoogleClientSecret>),
    redirect: &str,
    verifier: &SecretText,
    code: SecretText,
    deadline: Instant,
    trace: &diagnostics::Trace,
    access_mode: GoogleAccessMode,
) -> Result<PendingGoogleCredential, ErrorCode> {
    let (client_id, client_secret) = client;
    let cancel = AtomicBool::new(false);
    let mut fields = vec![
        ("client_id", client_id),
        ("code", code.text()),
        ("code_verifier", verifier.text()),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect),
    ];
    if let Some(secret) = &client_secret {
        fields.push(("client_secret", secret.value_for(client_id)?));
    }
    let body = form(&fields);
    trace.start(diagnostics::Stage::TokenExchange);
    let response = transport.request(http::Request {
        host: http::Host::Token,
        path: "/token",
        post: Some(body.as_slice()),
        bearer: None,
        limit: 65536,
        deadline,
        cancel: &cancel,
    });
    trace.result(diagnostics::Stage::TokenExchange, &response);
    let response = response?;
    trace.response(&response);
    let tokens = parse_tokens(response, None);
    trace.result(diagnostics::Stage::TokenResponse, &tokens);
    let mut tokens = tokens?;
    access_mode.validate_granted(&tokens.scopes)?;
    tokens.scopes = access_mode
        .required_scopes()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let refresh = tokens.refresh.ok_or(ErrorCode::AuthRequired);
    trace.result(diagnostics::Stage::RefreshCredential, &refresh);
    let refresh = refresh?;
    trace.start(diagnostics::Stage::Identity);
    let identity = identity(
        transport,
        &tokens.access,
        tokens.scopes,
        access_mode,
        deadline,
        &cancel,
    );
    trace.result(diagnostics::Stage::Identity, &identity);
    let identity = identity?;
    Ok(PendingGoogleCredential {
        identity,
        client_id: client_id.into(),
        refresh,
        client_secret,
    })
}
