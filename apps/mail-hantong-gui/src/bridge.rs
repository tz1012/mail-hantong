//! GUI-only IPC adapters. Read actions never request registration or Windows Hello.
use hantong_contracts::{
    ErrorCode,
    folders::{FolderCursor, FolderListRequest},
    message_search::{
        HeaderProjection, ImapMetadataSearchCursor, ImapMetadataSearchItem,
        ImapMetadataSearchRequest,
    },
    registration::{RegistrationCommand, RegistrationReceipt},
    uid_page::{UidMailboxEncoding, UidMailboxIdentity},
};
use hantong_desktop::{
    MailAccount, MailAction, MailActionResult, MailFolder, MailMessage, MailWorkspace, PageCursor,
};

pub(crate) trait Backend {
    fn registration(
        &mut self,
        command: RegistrationCommand,
    ) -> Result<RegistrationReceipt, ErrorCode>;
    fn accounts(&mut self) -> Result<MailActionResult, ErrorCode>;
    fn folders(&mut self, request: FolderListRequest) -> Result<MailActionResult, ErrorCode>;
    fn messages(
        &mut self,
        request: ImapMetadataSearchRequest,
    ) -> Result<MailActionResult, ErrorCode>;
}
pub(crate) struct IpcBackend;
impl Backend for IpcBackend {
    fn registration(
        &mut self,
        command: RegistrationCommand,
    ) -> Result<RegistrationReceipt, ErrorCode> {
        hantong_ipc_client::registration::exchange(command)
    }
    fn accounts(&mut self) -> Result<MailActionResult, ErrorCode> {
        let response = hantong_ipc_client::accounts::list_gui_accounts(false)?;
        let rows = response
            .items()
            .iter()
            .map(|a| {
                MailAccount::new(a.account_id(), a.display_name().unwrap_or(a.provider()))
                    .ok_or(ErrorCode::InvalidInput)
            })
            .collect::<Result<_, _>>()?;
        Ok(MailActionResult::Accounts(rows))
    }
    fn folders(&mut self, request: FolderListRequest) -> Result<MailActionResult, ErrorCode> {
        let account = request.account_id().to_owned();
        let page = hantong_ipc_client::folders::list_gui_folders(request)?;
        let rows = page
            .items()
            .iter()
            .filter(|f| {
                f.value()["no_select"].as_bool() == Some(false)
                    && f.value()["non_existent"].as_bool() == Some(false)
            })
            .map(|f| {
                MailFolder::new_with_encoding(
                    &account,
                    f.display_name(),
                    f.wire_hex(),
                    f.encoding(),
                )
                .ok_or(ErrorCode::InvalidInput)
            })
            .collect::<Result<_, _>>()?;
        Ok(MailActionResult::Folders {
            rows,
            next_cursor: page.next_cursor().map(str::to_owned),
        })
    }
    fn messages(
        &mut self,
        request: ImapMetadataSearchRequest,
    ) -> Result<MailActionResult, ErrorCode> {
        let account = request.account_id().to_owned();
        let page = hantong_ipc_client::message_search::search_gui(request)?;
        let mut rows = Vec::new();
        for item in page.items() {
            let header = page
                .headers()
                .get(item.header_content_index() as usize)
                .ok_or(ErrorCode::InvalidInput)?;
            rows.push(display_message(&account, item, header)?);
        }
        let next_cursor = page
            .next_cursor()
            .map(|v| PageCursor::new(v).ok_or(ErrorCode::InvalidInput))
            .transpose()?;
        Ok(MailActionResult::Messages {
            rows,
            next_cursor,
            observed: page.observed_at().is_some(),
        })
    }
}
fn display_message(
    account: &str,
    item: &ImapMetadataSearchItem,
    header: &HeaderProjection,
) -> Result<MailMessage, ErrorCode> {
    if item.account_id() != account {
        return Err(ErrorCode::InvalidInput);
    }
    let subject = header
        .subject()
        .map(crate::header_display::decode_subject)
        .unwrap_or_default();
    let sender = header
        .from()
        .and_then(|v| v.first())
        .map(|a| {
            let mailbox = a.mailbox().map(String::from_utf8_lossy).unwrap_or_default();
            let host = a.host().map(String::from_utf8_lossy).unwrap_or_default();
            if host.is_empty() {
                mailbox.into_owned()
            } else {
                format!("{mailbox}@{host}")
            }
        })
        .unwrap_or_default();
    MailMessage::new(
        account,
        item.internal_message_id(),
        &sender,
        &subject,
        &utc_date(item.internal_date_utc_ms()),
        item.is_read(),
        item.is_favorite(),
    )
    .ok_or(ErrorCode::InvalidInput)
}
// Gregorian civil date from a bounded UTC Unix timestamp; no locale or timezone lookup.
fn utc_date(milliseconds: i64) -> String {
    let seconds = milliseconds.div_euclid(1000);
    let days = seconds.div_euclid(86400) + 719468;
    let era = days.div_euclid(146097);
    let day = days - era * 146097;
    let year = (day - day / 1460 + day / 36524 - day / 146096) / 365;
    let ordinal = day - (365 * year + year / 4 - year / 100);
    let month = (5 * ordinal + 2) / 153;
    let date = ordinal - (153 * month + 2) / 5 + 1;
    let month = month + if month < 10 { 3 } else { -9 };
    let year = year + era * 400 + i64::from(month <= 2);
    let time = seconds.rem_euclid(86400);
    format!(
        "{year:04}-{month:02}-{date:02} {:02}:{:02}:{:02} UTC",
        time / 3600,
        time / 60 % 60,
        time % 60
    )
}

