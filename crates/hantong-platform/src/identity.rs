//! Cross-platform shape for the platform signing primitive.

use hantong_contracts::ErrorCode;

/// A persisted client signing key is available only through Windows CNG.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum IdentityCapability {
    Unsupported,
}

pub fn unsupported() -> Result<(), ErrorCode> {
    Err(ErrorCode::CapabilityUnsupported)
}
