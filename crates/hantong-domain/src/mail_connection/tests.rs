use super::*;

fn json(imap_host: &str, smtp: Option<&str>) -> Vec<u8> {
    let smtp = smtp.map(|host| format!(r#","smtp":{{"settings_version":2,"host":"{host}","port":465,"tls":"implicit_tls","login_name":"out","authentication":"oauth2"}}"#)).unwrap_or_default();
    format!(r#"{{"schema_version":1,"account_id":"acct_1","imap":{{"settings_version":1,"host":"{imap_host}","port":993,"tls":"implicit_tls","login_name":"in","authentication":"app_password"}}{smtp}}}"#).into_bytes()
}

#[test]
fn ext01_m201_validates_and_normalizes_endpoints_independently() {
    let config =
        MailConnectionConfig::from_json(&json("MAIL.Example.COM", Some("smtp.example.com")))
            .unwrap();
    assert_eq!(config.imap().host(), "mail.example.com");
    assert_eq!(config.smtp().unwrap().host(), "smtp.example.com");
    assert_eq!(config.imap().port(), 993);
    assert_eq!(config.smtp().unwrap().port(), 465);
    assert_eq!(
        config.smtp().unwrap().authentication(),
        Authentication::Oauth2
    );
    let roundtrip = serde_json::to_vec(&config).unwrap();
    let reparsed = MailConnectionConfig::from_json(&roundtrip).unwrap();
    assert_eq!(config, reparsed);
}

#[test]
fn ext01_m201_accepts_standard_ips_and_rejects_ambiguous_hosts() {
    for host in [
        "127.0.0.1",
        "::1",
        "2001:db8::1",
        "localhost",
        "face.example",
        "0xdead.example",
        "imap.0xbeef.example",
    ] {
        assert!(
            MailConnectionConfig::from_json(&json(host, None)).is_ok(),
            "{host}"
        );
    }
    for host in [
        "127.1",
        "1.2.3.999",
        "[::1]",
        "::1%3",
        "https://x",
        "user@x",
        "x/path",
        "x.",
        "x_y",
        "x y",
        "0x7f000001",
        "0x7f.1",
        "0x7f.0x0.0x0.1",
    ] {
        assert_eq!(
            MailConnectionConfig::from_json(&json(host, None)),
            Err(MailConnectionError::Invalid),
            "{host}"
        );
    }
}

#[test]
fn ext01_m201_enforces_versions_ports_and_login_bounds() {
    for port in [0, 65536] {
        let mut v = String::from_utf8(json("imap.example", None)).unwrap();
        v = v.replace("\"port\":993", &format!("\"port\":{port}"));
        assert!(MailConnectionConfig::from_json(v.as_bytes()).is_err());
    }
    for port in [1, 65535] {
        let mut v = String::from_utf8(json("imap.example", None)).unwrap();
        v = v.replace("\"port\":993", &format!("\"port\":{port}"));
        assert!(MailConnectionConfig::from_json(v.as_bytes()).is_ok());
    }
    let float_port = String::from_utf8(json("imap.example", None))
        .unwrap()
        .replace("\"port\":993", "\"port\":993.5");
    assert!(MailConnectionConfig::from_json(float_port.as_bytes()).is_err());
    for version in [0, i64::MAX as u64 + 1] {
        let mut v = String::from_utf8(json("imap.example", None)).unwrap();
        v = v.replace(
            "\"settings_version\":1",
            &format!("\"settings_version\":{version}"),
        );
        assert!(MailConnectionConfig::from_json(v.as_bytes()).is_err());
    }
    let max_version = String::from_utf8(json("imap.example", None))
        .unwrap()
        .replace(
            "\"settings_version\":1",
            &format!("\"settings_version\":{}", i64::MAX),
        );
    assert!(MailConnectionConfig::from_json(max_version.as_bytes()).is_ok());
    for login in ["", " ", " in", "in ", "a\n"] {
        let mut v = String::from_utf8(json("imap.example", None)).unwrap();
        v = v.replace(
            "\"login_name\":\"in\"",
            &format!("\"login_name\":{login:?}"),
        );
        assert!(MailConnectionConfig::from_json(v.as_bytes()).is_err());
    }
}

#[test]
fn ext01_m201_rejects_secrets_unknowns_duplicates_and_wrong_shapes() {
    for extra in [
        r#","password":"secret""#,
        r#","token":"secret""#,
        r#","secret_handle":"x""#,
    ] {
        let mut v = String::from_utf8(json("imap.example", None)).unwrap();
        v.insert_str(v.len() - 1, extra);
        assert!(MailConnectionConfig::from_json(v.as_bytes()).is_err());
    }
    let duplicate = br#"{"schema_version":1,"account_id":"acct_1","account_id":"acct_2","imap":{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"in","authentication":"app_password"}}"#;
    assert_eq!(
        MailConnectionConfig::from_json(duplicate),
        Err(MailConnectionError::Invalid)
    );
    let duplicate_nested = br#"{"schema_version":1,"account_id":"acct_1","imap":{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"in","authentication":"app_password","authentication":"oauth2"}}"#;
    assert_eq!(
        MailConnectionConfig::from_json(duplicate_nested),
        Err(MailConnectionError::Invalid)
    );
    let duplicate_null_smtp = br#"{"schema_version":1,"account_id":"acct_1","imap":{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"in","authentication":"app_password"},"smtp":null,"smtp":null}"#;
    assert_eq!(
        MailConnectionConfig::from_json(duplicate_null_smtp),
        Err(MailConnectionError::Invalid)
    );
    let null_smtp = json("imap.example", None);
    assert!(MailConnectionConfig::from_json(&null_smtp).is_ok());
    assert!(
        MailConnectionConfig::from_json(
            br#"{"schema_version":1,"account_id":"acct_1","imap":null}"#
        )
        .is_err()
    );
}

#[test]
fn ext01_m201_rejects_malformed_json_tls_auth_and_unknown_nested_fields() {
    let valid = json("imap.example", None);
    let mut bom = vec![0xef, 0xbb, 0xbf];
    bom.extend_from_slice(&valid);
    assert_eq!(
        MailConnectionConfig::from_json(&bom),
        Err(MailConnectionError::Invalid)
    );
    let mut invalid_utf8 = valid.clone();
    invalid_utf8.extend_from_slice(&[0xff]);
    assert_eq!(
        MailConnectionConfig::from_json(&invalid_utf8),
        Err(MailConnectionError::Invalid)
    );
    let mut trailing = valid.clone();
    trailing.extend_from_slice(br#"{}"#);
    assert_eq!(
        MailConnectionConfig::from_json(&trailing),
        Err(MailConnectionError::Invalid)
    );
    for tls in ["plain", "auto", "opportunistic", "verify_false"] {
        let value = String::from_utf8(valid.clone())
            .unwrap()
            .replace("\"implicit_tls\"", &format!("\"{tls}\""));
        assert!(MailConnectionConfig::from_json(value.as_bytes()).is_err());
    }
    for authentication in ["password", "token", "unknown"] {
        let value = String::from_utf8(valid.clone())
            .unwrap()
            .replace("\"app_password\"", &format!("\"{authentication}\""));
        assert!(MailConnectionConfig::from_json(value.as_bytes()).is_err());
    }
    let nested = String::from_utf8(valid).unwrap().replace(
        "\"authentication\":\"app_password\"}",
        "\"authentication\":\"app_password\",\"password\":\"secret\"}",
    );
    assert!(MailConnectionConfig::from_json(nested.as_bytes()).is_err());
}

#[test]
fn ext01_m201_enforces_exact_host_and_account_boundaries() {
    let label_63 = format!("{}.example", "a".repeat(63));
    assert!(MailConnectionConfig::from_json(&json(&label_63, None)).is_ok());
    let label_64 = format!("{}.example", "a".repeat(64));
    assert!(MailConnectionConfig::from_json(&json(&label_64, None)).is_err());
    let total_253 = format!(
        "{}.{}.{}.{}",
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61)
    );
    assert_eq!(total_253.len(), 253);
    assert!(MailConnectionConfig::from_json(&json(&total_253, None)).is_ok());
    let total_254 = format!(
        "{}.{}.{}.{}",
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(62)
    );
    assert_eq!(total_254.len(), 254);
    assert!(MailConnectionConfig::from_json(&json(&total_254, None)).is_err());
    let account_128 = format!(
        r#"{{"schema_version":1,"account_id":"{}","imap":{{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"in","authentication":"app_password"}}}}"#,
        "a".repeat(128)
    );
    assert!(MailConnectionConfig::from_json(account_128.as_bytes()).is_ok());
    let account_129 = account_128.replace(&"a".repeat(128), &"a".repeat(129));
    assert!(MailConnectionConfig::from_json(account_129.as_bytes()).is_err());
}

#[test]
fn ext01_m201_getters_keep_smtp_and_imap_values_separate_and_debug_redacted() {
    let config =
        MailConnectionConfig::from_json(&json("imap.example", Some("smtp.example"))).unwrap();
    assert_eq!(config.imap().settings_version(), 1);
    assert_eq!(config.imap().login_name(), "in");
    assert_eq!(config.imap().tls(), TlsMode::ImplicitTls);
    assert_eq!(config.smtp().unwrap().settings_version(), 2);
    assert_eq!(config.smtp().unwrap().login_name(), "out");
    assert_eq!(config.smtp().unwrap().tls(), TlsMode::ImplicitTls);
    assert!(!format!("{:?}", config.imap()).contains("imap.example"));
    assert!(!format!("{:?}", config.smtp().unwrap()).contains("smtp.example"));
}

#[test]
fn ext01_m201_rejects_oversized_unicode_and_redacts_debug() {
    let exact_login = String::from_utf8(json("imap.example", None))
        .unwrap()
        .replace(
            "\"login_name\":\"in\"",
            &format!("\"login_name\":{login:?}", login = "😀".repeat(320)),
        );
    assert!(MailConnectionConfig::from_json(exact_login.as_bytes()).is_ok());
    let mut v = String::from_utf8(json("imap.example", None)).unwrap();
    v = v.replace(
        "\"login_name\":\"in\"",
        &format!("\"login_name\":{login:?}", login = "가".repeat(321)),
    );
    assert!(MailConnectionConfig::from_json(v.as_bytes()).is_err());
    assert_eq!(
        MailConnectionConfig::from_json(&vec![b' '; 16 * 1024 + 1]),
        Err(MailConnectionError::TooLarge)
    );
    let config = MailConnectionConfig::from_json(&json("secret.example", None)).unwrap();
    let debug = format!("{config:?}");
    assert!(!debug.contains("secret.example"));
    assert!(!debug.contains("acct_1"));
    assert!(!debug.contains("in"));
}
