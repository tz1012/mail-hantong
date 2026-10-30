//! Immutable, non-authorizing review data for the default CLI scope refresh.

use crate::resources::{TextKey, text};
use hantong_contracts::{
    ErrorCode,
    auth::{default_cli_legacy_tools, is_supported_default_cli_toolset_transition},
};
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
    old_tool_ids: Vec<String>,
    target_tool_ids: Vec<String>,
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
        Self::new_with_tools(
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
            default_cli_legacy_tools(),
            default_cli_legacy_tools(),
            created_at_utc_ms,
            expires_at_utc_ms,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_tools(
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
        old_tool_ids: Vec<String>,
        target_tool_ids: Vec<String>,
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
            || !is_supported_default_cli_toolset_transition(&old_tool_ids, &target_tool_ids)
            || (old_account_ids == target_account_ids && old_tool_ids == target_tool_ids)
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
            old_tool_ids,
            target_tool_ids,
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
    pub fn old_tool_ids(&self) -> &[String] {
        &self.old_tool_ids
    }
    pub fn target_tool_ids(&self) -> &[String] {
        &self.target_tool_ids
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
        let old_tools = self.old_tool_ids.join(", ");
        let target_tools = self.target_tool_ids.join(", ");
        let mut summary = format!(
            "{}\r\n{}: {}\r\n{}:\r\n{}\r\n{}:\r\n{}\r\n{}: {}\r\n{}: {}\r\n{}: {} -> {}\r\n{}",
            t(TextKey::ScopeRefreshIntroduction),
            t(TextKey::Profile),
            t(TextKey::ProfileValue),
            t(TextKey::ScopeRefreshCurrentScope),
            old,
            t(TextKey::ScopeRefreshTargetScope),
            target,
            t(TextKey::ScopeRefreshCurrentTools),
            old_tools,
            t(TextKey::ScopeRefreshTargetTools),
            target_tools,
            t(TextKey::ScopeRefreshEpochChange),
            self.base_profile_epoch,
            self.target_profile_epoch(),
            t(TextKey::ScopeRefreshDisconnected),
        );
        if self
            .target_tool_ids
            .iter()
            .any(|tool| tool == "mail.sync.page")
        {
            summary.push_str("\r\n");
            summary.push_str(t(TextKey::ScopeRefreshUidPagePermission));
        }
        summary
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

    #[test]
    fn cli15_uid_scope_review_explains_exact_tool_only_upgrade_in_both_languages() {
        let folder = hantong_contracts::auth::default_cli_folder_tools();
        let mut uid = folder.clone();
        uid.push("mail.sync.page".into());
        let build = |old, target| {
            ScopeRefreshReview::new_with_tools(
                [1; 32],
                1,
                [2; 16],
                [3; 32],
                4,
                5,
                [6; 32],
                [7; 32],
                vec!["acct".into()],
                vec!["acct".into()],
                old,
                target,
                1,
                300001,
            )
        };
        let review = build(folder.clone(), uid.clone()).unwrap();
        for (language, explanation) in [(Language::En, "metadata"), (Language::Ko, "메타데이터")]
        {
            let summary = review.summary(language);
            assert!(summary.contains("mail.sync.page"));
            assert!(summary.contains(explanation));
            assert!(
                summary.contains("mail.accounts.list, mail.folders.list, mail.folders.refresh")
            );
        }
        assert_eq!(build(uid.clone(), folder), Err(ErrorCode::InvalidInput));
        assert_eq!(
            build(uid.clone(), default_cli_legacy_tools()),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(build(uid.clone(), uid), Err(ErrorCode::InvalidInput));
    }

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
        assert_eq!(value.target_tool_ids(), &["mail.accounts.list"]);
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

    #[test]
    fn cli15_scope_refresh_review_binds_tool_only_upgrade_and_displays_tools() {
        let value = ScopeRefreshReview::new_with_tools(
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
            default_cli_legacy_tools(),
            hantong_contracts::auth::default_cli_folder_tools(),
            1_700_000_000_000,
            1_700_000_300_000,
        )
        .unwrap();
        assert!(value.summary(Language::En).contains("mail.folders.refresh"));
        assert_ne!(
            value,
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
                1_700_000_000_000,
                1_700_000_300_000
            )
            .unwrap()
        );
    }
    #[test]
    fn cli15_native_review_tool_only_delta_has_distinct_bilingual_labels_and_no_downgrade() {
        let build = |old, new| {
            ScopeRefreshReview::new_with_tools(
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
                old,
                new,
                1,
                300001,
            )
        };
        let review = build(
            default_cli_legacy_tools(),
            hantong_contracts::auth::default_cli_folder_tools(),
        )
        .unwrap();
        assert!(
            build(
                hantong_contracts::auth::default_cli_folder_tools(),
                default_cli_legacy_tools()
            )
            .is_err()
        );
        for (lang, old, new) in [
            (Language::En, "Current allowed tools", "New allowed tools"),
            (Language::Ko, "현재 허용 도구", "새 허용 도구"),
        ] {
            let text = review.summary(lang);
            assert!(text.contains(&format!("{old}: mail.accounts.list\r\n")));
            assert!(text.contains(&format!(
                "{new}: mail.accounts.list, mail.folders.list, mail.folders.refresh\r\n"
            )));
        }
    }
}
