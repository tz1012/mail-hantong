use super::*;
use http::Transport;
use std::sync::Mutex;
struct Fake {
    responses: Mutex<std::collections::VecDeque<http::Response>>,
    calls: Mutex<Vec<(http::Host, String)>>,
}
impl Fake {
    fn new(values: Vec<Value>) -> Self {
        Self {
            responses: Mutex::new(
                values
                    .into_iter()
                    .map(|v| http::Response {
                        status: 200,
                        body: SensitiveBuffer(serde_json::to_vec(&v).unwrap()),
                    })
                    .collect(),
            ),
            calls: Mutex::new(Vec::new()),
        }
    }
}
impl Transport for Fake {
    fn request(&self, r: http::Request<'_>) -> Result<http::Response, ErrorCode> {
        http::check(r.deadline, r.cancel)?;
        self.calls.lock().unwrap().push((r.host, r.path.into()));
        if let Some(post) = r.post {
            let post = std::str::from_utf8(post).unwrap();
            assert!(!post.contains("client_secret"));
            assert!(post.contains("code_verifier=") || post.contains("grant_type=refresh_token"));
        }
        let response = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or(ErrorCode::Offline)?;
        if response.body.0.len() > r.limit {
            return Err(ErrorCode::QuotaExceeded);
        }
        Ok(response)
    }
}
fn tokens() -> Value {
    serde_json::json!({"token_type":"Bearer","access_token":"fixture-access","refresh_token":"fixture-refresh","scope":format!("openid https://www.googleapis.com/auth/userinfo.email {}",GOOGLE_READ_SCOPE)})
}
#[test]
fn acc02_pkce_rfc7636_vector_and_account_hash_stability() {
    assert_eq!(
        crypto::base64url(&crypto::sha256(b"dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk").unwrap()),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    assert_eq!(
        hantong_contracts::wire_hex::encode(&crypto::sha256(b"abc").unwrap()),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_ne!(
        account_id("subject-a").unwrap(),
        account_id("subject-b").unwrap()
    );
}
#[test]
fn acc02_callback_rejects_duplicate_fields_wrong_host_state_and_injection() {
    let valid=b"GET /google/oauth/callback?state=state&code=fixture-code HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n";
    assert_eq!(
        oauth::callback(valid, "127.0.0.1:1234", "state")
            .unwrap()
            .text(),
        "fixture-code"
    );
    assert!(oauth::callback(valid, "localhost:1234", "state").is_err());
    assert!(oauth::callback(valid, "127.0.0.1:1234", "other").is_err());
    for query in [
        "state=state&code=a&code=b",
        "state=state&code=a&%63ode=b",
        "state=state&state=state&code=a",
        "state=state&error=denied&code=a",
        "state=state&code=%0D%0A",
        "state=state&code=%FF",
    ] {
        let request =
            format!("GET /google/oauth/callback?{query} HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n");
        assert!(
            oauth::callback(request.as_bytes(), "127.0.0.1:1234", "state").is_err(),
            "{query}"
        );
    }
}
#[test]
fn acc01_exchange_requires_refresh_read_scope_and_verified_stable_subject() {
    let identity =
        serde_json::json!({"sub":"subject-a","email":"fixture@example.test","email_verified":true});
    let fake = Fake::new(vec![tokens(), identity.clone()]);
    let pending = oauth::complete_code(
        &fake,
        "fixture.apps.googleusercontent.com",
        "http://127.0.0.1:1234/google/oauth/callback",
        &SecretText::from_string("verifier".into()).unwrap(),
        SecretText::from_string("code".into()).unwrap(),
        Instant::now() + Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(
        pending.identity().account_id,
        account_id("subject-a").unwrap()
    );
    assert_eq!(pending.refresh.text(), "fixture-refresh");
    assert_eq!(pending.binding(1).unwrap().credential_version, 1);
    assert!(pending.binding(0).is_err());
    assert!(!format!("{pending:?}").contains("fixture"));
    for (index, mut token) in [tokens(), tokens(), tokens()].into_iter().enumerate() {
        match index {
            0 => {
                token.as_object_mut().unwrap().remove("refresh_token");
            }
            1 => {
                token["scope"] = Value::String("openid email".into());
            }
            _ => {
                token["token_type"] = Value::String("MAC".into());
            }
        }
        let fake = Fake::new(vec![token, identity.clone()]);
        assert!(
            oauth::complete_code(
                &fake,
                "fixture.apps.googleusercontent.com",
                "r",
                &SecretText::from_string("verifier".into()).unwrap(),
                SecretText::from_string("code".into()).unwrap(),
                Instant::now() + Duration::from_secs(1)
            )
            .is_err()
        );
    }
    let mut unverified = identity;
    unverified["email_verified"] = Value::Bool(false);
    let fake = Fake::new(vec![tokens(), unverified]);
    assert!(
        oauth::complete_code(
            &fake,
            "fixture.apps.googleusercontent.com",
            "r",
            &SecretText::from_string("verifier".into()).unwrap(),
            SecretText::from_string("code".into()).unwrap(),
            Instant::now() + Duration::from_secs(1)
        )
        .is_err()
    );
    assert!(normalize_scopes("openid email").is_err());
}
#[test]
fn sync02_gmail_fetch_uses_fixed_me_read_paths_and_encodes_page_data() {
    let fake = Fake::new(vec![
        serde_json::json!({"labels":[{"id":"INBOX","name":"Inbox","type":"system"}]}),
        serde_json::json!({"messages":[{"id":"abc","threadId":"def"}],"nextPageToken":"opaque-next","resultSizeEstimate":1}),
        serde_json::json!({"id":"abc","threadId":"def","internalDate":"123","labelIds":["INBOX"],"sizeEstimate":100,"snippet":"preview","payload":{"mimeType":"text/plain","headers":[{"name":"Subject","value":"Subject"},{"name":"From","value":"Sender"}],"body":{"size":5,"data":"aGVsbG8"}}}),
    ]);
    let binding = GoogleAccountBinding {
        account_id: account_id("s").unwrap(),
        subject: "s".into(),
        client_id: "fixture.apps.googleusercontent.com".into(),
        credential_version: 1,
    };
    let request = GoogleSyncRequest {
        account_id: binding.account_id.clone(),
        label_id: Some("INBOX".into()),
        page_token: Some("opaque&maxResults=999".into()),
        limit: 2,
        expected_version: 0,
        operation_id: "op".into(),
        idempotency_key: "idem".into(),
    };
    let page = gmail::fetch(
        &fake,
        &binding,
        &request,
        &SecretText::from_string("access".into()).unwrap(),
        Instant::now() + Duration::from_secs(1),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(page.messages[0].body_text.as_deref(), Some("hello"));
    assert_eq!(
        fake.calls.lock().unwrap()[1].1,
        "/gmail/v1/users/me/messages?maxResults=2&labelIds=INBOX&pageToken=opaque%26maxResults%3D999"
    );
}

#[test]
fn acc06_refresh_rechecks_subject_and_isolates_accounts() {
    let binding = GoogleAccountBinding {
        account_id: account_id("subject-a").unwrap(),
        subject: "subject-a".into(),
        client_id: "fixture.apps.googleusercontent.com".into(),
        credential_version: 1,
    };
    let scopes = GOOGLE_SCOPES
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let refresh = SecretText::from_string("refresh-fixture".into()).unwrap();
    let mut refreshed = tokens();
    refreshed.as_object_mut().unwrap().remove("scope");
    refreshed.as_object_mut().unwrap().remove("refresh_token");
    for (subject, expected) in [("subject-b", false), ("subject-a", true)] {
        let fake = Fake::new(vec![
            refreshed.clone(),
            serde_json::json!({"sub":subject,"email":"alias@example.test","email_verified":true}),
        ]);
        assert_eq!(
            refresh_verified(
                &fake,
                &binding,
                &refresh,
                &scopes,
                Instant::now() + Duration::from_secs(1),
                &AtomicBool::new(false)
            )
            .is_ok(),
            expected
        );
        assert_eq!(fake.calls.lock().unwrap().len(), 2);
    }
}
#[test]
fn sync02_unsupported_large_and_attachment_bodies_keep_metadata() {
    for payload in [
        serde_json::json!({"mimeType":"application/pdf","body":{"attachmentId":"a"}}),
        serde_json::json!({"mimeType":"text/plain","body":{"size":65537,"data":""}}),
        serde_json::json!({"mimeType":"text/plain","filename":"attachment.txt","body":{"data":"aGVsbG8"}}),
    ] {
        let value =
            serde_json::json!({"id":"a","threadId":"b","internalDate":"0","payload":payload});
        let m = gmail::parse_message(&value, false).unwrap();
        assert!(m.body_text.is_none());
        assert!(m.body_error.is_some());
        m.validate().unwrap();
    }
    let value = serde_json::json!({"id":"a","threadId":"b","internalDate":"0","payload":{"mimeType":"text/html","body":{"data":crypto::base64url(b"hello<script>alert(1)</script><img src='https://tracking.invalid/x'>")}}});
    let m = gmail::parse_message(&value, false).unwrap();
    assert_eq!(m.body_text.as_deref(), Some("hello"));
}

#[test]
fn sync02_large_detail_response_falls_back_to_metadata_without_losing_page() {
    let fake = Fake::new(vec![
        serde_json::json!({"labels":[]}),
        serde_json::json!({"messages":[{"id":"a","threadId":"b"}]}),
        serde_json::json!({"oversize":"x".repeat(2*1024*1024)}),
        serde_json::json!({"id":"a","threadId":"b","internalDate":"0","sizeEstimate":99999999,"payload":{"headers":[{"name":"Subject","value":"metadata retained"}]}}),
    ]);
    let binding = GoogleAccountBinding {
        account_id: account_id("subject").unwrap(),
        subject: "subject".into(),
        client_id: "fixture.apps.googleusercontent.com".into(),
        credential_version: 1,
    };
    let request = GoogleSyncRequest {
        account_id: binding.account_id.clone(),
        label_id: None,
        page_token: None,
        limit: 1,
        expected_version: 0,
        operation_id: "op".into(),
        idempotency_key: "idem".into(),
    };
    let page = gmail::fetch(
        &fake,
        &binding,
        &request,
        &SecretText::from_string("access".into()).unwrap(),
        Instant::now() + Duration::from_secs(2),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(page.messages[0].subject, "metadata retained");
    assert_eq!(
        page.messages[0].body_error.as_deref(),
        Some("QUOTA_EXCEEDED")
    );
    assert!(page.messages[0].body_text.is_none());
    assert!(fake.calls.lock().unwrap()[3].1.contains("format=metadata"));
}
#[test]
fn ext03_transport_rejects_cross_host_mutations_and_expired_or_cancelled_requests() {
    let native = http::NativeTransport;
    let cancel = AtomicBool::new(false);
    let deadline = Instant::now() + Duration::from_secs(1);
    for (host, path, post, bearer) in [
        (
            http::Host::Gmail,
            "/gmail/v1/users/me/messages/send",
            Some(b"x".as_slice()),
            Some("access"),
        ),
        (http::Host::Token, "/other", Some(b"x".as_slice()), None),
        (http::Host::Identity, "/v1/userinfo", None, None),
    ] {
        assert!(matches!(
            native.request(http::Request {
                host,
                path,
                post,
                bearer,
                limit: 1,
                deadline,
                cancel: &cancel
            }),
            Err(ErrorCode::InvalidInput)
        ));
    }
    cancel.store(true, Ordering::Relaxed);
    assert_eq!(http::check(deadline, &cancel), Err(ErrorCode::Offline));
}
#[cfg(windows)]
#[tokio::test]
async fn acc02_loopback_completes_once_and_closes_listener_without_provider_network() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let authorization = begin_google_authorization("fixture.apps.googleusercontent.com", false)
        .await
        .unwrap();
    assert!(
        authorization
            .authorization_url()
            .contains("code_challenge_method=S256")
    );
    assert!(
        authorization
            .authorization_url()
            .contains("prompt=select_account")
    );
    assert!(!authorization.authorization_url().contains("client_secret"));
    // A provider error exercises callback consumption without contacting Google.
    let url = authorization.authorization_url().to_string();
    let query = url.split('?').nth(1).unwrap();
    let redirect = query
        .split('&')
        .find(|p| p.starts_with("redirect_uri="))
        .unwrap()
        .trim_start_matches("redirect_uri=");
    let redirect = redirect.replace("%3A", ":").replace("%2F", "/");
    let address = redirect
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap()
        .to_string();
    let complete = tokio::spawn(authorization.complete());
    let mut stream = tokio::net::TcpStream::connect(&address).await.unwrap();
    stream.write_all(format!("GET /google/oauth/callback?error=denied&state=stale HTTP/1.1\r\nHost: {address}\r\n\r\n").as_bytes()).await.unwrap();
    let mut out = Vec::new();
    stream.read_to_end(&mut out).await.unwrap();
    assert!(matches!(
        complete.await.unwrap(),
        Err(ErrorCode::AuthRequired)
    ));
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    assert!(!String::from_utf8_lossy(&out).contains("denied"));
}
#[cfg(windows)]
#[test]
fn sec01_google_generation_case_safe_slots_and_dpapi_binding() {
    let root =
        std::env::temp_dir().join(format!("hantong-google-generation-{}", std::process::id()));
    let store = SecretStore::open(&root).unwrap();
    let identity = GoogleIdentity {
        account_id: account_id("subject").unwrap(),
        subject: "subject".into(),
        email: "fixture@example.test".into(),
        granted_scopes: GOOGLE_SCOPES.iter().map(|s| s.to_string()).collect(),
    };
    let pending = PendingGoogleCredential {
        identity,
        client_id: "fixture.apps.googleusercontent.com".into(),
        refresh: SecretText::from_string("refresh-fixture".into()).unwrap(),
    };
    let binding = pending.binding(1).unwrap();
    pending.store(&store, &binding).unwrap();
    let a = store.handle(&binding.account_id, crate::SecretKind::GoogleOAuth(1));
    let b = store.handle(&binding.account_id, crate::SecretKind::GoogleOAuth(2));
    assert_ne!(a.path, b.path);
    assert!(
        a.path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("v3-")
    );
    std::fs::copy(&a.path, &b.path).unwrap();
    assert!(crate::windows_impl::read_google_credential(&b.path, &binding.account_id, 2).is_err());
    let raw = std::fs::read(a.path).unwrap();
    assert!(
        !raw.windows(b"refresh-fixture".len())
            .any(|w| w == b"refresh-fixture")
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
