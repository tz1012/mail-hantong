//! Durable IDs for immutable IMAP message locations.
use crate::registrations::text_column;
use hantong_contracts::{
    ErrorCode,
    uid_page::{UidMailboxEncoding, UidMailboxIdentity},
};
use hantong_domain::{imap_sync::ImapUidPage, mailboxes::ImapMailboxName};
use rusqlite::{Connection, Transaction, params};

const MAX_MESSAGES: i64 = 1_000_000;
const INSERT: &str = "INSERT INTO imap_message_identities(internal_message_id,account_id,mailbox_key,mailbox_encoding,uid_validity,uid) VALUES(?1,?2,?3,?4,?5,?6)";

fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    r.map_err(|_| ErrorCode::InternalError)
}
fn frame(bytes: &mut Vec<u8>, value: &[u8]) -> Result<(), ErrorCode> {
    bytes.extend_from_slice(
        &u32::try_from(value.len())
            .map_err(|_| ErrorCode::InternalError)?
            .to_be_bytes(),
    );
    bytes.extend_from_slice(value);
    Ok(())
}
pub(crate) fn location_id(
    account: &str,
    mailbox: &ImapMailboxName,
    validity: u32,
    uid: u32,
) -> Result<String, ErrorCode> {
    crate::mail_connection::input_account(account).map_err(|_| ErrorCode::InternalError)?;
    if validity == 0 || uid == 0 {
        return Err(ErrorCode::InternalError);
    }
    let mut bytes = b"mail-hantong.imap-message-location.v1\0".to_vec();
    frame(&mut bytes, account.as_bytes())?;
    frame(&mut bytes, mailbox.wire_name())?;
    frame(&mut bytes, crate::uid_epochs::encoding(mailbox).as_bytes())?;
    bytes.extend_from_slice(&validity.to_be_bytes());
    bytes.extend_from_slice(&uid.to_be_bytes());
    let digest = hantong_platform::sha256(&bytes).map_err(|_| ErrorCode::InternalError)?;
    Ok(format!(
        "msg_{}",
        hantong_contracts::wire_hex::encode(&digest)
    ))
}
fn stored_name(key: &str, encoding: &str) -> rusqlite::Result<ImapMailboxName> {
    let identity = UidMailboxIdentity::new(
        key,
        UidMailboxEncoding::parse(encoding).map_err(|_| rusqlite::Error::InvalidQuery)?,
    )
    .map_err(|_| rusqlite::Error::InvalidQuery)?;
    crate::uid_page_jobs::storage::mailbox(&identity).map_err(|_| rusqlite::Error::InvalidQuery)
}

fn validate_preview_identity(
    c: &Connection,
    id: &str,
    account: &str,
    mailbox: &ImapMailboxName,
    validity: u32,
    uid: u32,
) -> Result<(), ErrorCode> {
    let collision: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM imap_initial_preview_messages WHERE (internal_message_id=?1 AND (account_id<>?2 OR mailbox_key<>?3 OR mailbox_encoding<>?4 OR uid_validity<>?5 OR uid<>?6)) OR (account_id=?2 AND mailbox_key=?3 AND mailbox_encoding=?4 AND uid_validity=?5 AND uid=?6 AND internal_message_id<>?1))",
        params![id, account, crate::uid_epochs::key(mailbox), crate::uid_epochs::encoding(mailbox), i64::from(validity), i64::from(uid)], |r| r.get(0),
    ))?;
    if collision {
        return Err(ErrorCode::InternalError);
    }
    Ok(())
}

