//! Unix-only authentication transcript v2.
//!
//! This is an additive contract.  Windows IPC continues to use
//! [`crate::auth::AuthTranscript`] (v1); Unix transports must select this
//! version explicitly and never downgrade to v1.

use crate::{ErrorCode, auth::RequestedScope};

const DOMAIN: &[u8] = b"mail-hantong/ipc-auth/v2\0";
const AUTH_VERSION: u16 = 2;
const MAX_TRANSCRIPT_BYTES: usize = 32_768;
const MAX_LIFETIME_MS: u64 = 30_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum UnixPlatform {
    Linux = 1,
    MacOs = 2,
}

impl UnixPlatform {
    fn from_wire(value: u8) -> Result<Self, ErrorCode> {
        match value {
            1 => Ok(Self::Linux),
            2 => Ok(Self::MacOs),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct UnixPeerContextV2 {
    pub platform: UnixPlatform,
    pub effective_uid: u32,
    pub effective_gid: u32,
    pub peer_pid: u32,
    pub session_id: u32,
    pub process_instance: [u8; 32],
}

impl core::fmt::Debug for UnixPeerContextV2 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("UnixPeerContextV2 { <redacted> }")
    }
}

impl UnixPeerContextV2 {
    pub fn new(
        platform: UnixPlatform,
        effective_uid: u32,
        effective_gid: u32,
        peer_pid: u32,
        session_id: u32,
        process_instance: [u8; 32],
    ) -> Result<Self, ErrorCode> {
        if peer_pid == 0 || session_id == 0 || process_instance == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            platform,
            effective_uid,
            effective_gid,
            peer_pid,
            session_id,
            process_instance,
        })
    }

    fn validate(&self) -> Result<(), ErrorCode> {
        if self.peer_pid == 0 || self.session_id == 0 || self.process_instance == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        match self.platform {
            UnixPlatform::Linux | UnixPlatform::MacOs => Ok(()),
        }
    }

    fn encode(&self, out: &mut Vec<u8>) {
        out.push(self.platform as u8);
        out.extend_from_slice(&self.effective_uid.to_le_bytes());
        out.extend_from_slice(&self.effective_gid.to_le_bytes());
        out.extend_from_slice(&self.peer_pid.to_le_bytes());
        out.extend_from_slice(&self.session_id.to_le_bytes());
        out.extend_from_slice(&self.process_instance);
    }

    fn decode(cursor: &mut Cursor<'_>) -> Result<Self, ErrorCode> {
        Self::new(
            UnixPlatform::from_wire(cursor.u8()?)?,
            cursor.u32()?,
            cursor.u32()?,
            cursor.u32()?,
            cursor.u32()?,
            cursor.array()?,
        )
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct UnixAuthTranscriptV2 {
    pub registration_id: [u8; 16],
    pub profile_epoch: u64,
    pub issued_at_utc_ms: i64,
    pub expires_at_utc_ms: i64,
    pub server_instance_id: [u8; 32],
    pub server_nonce: [u8; 32],
    pub client_nonce: [u8; 32],
    pub server_public_key_hash: [u8; 32],
    pub client_public_key_hash: [u8; 32],
    pub server: UnixPeerContextV2,
    pub client: UnixPeerContextV2,
    pub scope: RequestedScope,
}

impl core::fmt::Debug for UnixAuthTranscriptV2 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("UnixAuthTranscriptV2 { <redacted> }")
    }
}

