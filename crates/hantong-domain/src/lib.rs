#![forbid(unsafe_code)]

//! Provider-neutral configuration and bounded time-zone interpretation.

pub mod account_provisioning;
pub mod accounts;
pub mod capabilities;
pub mod config;
pub mod imap_reconcile;
pub mod imap_recovery;
pub mod imap_sync;
pub mod mail_connection;
pub mod mailboxes;
pub mod smtp_capabilities;
pub(crate) mod timezone;

#[cfg(test)]
mod config_tests {
    use super::config::*;
    use super::timezone;

    fn base() -> Config {
        Config::defaults("UTC").unwrap()
    }

    #[test]
    fn m105_defaults_keep_cli_english_and_gui_os_fallback_independent() {
        let c = base();
        assert_eq!(c.cli.language, Language::En);
        assert_eq!(c.initial_gui_language(true), Language::Ko);
        assert_eq!(c.initial_gui_language(false), Language::En);
    }
    #[test]
    fn m105_json_rejects_unknown_duplicate_and_invalid_nested_values_without_echo() {
        let c = base();
        let mut text = serde_json::to_vec(&c).unwrap();
        text.extend_from_slice(b" ");
        assert!(Config::from_json(&text).is_ok());
        assert_eq!(Config::from_json(br#"{"schema_version":1,"settings_version":1,"cli":{"language":"en","language":"ko"},"gui":{"theme":"system"},"locale":{"timezone_id":"UTC"}}"#), Err(ConfigError::InvalidJson));
        assert_eq!(Config::from_json(br#"{"schema_version":1,"settings_version":1,"cli":{"language":"en"},"gui":{"theme":"system","extra":true},"locale":{"timezone_id":"UTC"}}"#), Err(ConfigError::InvalidJson));
        assert_eq!(Config::from_json(br#"{"schema_version":1,"settings_version":1,"cli":{"language":"en"},"gui":{"theme":"system"},"locale":{"timezone_id":"+09:00"}}"#), Err(ConfigError::InvalidTimezone));
        assert!(!format!("{:?}", ConfigError::InvalidJson).contains("secret"));
    }
    #[test]
    fn m105_rejects_oversized_input() {
        let mut value = serde_json::to_vec(&base()).unwrap();
        value.resize(65 * 1024, b' ');
        assert_eq!(Config::from_json(&value), Err(ConfigError::TooLarge));
    }
    #[test]
    fn m105_unprivileged_cas_and_security_mutation_are_blocked() {
        let previous = base();
        let mut candidate = previous.clone();
        assert_eq!(
            candidate.validate_unprivileged_replacement(&previous, 0),
            Err(ConfigError::StaleVersion)
        );
        candidate.security.external_ai_enabled = true;
        assert_eq!(
            candidate.validate_unprivileged_replacement(&previous, 1),
            Err(ConfigError::SecurityProtected)
        );
        candidate.security = previous.security.clone();
        let accepted = candidate
            .validate_unprivileged_replacement(&previous, 1)
            .unwrap();
        assert_eq!(accepted.settings_version, 2);
    }
    #[test]
    fn m105_cas_exhaustion_is_rejected() {
        let mut p = base();
        p.settings_version = i64::MAX as u64;
        let c = p.clone();
        assert_eq!(
            c.validate_unprivileged_replacement(&p, p.settings_version),
            Err(ConfigError::VersionExhausted)
        );
    }
    #[test]
    fn m105_timezone_validation_and_midnight_reject_bad_ids() {
        assert!(timezone::validate_id("America/Los_Angeles").is_ok());
        assert!(timezone::validate_id("../secret").is_err());
        assert!(timezone::midnight_timestamp("UTC", 2026, 1, 1).is_ok());
    }

    #[test]
    fn m105_optional_gui_language_is_omitted_and_explicit_null_is_rejected() {
        let encoded = serde_json::to_value(base()).unwrap();
        assert!(encoded["gui"].get("language").is_none());
        let mut invalid = encoded;
        invalid["gui"]["language"] = serde_json::Value::Null;
        assert_eq!(
            Config::from_json(&serde_json::to_vec(&invalid).unwrap()),
            Err(ConfigError::InvalidJson)
        );
    }

    #[test]
    fn mail03_midnight_rejects_dst_gap_fold_and_skipped_date_without_panics() {
        for (zone, year, month, day) in [
            ("America/Sao_Paulo", 2018, 11, 4),
            ("America/Havana", 2020, 11, 1),
            ("Pacific/Apia", 2011, 12, 30),
        ] {
            assert_eq!(
                timezone::midnight_timestamp(zone, year, month, day),
                Err(timezone::TimezoneError::AmbiguousOrSkipped)
            );
        }
        for (year, month, day) in [(2026, 0, 1), (2026, 2, 30), (2026, 13, 1)] {
            assert_eq!(
                timezone::midnight_timestamp("UTC", year, month, day),
                Err(timezone::TimezoneError::Invalid)
            );
        }
    }

    #[test]
    fn sec08_invalid_configuration_debug_cannot_reflect_free_text() {
        let mut c = base();
        c.locale.timezone_id = "private-sentinel@example.invalid\nsecret".into();
        assert!(!format!("{c:?}").contains("sentinel"));
        assert_eq!(c.validate(), Err(ConfigError::InvalidTimezone));
    }
}