/// Streams scalar locators only. The caller owns the migration transaction.
pub(crate) fn backfill_v14(c: &Connection) -> rusqlite::Result<()> {
    let (base,mapped):(i64,i64)=c.query_row("SELECT (SELECT count(*) FROM imap_uid_messages),(SELECT count(*) FROM imap_message_identities)",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
    if !(0..=MAX_MESSAGES).contains(&base) || mapped != 0 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let invalid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM imap_uid_messages m LEFT JOIN imap_uid_page_operations o ON o.operation_id=m.operation_id LEFT JOIN imap_uid_epochs e ON e.account_id=m.account_id AND e.mailbox_key=m.mailbox_key AND e.mailbox_encoding=m.mailbox_encoding AND e.uid_validity=m.uid_validity LEFT JOIN imap_uid_cursors u ON u.account_id=m.account_id AND u.mailbox_key=m.mailbox_key AND u.mailbox_encoding=m.mailbox_encoding WHERE o.operation_id IS NULL OR e.first_operation_id IS NULL OR u.last_operation_id IS NULL OR m.account_id<>o.account_id OR m.mailbox_key<>o.mailbox_key OR m.mailbox_encoding<>o.mailbox_encoding OR m.uid_validity<>o.uid_validity OR m.uid<=o.after_uid OR m.uid>o.covered_through_uid OR o.result_cursor_version<e.first_cursor_version OR o.result_cursor_version>coalesce(e.last_cursor_version,u.version))",[],|r|r.get(0))?;
    if invalid {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let mut source=c.prepare("SELECT account_id,mailbox_key,mailbox_encoding,uid_validity,uid FROM imap_uid_messages ORDER BY account_id,mailbox_key,mailbox_encoding,uid_validity,uid LIMIT 1000001")?;
    let mut insert = c.prepare(INSERT)?;
    let mut verify=c.prepare("SELECT internal_message_id FROM imap_message_identities WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4 AND uid=?5")?;
    let mut rows = source.query([])?;
    let mut seen = 0i64;
    while let Some(row) = rows.next()? {
        seen += 1;
        if seen > MAX_MESSAGES {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let account = text_column(row, 0, 128)?;
        let key = text_column(row, 1, 2048)?;
        let encoding = text_column(row, 2, 13)?;
        let validity: u32 = row.get(3)?;
        let uid: u32 = row.get(4)?;
        let name = stored_name(key, encoding)?;
        let id = location_id(account, &name, validity, uid)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        if insert.execute(params![
            id,
            account,
            key,
            encoding,
            i64::from(validity),
            i64::from(uid)
        ])? != 1
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let written = verify.query_row(
            params![account, key, encoding, i64::from(validity), i64::from(uid)],
            |r| Ok(text_column(r, 0, 68)?.to_owned()),
        )?;
        if written != id {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    if seen != base {
        return Err(rusqlite::Error::InvalidQuery);
    }
    validate_history(c)
}

pub(crate) fn insert_page(c: &Transaction<'_>, page: &ImapUidPage) -> Result<(), ErrorCode> {
    let count: i64 = db(
        c.query_row("SELECT count(*) FROM imap_message_identities", [], |r| {
            r.get(0)
        }),
    )?;
    if !(0..=MAX_MESSAGES).contains(&count) {
        return Err(ErrorCode::InternalError);
    }
    // Shared apply checked the base-message quota before any write. A mapping
    // count that disagrees here is corruption and must roll back that apply.
    if count + page.messages().len() as i64 > MAX_MESSAGES {
        return Err(ErrorCode::InternalError);
    }
    let r = page.request();
    let key = crate::uid_epochs::key(r.mailbox());
    let encoding = crate::uid_epochs::encoding(r.mailbox());
    let mut insert = db(c.prepare(INSERT))?;
    for message in page.messages() {
        let id = location_id(
            r.account_id(),
            r.mailbox(),
            page.uid_validity(),
            message.uid(),
        )?;
        validate_preview_identity(
            c,
            &id,
            r.account_id(),
            r.mailbox(),
            page.uid_validity(),
            message.uid(),
        )?;
        if db(insert.execute(params![
            id,
            r.account_id(),
            key,
            encoding,
            i64::from(page.uid_validity()),
            i64::from(message.uid())
        ]))? != 1
        {
            return Err(ErrorCode::InternalError);
        }
    }
    validate_page(c, page)?;
    Ok(())
}

/// One indexed bounded batch, aligned with the immutable page's ascending UIDs.
pub(crate) fn validate_page(c: &Connection, page: &ImapUidPage) -> Result<Vec<String>, ErrorCode> {
    let r = page.request();
    let mut s=db(c.prepare("SELECT internal_message_id,uid FROM imap_message_identities WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4 AND uid>?5 AND uid<=?6 ORDER BY uid LIMIT 65"))?;
    let values = db(db(s.query_map(
        params![
            r.account_id(),
            crate::uid_epochs::key(r.mailbox()),
            crate::uid_epochs::encoding(r.mailbox()),
            i64::from(page.uid_validity()),
            i64::from(r.after_uid()),
            i64::from(page.covered_through_uid())
        ],
        |row| Ok((text_column(row, 0, 68)?.to_owned(), row.get::<_, u32>(1)?)),
    ))?
    .collect::<rusqlite::Result<Vec<_>>>())?;
    if values.len() != page.messages().len() {
        return Err(ErrorCode::InternalError);
    }
    let mut ids = Vec::with_capacity(values.len());
    for ((id, uid), message) in values.into_iter().zip(page.messages()) {
        if uid != message.uid()
            || id != location_id(r.account_id(), r.mailbox(), page.uid_validity(), uid)?
        {
            return Err(ErrorCode::InternalError);
        }
        validate_preview_identity(
            c,
            &id,
            r.account_id(),
            r.mailbox(),
            page.uid_validity(),
            uid,
        )?;
        ids.push(id);
    }
    Ok(ids)
}

/// Startup verifies the scalar bijection; selected reads verify ID hashes.
pub(crate) fn validate_history(c: &Connection) -> rusqlite::Result<()> {
    let (base,mapped):(i64,i64)=c.query_row("SELECT (SELECT count(*) FROM imap_uid_messages),(SELECT count(*) FROM imap_message_identities)",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
    if !(0..=MAX_MESSAGES).contains(&base) || mapped != base {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let invalid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM imap_uid_messages m WHERE NOT EXISTS(SELECT 1 FROM imap_message_identities i WHERE i.account_id=m.account_id AND i.mailbox_key=m.mailbox_key AND i.mailbox_encoding=m.mailbox_encoding AND i.uid_validity=m.uid_validity AND i.uid=m.uid)) OR EXISTS(SELECT 1 FROM imap_message_identities i WHERE NOT EXISTS(SELECT 1 FROM imap_uid_messages m WHERE m.account_id=i.account_id AND m.mailbox_key=i.mailbox_key AND m.mailbox_encoding=i.mailbox_encoding AND m.uid_validity=i.uid_validity AND m.uid=i.uid))",[],|r|r.get(0))?;
    if invalid {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
