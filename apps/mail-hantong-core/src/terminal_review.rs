//! Core-owned terminal review tickets.
//!
//! Tickets are deliberately kept in the running Core and are consumed once.
//! The pipe layer supplies the observed peer context; values in the JSON
//! request are never treated as OS identity or approval evidence.
use hantong_contracts::{ErrorCode, terminal_review::TerminalReviewField};
#[cfg(windows)]
use hantong_contracts::{
    auth::PeerContext,
    terminal_review::{TerminalReviewKind, TerminalReviewReceipt},
};
#[cfg(windows)]
use hantong_platform::random_nonce;
#[cfg(windows)]
use hantong_secrets::TerminalSecretTicket;
#[cfg(windows)]
use hantong_secrets::{TerminalSecretBinding, TerminalSecretHandle};
#[cfg(windows)]
use hantong_store::mail_send::PendingSend;
#[cfg(windows)]
use std::collections::BTreeMap;

#[cfg(windows)]
pub(crate) struct Entry {
    pub kind: TerminalReviewKind,
    pub request_id: [u8; 32],
    pub account_id: String,
    pub payload_hash: [u8; 32],
    pub owner: [u8; 16],
    pub epoch: u64,
    pub expires_at_utc_ms: i64,
    pub peer: PeerContext,
    pub secret_ticket: Option<TerminalSecretTicket>,
}

#[cfg(windows)]
pub(crate) enum TerminalCapture {
    Imap {
        id: [u8; 32],
        ticket: TerminalSecretTicket,
        handle: TerminalSecretHandle,
        binding: TerminalSecretBinding,
    },
    Smtp {
        id: [u8; 32],
        ticket: TerminalSecretTicket,
        handle: TerminalSecretHandle,
        binding: TerminalSecretBinding,
    },
}

#[cfg(windows)]
pub(crate) fn make_ticket(
    map: &mut BTreeMap<[u8; 32], Entry>,
    pending: &PendingSend,
    peer: PeerContext,
    now: i64,
    fields: Vec<TerminalReviewField>,
) -> Result<TerminalReviewReceipt, ErrorCode> {
    let ticket = random_nonce()?;
    let payload_hash = *pending.request.payload_hash();
    make_entry_ticket(
        map,
        ticket,
        TerminalReviewKind::PersonalSend,
        pending.review_id,
        pending.request.account_id().into(),
        payload_hash,
        pending.owner_registration_id,
        pending.profile_epoch,
        pending.expires_at_utc_ms,
        peer,
        now,
        fields,
    )
}

// Keep every independently validated binding explicit at the ticket boundary.
#[allow(clippy::too_many_arguments)]
#[cfg(windows)]
pub(crate) fn make_entry_ticket(
    map: &mut BTreeMap<[u8; 32], Entry>,
    ticket: [u8; 32],
    kind: TerminalReviewKind,
    request_id: [u8; 32],
    account_id: String,
    payload_hash: [u8; 32],
    owner: [u8; 16],
    epoch: u64,
    expires_at_utc_ms: i64,
    peer: PeerContext,
    now: i64,
    fields: Vec<TerminalReviewField>,
) -> Result<TerminalReviewReceipt, ErrorCode> {
    if request_id == [0; 32] || payload_hash == [0; 32] || owner == [0; 16] {
        return Err(ErrorCode::InvalidInput);
    }
    prune_expired(map, now);
    if expires_at_utc_ms <= now {
        return Err(ErrorCode::ApprovalExpired);
    }
    if map.len() >= 64 {
        return Err(ErrorCode::RateLimited);
    }
    let expires = expires_at_utc_ms;
    map.insert(
        ticket,
        Entry {
            kind,
            request_id,
            account_id,
            payload_hash,
            owner,
            epoch,
            expires_at_utc_ms: expires,
            peer,
            secret_ticket: None,
        },
    );
    Ok(TerminalReviewReceipt {
        secret_ticket_opaque: None,
        kind,
        request_id,
        ticket: Some(ticket),
        snapshot_hash: payload_hash,
        owner_registration_id: owner,
        profile_epoch: epoch,
        expires_at_utc_ms: expires,
        state: "pending".into(),
        fields,
    })
}

#[cfg(windows)]
pub(crate) fn prune_expired(map: &mut BTreeMap<[u8; 32], Entry>, now: i64) {
    map.retain(|_, entry| entry.expires_at_utc_ms > now);
}

pub(crate) fn field(label: &str, value: String) -> Result<TerminalReviewField, ErrorCode> {
    TerminalReviewField::new(label.into(), value)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn peer() -> PeerContext {
        PeerContext::new("S-1-5-21-1-2-3-4", "S-1-5-5-1-2").unwrap()
    }

    #[test]
    fn terminal_ticket_rejects_expiry_and_prunes_bound_entries() {
        let mut map = BTreeMap::new();
        assert_eq!(
            make_entry_ticket(
                &mut map,
                [1; 32],
                TerminalReviewKind::Registration,
                [2; 32],
                "acct".into(),
                [3; 32],
                [4; 16],
                1,
                10,
                peer(),
                10,
                vec![]
            ),
            Err(ErrorCode::ApprovalExpired)
        );
        make_entry_ticket(
            &mut map,
            [1; 32],
            TerminalReviewKind::Registration,
            [2; 32],
            "acct".into(),
            [3; 32],
            [4; 16],
            1,
            20,
            peer(),
            10,
            vec![],
        )
        .unwrap();
        prune_expired(&mut map, 20);
        assert!(map.is_empty());
    }

    #[test]
    fn terminal_ticket_map_is_bounded_and_replay_identity_is_unique() {
        let mut map = BTreeMap::new();
        for i in 0..64u8 {
            make_entry_ticket(
                &mut map,
                [i; 32],
                TerminalReviewKind::ProtectedSend,
                [i.wrapping_add(1); 32],
                "acct".into(),
                [3; 32],
                [4; 16],
                1,
                10_000,
                peer(),
                1,
                vec![],
            )
            .unwrap();
        }
        assert_eq!(
            make_entry_ticket(
                &mut map,
                [99; 32],
                TerminalReviewKind::ProtectedSend,
                [9; 32],
                "acct".into(),
                [3; 32],
                [4; 16],
                1,
                10_000,
                peer(),
                1,
                vec![]
            ),
            Err(ErrorCode::RateLimited)
        );
        assert!(map.remove(&[0; 32]).is_some());
        assert!(map.contains_key(&[1; 32]));
    }

    #[test]
    fn terminal_secret_submit_requires_exact_opaque_ticket_shape() {
        assert!(
            hantong_contracts::terminal_review::TerminalReviewSecretSubmit::new(
                [1; 32],
                [2; 32],
                vec![0; 319],
                [3; 32]
            )
            .is_err()
        );
        assert!(
            hantong_contracts::terminal_review::TerminalReviewSecretSubmit::new(
                [1; 32],
                [2; 32],
                vec![0; 320],
                [3; 32]
            )
            .is_ok()
        );
    }
}
