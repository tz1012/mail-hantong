//! Model-2 account-qualified authentication transcript.
//!
//! This is deliberately additive.  The existing Windows v1 and Unix v2
//! transcripts remain frozen and are never accepted as this representation.

#[cfg(test)]
use crate::account_auth_peer::encode as encode_peer;

use crate::{
    ErrorCode,
    account_auth_peer::{CanonicalPeer, decode as decode_peer},
    account_scope::AccountScopeV2,
    auth::SignerRole,
};

const WINDOWS_DOMAIN: [u8; 32] = *b"mail-hantong/account-auth/w2\0\0\0\0";
const UNIX_DOMAIN: [u8; 32] = *b"mail-hantong/account-auth/u3\0\0\0\0";
const MAX_TRANSCRIPT_BYTES: usize = 32 * 1024;
const MAX_PEER_BYTES: usize = 4 * 1024;
const MAX_SCOPE_BYTES: usize = 24 * 1024;

/// The authentication revision is part of the signed model-2 transcript.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum AccountAuthRevision {
    WindowsV2 = 2,
    UnixV3 = 3,
}

impl AccountAuthRevision {
    fn from_wire(value: u8) -> Result<Self, ErrorCode> {
        match value {
            2 => Ok(Self::WindowsV2),
            3 => Ok(Self::UnixV3),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
}

/// Canonical signed input for a model-2 account-qualified connection.
///
/// `server_peer` and `client_peer` are opaque canonical identity encodings
/// owned by the platform contract.  They are included byte-for-byte and are
/// never interpreted as authorization by this parser.
#[derive(Clone, Eq, PartialEq)]
pub struct AccountAuthTranscript {
    pub revision: AccountAuthRevision,
    pub registration_id: [u8; 16],
    pub profile_epoch: u64,
    pub issued_at_utc_ms: i64,
    pub expires_at_utc_ms: i64,
    pub server_instance_id: [u8; 32],
    pub server_nonce: [u8; 32],
    pub client_nonce: [u8; 32],
    pub server_key_hash: [u8; 32],
    pub client_key_hash: [u8; 32],
    pub server_peer: Vec<u8>,
    pub client_peer: Vec<u8>,
    pub scope: AccountScopeV2,
}

impl core::fmt::Debug for AccountAuthTranscript {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("AccountAuthTranscript { <redacted> }")
    }
}

impl AccountAuthTranscript {
    pub fn encode(&self, role: SignerRole) -> Result<Vec<u8>, ErrorCode> {
        if self.registration_id == [0; 16]
            || self.profile_epoch == 0
            || self.issued_at_utc_ms < 0
            || self.expires_at_utc_ms < 0
            || self
                .expires_at_utc_ms
                .checked_sub(self.issued_at_utc_ms)
                .and_then(|v| u64::try_from(v).ok())
                .is_none_or(|v| !(1..=30_000).contains(&v))
            || self.server_peer.is_empty()
            || self.server_peer.len() > MAX_PEER_BYTES
            || self.client_peer.is_empty()
            || self.client_peer.len() > MAX_PEER_BYTES
        {
            return Err(ErrorCode::InvalidInput);
        }
        validate_peer_pair(self.revision, &self.server_peer, &self.client_peer)?;
        let scope = self.scope.to_json()?;
        if scope.len() > MAX_SCOPE_BYTES {
            return Err(ErrorCode::InvalidInput);
        }
        let mut out = Vec::with_capacity(1024 + scope.len());
        out.extend_from_slice(domain(self.revision));
        out.push(role as u8);
        out.push(self.revision as u8);
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&self.registration_id);
        out.extend_from_slice(&self.profile_epoch.to_le_bytes());
        out.extend_from_slice(&self.issued_at_utc_ms.to_le_bytes());
        out.extend_from_slice(&self.expires_at_utc_ms.to_le_bytes());
        for bytes in [
            &self.server_instance_id,
            &self.server_nonce,
            &self.client_nonce,
            &self.server_key_hash,
            &self.client_key_hash,
        ] {
            out.extend_from_slice(bytes);
        }
        push_bytes(&mut out, &self.server_peer)?;
        push_bytes(&mut out, &self.client_peer)?;
        let scope_len = u32::try_from(scope.len()).map_err(|_| ErrorCode::InvalidInput)?;
        out.extend_from_slice(&scope_len.to_le_bytes());
        out.extend_from_slice(&scope);
        if out.len() > MAX_TRANSCRIPT_BYTES {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(out)
    }

    pub fn decode(bytes: &[u8], expected_role: SignerRole) -> Result<Self, ErrorCode> {
        Self::decode_inner(bytes, expected_role, None)
    }

    pub fn decode_for_revision(
        bytes: &[u8],
        expected_role: SignerRole,
        expected_revision: AccountAuthRevision,
    ) -> Result<Self, ErrorCode> {
        Self::decode_inner(bytes, expected_role, Some(expected_revision))
    }

