//! Non-secret human SMTP setup bootstrap. No approval or credential input.
use crate::{
    ErrorCode,
    imap_setup::{ImapSetupInput, ImapSetupTls},
};
#[derive(Clone, Eq, PartialEq)]
pub struct SmtpSetupInput {
    account: String,
    endpoint: ImapSetupInput,
    sender_address: Option<String>,
}
impl std::fmt::Debug for SmtpSetupInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SmtpSetupInput(<redacted>)")
    }
}
impl SmtpSetupInput {
    pub fn new(
        account: String,
        host: String,
        port: u16,
        tls: ImapSetupTls,
        login: String,
    ) -> Result<Self, ErrorCode> {
        if account.is_empty()
            || account.len() > 128
            || !account
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(ErrorCode::InvalidInput);
        }
        Self::new_with_sender(account, host, port, tls, login.clone(), None)
    }
    pub fn new_with_sender(
        account: String,
        host: String,
        port: u16,
        tls: ImapSetupTls,
        login: String,
        sender_address: Option<String>,
    ) -> Result<Self, ErrorCode> {
        if account.is_empty()
            || account.len() > 128
            || !account
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(ErrorCode::InvalidInput);
        }
        let from = sender_address.clone().unwrap_or_else(|| login.clone());
        crate::mail_send::DraftCreateRequest::new(
            &account,
            vec![from],
            String::new(),
            String::new(),
            "smtp-input",
            "smtp-input",
            0,
            [0; 32],
        )?;
        Ok(Self {
            account,
            endpoint: ImapSetupInput::new(None, None, host, port, tls, login)?,
            sender_address,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account
    }
    pub fn host(&self) -> &str {
        self.endpoint.host()
    }
    pub fn port(&self) -> u16 {
        self.endpoint.port()
    }
    pub fn tls(&self) -> ImapSetupTls {
        self.endpoint.tls()
    }
    pub fn login_name(&self) -> &str {
        self.endpoint.login_name()
    }
    pub fn sender_address(&self) -> Option<&str> {
        self.sender_address.as_deref()
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SmtpSetupCommand {
    Request(SmtpSetupInput),
    Status([u8; 32]),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmtpSetupState {
    Pending,
    Ready,
    Denied,
    Expired,
    Failed,
    Interrupted,
}
impl SmtpSetupState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
    pub fn parse(s: &str) -> Result<Self, ErrorCode> {
        [
            Self::Pending,
            Self::Ready,
            Self::Denied,
            Self::Expired,
            Self::Failed,
            Self::Interrupted,
        ]
        .into_iter()
        .find(|v| v.as_str() == s)
        .ok_or(ErrorCode::InvalidInput)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmtpSetupReceipt {
    pub request_id: [u8; 32],
    pub expires_at_utc_ms: i64,
    pub state: SmtpSetupState,
    pub error_code: Option<ErrorCode>,
}
impl SmtpSetupReceipt {
    pub fn validate(&self) -> Result<(), ErrorCode> {
        if self.request_id == [0; 32]
            || self.expires_at_utc_ms <= 0
            || !match self.state {
                SmtpSetupState::Pending | SmtpSetupState::Ready => self.error_code.is_none(),
                SmtpSetupState::Denied => self.error_code == Some(ErrorCode::ApprovalDenied),
                SmtpSetupState::Expired => self.error_code == Some(ErrorCode::ApprovalExpired),
                SmtpSetupState::Interrupted => self.error_code == Some(ErrorCode::ApprovalRequired),
                SmtpSetupState::Failed => self.error_code.is_some(),
            }
        {
            Err(ErrorCode::InvalidInput)
        } else {
            Ok(())
        }
    }
}
