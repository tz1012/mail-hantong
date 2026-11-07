//! Canonical peer identity bytes for account-qualified authentication.
//!
//! The bytes in an [`AccountAuthTranscript`](crate::account_auth::AccountAuthTranscript)
//! are typed and self-identifying. Windows uses the validated user and logon
//! SIDs; Unix uses the validated platform, uid/gid, pid, session, and process
//! instance. These encodings are bounded, canonical, and are only transcript
//! inputs: they do not independently grant trust or authorization.

use crate::{
    ErrorCode,
    auth::PeerContext,
    unix_auth::{UnixPeerContextV2, UnixPlatform},
};

pub const MAX_PEER_BYTES: usize = 4 * 1024;
const WINDOWS_TAG: u8 = 1;
const UNIX_TAG: u8 = 2;

#[derive(Clone, Eq, PartialEq)]
pub enum CanonicalPeer {
    Windows(PeerContext),
    Unix(UnixPeerContextV2),
}

impl core::fmt::Debug for CanonicalPeer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("CanonicalPeer { <redacted> }")
    }
}

pub fn encode(peer: &CanonicalPeer) -> Result<Vec<u8>, ErrorCode> {
    let mut out = Vec::new();
    match peer {
        CanonicalPeer::Windows(peer) => {
            out.push(WINDOWS_TAG);
            push_sid(&mut out, peer.user_sid())?;
            push_sid(&mut out, peer.logon_sid())?;
        }
        CanonicalPeer::Unix(peer) => {
            if peer.peer_pid == 0 || peer.session_id == 0 || peer.process_instance == [0; 32] {
                return Err(ErrorCode::InvalidInput);
            }
            out.push(UNIX_TAG);
            out.push(peer.platform as u8);
            out.extend_from_slice(&peer.effective_uid.to_le_bytes());
            out.extend_from_slice(&peer.effective_gid.to_le_bytes());
            out.extend_from_slice(&peer.peer_pid.to_le_bytes());
            out.extend_from_slice(&peer.session_id.to_le_bytes());
            out.extend_from_slice(&peer.process_instance);
        }
    }
    if out.len() > MAX_PEER_BYTES {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(out)
}

pub fn decode(bytes: &[u8]) -> Result<CanonicalPeer, ErrorCode> {
    if bytes.is_empty() || bytes.len() > MAX_PEER_BYTES {
        return Err(ErrorCode::InvalidInput);
    }
    let peer = match bytes[0] {
        WINDOWS_TAG => {
            let mut cursor = Cursor {
                bytes: &bytes[1..],
                offset: 0,
            };
            let user = cursor.sid()?;
            let logon = cursor.sid()?;
            if cursor.offset != cursor.bytes.len() {
                return Err(ErrorCode::InvalidInput);
            }
            CanonicalPeer::Windows(PeerContext::new(&user, &logon)?)
        }
        UNIX_TAG => {
            if bytes.len() != 1 + 1 + 4 * 4 + 32 {
                return Err(ErrorCode::InvalidInput);
            }
            let mut cursor = Cursor {
                bytes: &bytes[1..],
                offset: 0,
            };
            let platform = match cursor.u8()? {
                1 => UnixPlatform::Linux,
                2 => UnixPlatform::MacOs,
                _ => return Err(ErrorCode::InvalidInput),
            };
            let peer = UnixPeerContextV2::new(
                platform,
                cursor.u32()?,
                cursor.u32()?,
                cursor.u32()?,
                cursor.u32()?,
                cursor.array()?,
            )?;
            if cursor.offset != cursor.bytes.len() {
                return Err(ErrorCode::InvalidInput);
            }
            CanonicalPeer::Unix(peer)
        }
        _ => return Err(ErrorCode::InvalidInput),
    };
    if encode(&peer)?.as_slice() != bytes {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(peer)
}

fn push_sid(out: &mut Vec<u8>, sid: &str) -> Result<(), ErrorCode> {
    let bytes = sid.as_bytes();
    let len = u16::try_from(bytes.len()).map_err(|_| ErrorCode::InvalidInput)?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Cursor<'_> {
    fn take(&mut self, len: usize) -> Result<&[u8], ErrorCode> {
        let end = self
            .offset
            .checked_add(len)
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
    fn u32(&mut self) -> Result<u32, ErrorCode> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| ErrorCode::InvalidInput)?,
        ))
    }
    fn array(&mut self) -> Result<[u8; 32], ErrorCode> {
        self.take(32)?
            .try_into()
            .map_err(|_| ErrorCode::InvalidInput)
    }
    fn sid(&mut self) -> Result<String, ErrorCode> {
        let len = u16::from_le_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| ErrorCode::InvalidInput)?,
        ) as usize;
        let value = std::str::from_utf8(self.take(len)?).map_err(|_| ErrorCode::InvalidInput)?;
        Ok(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec03_canonical_peer_roundtrips_windows_and_unix() {
        let windows =
            CanonicalPeer::Windows(PeerContext::new("S-1-5-21-1", "S-1-5-5-0-1").unwrap());
        assert_eq!(decode(&encode(&windows).unwrap()), Ok(windows));
        let unix = CanonicalPeer::Unix(
            UnixPeerContextV2::new(UnixPlatform::Linux, 1000, 1000, 7, 8, [9; 32]).unwrap(),
        );
        assert_eq!(decode(&encode(&unix).unwrap()), Ok(unix));
    }

    #[test]
    fn sec03_canonical_peer_rejects_wrong_tags_lengths_and_trailing_bytes() {
        assert_eq!(decode(&[]), Err(ErrorCode::InvalidInput));
        assert_eq!(decode(&[0]), Err(ErrorCode::InvalidInput));
        let mut bytes = encode(&CanonicalPeer::Windows(
            PeerContext::new("S-1-5-21-1", "S-1-5-5-0-1").unwrap(),
        ))
        .unwrap();
        bytes.push(0);
        assert_eq!(decode(&bytes), Err(ErrorCode::InvalidInput));
        let mut unix = encode(&CanonicalPeer::Unix(
            UnixPeerContextV2::new(UnixPlatform::Linux, 1, 1, 1, 1, [1; 32]).unwrap(),
        ))
        .unwrap();
        unix[1] = 3;
        assert_eq!(decode(&unix), Err(ErrorCode::InvalidInput));
    }
}
