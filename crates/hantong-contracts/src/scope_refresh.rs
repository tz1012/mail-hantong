//! Public scope-refresh progress data, never an approval or authentication proof.
use crate::ErrorCode;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScopeRefreshCommand {
    Request,
    Status([u8; 32]),
}

impl std::fmt::Debug for ScopeRefreshCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Request => "ScopeRefreshCommand::Request",
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
