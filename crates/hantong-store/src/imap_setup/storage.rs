use super::*;
use crate::{
    account_provisioning::{finish_probe, read_state, stage_account, validate_capabilities},
    registrations::{blob_column, text_column},
};
use hantong_domain::{accounts::AccountStatus, mail_connection::MailConnectionConfig};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

const REQUEST_LIMIT: usize = 1024;
const EVENT_LIMIT: usize = 8;
const TTL: i64 = 300000;

fn db<T>(value: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    value.map_err(|_| ErrorCode::InternalError)
}
fn invalid<T>() -> rusqlite::Result<T> {
    Err(rusqlite::Error::InvalidQuery)
}
fn optional_text(row: &Row<'_>, index: usize, limit: usize) -> rusqlite::Result<Option<String>> {
    if row.get_ref(index)?.data_type() == rusqlite::types::Type::Null {
        Ok(None)
    } else {
        Ok(Some(text_column(row, index, limit)?.to_owned()))
    }
}
fn phase_text(phase: ImapSetupPhase) -> &'static str {
    match phase {
        ImapSetupPhase::ReviewPending => "review_pending",
        ImapSetupPhase::AccountStaged => "account_staged",
        ImapSetupPhase::ReviewClosed => "review_closed",
        ImapSetupPhase::CredentialStoreStarted => "credential_store_started",
        ImapSetupPhase::CredentialStoreReturned => "credential_store_returned",
        ImapSetupPhase::ProbeStarted => "probe_started",
        ImapSetupPhase::ProbeFinished => "probe_finished",
    }
}
fn parse_phase(s: &str) -> rusqlite::Result<ImapSetupPhase> {
    match s {
        "review_pending" => Ok(ImapSetupPhase::ReviewPending),
        "account_staged" => Ok(ImapSetupPhase::AccountStaged),
        "review_closed" => Ok(ImapSetupPhase::ReviewClosed),
        "credential_store_started" => Ok(ImapSetupPhase::CredentialStoreStarted),
        "credential_store_returned" => Ok(ImapSetupPhase::CredentialStoreReturned),
        "probe_started" => Ok(ImapSetupPhase::ProbeStarted),
        "probe_finished" => Ok(ImapSetupPhase::ProbeFinished),
        _ => invalid(),
    }
}
fn event_phase(event: &ImapSetupEvent) -> ImapSetupPhase {
    match event {
        ImapSetupEvent::RequestCreated => ImapSetupPhase::ReviewPending,
        ImapSetupEvent::AccountStaged => ImapSetupPhase::AccountStaged,
        ImapSetupEvent::ReviewClosed(_) => ImapSetupPhase::ReviewClosed,
        ImapSetupEvent::CredentialStoreStarted => ImapSetupPhase::CredentialStoreStarted,
        ImapSetupEvent::CredentialStoreReturned(_) => ImapSetupPhase::CredentialStoreReturned,
        ImapSetupEvent::ProbeStarted => ImapSetupPhase::ProbeStarted,
        ImapSetupEvent::ProbeFinished(_) => ImapSetupPhase::ProbeFinished,
    }
}
fn allowed(previous: Option<&ImapSetupEvent>, event: &ImapSetupEvent) -> bool {
    matches!(
        (previous, event),
        (None, ImapSetupEvent::RequestCreated)
            | (
                Some(ImapSetupEvent::RequestCreated),
                ImapSetupEvent::AccountStaged | ImapSetupEvent::ReviewClosed(_)
            )
            | (
                Some(ImapSetupEvent::AccountStaged),
                ImapSetupEvent::CredentialStoreStarted
            )
            | (
                Some(ImapSetupEvent::CredentialStoreStarted),
                ImapSetupEvent::CredentialStoreReturned(_)
            )
            | (
                Some(ImapSetupEvent::CredentialStoreReturned(Ok(()))),
                ImapSetupEvent::ProbeStarted
            )
            | (
                Some(ImapSetupEvent::ProbeStarted),
                ImapSetupEvent::ProbeFinished(_)
            )
    )
}
fn requires_fresh_review(event: &ImapSetupEvent) -> bool {
    matches!(
        event,
        ImapSetupEvent::AccountStaged
            | ImapSetupEvent::CredentialStoreStarted
            | ImapSetupEvent::ProbeStarted
    )
}

