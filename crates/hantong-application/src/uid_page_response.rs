use hantong_contracts::{
    ErrorCode,
    uid_page::{UidCursorSnapshot, UidMailboxEncoding, UidMailboxIdentity, UidPageReceipt},
    uid_reconcile::UidReconcileReceipt,
    uid_recovery::UidRecoveryReceipt,
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
pub fn render_uid_reconcile_receipt(v: &UidReconcileReceipt) -> Value {
    let result=v.result().map(|r|json!({"generation":r.generation(),"frozen_cursor_version":r.frozen_cursor_version(),"source_cursor_version":r.source_cursor_version(),"after_uid":r.after_uid(),"through_uid":r.through_uid(),"observed_count":r.observed_count(),"flags_changed_count":r.flags_changed_count(),"newly_absent_count":r.newly_absent_count(),"remaining_pages":r.remaining_pages(),"pass_complete":r.pass_complete()}));
    json!({"schema_version":1,"ok":true,"account_id":v.account_id(),"execution_id":hantong_contracts::wire_hex::encode(v.execution_id()),"state":v.state().as_str(),"result":result,"error_code":v.error().map(ErrorCode::as_str)})
}
pub fn render_uid_recovery_receipt(v: &UidRecoveryReceipt) -> Value {
    let result=v.result().map(|r|json!({"epoch_generation":r.epoch_generation(),"cursor_version":r.cursor_version(),"covered_through_uid":r.covered_through_uid(),"inserted_count":r.inserted_count()}));
    json!({"schema_version":1,"ok":true,"account_id":v.account_id(),"execution_id":hantong_contracts::wire_hex::encode(v.execution_id()),"state":v.state().as_str(),"result":result,"error_code":v.error().map(ErrorCode::as_str)})
}