    fn decode_inner(
        bytes: &[u8],
        expected_role: SignerRole,
        expected_revision: Option<AccountAuthRevision>,
    ) -> Result<Self, ErrorCode> {
        if bytes.len() > MAX_TRANSCRIPT_BYTES || bytes.len() < WINDOWS_DOMAIN.len() + 5 {
            return Err(ErrorCode::InvalidInput);
        }
        let mut cursor = Cursor { bytes, offset: 0 };
        let domain_bytes = cursor.take(WINDOWS_DOMAIN.len())?;
        let role = cursor.u8()?;
        if role != expected_role as u8 {
            return Err(ErrorCode::InvalidInput);
        }
        let revision = AccountAuthRevision::from_wire(cursor.u8()?)?;
        if domain_bytes != domain(revision) {
            return Err(ErrorCode::InvalidInput);
        }
        if expected_revision.is_some_and(|expected| expected != revision) {
            return Err(ErrorCode::InvalidInput);
        }
        if cursor.u16()? != 2 {
            return Err(ErrorCode::InvalidInput);
        }
        let registration_id = cursor.array()?;
        let profile_epoch = cursor.u64()?;
        let issued_at_utc_ms = cursor.i64()?;
        let expires_at_utc_ms = cursor.i64()?;
        let server_instance_id = cursor.array()?;
        let server_nonce = cursor.array()?;
        let client_nonce = cursor.array()?;
        let server_key_hash = cursor.array()?;
        let client_key_hash = cursor.array()?;
        let server_peer = cursor.bytes(MAX_PEER_BYTES)?;
        let client_peer = cursor.bytes(MAX_PEER_BYTES)?;
        let scope_bytes = cursor.bytes(MAX_SCOPE_BYTES)?;
        let scope = AccountScopeV2::from_json(&scope_bytes)?;
        let transcript = Self {
            revision,
            registration_id,
            profile_epoch,
            issued_at_utc_ms,
            expires_at_utc_ms,
            server_instance_id,
            server_nonce,
            client_nonce,
            server_key_hash,
            client_key_hash,
            server_peer,
            client_peer,
            scope,
        };
        validate_peer_pair(revision, &transcript.server_peer, &transcript.client_peer)?;
        if cursor.offset != bytes.len() || transcript.encode(expected_role)?.as_slice() != bytes {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(transcript)
    }

    pub fn scope(&self) -> &AccountScopeV2 {
        &self.scope
    }
}

fn domain(revision: AccountAuthRevision) -> &'static [u8; 32] {
    match revision {
        AccountAuthRevision::WindowsV2 => &WINDOWS_DOMAIN,
        AccountAuthRevision::UnixV3 => &UNIX_DOMAIN,
    }
}

fn validate_peer_pair(
    revision: AccountAuthRevision,
    server: &[u8],
    client: &[u8],
) -> Result<(), ErrorCode> {
    let server = decode_peer(server)?;
    let client = decode_peer(client)?;
    match (revision, server, client) {
        (AccountAuthRevision::WindowsV2, CanonicalPeer::Windows(_), CanonicalPeer::Windows(_)) => {
            Ok(())
        }
        (AccountAuthRevision::UnixV3, CanonicalPeer::Unix(server), CanonicalPeer::Unix(client))
            if server.platform == client.platform
                && server.effective_uid == client.effective_uid =>
        {
            Ok(())
        }
        _ => Err(ErrorCode::InvalidInput),
    }
}

fn push_bytes(out: &mut Vec<u8>, value: &[u8]) -> Result<(), ErrorCode> {
    let len = u32::try_from(value.len()).map_err(|_| ErrorCode::InvalidInput)?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(value);
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_scope::{AccountGrantV2, AccountScopeV2};

    fn scope() -> AccountScopeV2 {
        AccountScopeV2::new(
            vec!["mail.accounts.list".into()],
            vec![
                AccountGrantV2::new(
                    "acct".into(),
                    vec!["mail.messages.get".into(), "mail.send.execute".into()],
                    false,
                )
                .unwrap(),
            ],
        )
        .unwrap()
    }

    fn transcript() -> AccountAuthTranscript {
        AccountAuthTranscript {
            revision: AccountAuthRevision::UnixV3,
            registration_id: [1; 16],
            profile_epoch: 1,
            issued_at_utc_ms: 1_000,
            expires_at_utc_ms: 2_000,
            server_instance_id: [2; 32],
            server_nonce: [3; 32],
            client_nonce: [4; 32],
            server_key_hash: [5; 32],
            client_key_hash: [6; 32],
            server_peer: encode_peer(&CanonicalPeer::Unix(
                crate::unix_auth::UnixPeerContextV2::new(
                    crate::unix_auth::UnixPlatform::Linux,
                    1000,
                    1000,
                    7,
                    8,
                    [9; 32],
                )
                .unwrap(),
            ))
            .unwrap(),
            client_peer: encode_peer(&CanonicalPeer::Unix(
                crate::unix_auth::UnixPeerContextV2::new(
                    crate::unix_auth::UnixPlatform::Linux,
                    1000,
                    1000,
                    8,
                    9,
                    [10; 32],
                )
                .unwrap(),
            ))
            .unwrap(),
            scope: scope(),
        }
    }

    #[test]
    fn sec03_model2_transcript_roundtrips_and_redacts() {
        let value = transcript();
        let bytes = value.encode(SignerRole::Server).unwrap();
        assert_eq!(
            AccountAuthTranscript::decode_for_revision(
                &bytes,
                SignerRole::Server,
                AccountAuthRevision::UnixV3,
            ),
            Ok(value.clone())
        );
        assert!(AccountAuthTranscript::decode(&bytes, SignerRole::Client).is_err());
        assert_eq!(format!("{value:?}"), "AccountAuthTranscript { <redacted> }");
    }

    #[test]
    fn sec03_model2_transcript_rejects_role_revision_scope_and_nonce_tampering() {
        let value = transcript();
        let original = value.encode(SignerRole::Server).unwrap();
        for index in [
            WINDOWS_DOMAIN.len(),
            WINDOWS_DOMAIN.len() + 1,
            WINDOWS_DOMAIN.len() + 2,
        ] {
            let mut tampered = original.clone();
            tampered[index] ^= 1;
            assert!(
                AccountAuthTranscript::decode_for_revision(
                    &tampered,
                    SignerRole::Server,
                    AccountAuthRevision::UnixV3,
                )
                .is_err()
            );
        }
        let nonce_index = WINDOWS_DOMAIN.len() + 2 + 2 + 16 + 8 + 8 + 8 + 32 + 32;
        let mut nonce_tampered = original.clone();
        nonce_tampered[nonce_index] ^= 1;
        let decoded = AccountAuthTranscript::decode_for_revision(
            &nonce_tampered,
            SignerRole::Server,
            AccountAuthRevision::UnixV3,
        )
        .unwrap();
        assert_ne!(decoded.client_nonce, value.client_nonce);
        assert_ne!(decoded.encode(SignerRole::Server).unwrap(), original);
        let mut trailing = original;
        trailing.push(0);
        assert!(AccountAuthTranscript::decode(&trailing, SignerRole::Server).is_err());
    }

    #[test]
    fn sec03_model2_rejects_cross_revision_domain_and_peer_platform_replay() {
        let value = transcript();
        let mut bytes = value.encode(SignerRole::Server).unwrap();
        bytes[WINDOWS_DOMAIN.len() + 1] = AccountAuthRevision::WindowsV2 as u8;
        assert!(AccountAuthTranscript::decode(&bytes, SignerRole::Server).is_err());

        let mut mismatched = value.clone();
        mismatched.revision = AccountAuthRevision::WindowsV2;
        assert_eq!(
            mismatched.encode(SignerRole::Server),
            Err(ErrorCode::InvalidInput)
        );

        let mut uid_mismatch = value;
        uid_mismatch.client_peer = encode_peer(&CanonicalPeer::Unix(
            crate::unix_auth::UnixPeerContextV2::new(
                crate::unix_auth::UnixPlatform::Linux,
                1001,
                1000,
                8,
                9,
                [10; 32],
            )
            .unwrap(),
        ))
        .unwrap();
        assert_eq!(
            uid_mismatch.encode(SignerRole::Server),
            Err(ErrorCode::InvalidInput)
        );
    }
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], ErrorCode> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(ErrorCode::InvalidInput)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ErrorCode::InvalidInput)?;
        self.offset = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8, ErrorCode> {
        Ok(*self.take(1)?.first().ok_or(ErrorCode::InvalidInput)?)
    }
    fn u16(&mut self) -> Result<u16, ErrorCode> {
        Ok(u16::from_le_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| ErrorCode::InvalidInput)?,
        ))
    }
    fn u32(&mut self) -> Result<u32, ErrorCode> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| ErrorCode::InvalidInput)?,
        ))
    }
    fn u64(&mut self) -> Result<u64, ErrorCode> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| ErrorCode::InvalidInput)?,
        ))
    }
    fn i64(&mut self) -> Result<i64, ErrorCode> {
        Ok(i64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| ErrorCode::InvalidInput)?,
        ))
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], ErrorCode> {
        self.take(N)?
            .try_into()
            .map_err(|_| ErrorCode::InvalidInput)
    }
    fn bytes(&mut self, max: usize) -> Result<Vec<u8>, ErrorCode> {
        let len = self.u32()? as usize;
        if len == 0 || len > max {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(self.take(len)?.to_vec())
    }
}
