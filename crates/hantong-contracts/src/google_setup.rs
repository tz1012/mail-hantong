//! Public bootstrap input/status only; never contains authorization evidence.
use crate::{
    ErrorCode,
    google::{validate_google_account_id, validate_google_client_id},
};
#[derive(Clone, Eq, PartialEq)]
pub struct GoogleSetupInput {
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
}
#[derive(Clone, Eq, PartialEq)]
pub struct GoogleSetupReceipt {
    pub request_id: [u8; 32],
    pub expires_at_utc_ms: i64,
    pub state: String,
    pub account_id: Option<String>,
    pub login_url: Option<String>,
    pub error_code: Option<ErrorCode>,
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
            "queued" | "awaiting_browser" | "awaiting_review" => self.error_code.is_none(),
            "ready" => {
                self.error_code.is_none() && self.account_id.is_some() && self.login_url.is_none()
            }
            "denied" => self.error_code == Some(ErrorCode::ApprovalDenied),
            "expired" => self.error_code == Some(ErrorCode::ApprovalExpired),
            "failed" | "interrupted" => self.error_code.is_some(),
            _ => false,
        };
        if !valid
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
        serde_json::json!({"protocol_version":1,"request_id":crate::wire_hex::encode(&self.request_id),"expires_at_utc_ms":self.expires_at_utc_ms,"state":self.state,"account_id":self.account_id,"login_url":self.login_url,"error_code":self.error_code.map(ErrorCode::as_str)})
    }
}
