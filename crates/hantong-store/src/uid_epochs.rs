//! Immutable UID namespace boundaries; only retirement fields may change once.
#![cfg(windows)]
use crate::{
    imap_sync::storage::{Operation, read_operation},
    registrations::{blob_column, text_column},
};
use hantong_contracts::ErrorCode;
use hantong_domain::mailboxes::{ImapMailboxName, MailboxEncoding};
use rusqlite::{Connection, OptionalExtension, params};

fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    r.map_err(|_| ErrorCode::InternalError)
}
pub(crate) fn key(name: &ImapMailboxName) -> String {
    hantong_contracts::wire_hex::encode(name.wire_name())
}
pub(crate) fn encoding(name: &ImapMailboxName) -> &'static str {
    match name.encoding() {
        MailboxEncoding::ModifiedUtf7 => "modified_utf7",
        MailboxEncoding::Utf8 => "utf8",
    }
}
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct Epoch {
    pub(crate) account: String,
    pub(crate) mailbox: ImapMailboxName,
    pub(crate) validity: u32,
    pub(crate) generation: u64,
    pub(crate) first: u64,
    pub(crate) first_id: String,
    pub(crate) endpoint_version: u64,
    pub(crate) endpoint_hash: [u8; 32],
    pub(crate) created: i64,
    pub(crate) last: Option<u64>,
    pub(crate) last_id: Option<String>,
    pub(crate) retired: Option<i64>,
    pub(crate) recovery: Option<[u8; 32]>,
}
impl std::fmt::Debug for Epoch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Epoch { <redacted> }")
    }
}
fn opt_text(r: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<Option<String>> {
    if matches!(r.get_ref(i)?, rusqlite::types::ValueRef::Null) {
        Ok(None)
    } else {
        Ok(Some(text_column(r, i, 128)?.to_owned()))
    }
}
fn opt_hash(r: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<Option<[u8; 32]>> {
    if matches!(r.get_ref(i)?, rusqlite::types::ValueRef::Null) {
        Ok(None)
    } else {
        Ok(Some(blob_column::<32>(r, i)?))
    }
}
pub(crate) fn raw(
    c: &Connection,
    account: &str,
    name: &ImapMailboxName,
    validity: u32,
) -> Result<Option<Epoch>, ErrorCode> {
    let row=db(c.query_row("SELECT epoch_generation,first_cursor_version,first_operation_id,endpoint_version,endpoint_hash,created_at_utc_ms,last_cursor_version,retired_last_operation_id,retired_at_utc_ms,recovery_execution_id FROM imap_uid_epochs WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4",params![account,key(name),encoding(name),i64::from(validity)],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,text_column(r,2,128)?.to_owned(),r.get::<_,i64>(3)?,blob_column::<32>(r,4)?,r.get::<_,i64>(5)?,r.get::<_,Option<i64>>(6)?,opt_text(r,7)?,r.get::<_,Option<i64>>(8)?,opt_hash(r,9)?))).optional())?;
    let Some((g, first, id, ev, eh, created, last, last_id, retired, recovery)) = row else {
        return Ok(None);
    };
    if validity == 0
        || g <= 0
        || first <= 0
        || g > first
        || ev <= 0
        || created <= 0
        || last.is_some_and(|n| n < first)
        || retired.is_some_and(|at| at < created)
        || last.is_some() != last_id.is_some()
        || last.is_some() != retired.is_some()
        || (g == 1 && (first != 1 || recovery.is_some()))
        || (g > 1 && (first == 1 || recovery.is_none()))
        || recovery == Some([0; 32])
    {
        return Err(ErrorCode::InternalError);
    }
    Ok(Some(Epoch {
        account: account.into(),
        mailbox: name.clone(),
        validity,
        generation: g as u64,
        first: first as u64,
        first_id: id,
        endpoint_version: ev as u64,
        endpoint_hash: eh,
        created,
        last: last.map(|v| v as u64),
        last_id,
        retired,
        recovery,
    }))
}
fn same_operation(e: &Epoch, op: &Operation) -> bool {
    let r = op.page.request();
    r.account_id() == e.account
        && r.mailbox().wire_name() == e.mailbox.wire_name()
        && r.mailbox().encoding() == e.mailbox.encoding()
        && op.page.uid_validity() == e.validity
        && r.endpoint_version() == e.endpoint_version
        && op.endpoint_hash == e.endpoint_hash
}
pub(crate) fn boundary_last(c: &Connection, e: &Epoch) -> Result<Operation, ErrorCode> {
    let (version, id) = if let (Some(last), Some(id)) = (e.last, &e.last_id) {
        (last, id.clone())
    } else {
        let (version,validity,id):(i64,i64,String)=db(c.query_row("SELECT version,uid_validity,last_operation_id FROM imap_uid_cursors WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3",params![e.account,key(&e.mailbox),encoding(&e.mailbox)],|r|Ok((r.get(0)?,r.get(1)?,text_column(r,2,128)?.to_owned()))))?;
        if version <= 0 || validity != i64::from(e.validity) {
            return Err(ErrorCode::InternalError);
        }
        (version as u64, id)
    };
    let op = read_operation(c, &id)?.ok_or(ErrorCode::InternalError)?;
    if !same_operation(e, &op)
        || op.receipt.cursor_version() != version
        || version < e.first
        || op.at < e.created
        || e.retired.is_some_and(|at| op.at > at)
    {
        return Err(ErrorCode::InternalError);
    }
    Ok(op)
}
pub(crate) fn read(
    c: &Connection,
    account: &str,
    name: &ImapMailboxName,
    validity: u32,
) -> Result<Option<Epoch>, ErrorCode> {
    let Some(e) = raw(c, account, name, validity)? else {
        return Ok(None);
    };
    let first = read_operation(c, &e.first_id)?.ok_or(ErrorCode::InternalError)?;
    if !same_operation(&e, &first)
        || first.receipt.cursor_version() != e.first
        || first.page.request().after_uid() != 0
        || first.at != e.created
        || (e.generation == 1 && first.page.request().expected_uid_validity().is_some())
        || (e.generation > 1 && first.page.request().expected_uid_validity() != Some(validity))
    {
        return Err(ErrorCode::InternalError);
    }
    let last = boundary_last(c, &e)?;
    let (count,min,max,bad):(i64,i64,i64,i64)=db(c.query_row("SELECT count(*),coalesce(min(result_cursor_version),0),coalesce(max(result_cursor_version),0),coalesce(sum(CASE WHEN endpoint_version<>?5 OR endpoint_hash<>?6 THEN 1 ELSE 0 END),0) FROM imap_uid_page_operations WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4",params![account,key(name),encoding(name),i64::from(validity),e.endpoint_version as i64,e.endpoint_hash.as_slice()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))))?;
    if min != e.first as i64
        || max != last.receipt.cursor_version() as i64
        || count != max - min + 1
        || bad != 0
    {
        return Err(ErrorCode::InternalError);
    }
    let (epochs,maximum,active):(i64,i64,i64)=db(c.query_row("SELECT count(*),coalesce(max(epoch_generation),0),coalesce(sum(CASE WHEN last_cursor_version IS NULL THEN 1 ELSE 0 END),0) FROM imap_uid_epochs WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3",params![account,key(name),encoding(name)],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))))?;
    if epochs != maximum
        || active != 1
        || e.generation > maximum as u64
        || (e.last.is_none() && e.generation != maximum as u64)
    {
        return Err(ErrorCode::InternalError);
    }
    if e.generation > 1 {
        let previous_validity:i64=db(c.query_row("SELECT uid_validity FROM imap_uid_epochs WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND epoch_generation=?4",params![account,key(name),encoding(name),e.generation as i64-1],|r|r.get(0)))?;
        let previous = raw(
            c,
            account,
            name,
            u32::try_from(previous_validity).map_err(|_| ErrorCode::InternalError)?,
        )?
        .ok_or(ErrorCode::InternalError)?;
        if previous.last != Some(e.first - 1)
            || previous.retired != Some(e.created)
            || previous.endpoint_version != e.endpoint_version
            || previous.endpoint_hash != e.endpoint_hash
        {
            return Err(ErrorCode::InternalError);
        }
        let previous_last = boundary_last(c, &previous)?;
        if previous_last.at > e.created {
            return Err(ErrorCode::InternalError);
        }
        crate::uid_recovery_jobs::validate_epoch_link(c, &previous, &e, &first)?;
    }
    if let Some(last) = e.last {
        let next_validity:u32=db(c.query_row("SELECT uid_validity FROM imap_uid_epochs WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND epoch_generation=?4",params![account,key(name),encoding(name),e.generation as i64+1],|r|r.get(0)))?;
        let next = raw(c, account, name, next_validity)?.ok_or(ErrorCode::InternalError)?;
        let next_first = read_operation(c, &next.first_id)?.ok_or(ErrorCode::InternalError)?;
        if last.checked_add(1) != Some(next.first)
            || e.retired != Some(next.created)
            || next.validity == validity
            || next.endpoint_version != e.endpoint_version
            || next.endpoint_hash != e.endpoint_hash
            || !same_operation(&next, &next_first)
        {
            return Err(ErrorCode::InternalError);
        }
        crate::uid_recovery_jobs::validate_epoch_link(c, &e, &next, &next_first)?;
    }
    Ok(Some(e))
}
/// Validate an immutable receipt against its own namespace without job-reader recursion.
pub(crate) fn validate_operation(c: &Connection, op: &Operation) -> Result<(), ErrorCode> {
    let request = op.page.request();
    let epoch = read(
        c,
        request.account_id(),
        request.mailbox(),
        op.page.uid_validity(),
    )?
    .ok_or(ErrorCode::InternalError)?;
    let last = boundary_last(c, &epoch)?;
    if !same_operation(&epoch, op)
        || op.receipt.cursor_version() < epoch.first
        || op.receipt.cursor_version() > last.receipt.cursor_version()
        || op.at < epoch.created
        || epoch.retired.is_some_and(|at| op.at > at)
    {
        return Err(ErrorCode::InternalError);
    }
    Ok(())
}
pub(crate) fn insert_initial(c: &Connection, op: &Operation) -> Result<(), ErrorCode> {
    if op.expected != 0
        || op.receipt.cursor_version() != 1
        || op.page.request().after_uid() != 0
        || op.page.request().expected_uid_validity().is_some()
    {
        return Err(ErrorCode::InternalError);
    }
    let r = op.page.request();
    if db(c.execute("INSERT INTO imap_uid_epochs(account_id,mailbox_key,mailbox_encoding,uid_validity,epoch_generation,first_cursor_version,first_operation_id,endpoint_version,endpoint_hash,created_at_utc_ms) VALUES(?1,?2,?3,?4,1,1,?5,?6,?7,?8)",params![r.account_id(),key(r.mailbox()),encoding(r.mailbox()),i64::from(op.page.uid_validity()),op.id,r.endpoint_version() as i64,op.endpoint_hash.as_slice(),op.at]))?!=1{return Err(ErrorCode::InternalError)}
    Ok(())
}
pub(crate) fn check_quota(c: &Connection) -> Result<(), ErrorCode> {
    let count: i64 = db(c.query_row("SELECT count(*) FROM imap_uid_epochs", [], |r| r.get(0)))?;
    if count > 100_000 {
        Err(ErrorCode::InternalError)
    } else if count == 100_000 {
        Err(ErrorCode::QuotaExceeded)
    } else {
        Ok(())
    }
}
/// v13 decoding stays bounded to the first/current pages; scalar history is checked separately.
pub(crate) fn backfill_v13(c: &Connection) -> rusqlite::Result<()> {
    backfill(c).map_err(|_| rusqlite::Error::InvalidQuery)
}
fn backfill(c: &Connection) -> Result<(), ErrorCode> {
    let mut stmt=db(c.prepare("SELECT account_id,mailbox_key,mailbox_encoding,uid_validity FROM imap_uid_cursors ORDER BY account_id,mailbox_key,mailbox_encoding"))?;
    let mut rows = db(stmt.query([]))?;
    while let Some(r) = db(rows.next())? {
        let account = text_column(r, 0, 128).map_err(|_| ErrorCode::InternalError)?;
        let wire = text_column(r, 1, 2048).map_err(|_| ErrorCode::InternalError)?;
        let enc = text_column(r, 2, 13).map_err(|_| ErrorCode::InternalError)?;
        let validity = db(r.get::<_, u32>(3))?;
        let identity = hantong_contracts::uid_page::UidMailboxIdentity::new(
            wire,
            hantong_contracts::uid_page::UidMailboxEncoding::parse(enc)?,
        )
        .map_err(|_| ErrorCode::InternalError)?;
        let name = crate::uid_page_jobs::storage::mailbox(&identity)
            .map_err(|_| ErrorCode::InternalError)?;
        let cursor = crate::imap_sync::storage::read_cursor_v13(c, account, &name)?
            .ok_or(ErrorCode::InternalError)?;
        if cursor.value.uid_validity() != validity {
            return Err(ErrorCode::InternalError);
        }
        let bad:i64=db(c.query_row("SELECT (SELECT count(*) FROM imap_uid_page_operations WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND (uid_validity<>?4 OR endpoint_version<>?5 OR endpoint_hash<>?6))+(SELECT count(*) FROM imap_uid_messages WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity<>?4)+(SELECT count(*) FROM imap_uid_reconcile_heads_v13 WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity<>?4)",params![account,wire,enc,i64::from(validity),cursor.value.endpoint_version() as i64,cursor.endpoint_hash.as_slice()],|r|r.get(0)))?;
        if bad != 0 {
            return Err(ErrorCode::InternalError);
        }
        let first_id:String=db(c.query_row("SELECT operation_id FROM imap_uid_page_operations WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND result_cursor_version=1",params![account,wire,enc],|r|Ok(text_column(r,0,128)?.to_owned())))?;
        let first = read_operation(c, &first_id)?.ok_or(ErrorCode::InternalError)?;
        if first.page.uid_validity() != validity || first.endpoint_hash != cursor.endpoint_hash {
            return Err(ErrorCode::InternalError);
        }
        check_quota(c)?;
        insert_initial(c, &first)?;
    }
    Ok(())
}
pub(crate) fn validate_history(c: &Connection) -> rusqlite::Result<()> {
    let count: i64 = c.query_row("SELECT count(*) FROM imap_uid_epochs", [], |r| r.get(0))?;
    let bad:i64=c.query_row("SELECT count(*) FROM imap_uid_cursors c LEFT JOIN imap_uid_epochs e ON e.account_id=c.account_id AND e.mailbox_key=c.mailbox_key AND e.mailbox_encoding=c.mailbox_encoding AND e.uid_validity=c.uid_validity AND e.last_cursor_version IS NULL WHERE e.uid_validity IS NULL OR e.first_cursor_version>c.version OR e.endpoint_version<>c.endpoint_version OR e.endpoint_hash<>c.endpoint_hash",[],|r|r.get(0))?;
    let orphan: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM imap_uid_epochs e WHERE NOT EXISTS(SELECT 1 FROM imap_uid_cursors c WHERE c.account_id=e.account_id AND c.mailbox_key=e.mailbox_key AND c.mailbox_encoding=e.mailbox_encoding))", [], |r| r.get(0))?;
    if count > 100_000 || bad != 0 || orphan {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}
