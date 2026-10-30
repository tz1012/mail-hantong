use hantong_contracts::{
    ErrorCode,
    uid_page::{UidCursorSnapshot, UidMailboxEncoding, UidMailboxIdentity, UidPageReceipt},
};
use hantong_domain::mailboxes::{ImapMailboxName, MailboxEncoding};
use serde_json::{Value, json};

pub fn validated_uid_mailbox(value: &UidMailboxIdentity) -> Result<ImapMailboxName, ErrorCode> {
    let wire = hantong_contracts::wire_hex::decode_bounded(value.wire_hex(), 1024)?;
    let encoding = match value.encoding() {
        UidMailboxEncoding::ModifiedUtf7 => MailboxEncoding::ModifiedUtf7,
        UidMailboxEncoding::Utf8 => MailboxEncoding::Utf8,
    };
    let name = ImapMailboxName::from_wire(&wire, encoding).map_err(|_| ErrorCode::InvalidInput)?;
    if name.wire_name() != wire {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(name)
}
pub fn render_uid_cursor(v: &UidCursorSnapshot) -> Value {
    json!({"schema_version":1,"ok":true,"account_id":v.account_id(),"wire_hex":v.mailbox().wire_hex(),"encoding":v.mailbox().encoding().as_str(),"catalog_generation":v.catalog_generation(),"cursor_version":v.cursor_version(),"covered_through_uid":v.covered_through_uid(),"binding_current":v.binding_current()})
}
pub fn render_uid_receipt(v: &UidPageReceipt) -> Value {
    json!({"schema_version":1,"ok":true,"account_id":v.account_id(),"execution_id":hantong_contracts::wire_hex::encode(v.execution_id()),"state":v.state().as_str(),"cursor_version":v.cursor_version(),"covered_through_uid":v.covered_through_uid(),"inserted_count":v.inserted_count(),"error_code":v.error().map(ErrorCode::as_str)})
}
