//! Public bootstrap input/status only; never contains authorization evidence.
use crate::{
    ErrorCode,
    google::{validate_google_account_id, validate_google_client_id},
};
/// Finite personal CLI feature selection. This value is not authorization proof.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GoogleLocalProfile {
    #[default]
    None,
    PersonalRead,
    PersonalReadSend,
}
impl GoogleLocalProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::PersonalRead => "personal_read",
            Self::PersonalReadSend => "personal_read_send",
        }
    }
    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        match value {
            "none" => Ok(Self::None),
            "personal_read" => Ok(Self::PersonalRead),
            "personal_read_send" => Ok(Self::PersonalReadSend),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
    pub fn access_mode(self) -> Option<crate::google_write::GoogleAccessMode> {
        match self {
            Self::None => None,
            Self::PersonalRead => Some(crate::google_write::GoogleAccessMode::ReadOnly),
            Self::PersonalReadSend => Some(crate::google_write::GoogleAccessMode::ReadSend),
        }
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct GoogleSetupInput {
    pub access_mode: crate::google_write::GoogleAccessMode,
    pub local_profile: GoogleLocalProfile,
    pub client_id: Option<String>,
    pub display_name: Option<String>,
    pub reauthorize_account: Option<String>,
    pub force_consent: bool,
}
impl std::fmt::Debug for GoogleSetupInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GoogleSetupInput(<redacted>)")
    }
}
impl GoogleSetupInput {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        if let Some(mode) = self.local_profile.access_mode()
            && (self.access_mode != mode
                || (self.reauthorize_account.is_some()
                    && self.local_profile != GoogleLocalProfile::PersonalReadSend))
        {
            return Err(ErrorCode::InvalidInput);
        }
        if let Some(s) = &self.client_id {
            validate_google_client_id(s)?
        }
        if let Some(s) = &self.reauthorize_account {
            validate_google_account_id(s)?
        }
        if self.display_name.as_ref().is_some_and(|s| {
            s.is_empty()
                || s.len() > 512
                || s.chars().count() > 128
                || s.chars().any(char::is_control)
        }) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GoogleSetupCommand {
    Request(GoogleSetupInput),
    Status([u8; 32]),
    ClientTicket([u8; 32]),
    StageClient {
        request_id: [u8; 32],
        ticket: Vec<u8>,
        staged_handle: [u8; 32],
    },
}
#[derive(Clone, Eq, PartialEq)]
pub struct GoogleSetupReceipt {
    pub request_id: [u8; 32],
    pub expires_at_utc_ms: i64,
    pub state: String,
    pub account_id: Option<String>,
    pub login_url: Option<String>,
    pub error_code: Option<ErrorCode>,
    pub client_ticket: Option<Vec<u8>>,
    /// Session routing metadata only; never authorization evidence.
    pub local_profile: GoogleLocalProfile,
}
impl std::fmt::Debug for GoogleSetupReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GoogleSetupReceipt(<redacted>)")
    }
}
impl GoogleSetupReceipt {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        if self.request_id == [0; 32] || self.expires_at_utc_ms <= 0 {
            return Err(ErrorCode::InvalidInput);
        }
        if let Some(id) = &self.account_id {
            validate_google_account_id(id)?
        }
        let valid = match self.state.as_str() {
            "queued" | "awaiting_client" | "awaiting_browser" | "awaiting_review" => {
                self.error_code.is_none()
            }
            "ready" => {
                self.error_code.is_none() && self.account_id.is_some() && self.login_url.is_none()
            }
            "denied" => self.error_code == Some(ErrorCode::ApprovalDenied),
            "expired" => self.error_code == Some(ErrorCode::ApprovalExpired),
            "failed" | "interrupted" => self.error_code.is_some(),
            _ => false,
        };
        if self
            .client_ticket
            .as_ref()
            .is_some_and(|v| v.len() != 32 && v.len() != 320)
            || !valid
            || self.login_url.as_ref().is_some_and(|u| {
                u.len() > 8192
                    || !u.starts_with("https://accounts.google.com/o/oauth2/v2/auth?")
                    || u.chars().any(char::is_control)
                    || self.state != "awaiting_browser"
            })
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(())
    }
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({"protocol_version":1,"request_id":crate::wire_hex::encode(&self.request_id),"expires_at_utc_ms":self.expires_at_utc_ms,"state":self.state,"account_id":self.account_id,"login_url":self.login_url,"error_code":self.error_code.map(ErrorCode::as_str),"client_ticket":self.client_ticket.as_ref().map(|v| crate::wire_hex::encode(v)),"local_profile":self.local_profile.as_str()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::google_write::GoogleAccessMode;

    #[test]
    fn acc02_personal_features_require_exact_mode_without_reauthorization() {
        for (profile, mode) in [
            (GoogleLocalProfile::PersonalRead, GoogleAccessMode::ReadOnly),
            (
                GoogleLocalProfile::PersonalReadSend,
                GoogleAccessMode::ReadSend,
            ),
        ] {
            let mut input = GoogleSetupInput {
                local_profile: profile,
                access_mode: mode,
                client_id: None,
                display_name: None,
                reauthorize_account: None,
                force_consent: false,
            };
            assert_eq!(input.validate(), Ok(()));
            input.access_mode = GoogleAccessMode::Modify;
            assert_eq!(input.validate(), Err(ErrorCode::InvalidInput));
            input.access_mode = mode;
            input.reauthorize_account = Some(format!("google_{}", "a".repeat(64)));
            assert_eq!(
                input.validate(),
                if profile == GoogleLocalProfile::PersonalReadSend {
                    Ok(())
                } else {
                    Err(ErrorCode::InvalidInput)
                }
            );
        }
        for invalid in ["all", "personal_modify", "PERSONAL_READ", "approved", ""] {
            assert_eq!(
                GoogleLocalProfile::parse(invalid),
                Err(ErrorCode::InvalidInput)
            );
        }
    }
}
