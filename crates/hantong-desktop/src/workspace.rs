//! Bounded presentation data. No registration or authentication authority lives here.
use crate::{MailAction, MailActionResult};
use hantong_contracts::{
    ErrorCode,
    folders::FolderCursor,
    message_search::{ImapMetadataFilter, ImapMetadataSearchCursor},
    uid_page::{UidMailboxEncoding, UidMailboxIdentity},
};
use std::collections::BTreeSet;

const MAX_TEXT: usize = 512;
fn safe_text(value: &str) -> String {
    let mut output = String::new();
    for c in value.chars() {
        let ignorable = matches!(c as u32, 0x00ad | 0x034f | 0x061c | 0x115f | 0x1160 | 0x17b4..=0x17b5 | 0x180b..=0x180f | 0x200b..=0x200f | 0x2028..=0x202e | 0x2060..=0x206f | 0xfe00..=0xfe0f | 0xfeff | 0xe0000..=0xe0fff);
        if c.is_control() || ignorable {
            continue;
        }
        if output.len() + c.len_utf8() > MAX_TEXT {
            while output.len() > MAX_TEXT - '…'.len_utf8() {
                output.pop();
            }
            output.push('…');
            break;
        }
        output.push(c);
    }
    output
}
fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailAccount {
    pub account_id: String,
    pub label: String,
}
impl MailAccount {
    pub fn new(account_id: &str, label: &str) -> Option<Self> {
        valid_identity(account_id).then(|| Self {
            account_id: account_id.into(),
            label: safe_text(label),
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailFolder {
    pub account_id: String,
    pub display_name: String,
    pub wire_hex: String,
    pub encoding: String,
}
impl MailFolder {
    pub fn new_with_encoding(
        account_id: &str,
        display_name: &str,
        wire_hex: &str,
        encoding: &str,
    ) -> Option<Self> {
        if !valid_identity(account_id) {
            return None;
        }
        UidMailboxIdentity::new(wire_hex, UidMailboxEncoding::parse(encoding).ok()?).ok()?;
        Some(Self {
            account_id: account_id.into(),
            display_name: safe_text(display_name),
            wire_hex: wire_hex.into(),
            encoding: encoding.into(),
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailMessage {
    pub account_id: String,
    pub internal_id: String,
    pub sender: String,
    pub subject: String,
    pub date_utc: String,
    pub is_read: bool,
    pub is_favorite: bool,
}
impl MailMessage {
    pub fn new(
        account_id: &str,
        internal_id: &str,
        sender: &str,
        subject: &str,
        date_utc: &str,
        is_read: bool,
        is_favorite: bool,
    ) -> Option<Self> {
        if !valid_identity(account_id)
            || internal_id.len() != 68
            || !internal_id.starts_with("msg_")
            || !internal_id[4..]
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return None;
        }
        Some(Self {
            account_id: account_id.into(),
            internal_id: internal_id.into(),
            sender: safe_text(sender),
            subject: safe_text(subject),
            date_utc: safe_text(date_utc),
            is_read,
            is_favorite,
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageCursor(String);
impl PageCursor {
    pub fn new(value: &str) -> Option<Self> {
        ImapMetadataSearchCursor::parse(value)
            .ok()
            .map(|_| Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailWorkspace {
    accounts: Vec<MailAccount>,
    folders: Vec<MailFolder>,
    messages: Vec<MailMessage>,
    next_cursor: Option<PageCursor>,
    folder_cursor: Option<String>,
    selected_account: Option<String>,
    selected_folder: Option<usize>,
    selected_message: Option<usize>,
    filter: ImapMetadataFilter,
}
impl Default for MailWorkspace {
    fn default() -> Self {
        Self {
            accounts: Vec::new(),
            folders: Vec::new(),
            messages: Vec::new(),
            next_cursor: None,
            folder_cursor: None,
            selected_account: None,
            selected_folder: None,
            selected_message: None,
            filter: ImapMetadataFilter::All,
        }
    }
}
impl MailWorkspace {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn accounts(&self) -> &[MailAccount] {
        &self.accounts
    }
    pub fn folders(&self) -> &[MailFolder] {
        &self.folders
    }
    pub fn messages(&self) -> &[MailMessage] {
        &self.messages
    }
    pub fn next_cursor(&self) -> Option<&PageCursor> {
        self.next_cursor.as_ref()
    }
    pub fn folder_cursor(&self) -> Option<&str> {
        self.folder_cursor.as_deref()
    }
    pub fn selected_account(&self) -> Option<&str> {
        self.selected_account.as_deref()
    }
    pub fn selected_folder(&self) -> Option<&MailFolder> {
        self.selected_folder.and_then(|i| self.folders.get(i))
    }
    pub fn selected_message(&self) -> Option<&MailMessage> {
        self.selected_message.and_then(|i| self.messages.get(i))
    }
    pub fn message_index(&self) -> Option<usize> {
        self.selected_message
    }
    pub fn folder_index(&self) -> Option<usize> {
        self.selected_folder
    }
    pub fn filter(&self) -> ImapMetadataFilter {
        self.filter
    }
    pub fn set_filter(&mut self, filter: ImapMetadataFilter) -> bool {
        if self.selected_folder().is_none() || self.filter == filter {
            return false;
        }
        self.filter = filter;
        self.clear_messages();
        true
    }
    pub fn restart_current_folder(&mut self) -> Option<MailAction> {
        self.selected_folder()?;
        self.clear_messages();
        self.message_action(false)
    }
    pub fn select_account(&mut self, account_id: &str) -> bool {
        if !self.accounts.iter().any(|a| a.account_id == account_id) {
            return false;
        }
        self.selected_account = Some(account_id.into());
        self.filter = ImapMetadataFilter::All;
        self.folders.clear();
        self.folder_cursor = None;
        self.selected_folder = None;
        self.clear_messages();
        true
    }
    pub fn select_folder(&mut self, index: usize) -> bool {
        if self
            .folders
            .get(index)
            .is_none_or(|f| Some(f.account_id.as_str()) != self.selected_account())
        {
            return false;
        }
        self.selected_folder = Some(index);
        self.clear_messages();
        true
    }
    pub fn select_message(&mut self, index: usize) -> bool {
        if index >= self.messages.len() {
            return false;
        }
        self.selected_message = Some(index);
        true
    }
    fn clear_messages(&mut self) {
        self.messages.clear();
        self.next_cursor = None;
        self.selected_message = None;
    }
    pub fn message_action(&self, next: bool) -> Option<MailAction> {
        let folder = self.selected_folder()?;
        Some(MailAction::Messages {
            account_id: folder.account_id.clone(),
            folder_wire_hex: folder.wire_hex.clone(),
            encoding: folder.encoding.clone(),
            filter: self.filter,
            cursor: if next {
                Some(self.next_cursor()?.as_str().into())
            } else {
                None
            },
        })
    }
    /// Reject stale account/folder/cursor responses before changing any displayed state.
    pub fn apply(
        &mut self,
        action: &MailAction,
        result: &MailActionResult,
    ) -> Result<(), ErrorCode> {
        let invalid = ErrorCode::InvalidInput;
        match (action, result) {
            (
                MailAction::Connect | MailAction::RegistrationStatus,
                MailActionResult::Registration(_),
            ) => Ok(()),
            (MailAction::Accounts, MailActionResult::Accounts(rows)) => {
                let mut ids = BTreeSet::new();
                if rows.len() > 128
                    || rows.iter().any(|r| {
                        !ids.insert(&r.account_id)
                            || MailAccount::new(&r.account_id, &r.label).as_ref() != Some(r)
                    })
                {
                    return Err(invalid);
                }
                self.clear();
                self.accounts = rows.clone();
                Ok(())
            }
            (
                MailAction::Folders { account_id, cursor },
                MailActionResult::Folders { rows, next_cursor },
            ) => {
                if self.selected_account() != Some(account_id.as_str())
                    || cursor.as_deref() != self.folder_cursor()
                {
                    return Err(ErrorCode::Conflict);
                }
                let mut ids = BTreeSet::new();
                if rows.len() > 32
                    || rows.iter().any(|r| {
                        r.account_id != *account_id
                            || !ids.insert((&r.wire_hex, &r.encoding))
                            || MailFolder::new_with_encoding(
                                &r.account_id,
                                &r.display_name,
                                &r.wire_hex,
                                &r.encoding,
                            )
                            .as_ref()
                                != Some(r)
                    })
                {
                    return Err(invalid);
                }
                if let Some(value) = next_cursor {
                    let parsed = FolderCursor::parse(value)?;
                    if parsed.account_id() != account_id || Some(value) == cursor.as_ref() {
                        return Err(invalid);
                    }
                }
                self.folders = rows.clone();
                self.folder_cursor = next_cursor.clone();
                self.selected_folder = None;
                self.clear_messages();
                Ok(())
            }
            (
                MailAction::Messages {
                    account_id,
                    folder_wire_hex,
                    encoding,
                    filter,
                    cursor,
                },
                MailActionResult::Messages {
                    rows,
                    next_cursor,
                    observed,
                },
            ) => {
                let Some(folder) = self.selected_folder() else {
                    return Err(ErrorCode::Conflict);
                };
                if &folder.account_id != account_id
                    || &folder.wire_hex != folder_wire_hex
                    || &folder.encoding != encoding
                    || self.filter != *filter
                    || cursor.as_deref() != self.next_cursor().map(PageCursor::as_str)
                {
                    return Err(ErrorCode::Conflict);
                }
                let mut ids = BTreeSet::new();
                if (!observed && (!rows.is_empty() || next_cursor.is_some()))
                    || rows.len() > 32
                    || rows.iter().any(|r| {
                        r.account_id != *account_id
                            || (*filter == ImapMetadataFilter::Unread && r.is_read)
                            || (*filter == ImapMetadataFilter::Favorite && !r.is_favorite)
                            || !ids.insert(&r.internal_id)
                            || MailMessage::new(
                                &r.account_id,
                                &r.internal_id,
                                &r.sender,
                                &r.subject,
                                &r.date_utc,
                                r.is_read,
                                r.is_favorite,
                            )
                            .as_ref()
                                != Some(r)
                    })
                    || (next_cursor.is_some()
                        && next_cursor.as_ref().map(PageCursor::as_str) == cursor.as_deref())
                {
                    return Err(invalid);
                }
                self.messages = rows.clone();
                self.next_cursor = next_cursor.clone();
                self.selected_message = None;
                Ok(())
            }
            _ => Err(invalid),
        }
    }
}
#[cfg(test)]
mod tests;
