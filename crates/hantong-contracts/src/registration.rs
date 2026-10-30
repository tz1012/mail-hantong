//! Public registration progress data, never an approval or authentication proof.
use crate::ErrorCode;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RegistrationCommand {
    Request,
    Status([u8; 32]),
    GuiRequest,
    GuiStatus([u8; 32]),
}

impl std::fmt::Debug for RegistrationCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Request => "RegistrationCommand::Request",
            Self::Status(_) => "RegistrationCommand::Status(<redacted>)",
            Self::GuiRequest => "RegistrationCommand::GuiRequest",
            Self::GuiStatus(_) => "RegistrationCommand::GuiStatus(<redacted>)",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistrationProgress {
    Pending,
    Denied,
    Expired,
    Provisioning,
    Ready,
}

impl RegistrationProgress {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Provisioning => "provisioning",
            Self::Ready => "ready",
        }
    }
    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        match value {
            "pending" => Ok(Self::Pending),
            "denied" => Ok(Self::Denied),
            "expired" => Ok(Self::Expired),
            "provisioning" => Ok(Self::Provisioning),
            "ready" => Ok(Self::Ready),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RegistrationReceipt {
    request_id: [u8; 32],
    expires_at_utc_ms: i64,
    state: RegistrationProgress,
}

impl std::fmt::Debug for RegistrationReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistrationReceipt")
            .field("request_id", &"<redacted>")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl RegistrationReceipt {
    pub fn new(
        request_id: [u8; 32],
        expires_at_utc_ms: i64,
        state: RegistrationProgress,
    ) -> Result<Self, ErrorCode> {
        if request_id == [0; 32] || expires_at_utc_ms <= 0 {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            request_id,
            expires_at_utc_ms,
            state,
        })
    }
    pub fn request_id(&self) -> &[u8; 32] {
        &self.request_id
    }
    pub fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }
    pub fn state(&self) -> RegistrationProgress {
        self.state
    }
}
