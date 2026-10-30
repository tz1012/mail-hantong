use super::*;

/// Scalar descriptor. Reading this does not decode immutable page/header JSON.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct PreviewSource {
    pub(crate) ordinal: u8,
    pub(crate) execution_id: [u8; 32],
    pub(crate) after_uid: u32,
    pub(crate) through_uid: u32,
    pub(crate) page_hash: [u8; 32],
    pub(crate) inserted_count: u8,
    pub(crate) stored_at_utc_ms: i64,
}
pub(crate) struct StoredPreviewPage {
    pub(crate) page: ImapUidReconcilePage,
    pub(crate) message_ids: Vec<String>,
}
pub(crate) fn preview_source(
    c: &Connection,
    p: &PreviewPlan,
    ordinal: u8,
) -> Result<Option<PreviewSource>, ErrorCode> {
    if ordinal >= p.progress().completed_page_count() {
        return Ok(None);
    }
    let raw=db(c.query_row("SELECT execution_id,after_uid,covered_through_uid,page_hash,inserted_count,stored_at_utc_ms,(SELECT count(*) FROM imap_initial_preview_messages m WHERE m.plan_id=x.plan_id AND m.ordinal=x.ordinal) FROM imap_initial_preview_pages x WHERE plan_id=?1 AND ordinal=?2",params![p.id().as_slice(),i64::from(ordinal)],|r|Ok((blob_column::<32>(r,0)?,r.get::<_,u32>(1)?,r.get::<_,u32>(2)?,blob_column::<32>(r,3)?,r.get::<_,u8>(4)?,r.get::<_,i64>(5)?,u64_column(r,6)?))).optional())?;
    let Some((execution, after, through, digest, count, at, children)) = raw else {
        return Err(ErrorCode::InternalError);
    };
    let step = range(p, ordinal)?;
    let row = read_row(c, execution)?.ok_or(ErrorCode::InternalError)?;
    if row.state != InitialSyncJobState::Succeeded
        || row.request.action() != InitialSyncAction::Resume
        || row.plan != Some(*p.id())
        || row.step.map(|s| (s.1, s.2, s.3)) != Some((ordinal, after, through))
        || row.completed != Some(at)
        || step != (after, through)
        || count > 64
        || children != u64::from(count)
        || at < p.created
        || at > p.snapshot.stored_at_utc_ms()
    {
        return Err(ErrorCode::InternalError);
    }
    Ok(Some(PreviewSource {
        ordinal,
        execution_id: execution,
        after_uid: after,
        through_uid: through,
        page_hash: digest,
        inserted_count: count,
        stored_at_utc_ms: at,
    }))
}
fn range(p: &PreviewPlan, ordinal: u8) -> Result<(u32, u32), ErrorCode> {
    let ceiling = u64::from(p.progress().target_uid_ceiling());
    if ceiling == 0 || ordinal >= p.progress().selected_page_count() {
        return Err(ErrorCode::InternalError);
    }
    let block = (ceiling - 1) / 64 - u64::from(ordinal);
    let after = block * 64;
    Ok((after as u32, ceiling.min(after + 64) as u32))
}
pub(crate) fn read_preview_page(
    c: &Connection,
    p: &PreviewPlan,
    source: &PreviewSource,
) -> Result<StoredPreviewPage, ErrorCode> {
    let selected = preview_source(c, p, source.ordinal)?.ok_or(ErrorCode::InternalError)?;
    if selected != *source {
        return Err(ErrorCode::InternalError);
    }
    let json: String = db(c.query_row(
        "SELECT page_json FROM imap_initial_preview_pages WHERE plan_id=?1 AND ordinal=?2",
        params![p.id().as_slice(), i64::from(source.ordinal)],
        |r| Ok(text_column(r, 0, 8388608)?.into()),
    ))?;
    checked_json(&json, &source.page_hash)?;
    let page = ImapUidReconcilePage::from_storage_json(json.as_bytes())
        .map_err(|_| ErrorCode::InternalError)?;
    let row = read_row(c, source.execution_id)?.ok_or(ErrorCode::InternalError)?;
    let request = page.request();
    if request.account_id() != p.snapshot.account_id()
        || request.mailbox() != &mailbox(p.snapshot.mailbox())?
        || request.uid_validity() != p.progress().uid_validity()
        || request.endpoint_version() != row.binding.ev
        || request.after_uid() != source.after_uid
        || request.through_uid() != source.through_uid
        || request.minimum_uid_next() != p.progress().uid_next()
        || page.messages().len() != usize::from(source.inserted_count)
    {
        return Err(ErrorCode::InternalError);
    }
    let mut stmt=db(c.prepare("SELECT uid,internal_message_id,account_id,mailbox_key,mailbox_encoding,uid_validity,metadata_json,metadata_hash FROM imap_initial_preview_messages WHERE plan_id=?1 AND ordinal=?2 ORDER BY uid LIMIT 65"))?;
    let mut rows = db(stmt.query(params![p.id().as_slice(), i64::from(source.ordinal)]))?;
    let mut ids = Vec::with_capacity(page.messages().len());
    for message in page.messages() {
        let r = db(rows.next())?.ok_or(ErrorCode::InternalError)?;
        let uid: u32 = db(r.get(0))?;
        let id = db(text_column(r, 1, 68))?;
        let metadata = db(text_column(r, 6, 8388608))?;
        let digest = db(blob_column::<32>(r, 7))?;
        checked_json(metadata, &digest)?;
        if uid != message.uid()
            || db(text_column(r, 2, 128))? != request.account_id()
            || db(text_column(r, 3, 2048))? != p.snapshot.mailbox().wire_hex()
            || db(text_column(r, 4, 13))? != p.snapshot.mailbox().encoding().as_str()
            || db(r.get::<_, u32>(5))? != request.uid_validity()
            || message
                .to_storage_json()
                .map_err(|_| ErrorCode::InternalError)?
                != metadata.as_bytes()
            || crate::message_identities::location_id(
                request.account_id(),
                request.mailbox(),
                request.uid_validity(),
                uid,
            )? != id
        {
            return Err(ErrorCode::InternalError);
        }
        collision(
            c,
            id,
            request.account_id(),
            p.snapshot.mailbox(),
            request.uid_validity(),
            uid,
        )?;
        ids.push(id.into());
    }
    if db(rows.next())?.is_some() {
        return Err(ErrorCode::InternalError);
    }
    Ok(StoredPreviewPage {
        page,
        message_ids: ids,
    })
}
fn collision(
    c: &Connection,
    id: &str,
    account: &str,
    mailbox: &UidMailboxIdentity,
    validity: u32,
    uid: u32,
) -> Result<(), ErrorCode> {
    let bad:bool=db(c.query_row("SELECT EXISTS(SELECT 1 FROM imap_message_identities WHERE internal_message_id=?1 AND (account_id<>?2 OR mailbox_key<>?3 OR mailbox_encoding<>?4 OR uid_validity<>?5 OR uid<>?6)) OR EXISTS(SELECT 1 FROM imap_message_identities WHERE account_id=?2 AND mailbox_key=?3 AND mailbox_encoding=?4 AND uid_validity=?5 AND uid=?6 AND internal_message_id<>?1) OR EXISTS(SELECT 1 FROM imap_initial_preview_messages WHERE internal_message_id=?1 AND (account_id<>?2 OR mailbox_key<>?3 OR mailbox_encoding<>?4 OR uid_validity<>?5 OR uid<>?6)) OR EXISTS(SELECT 1 FROM imap_initial_preview_messages WHERE account_id=?2 AND mailbox_key=?3 AND mailbox_encoding=?4 AND uid_validity=?5 AND uid=?6 AND internal_message_id<>?1)",params![id,account,mailbox.wire_hex(),mailbox.encoding().as_str(),i64::from(validity),i64::from(uid)],|r|r.get(0)))?;
    if bad {
        Err(ErrorCode::InternalError)
    } else {
        Ok(())
    }
}
pub(super) fn insert(
    tx: &Transaction<'_>,
    p: &PreviewPlan,
    r: &Row,
    page: &ImapUidReconcilePage,
    at: i64,
) -> Result<(), ErrorCode> {
    let ordinal = r.step.ok_or(ErrorCode::InternalError)?.1;
    let request = page.request();
    let json = String::from_utf8(
        page.to_storage_json()
            .map_err(|_| ErrorCode::InternalError)?,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    change(db(tx.execute("INSERT INTO imap_initial_preview_pages(plan_id,ordinal,execution_id,after_uid,covered_through_uid,page_json,page_hash,inserted_count,stored_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![p.id().as_slice(),i64::from(ordinal),r.id.as_slice(),i64::from(request.after_uid()),i64::from(request.through_uid()),json,hash(json.as_bytes())?.as_slice(),page.messages().len() as i64,at]))?)?;
    for m in page.messages() {
        let id = crate::message_identities::location_id(
            request.account_id(),
            request.mailbox(),
            request.uid_validity(),
            m.uid(),
        )?;
        collision(
            tx,
            &id,
            request.account_id(),
            p.snapshot.mailbox(),
            request.uid_validity(),
            m.uid(),
        )?;
        let json = String::from_utf8(m.to_storage_json().map_err(|_| ErrorCode::InternalError)?)
            .map_err(|_| ErrorCode::InternalError)?;
        change(db(tx.execute("INSERT INTO imap_initial_preview_messages(plan_id,ordinal,uid,internal_message_id,account_id,mailbox_key,mailbox_encoding,uid_validity,metadata_json,metadata_hash) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![p.id().as_slice(),i64::from(ordinal),i64::from(m.uid()),id,request.account_id(),p.snapshot.mailbox().wire_hex(),p.snapshot.mailbox().encoding().as_str(),i64::from(request.uid_validity()),json,hash(json.as_bytes())?.as_slice()]))?)?;
    }
    Ok(())
}
