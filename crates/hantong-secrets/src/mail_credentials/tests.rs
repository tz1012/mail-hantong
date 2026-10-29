#![cfg(windows)]

use super::*;
use hantong_domain::mail_connection::{MailConnectionConfig, SmtpEndpointConfig};
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct TempGuard {
    root: PathBuf,
}

impl TempGuard {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "mail-hantong-mail-credentials-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(!root.exists(), "fixture already exists: {root:?}");
        fs::create_dir(&root).unwrap();
        Self { root }
    }

    fn store(&self) -> SecretStore {
        SecretStore::open(&self.root).unwrap()
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("test secret handles released before cleanup");
    }
}

fn config(
    imap_host: &str,
    imap_version: u64,
    smtp_host: Option<&str>,
    smtp_version: u64,
) -> MailConnectionConfig {
    let smtp = smtp_host
        .map(|host| {
            format!(
                r#","smtp":{{"settings_version":{smtp_version},"host":"{host}","port":465,"tls":"implicit_tls","login_name":"out","authentication":"oauth2"}}"#
            )
        })
        .unwrap_or_default();
    let json = format!(
        r#"{{"schema_version":1,"account_id":"acct_1","imap":{{"settings_version":{imap_version},"host":"{imap_host}","port":993,"tls":"implicit_tls","login_name":"in","authentication":"app_password"}}{smtp}}}"#
    );
    MailConnectionConfig::from_json(json.as_bytes()).unwrap()
}

fn smtp(config: &MailConnectionConfig) -> &SmtpEndpointConfig {
    config.smtp().expect("SMTP fixture")
}

fn valid_envelope(
    account: &str,
    host: &str,
    port: u16,
    login: &str,
    version: u64,
    credential: &[u8],
    trailing: bool,
) -> Vec<u8> {
    let mut canonical = b"mail-hantong.mail-credential.v1\0".to_vec();
    let string = |dst: &mut Vec<u8>, value: &[u8]| {
        dst.extend_from_slice(&(value.len() as u32).to_be_bytes());
        dst.extend_from_slice(value);
    };
    string(&mut canonical, account.as_bytes());
    canonical.push(0);
    canonical.extend_from_slice(&version.to_be_bytes());
    string(&mut canonical, host.as_bytes());
    canonical.extend_from_slice(&port.to_be_bytes());
    canonical.push(0);
    string(&mut canonical, login.as_bytes());
    canonical.push(0);
    canonical.extend_from_slice(&(credential.len() as u32).to_be_bytes());
    canonical.extend_from_slice(credential);
    if trailing {
        canonical.push(0xff);
    }
    canonical
}

struct FailingReader;
impl Read for FailingReader {
    fn read(&mut self, _out: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("fixture reader failure"))
    }
}

#[test]
fn sec01_mail_bound_credentials_round_trip_and_reopen() {
    let fixture = TempGuard::new("roundtrip");
    let config = config("imap.example", 1, Some("smtp.example"), 2);
    let store = fixture.store();
    let imap = store
        .store_imap_credential("acct_1", config.imap(), " app password ".as_bytes())
        .unwrap();
    let smtp_handle = store
        .store_smtp_credential("acct_1", smtp(&config), b"oauth-access-token".as_slice())
        .unwrap();
    store
        .verify_imap_credential("acct_1", config.imap())
        .unwrap();
    store
        .verify_smtp_credential("acct_1", smtp(&config))
        .unwrap();
    drop(store);

    let reopened = fixture.store();
    reopened
        .verify_imap_credential("acct_1", config.imap())
        .unwrap();
    reopened
        .verify_smtp_credential("acct_1", smtp(&config))
        .unwrap();
    assert!(reopened.contains(&imap));
    assert!(reopened.contains(&smtp_handle));
}

