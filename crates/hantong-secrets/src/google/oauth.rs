use super::*;
#[cfg(not(any(windows, test)))]
use crate::SensitiveBuffer;
#[cfg(windows)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub struct GoogleAuthorization {
    url: SensitiveBuffer,
    #[cfg(windows)]
    listener: tokio::net::TcpListener,
    #[cfg(windows)]
    state: SecretText,
    #[cfg(windows)]
    verifier: SecretText,
    #[cfg(windows)]
    client_id: String,
    #[cfg(windows)]
    redirect: String,
    #[cfg(windows)]
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
    validate_google_client_id(client_id)?;
    #[cfg(not(windows))]
    {
        let _ = force_consent;
        Err(ErrorCode::CapabilityUnsupported)
    }
    #[cfg(windows)]
    {
        let mut random = SensitiveBuffer(vec![0; 64]);
        crate::windows_impl::random_bytes(&mut random.0).map_err(|_| ErrorCode::InternalError)?;
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
                    ("scope", &GOOGLE_SCOPES.join(" ")),
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
            redirect,
            deadline: Instant::now() + Duration::from_secs(300),
        })
    }
}
impl GoogleAuthorization {
    pub fn authorization_url(&self) -> &str {
        std::str::from_utf8(self.url.as_slice()).unwrap_or("")
    }
    pub async fn complete(self) -> Result<PendingGoogleCredential, ErrorCode> {
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ErrorCode::AuthRequired);
            }
            let code = tokio::time::timeout(remaining, async {
                let (mut stream, peer) = self.listener.accept().await.map_err(|_| ErrorCode::AuthRequired)?;
                if !peer.ip().is_loopback() { return Err(ErrorCode::AuthRequired); }
                let mut request = SensitiveBuffer(vec![0;8193]);
                let mut used = 0;
                loop {
                    let n = stream.read(&mut request.0[used..]).await.map_err(|_| ErrorCode::AuthRequired)?;
                    if n == 0 { return Err(ErrorCode::AuthRequired); }
                    used += n;
                    if used > 8192 { return Err(ErrorCode::AuthRequired); }
                    if request.0[..used].windows(4).any(|w| w == b"\r\n\r\n") { break; }
                }
                let host = format!("127.0.0.1:{}", self.listener.local_addr().map_err(|_| ErrorCode::AuthRequired)?.port());
                let result = callback(&request.0[..used], &host, self.state.text());
                let status = if result.is_ok() { "200 OK" } else { "400 Bad Request" };
                let reply = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n");
                let _ = stream.write_all(reply.as_bytes()).await;
                let _ = stream.shutdown().await;
                result
            }).await.map_err(|_| ErrorCode::AuthRequired)??;
            // Closing the only listener before exchange makes replay impossible.
            drop(self.listener);
            let deadline = self.deadline;
            tokio::task::spawn_blocking(move || {
                complete_code(
                    &http::NativeTransport,
                    &self.client_id,
                    &self.redirect,
                    &self.verifier,
                    code,
                    deadline,
                )
            })
            .await
            .map_err(|_| ErrorCode::InternalError)?
        }
    }
}
#[cfg(any(windows, test))]
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
#[cfg(any(windows, test))]
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
#[cfg(any(windows, test))]
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
#[cfg(any(windows, test))]
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
#[cfg(any(windows, test))]
pub(super) fn complete_code(
    transport: &impl http::Transport,
    client_id: &str,
    redirect: &str,
    verifier: &SecretText,
    code: SecretText,
    deadline: Instant,
) -> Result<PendingGoogleCredential, ErrorCode> {
    let cancel = AtomicBool::new(false);
    let body = form(&[
        ("client_id", client_id),
        ("code", code.text()),
        ("code_verifier", verifier.text()),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect),
    ]);
    let tokens = parse_tokens(
        transport.request(http::Request {
            host: http::Host::Token,
            path: "/token",
            post: Some(body.as_slice()),
            bearer: None,
            limit: 65536,
            deadline,
            cancel: &cancel,
        })?,
        None,
    )?;
    let refresh = tokens.refresh.ok_or(ErrorCode::AuthRequired)?;
    let identity = identity(transport, &tokens.access, tokens.scopes, deadline, &cancel)?;
    Ok(PendingGoogleCredential {
        identity,
        client_id: client_id.into(),
        refresh,
    })
}
