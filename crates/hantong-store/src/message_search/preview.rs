use super::*;
use crate::initial_sync::storage::{PreviewPlan, PreviewSource, preview_source, read_preview_page};
use hantong_contracts::message_search::ImapMetadataSource;

pub(super) struct PreviewContext {
    prefix: Vec<u8>,
    pub(super) binding_current: bool,
    pub(super) observed_at: i64,
}
impl PreviewContext {
    pub(super) fn token(
        &self,
        kind: ImapMetadataSource,
        position: u64,
        page_hash: &[u8; 32],
        resume: u32,
        expiry: i64,
    ) -> Result<ImapMetadataSearchCursor, ErrorCode> {
        let mut bytes = self.prefix.clone();
        bytes.push(match kind {
            ImapMetadataSource::Preview => 1,
            ImapMetadataSource::Base => 2,
        });
        bytes.extend_from_slice(&position.to_be_bytes());
        bytes.extend_from_slice(page_hash);
        bytes.extend_from_slice(&resume.to_be_bytes());
        bytes.extend_from_slice(&expiry.to_be_bytes());
        ImapMetadataSearchCursor::parse(&format!(
            "m2:{}:{}:{}:{}:{}",
            hantong_contracts::wire_hex::encode(&hash(&bytes)?),
            kind.as_str(),
            position,
            resume,
            expiry
        ))
        .map_err(|_| ErrorCode::InternalError)
    }
}

pub(super) fn search(
    c: &Connection,
    r: &ImapMetadataSearchRequest,
    name: &ImapMailboxName,
    base: Option<&Snapshot>,
    plan: PreviewPlan,
    common: &[u8],
    catalog_generation: u64,
) -> Result<ImapMetadataSearchPage, ErrorCode> {
    let now = db(crate::utc_millis())?;
    if r.cursor()
        .is_some_and(|cursor| cursor.source_kind().is_none() || now >= cursor.expires_at_utc_ms())
    {
        return Err(ErrorCode::CursorExpired);
    }
    let progress = plan.snapshot.progress();
    let compatible = base.is_none_or(|s| s.epoch.validity == progress.uid_validity());
    let covered = base.map_or(0, |s| s.cursor.value.covered_through_uid());
    let completed = progress.completed_page_count();
    let latest_observed = if completed == 0 {
        plan.observed_at_utc_ms()
    } else {
        preview_source(c, &plan, completed - 1)?
            .ok_or(ErrorCode::InternalError)?
            .stored_at_utc_ms
    };
    let binding_current = compatible
        && plan.binding_current(c)?
        && base.is_none_or(|snapshot| snapshot.binding_current);
    let observed_at = match base {
        Some(snapshot) if compatible => {
            latest_observed.max(snapshot.cursor.value.stored_at_utc_ms())
        }
        Some(snapshot) => snapshot.cursor.value.stored_at_utc_ms(),
        None => latest_observed,
    };
    let mut prefix = b"mail-hantong.imap-metadata-search-cursor.v2\0".to_vec();
    prefix.extend_from_slice(common);
    match base {
        Some(snapshot) => {
            prefix.push(1);
            frame(&mut prefix, &snapshot.prefix)?;
        }
        None => prefix.push(0),
    }
    prefix.push(1);
    prefix.extend_from_slice(plan.snapshot.plan_id());
    prefix.extend_from_slice(&plan.snapshot.version().to_be_bytes());
    prefix.extend_from_slice(&plan.snapshot.generation().to_be_bytes());
    prefix.extend_from_slice(&progress.uid_validity().to_be_bytes());
    prefix.extend_from_slice(&progress.target_uid_ceiling().to_be_bytes());
    prefix.push(completed);
    prefix.push(u8::from(binding_current));
    frame(&mut prefix, r.filter().as_str().as_bytes())?;
    prefix.extend_from_slice(&u64::from(r.limit()).to_be_bytes());
    let context = PreviewContext {
        prefix,
        binding_current,
        observed_at,
    };
    let expiry = r.cursor().map_or_else(
        || now.checked_add(300_000).ok_or(ErrorCode::InternalError),
        |cursor| Ok(cursor.expires_at_utc_ms()),
    )?;
    if let Some(cursor) = r.cursor()
        && cursor.source_kind() == Some(ImapMetadataSource::Base)
    {
        return consume(
            c,
            r,
            name,
            base.ok_or(ErrorCode::CursorExpired)?,
            Some(&context),
        );
    }
    let ordinal = r
        .cursor()
        .map_or(0, |cursor| (cursor.source_position() - 1) as u8);
    let selected = if compatible {
        preview_source(c, &plan, ordinal)?.filter(|source| source.through_uid > covered)
    } else {
        None
    };
    let Some(selected) = selected else {
        if r.cursor().is_some() {
            return Err(ErrorCode::CursorExpired);
        }
        return match base {
            Some(snapshot) => consume(c, r, name, snapshot, Some(&context)),
            None => result(r, catalog_generation, &context, vec![], vec![], None),
        };
    };
    consume_preview(
        c,
        r,
        &plan,
        base,
        &selected,
        &context,
        (catalog_generation, covered, expiry),
    )
}