#[test]
fn sec01_mail_binding_rejects_each_endpoint_field_and_version_changes() {
    let fixture = TempGuard::new("binding");
    let store = fixture.store();
    let base = config("imap.example", 1, Some("smtp.example"), 2);
    store
        .store_imap_credential("acct_1", base.imap(), b"password".as_slice())
        .unwrap();

    for (host, version, port, tls, login, auth) in [
        (
            "other.example",
            1,
            993,
            "implicit_tls",
            "in",
            "app_password",
        ),
        ("imap.example", 2, 993, "implicit_tls", "in", "app_password"),
        ("imap.example", 1, 995, "implicit_tls", "in", "app_password"),
        (
            "imap.example",
            1,
            993,
            "starttls_required",
            "in",
            "app_password",
        ),
        (
            "imap.example",
            1,
            993,
            "implicit_tls",
            "other",
            "app_password",
        ),
        ("imap.example", 1, 993, "implicit_tls", "in", "oauth2"),
    ] {
        let json = format!(
            r#"{{"schema_version":1,"account_id":"acct_1","imap":{{"settings_version":{version},"host":"{host}","port":{port},"tls":"{tls}","login_name":"{login}","authentication":"{auth}"}}}}"#
        );
        let changed = MailConnectionConfig::from_json(json.as_bytes()).unwrap();
        assert_eq!(
            store.verify_imap_credential("acct_1", changed.imap()),
            Err(if auth == "oauth2" {
                SecretError::NotFound
            } else {
                SecretError::CredentialMismatch
            })
        );
    }

    let higher = config("imap.example", 3, Some("smtp.example"), 2);
    assert_eq!(
        store.verify_imap_credential("acct_1", higher.imap()),
        Err(SecretError::CredentialMismatch)
    );
}

#[test]
fn sec01_mail_credentials_are_isolated_by_account_role_and_oauth_slot() {
    let fixture = TempGuard::new("isolation");
    let store = fixture.store();
    let cfg = config("imap.example", 1, Some("smtp.example"), 1);
    store
        .store_imap_credential("acct_1", cfg.imap(), b"imap-password".as_slice())
        .unwrap();
    store
        .store_smtp_credential("acct_1", smtp(&cfg), b"smtp-token".as_slice())
        .unwrap();

    let acct2 = config("imap.example", 1, Some("smtp.example"), 1);
    store
        .store_imap_credential("acct_2", acct2.imap(), b"other-account-password".as_slice())
        .unwrap();
    assert_eq!(store.verify_imap_credential("acct_2", acct2.imap()), Ok(()));
    assert_eq!(store.verify_smtp_credential("acct_1", smtp(&cfg)), Ok(()));
    assert_ne!(store.verify_smtp_credential("acct_2", smtp(&acct2)), Ok(()));

    let oauth_json = r#"{"schema_version":1,"account_id":"acct_1","imap":{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"in","authentication":"oauth2"}}"#;
    let oauth = MailConnectionConfig::from_json(oauth_json.as_bytes()).unwrap();
    assert_eq!(
        store.verify_imap_credential("acct_1", oauth.imap()),
        Err(SecretError::NotFound)
    );

    let raw = store
        .store_from_reader(
            "acct_1",
            SecretKind::Mail(MailCredentialSlot::ImapAppPassword),
            b"legacy-raw".as_slice(),
        )
        .unwrap();
    assert_eq!(
        store.verify_imap_credential("acct_1", cfg.imap()),
        Err(SecretError::CredentialMismatch)
    );
    assert!(store.contains(&raw));
}

#[test]
fn sec01_mail_credential_input_bounds_and_invalid_reader_preserve_ciphertext() {
    let fixture = TempGuard::new("bounds");
    let store = fixture.store();
    let cfg = config("imap.example", 1, None, 1);
    let handle = store
        .store_imap_credential("acct_1", cfg.imap(), b"old-secret".as_slice())
        .unwrap();
    let old_ciphertext = fs::read(&handle.path).unwrap();

    for invalid in [b"".as_slice(), &[0xff], &[0x1f], &vec![b'x'; 1025]] {
        assert_eq!(
            store.store_imap_credential("acct_1", cfg.imap(), invalid),
            Err(SecretError::InvalidCredential)
        );
        assert_eq!(&fs::read(&handle.path).unwrap(), &old_ciphertext);
    }
    assert_eq!(
        store.store_imap_credential("acct_1", cfg.imap(), FailingReader),
        Err(SecretError::Io)
    );
    assert_eq!(&fs::read(&handle.path).unwrap(), &old_ciphertext);

    let oauth_json = r#"{"schema_version":1,"account_id":"acct_1","imap":{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"in","authentication":"oauth2"}}"#;
    let oauth = MailConnectionConfig::from_json(oauth_json.as_bytes()).unwrap();
    assert!(
        store
            .store_imap_credential("acct_1", oauth.imap(), vec![b'x'; 16_384].as_slice())
            .is_ok()
    );
    assert_eq!(
        store.store_imap_credential("acct_1", oauth.imap(), vec![b'x'; 16_385].as_slice()),
        Err(SecretError::InvalidCredential)
    );
}