pub(crate) struct GuiBridge<B = IpcBackend> {
    request_id: Option<[u8; 32]>,
    workspace: MailWorkspace,
    backend: B,
}
impl GuiBridge<IpcBackend> {
    pub fn new() -> Self {
        Self {
            request_id: None,
            workspace: MailWorkspace::default(),
            backend: IpcBackend,
        }
    }
}
impl<B: Backend> GuiBridge<B> {
    pub fn action(&mut self, action: MailAction) -> Result<MailActionResult, ErrorCode> {
        // Validate selection and cursor before crossing IPC, using a tentative
        // local snapshot so a failed read cannot accidentally change its binding.
        let mut next = self.workspace.clone();
        let result = match &action {
            MailAction::Connect => {
                let receipt = self.backend.registration(RegistrationCommand::GuiRequest)?;
                self.request_id = Some(*receipt.request_id());
                MailActionResult::Registration(receipt.state())
            }
            MailAction::RegistrationStatus => {
                let id = self.request_id.ok_or(ErrorCode::NotFound)?;
                let receipt = self
                    .backend
                    .registration(RegistrationCommand::GuiStatus(id))?;
                if receipt.request_id() != &id {
                    return Err(ErrorCode::InvalidInput);
                }
                MailActionResult::Registration(receipt.state())
            }
            MailAction::Accounts => {
                self.workspace.clear();
                self.backend.accounts()?
            }
            MailAction::Folders { account_id, cursor } => {
                if cursor.is_none() && !next.select_account(account_id) {
                    return Err(ErrorCode::ScopeDenied);
                }
                if next.selected_account() != Some(account_id)
                    || cursor.as_deref() != next.folder_cursor()
                {
                    return Err(ErrorCode::Conflict);
                }
                self.backend.folders(FolderListRequest::new(
                    account_id,
                    None,
                    cursor.as_deref().map(FolderCursor::parse).transpose()?,
                )?)?
            }
            MailAction::Messages {
                account_id,
                folder_wire_hex,
                encoding,
                filter,
                cursor,
            } => {
                if next.selected_account() != Some(account_id) {
                    return Err(ErrorCode::ScopeDenied);
                }
                let index = next
                    .folders()
                    .iter()
                    .position(|f| {
                        f.account_id == *account_id
                            && f.wire_hex == *folder_wire_hex
                            && f.encoding == *encoding
                    })
                    .ok_or(ErrorCode::ScopeDenied)?;
                if cursor.is_none() {
                    next.select_folder(index);
                    next.set_filter(*filter);
                }
                if next.message_action(cursor.is_some()).as_ref() != Some(&action) {
                    return Err(ErrorCode::Conflict);
                }
                let mailbox =
                    UidMailboxIdentity::new(folder_wire_hex, UidMailboxEncoding::parse(encoding)?)?;
                let cursor = cursor
                    .as_deref()
                    .map(ImapMetadataSearchCursor::parse)
                    .transpose()?;
                self.backend.messages(ImapMetadataSearchRequest::new(
                    account_id,
                    mailbox,
                    Some(*filter),
                    None,
                    cursor,
                )?)?
            }
        };
        next.apply(&action, &result)?;
        self.workspace = next;
        Ok(result)
    }
}
#[cfg(test)]
mod tests;
