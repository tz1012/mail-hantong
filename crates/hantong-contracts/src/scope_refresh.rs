//! Public scope-refresh progress data, never an approval or authentication proof.
use crate::ErrorCode;

#[derive(Clone, PartialEq, Eq)]
pub enum ScopeRefreshCommand {
    Request,
    RequestManage(ManageAccounts),
    RequestGoogleSend(GoogleSendAccounts),
    RequestGoogleChanges(GoogleSendAccounts),
    RequestGoogleReadOnly(crate::registration::GoogleReadAccount),
    AddGoogleReadOnly(crate::registration::GoogleReadAccount),
    Status([u8; 32]),
}

impl std::fmt::Debug for ScopeRefreshCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::RequestManage(_) => "ScopeRefreshCommand::RequestManage(<redacted>)",
            Self::RequestGoogleChanges(_) => {
                "ScopeRefreshCommand::RequestGoogleChanges(<redacted>)"
            }
            Self::RequestGoogleSend(_) => "ScopeRefreshCommand::RequestGoogleSend(<redacted>)",
            Self::Request => "ScopeRefreshCommand::Request",
            Self::RequestGoogleReadOnly(_) => {
                "ScopeRefreshCommand::RequestGoogleReadOnly(<redacted>)"
            }
            Self::AddGoogleReadOnly(_) => "ScopeRefreshCommand::AddGoogleReadOnly(<redacted>)",
            Self::Status(_) => "ScopeRefreshCommand::Status(<redacted>)",
        })
    }
}

/// Maximum aggregate UTF-8 bytes for an explicit manage-account selection.
/// This leaves room for the JSON-RPC envelope while keeping the request below
/// the authenticated 16 KiB frame limit.
pub const MAX_MANAGE_ACCOUNT_BYTES: usize = 12_288;

/// Explicit, canonical selection of generic account identifiers for terminal
/// manage scope. Empty input is rejected and never means "all accounts".
#[derive(Clone, PartialEq, Eq)]
pub struct ManageAccounts {
    ids: Vec<String>,
}

impl std::fmt::Debug for ManageAccounts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ManageAccounts(<redacted>)")
    }
}

impl ManageAccounts {
    pub fn parse(values: &[String]) -> Result<Self, ErrorCode> {
        if values.is_empty() || values.len() > 128 {
            return Err(ErrorCode::InvalidInput);
        }
        values.iter().try_fold(0usize, |total, value| {
            total
                .checked_add(value.len())
                .filter(|bytes| *bytes <= MAX_MANAGE_ACCOUNT_BYTES)
                .ok_or(ErrorCode::InvalidInput)
        })?;
        let scope = crate::auth::RequestedScope::new(values.to_vec(), Vec::new())?;
        if scope.accounts().is_empty() {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            ids: scope.accounts().to_vec(),
        })
    }

    pub fn accounts(&self) -> Vec<String> {
        self.ids.clone()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeRefreshProgress {
    Pending,
    Denied,
    Expired,
    Provisioning,
    Ready,
    Interrupted,
}

impl ScopeRefreshProgress {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Provisioning => "provisioning",
            Self::Ready => "ready",
            Self::Interrupted => "interrupted",
        }
    }
    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        match value {
            "pending" => Ok(Self::Pending),
            "denied" => Ok(Self::Denied),
            "expired" => Ok(Self::Expired),
            "provisioning" => Ok(Self::Provisioning),
            "ready" => Ok(Self::Ready),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ScopeRefreshReceipt {
    refresh_id: [u8; 32],
    expires_at_utc_ms: i64,
    state: ScopeRefreshProgress,
}

impl std::fmt::Debug for ScopeRefreshReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopeRefreshReceipt")
            .field("refresh_id", &"<redacted>")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl ScopeRefreshReceipt {
    pub fn new(
        refresh_id: [u8; 32],
        expires_at_utc_ms: i64,
        state: ScopeRefreshProgress,
    ) -> Result<Self, ErrorCode> {
        if refresh_id == [0; 32] || expires_at_utc_ms <= 0 {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            refresh_id,
            expires_at_utc_ms,
            state,
        })
    }
    pub fn refresh_id(&self) -> &[u8; 32] {
        &self.refresh_id
    }
    pub fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }
    pub fn state(&self) -> ScopeRefreshProgress {
        self.state
    }
}