#[test]
fn sec01_mail_credential_malformed_raw_envelope_is_not_bound() {
    let fixture = TempGuard::new("malformed");
    let store = fixture.store();
    let cfg = config("imap.example", 1, None, 1);
    let raw = store
        .store_from_reader(
            "acct_1",
            SecretKind::Mail(MailCredentialSlot::ImapAppPassword),
            valid_envelope("acct_1", "imap.example", 993, "in", 1, b"password", true).as_slice(),
        )
        .unwrap();
    assert_eq!(
        store.verify_imap_credential("acct_1", cfg.imap()),
        Err(SecretError::CredentialMismatch)
    );
    assert!(store.contains(&raw));
}

#[test]
fn sec01_mail_credentials_debug_and_errors_redact_sensitive_context() {
    let fixture = TempGuard::new("debug");
    let store = fixture.store();
    let cfg = config("imap.example", 1, None, 1);
    let handle = store
        .store_imap_credential("acct_1", cfg.imap(), b"fixture-secret".as_slice())
        .unwrap();
    let debug = format!("{handle:?}");
    assert!(!debug.contains("acct_1"));
    assert!(!debug.contains("fixture-secret"));
    assert!(debug.contains("<redacted>"));
    assert!(!format!("{:?}", cfg.imap()).contains("imap.example"));
    assert!(
        !SecretError::CredentialMismatch
            .to_string()
            .contains("imap.example")
    );
}

#[test]
fn sec01_mail_maximum_exact_bytes_and_oauth_rejection_preserve_ciphertext() {
    let f = TempGuard::new("exact");
    let s = f.store();
    let c = config("imap.example", 1, Some("smtp.example"), 1);
    let password = format!("{} ", "가".repeat(341));
    assert_eq!(password.len(), 1024);
    let h = s
        .store_imap_credential("acct_1", c.imap(), password.as_bytes())
        .unwrap();
    s.verify_imap_credential("acct_1", c.imap()).unwrap();
    let plain = windows_impl::read_for_test(&h.path, &h.account_id, h.kind).unwrap();
    assert_eq!(
        plain.as_slice(),
        valid_envelope(
            "acct_1",
            "imap.example",
            993,
            "in",
            1,
            password.as_bytes(),
            false
        )
    );
    let token = vec![b'x'; 16384];
    let h = s
        .store_smtp_credential("acct_1", smtp(&c), token.as_slice())
        .unwrap();
    let before = fs::read(&h.path).unwrap();
    for invalid in [
        b"has space".as_slice(),
        b"carriage\rreturn",
        b"line\nfeed",
        b"\0",
        b"\x01",
        b"\x7f",
        "한".as_bytes(),
        vec![b'x'; 16385].as_slice(),
    ] {
        assert_eq!(
            s.store_smtp_credential("acct_1", smtp(&c), invalid),
            Err(SecretError::InvalidCredential)
        );
        assert_eq!(fs::read(&h.path).unwrap(), before);
    }
    s.verify_smtp_credential("acct_1", smtp(&c)).unwrap();
}

