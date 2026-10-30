use super::*;
use serde_json::{Value, json};

pub(super) fn canonical(v: &Value) -> Result<String, ErrorCode> {
    serde_json::to_string(v).map_err(|_| ErrorCode::InternalError)
}
fn array(s: &str, n: usize) -> Result<Vec<Value>, ErrorCode> {
    let v: Value = serde_json::from_str(s).map_err(|_| ErrorCode::InternalError)?;
    if canonical(&v)? != s {
        return Err(ErrorCode::InternalError);
    }
    let a = v.as_array().ok_or(ErrorCode::InternalError)?;
    if a.len() != n {
        return Err(ErrorCode::InternalError);
    }
    Ok(a.clone())
}
fn string(v: &Value) -> Result<&str, ErrorCode> {
    v.as_str().ok_or(ErrorCode::InternalError)
}
fn num<T: TryFrom<u64>>(v: &Value) -> Result<T, ErrorCode> {
    T::try_from(v.as_u64().ok_or(ErrorCode::InternalError)?).map_err(|_| ErrorCode::InternalError)
}
fn optnum<T: TryFrom<u64>>(v: &Value) -> Result<Option<T>, ErrorCode> {
    if v.is_null() {
        Ok(None)
    } else {
        num(v).map(Some)
    }
}
fn boolean(v: &Value) -> Result<bool, ErrorCode> {
    v.as_bool().ok_or(ErrorCode::InternalError)
}
fn bytes<const N: usize>(v: &Value) -> Result<[u8; N], ErrorCode> {
    hantong_contracts::wire_hex::decode_fixed(string(v)?).map_err(|_| ErrorCode::InternalError)
}
fn identity(key: &Value, encoding: &Value) -> Result<UidMailboxIdentity, ErrorCode> {
    UidMailboxIdentity::new(
        string(key)?,
        UidMailboxEncoding::parse(string(encoding)?).map_err(|_| ErrorCode::InternalError)?,
    )
    .map_err(|_| ErrorCode::InternalError)
}
pub(super) fn request_json(r: &InitialSyncRequest) -> Result<String, ErrorCode> {
    canonical(&json!([
        r.action().as_str(),
        r.account_id(),
        r.mailbox().wire_hex(),
        r.mailbox().encoding().as_str(),
        r.operation_id(),
        r.idempotency_key(),
        r.intent_hash(),
        r.expected_catalog_generation(),
        r.expected_preview_generation(),
        r.recent_pages(),
        r.plan_id().map(|v| hex(v)),
        r.expected_plan_version()
    ]))
}
pub(super) fn request(s: &str) -> Result<InitialSyncRequest, ErrorCode> {
    let a = array(s, 12)?;
    let action = InitialSyncAction::parse(string(&a[0])?).map_err(|_| ErrorCode::InternalError)?;
    let m = InitialSyncMutation::new(string(&a[4])?, string(&a[5])?, string(&a[6])?)
        .map_err(|_| ErrorCode::InternalError)?;
    let r = match action {
        InitialSyncAction::Start => InitialSyncRequest::new_start(
            string(&a[1])?,
            identity(&a[2], &a[3])?,
            m,
            num(&a[7])?,
            num(&a[8])?,
            num(&a[9])?,
        ),
        InitialSyncAction::Resume => InitialSyncRequest::new_resume(
            string(&a[1])?,
            identity(&a[2], &a[3])?,
            m,
            bytes(&a[10])?,
            num(&a[11])?,
            num(&a[7])?,
        ),
        InitialSyncAction::Pause => InitialSyncRequest::new_pause(
            string(&a[1])?,
            identity(&a[2], &a[3])?,
            m,
            bytes(&a[10])?,
            num(&a[11])?,
        ),
    }
    .map_err(|_| ErrorCode::InternalError)?;
    if request_json(&r)? != s || hex(&hash(&r.intent_bytes())?) != r.intent_hash() {
        return Err(ErrorCode::InternalError);
    }
    Ok(r)
}
pub(super) fn binding_json(b: &Binding) -> Result<String, ErrorCode> {
    canonical(&json!([
        b.av,
        b.cg,
        b.ev,
        hex(&b.endpoint_hash),
        hex(&b.catalog_hash),
        b.validity,
        b.epoch
    ]))
}
pub(super) fn binding(s: &str) -> Result<Binding, ErrorCode> {
    let a = array(s, 7)?;
    let b = Binding {
        av: num(&a[0])?,
        cg: num(&a[1])?,
        ev: num(&a[2])?,
        endpoint_hash: bytes(&a[3])?,
        catalog_hash: bytes(&a[4])?,
        validity: optnum(&a[5])?,
        epoch: optnum(&a[6])?,
    };
    if b.av == 0
        || b.cg == 0
        || b.ev == 0
        || b.av > i64::MAX as u64
        || b.cg > i64::MAX as u64
        || b.ev > i64::MAX as u64
        || b.validity == Some(0)
        || b.epoch == Some(0)
        || b.epoch.is_some_and(|v| v > i64::MAX as u64)
        || b.validity.is_some() != b.epoch.is_some()
        || binding_json(&b)? != s
    {
        return Err(ErrorCode::InternalError);
    }
    Ok(b)
}
pub(super) fn snapshot_json(s: &InitialSyncPlanSnapshot) -> Result<String, ErrorCode> {
    let p = s.progress();
    canonical(&json!([
        hex(s.plan_id()),
        s.account_id(),
        s.mailbox().wire_hex(),
        s.mailbox().encoding().as_str(),
        s.generation(),
        s.version(),
        p.uid_validity(),
        p.uid_next(),
        p.recent_pages(),
        p.completed_page_count(),
        p.state().as_str(),
        p.model_version(),
        s.is_current(),
        s.binding_current(),
        s.stored_at_utc_ms()
    ]))
}
pub(super) fn snapshot(s: &str) -> Result<InitialSyncPlanSnapshot, ErrorCode> {
    let a = array(s, 15)?;
    let p = InitialSyncProgress::new(
        num(&a[6])?,
        num(&a[7])?,
        num(&a[8])?,
        num(&a[9])?,
        InitialSyncState::parse(string(&a[10])?).map_err(|_| ErrorCode::InternalError)?,
        num(&a[11])?,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    model(&p)?;
    let value = InitialSyncPlanSnapshot::new(
        InitialSyncPlanIdentity::new(
            bytes(&a[0])?,
            string(&a[1])?,
            identity(&a[2], &a[3])?,
            num(&a[4])?,
        )
        .map_err(|_| ErrorCode::InternalError)?,
        num(&a[5])?,
        p,
        boolean(&a[12])?,
        boolean(&a[13])?,
        a[14].as_i64().ok_or(ErrorCode::InternalError)?,
    )
    .map_err(|_| ErrorCode::InternalError)?;
    if snapshot_json(&value)? != s {
        return Err(ErrorCode::InternalError);
    }
    Ok(value)
}
