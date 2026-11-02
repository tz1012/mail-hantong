//! Core-owned terminal capture for an advanced Google client secret.
//!
//! The foreground helper receives only the opaque ticket.  Secrets owns the
//! masked console read and DPAPI staging; this module retains the live ticket
//! and staged handle until the OAuth worker consumes them.

use hantong_contracts::{ErrorCode, auth::PeerContext};
use hantong_platform::ServerSigningKey;
use hantong_secrets::{
    SecretStore, TerminalSecretBinding, TerminalSecretHandle, TerminalSecretTicket,
};
use std::{cell::RefCell, collections::BTreeMap, path::Path};

const REQUEST_KIND: [u8; 16] = *b"google-client-v1";

pub(crate) struct Pending {
    ticket: TerminalSecretTicket,
    handle: Option<TerminalSecretHandle>,
    client_id: String,
}

pub(crate) type Captures = RefCell<BTreeMap<[u8; 32], Pending>>;

fn hash(value: &[u8]) -> Result<[u8; 32], ErrorCode> {
    hantong_platform::sha256(value)
}

fn binding(
    id: [u8; 32],
    client_id: &str,
    peer: &PeerContext,
    server: &ServerSigningKey,
    expires_at_utc_ms: i64,
) -> Result<TerminalSecretBinding, ErrorCode> {
    let payload_hash = hash(client_id.as_bytes())?;
    let account_hash = hash(format!("google-client/account/{client_id}").as_bytes())?;
    let endpoint_hash = hash(b"google-client-secret")?;
    let sid_hash = hash(peer.user_sid().as_bytes())?;
    let logon_hash = hash(peer.logon_sid().as_bytes())?;
    let server_hash = hash(&server.public_key()?)?;
    TerminalSecretBinding::new(
        REQUEST_KIND,
        id,
        payload_hash,
        account_hash,
        endpoint_hash,
        1,
        sid_hash,
        logon_hash,
        server_hash,
        expires_at_utc_ms,
    )
    .map_err(|_| ErrorCode::InvalidInput)
}

pub(crate) fn issue(
    captures: &Captures,
    root: &Path,
    id: [u8; 32],
    client_id: &str,
    peer: &PeerContext,
    server: &ServerSigningKey,
    expires_at_utc_ms: i64,
) -> Result<[u8; 320], ErrorCode> {
    if id == [0; 32] || client_id.is_empty() {
        return Err(ErrorCode::InvalidInput);
    }
    let mut captures_ref = captures.borrow_mut();
    let binding = binding(id, client_id, peer, server, expires_at_utc_ms)?;
    if let Some(pending) = captures_ref.get(&id) {
        if pending.ticket.binding() != binding
            || pending.handle.is_some()
            || pending.ticket.is_expired()
        {
            return Err(ErrorCode::Conflict);
        }
        return Ok(pending.ticket.to_opaque());
    }
    let store = SecretStore::open(root).map_err(|_| ErrorCode::InternalError)?;
    let ticket = store
        .issue_terminal_secret_ticket(binding)
        .map_err(|_| ErrorCode::InternalError)?;
    let opaque = ticket.to_opaque();
    captures_ref.insert(
        id,
        Pending {
            ticket,
            handle: None,
            client_id: client_id.to_owned(),
        },
    );
    Ok(opaque)
}

pub(crate) fn stage(
    captures: &Captures,
    root: &Path,
    id: [u8; 32],
    opaque: &[u8],
    staged_handle: [u8; 32],
) -> Result<[u8; 32], ErrorCode> {
    let ticket = TerminalSecretTicket::from_opaque(opaque).map_err(|_| ErrorCode::InvalidInput)?;
    let mut map = captures.borrow_mut();
    let pending = map.get_mut(&id).ok_or(ErrorCode::NotFound)?;
    if pending.ticket.binding() != ticket.binding()
        || pending.ticket.to_opaque().as_slice() != opaque
        || pending.handle.is_some()
    {
        return Err(ErrorCode::Conflict);
    }
    let store = SecretStore::open(root).map_err(|_| ErrorCode::InternalError)?;
    let handle = store
        .terminal_secret_handle_from_opaque(&pending.ticket, staged_handle)
        .map_err(|_| ErrorCode::Conflict)?;
    let opaque_id = handle.opaque_id();
    pending.handle = Some(handle);
    Ok(opaque_id)
}

pub(crate) fn consume(
    captures: &Captures,
    root: &Path,
    id: [u8; 32],
    server: &ServerSigningKey,
    peer: &PeerContext,
    expires_at_utc_ms: i64,
) -> Result<hantong_secrets::google::GoogleClientSecret, ErrorCode> {
    if !captures.borrow().contains_key(&id) {
        return Err(ErrorCode::HumanStepRequired);
    }
    if captures
        .borrow()
        .get(&id)
        .and_then(|p| p.handle.as_ref())
        .is_none()
    {
        return Err(ErrorCode::HumanStepRequired);
    }
    let pending = captures
        .borrow_mut()
        .remove(&id)
        .ok_or(ErrorCode::NotFound)?;
    let handle = pending.handle.ok_or(ErrorCode::ApprovalRequired)?;
    let expected = binding(id, &pending.client_id, peer, server, expires_at_utc_ms)?;
    let store = SecretStore::open(root).map_err(|_| ErrorCode::InternalError)?;
    store
        .consume_terminal_google_client_secret(pending.ticket, handle, expected, &pending.client_id)
        .map_err(|_| ErrorCode::Conflict)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_capture_binding_is_distinct_and_nonzero() {
        assert_ne!(REQUEST_KIND, [0; 16]);
        assert_eq!(REQUEST_KIND, *b"google-client-v1");
    }
}