#[test]
fn sec01_mail_envelope_declared_length_truncation_and_controls_are_rejected() {
    let f = TempGuard::new("envelope");
    let s = f.store();
    let c = config("imap.example", 1, None, 1);
    let valid = valid_envelope("acct_1", "imap.example", 993, "in", 1, b"password", false);
    let write = |bytes: &[u8]| {
        s.store_from_reader(
            "acct_1",
            SecretKind::Mail(MailCredentialSlot::ImapAppPassword),
            bytes,
        )
        .unwrap()
    };
    write(&valid);
    s.verify_imap_credential("acct_1", c.imap()).unwrap();
    let length_offset = valid.len() - 8 - 4;
    for length in [0u32, 7, 9, u32::MAX] {
        let mut bad = valid.clone();
        bad[length_offset..length_offset + 4].copy_from_slice(&length.to_be_bytes());
        write(&bad);
        assert_eq!(
            s.verify_imap_credential("acct_1", c.imap()),
            Err(SecretError::CredentialMismatch)
        );
    }
    for cut in [1, length_offset, length_offset + 3, valid.len() - 1] {
        write(&valid[..cut]);
        assert_eq!(
            s.verify_imap_credential("acct_1", c.imap()),
            Err(SecretError::CredentialMismatch)
        );
    }
    let mut bad = valid.clone();
    *bad.last_mut().unwrap() = 0x01;
    write(&bad);
    assert_eq!(
        s.verify_imap_credential("acct_1", c.imap()),
        Err(SecretError::CredentialMismatch)
    );
}

#[test]
fn sec01_mail_envelope_cannot_be_reprotected_for_another_role_account_or_auth_slot() {
    let f = TempGuard::new("inner-binding");
    let s = f.store();
    let c = config("imap.example", 1, Some("smtp.example"), 1);
    let valid = valid_envelope("acct_1", "imap.example", 993, "in", 1, b"password", false);
    s.store_from_reader(
        "acct_2",
        SecretKind::Mail(MailCredentialSlot::ImapAppPassword),
        valid.as_slice(),
    )
    .unwrap();
    assert_eq!(
        s.verify_imap_credential("acct_2", c.imap()),
        Err(SecretError::CredentialMismatch)
    );
    let mut value = serde_json::to_value(&c).unwrap();
    value["smtp"] = value["imap"].clone();
    let same = MailConnectionConfig::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
    s.store_from_reader(
        "acct_1",
        SecretKind::Mail(MailCredentialSlot::SmtpAppPassword),
        valid.as_slice(),
    )
    .unwrap();
    assert_eq!(
        s.verify_smtp_credential("acct_1", smtp(&same)),
        Err(SecretError::CredentialMismatch)
    );
    value["imap"]["authentication"] = serde_json::Value::from("oauth2");
    let oauth = MailConnectionConfig::from_json(&serde_json::to_vec(&value).unwrap()).unwrap();
    s.store_from_reader(
        "acct_1",
        SecretKind::Mail(MailCredentialSlot::ImapOAuthRefresh),
        b"refresh-only".as_slice(),
    )
    .unwrap();
    assert_eq!(
        s.verify_imap_credential("acct_1", oauth.imap()),
        Err(SecretError::NotFound)
    );
    s.store_from_reader(
        "acct_1",
        SecretKind::Mail(MailCredentialSlot::ImapOAuthAccess),
        valid.as_slice(),
    )
    .unwrap();
    assert_eq!(
        s.verify_imap_credential("acct_1", oauth.imap()),
        Err(SecretError::CredentialMismatch)
    );
}

#[test]
fn sec01_mail_reader_stops_at_limit_plus_one_and_partial_io_error_preserves_old_value() {
    struct Counted<'a>(&'a std::cell::Cell<usize>);
    impl Read for Counted<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            out.fill(b'x');
            self.0.set(self.0.get() + out.len());
            Ok(out.len())
        }
    }
    struct PartialFailure(bool);
    impl Read for PartialFailure {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.0 {
                Err(io::Error::other("fixture"))
            } else {
                out[0] = b'x';
                self.0 = true;
                Ok(1)
            }
        }
    }
    let f = TempGuard::new("bounded-reader");
    let s = f.store();
    let c = config("imap.example", 1, None, 1);
    let h = s
        .store_imap_credential("acct_1", c.imap(), b"old".as_slice())
        .unwrap();
    let before = fs::read(&h.path).unwrap();
    let count = std::cell::Cell::new(0);
    assert_eq!(
        s.store_imap_credential("acct_1", c.imap(), Counted(&count)),
        Err(SecretError::InvalidCredential)
    );
    assert_eq!(count.get(), 1025);
    assert_eq!(
        s.store_imap_credential("acct_1", c.imap(), PartialFailure(false)),
        Err(SecretError::Io)
    );
    assert_eq!(fs::read(&h.path).unwrap(), before);
    s.verify_imap_credential("acct_1", c.imap()).unwrap();
}
