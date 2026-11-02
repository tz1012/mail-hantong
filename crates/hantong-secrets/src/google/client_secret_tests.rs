use super::*;

#[test]
fn acc02_requested_access_mode_is_bound_and_extra_provider_scopes_are_not_published() {
    for mode in [
        GoogleAccessMode::ReadOnly,
        GoogleAccessMode::ReadSend,
        GoogleAccessMode::Modify,
    ] {
        let mut token = super::tests::tokens();
        token["scope"] = format!(
            "{} https://mail.google.com/",
            mode.required_scopes().join(" ")
        )
        .into();
        let fake = super::tests::Fake::new(vec![
            token,
            serde_json::json!({"sub":"fixture-subject","email":"fixture@example.test","email_verified":true}),
        ]);
        let pending = oauth::complete_code_traced_for_mode(
            &fake,
            (CLIENT, None),
            "http://127.0.0.1:1234/google/oauth/callback",
            &SecretText::from_string("verifier-fixture".into()).unwrap(),
            SecretText::from_string("code-fixture".into()).unwrap(),
            Instant::now() + Duration::from_secs(5),
            &diagnostics::Trace::new(),
            mode,
        )
        .unwrap();
        assert_eq!(pending.identity.access_mode, mode);
        assert_eq!(pending.binding(1).unwrap().access_mode, mode);
        assert_eq!(pending.identity.granted_scopes, mode.required_scopes());
        assert!(
            !pending
                .identity
                .granted_scopes
                .iter()
                .any(|s| s == "https://mail.google.com/")
        );
    }
    let fake = super::tests::Fake::new(vec![super::tests::tokens()]);
    let outcome = oauth::complete_code_traced_for_mode(
        &fake,
        (CLIENT, None),
        "http://127.0.0.1:1234/google/oauth/callback",
        &SecretText::from_string("verifier-fixture".into()).unwrap(),
        SecretText::from_string("code-fixture".into()).unwrap(),
        Instant::now() + Duration::from_secs(5),
        &diagnostics::Trace::new(),
        GoogleAccessMode::Modify,
    );
    assert_eq!(outcome.err(), Some(ErrorCode::ScopeDenied));
    assert_eq!(fake.calls.lock().unwrap().len(), 1);
}

