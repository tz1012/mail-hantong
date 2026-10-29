//! Provider-neutral, non-authoritative account display values.

use serde::Serialize;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountProvider {
    Gmail,
    Graph,
    Imap,
    Archive,
}
impl AccountProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gmail => "gmail",
            Self::Graph => "graph",
            Self::Imap => "imap",
            Self::Archive => "archive",
        }
    }
    pub fn parse(value: &str) -> Result<Self, AccountDisplayError> {
        match value {
            "gmail" => Ok(Self::Gmail),
            "graph" => Ok(Self::Graph),
            "imap" => Ok(Self::Imap),
            "archive" => Ok(Self::Archive),
            _ => Err(AccountDisplayError::InvalidInput),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    Ready,
    Connecting,
    ReauthRequired,
    Offline,
    Disabled,
    Error,
}
impl AccountStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Connecting => "connecting",
            Self::ReauthRequired => "reauth_required",
            Self::Offline => "offline",
            Self::Disabled => "disabled",
            Self::Error => "error",
        }
    }
    pub fn parse(value: &str) -> Result<Self, AccountDisplayError> {
        match value {
            "ready" => Ok(Self::Ready),
            "connecting" => Ok(Self::Connecting),
            "reauth_required" => Ok(Self::ReauthRequired),
            "offline" => Ok(Self::Offline),
            "disabled" => Ok(Self::Disabled),
            "error" => Ok(Self::Error),
            _ => Err(AccountDisplayError::InvalidInput),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountDisplayError {
    InvalidInput,
}

#[derive(Clone, PartialEq, Serialize)]
pub struct AccountSummary {
    pub account_id: String,
    pub provider: AccountProvider,
    pub primary_address: Option<String>,
    pub display_name: Option<String>,
    pub color: Option<String>,
    pub account_number: u64,
    pub status: AccountStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,
}
impl fmt::Debug for AccountSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountSummary { <redacted> }")
    }
}
impl AccountSummary {
    pub fn validate(&self) -> Result<(), AccountDisplayError> {
        if !valid_id(&self.account_id)
            || self.account_number == 0
            || self.account_number > i64::MAX as u64
        {
            return Err(AccountDisplayError::InvalidInput);
        }
        if let Some(value) = &self.primary_address {
            valid_display(value, 320, 1280)?;
        }
        if let Some(value) = &self.display_name {
            valid_display(value, 128, 512)?;
        }
        valid_color(self.color.as_deref())?;
        if let Some(values) = &self.capabilities {
            if values.len() > 128 || values.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(AccountDisplayError::InvalidInput);
            }
            for value in values {
                if value.is_empty()
                    || value.len() > 96
                    || !value.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-')
                    })
                {
                    return Err(AccountDisplayError::InvalidInput);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq)]
pub struct AccountSnapshot {
    pub items: Vec<AccountSummary>,
    pub observed_at_utc_ms: i64,
}
impl fmt::Debug for AccountSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountSnapshot { <redacted> }")
    }
}
impl AccountSnapshot {
    pub fn observed_at_rfc3339(&self) -> Result<String, AccountDisplayError> {
        if self.observed_at_utc_ms <= 0 {
            return Err(AccountDisplayError::InvalidInput);
        }
        jiff::Timestamp::from_millisecond(self.observed_at_utc_ms)
            .map(|value| value.to_string())
            .map_err(|_| AccountDisplayError::InvalidInput)
    }

    pub fn validate(&self) -> Result<(), AccountDisplayError> {
        self.observed_at_rfc3339()?;
        let mut ids = std::collections::BTreeSet::new();
        if !(1..=128).contains(&self.items.len())
            || self.items.iter().any(|item| !ids.insert(&item.account_id))
            || self.items.windows(2).any(|pair| {
                (pair[0].account_number, &pair[0].account_id)
                    > (pair[1].account_number, &pair[1].account_id)
            })
        {
            return Err(AccountDisplayError::InvalidInput);
        }
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }
}

/// Formats a positive UTC millisecond timestamp with the fixed millisecond
/// precision required by tool result contracts.
pub fn utc_millis_rfc3339(value: i64) -> Result<String, AccountDisplayError> {
    if value <= 0 {
        return Err(AccountDisplayError::InvalidInput);
    }
    jiff::Timestamp::from_millisecond(value)
        .map(|timestamp| timestamp.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .map_err(|_| AccountDisplayError::InvalidInput)
}

pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
pub(crate) fn valid_color(value: Option<&str>) -> Result<(), AccountDisplayError> {
    if let Some(value) = value
        && (value.len() != 7
            || !value.starts_with('#')
            || !value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit))
    {
        return Err(AccountDisplayError::InvalidInput);
    }
    Ok(())
}

