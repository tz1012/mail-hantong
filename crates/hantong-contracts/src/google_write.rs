//! Bounded, untrusted write inputs. None of these types conveys approval.
use crate::{ErrorCode, google::validate_google_account_id, mail_send::DraftCreateRequest};
use std::{collections::BTreeSet, fmt};

pub const GOOGLE_SEND_SCOPE: &str = "https://www.googleapis.com/auth/gmail.send";
pub const GOOGLE_MODIFY_SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GoogleAccessMode {
    #[default]
    ReadOnly,
    ReadSend,
    Modify,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoogleOperation {
    Read,
    Send,
    SaveDraft,
    ChangeLabels,
    TrashMessage,
}

impl GoogleAccessMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "readonly",
            Self::ReadSend => "read-send",
            Self::Modify => "modify",
        }
    }
    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        match value {
            "readonly" => Ok(Self::ReadOnly),
            "read-send" => Ok(Self::ReadSend),
            "modify" => Ok(Self::Modify),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
    pub fn required_scopes(self) -> &'static [&'static str] {
        match self {
            Self::ReadOnly => &crate::google::GOOGLE_SCOPES,
            Self::ReadSend => &[
                "openid",
                "email",
                crate::google::GOOGLE_READ_SCOPE,
                GOOGLE_SEND_SCOPE,
            ],
            Self::Modify => &["openid", "email", GOOGLE_MODIFY_SCOPE],
        }
    }
    /// Decode only a locally stored canonical request, never provider scopes.
    pub fn from_requested_scopes(scopes: &[String]) -> Result<Self, ErrorCode> {
        [Self::ReadOnly, Self::ReadSend, Self::Modify]
            .into_iter()
            .find(|mode| {
                scopes.len() == mode.required_scopes().len()
                    && mode
                        .required_scopes()
                        .iter()
                        .all(|s| scopes.iter().filter(|v| *v == s).count() == 1)
            })
            .ok_or(ErrorCode::ScopeDenied)
    }
    /// Checks this explicitly selected mode; never derives a stronger mode.
    pub fn validate_granted(self, granted: &[String]) -> Result<(), ErrorCode> {
        if granted.len() > 32
            || granted
                .iter()
                .any(|s| s.is_empty() || s.len() > 256 || !s.bytes().all(|b| b.is_ascii_graphic()))
        {
            return Err(ErrorCode::ScopeDenied);
        }
        if self.required_scopes().iter().all(|required| {
            granted.iter().any(|g| {
                g == required
                    || (*required == "email"
                        && g == "https://www.googleapis.com/auth/userinfo.email")
            })
        }) {
            Ok(())
        } else {
            Err(ErrorCode::ScopeDenied)
        }
    }
    pub fn check_operation(self, operation: GoogleOperation) -> Result<(), ErrorCode> {
        let permitted = match operation {
            GoogleOperation::Read => true,
            GoogleOperation::Send => self != Self::ReadOnly,
            GoogleOperation::SaveDraft
            | GoogleOperation::ChangeLabels
            | GoogleOperation::TrashMessage => self == Self::Modify,
        };
        if permitted {
            Ok(())
        } else {
            Err(ErrorCode::ScopeDenied)
        }
    }
}

pub const MAX_ATTACHMENT_BYTES: usize = 1024 * 1024;
pub const MAX_ATTACHMENT_COUNT: usize = 8;

