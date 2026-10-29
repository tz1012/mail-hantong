//! Explicit attempt replacement. No credential or network effects are performed here.
use super::*;
use crate::account_provisioning::ImapProvisioningState;

fn historical_versions(request: &ImapSetupRequest) -> Option<(u64, u64)> {
    let last = request
        .events
        .iter()
        .rev()
        .find(|e| !matches!(e.event, ImapSetupEvent::Superseded(_)))?;
    match &last.event {
        ImapSetupEvent::RequestCreated | ImapSetupEvent::ReviewClosed(_) => request
            .intent
            .resume
            .as_ref()
            .and_then(|r| r.existing_versions),
        ImapSetupEvent::ProbeFinished(_) => {
            let (a, o) = effect_versions(&request.intent);
            Some((a + 1, o + 1))
        }
        _ => Some(effect_versions(&request.intent)),
    }
}
fn child_id(request: &ImapSetupRequest) -> Option<[u8; 32]> {
    match request.events.last().map(|e| &e.event) {
        Some(ImapSetupEvent::Superseded(id)) => Some(*id),
        _ => None,
    }
}
pub(super) fn validate_graph(
    requests: &BTreeMap<[u8; 32], ImapSetupRequest>,
) -> Result<(), ErrorCode> {
    let mut visited = BTreeSet::new();
    for root in requests.values().filter(|r| r.intent.resume.is_none()) {
        let mut current = root;
        loop {
            if !visited.insert(current.intent.request_id) || visited.len() > REQUEST_LIMIT {
                return Err(ErrorCode::InternalError);
            }
            let Some(id) = child_id(current) else {
                break;
            };
            let child = requests.get(&id).ok_or(ErrorCode::InternalError)?;
            let binding = child
                .intent
                .resume
                .as_ref()
                .ok_or(ErrorCode::InternalError)?;
            if binding.parent_request_id != current.intent.request_id
                || binding.parent_version + 1 != current.version
                || child.created_at_utc_ms != current.updated_at_utc_ms
                || child.intent.draft != current.intent.draft
                || child.intent.endpoint != current.intent.endpoint
                || child.intent.mutation.payload_hash() != current.intent.mutation.payload_hash()
                || binding.existing_versions != historical_versions(current)
            {
                return Err(ErrorCode::InternalError);
            }
            current = child;
        }
    }
    if visited.len() != requests.len() {
        return Err(ErrorCode::InternalError);
    }
    Ok(())
}
pub(super) fn read_chain(
    connection: &Connection,
    id: &[u8; 32],
) -> Result<BTreeMap<[u8; 32], ImapSetupRequest>, ErrorCode> {
    let Some(first) = read_local(connection, id, false)? else {
        return Ok(BTreeMap::new());
    };
    let mut requests = BTreeMap::from([(*id, first)]);
    let mut queue = vec![*id];
    let mut expanded = BTreeSet::new();
    while let Some(id) = queue.pop() {
        if !expanded.insert(id) {
            continue;
        }
        let current = requests.get(&id).ok_or(ErrorCode::InternalError)?;
        let mut linked = Vec::with_capacity(4);
        if let Some(binding) = &current.intent.resume {
            linked.push(binding.parent_request_id);
        }
        if let Some(child) = child_id(current) {
            linked.push(child);
        }
        // Both inverse relationships must agree, including a child omitted from
        // a damaged parent's final event. Each relationship is one-to-one.
        for query in [
            "SELECT request_id FROM imap_setup_requests WHERE resume_of_request_id=?1 LIMIT 2",
            "SELECT request_id FROM imap_setup_events WHERE child_request_id=?1 LIMIT 2",
        ] {
            let mut statement = db(connection.prepare(query))?;
            let ids: Vec<[u8; 32]> =
                db(db(statement.query_map([id.as_slice()], |row| blob_column(row, 0)))?.collect())?;
            if ids.len() > 1 {
                return Err(ErrorCode::InternalError);
            }
            linked.extend(ids);
        }
        for linked_id in linked {
            if let std::collections::btree_map::Entry::Vacant(entry) = requests.entry(linked_id) {
                let request =
                    read_local(connection, &linked_id, false)?.ok_or(ErrorCode::InternalError)?;
                entry.insert(request);
                if requests.len() > REQUEST_LIMIT {
                    return Err(ErrorCode::QuotaExceeded);
                }
            }
            if !expanded.contains(&linked_id) {
                queue.push(linked_id);
            }
        }
    }
    validate_graph(&requests)?;
    Ok(requests)
}
pub(in super::super) fn resolve_leaf(
    connection: &Connection,
    id: &[u8; 32],
) -> Result<ImapSetupRequest, ErrorCode> {
    let requests = read_chain(connection, id)?;
    let mut current = requests.get(id).ok_or(ErrorCode::NotFound)?;
    // read_chain has already validated cycles and every link within the bound.
    while let Some(child) = child_id(current) {
        current = requests.get(&child).ok_or(ErrorCode::InternalError)?;
    }
    Ok(current.clone())
}
pub(super) fn exact_target(
    tx: &Transaction<'_>,
    intent: &NewImapSetupRequest,
    versions: (u64, u64),
) -> Result<ImapProvisioningState, ErrorCode> {
    let current = read_state(tx, intent.draft.account_id())?;
    let draft = db(tx.query_row(
        "SELECT display_name,color FROM accounts WHERE account_id=?1",
        [intent.draft.account_id()],
        |row| {
            NewImapAccountDraft::new(
                intent.draft.account_id().into(),
                optional_text(row, 0, 512)?,
                optional_text(row, 1, 7)?,
            )
            .map_err(|_| rusqlite::Error::InvalidQuery)
        },
    ))?;
    if current.account_version != versions.0
        || current
            .observation
            .as_ref()
            .map_or(0, |o| o.observation_version)
            != versions.1
        || current.endpoint != intent.endpoint
        || draft != intent.draft
        || matches!(
            current.status,
            AccountStatus::Ready | AccountStatus::Disabled
        )
    {
        return Err(ErrorCode::Conflict);
    }
    Ok(current)
}
pub(in super::super) fn prepare_resume(
    tx: &Transaction<'_>,
    expected: &ImapSetupRequest,
    new_id: [u8; 32],
    mutation: MailMutation,
) -> Result<ImapSetupRequest, ErrorCode> {
    let current = read_request(tx, &expected.intent.request_id)?.ok_or(ErrorCode::NotFound)?;
    if current != *expected
        || current.phase == ImapSetupPhase::Superseded
        || matches!(
            current.events.last().map(|e| &e.event),
            Some(ImapSetupEvent::ProbeFinished(
                InitialImapProbeOutcome::Ready(_)
            ))
        )
    {
        return Err(ErrorCode::Conflict);
    }
    let collision: bool = db(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM imap_setup_requests WHERE request_id=?1 OR operation_id=?2 OR (account_id=?3 AND idempotency_key=?4)) OR EXISTS(SELECT 1 FROM mail_connection_operations WHERE operation_id=?2 OR (account_id=?3 AND idempotency_key=?4))",
        params![
            new_id.as_slice(), mutation.operation_id(),
            current.intent.draft.account_id(), mutation.idempotency_key(),
        ],
        |row| row.get(0),
    ))?;
    if collision {
        return Err(ErrorCode::Conflict);
    }
    if check_capacity(tx)? == REQUEST_LIMIT {
        return Err(ErrorCode::QuotaExceeded);
    }
    let now = db(crate::utc_millis())?;
    if now < current.updated_at_utc_ms {
        return Err(ErrorCode::InternalError);
    }
    let existing = historical_versions(&current);
    if let Some((account, observation)) = existing {
        if account > i64::MAX as u64 - 2 || observation >= i64::MAX as u64 {
            return Err(ErrorCode::Conflict);
        }
        exact_target(tx, &current.intent, (account, observation))?;
    } else {
        let exists: bool = db(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1)",
            [current.intent.draft.account_id()],
            |row| row.get(0),
        ))?;
        if exists {
            return Err(ErrorCode::Conflict);
        }
    }
    let hash = resume_hash_for(
        &current.intent.request_id,
        current.version,
        current.intent.mutation.payload_hash(),
        existing,
    )?;
    let mut intent = NewImapSetupRequest::new(
        new_id,
        current.intent.draft.clone(),
        current.intent.endpoint.clone(),
        mutation,
    )?;
    intent.resume = Some(ImapSetupResumeBinding {
        parent_request_id: current.intent.request_id,
        parent_version: current.version,
        existing_versions: existing,
        payload_hash: hash,
    });
    // Deactivate the parent first for the active-account unique index. Neither
    // incomplete relationship is observable outside this Immediate transaction.
    update_request(tx, &current, ImapSetupPhase::Superseded, now)?;
    insert_request(tx, &intent, now)?;
    append(
        tx,
        &current.intent,
        current.version + 1,
        now,
        &ImapSetupEvent::Superseded(new_id),
    )?;
    read_request(tx, &new_id)?.ok_or(ErrorCode::InternalError)
}
pub(super) fn stage_existing(
    tx: &Transaction<'_>,
    intent: &NewImapSetupRequest,
) -> Result<(), ErrorCode> {
    let versions = intent
        .resume
        .as_ref()
        .and_then(|r| r.existing_versions)
        .ok_or(ErrorCode::Conflict)?;
    exact_target(tx, intent, versions)?;
    let changed=db(tx.execute(
        "UPDATE accounts SET status='connecting',settings_version=?2 WHERE account_id=?1 AND provider='imap' AND settings_version=?3 AND status NOT IN ('ready','disabled')",
        params![intent.draft.account_id(),versions.0 as i64+1,versions.0 as i64],
    ))?;
    if changed != 1 {
        return Err(ErrorCode::Conflict);
    }
    Ok(())
}
