use super::*;
use crate::{
    imap_sync::storage::{Cursor, current_endpoint, read_cursor, read_operation},
    registrations::{blob_column, text_column},
    uid_epochs::Epoch,
    uid_reconcile_jobs::SearchHead,
};
use hantong_contracts::message_search::{
    HeaderAddressProjection, HeaderProjection, ImapMetadataFilter, ImapMetadataSearchCursor,
    ImapMetadataSearchItem,
};
use hantong_domain::{
    accounts::{AccountProvider, AccountStatus},
    imap_sync::ImapMessageMetadata,
    mailboxes::ImapMailboxName,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    r.map_err(|_| ErrorCode::InternalError)
}
fn frame(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ErrorCode> {
    out.extend_from_slice(
        &u32::try_from(bytes.len())
            .map_err(|_| ErrorCode::InternalError)?
            .to_be_bytes(),
    );
    out.extend_from_slice(bytes);
    Ok(())
}
fn hash(bytes: &[u8]) -> Result<[u8; 32], ErrorCode> {
    hantong_platform::sha256(bytes).map_err(|_| ErrorCode::InternalError)
}
fn missing(r: &ImapMetadataSearchRequest) -> ErrorCode {
    if r.cursor().is_some() {
        ErrorCode::CursorExpired
    } else {
        ErrorCode::NotFound
    }
}

struct Account {
    provider: AccountProvider,
    status: AccountStatus,
    version: u64,
}
fn account(c: &Connection, id: &str) -> Result<Option<Account>, ErrorCode> {
    let raw = db(c
        .query_row(
            "SELECT provider,status,settings_version FROM accounts WHERE account_id=?1",
            [id],
            |r| {
                Ok((
                    text_column(r, 0, 16)?.to_owned(),
                    text_column(r, 1, 32)?.to_owned(),
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .optional())?;
    raw.map(|(provider, status, version)| {
        if version <= 0 {
            return Err(ErrorCode::InternalError);
        }
        Ok(Account {
            provider: AccountProvider::parse(&provider).map_err(|_| ErrorCode::InternalError)?,
            status: AccountStatus::parse(&status).map_err(|_| ErrorCode::InternalError)?,
            version: version as u64,
        })
    })
    .transpose()
}
struct Snapshot {
    cursor: Cursor,
    epoch: Epoch,
    head: Option<SearchHead>,
    prefix: Vec<u8>,
    catalog_generation: u64,
    binding_current: bool,
}
struct Source {
    id: String,
    page_hash: [u8; 32],
    version: u64,
    validity: u32,
    endpoint_version: u64,
    endpoint_hash: [u8; 32],
    after: u32,
    covered: u32,
    next: u32,
    count: u32,
    at: i64,
}
fn source(
    c: &Connection,
    r: &ImapMetadataSearchRequest,
    version: u64,
) -> Result<Option<Source>, ErrorCode> {
    let raw=db(c.query_row("SELECT operation_id,page_hash,result_cursor_version,uid_validity,endpoint_version,endpoint_hash,after_uid,covered_through_uid,uid_next_at_start,inserted_count,applied_at_utc_ms FROM imap_uid_page_operations WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND result_cursor_version=?4",params![r.account_id(),r.mailbox().wire_hex(),r.mailbox().encoding().as_str(),i64::try_from(version).map_err(|_|ErrorCode::InternalError)?],|row|Ok((text_column(row,0,128)?.to_owned(),blob_column::<32>(row,1)?,row.get::<_,i64>(2)?,row.get::<_,u32>(3)?,row.get::<_,i64>(4)?,blob_column::<32>(row,5)?,row.get::<_,u32>(6)?,row.get::<_,u32>(7)?,row.get::<_,u32>(8)?,row.get::<_,u32>(9)?,row.get::<_,i64>(10)?))).optional())?;
    raw.map(
        |(id, page_hash, v, validity, ev, eh, after, covered, next, count, at)| {
            crate::mail_connection::input_account(&id).map_err(|_| ErrorCode::InternalError)?;
            if v <= 0
                || v as u64 != version
                || validity == 0
                || ev <= 0
                || after > covered
                || covered >= next
                || covered - after > 64
                || count > 64
                || count > covered - after
                || at <= 0
            {
                return Err(ErrorCode::InternalError);
            }
            Ok(Source {
                id,
                page_hash,
                version,
                validity,
                endpoint_version: ev as u64,
                endpoint_hash: eh,
                after,
                covered,
                next,
                count,
                at,
            })
        },
    )
    .transpose()
}
fn source_binding(s: &Source, snapshot: &Snapshot) -> Result<(), ErrorCode> {
    if s.version < snapshot.epoch.first
        || s.version > snapshot.cursor.value.version()
        || s.validity != snapshot.epoch.validity
        || s.endpoint_version != snapshot.epoch.endpoint_version
        || s.endpoint_hash != snapshot.epoch.endpoint_hash
        || s.at < snapshot.epoch.created
        || s.at > snapshot.cursor.value.stored_at_utc_ms()
    {
        return Err(ErrorCode::InternalError);
    }
    Ok(())
}
fn token(
    snapshot: &Snapshot,
    source: &Source,
    resume: u32,
    expiry: i64,
) -> Result<ImapMetadataSearchCursor, ErrorCode> {
    let mut bytes = snapshot.prefix.clone();
    frame(&mut bytes, source.id.as_bytes())?;
    bytes.extend_from_slice(&source.page_hash);
    bytes.extend_from_slice(&source.version.to_be_bytes());
    bytes.extend_from_slice(&resume.to_be_bytes());
    bytes.extend_from_slice(&expiry.to_be_bytes());
    ImapMetadataSearchCursor::parse(&format!(
        "m1:{}:{}:{}:{}",
        hantong_contracts::wire_hex::encode(&hash(&bytes)?),
        source.version,
        resume,
        expiry
    ))
    .map_err(|_| ErrorCode::InternalError)
}
fn projection(m: &ImapMessageMetadata) -> Result<HeaderProjection, ErrorCode> {
    let fields = m.envelope().fields();
    let from = fields
        .from
        .as_ref()
        .map(|values| {
            values
                .iter()
                .map(|a| {
                    HeaderAddressProjection::new(
                        a.host().map(<[u8]>::to_vec),
                        a.mailbox().map(<[u8]>::to_vec),
                        a.name().map(<[u8]>::to_vec),
                        a.route().map(<[u8]>::to_vec),
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()
        .map_err(|_| ErrorCode::InternalError)?;
    HeaderProjection::new(fields.subject.clone(), from).map_err(|_| ErrorCode::InternalError)
}
fn no_history(c: &Connection, r: &ImapMetadataSearchRequest) -> Result<(), ErrorCode> {
    let exists:bool=db(c.query_row("SELECT EXISTS(SELECT 1 FROM imap_uid_epochs WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3) OR EXISTS(SELECT 1 FROM imap_uid_page_operations WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3) OR EXISTS(SELECT 1 FROM imap_uid_messages WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3) OR EXISTS(SELECT 1 FROM imap_message_identities WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3) OR EXISTS(SELECT 1 FROM imap_uid_reconcile_heads WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3) OR EXISTS(SELECT 1 FROM imap_uid_reconcile_requests WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3) OR EXISTS(SELECT 1 FROM imap_uid_reconcile_messages WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3) OR EXISTS(SELECT 1 FROM imap_uid_recovery_requests WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3) OR EXISTS(SELECT 1 FROM imap_uid_page_requests WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND (expected_cursor_version>0 OR state='succeeded'))",params![r.account_id(),r.mailbox().wire_hex(),r.mailbox().encoding().as_str()],|row|row.get(0)))?;
    if exists {
        return Err(ErrorCode::InternalError);
    }
    Ok(())
}
pub(super) fn search(
    store: &Store,
    r: &ImapMetadataSearchRequest,
) -> Result<ImapMetadataSearchPage, ErrorCode> {
    crate::mail_connection::input_account(r.account_id())?;
    let name = crate::uid_page_jobs::storage::mailbox(r.mailbox())?;
    store.mail_transaction(TransactionBehavior::Deferred, |tx| {
        let account = account(tx, r.account_id())?;
        let endpoint = current_endpoint(tx, r.account_id())?;
        let catalog = crate::imap_catalog::load_catalog(tx, r.account_id())?;
        let (Some(account), Some(endpoint), Some(catalog)) = (account, endpoint, catalog) else {
            return Err(missing(r));
        };
        if !catalog.snapshot().mailboxes().iter().any(|entry| {
            entry.selectable()
                && entry.name().wire_name() == name.wire_name()
                && entry.name().encoding() == name.encoding()
        }) {
            return Err(missing(r));
        }
        let Some(cursor) = read_cursor(tx, r.account_id(), &name)? else {
            no_history(tx, r)?;
            if r.cursor().is_some() {
                return Err(ErrorCode::CursorExpired);
            }
            return ImapMetadataSearchPage::new(
                r.account_id(),
                r.mailbox().clone(),
                catalog.generation(),
                catalog.binding_current(),
                None,
                vec![],
                vec![],
                None,
            )
            .map_err(|_| ErrorCode::InternalError);
        };
        let epoch =
            crate::uid_epochs::read(tx, r.account_id(), &name, cursor.value.uid_validity())?
                .ok_or(ErrorCode::InternalError)?;
        if epoch.last.is_some() {
            return Err(ErrorCode::InternalError);
        }
        let head = crate::uid_reconcile_jobs::search_head(
            tx,
            r.account_id(),
            r.mailbox(),
            epoch.validity,
        )?;
        let endpoint_hash = crate::imap_catalog::endpoint_hash(r.account_id(), &endpoint)?;
        let catalog_hash = db(tx.query_row(
            "SELECT snapshot_hash FROM imap_mailbox_catalogs WHERE account_id=?1",
            [r.account_id()],
            |row| blob_column::<32>(row, 0),
        ))?;
        let binding_current = catalog.binding_current() && cursor.value.binding_current();
        let mut prefix = b"mail-hantong.imap-metadata-search-cursor.v1\0".to_vec();
        for bytes in [
            r.account_id().as_bytes(),
            name.wire_name(),
            r.mailbox().encoding().as_str().as_bytes(),
            account.status.as_str().as_bytes(),
            account.provider.as_str().as_bytes(),
        ] {
            frame(&mut prefix, bytes)?;
        }
        prefix.extend_from_slice(&account.version.to_be_bytes());
        prefix.extend_from_slice(&endpoint.settings_version().to_be_bytes());
        prefix.extend_from_slice(&endpoint_hash);
        prefix.extend_from_slice(&catalog.generation().to_be_bytes());
        prefix.extend_from_slice(&catalog_hash);
        for n in [
            cursor.value.account_version(),
            cursor.value.catalog_generation(),
            cursor.value.endpoint_version(),
        ] {
            prefix.extend_from_slice(&n.to_be_bytes());
        }
        prefix.extend_from_slice(&cursor.endpoint_hash);
        prefix.extend_from_slice(&cursor.value.version().to_be_bytes());
        prefix.extend_from_slice(&cursor.value.stored_at_utc_ms().to_be_bytes());
        prefix.extend_from_slice(&epoch.validity.to_be_bytes());
        prefix.extend_from_slice(&epoch.generation.to_be_bytes());
        prefix.extend_from_slice(&epoch.first.to_be_bytes());
        if let Some(head) = &head {
            prefix.push(1);
            prefix.extend_from_slice(&head.generation().to_be_bytes());
            prefix.extend_from_slice(&head.version().to_be_bytes());
        } else {
            prefix.push(0);
        }
        prefix.push(u8::from(binding_current));
        frame(&mut prefix, r.filter().as_str().as_bytes())?;
        prefix.extend_from_slice(&u64::from(r.limit()).to_be_bytes());
        let snapshot = Snapshot {
            cursor,
            epoch,
            head,
            prefix,
            catalog_generation: catalog.generation(),
            binding_current,
        };
        consume(tx, r, &name, &snapshot)
    })
}

fn consume(
    c: &Connection,
    r: &ImapMetadataSearchRequest,
    name: &ImapMailboxName,
    snapshot: &Snapshot,
) -> Result<ImapMetadataSearchPage, ErrorCode> {
    let now = db(crate::utc_millis())?;
    let (version, resume, expiry) = if let Some(cursor) = r.cursor() {
        if now >= cursor.expires_at_utc_ms() {
            return Err(ErrorCode::CursorExpired);
        }
        (
            cursor.source_cursor_version(),
            cursor.resume_uid(),
            cursor.expires_at_utc_ms(),
        )
    } else {
        (
            snapshot.cursor.value.version(),
            u32::MAX,
            now.checked_add(300_000).ok_or(ErrorCode::InternalError)?,
        )
    };
    if version < snapshot.epoch.first || version > snapshot.cursor.value.version() {
        return Err(ErrorCode::CursorExpired);
    }
    let selected = source(c, r, version)?.ok_or(ErrorCode::InternalError)?;
    source_binding(&selected, snapshot)?;
    let previous = if version > snapshot.epoch.first {
        let old = source(c, r, version - 1)?.ok_or(ErrorCode::InternalError)?;
        source_binding(&old, snapshot)?;
        if old.covered != selected.after || old.next > selected.next || old.at > selected.at {
            return Err(ErrorCode::InternalError);
        }
        Some(old)
    } else {
        if selected.after != 0 {
            return Err(ErrorCode::InternalError);
        }
        None
    };
    if version < snapshot.cursor.value.version() {
        let next = source(c, r, version + 1)?.ok_or(ErrorCode::InternalError)?;
        source_binding(&next, snapshot)?;
        if selected.covered != next.after || selected.next > next.next || selected.at > next.at {
            return Err(ErrorCode::InternalError);
        }
    }
    let source = read_operation(c, &selected.id)?.ok_or(ErrorCode::InternalError)?;
    let p = &source.page;
    if source.page_hash != selected.page_hash
        || source.receipt.cursor_version() != version
        || p.request().account_id() != r.account_id()
        || p.request().mailbox().wire_name() != name.wire_name()
        || p.request().mailbox().encoding() != name.encoding()
        || p.uid_validity() != selected.validity
        || p.request().endpoint_version() != selected.endpoint_version
        || source.endpoint_hash != selected.endpoint_hash
        || p.request().after_uid() != selected.after
        || p.covered_through_uid() != selected.covered
        || p.uid_next_at_start() != selected.next
        || p.messages().len() != selected.count as usize
        || source.at != selected.at
    {
        return Err(ErrorCode::InternalError);
    }
    let count:i64=db(c.query_row("SELECT count(*) FROM imap_uid_messages WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4 AND uid>?5 AND uid<=?6",params![r.account_id(),r.mailbox().wire_hex(),r.mailbox().encoding().as_str(),i64::from(selected.validity),i64::from(selected.after),i64::from(selected.covered)],|row|row.get(0)))?;
    if count != i64::from(selected.count) {
        return Err(ErrorCode::InternalError);
    }
    let ids = crate::message_identities::validate_page(c, p)?;
    if resume != u32::MAX
        && (!p.messages().iter().any(|m| m.uid() == resume)
            || !p.messages().iter().any(|m| m.uid() < resume))
    {
        return Err(ErrorCode::CursorExpired);
    }
    if let Some(cursor) = r.cursor()
        && &token(snapshot, &selected, resume, expiry)? != cursor
    {
        return Err(ErrorCode::CursorExpired);
    }
    let view = crate::uid_reconcile_jobs::search_source(c, source, snapshot.head.as_ref())?;
    let mut items = Vec::new();
    let mut headers = Vec::new();
    let mut bytes = 8192usize;
    let mut checkpoint = resume;
    let mut remaining = false;
    for (index, message) in view
        .source
        .page
        .messages()
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, m)| m.uid() < resume)
    {
        if items.len() >= r.limit() as usize {
            remaining = true;
            break;
        }
        let flags = match &view.flags {
            None => Some(message.flags()),
            Some(values) => values[index].as_deref(),
        };
        let Some(flags) = flags else {
            checkpoint = message.uid();
            continue;
        };
        let read = flags.iter().any(|f| f == "\\seen");
        let favorite = flags.iter().any(|f| f == "\\flagged");
        if (r.filter() == ImapMetadataFilter::Unread && read)
            || (r.filter() == ImapMetadataFilter::Favorite && !favorite)
        {
            checkpoint = message.uid();
            continue;
        }
        let header = projection(message)?;
        let weight = header
            .projection_json()
            .len()
            .checked_mul(2)
            .and_then(|n| n.checked_add(4096))
            .ok_or(ErrorCode::QuotaExceeded)?;
        let total = bytes.checked_add(weight).ok_or(ErrorCode::QuotaExceeded)?;
        if total > 524_288 {
            if items.is_empty() {
                return Err(ErrorCode::QuotaExceeded);
            }
            remaining = true;
            break;
        }
        items.push(
            ImapMetadataSearchItem::new(
                &ids[index],
                r.account_id(),
                message.uid(),
                message.internal_date_utc_ms(),
                message.size(),
                read,
                favorite,
                items.len() as u32,
            )
            .map_err(|_| ErrorCode::InternalError)?,
        );
        headers.push(header);
        bytes = total;
        checkpoint = message.uid();
    }
    let next = if remaining {
        Some(token(snapshot, &selected, checkpoint, expiry)?)
    } else {
        previous
            .as_ref()
            .map(|old| token(snapshot, old, u32::MAX, expiry))
            .transpose()?
    };
    ImapMetadataSearchPage::new(
        r.account_id(),
        r.mailbox().clone(),
        snapshot.catalog_generation,
        snapshot.binding_current,
        Some(snapshot.cursor.value.stored_at_utc_ms()),
        items,
        headers,
        next,
    )
    .map_err(|_| ErrorCode::InternalError)
}