#[derive(Clone, Eq, PartialEq)]
pub struct AttachmentSnapshot {
    name: String,
    media_type: String,
    bytes: Vec<u8>,
}
impl fmt::Debug for AttachmentSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AttachmentSnapshot(<redacted>)")
    }
}
impl AttachmentSnapshot {
    pub fn new(name: String, media_type: String, bytes: Vec<u8>) -> Result<Self, ErrorCode> {
        if name.is_empty()
            || name.len() > 120
            || name.starts_with('.')
            || name.contains("..")
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || !matches!(
                media_type.as_str(),
                "text/plain"
                    | "application/octet-stream"
                    | "application/pdf"
                    | "image/png"
                    | "image/jpeg"
            )
            || bytes.len() > MAX_ATTACHMENT_BYTES
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            name,
            media_type,
            bytes,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn media_type(&self) -> &str {
        &self.media_type
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Immutable byte snapshots for later encrypted persistence and native review.
/// Construction does not create a stored draft, approval, or send operation.
#[derive(Clone, Eq, PartialEq)]
pub struct ComposeSnapshot {
    draft: DraftCreateRequest,
    attachments: Vec<AttachmentSnapshot>,
}
impl fmt::Debug for ComposeSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ComposeSnapshot(<redacted>)")
    }
}
impl ComposeSnapshot {
    pub fn new(
        draft: DraftCreateRequest,
        attachments: Vec<AttachmentSnapshot>,
    ) -> Result<Self, ErrorCode> {
        let mut names = BTreeSet::new();
        if attachments.len() > MAX_ATTACHMENT_COUNT
            || attachments.iter().map(|a| a.bytes.len()).sum::<usize>() > MAX_ATTACHMENT_BYTES
            || attachments
                .iter()
                .any(|a| !names.insert(a.name.to_ascii_lowercase()))
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self { draft, attachments })
    }
    pub fn draft(&self) -> &DraftCreateRequest {
        &self.draft
    }
    pub fn attachments(&self) -> &[AttachmentSnapshot] {
        &self.attachments
    }
    pub fn canonical_payload(&self) -> Vec<u8> {
        if self.attachments.is_empty() {
            return self.draft.canonical_payload();
        }
        let mut out = b"mail-hantong.compose.attachments.v1\0".to_vec();
        fn field(out: &mut Vec<u8>, value: &[u8]) {
            out.extend_from_slice(&(value.len() as u64).to_be_bytes());
            out.extend_from_slice(value);
        }
        field(&mut out, &self.draft.canonical_payload());
        out.extend_from_slice(&(self.attachments.len() as u64).to_be_bytes());
        for attachment in &self.attachments {
            field(&mut out, attachment.name.as_bytes());
            field(&mut out, attachment.media_type.as_bytes());
            field(&mut out, &attachment.bytes);
        }
        out
    }
}
impl From<DraftCreateRequest> for ComposeSnapshot {
    fn from(draft: DraftCreateRequest) -> Self {
        Self {
            draft,
            attachments: vec![],
        }
    }
}
impl std::ops::Deref for ComposeSnapshot {
    type Target = DraftCreateRequest;
    fn deref(&self) -> &Self::Target {
        &self.draft
    }
}

/// Provider-bound review data; the SMTP type remains owned by the domain layer.
#[derive(Clone, Eq, PartialEq)]
pub enum MailSendTarget<Smtp> {
    Smtp(Smtp),
    Gmail {
        binding: crate::google::GoogleAccountBinding,
        sender: String,
    },
}
impl<Smtp> From<Smtp> for MailSendTarget<Smtp> {
    fn from(value: Smtp) -> Self {
        Self::Smtp(value)
    }
}
impl<Smtp> fmt::Debug for MailSendTarget<Smtp> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MailSendTarget(<redacted>)")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct MessageLabelChange {
    account_id: String,
    message_id: String,
    add: Vec<String>,
    remove: Vec<String>,
}
impl fmt::Debug for MessageLabelChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MessageLabelChange(<redacted>)")
    }
}
impl MessageLabelChange {
    pub fn new(
        account_id: String,
        message_id: String,
        mut add: Vec<String>,
        mut remove: Vec<String>,
    ) -> Result<Self, ErrorCode> {
        validate_google_account_id(&account_id)?;
        let provider_id = |s: &str| {
            !s.is_empty()
                && s.len() <= 256
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        };
        if !provider_id(&message_id) {
            return Err(ErrorCode::InvalidInput);
        }
        validate_label_delta(&add, &remove)?;
        add.sort_unstable();
        remove.sort_unstable();
        Ok(Self {
            account_id,
            message_id,
            add,
            remove,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn message_id(&self) -> &str {
        &self.message_id
    }
    pub fn add(&self) -> &[String] {
        &self.add
    }
    pub fn remove(&self) -> &[String] {
        &self.remove
    }
    pub fn canonical_payload(&self) -> Vec<u8> {
        let mut out = b"mail-hantong.gmail.labels.v1\0".to_vec();
        out.extend(
            serde_json::to_vec(&(&self.account_id, &self.message_id, &self.add, &self.remove))
                .expect("bounded strings"),
        );
        out
    }
}

pub fn validate_label_delta(add: &[String], remove: &[String]) -> Result<(), ErrorCode> {
    if add.len() > 100 || remove.len() > 100 || (add.is_empty() && remove.is_empty()) {
        return Err(ErrorCode::InvalidInput);
    }
    let mut seen = BTreeSet::new();
    for label in add.iter().chain(remove) {
        let valid = matches!(
            label.as_str(),
            "UNREAD"
                | "STARRED"
                | "IMPORTANT"
                | "INBOX"
                | "CATEGORY_PERSONAL"
                | "CATEGORY_SOCIAL"
                | "CATEGORY_PROMOTIONS"
                | "CATEGORY_UPDATES"
                | "CATEGORY_FORUMS"
        ) || (label.len() <= 256
            && label.strip_prefix("Label_").is_some_and(|suffix| {
                !suffix.is_empty()
                    && suffix
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            }));
        if !valid || !seen.insert(label) {
            return Err(ErrorCode::InvalidInput);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "google_write/tests.rs"]
mod tests;
