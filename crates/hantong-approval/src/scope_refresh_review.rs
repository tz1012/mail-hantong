//! Immutable, non-authorizing review data for the default CLI scope refresh.

use crate::resources::{TextKey, text};
use hantong_contracts::ErrorCode;
use hantong_domain::config::Language;
use std::fmt;

const MAX_LIFETIME_MS: i64 = 300_000;
const MAX_ACCOUNT_IDS: usize = 128;

impl crate::registration_review::ReviewSnapshot for ScopeRefreshReview {
    fn created_at_utc_ms(&self) -> i64 {
        self.created_at_utc_ms
    }
    fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ScopeRefreshReview {
    refresh_id: [u8; 32],
    refresh_version: u64,
    registration_id: [u8; 16],
    original_request_id: [u8; 32],
    base_registration_version: u64,
    base_profile_epoch: u64,
    client_public_key_hash: [u8; 32],
    server_public_key_hash: [u8; 32],
    old_account_ids: Vec<String>,
    target_account_ids: Vec<String>,
    created_at_utc_ms: i64,
    expires_at_utc_ms: i64,
}

impl fmt::Debug for ScopeRefreshReview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ScopeRefreshReview { <redacted> }")
    }
}

impl ScopeRefreshReview {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        refresh_id: [u8; 32],
        refresh_version: u64,
        registration_id: [u8; 16],
        original_request_id: [u8; 32],
        base_registration_version: u64,
        base_profile_epoch: u64,
        client_public_key_hash: [u8; 32],
        server_public_key_hash: [u8; 32],
        old_account_ids: Vec<String>,
        target_account_ids: Vec<String>,
        created_at_utc_ms: i64,
        expires_at_utc_ms: i64,
    ) -> Result<Self, ErrorCode> {
        let lifetime = expires_at_utc_ms.checked_sub(created_at_utc_ms);
        if refresh_id == [0; 32]
            || refresh_version == 0
            || refresh_version > i64::MAX as u64
            || registration_id == [0; 16]
            || original_request_id == [0; 32]
            || base_registration_version == 0
            || base_registration_version > (i64::MAX as u64).saturating_sub(2)
            || base_profile_epoch == 0
            || base_profile_epoch > (i64::MAX as u64).saturating_sub(1)
            || client_public_key_hash == [0; 32]
            || server_public_key_hash == [0; 32]
            || created_at_utc_ms <= 0
            || !lifetime.is_some_and(|value| (1..=MAX_LIFETIME_MS).contains(&value))
            || !valid_account_ids(&old_account_ids)
            || !valid_account_ids(&target_account_ids)
            || old_account_ids == target_account_ids
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            refresh_id,
            refresh_version,
            registration_id,
            original_request_id,
            base_registration_version,
            base_profile_epoch,
            client_public_key_hash,
            server_public_key_hash,
            old_account_ids,
            target_account_ids,
            created_at_utc_ms,
            expires_at_utc_ms,
        })
    }

    pub fn refresh_id(&self) -> &[u8; 32] {
        &self.refresh_id
    }
    pub fn refresh_version(&self) -> u64 {
        self.refresh_version
    }
    pub fn registration_id(&self) -> &[u8; 16] {
        &self.registration_id
    }
    pub fn original_request_id(&self) -> &[u8; 32] {
        &self.original_request_id
    }
    pub fn base_registration_version(&self) -> u64 {
        self.base_registration_version
    }
    pub fn base_profile_epoch(&self) -> u64 {
        self.base_profile_epoch
    }
    pub fn target_profile_epoch(&self) -> u64 {
        self.base_profile_epoch + 1
    }
    pub fn client_public_key_hash(&self) -> &[u8; 32] {
        &self.client_public_key_hash
    }
    pub fn server_public_key_hash(&self) -> &[u8; 32] {
        &self.server_public_key_hash
    }
    pub fn old_account_ids(&self) -> &[String] {
        &self.old_account_ids
    }
    pub fn target_account_ids(&self) -> &[String] {
        &self.target_account_ids
    }
    pub fn created_at_utc_ms(&self) -> i64 {
        self.created_at_utc_ms
    }
    pub fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }

    pub fn profile_name(&self) -> &'static str {
        "default"
    }
    pub fn read_only(&self) -> bool {
        true
    }
    pub fn requests_per_minute(&self) -> u32 {
        60
    }
    pub fn tool_name(&self) -> &'static str {
        "mail.accounts.list"
    }

    pub fn summary(&self, language: Language) -> String {
        let t = |key| text(language, key);
        let old = if self.old_account_ids.is_empty() {
            t(TextKey::ScopeRefreshNoAccounts).to_owned()
        } else {
            self.old_account_ids.join("\r\n")
        };
        let target = if self.target_account_ids.is_empty() {
            t(TextKey::ScopeRefreshNoAccounts).to_owned()
        } else {
            self.target_account_ids.join("\r\n")
        };
        format!(
            "{}\r\n{}: {}\r\n{}:\r\n{}\r\n{}:\r\n{}\r\n{}: {} -> {}\r\n{}",
            t(TextKey::ScopeRefreshIntroduction),
            t(TextKey::Profile),
            t(TextKey::ProfileValue),
            t(TextKey::ScopeRefreshCurrentScope),
            old,
            t(TextKey::ScopeRefreshTargetScope),
            target,
            t(TextKey::ScopeRefreshEpochChange),
            self.base_profile_epoch,
            self.target_profile_epoch(),
            t(TextKey::ScopeRefreshDisconnected),
        )
    }
}

