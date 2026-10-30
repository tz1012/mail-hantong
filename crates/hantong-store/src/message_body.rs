//! Account/namespace-bound SQLCipher body cache; only Core may call this repository.
use crate::Store;
use hantong_contracts::{
    ErrorCode,
    message_body::{MessageBody, MessageBodyRequest},
};
use hantong_domain::mail_connection::ImapEndpointConfig;

impl Store {
    pub fn locate_message_body(
        &self,
        account: &str,
        message: &str,
        max_bytes: usize,
    ) -> Result<MessageBodyRequest, ErrorCode> {
        if account.is_empty()
            || account.len() > 128
            || !account
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || message.len() != 68
            || !message.starts_with("msg_")
            || !message[4..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ErrorCode::InvalidInput);
        }
        #[cfg(windows)]
        {
            self.with_connection(|c| Ok(storage::locate(c, account, message, max_bytes)))?
        }
        #[cfg(not(windows))]
        {
            let _ = max_bytes;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }

    pub fn body_endpoint(
        &self,
        request: &MessageBodyRequest,
    ) -> Result<ImapEndpointConfig, ErrorCode> {
        #[cfg(windows)]
        {
            self.with_connection(|c| Ok(storage::binding(c, request)))?
        }
        #[cfg(not(windows))]
        {
            let _ = request;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn cached_message_body(
        &self,
        request: &MessageBodyRequest,
    ) -> Result<Option<MessageBody>, ErrorCode> {
        #[cfg(windows)]
        {
            self.with_connection(|c| Ok(storage::cached(c, request)))?
        }
        #[cfg(not(windows))]
        {
            let _ = request;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn cache_message_body(
        &self,
        body: &MessageBody,
        endpoint_version: u64,
    ) -> Result<(), ErrorCode> {
        #[cfg(windows)]
        {
            self.with_connection(|c| Ok(storage::put(c, body, endpoint_version)))?
        }
        #[cfg(not(windows))]
        {
            let _ = (body, endpoint_version);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}
#[cfg(windows)]
mod storage {
    use super::*;
    use hantong_contracts::{uid_page::UidMailboxEncoding, wire_hex};
    use hantong_domain::mailboxes::{ImapMailboxName, MailboxEncoding};
    use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
    fn db<T>(r: rusqlite::Result<T>) -> Result<T, ErrorCode> {
        r.map_err(|_| ErrorCode::InternalError)
    }
    fn encoding(r: &MessageBodyRequest) -> &'static str {
        match r.mailbox().encoding() {
            UidMailboxEncoding::Utf8 => "utf8",
            UidMailboxEncoding::ModifiedUtf7 => "modified_utf7",
        }
    }
    pub(super) fn locate(
        c: &Connection,
        account: &str,
        message: &str,
        max: usize,
    ) -> Result<MessageBodyRequest, ErrorCode> {
        let mut statement=db(c.prepare("SELECT mailbox_key,mailbox_encoding,uid_validity,uid FROM imap_message_identities WHERE account_id=?1 AND internal_message_id=?2 UNION SELECT mailbox_key,mailbox_encoding,uid_validity,uid FROM imap_initial_preview_messages WHERE account_id=?1 AND internal_message_id=?2 LIMIT 2"))?;
        let rows = db(statement.query_map(params![account, message], |r| {
            Ok((
                crate::registrations::text_column(r, 0, 2048)?.to_owned(),
                crate::registrations::text_column(r, 1, 13)?.to_owned(),
                r.get::<_, u32>(2)?,
                r.get::<_, u32>(3)?,
            ))
        }))?;
        let values = db(rows.collect::<rusqlite::Result<Vec<_>>>())?;
        if values.len() > 1 {
            return Err(ErrorCode::InternalError);
        }
        let row = values.into_iter().next().ok_or(ErrorCode::NotFound)?;
        let request = MessageBodyRequest::new(
            account,
            hantong_contracts::uid_page::UidMailboxIdentity::new(
                &row.0,
                UidMailboxEncoding::parse(&row.1)?,
            )?,
            row.2,
            row.3,
            max,
        )?;
        let mailbox = ImapMailboxName::from_wire(
            &wire_hex::decode_bounded(&row.0, 1024)?,
            match request.mailbox().encoding() {
                UidMailboxEncoding::Utf8 => MailboxEncoding::Utf8,
                UidMailboxEncoding::ModifiedUtf7 => MailboxEncoding::ModifiedUtf7,
            },
        )
        .map_err(|_| ErrorCode::InternalError)?;
        if crate::message_identities::location_id(account, &mailbox, row.2, row.3)? != message {
            return Err(ErrorCode::InternalError);
        }
        binding(c, &request)?;
        Ok(request)
    }
    pub(super) fn binding(
        c: &Connection,
        r: &MessageBodyRequest,
    ) -> Result<ImapEndpointConfig, ErrorCode> {
        crate::mail_connection::check_account(c, r.account_id())?;
        let endpoint = crate::imap_sync::storage::current_endpoint(c, r.account_id())?
            .ok_or(ErrorCode::NotFound)?;
        let name = ImapMailboxName::from_wire(
            &wire_hex::decode_bounded(r.mailbox().wire_hex(), 1024)?,
            match r.mailbox().encoding() {
                UidMailboxEncoding::Utf8 => MailboxEncoding::Utf8,
                UidMailboxEncoding::ModifiedUtf7 => MailboxEncoding::ModifiedUtf7,
            },
        )
        .map_err(|_| ErrorCode::InvalidInput)?;
        if wire_hex::encode(name.wire_name()) != r.mailbox().wire_hex() {
            return Err(ErrorCode::InvalidInput);
        }
        let cursor = crate::imap_sync::storage::read_cursor(c, r.account_id(), &name)?;
        let preview =
            crate::initial_sync::storage::current_preview(c, r.account_id(), r.mailbox())?;
        match cursor {
            Some(cursor) => {
                if !cursor.value.binding_current()
                    || cursor.value.uid_validity() != r.uidvalidity()
                    || cursor.value.endpoint_version() != endpoint.settings_version()
                {
                    return Err(ErrorCode::Conflict);
                }
                if preview.as_ref().is_some_and(|p| {
                    p.snapshot.stored_at_utc_ms() >= cursor.value.stored_at_utc_ms()
                        && p.snapshot.progress().uid_validity() != r.uidvalidity()
                }) {
                    return Err(ErrorCode::Conflict);
                }
            }
            None => {
                let preview = preview.ok_or(ErrorCode::NotFound)?;
                if !preview.binding_current(c)?
                    || preview.snapshot.progress().uid_validity() != r.uidvalidity()
                {
                    return Err(ErrorCode::Conflict);
                }
            }
        }
        Ok(endpoint)
    }
    pub(super) fn cached(
        c: &Connection,
        r: &MessageBodyRequest,
    ) -> Result<Option<MessageBody>, ErrorCode> {
        let tx = db(Transaction::new_unchecked(c, TransactionBehavior::Deferred))?;
        let endpoint = binding(&tx, r)?;
        let raw=db(tx.query_row("SELECT endpoint_version,body_bytes,body_hash FROM imap_body_cache WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4 AND uid=?5",params![r.account_id(),r.mailbox().wire_hex(),encoding(r),r.uidvalidity(),r.uid()],|row|{let bytes=row.get_ref(1)?.as_blob()?;if bytes.len()>hantong_contracts::message_body::MAX_BODY_BYTES{return Err(rusqlite::Error::InvalidQuery)}let hash=crate::registrations::blob_column::<32>(row,2)?;Ok((row.get::<_,i64>(0)?,bytes.to_vec(),hash.to_vec()))}).optional())?;
        let result = raw
            .map(|(version, bytes, hash)| {
                if u64::try_from(version).ok() != Some(endpoint.settings_version()) {
                    return Err(ErrorCode::Conflict);
                }
                if bytes.len() > hantong_contracts::message_body::MAX_BODY_BYTES
                    || hash.as_slice() != hantong_platform::sha256_storage_payload(&bytes)?
                {
                    return Err(ErrorCode::InternalError);
                }
                if bytes.len() > r.max_bytes() {
                    return Err(ErrorCode::QuotaExceeded);
                }
                MessageBody::new(r.clone(), bytes, false)
            })
            .transpose()?;
        db(tx.commit())?;
        Ok(result)
    }
    pub(super) fn put(c: &Connection, b: &MessageBody, version: u64) -> Result<(), ErrorCode> {
        let tx = db(Transaction::new_unchecked(
            c,
            TransactionBehavior::Immediate,
        ))?;
        let r = b.key();
        if binding(&tx, r)?.settings_version() != version {
            return Err(ErrorCode::Conflict);
        }
        let hash = hantong_platform::sha256_storage_payload(b.bytes())?;
        db(tx.execute("INSERT INTO imap_body_cache(account_id,mailbox_key,mailbox_encoding,uid_validity,uid,endpoint_version,body_bytes,body_hash,stored_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(account_id,mailbox_key,mailbox_encoding,uid_validity,uid) DO NOTHING",params![r.account_id(),r.mailbox().wire_hex(),encoding(r),r.uidvalidity(),r.uid(),i64::try_from(version).map_err(|_|ErrorCode::InvalidInput)?,b.bytes(),hash.as_slice(),crate::utc_millis().map_err(|_|ErrorCode::InternalError)?]))?;
        let existing:(Vec<u8>,Vec<u8>)=db(tx.query_row("SELECT body_bytes,body_hash FROM imap_body_cache WHERE account_id=?1 AND mailbox_key=?2 AND mailbox_encoding=?3 AND uid_validity=?4 AND uid=?5",params![r.account_id(),r.mailbox().wire_hex(),encoding(r),r.uidvalidity(),r.uid()],|row|Ok((row.get(0)?,row.get(1)?))))?;
        if existing.0 != b.bytes() || existing.1 != hash {
            return Err(ErrorCode::Conflict);
        }
        db(tx.commit())?;
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::uid_reconcile_jobs::tests::{Fixture, append, identity};
    fn request(validity: u32) -> MessageBodyRequest {
        MessageBodyRequest::new("acct", identity(b"INBOX"), validity, 1, 1024).unwrap()
    }
    #[test]
    fn mail03_body_cache_is_bound_to_current_namespace_endpoint_and_bytes() {
        let f = Fixture::new();
        append(&f, 0, 0, 11, &[1], "body-base");
        let r = request(7);
        assert!(f.store().cached_message_body(&r).unwrap().is_none());
        let body = MessageBody::new(r.clone(), b"Hello".to_vec(), false).unwrap();
        f.store().cache_message_body(&body, 1).unwrap();
        assert_eq!(
            f.store().cached_message_body(&r).unwrap().unwrap().bytes(),
            b"Hello"
        );
        assert_eq!(
            f.store().cached_message_body(&request(8)),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            f.store().cache_message_body(&body, 2),
            Err(ErrorCode::Conflict)
        );
        let changed = MessageBody::new(r.clone(), b"Changed".to_vec(), false).unwrap();
        assert_eq!(
            f.store().cache_message_body(&changed, 1),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            f.store().cached_message_body(&r).unwrap().unwrap().bytes(),
            b"Hello"
        );
    }
    #[test]
    fn sec08_body_cache_rejects_tampering_before_return() {
        let f = Fixture::new();
        append(&f, 0, 0, 11, &[1], "body-base");
        let r = request(7);
        let body = MessageBody::new(r.clone(), b"Hello".to_vec(), false).unwrap();
        f.store().cache_message_body(&body, 1).unwrap();
        f.store()
            .with_connection(|c| c.execute("UPDATE imap_body_cache SET body_bytes=x'01'", []))
            .unwrap();
        assert_eq!(
            f.store().cached_message_body(&r),
            Err(ErrorCode::InternalError)
        );
    }
}
