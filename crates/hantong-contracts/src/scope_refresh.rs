//! Public scope-refresh progress data, never an approval or authentication proof.
use crate::ErrorCode;

#[derive(Clone, PartialEq, Eq)]
pub enum ScopeRefreshCommand {
    Request,
    RequestGoogleSend(GoogleSendAccounts),
    RequestGoogleChanges(GoogleSendAccounts),
    RequestGoogleReadOnly(crate::registration::GoogleReadAccount),
    AddGoogleReadOnly(crate::registration::GoogleReadAccount),
    Status([u8; 32]),
}

impl std::fmt::Debug for ScopeRefreshCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
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