fn valid_account_ids(values: &[String]) -> bool {
    values.len() <= MAX_ACCOUNT_IDS
        && values.windows(2).all(|pair| pair[0] < pair[1])
        && values.iter().all(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review(old: Vec<String>, target: Vec<String>) -> ScopeRefreshReview {
        ScopeRefreshReview::new(
            [1; 32],
            1,
            [2; 16],
            [3; 32],
            4,
            5,
            [6; 32],
            [7; 32],
            old,
            target,
            1_700_000_000_000,
            1_700_000_300_000,
        )
        .unwrap()
    }

    #[test]
    fn cli15_scope_refresh_review_binds_old_target_and_epoch() {
        let value = review(vec!["acct_a".into()], vec!["acct_b".into()]);
        assert_eq!(value.old_account_ids(), &["acct_a"]);
        assert_eq!(value.target_account_ids(), &["acct_b"]);
        assert_eq!(value.target_profile_epoch(), 6);
        assert!(value.read_only());
        assert_eq!(value.tool_name(), "mail.accounts.list");
        assert_ne!(value, review(vec!["acct_a".into()], vec!["acct_c".into()]));
    }

    #[test]
    fn cli15_scope_refresh_review_rejects_invalid_lists_and_bounds() {
        assert!(
            ScopeRefreshReview::new(
                [1; 32],
                1,
                [2; 16],
                [3; 32],
                0,
                5,
                [6; 32],
                [7; 32],
                vec!["acct_a".into()],
                vec!["acct_b".into()],
                1,
                2,
            )
            .is_err()
        );
        assert!(
            ScopeRefreshReview::new(
                [1; 32],
                1,
                [2; 16],
                [3; 32],
                4,
                0,
                [6; 32],
                [7; 32],
                vec!["acct_a".into()],
                vec!["acct_b".into()],
                1,
                2,
            )
            .is_err()
        );
        assert!(
            ScopeRefreshReview::new(
                [1; 32],
                1,
                [2; 16],
                [3; 32],
                4,
                5,
                [6; 32],
                [7; 32],
                vec!["acct_b".into(), "acct_a".into()],
                vec!["acct_c".into()],
                1,
                2,
            )
            .is_err()
        );
        assert!(
            ScopeRefreshReview::new(
                [1; 32],
                1,
                [2; 16],
                [3; 32],
                4,
                5,
                [6; 32],
                [7; 32],
                vec!["acct_a".into()],
                vec!["acct_a".into()],
                1,
                2,
            )
            .is_err()
        );
        assert!(
            ScopeRefreshReview::new(
                [1; 32],
                1,
                [2; 16],
                [3; 32],
                4,
                5,
                [6; 32],
                [7; 32],
                vec!["bad.id".into()],
                vec!["acct_b".into()],
                1,
                2,
            )
            .is_err()
        );
        assert!(
            ScopeRefreshReview::new(
                [1; 32],
                1,
                [2; 16],
                [3; 32],
                4,
                5,
                [6; 32],
                [7; 32],
                vec!["acct_a".into()],
                vec!["acct_b".into()],
                1,
                300_002,
            )
            .is_err()
        );
    }

    #[test]
    fn cli15_scope_refresh_summary_is_bilingual_and_explicit() {
        let value = review(vec!["acct_a".into()], vec!["acct_b".into()]);
        assert!(
            value
                .summary(Language::En)
                .contains("Current allowed accounts")
        );
        assert!(value.summary(Language::En).contains("acct_a\r\n"));
        assert!(value.summary(Language::En).contains("acct_b\r\n"));
        assert!(value.summary(Language::Ko).contains("현재 허용 계정"));
    }
}