/// Explicit, canonical bounded selection. Never interprets empty as all accounts.
#[derive(Clone, PartialEq, Eq)]
pub struct GoogleSendAccounts {
    ids: Vec<[u8; 32]>,
}
impl std::fmt::Debug for GoogleSendAccounts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GoogleSendAccounts(<redacted>)")
    }
}
impl GoogleSendAccounts {
    pub fn parse(values: &[String]) -> Result<Self, ErrorCode> {
        if values.is_empty() || values.len() > 128 {
            return Err(ErrorCode::InvalidInput);
        }
        let mut values = values.to_vec();
        values.sort();
        if values.windows(2).any(|v| v[0] == v[1]) {
            return Err(ErrorCode::InvalidInput);
        }
        let mut ids = Vec::with_capacity(values.len());
        for value in &values {
            crate::google::validate_google_account_id(value)?;
            ids.push(crate::wire_hex::decode_fixed(&value[7..])?);
        }
        Ok(Self { ids })
    }
    pub fn accounts(&self) -> Vec<String> {
        self.ids
            .iter()
            .map(|v| format!("google_{}", crate::wire_hex::encode(v)))
            .collect()
    }
}
#[cfg(test)]
mod google_send_selection_tests {
    use super::*;
    #[test]
    fn gmail_write_selected_scope_rejects_empty_duplicate_foreign_and_preserves_exact_set() {
        let a = format!("google_{}", "1".repeat(64));
        let b = format!("google_{}", "2".repeat(64));
        assert_eq!(
            GoogleSendAccounts::parse(&[b.clone(), a.clone()])
                .unwrap()
                .accounts(),
            vec![a.clone(), b]
        );
        for values in [vec![], vec![a.clone(), a], vec!["imap-account".into()]] {
            assert!(GoogleSendAccounts::parse(&values).is_err());
        }
    }
}

#[cfg(test)]
mod manage_selection_tests {
    use super::*;

    #[test]
    fn manage_selection_is_explicit_canonical_and_generic() {
        let values = vec!["smtp_account".into(), "imap_account".into()];
        let selected = ManageAccounts::parse(&values).unwrap();
        assert_eq!(selected.accounts(), vec!["imap_account", "smtp_account"]);
        assert!(!format!("{selected:?}").contains("imap_account"));
    }

    #[test]
    fn manage_selection_rejects_empty_duplicates_and_unbounded_total() {
        assert_eq!(ManageAccounts::parse(&[]), Err(ErrorCode::InvalidInput));
        let duplicate = vec!["imap_account".into(), "imap_account".into()];
        assert_eq!(
            ManageAccounts::parse(&duplicate),
            Err(ErrorCode::InvalidInput)
        );
        let oversized: Vec<String> = (0..97)
            .map(|index| format!("{}{:02}", "a".repeat(125), index))
            .collect();
        assert_eq!(
            ManageAccounts::parse(&oversized),
            Err(ErrorCode::InvalidInput)
        );
    }

    #[test]
    fn manage_selection_accepts_per_identifier_limit_but_rejects_invalid_ids() {
        let value = "a".repeat(128);
        assert_eq!(
            ManageAccounts::parse(std::slice::from_ref(&value))
                .unwrap()
                .accounts(),
            vec![value]
        );
        for values in [
            vec![String::new()],
            vec!["bad.id".into()],
            vec!["a".repeat(129)],
        ] {
            assert_eq!(ManageAccounts::parse(&values), Err(ErrorCode::InvalidInput));
        }
    }
}