#[test]
fn sec02_legacy_bundle_cannot_gain_write_mode_and_new_mode_is_explicit() {
    for format in [3, 4] {
        let mut legacy = SecretJson(
            serde_json::json!({"format":format,"scopes":["openid","email",super::super::google::GOOGLE_READ_SCOPE,"https://mail.google.com/"]}),
        );
        assert_eq!(
            client_secret::bundle_mode(&legacy),
            Ok(GoogleAccessMode::ReadOnly)
        );
        legacy.0["access_mode"] = "modify".into();
        assert_eq!(
            client_secret::bundle_mode(&legacy),
            Err(ErrorCode::AuthRequired)
        );
    }
    let binding = GoogleAccountBinding {
        access_mode: GoogleAccessMode::Modify,
        account_id: account_id("fixture-subject").unwrap(),
        subject: "fixture-subject".into(),
        client_id: CLIENT.into(),
        credential_version: 1,
    };
    let refresh = SecretText::from_string("synthetic-refresh".into()).unwrap();
    let scopes = binding
        .access_mode
        .required_scopes()
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let mut bundle = client_secret::bundle(&binding, &refresh, &scopes, Some(&secret())).unwrap();
    assert_eq!(bundle.0["format"], 5);
    assert_eq!(
        client_secret::bundle_mode(&bundle),
        Ok(GoogleAccessMode::Modify)
    );
    assert!(
        client_secret::from_bundle(&mut bundle, CLIENT)
            .unwrap()
            .unwrap()
            .matches(CLIENT)
    );
    bundle.0["access_mode"] = "all".into();
    assert_eq!(
        client_secret::bundle_mode(&bundle),
        Err(ErrorCode::AuthRequired)
    );
    assert!(client_secret::bundle(&binding, &refresh, &[], None).is_err());
}
const CLIENT: &str = "fixture.apps.googleusercontent.com";
const SECRET: &str = "synthetic-client-secret&=+%";
fn secret() -> GoogleClientSecret {
    GoogleClientSecret::from_capture(CLIENT, SensitiveBuffer(SECRET.as_bytes().to_vec())).unwrap()
}
struct CheckedTransport {
    inner: tests::Fake,
    refresh: bool,
}
impl http::Transport for CheckedTransport {
    fn request(&self, request: http::Request<'_>) -> Result<http::Response, ErrorCode> {
        if request.host == http::Host::Token {
            assert_eq!(request.path, "/token");
            assert!(request.bearer.is_none());
            let body = std::str::from_utf8(request.post.unwrap()).unwrap();
            assert!(body.contains("client_secret=synthetic-client-secret%26%3D%2B%25"));
            assert_eq!(body.matches("client_secret=").count(), 1);
            assert!(body.contains(if self.refresh {
                "grant_type=refresh_token"
            } else {
                "code_verifier=verifier-fixture"
            }));
        } else {
            assert!(request.post.is_none());
            assert!(!request.path.contains(SECRET));
        }
        http::check(request.deadline, request.cancel)?;
        self.inner
            .calls
            .lock()
            .unwrap()
            .push((request.host, request.path.into()));
        let response = self
            .inner
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or(ErrorCode::Offline)?;
        if response.body.0.len() > request.limit {
            return Err(ErrorCode::QuotaExceeded);
        }
        Ok(response)
    }
}
fn fake(refresh: bool) -> CheckedTransport {
    CheckedTransport {
        inner: tests::Fake::new(vec![
            tests::tokens(),
            serde_json::json!({"sub":"fixture-subject","email":"fixture@example.test","email_verified":true}),
        ]),
        refresh,
    }
}
#[test]
fn acc02_sec08_client_secret_exchange_stays_bound_encoded_and_out_of_diagnostics() {
    let transport = fake(false);
    let (pending, logs) = diagnostics::capture(|| {
        oauth::complete_code_traced(
            &transport,
            (CLIENT, Some(secret())),
            "http://127.0.0.1:1234/google/oauth/callback",
            &SecretText::from_string("verifier-fixture".into()).unwrap(),
            SecretText::from_string("code-fixture".into()).unwrap(),
            Instant::now() + Duration::from_secs(5),
            &diagnostics::Trace::new(),
        )
    });
    let pending = pending.unwrap();
    assert!(pending.client_secret.as_ref().unwrap().matches(CLIENT));
    assert!(!format!("{pending:?} {:?}", secret()).contains(SECRET));
    assert!(!logs.join("\n").contains("synthetic-client-secret"));
    let transport = fake(false);
    assert!(matches!(
        oauth::complete_code_traced(
            &transport,
            ("other.apps.googleusercontent.com", Some(secret())),
            "http://127.0.0.1:1234/google/oauth/callback",
            &SecretText::from_string("verifier-fixture".into()).unwrap(),
            SecretText::from_string("code-fixture".into()).unwrap(),
            Instant::now() + Duration::from_secs(5),
            &diagnostics::Trace::new()
        ),
        Err(ErrorCode::ScopeDenied)
    ));
    assert!(transport.inner.calls.lock().unwrap().is_empty());
}
#[test]
fn acc03_sec01_client_secret_refresh_rotation_and_legacy_bundle_are_compatible() {
    let binding = GoogleAccountBinding {
        access_mode: hantong_contracts::google_write::GoogleAccessMode::ReadOnly,
        account_id: account_id("fixture-subject").unwrap(),
        subject: "fixture-subject".into(),
        client_id: CLIENT.into(),
        credential_version: 1,
    };
    let refresh = SecretText::from_string("synthetic-refresh".into()).unwrap();
    let scopes = GOOGLE_SCOPES
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let mut bundle = client_secret::bundle(&binding, &refresh, &scopes, Some(&secret())).unwrap();
    let held = client_secret::from_bundle(&mut bundle, CLIENT)
        .unwrap()
        .unwrap();
    let result = refresh_verified(
        &fake(true),
        &binding,
        &refresh,
        &scopes,
        Some(&held),
        Instant::now() + Duration::from_secs(5),
        &AtomicBool::new(false),
    )
    .unwrap();
    let mut rotated = client_secret::bundle(
        &binding,
        result.refresh.as_ref().unwrap(),
        &result.scopes,
        Some(&held),
    )
    .unwrap();
    assert_eq!(
        client_secret::from_bundle(&mut rotated, CLIENT)
            .unwrap()
            .unwrap()
            .value_for(CLIENT)
            .unwrap(),
        SECRET
    );
    let mut old = SecretJson(serde_json::json!({"format":3}));
    assert!(
        client_secret::from_bundle(&mut old, CLIENT)
            .unwrap()
            .is_none()
    );
    old.0["client_secret"] = "unexpected".into();
    assert!(client_secret::from_bundle(&mut old, CLIENT).is_err());
    for invalid in [
        serde_json::json!(null),
        serde_json::json!(23),
        serde_json::json!(""),
    ] {
        let mut malformed = SecretJson(serde_json::json!({"format":4,"client_secret":invalid}));
        assert!(client_secret::from_bundle(&mut malformed, CLIENT).is_err());
    }
}
#[cfg(windows)]
#[test]
fn sec01_google_client_secret_dpapi_storage_is_account_and_generation_bound() {
    let root = std::env::temp_dir().join(format!("hantong-client-secret-{}", std::process::id()));
    let store = SecretStore::open(&root).unwrap();
    let pending = PendingGoogleCredential {
        identity: GoogleIdentity {
            access_mode: hantong_contracts::google_write::GoogleAccessMode::ReadOnly,
            account_id: account_id("fixture-subject").unwrap(),
            subject: "fixture-subject".into(),
            email: "fixture@example.test".into(),
            granted_scopes: GOOGLE_SCOPES.iter().map(|s| s.to_string()).collect(),
        },
        client_id: CLIENT.into(),
        refresh: SecretText::from_string("synthetic-refresh".into()).unwrap(),
        client_secret: Some(secret()),
    };
    let binding = pending.binding(1).unwrap();
    pending.store(&store, &binding).unwrap();
    let handle = store.handle(&binding.account_id, crate::SecretKind::GoogleOAuth(1));
    let ciphertext = std::fs::read(&handle.path).unwrap();
    assert!(
        !ciphertext
            .windows(SECRET.len())
            .any(|w| w == SECRET.as_bytes())
    );
    let plain =
        crate::windows_impl::read_google_credential(&handle.path, &binding.account_id, 1).unwrap();
    let mut bundle = SecretJson::parse(plain.as_slice()).unwrap();
    assert_eq!(
        client_secret::from_bundle(&mut bundle, CLIENT)
            .unwrap()
            .unwrap()
            .value_for(CLIENT)
            .unwrap(),
        SECRET
    );
    assert!(
        crate::windows_impl::read_google_credential(&handle.path, &binding.account_id, 2).is_err()
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn sec01_acc02_common_app_secret_reuse_keeps_two_account_tokens_and_bindings_separate() {
    let root = std::env::temp_dir().join(format!(
        "hantong-reuse-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = SecretStore::open(&root).unwrap();
    let mut bindings = vec![];
    for (subject, refresh) in [
        ("synthetic-one", "refresh-one"),
        ("synthetic-two", "refresh-two"),
    ] {
        let pending = PendingGoogleCredential {
            identity: GoogleIdentity {
                access_mode: hantong_contracts::google_write::GoogleAccessMode::ReadOnly,
                account_id: account_id(subject).unwrap(),
                subject: subject.into(),
                email: "synthetic@example.test".into(),
                granted_scopes: GOOGLE_SCOPES.iter().map(|s| s.to_string()).collect(),
            },
            client_id: CLIENT.into(),
            refresh: SecretText::from_string(refresh.into()).unwrap(),
            client_secret: Some(secret()),
        };
        let binding = pending.binding(1).unwrap();
        pending.store(&store, &binding).unwrap();
        bindings.push(binding);
    }
    for (binding, refresh) in bindings.iter().zip(["refresh-one", "refresh-two"]) {
        let handle = store.handle(&binding.account_id, crate::SecretKind::GoogleOAuth(1));
        let before = std::fs::read(&handle.path).unwrap();
        let held = store
            .google_client_credential(binding, CLIENT)
            .unwrap()
            .unwrap();
        assert!(held.matches(CLIENT));
        assert!(held.value_for(CLIENT).unwrap() == SECRET);
        assert!(!format!("{held:?}").contains(SECRET));
        let bundle = store.read_bound_google_bundle(binding).unwrap();
        assert!(bundle.0["refresh_token"].as_str() == Some(refresh));
        assert_eq!(std::fs::read(&handle.path).unwrap(), before);
        assert!(matches!(
            store.google_client_credential(binding, "other.apps.googleusercontent.com"),
            Err(ErrorCode::ScopeDenied)
        ));
        let mut stale = binding.clone();
        stale.credential_version += 1;
        assert!(store.google_client_credential(&stale, CLIENT).is_err());
        let mut crossed = binding.clone();
        crossed.subject = "different-subject".into();
        assert!(store.google_client_credential(&crossed, CLIENT).is_err());
    }
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn sec02_protected_bundle_rejects_mode_forgery_and_keeps_accounts_isolated() {
    let root = std::env::temp_dir().join(format!(
        "hantong-access-modes-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = SecretStore::open(&root).unwrap();
    for (subject, mode) in [
        ("synthetic-readonly", GoogleAccessMode::ReadOnly),
        ("synthetic-modify", GoogleAccessMode::Modify),
    ] {
        let pending = PendingGoogleCredential {
            identity: GoogleIdentity {
                access_mode: mode,
                account_id: account_id(subject).unwrap(),
                subject: subject.into(),
                email: "synthetic@example.invalid".into(),
                granted_scopes: mode
                    .required_scopes()
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            },
            client_id: CLIENT.into(),
            refresh: SecretText::from_string("synthetic-refresh".into()).unwrap(),
            client_secret: Some(secret()),
        };
        let binding = pending.binding(1).unwrap();
        pending.store(&store, &binding).unwrap();
        let handle = store.handle(&binding.account_id, crate::SecretKind::GoogleOAuth(1));
        let before = std::fs::read(&handle.path).unwrap();
        assert!(
            store
                .google_client_credential(&binding, CLIENT)
                .unwrap()
                .is_some()
        );
        let mut forged = binding.clone();
        forged.access_mode = if mode == GoogleAccessMode::ReadOnly {
            GoogleAccessMode::Modify
        } else {
            GoogleAccessMode::ReadOnly
        };
        assert_eq!(
            store.read_bound_google_bundle(&forged).err(),
            Some(ErrorCode::AuthRequired)
        );
        forged = binding.clone();
        forged.subject = "different-subject".into();
        assert_eq!(
            store.read_bound_google_bundle(&forged).err(),
            Some(ErrorCode::AuthRequired)
        );
        assert_eq!(before, std::fs::read(&handle.path).unwrap());
    }
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
