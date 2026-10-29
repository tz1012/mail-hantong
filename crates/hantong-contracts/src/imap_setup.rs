//! Strict, non-secret bootstrap messages for new IMAP setup.
use crate::ErrorCode;
use std::fmt;

const MAX_NAME: usize = 128;
const MAX_HOST: usize = 253;
const MAX_LOGIN: usize = 320;

#[derive(Clone, Eq, PartialEq)]
pub struct ImapSetupInput {
    display_name: Option<String>,
    color: Option<String>,
    host: String,
    port: u16,
    tls: ImapSetupTls,
    login_name: String,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapSetupTls {
    ImplicitTls,
    StarttlsRequired,
}
#[derive(Clone, Eq, PartialEq)]
pub enum ImapSetupCommand {
    Request(ImapSetupInput),
    Status([u8; 32]),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapSetupState {
    Pending,
    Connecting,
    Ready,
    AuthRequired,
    TlsFailed,
    Offline,
    Error,
    Denied,
    Cancelled,
    Expired,
    Interrupted,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapSetupNextAction {
    HelperRequired,
    Wait,
    ClientRegistrationRequired,
    ProfileScopeReviewRequired,
    ResumeRequired,
    NewRequestRequired,
}
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ImapSetupReceipt {
    request_id: [u8; 32],
    expires_at_utc_ms: i64,
    state: ImapSetupState,
    next_action: ImapSetupNextAction,
}

fn text(value: &str, max: usize, allow_space: bool) -> bool {
    !value.is_empty()
        && value.chars().count() <= max
        && value.len() <= max * 4
        && (allow_space || !value.chars().all(char::is_whitespace))
        && !value.chars().any(char::is_control)
}
impl ImapSetupInput {
    pub fn new(
        display_name: Option<String>,
        color: Option<String>,
        host: String,
        port: u16,
        tls: ImapSetupTls,
        login_name: String,
    ) -> Result<Self, ErrorCode> {
        if display_name
            .as_deref()
            .is_some_and(|v| !text(v, MAX_NAME, false) || v.len() > 512)
            || color.as_deref().is_some_and(|v| !is_color(v))
            || !text(&host, MAX_HOST, false)
            || host.bytes().any(|b| !b.is_ascii())
            || host.chars().any(char::is_whitespace)
            || port == 0
            || !text(&login_name, MAX_LOGIN, false)
            || login_name.len() > 1280
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            display_name,
            color,
            host,
            port,
            tls,
            login_name,
        })
    }
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }
    pub fn color(&self) -> Option<&str> {
        self.color.as_deref()
    }
    pub fn host(&self) -> &str {
        &self.host
    }
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn tls(&self) -> ImapSetupTls {
        self.tls
    }
    pub fn login_name(&self) -> &str {
        &self.login_name
    }
}
fn is_color(v: &str) -> bool {
    v.len() == 7 && v.as_bytes()[0] == b'#' && v[1..].bytes().all(|b| b.is_ascii_hexdigit())
}
impl ImapSetupState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Connecting => "connecting",
            Self::Ready => "ready",
            Self::AuthRequired => "auth_required",
            Self::TlsFailed => "tls_failed",
            Self::Offline => "offline",
            Self::Error => "error",
            Self::Denied => "denied",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
            Self::Interrupted => "interrupted",
        }
    }
    pub fn parse(v: &str) -> Result<Self, ErrorCode> {
        Self::ALL
            .into_iter()
            .find(|x| x.as_str() == v)
            .ok_or(ErrorCode::InvalidInput)
    }
    pub const ALL: [Self; 11] = [
        Self::Pending,
        Self::Connecting,
        Self::Ready,
        Self::AuthRequired,
        Self::TlsFailed,
        Self::Offline,
        Self::Error,
        Self::Denied,
        Self::Cancelled,
        Self::Expired,
        Self::Interrupted,
    ];
}
impl ImapSetupNextAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HelperRequired => "helper_required",
            Self::Wait => "wait",
            Self::ClientRegistrationRequired => "client_registration_required",
            Self::ProfileScopeReviewRequired => "profile_scope_review_required",
            Self::ResumeRequired => "resume_required",
            Self::NewRequestRequired => "new_request_required",
        }
    }
    pub fn parse(v: &str) -> Result<Self, ErrorCode> {
        Self::ALL
            .into_iter()
            .find(|x| x.as_str() == v)
            .ok_or(ErrorCode::InvalidInput)
    }
    pub const ALL: [Self; 6] = [
        Self::HelperRequired,
        Self::Wait,
        Self::ClientRegistrationRequired,
        Self::ProfileScopeReviewRequired,
        Self::ResumeRequired,
        Self::NewRequestRequired,
    ];
}
impl ImapSetupReceipt {
    pub fn new(
        request_id: [u8; 32],
        expires_at_utc_ms: i64,
        state: ImapSetupState,
        next_action: ImapSetupNextAction,
    ) -> Result<Self, ErrorCode> {
        if request_id == [0; 32] || expires_at_utc_ms <= 0 || !valid_action(state, next_action) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            request_id,
            expires_at_utc_ms,
            state,
            next_action,
        })
    }
    pub fn request_id(&self) -> &[u8; 32] {
        &self.request_id
    }
    pub fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }
    pub fn state(&self) -> ImapSetupState {
        self.state
    }
    pub fn next_action(&self) -> ImapSetupNextAction {
        self.next_action
    }
}
fn valid_action(s: ImapSetupState, a: ImapSetupNextAction) -> bool {
    match s {
        ImapSetupState::Pending => a == ImapSetupNextAction::HelperRequired,
        ImapSetupState::Connecting => a == ImapSetupNextAction::Wait,
        ImapSetupState::Ready => matches!(
            a,
            ImapSetupNextAction::ClientRegistrationRequired
                | ImapSetupNextAction::ProfileScopeReviewRequired
        ),
        ImapSetupState::AuthRequired
        | ImapSetupState::TlsFailed
        | ImapSetupState::Offline
        | ImapSetupState::Interrupted => a == ImapSetupNextAction::ResumeRequired,
        ImapSetupState::Error => matches!(
            a,
            ImapSetupNextAction::ResumeRequired | ImapSetupNextAction::NewRequestRequired
        ),
        ImapSetupState::Denied | ImapSetupState::Cancelled | ImapSetupState::Expired => {
            a == ImapSetupNextAction::NewRequestRequired
        }
    }
}
impl fmt::Debug for ImapSetupInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapSetupInput { <redacted> }")
    }
}
impl fmt::Debug for ImapSetupCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapSetupCommand { <redacted> }")
    }
}
impl fmt::Debug for ImapSetupReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImapSetupReceipt { <redacted> }")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(host: &str, login: &str) -> Result<ImapSetupInput, ErrorCode> {
        ImapSetupInput::new(
            Some("Display".into()),
            Some("#aBc123".into()),
            host.into(),
            993,
            ImapSetupTls::ImplicitTls,
            login.into(),
        )
    }

    #[test]
    fn setup_input_enforces_exact_byte_and_character_bounds() {
        assert!(input("imap.example", "a b").is_ok());
        assert!(input("imap example", "user").is_err());
        assert!(input(&"a".repeat(254), "user").is_err());
        assert!(input("imap.example", &"a".repeat(321)).is_err());
        assert!(input("imap.example", "   ").is_err());
        assert!(input("imap.example", "u\u{0000}ser").is_err());
        assert!(input("imap.example", "사용자").is_ok());
    }

    #[test]
    fn setup_receipt_allows_the_documented_error_actions_only() {
        for action in [
            ImapSetupNextAction::ResumeRequired,
            ImapSetupNextAction::NewRequestRequired,
        ] {
            assert!(ImapSetupReceipt::new([1; 32], 1, ImapSetupState::Error, action).is_ok());
        }
        for state in ImapSetupState::ALL {
            let allowed = match state {
                ImapSetupState::Pending => &[ImapSetupNextAction::HelperRequired][..],
                ImapSetupState::Connecting => &[ImapSetupNextAction::Wait][..],
                ImapSetupState::Ready => &[
                    ImapSetupNextAction::ClientRegistrationRequired,
                    ImapSetupNextAction::ProfileScopeReviewRequired,
                ][..],
                ImapSetupState::AuthRequired
                | ImapSetupState::TlsFailed
                | ImapSetupState::Offline
                | ImapSetupState::Interrupted => &[ImapSetupNextAction::ResumeRequired][..],
                ImapSetupState::Error => &[
                    ImapSetupNextAction::ResumeRequired,
                    ImapSetupNextAction::NewRequestRequired,
                ][..],
                ImapSetupState::Denied | ImapSetupState::Cancelled | ImapSetupState::Expired => {
                    &[ImapSetupNextAction::NewRequestRequired][..]
                }
            };
            for action in ImapSetupNextAction::ALL {
                let result = ImapSetupReceipt::new([1; 32], 1, state, action);
                assert_eq!(result.is_ok(), allowed.contains(&action));
            }
        }
    }
}