pub(crate) fn valid_display(
    value: &str,
    max_chars: usize,
    max_bytes: usize,
) -> Result<(), AccountDisplayError> {
    if value.is_empty()
        || value.len() > max_bytes
        || value.chars().count() > max_chars
        || value.trim().is_empty()
        || value.chars().any(char::is_control)
    {
        Err(AccountDisplayError::InvalidInput)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn summary() -> AccountSummary {
        AccountSummary {
            account_id: "account-1".into(),
            provider: AccountProvider::Gmail,
            primary_address: Some("user@example.test".into()),
            display_name: Some("User".into()),
            color: Some("#aBc012".into()),
            account_number: 1,
            status: AccountStatus::Ready,
            capabilities: Some(vec!["mail.read".into(), "mail.send".into()]),
        }
    }
    #[test]
    fn mail01_provider_status_roundtrip_and_serialization_shape() {
        for provider in [
            AccountProvider::Gmail,
            AccountProvider::Graph,
            AccountProvider::Imap,
            AccountProvider::Archive,
        ] {
            assert_eq!(AccountProvider::parse(provider.as_str()), Ok(provider));
        }
        for status in [
            AccountStatus::Ready,
            AccountStatus::Connecting,
            AccountStatus::ReauthRequired,
            AccountStatus::Offline,
            AccountStatus::Disabled,
            AccountStatus::Error,
        ] {
            assert_eq!(AccountStatus::parse(status.as_str()), Ok(status));
        }
        let encoded = serde_json::to_value(summary()).unwrap();
        assert_eq!(encoded["provider"], json!("gmail"));
        assert_eq!(encoded["status"], json!("ready"));
        assert!(encoded.get("provider_subject").is_none());
    }
    #[test]
    fn mail01_bounds_reject_bad_ids_display_values_and_numbers() {
        let mut item = summary();
        item.account_id = "한글".into();
        assert!(item.validate().is_err());
        item = summary();
        item.primary_address = Some(" ".into());
        assert!(item.validate().is_err());
        item = summary();
        item.primary_address = Some("😀".repeat(321));
        assert!(item.validate().is_err());
        item = summary();
        item.display_name = Some("x\n".into());
        assert!(item.validate().is_err());
        item = summary();
        item.color = Some("#12345g".into());
        assert!(item.validate().is_err());
        item = summary();
        item.account_number = 0;
        assert!(item.validate().is_err());
        item.account_number = i64::MAX as u64 + 1;
        assert!(item.validate().is_err());
    }
    #[test]
    fn mail01_capabilities_are_sorted_unique_ascii_and_bounded() {
        let mut item = summary();
        item.primary_address = Some("😀".repeat(320));
        item.display_name = Some("😀".repeat(128));
        item.account_id = "a".repeat(128);
        item.account_number = i64::MAX as u64;
        item.capabilities = Some(
            (0..128)
                .map(|i| format!("cap{i:03}{}", "x".repeat(90)))
                .collect(),
        );
        assert!(item.validate().is_ok());
        item.capabilities = Some(vec!["mail.send".into(), "mail.read".into()]);
        assert!(item.validate().is_err());
        item.capabilities = Some(vec!["mail.read".into(), "mail.read".into()]);
        assert!(item.validate().is_err());
        item.capabilities = Some((0..129).map(|i| format!("cap{i:03}")).collect());
        assert!(item.validate().is_err());
        item.capabilities = Some(vec!["한글".into()]);
        assert!(item.validate().is_err());
        item.capabilities = Some(vec!["a".repeat(97)]);
        assert!(item.validate().is_err());
    }
    #[test]
    fn mail01_debug_redacts_display_values_and_snapshot_validates_time() {
        let item = summary();
        let text = format!("{item:?}");
        assert!(!text.contains("user@example"));
        assert!(!text.contains("account-1"));
        let snapshot = AccountSnapshot {
            items: vec![item],
            observed_at_utc_ms: 1,
        };
        assert!(snapshot.validate().is_ok());
        assert_eq!(
            snapshot.observed_at_rfc3339().unwrap(),
            "1970-01-01T00:00:00.001Z"
        );
        assert!(!format!("{snapshot:?}").contains("user@example"));
        assert!(
            AccountSnapshot {
                items: Vec::new(),
                observed_at_utc_ms: 0
            }
            .validate()
            .is_err()
        );
        for timestamp in [0, -1, i64::MAX] {
            let snapshot = AccountSnapshot {
                items: Vec::new(),
                observed_at_utc_ms: timestamp,
            };
            assert!(snapshot.observed_at_rfc3339().is_err());
        }
    }
}