fn endpoint_from_json(
    account: &str,
    json: serde_json::Value,
) -> rusqlite::Result<ImapEndpointConfig> {
    let wire = serde_json::json!({"schema_version":1,"account_id":account,"imap":json});
    let bytes = serde_json::to_vec(&wire).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let parsed =
        MailConnectionConfig::from_json(&bytes).map_err(|_| rusqlite::Error::InvalidQuery)?;
    if serde_json::to_value(parsed.imap()).map_err(|_| rusqlite::Error::InvalidQuery)? != json {
        return invalid();
    }
    Ok(parsed.imap().clone())
}
fn read_request_row(row: &Row<'_>) -> rusqlite::Result<ImapSetupRequest> {
    let account = text_column(row, 1, 128)?;
    let draft = NewImapAccountDraft::new(
        account.to_owned(),
        optional_text(row, 4, 512)?,
        optional_text(row, 5, 7)?,
    )
    .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let endpoint = endpoint_from_json(
        account,
        serde_json::json!({
            "settings_version": row.get::<_,i64>(6)?,
            "host": text_column(row,7,253)?,
            "port": row.get::<_,i64>(8)?,
            "tls": text_column(row,9,32)?,
            "login_name": text_column(row,10,1280)?,
            "authentication": text_column(row,11,32)?,
        }),
    )?;
    let mutation = MailMutation::new(
        text_column(row, 2, 128)?.to_owned(),
        text_column(row, 3, 128)?.to_owned(),
        0,
        blob_column(row, 12)?,
    )
    .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let intent = NewImapSetupRequest::new(blob_column(row, 0)?, draft, endpoint, mutation)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let version: i64 = row.get(15)?;
    let created: i64 = row.get(16)?;
    let expires: i64 = row.get(17)?;
    let updated: i64 = row.get(18)?;
    if text_column(row, 13, 32)? != "local_same_logon_bootstrap"
        || !(1..=8).contains(&version)
        || created <= 0
        || created.checked_add(TTL) != Some(expires)
        || updated < created
    {
        return invalid();
    }
    Ok(ImapSetupRequest {
        intent,
        phase: parse_phase(text_column(row, 14, 32)?)?,
        version: version as u64,
        created_at_utc_ms: created,
        expires_at_utc_ms: expires,
        updated_at_utc_ms: updated,
        events: vec![],
    })
}
fn parse_error(value: &str) -> rusqlite::Result<ErrorCode> {
    ErrorCode::ALL
        .into_iter()
        .find(|e| e.as_str() == value)
        .ok_or(rusqlite::Error::InvalidQuery)
}
fn read_event(row: &Row<'_>) -> rusqlite::Result<ImapSetupEventRecord> {
    let seq: i64 = row.get(0)?;
    let time: i64 = row.get(2)?;
    if !(1..=8).contains(&seq) || time <= 0 {
        return invalid();
    }
    let kind = text_column(row, 1, 32)?;
    let result = optional_text(row, 3, 32)?;
    let error = optional_text(row, 4, 32)?;
    let probe = optional_text(row, 5, 32)?;
    let caps = optional_text(row, 6, 256)?;
    let versions = (
        row.get::<_, Option<i64>>(7)?,
        row.get::<_, Option<i64>>(8)?,
        row.get::<_, Option<i64>>(9)?,
    );
    let none = (None, None, None);
    let event = match kind {
        "request_created" | "credential_store_started" | "probe_started"
            if result.is_none()
                && error.is_none()
                && probe.is_none()
                && caps.is_none()
                && versions == none =>
        {
            match kind {
                "request_created" => ImapSetupEvent::RequestCreated,
                "credential_store_started" => ImapSetupEvent::CredentialStoreStarted,
                _ => ImapSetupEvent::ProbeStarted,
            }
        }
        "account_staged"
            if result.is_none()
                && error.is_none()
                && probe.is_none()
                && caps.is_none()
                && versions == (Some(1), Some(1), Some(0)) =>
        {
            ImapSetupEvent::AccountStaged
        }
        "review_closed"
            if result.as_deref() == Some("returned_error")
                && probe.is_none()
                && caps.is_none()
                && versions == none =>
        {
            let error = parse_error(error.as_deref().ok_or(rusqlite::Error::InvalidQuery)?)?;
            if !review_error(error) {
                return invalid();
            }
            ImapSetupEvent::ReviewClosed(error)
        }
        "credential_store_returned" if probe.is_none() && caps.is_none() && versions == none => {
            let result = match (result.as_deref(), error.as_deref()) {
                (Some("returned_ok"), None) => Ok(()),
                (Some("returned_error"), Some(code)) => {
                    let code = parse_error(code)?;
                    if !credential_error(code) {
                        return invalid();
                    }
                    Err(code)
                }
                _ => return invalid(),
            };
            ImapSetupEvent::CredentialStoreReturned(result)
        }
        "probe_finished"
            if result.is_none() && error.is_none() && versions == (Some(2), Some(1), Some(1)) =>
        {
            let values: Vec<String> =
                serde_json::from_str(caps.as_deref().ok_or(rusqlite::Error::InvalidQuery)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
            validate_capabilities(&values).map_err(|_| rusqlite::Error::InvalidQuery)?;
            let outcome = match probe.as_deref() {
                Some("ready") => InitialImapProbeOutcome::Ready(values),
                Some("auth_required") if values.is_empty() => InitialImapProbeOutcome::AuthRequired,
                Some("tls_failed") if values.is_empty() => InitialImapProbeOutcome::TlsFailed,
                Some("offline") if values.is_empty() => InitialImapProbeOutcome::Offline,
                Some("error") if values.is_empty() => InitialImapProbeOutcome::Error,
                _ => return invalid(),
            };
            ImapSetupEvent::ProbeFinished(outcome)
        }
        _ => return invalid(),
    };
    Ok(ImapSetupEventRecord {
        sequence: seq as u64,
        occurred_at_utc_ms: time,
        event,
    })
}

pub(super) fn read_request(
    connection: &Connection,
    id: &[u8; 32],
) -> Result<Option<ImapSetupRequest>, ErrorCode> {
    let Some(mut request)=db(connection.query_row(
        "SELECT request_id,account_id,operation_id,idempotency_key,display_name,color,endpoint_version,host,port,tls,login_name,authentication,payload_hash,origin,phase,version,created_at_utc_ms,expires_at_utc_ms,updated_at_utc_ms FROM imap_setup_requests WHERE request_id=?1",
        [id.as_slice()],read_request_row).optional())? else {return Ok(None);};
    let mut stmt = db(connection.prepare(
        "SELECT seq,kind,occurred_at_utc_ms,result_kind,error_code,probe_outcome,capabilities,account_version,endpoint_version,observation_version FROM imap_setup_events WHERE request_id=?1 ORDER BY seq LIMIT 9",
    ))?;
    request.events = db(db(stmt.query_map([id.as_slice()], read_event))?.collect())?;
    if request.events.len() > EVENT_LIMIT {
        return Err(ErrorCode::QuotaExceeded);
    }
    let mut previous = None;
    let mut time = request.created_at_utc_ms;
    for (index, event) in request.events.iter().enumerate() {
        if event.sequence != index as u64 + 1
            || !allowed(previous, &event.event)
            || event.occurred_at_utc_ms < time
            || (index == 0 && event.occurred_at_utc_ms != request.created_at_utc_ms)
            || (requires_fresh_review(&event.event)
                && event.occurred_at_utc_ms >= request.expires_at_utc_ms)
            || (event.event == ImapSetupEvent::ReviewClosed(ErrorCode::ApprovalExpired)
                && event.occurred_at_utc_ms < request.expires_at_utc_ms)
        {
            return Err(ErrorCode::InternalError);
        }
        previous = Some(&event.event);
        time = event.occurred_at_utc_ms;
    }
    let Some(last) = request.events.last() else {
        return Err(ErrorCode::InternalError);
    };
    if request.version != last.sequence
        || request.phase != event_phase(&last.event)
        || request.updated_at_utc_ms != time
    {
        return Err(ErrorCode::InternalError);
    }
    if let Some(staged) = request
        .events
        .iter()
        .find(|e| e.event == ImapSetupEvent::AccountStaged)
    {
        validate_links(connection, &request.intent, staged.occurred_at_utc_ms)?;
    }
    Ok(Some(request))
}

// Validate historical linkage without comparing mutable account settings to the old review.
fn validate_links(
    connection: &Connection,
    intent: &NewImapSetupRequest,
    staged_at: i64,
) -> Result<(), ErrorCode> {
    let account = intent.draft.account_id();
    let mutation = &intent.mutation;
    db(connection.query_row(
        "SELECT account_id,provider,primary_address,display_name,color,account_number,status,capabilities,settings_version FROM accounts WHERE account_id=?1",
        [account],
        |row| {
            if text_column(row, 1, 16)? != "imap" {
                return invalid();
            }
            crate::accounts::read_account(row, true)?;
            let values: Vec<String> = serde_json::from_str(text_column(row, 7, 16384)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            validate_capabilities(&values).map_err(|_| rusqlite::Error::InvalidQuery)?;
            if row.get::<_, i64>(8)? <= 0 {
                return invalid();
            }
            Ok(())
        },
    ))?;
    db(connection.query_row(
        "SELECT settings_version,host,port,tls,login_name,authentication,updated_at_utc_ms FROM mail_connection_settings WHERE account_id=?1 AND endpoint_kind='imap'",
        [account],
        |row| {
            if row.get::<_, i64>(6)? <= 0 {
                return invalid();
            }
            endpoint_from_json(account, serde_json::json!({
                "settings_version": row.get::<_, i64>(0)?,
                "host": text_column(row, 1, 253)?,
                "port": row.get::<_, i64>(2)?,
                "tls": text_column(row, 3, 32)?,
                "login_name": text_column(row, 4, 1280)?,
                "authentication": text_column(row, 5, 32)?,
            }))
            .map(|_| ())
        },
    ))?;
    db(connection.query_row(
        "SELECT account_id,idempotency_key,endpoint_kind,payload_hash,expected_version,result_version,completed_at_utc_ms FROM mail_connection_operations WHERE operation_id=?1",
        [mutation.operation_id()],
        |row| {
            if text_column(row, 0, 128)? != account
                || text_column(row, 1, 128)? != mutation.idempotency_key()
                || text_column(row, 2, 4)? != "imap"
                || &blob_column::<32>(row, 3)? != mutation.payload_hash()
                || row.get::<_, i64>(4)? != 0
                || row.get::<_, i64>(5)? != 1
                || row.get::<_, i64>(6)? != staged_at {
                return invalid();
            }
            Ok(())
        },
    ))?;
    Ok(())
}
fn check_capacity(connection: &Connection) -> Result<usize, ErrorCode> {
    let count: i64 = db(connection.query_row(
        "SELECT count(*) FROM (SELECT request_id FROM imap_setup_requests LIMIT 1025)",
        [],
        |row| row.get(0),
    ))?;
    let events: i64 = db(connection.query_row(
        "SELECT count(*) FROM (SELECT request_id FROM imap_setup_events LIMIT 8193)",
        [],
        |row| row.get(0),
    ))?;
    if count > REQUEST_LIMIT as i64 || events > (REQUEST_LIMIT * EVENT_LIMIT) as i64 {
        return Err(ErrorCode::QuotaExceeded);
    }
    let orphan: bool = db(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM imap_setup_events e LEFT JOIN imap_setup_requests r ON r.request_id=e.request_id WHERE r.request_id IS NULL)",
        [],
        |row| row.get(0),
    ))?;
    if orphan {
        return Err(ErrorCode::InternalError);
    }
    Ok(count as usize)
}
pub(super) fn read_all(connection: &Connection) -> Result<Vec<ImapSetupRequest>, ErrorCode> {
    check_capacity(connection)?;
    let mut stmt = db(connection
        .prepare("SELECT request_id FROM imap_setup_requests ORDER BY request_id LIMIT 1025"))?;
    let ids: Vec<[u8; 32]> = db(db(stmt.query_map([], |row| blob_column(row, 0)))?.collect())?;
    if ids.len() > REQUEST_LIMIT {
        return Err(ErrorCode::QuotaExceeded);
    }
    ids.iter()
        .map(|id| read_request(connection, id)?.ok_or(ErrorCode::InternalError))
        .collect()
}
fn assert_new_target(tx: &Transaction<'_>, intent: &NewImapSetupRequest) -> Result<(), ErrorCode> {
    let exists: bool = db(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM accounts WHERE account_id=?1) OR EXISTS(SELECT 1 FROM mail_connection_operations WHERE operation_id=?2 OR (account_id=?1 AND idempotency_key=?3))",
        params![intent.draft.account_id(),intent.mutation.operation_id(),intent.mutation.idempotency_key()],
        |row| row.get(0),
    ))?;
    if exists {
        return Err(ErrorCode::Conflict);
    }
    Ok(())
}
pub(super) fn create(
    tx: &Transaction<'_>,
    intent: &NewImapSetupRequest,
) -> Result<ImapSetupRequest, ErrorCode> {
    if let Some(current) = read_request(tx, &intent.request_id)? {
        return if current.intent == *intent {
            Ok(current)
        } else {
            Err(ErrorCode::Conflict)
        };
    }
    assert_new_target(tx, intent)?;
    let conflict: bool = db(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM imap_setup_requests WHERE operation_id=?1 OR (account_id=?2 AND (idempotency_key=?3 OR phase IN ('review_pending','account_staged','credential_store_started','credential_store_returned','probe_started'))))",
        params![intent.mutation.operation_id(),intent.draft.account_id(),intent.mutation.idempotency_key()],
        |row| row.get(0),
    ))?;
    if conflict {
        return Err(ErrorCode::Conflict);
    }
    if check_capacity(tx)? == REQUEST_LIMIT {
        return Err(ErrorCode::QuotaExceeded);
    }
    let now = db(crate::utc_millis())?;
    let expires = now.checked_add(TTL).ok_or(ErrorCode::InternalError)?;
    let endpoint = &intent.endpoint;
    let tls = match endpoint.tls() {
        hantong_domain::mail_connection::TlsMode::ImplicitTls => "implicit_tls",
        hantong_domain::mail_connection::TlsMode::StarttlsRequired => "starttls_required",
    };
    db(tx.execute(
        "INSERT INTO imap_setup_requests(request_id,account_id,operation_id,idempotency_key,display_name,color,endpoint_version,host,port,tls,login_name,authentication,payload_hash,origin,phase,version,created_at_utc_ms,expires_at_utc_ms,updated_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?6,1,?7,?8,?9,?10,'app_password',?11,'local_same_logon_bootstrap','review_pending',1,?12,?13,?12)",
        params![
            intent.request_id.as_slice(), intent.draft.account_id(),
            intent.mutation.operation_id(), intent.mutation.idempotency_key(),
            intent.draft.display_name(), intent.draft.color(),
            endpoint.host(), endpoint.port(), tls, endpoint.login_name(),
            intent.mutation.payload_hash().as_slice(), now, expires,
        ],
    ))?;
    append(
        tx,
        &intent.request_id,
        1,
        now,
        &ImapSetupEvent::RequestCreated,
    )?;
    read_request(tx, &intent.request_id)?.ok_or(ErrorCode::InternalError)
}
fn exact_connecting(tx: &Transaction<'_>, intent: &NewImapSetupRequest) -> Result<(), ErrorCode> {
    let current = read_state(tx, intent.draft.account_id())?;
    if current.status != AccountStatus::Connecting
        || current.account_version != 1
        || current.endpoint != intent.endpoint
        || current.observation.is_some()
    {
        return Err(ErrorCode::Conflict);
    }
    Ok(())
}
pub(super) fn change(
    tx: &Transaction<'_>,
    expected: &ImapSetupRequest,
    event: ImapSetupEvent,
) -> Result<ImapSetupRequest, ErrorCode> {
    let current = read_request(tx, &expected.intent.request_id)?.ok_or(ErrorCode::NotFound)?;
    if current != *expected || !allowed(current.events.last().map(|e| &e.event), &event) {
        return Err(ErrorCode::Conflict);
    }
    check_capacity(tx)?;
    let now = db(crate::utc_millis())?;
    if requires_fresh_review(&event) {
        if now < current.updated_at_utc_ms || now >= current.expires_at_utc_ms {
            return Err(ErrorCode::ApprovalExpired);
        }
    } else if now < current.updated_at_utc_ms {
        return Err(ErrorCode::InternalError);
    }
    if event == ImapSetupEvent::ReviewClosed(ErrorCode::ApprovalExpired)
        && now < current.expires_at_utc_ms
    {
        return Err(ErrorCode::Conflict);
    }
    match &event {
        ImapSetupEvent::AccountStaged => {
            assert_new_target(tx, &current.intent)?;
            stage_account(
                tx,
                &current.intent.draft,
                &current.intent.endpoint,
                &current.intent.mutation,
                now,
            )?;
        }
        ImapSetupEvent::CredentialStoreStarted | ImapSetupEvent::ProbeStarted => {
            exact_connecting(tx, &current.intent)?
        }
        ImapSetupEvent::ProbeFinished(outcome) => {
            exact_connecting(tx, &current.intent)?;
            finish_probe(tx, current.intent.draft.account_id(), 1, 1, 0, outcome, now)?;
        }
        _ => {}
    }
    let next = current.version + 1;
    let count = db(tx.execute(
        "UPDATE imap_setup_requests SET phase=?2,version=?3,updated_at_utc_ms=?4 WHERE request_id=?1 AND version=?5",
        params![
            current.intent.request_id.as_slice(), phase_text(event_phase(&event)),
            next as i64, now, current.version as i64,
        ],
    ))?;
    if count != 1 {
        return Err(ErrorCode::Conflict);
    }
    append(tx, &current.intent.request_id, next, now, &event)?;
    read_request(tx, &current.intent.request_id)?.ok_or(ErrorCode::InternalError)
}
fn append(
    tx: &Transaction<'_>,
    id: &[u8; 32],
    seq: u64,
    now: i64,
    event: &ImapSetupEvent,
) -> Result<(), ErrorCode> {
    let existing: i64 = db(tx.query_row(
        "SELECT count(*) FROM (SELECT request_id FROM imap_setup_events LIMIT 8193)",
        [],
        |row| row.get(0),
    ))?;
    if existing >= (REQUEST_LIMIT * EVENT_LIMIT) as i64 {
        return Err(ErrorCode::QuotaExceeded);
    }
    let (kind, result, error, probe, caps, versions) = match event {
        ImapSetupEvent::RequestCreated => (
            "request_created",
            None,
            None,
            None,
            None,
            (None, None, None),
        ),
        ImapSetupEvent::AccountStaged => (
            "account_staged",
            None,
            None,
            None,
            None,
            (Some(1), Some(1), Some(0)),
        ),
        ImapSetupEvent::ReviewClosed(e) => (
            "review_closed",
            Some("returned_error"),
            Some(e.as_str()),
            None,
            None,
            (None, None, None),
        ),
        ImapSetupEvent::CredentialStoreStarted => (
            "credential_store_started",
            None,
            None,
            None,
            None,
            (None, None, None),
        ),
        ImapSetupEvent::CredentialStoreReturned(result) => {
            let (kind, error) = match result {
                Ok(()) => ("returned_ok", None),
                Err(e) => ("returned_error", Some(e.as_str())),
            };
            (
                "credential_store_returned",
                Some(kind),
                error,
                None,
                None,
                (None, None, None),
            )
        }
        ImapSetupEvent::ProbeStarted => {
            ("probe_started", None, None, None, None, (None, None, None))
        }
        ImapSetupEvent::ProbeFinished(outcome) => {
            let (kind, values) = match outcome {
                InitialImapProbeOutcome::Ready(values) => ("ready", values.as_slice()),
                InitialImapProbeOutcome::AuthRequired => ("auth_required", &[][..]),
                InitialImapProbeOutcome::TlsFailed => ("tls_failed", &[][..]),
                InitialImapProbeOutcome::Offline => ("offline", &[][..]),
                InitialImapProbeOutcome::Error => ("error", &[][..]),
            };
            let json = serde_json::to_string(values).map_err(|_| ErrorCode::InternalError)?;
            (
                "probe_finished",
                None,
                None,
                Some(kind),
                Some(json),
                (Some(2), Some(1), Some(1)),
            )
        }
    };
    db(tx.execute(
        "INSERT INTO imap_setup_events(request_id,seq,kind,occurred_at_utc_ms,result_kind,error_code,probe_outcome,capabilities,account_version,endpoint_version,observation_version) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            id.as_slice(), seq as i64, kind, now, result, error, probe, caps,
            versions.0, versions.1, versions.2,
        ],
    ))?;
    Ok(())
}