fn preview_token(
    context: &PreviewContext,
    source: &PreviewSource,
    resume: u32,
    expiry: i64,
) -> Result<ImapMetadataSearchCursor, ErrorCode> {
    context.token(
        ImapMetadataSource::Preview,
        u64::from(source.ordinal) + 1,
        &source.page_hash,
        resume,
        expiry,
    )
}
fn next_source(
    c: &Connection,
    r: &ImapMetadataSearchRequest,
    plan: &PreviewPlan,
    base: Option<&Snapshot>,
    selected: &PreviewSource,
    context: &PreviewContext,
    expiry: i64,
) -> Result<Option<ImapMetadataSearchCursor>, ErrorCode> {
    let covered = base.map_or(0, |s| s.cursor.value.covered_through_uid());
    if let Some(next) = preview_source(c, plan, selected.ordinal + 1)?
        && next.through_uid > covered
    {
        if next.through_uid != selected.after_uid
            || next.stored_at_utc_ms < selected.stored_at_utc_ms
        {
            return Err(ErrorCode::InternalError);
        }
        return preview_token(context, &next, u32::MAX, expiry).map(Some);
    }
    base.map(|snapshot| {
        let source =
            source(c, r, snapshot.cursor.value.version())?.ok_or(ErrorCode::InternalError)?;
        source_binding(&source, snapshot)?;
        source_token(snapshot, &source, u32::MAX, expiry, Some(context))
    })
    .transpose()
}
fn consume_preview(
    c: &Connection,
    r: &ImapMetadataSearchRequest,
    plan: &PreviewPlan,
    base: Option<&Snapshot>,
    selected: &PreviewSource,
    context: &PreviewContext,
    bounds: (u64, u32, i64),
) -> Result<ImapMetadataSearchPage, ErrorCode> {
    let (catalog_generation, covered, expiry) = bounds;
    let resume = r.cursor().map_or(u32::MAX, |cursor| cursor.resume_uid());
    if let Some(cursor) = r.cursor()
        && &preview_token(context, selected, resume, expiry)? != cursor
    {
        return Err(ErrorCode::CursorExpired);
    }
    let stored = read_preview_page(c, plan, selected)?;
    if resume != u32::MAX
        && (!stored
            .page
            .messages()
            .iter()
            .any(|m| m.uid() == resume && m.uid() > covered)
            || !stored
                .page
                .messages()
                .iter()
                .any(|m| m.uid() < resume && m.uid() > covered))
    {
        return Err(ErrorCode::CursorExpired);
    }
    let mut items = Vec::new();
    let mut headers = Vec::new();
    let mut bytes = 8192usize;
    let mut checkpoint = resume;
    let mut remaining = false;
    for (index, message) in stored
        .page
        .messages()
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, message)| message.uid() > covered && message.uid() < resume)
    {
        if items.len() >= r.limit() as usize {
            remaining = true;
            break;
        }
        let read = message.flags().iter().any(|flag| flag == "\\seen");
        let favorite = message.flags().iter().any(|flag| flag == "\\flagged");
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
                &stored.message_ids[index],
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
        Some(preview_token(context, selected, checkpoint, expiry)?)
    } else {
        next_source(c, r, plan, base, selected, context, expiry)?
    };
    result(r, catalog_generation, context, items, headers, next)
}
fn result(
    r: &ImapMetadataSearchRequest,
    catalog_generation: u64,
    context: &PreviewContext,
    items: Vec<ImapMetadataSearchItem>,
    headers: Vec<HeaderProjection>,
    next: Option<ImapMetadataSearchCursor>,
) -> Result<ImapMetadataSearchPage, ErrorCode> {
    ImapMetadataSearchPage::new(
        r.account_id(),
        r.mailbox().clone(),
        catalog_generation,
        context.binding_current,
        Some(context.observed_at),
        items,
        headers,
        next,
    )
    .map_err(|_| ErrorCode::InternalError)
}
