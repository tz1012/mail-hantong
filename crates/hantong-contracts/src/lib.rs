#![forbid(unsafe_code)]

//! Stable identifiers shared by the CLI and the core.

pub mod audit;
pub mod auth;
pub mod client;
pub mod folders;
pub mod imap_setup;
pub mod message_search;
pub mod platform;
pub mod registration;
pub mod uid_inspection;
pub mod uid_page;
pub mod uid_reconcile;
pub mod uid_recovery;
pub mod wire_hex;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCode {
    InvalidInput,
    AuthRequired,
    ScopeDenied,
    PolicyBlocked,
    Conflict,
    RateLimited,
    Offline,
    NotFound,
    SendUnknown,
    ApprovalRequired,
    ApprovalExpired,
    ApprovalDenied,
    ApprovalRateLimited,
    ClientNotRegistered,
    ClientSuspended,
    HumanStepRequired,
    CoreUnavailable,
    VersionMismatch,
    CursorExpired,
    CapabilityUnsupported,
    QuotaExceeded,
    InternalError,
}

impl ErrorCode {
    pub const ALL: [Self; 22] = [
        Self::InvalidInput,
        Self::AuthRequired,
        Self::ScopeDenied,
        Self::PolicyBlocked,
        Self::Conflict,
        Self::RateLimited,
        Self::Offline,
        Self::NotFound,
        Self::SendUnknown,
        Self::ApprovalRequired,
        Self::ApprovalExpired,
        Self::ApprovalDenied,
        Self::ApprovalRateLimited,
        Self::ClientNotRegistered,
        Self::ClientSuspended,
        Self::HumanStepRequired,
        Self::CoreUnavailable,
        Self::VersionMismatch,
        Self::CursorExpired,
        Self::CapabilityUnsupported,
        Self::QuotaExceeded,
        Self::InternalError,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "INVALID_INPUT",
            Self::AuthRequired => "AUTH_REQUIRED",
            Self::ScopeDenied => "SCOPE_DENIED",
            Self::PolicyBlocked => "POLICY_BLOCKED",
            Self::Conflict => "CONFLICT",
            Self::RateLimited => "RATE_LIMITED",
            Self::Offline => "OFFLINE",
            Self::NotFound => "NOT_FOUND",
            Self::SendUnknown => "SEND_UNKNOWN",
            Self::ApprovalRequired => "APPROVAL_REQUIRED",
            Self::ApprovalExpired => "APPROVAL_EXPIRED",
            Self::ApprovalDenied => "APPROVAL_DENIED",
            Self::ApprovalRateLimited => "APPROVAL_RATE_LIMITED",
            Self::ClientNotRegistered => "CLIENT_NOT_REGISTERED",
            Self::ClientSuspended => "CLIENT_SUSPENDED",
            Self::HumanStepRequired => "HUMAN_STEP_REQUIRED",
            Self::CoreUnavailable => "CORE_UNAVAILABLE",
            Self::VersionMismatch => "VERSION_MISMATCH",
            Self::CursorExpired => "CURSOR_EXPIRED",
            Self::CapabilityUnsupported => "CAPABILITY_UNSUPPORTED",
            Self::QuotaExceeded => "QUOTA_EXCEEDED",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }

    pub const fn exit_code(self) -> i32 {
        match self {
            Self::InvalidInput => 2,
            Self::AuthRequired => 10,
            Self::ScopeDenied => 11,
            Self::PolicyBlocked => 12,
            Self::Conflict => 13,
            Self::RateLimited => 14,
            Self::Offline => 15,
            Self::NotFound => 16,
            Self::SendUnknown => 17,
            Self::ApprovalRequired => 18,
            Self::ApprovalExpired => 19,
            Self::ApprovalDenied => 20,
            Self::ApprovalRateLimited => 21,
            Self::ClientNotRegistered => 22,
            Self::ClientSuspended => 23,
            Self::HumanStepRequired => 24,
            Self::CoreUnavailable => 25,
            Self::VersionMismatch => 26,
            Self::CursorExpired => 27,
            Self::CapabilityUnsupported => 28,
            Self::QuotaExceeded => 29,
            Self::InternalError => 70,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ErrorCode;

    #[test]
    fn cli03_codes_and_exit_values_are_stable_and_unique() {
        let expected = [
            ("INVALID_INPUT", 2),
            ("AUTH_REQUIRED", 10),
            ("SCOPE_DENIED", 11),
            ("POLICY_BLOCKED", 12),
            ("CONFLICT", 13),
            ("RATE_LIMITED", 14),
            ("OFFLINE", 15),
            ("NOT_FOUND", 16),
            ("SEND_UNKNOWN", 17),
            ("APPROVAL_REQUIRED", 18),
            ("APPROVAL_EXPIRED", 19),
            ("APPROVAL_DENIED", 20),
            ("APPROVAL_RATE_LIMITED", 21),
            ("CLIENT_NOT_REGISTERED", 22),
            ("CLIENT_SUSPENDED", 23),
            ("HUMAN_STEP_REQUIRED", 24),
            ("CORE_UNAVAILABLE", 25),
            ("VERSION_MISMATCH", 26),
            ("CURSOR_EXPIRED", 27),
            ("CAPABILITY_UNSUPPORTED", 28),
            ("QUOTA_EXCEEDED", 29),
            ("INTERNAL_ERROR", 70),
        ];
        assert_eq!(ErrorCode::ALL.len(), expected.len());
        for (code, (name, exit)) in ErrorCode::ALL.iter().zip(expected) {
            assert_eq!(code.as_str(), name);
            assert_eq!(code.exit_code(), exit);
        }
    }
}

pub mod scope_refresh;
