use super::*;
use crate::{
    imap_catalog::{endpoint_hash, load_catalog},
    mail_connection::{MailEndpointKind, check_account, input_account, read_endpoint},
    registrations::{blob_column, text_column},
};
use hantong_domain::{mail_connection::MailConnectionConfig, mailboxes::MailboxEncoding};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    r.map_err(|_| ErrorCode::InternalError)
}
fn hash(b: &[u8]) -> Result<[u8; 32], ErrorCode> {
    hantong_platform::sha256(b).map_err(|_| ErrorCode::InternalError)
}
fn encoding(m: &ImapMailboxName) -> &'static str {
    match m.encoding() {
        MailboxEncoding::Utf8 => "utf8",
        MailboxEncoding::ModifiedUtf7 => "modified_utf7",
    }
}
fn key(m: &ImapMailboxName) -> String {
    hantong_contracts::wire_hex::encode(m.wire_name())
}
fn same_name(a: &ImapMailboxName, b: &ImapMailboxName) -> bool {
    a.wire_name() == b.wire_name() && a.encoding() == b.encoding()
}
fn payload_parts(
    av: u64,
    cg: u64,
    cv: u64,
    eh: &[u8; 32],
    json: &[u8],
) -> Result<[u8; 32], ErrorCode> {
    let mut bytes = b"mail-hantong.imap-uid-page-payload.v1\0".to_vec();
    for n in [av, cg, cv] {
        bytes.extend_from_slice(&n.to_be_bytes());
    }
    bytes.extend_from_slice(eh);
    bytes.extend_from_slice(&(json.len() as u64).to_be_bytes());
    bytes.extend_from_slice(json);
    hash(&bytes)
}
pub(super) fn payload_hash(
    av: u64,
    cg: u64,
    endpoint: &ImapEndpointConfig,
    page: &ImapUidPage,
    cv: u64,
) -> Result<[u8; 32], ErrorCode> {
    if !(1..=i64::MAX as u64).contains(&av)
        || !(1..=i64::MAX as u64).contains(&cg)
        || cv >= i64::MAX as u64
        || page.request().endpoint_version() != endpoint.settings_version()
    {
        return Err(ErrorCode::InvalidInput);
    }
    let json = page
        .to_storage_json()
        .map_err(|_| ErrorCode::InvalidInput)?;
    payload_parts(
        av,
        cg,
        cv,
        &endpoint_hash(page.request().account_id(), endpoint)?,
        &json,
    )
}
pub(crate) struct Operation {
    pub(crate) id: String,
    pub(crate) key: String,
    pub(crate) page: ImapUidPage,
    pub(crate) json: String,
    pub(crate) endpoint_hash: [u8; 32],
    pub(crate) payload_hash: [u8; 32],
    pub(crate) av: u64,
    pub(crate) cg: u64,
    pub(crate) expected: u64,
    pub(crate) receipt: ImapUidApplyReceipt,
    pub(crate) at: i64,
}
pub(crate) fn read_operation(c: &Connection, id: &str) -> Result<Option<Operation>, ErrorCode> {
    let raw=db(c.query_row("SELECT operation_id,account_id,idempotency_key,mailbox_key,mailbox_encoding,expected_cursor_version,result_cursor_version,account_version,catalog_generation,endpoint_version,uid_validity,uid_next_at_start,after_uid,covered_through_uid,inserted_count,applied_at_utc_ms,endpoint_hash,page_json,page_hash,payload_hash FROM imap_uid_page_operations WHERE operation_id=?1",[id],|r|{
  let strings=[text_column(r,0,128)?.to_owned(),text_column(r,1,128)?.to_owned(),text_column(r,2,128)?.to_owned(),text_column(r,3,2048)?.to_owned(),text_column(r,4,13)?.to_owned()];
  let mut n=[0i64;11];for (i,v) in n.iter_mut().enumerate(){*v=r.get(i+5)?;}
  Ok((strings,n,blob_column::<32>(r,16)?,text_column(r,17,8*1024*1024)?.to_owned(),blob_column::<32>(r,18)?,blob_column::<32>(r,19)?))
 }).optional())?;
    let Some((s, n, eh, json, ph, payload)) = raw else {
        return Ok(None);
    };
    for value in &s[..3] {
        input_account(value).map_err(|_| ErrorCode::InternalError)?;
    }
    if n[0] < 0
        || n[0] == i64::MAX
        || n[1] != n[0] + 1
        || n[2] <= 0
        || n[3] <= 0
        || n[4] <= 0
        || n[10] <= 0
    {
        return Err(ErrorCode::InternalError);
    }
    let page =
        ImapUidPage::from_storage_json(json.as_bytes()).map_err(|_| ErrorCode::InternalError)?;
    let r = page.request();
    if s[0] != id
        || s[1] != r.account_id()
        || s[3] != key(r.mailbox())
        || s[4] != encoding(r.mailbox())
        || n[4] != r.endpoint_version() as i64
        || n[5] != i64::from(page.uid_validity())
        || n[6] != i64::from(page.uid_next_at_start())
        || n[7] != i64::from(r.after_uid())
        || n[8] != i64::from(page.covered_through_uid())
        || n[9] != page.messages().len() as i64
        || hash(json.as_bytes())? != ph
        || payload_parts(n[2] as u64, n[3] as u64, n[0] as u64, &eh, json.as_bytes())? != payload
    {
        return Err(ErrorCode::InternalError);
    }
    if (n[0] == 0 && (r.expected_uid_validity().is_some() || r.after_uid() != 0))
        || (n[0] > 0 && r.expected_uid_validity() != Some(page.uid_validity()))
    {
        return Err(ErrorCode::InternalError);
    }
    let op = Operation {
        id: s[0].clone(),
        key: s[2].clone(),
        page,
        json,
        endpoint_hash: eh,
        payload_hash: payload,
        av: n[2] as u64,
        cg: n[3] as u64,
        expected: n[0] as u64,
        receipt: ImapUidApplyReceipt {
            cursor_version: n[1] as u64,
            covered_through_uid: n[8] as u32,
            inserted_count: n[9] as u32,
        },
        at: n[10],
    };
    // Every operation's receipt is checked against its immutable inserted rows.
    let mut stmt=db(c.prepare("SELECT account_id,mailbox_key,mailbox_encoding,uid_validity,uid,metadata_json,metadata_hash,stored_at_utc_ms FROM imap_uid_messages WHERE operation_id=?1 ORDER BY uid LIMIT 65"))?;
    let rows = db(stmt.query_map([id], |r| {
        Ok((
            text_column(r, 0, 128)?.to_owned(),
            text_column(r, 1, 2048)?.to_owned(),
            text_column(r, 2, 13)?.to_owned(),
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
            text_column(r, 5, 256 * 1024)?.to_owned(),
            blob_column::<32>(r, 6)?,
            r.get::<_, i64>(7)?,
        ))
    }))?;
    let rows = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
    if rows.len() != op.page.messages().len() {
        return Err(ErrorCode::InternalError);
    }
    for (row, message) in rows.iter().zip(op.page.messages()) {
        if row.0 != s[1]
            || row.1 != s[3]
            || row.2 != s[4]
            || row.3 != i64::from(op.page.uid_validity())
            || row.4 != i64::from(message.uid())
            || row.7 != op.at
            || hash(row.5.as_bytes())? != row.6
            || message
                .to_storage_json()
                .map_err(|_| ErrorCode::InternalError)?
                .as_slice()
                != row.5.as_bytes()
        {
            return Err(ErrorCode::InternalError);
        }
    }
    Ok(Some(op))
}
pub(crate) fn current_endpoint(
    c: &Connection,
    account: &str,
) -> Result<Option<ImapEndpointConfig>, ErrorCode> {
    let Some(stored) = read_endpoint(c, account, MailEndpointKind::Imap)? else {
        return Ok(None);
    };
    let bytes = serde_json::to_vec(
        &serde_json::json!({"schema_version":1,"account_id":account,"imap":stored.json}),
    )
    .map_err(|_| ErrorCode::InternalError)?;
    Ok(Some(
        MailConnectionConfig::from_json(&bytes)
            .map_err(|_| ErrorCode::InternalError)?
            .imap()
            .clone(),
    ))
}
pub(crate) struct Cursor {
    pub(crate) value: StoredImapUidCursor,
    pub(crate) endpoint_hash: [u8; 32],
}
pub(crate) fn read_cursor(
    c: &Connection,
    account: &str,
    mailbox: &ImapMailboxName,
) -> Result<Option<Cursor>, ErrorCode> {
    let raw=db(c.query_row("SELECT version,account_version,catalog_generation,endpoint_version,uid_validity,uid_next_at_start,covered_through_uid,stored_at_utc_ms,endpoint_hash,last_operation_id FROM imap_uid_cursors WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3",params![account,key(mailbox),encoding(mailbox)],|r|{
  let mut n=[0i64;8];for (i,v) in n.iter_mut().enumerate(){*v=r.get(i)?;}Ok((n,blob_column::<32>(r,8)?,text_column(r,9,128)?.to_owned()))
 }).optional())?;
    let Some((n, eh, id)) = raw else {
        return Ok(None);
    };
    if n[..6].iter().any(|v| *v <= 0)
        || n[4] > i64::from(u32::MAX)
        || n[5] > i64::from(u32::MAX)
        || n[6] < 0
        || n[6] >= n[5]
        || n[7] <= 0
    {
        return Err(ErrorCode::InternalError);
    }
    let op = read_operation(c, &id)?.ok_or(ErrorCode::InternalError)?;
    if op.page.request().account_id() != account
        || !same_name(op.page.request().mailbox(), mailbox)
        || op.receipt.cursor_version != n[0] as u64
        || op.av != n[1] as u64
        || op.cg != n[2] as u64
        || op.page.request().endpoint_version() != n[3] as u64
        || op.page.uid_validity() != n[4] as u32
        || op.page.uid_next_at_start() != n[5] as u32
        || op.page.covered_through_uid() != n[6] as u32
        || op.at != n[7]
        || op.endpoint_hash != eh
    {
        return Err(ErrorCode::InternalError);
    }
    let (count,max): (i64,i64)=db(c.query_row("SELECT count(*),coalesce(max(result_cursor_version),0) FROM imap_uid_page_operations WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3",params![account,key(mailbox),encoding(mailbox)],|r|Ok((r.get(0)?,r.get(1)?))))?;
    if count != n[0] || max != n[0] {
        return Err(ErrorCode::InternalError);
    }
    if op.expected > 0 {
        let previous_id:String=db(c.query_row("SELECT operation_id FROM imap_uid_page_operations WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND result_cursor_version=?4",params![account,key(mailbox),encoding(mailbox),op.expected as i64],|r|Ok(text_column(r,0,128)?.to_owned())))?;
        let previous = read_operation(c, &previous_id)?.ok_or(ErrorCode::InternalError)?;
        if previous.page.covered_through_uid() != op.page.request().after_uid()
            || previous.page.uid_validity() != op.page.uid_validity()
            || previous.endpoint_hash != op.endpoint_hash
            || previous.page.uid_next_at_start() > op.page.uid_next_at_start()
            || previous.at > op.at
        {
            return Err(ErrorCode::InternalError);
        }
    }
    let catalog = load_catalog(c, account)?;
    let endpoint = current_endpoint(c, account)?;
    let current = match (catalog, endpoint) {
        (Some(catalog), Some(endpoint)) => {
            catalog.binding_current()
                && catalog.account_version() == n[1] as u64
                && endpoint.settings_version() == n[3] as u64
                && endpoint_hash(account, &endpoint)? == eh
                && catalog
                    .snapshot()
                    .mailboxes()
                    .iter()
                    .any(|m| m.selectable() && same_name(m.name(), mailbox))
        }
        _ => false,
    };
    Ok(Some(Cursor {
        value: StoredImapUidCursor {
            version: n[0] as u64,
            account_version: n[1] as u64,
            catalog_generation: n[2] as u64,
            endpoint_version: n[3] as u64,
            uid_validity: n[4] as u32,
            uid_next_at_start: n[5] as u32,
            covered_through_uid: n[6] as u32,
            stored_at_utc_ms: n[7],
            binding_current: current,
        },
        endpoint_hash: eh,
    }))
}
pub(super) fn cursor(
    store: &Store,
    account: &str,
    mailbox: &ImapMailboxName,
) -> Result<Option<StoredImapUidCursor>, ErrorCode> {
    input_account(account)?;
    store.mail_transaction(TransactionBehavior::Deferred, |tx| {
        Ok(read_cursor(tx, account, mailbox)?.map(|c| c.value))
    })
}
pub(super) fn metadata(
    store: &Store,
    account: &str,
    mailbox: &ImapMailboxName,
    validity: u32,
    uid: u32,
) -> Result<Option<ImapMessageMetadata>, ErrorCode> {
    input_account(account)?;
    if validity == 0 || uid == 0 {
        return Err(ErrorCode::InvalidInput);
    }
    store.mail_transaction(TransactionBehavior::Deferred,|tx|{
  let raw=db(tx.query_row("SELECT operation_id FROM imap_uid_messages WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4 AND uid=?5",params![account,key(mailbox),encoding(mailbox),i64::from(validity),i64::from(uid)],|r|Ok(text_column(r,0,128)?.to_owned())).optional())?;
  let current=read_cursor(tx,account,mailbox)?;
  let Some(id)=raw else{
   let Some(current)=current else{return Ok(None)};
   if current.value.uid_validity!=validity||current.value.covered_through_uid<uid{return Ok(None)}
   // Absence is only a legitimate UID gap when the immutable covering page
   // also omits it. A deleted child row must not hide behind Option::None.
   let mut statement=db(tx.prepare("SELECT operation_id FROM imap_uid_page_operations WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4 AND covered_through_uid>=?5 AND covered_through_uid<=?6 AND after_uid<?5 LIMIT 2"))?;
   let upper=(u64::from(uid)+63).min(u64::from(u32::MAX)-1) as i64;
   let ids=db(db(statement.query_map(params![account,key(mailbox),encoding(mailbox),i64::from(validity),i64::from(uid),upper],|r|Ok(text_column(r,0,128)?.to_owned())))?.collect::<rusqlite::Result<Vec<_>>>())?;
   if ids.len()!=1{return Err(ErrorCode::InternalError)}
   let operation=read_operation(tx,&ids[0])?.ok_or(ErrorCode::InternalError)?;
   if operation.receipt.cursor_version>current.value.version||operation.page.messages().iter().any(|m|m.uid()==uid){return Err(ErrorCode::InternalError)}
   return Ok(None)
  };
  let current=current.ok_or(ErrorCode::InternalError)?;
  let op=read_operation(tx,&id)?.ok_or(ErrorCode::InternalError)?;
  if op.page.request().account_id()!=account||!same_name(op.page.request().mailbox(),mailbox)||op.page.uid_validity()!=validity||current.value.uid_validity!=validity||current.value.covered_through_uid<uid{return Err(ErrorCode::InternalError)}
  Ok(Some(op.page.messages().iter().find(|m|m.uid()==uid).ok_or(ErrorCode::InternalError)?.clone()))
 })
}
pub(crate) fn validate_history(c: &Connection) -> rusqlite::Result<()> {
    for (table, limit) in [
        ("imap_uid_cursors", 100000),
        ("imap_uid_page_operations", 100000),
        ("imap_uid_messages", 1000000),
    ] {
        let count: i64 = c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
        if count > limit {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    // The parent Store already checks global foreign keys before invoking this helper.
    Ok(())
}

pub(super) fn apply(
    store: &Store,
    av: u64,
    cg: u64,
    endpoint: &ImapEndpointConfig,
    page: &ImapUidPage,
    mutation: &MailMutation,
) -> Result<ImapUidApplyReceipt, ErrorCode> {
    store.mail_transaction(TransactionBehavior::Immediate, |tx| {
        if crate::uid_page_jobs::operation_reserved(
            tx,
            page.request().account_id(),
            mutation.operation_id(),
            mutation.idempotency_key(),
        )? || crate::uid_reconcile_jobs::operation_reserved(
            tx,
            page.request().account_id(),
            mutation.operation_id(),
            mutation.idempotency_key(),
        )? {
            return Err(ErrorCode::Conflict);
        }
        apply_tx(tx, av, cg, endpoint, page, mutation)
    })
}

/// The caller owns the transaction and has resolved operation/job ownership.
pub(crate) fn apply_tx(
    tx: &Transaction<'_>,
    av: u64,
    cg: u64,
    endpoint: &ImapEndpointConfig,
    page: &ImapUidPage,
    mutation: &MailMutation,
) -> Result<ImapUidApplyReceipt, ErrorCode> {
    let payload = payload_hash(av, cg, endpoint, page, mutation.expected_version())?;
    if &payload != mutation.payload_hash() {
        return Err(ErrorCode::InvalidInput);
    }
    let json = String::from_utf8(
        page.to_storage_json()
            .map_err(|_| ErrorCode::InvalidInput)?,
    )
    .map_err(|_| ErrorCode::InvalidInput)?;
    let page_hash = hash(json.as_bytes())?;
    let request = page.request();
    let account = request.account_id();
    let mailbox = request.mailbox();
    let mailbox_key = key(mailbox);
    let enc = encoding(mailbox);
    let eh = endpoint_hash(account, endpoint)?;
    let ids = {
        let mut stmt=db(tx.prepare("SELECT operation_id FROM imap_uid_page_operations WHERE operation_id=?1 OR (account_id=?2 AND idempotency_key=?3) LIMIT 2"))?;
        db(db(stmt.query_map(
            params![mutation.operation_id(), account, mutation.idempotency_key()],
            |r| Ok(text_column(r, 0, 128)?.to_owned()),
        ))?
        .collect::<rusqlite::Result<Vec<_>>>())?
    };
    if !ids.is_empty() {
        if ids.len() != 1 {
            return Err(ErrorCode::Conflict);
        }
        let op = read_operation(tx, &ids[0])?.ok_or(ErrorCode::InternalError)?;
        if op.id != mutation.operation_id()
            || op.key != mutation.idempotency_key()
            || op.page != *page
            || op.json != json
            || op.payload_hash != payload
            || op.av != av
            || op.cg != cg
            || op.expected != mutation.expected_version()
            || op.endpoint_hash != eh
        {
            return Err(ErrorCode::Conflict);
        }
        let current = read_cursor(tx, account, mailbox)?.ok_or(ErrorCode::InternalError)?;
        if current.value.version < op.receipt.cursor_version {
            return Err(ErrorCode::InternalError);
        }
        return Ok(op.receipt);
    }
    check_account(tx, account)?;
    let catalog = load_catalog(tx, account)?.ok_or(ErrorCode::NotFound)?;
    if catalog.generation() != cg
        || !catalog.binding_current()
        || catalog.account_version() != av
        || catalog.endpoint_version() != endpoint.settings_version()
    {
        return Err(ErrorCode::Conflict);
    }
    if !catalog
        .snapshot()
        .mailboxes()
        .iter()
        .any(|m| m.selectable() && same_name(m.name(), mailbox))
    {
        return Err(ErrorCode::Conflict);
    }
    let current = current_endpoint(tx, account)?.ok_or(ErrorCode::Conflict)?;
    if current != *endpoint {
        return Err(ErrorCode::Conflict);
    }
    let previous = read_cursor(tx, account, mailbox)?;
    match &previous {
        None => {
            if mutation.expected_version() != 0
                || request.expected_uid_validity().is_some()
                || request.after_uid() != 0
            {
                return Err(ErrorCode::Conflict);
            }
        }
        Some(old) => {
            if mutation.expected_version() != old.value.version
                || request.expected_uid_validity() != Some(old.value.uid_validity)
                || request.after_uid() != old.value.covered_through_uid
                || page.uid_validity() != old.value.uid_validity
                || page.uid_next_at_start() < old.value.uid_next_at_start
                || old.endpoint_hash != eh
                || old.value.endpoint_version != endpoint.settings_version()
            {
                return Err(ErrorCode::Conflict);
            }
        }
    }
    let (cursors,operations,messages):(i64,i64,i64)=db(tx.query_row("SELECT (SELECT count(*) FROM imap_uid_cursors),(SELECT count(*) FROM imap_uid_page_operations),(SELECT count(*) FROM imap_uid_messages)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))))?;
    if cursors > 100000 || operations > 100000 || messages > 1000000 {
        return Err(ErrorCode::InternalError);
    }
    if (previous.is_none() && cursors == 100000)
        || operations == 100000
        || messages + page.messages().len() as i64 > 1000000
    {
        return Err(ErrorCode::QuotaExceeded);
    }
    let overlapping:i64=db(tx.query_row("SELECT count(*) FROM imap_uid_messages WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4 AND uid>?5 AND uid<=?6",params![account,mailbox_key,enc,i64::from(page.uid_validity()),i64::from(request.after_uid()),i64::from(page.covered_through_uid())],|r|r.get(0)))?;
    if overlapping > 0 {
        return Err(ErrorCode::Conflict);
    }
    let at = db(crate::utc_millis())?;
    if previous
        .as_ref()
        .is_some_and(|old| old.value.stored_at_utc_ms > at)
    {
        return Err(ErrorCode::InternalError);
    }
    let next = mutation.expected_version() + 1;
    db(tx.execute("INSERT INTO imap_uid_page_operations(operation_id,account_id,idempotency_key,mailbox_key,mailbox_encoding,expected_cursor_version,result_cursor_version,account_version,catalog_generation,endpoint_version,endpoint_hash,page_json,page_hash,payload_hash,uid_validity,uid_next_at_start,after_uid,covered_through_uid,inserted_count,applied_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",params![mutation.operation_id(),account,mutation.idempotency_key(),mailbox_key,enc,mutation.expected_version() as i64,next as i64,av as i64,cg as i64,endpoint.settings_version() as i64,eh.as_slice(),json,page_hash.as_slice(),payload.as_slice(),i64::from(page.uid_validity()),i64::from(page.uid_next_at_start()),i64::from(request.after_uid()),i64::from(page.covered_through_uid()),page.messages().len() as i64,at]))?;
    let values = params![
        account,
        mailbox_key,
        enc,
        next as i64,
        av as i64,
        cg as i64,
        endpoint.settings_version() as i64,
        eh.as_slice(),
        i64::from(page.uid_validity()),
        i64::from(page.uid_next_at_start()),
        i64::from(page.covered_through_uid()),
        at,
        mutation.operation_id()
    ];
    if previous.is_none(){
   db(tx.execute("INSERT INTO imap_uid_cursors(account_id,mailbox_key,mailbox_encoding,version,account_version,catalog_generation,endpoint_version,endpoint_hash,uid_validity,uid_next_at_start,covered_through_uid,stored_at_utc_ms,last_operation_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",values))?;
  }else if db(tx.execute("UPDATE imap_uid_cursors SET version=?4,account_version=?5,catalog_generation=?6,endpoint_version=?7,endpoint_hash=?8,uid_validity=?9,uid_next_at_start=?10,covered_through_uid=?11,stored_at_utc_ms=?12,last_operation_id=?13 WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND version=?4-1",values))?!=1{return Err(ErrorCode::InternalError)}
    for message in page.messages() {
        let raw = String::from_utf8(
            message
                .to_storage_json()
                .map_err(|_| ErrorCode::InternalError)?,
        )
        .map_err(|_| ErrorCode::InternalError)?;
        db(tx.execute("INSERT INTO imap_uid_messages(account_id,mailbox_key,mailbox_encoding,uid_validity,uid,metadata_json,metadata_hash,stored_at_utc_ms,operation_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![account,mailbox_key,enc,i64::from(page.uid_validity()),i64::from(message.uid()),raw,hash(raw.as_bytes())?.as_slice(),at,mutation.operation_id()]))?;
    }
    // Validate the same strict reader before committing any of the three tables.
    read_cursor(tx, account, mailbox)?.ok_or(ErrorCode::InternalError)?;
    Ok(ImapUidApplyReceipt {
        cursor_version: next,
        covered_through_uid: page.covered_through_uid(),
        inserted_count: page.messages().len() as u32,
    })
}