impl UnixAuthTranscriptV2 {
    pub fn encode(&self, role: crate::auth::SignerRole) -> Result<Vec<u8>, ErrorCode> {
        if self.profile_epoch == 0
            || self.issued_at_utc_ms < 0
            || self.expires_at_utc_ms < 0
            || self
                .expires_at_utc_ms
                .checked_sub(self.issued_at_utc_ms)
                .and_then(|value| u64::try_from(value).ok())
                .is_none_or(|value| !(1..=MAX_LIFETIME_MS).contains(&value))
            || self.server.validate().is_err()
            || self.client.validate().is_err()
            || self.server.platform != self.client.platform
        {
            return Err(ErrorCode::InvalidInput);
        }
        let mut out = Vec::with_capacity(1024);
        out.extend_from_slice(DOMAIN);
        out.push(role as u8);
        out.extend_from_slice(&AUTH_VERSION.to_le_bytes());
        out.extend_from_slice(&self.registration_id);
        out.extend_from_slice(&self.profile_epoch.to_le_bytes());
        out.extend_from_slice(&self.issued_at_utc_ms.to_le_bytes());
        out.extend_from_slice(&self.expires_at_utc_ms.to_le_bytes());
        for bytes in [
            &self.server_instance_id,
            &self.server_nonce,
            &self.client_nonce,
            &self.server_public_key_hash,
            &self.client_public_key_hash,
        ] {
            out.extend_from_slice(bytes);
        }
        self.server.encode(&mut out);
        self.client.encode(&mut out);
        for values in [self.scope.accounts(), self.scope.tools()] {
            out.extend_from_slice(&(values.len() as u16).to_le_bytes());
            for value in values {
                push_string(&mut out, value);
            }
        }
        if out.len() > MAX_TRANSCRIPT_BYTES {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(out)
    }

    pub fn decode(bytes: &[u8], expected_role: crate::auth::SignerRole) -> Result<Self, ErrorCode> {
        if bytes.len() > MAX_TRANSCRIPT_BYTES || bytes.len() < DOMAIN.len() + 3 {
            return Err(ErrorCode::InvalidInput);
        }
        let mut cursor = Cursor { bytes, offset: 0 };
        if cursor.take(DOMAIN.len())? != DOMAIN || cursor.u8()? != expected_role as u8 {
            return Err(ErrorCode::InvalidInput);
        }
        if cursor.u16()? != AUTH_VERSION {
            return Err(ErrorCode::InvalidInput);
        }
        let transcript = Self {
            registration_id: cursor.array()?,
            profile_epoch: cursor.u64()?,
            issued_at_utc_ms: cursor.i64()?,
            expires_at_utc_ms: cursor.i64()?,
            server_instance_id: cursor.array()?,
            server_nonce: cursor.array()?,
            client_nonce: cursor.array()?,
            server_public_key_hash: cursor.array()?,
            client_public_key_hash: cursor.array()?,
            server: UnixPeerContextV2::decode(&mut cursor)?,
            client: UnixPeerContextV2::decode(&mut cursor)?,
            scope: RequestedScope::new(cursor.strings(128, 128)?, cursor.strings(128, 96)?)?,
        };
        if cursor.offset != bytes.len() || transcript.encode(expected_role)?.as_slice() != bytes {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(transcript)
    }
}

fn push_string(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u16).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
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

    fn string(&mut self, max_bytes: usize) -> Result<String, ErrorCode> {
        let length = self.u16()? as usize;
        if length > max_bytes {
            return Err(ErrorCode::InvalidInput);
        }
        String::from_utf8(self.take(length)?.to_vec()).map_err(|_| ErrorCode::InvalidInput)
    }

    fn strings(&mut self, max_items: usize, max_bytes: usize) -> Result<Vec<String>, ErrorCode> {
        let count = self.u16()? as usize;
        if count > max_items {
            return Err(ErrorCode::InvalidInput);
        }
        (0..count).map(|_| self.string(max_bytes)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> UnixAuthTranscriptV2 {
        UnixAuthTranscriptV2 {
            registration_id: [0x11; 16],
            profile_epoch: 1,
            issued_at_utc_ms: 1_000,
            expires_at_utc_ms: 31_000,
            server_instance_id: [0x22; 32],
            server_nonce: [0x33; 32],
            client_nonce: [0x44; 32],
            server_public_key_hash: [0x55; 32],
            client_public_key_hash: [0x66; 32],
            server: UnixPeerContextV2::new(UnixPlatform::Linux, 1000, 1000, 42, 7, [0x77; 32])
                .unwrap(),
            client: UnixPeerContextV2::new(UnixPlatform::Linux, 1000, 1000, 43, 7, [0x88; 32])
                .unwrap(),
            scope: RequestedScope::new(
                vec!["account_b".into(), "account_a".into()],
                vec!["mail.messages.get".into()],
            )
            .unwrap(),
        }
    }

    #[test]
    fn sec03_unix_v2_transcript_golden_roundtrip_and_role_separation() {
        let transcript = fixture();
        let server = transcript.encode(crate::auth::SignerRole::Server).unwrap();
        let client = transcript.encode(crate::auth::SignerRole::Client).unwrap();
        const EXPECTED_SERVER_HEX: &str = "6d61696c2d68616e746f6e672f6970632d617574682f763200010200111111111111111111111111111111110100000000000000e80300000000000018790000000000002222222222222222222222222222222222222222222222222222222222222222333333333333333333333333333333333333333333333333333333333333333344444444444444444444444444444444444444444444444444444444444444445555555555555555555555555555555555555555555555555555555555555555666666666666666666666666666666666666666666666666666666666666666601e8030000e80300002a00000007000000777777777777777777777777777777777777777777777777777777777777777701e8030000e80300002b000000070000008888888888888888888888888888888888888888888888888888888888888888020009006163636f756e745f6109006163636f756e745f62010011006d61696c2e6d657373616765732e676574";
        let expected = crate::wire_hex::decode_bounded(EXPECTED_SERVER_HEX, 371).unwrap();
        assert_eq!(server, expected);
        assert_eq!(server[25], 1);
        assert_eq!(client[25], 2);
        assert_ne!(server, client);
        assert_eq!(
            UnixAuthTranscriptV2::decode(&server, crate::auth::SignerRole::Server).unwrap(),
            transcript
        );
        assert_eq!(
            UnixAuthTranscriptV2::decode(&client, crate::auth::SignerRole::Client).unwrap(),
            transcript
        );
    }

    #[test]
    fn sec03_unix_v2_decoder_rejects_version_role_peer_and_time_mutations() {
        let original = fixture().encode(crate::auth::SignerRole::Server).unwrap();
        for (offset, value) in [(25, 2), (26, 3), (27, 1), (228, 3)] {
            let mut bytes = original.clone();
            bytes[offset] ^= value;
            assert!(
                UnixAuthTranscriptV2::decode(&bytes, crate::auth::SignerRole::Server).is_err(),
                "accepted mutation at {offset}"
            );
        }
        let mut negative_time = original.clone();
        negative_time[52..60].copy_from_slice(&(-1_i64).to_le_bytes());
        assert!(
            UnixAuthTranscriptV2::decode(&negative_time, crate::auth::SignerRole::Server).is_err()
        );
        let mut pid = original.clone();
        pid[237..241].fill(0);
        assert!(UnixAuthTranscriptV2::decode(&pid, crate::auth::SignerRole::Server).is_err());
        let mut truncation = original.clone();
        truncation.pop();
        assert!(
            UnixAuthTranscriptV2::decode(&truncation, crate::auth::SignerRole::Server).is_err()
        );
    }

    #[test]
    fn sec03_unix_v2_peer_rejects_zero_identity_and_unknown_platform() {
        assert!(UnixPeerContextV2::new(UnixPlatform::Linux, 1, 1, 0, 1, [1; 32]).is_err());
        assert!(UnixPeerContextV2::new(UnixPlatform::Linux, 1, 1, 1, 0, [1; 32]).is_err());
        assert!(UnixPeerContextV2::new(UnixPlatform::Linux, 1, 1, 1, 1, [0; 32]).is_err());
        assert!(UnixPlatform::from_wire(3).is_err());
    }

    #[test]
    fn sec03_unix_v2_encoder_revalidates_public_peer_fields_and_signed_time() {
        let mut negative = fixture();
        negative.issued_at_utc_ms = -1;
        assert!(negative.encode(crate::auth::SignerRole::Server).is_err());

        let mut invalid_peer = fixture();
        invalid_peer.server.peer_pid = 0;
        assert!(
            invalid_peer
                .encode(crate::auth::SignerRole::Server)
                .is_err()
        );

        let mut mixed_platform = fixture();
        mixed_platform.client.platform = UnixPlatform::MacOs;
        assert!(
            mixed_platform
                .encode(crate::auth::SignerRole::Server)
                .is_err()
        );
    }
}
