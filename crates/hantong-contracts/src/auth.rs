//! Canonical, bounded signature input. These values are data, not proof of
//! authentication or authorization; only the server's connection may own proof.

use crate::ErrorCode;
use std::fmt;

const MAX_ITEMS: usize = 128;
pub const MAX_TRANSCRIPT_BYTES: usize = 32_768;
const DOMAIN: &[u8] = b"mail-hantong/ipc-auth/v1\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignerRole {
    Server = 1,
    Client = 2,
}

#[derive(Clone, Eq, PartialEq)]
pub struct PeerContext {
    user: String,
    logon: String,
}
impl PeerContext {
    pub fn new(user: &str, logon: &str) -> Result<Self, ErrorCode> {
        if !valid_sid(user)
            || !valid_sid(logon)
            || !logon.starts_with("S-1-5-5-")
            || logon.split('-').count() != 6
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            user: user.into(),
            logon: logon.into(),
        })
    }
    pub fn user_sid(&self) -> &str {
        &self.user
    }
    pub fn logon_sid(&self) -> &str {
        &self.logon
    }
}
impl fmt::Debug for PeerContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PeerContext { <redacted> }")
    }
}

fn valid_sid(value: &str) -> bool {
    if value.len() > 184 {
        return false;
    }
    let Some(tail) = value.strip_prefix("S-1-") else {
        return false;
    };
    let mut numbers = tail.split('-');
    let canonical_number = |part: &str, max: u64| {
        !part.is_empty()
            && part.len() <= 15
            && (part == "0" || !part.starts_with('0'))
            && part.bytes().all(|b| b.is_ascii_digit())
            && part.parse::<u64>().is_ok_and(|n| n <= max)
    };
    if !numbers
        .next()
        .is_some_and(|n| canonical_number(n, (1u64 << 48) - 1))
    {
        return false;
    }
    let mut count = 0;
    for part in numbers {
        if !canonical_number(part, u32::MAX as u64) {
            return false;
        }
        count += 1;
    }
    (1..=15).contains(&count)
}

#[derive(Clone, Eq, PartialEq)]
pub struct RequestedScope {
    accounts: Vec<String>,
    tools: Vec<String>,
}
impl RequestedScope {
    pub fn new(mut accounts: Vec<String>, mut tools: Vec<String>) -> Result<Self, ErrorCode> {
        if accounts.len() > MAX_ITEMS
            || tools.len() > MAX_ITEMS
            || accounts.iter().any(|v| {
                v.is_empty()
                    || v.len() > 128
                    || !v
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            })
            || tools.iter().any(|v| {
                v.is_empty()
                    || v.len() > 96
                    || !v.as_bytes()[0].is_ascii_lowercase()
                    || !v
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.')
            })
        {
            return Err(ErrorCode::InvalidInput);
        }
        accounts.sort_unstable();
        tools.sort_unstable();
        if accounts.windows(2).any(|w| w[0] == w[1]) || tools.windows(2).any(|w| w[0] == w[1]) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self { accounts, tools })
    }
    pub fn accounts(&self) -> &[String] {
        &self.accounts
    }
    pub fn tools(&self) -> &[String] {
        &self.tools
    }
    pub fn permits_account(&self, account: &str, tool: &str) -> bool {
        self.accounts.iter().any(|v| v == account) && self.tools.iter().any(|v| v == tool)
    }
    pub fn intersection(&self, other: &Self) -> Self {
        Self {
            accounts: self
                .accounts
                .iter()
                .filter(|v| other.accounts.binary_search(v).is_ok())
                .cloned()
                .collect(),
            tools: self
                .tools
                .iter()
                .filter(|v| other.tools.binary_search(v).is_ok())
                .cloned()
                .collect(),
        }
    }
}
impl fmt::Debug for RequestedScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RequestedScope { <redacted> }")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct AuthTranscript {
    pub registration_id: [u8; 16],
    pub profile_epoch: u64,
    pub issued_at_utc_ms: i64,
    pub expires_at_utc_ms: i64,
    pub server_instance_id: [u8; 32],
    pub server_nonce: [u8; 32],
    pub client_nonce: [u8; 32],
    pub server_key_hash: [u8; 32],
    pub client_key_hash: [u8; 32],
    pub server: PeerContext,
    pub client: PeerContext,
    pub scope: RequestedScope,
}
impl fmt::Debug for AuthTranscript {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuthTranscript { <redacted> }")
    }
}
impl AuthTranscript {
    pub fn encode(&self, role: SignerRole) -> Result<Vec<u8>, ErrorCode> {
        self.issued_at_utc_ms
            .checked_add(30_000)
            .ok_or(ErrorCode::InvalidInput)?;
        let lifetime = self
            .expires_at_utc_ms
            .checked_sub(self.issued_at_utc_ms)
            .ok_or(ErrorCode::InvalidInput)?;
        if self.profile_epoch == 0 || self.issued_at_utc_ms < 0 || !(1..=30_000).contains(&lifetime)
        {
            return Err(ErrorCode::InvalidInput);
        }
        let mut out = Vec::with_capacity(1024);
        out.extend_from_slice(DOMAIN);
        out.push(role as u8);
        out.extend_from_slice(&1u16.to_le_bytes());
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
        for sid in [
            &self.server.user,
            &self.server.logon,
            &self.client.user,
            &self.client.logon,
        ] {
            push_string(&mut out, sid);
        }
        for list in [&self.scope.accounts, &self.scope.tools] {
            out.extend_from_slice(&(list.len() as u16).to_le_bytes());
            for value in list {
                push_string(&mut out, value);
            }
        }
        if out.len() > MAX_TRANSCRIPT_BYTES {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(out)
    }

    /// Decodes only the canonical binary representation emitted by `encode`.
    /// This is a bounded data parser, never an authentication proof verifier.
    pub fn decode(bytes: &[u8], expected_role: SignerRole) -> Result<Self, ErrorCode> {
        if bytes.len() > MAX_TRANSCRIPT_BYTES || bytes.len() < DOMAIN.len() + 3 {
            return Err(ErrorCode::InvalidInput);
        }
        let mut cursor = Cursor { bytes, offset: 0 };
        if cursor.take(DOMAIN.len())? != DOMAIN {
            return Err(ErrorCode::InvalidInput);
        }
        let role = cursor.u8()?;
        if role != expected_role as u8 || !matches!(role, 1 | 2) {
            return Err(ErrorCode::InvalidInput);
        }
        if cursor.u16()? != 1 {
            return Err(ErrorCode::InvalidInput);
        }
        let registration_id = cursor.array::<16>()?;
        let profile_epoch = cursor.u64()?;
        let issued_at_utc_ms = cursor.i64()?;
        let expires_at_utc_ms = cursor.i64()?;
        let server_instance_id = cursor.array::<32>()?;
        let server_nonce = cursor.array::<32>()?;
        let client_nonce = cursor.array::<32>()?;
        let server_key_hash = cursor.array::<32>()?;
        let client_key_hash = cursor.array::<32>()?;
        let server_user = cursor.string(184)?;
        let server_logon = cursor.string(184)?;
        let client_user = cursor.string(184)?;
        let client_logon = cursor.string(184)?;
        let accounts = cursor.strings(128, 128)?;
        let tools = cursor.strings(128, 96)?;
        if cursor.offset != bytes.len() {
            return Err(ErrorCode::InvalidInput);
        }
        let transcript = Self {
            registration_id,
            profile_epoch,
            issued_at_utc_ms,
            expires_at_utc_ms,
            server_instance_id,
            server_nonce,
            client_nonce,
            server_key_hash,
            client_key_hash,
            server: PeerContext::new(&server_user, &server_logon)?,
            client: PeerContext::new(&client_user, &client_logon)?,
            scope: RequestedScope::new(accounts, tools)?,
        };
        if transcript.encode(expected_role)?.as_slice() != bytes {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(transcript)
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], ErrorCode> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(ErrorCode::InvalidInput)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ErrorCode::InvalidInput)?;
        self.offset = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8, ErrorCode> {
        Ok(*self.take(1)?.first().unwrap())
    }
    fn u16(&mut self) -> Result<u16, ErrorCode> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, ErrorCode> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn i64(&mut self) -> Result<i64, ErrorCode> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], ErrorCode> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    fn string(&mut self, max_bytes: usize) -> Result<String, ErrorCode> {
        let length = self.u16()? as usize;
        if length > max_bytes {
            return Err(ErrorCode::InvalidInput);
        }
        let value = std::str::from_utf8(self.take(length)?).map_err(|_| ErrorCode::InvalidInput)?;
        if !value.is_ascii() {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(value.to_owned())
    }
    fn strings(&mut self, max_items: usize, max_bytes: usize) -> Result<Vec<String>, ErrorCode> {
        let count = self.u16()? as usize;
        if count > max_items {
            return Err(ErrorCode::InvalidInput);
        }
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.string(max_bytes)?);
        }
        Ok(values)
    }
}
fn push_string(out: &mut Vec<u8>, value: &str) {
    // Every string has already passed a private type constructor's bounds.
    out.extend_from_slice(&(value.len() as u16).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> AuthTranscript {
        AuthTranscript {
            registration_id: [0x11; 16],
            profile_epoch: 1,
            issued_at_utc_ms: 1000,
            expires_at_utc_ms: 31000,
            server_instance_id: [0x22; 32],
            server_nonce: [0x33; 32],
            client_nonce: [0x44; 32],
            server_key_hash: [0x55; 32],
            client_key_hash: [0x66; 32],
            server: PeerContext::new("S-1-5-21-1", "S-1-5-5-0-1").unwrap(),
            client: PeerContext::new("S-1-5-21-1", "S-1-5-5-0-1").unwrap(),
            scope: RequestedScope::new(
                vec!["account_b".into(), "account_a".into()],
                vec!["mail.messages.get".into()],
            )
            .unwrap(),
        }
    }

    #[test]
    fn sec03_transcript_has_exact_layout_and_role_separation() {
        let transcript = fixture();
        let bytes = transcript.encode(SignerRole::Server).unwrap();
        assert_eq!(&bytes[..25], b"mail-hantong/ipc-auth/v1\0");
        assert_eq!(bytes[25], 1);
        assert_eq!(&bytes[26..28], &1u16.to_le_bytes());
        assert_eq!(&bytes[28..44], &[0x11; 16]);
        assert_eq!(&bytes[44..52], &1u64.to_le_bytes());
        assert_eq!(&bytes[52..60], &1000i64.to_le_bytes());
        assert_eq!(&bytes[60..68], &31000i64.to_le_bytes());
        assert_eq!(&bytes[68..100], &[0x22; 32]);
        assert_eq!(&bytes[100..132], &[0x33; 32]);
        assert_eq!(&bytes[132..164], &[0x44; 32]);
        assert_eq!(&bytes[164..196], &[0x55; 32]);
        assert_eq!(&bytes[196..228], &[0x66; 32]);
        let expected_tail = b"\x0a\x00S-1-5-21-1\x0b\x00S-1-5-5-0-1\x0a\x00S-1-5-21-1\x0b\x00S-1-5-5-0-1\x02\x00\x09\x00account_a\x09\x00account_b\x01\x00\x11\x00mail.messages.get";
        assert_eq!(&bytes[228..], expected_tail);
        let mut client = transcript.encode(SignerRole::Client).unwrap();
        assert_eq!(client[25], 2);
        client[25] = 1;
        assert_eq!(client, bytes);
        client[25] = 2;
        assert_eq!(
            AuthTranscript::decode(&bytes, SignerRole::Server).unwrap(),
            transcript
        );
        assert_eq!(
            AuthTranscript::decode(&client, SignerRole::Client).unwrap(),
            transcript
        );
    }

    #[test]
    fn sec03_decoder_rejects_every_truncation_and_trailing_bytes() {
        let bytes = fixture().encode(SignerRole::Server).unwrap();
        for length in 0..bytes.len() {
            assert!(
                AuthTranscript::decode(&bytes[..length], SignerRole::Server).is_err(),
                "accepted truncation at {length}"
            );
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(AuthTranscript::decode(&trailing, SignerRole::Server).is_err());
        let mut oversized = vec![0u8; MAX_TRANSCRIPT_BYTES + 1];
        oversized[..bytes.len()].copy_from_slice(&bytes);
        assert!(AuthTranscript::decode(&oversized, SignerRole::Server).is_err());
    }

    #[test]
    fn sec03_decoder_rejects_domain_role_version_and_time_mutations() {
        let original = fixture().encode(SignerRole::Server).unwrap();
        for (offset, value) in [(0, b'X'), (25, 2), (26, 2), (52, 0xff), (60, 1)] {
            let mut bytes = original.clone();
            bytes[offset] ^= value;
            assert!(AuthTranscript::decode(&bytes, SignerRole::Server).is_err());
        }
        assert!(AuthTranscript::decode(&original, SignerRole::Client).is_err());
        let mut negative = original.clone();
        negative[52..60].copy_from_slice(&(-1i64).to_le_bytes());
        assert!(AuthTranscript::decode(&negative, SignerRole::Server).is_err());
        let mut zero_epoch = original.clone();
        zero_epoch[44..52].fill(0);
        assert!(AuthTranscript::decode(&zero_epoch, SignerRole::Server).is_err());
        let mut overflow = original.clone();
        overflow[52..60].copy_from_slice(&(i64::MAX - 1).to_le_bytes());
        overflow[60..68].copy_from_slice(&i64::MAX.to_le_bytes());
        assert!(AuthTranscript::decode(&overflow, SignerRole::Server).is_err());
    }

    #[test]
    fn sec03_decoder_rejects_invalid_strings_and_noncanonical_scope() {
        let original = fixture().encode(SignerRole::Server).unwrap();
        let mut sid_length = original.clone();
        sid_length[228..230].copy_from_slice(&185u16.to_le_bytes());
        assert!(AuthTranscript::decode(&sid_length, SignerRole::Server).is_err());
        let mut invalid_utf8 = original.clone();
        invalid_utf8[230] = 0xff;
        assert!(AuthTranscript::decode(&invalid_utf8, SignerRole::Server).is_err());
        let mut unsorted = original.clone();
        // account_a and account_b have equal lengths and occupy the tail after
        // the four fixed SID strings and the account count.
        let account_a = 280;
        let account_b = account_a + 2 + 9;
        for index in 0..11 {
            unsorted.swap(account_a + index, account_b + index);
        }
        assert!(AuthTranscript::decode(&unsorted, SignerRole::Server).is_err());
        let mut duplicate = original.clone();
        duplicate[account_b + 2..account_b + 11].copy_from_slice(b"account_a");
        assert!(AuthTranscript::decode(&duplicate, SignerRole::Server).is_err());
        let mut count = original.clone();
        count[278..280].copy_from_slice(&129u16.to_le_bytes());
        assert!(AuthTranscript::decode(&count, SignerRole::Server).is_err());
        let mut tool_count = original.clone();
        tool_count[302..304].copy_from_slice(&129u16.to_le_bytes());
        assert!(AuthTranscript::decode(&tool_count, SignerRole::Server).is_err());
        let mut bad_sid = original.clone();
        bad_sid[230] = b'T';
        assert!(AuthTranscript::decode(&bad_sid, SignerRole::Server).is_err());
    }

    #[test]
    fn sec03_decoder_roundtrips_maximum_valid_scope() {
        let scope = RequestedScope::new(
            (0..128)
                .map(|i| format!("account{i:03}{}", "x".repeat(118)))
                .collect(),
            (0..128)
                .map(|i| format!("mail.tool{i:03}{}", "x".repeat(84)))
                .collect(),
        )
        .unwrap();
        let transcript = AuthTranscript { scope, ..fixture() };
        let bytes = transcript.encode(SignerRole::Client).unwrap();
        assert!(bytes.len() <= MAX_TRANSCRIPT_BYTES);
        assert_eq!(
            AuthTranscript::decode(&bytes, SignerRole::Client).unwrap(),
            transcript
        );
    }

    #[test]
    fn sec03_scope_rejects_ambiguous_or_unbounded_identifiers() {
        for accounts in [
            vec!["a".into(), "a".into()],
            vec!["*".into()],
            vec!["a b".into()],
            vec!["a".repeat(129)],
            vec!["a".into(); 129],
        ] {
            assert!(RequestedScope::new(accounts, vec![]).is_err());
        }
        for tool in ["", "Mail.read", "mail_read", "mail.*", "mail.한글"] {
            assert!(RequestedScope::new(vec![], vec![tool.into()]).is_err());
        }
        let empty = RequestedScope::new(vec![], vec!["mail.messages.get".into()]).unwrap();
        assert!(!empty.permits_account("account_a", "mail.messages.get"));
        let full = fixture().scope;
        assert!(full.permits_account("account_a", "mail.messages.get"));
        assert!(!full.permits_account("account_a", "mail.send.execute"));
        assert!(!full.permits_account("account_c", "mail.messages.get"));
        let restricted =
            RequestedScope::new(vec!["account_a".into()], vec!["mail.messages.get".into()])
                .unwrap();
        assert_eq!(full.intersection(&restricted), restricted);
    }

    #[test]
    fn sec03_transcript_rejects_invalid_time_epoch_and_sid_without_echo() {
        for (issued, expires, epoch) in [
            (-1, 1, 1),
            (0, 0, 1),
            (20, 10, 1),
            (1, 30002, 1),
            (0, 1, 0),
            (i64::MAX, i64::MAX, 1),
            (i64::MAX - 1, i64::MAX, 1),
        ] {
            let mut t = fixture();
            t.issued_at_utc_ms = issued;
            t.expires_at_utc_ms = expires;
            t.profile_epoch = epoch;
            assert_eq!(
                t.encode(SignerRole::Client),
                Err(crate::ErrorCode::InvalidInput)
            );
        }
        for user in [
            "private@example.invalid",
            "S-01-5-21",
            "S-1-05-21",
            "S-1-5-021",
            "S-1-281474976710656-1",
            "S-1-5-4294967296",
            "S-1-5",
            "S-1-5-1\0",
        ] {
            assert!(PeerContext::new(user, "S-1-5-5-0-1").is_err());
        }
        assert!(PeerContext::new("S-1-5-21-1", "S-1-5-21-1").is_err());
        let debug = format!("{:?}", fixture());
        assert!(!debug.contains("account_a"));
        assert!(!debug.contains("S-1-5"));
    }
}

/// The initial default CLI grant remains deliberately minimal.
pub fn default_cli_legacy_tools() -> Vec<String> {
    vec!["mail.accounts.list".into()]
}

/// Exact read-only tool set presented by an explicit native scope upgrade.
pub fn default_cli_folder_tools() -> Vec<String> {
    vec![
        "mail.accounts.list".into(),
        "mail.folders.list".into(),
        "mail.folders.refresh".into(),
    ]
}

/// Exact UID-page grant offered only by a new explicit native scope review.
pub fn default_cli_uid_tools() -> Vec<String> {
    let mut tools = default_cli_folder_tools();
    tools.push("mail.sync.page".into());
    tools
}

/// Exact cached-header disclosure grant offered by a new native scope review.
pub fn default_cli_metadata_tools() -> Vec<String> {
    let mut tools = default_cli_uid_tools();
    tools.insert(3, "mail.messages.search".into());
    tools
}

fn default_cli_toolset_rank(tools: &[String]) -> Option<u8> {
    if tools == default_cli_legacy_tools() {
        Some(0)
    } else if tools == default_cli_folder_tools() {
        Some(1)
    } else if tools == default_cli_uid_tools() {
        Some(2)
    } else if tools == default_cli_metadata_tools() {
        Some(3)
    } else {
        None
    }
}

/// Validates the canonical order as well as membership; never widens scope.
pub fn is_supported_default_cli_toolset(tools: &[String]) -> bool {
    default_cli_toolset_rank(tools).is_some()
}

/// Equal tool sets may accompany an account change; callers reject a full no-op.
pub fn is_supported_default_cli_toolset_transition(old: &[String], target: &[String]) -> bool {
    match (
        default_cli_toolset_rank(old),
        default_cli_toolset_rank(target),
    ) {
        (Some(old), Some(target)) => old <= target,
        _ => false,
    }
}

#[cfg(test)]
mod default_toolset_tests {
    use super::*;
    #[test]
    fn cli15_metadata_scope_is_exact_and_never_widens_existing_grants() {
        let metadata: Vec<String> = [
            "mail.accounts.list",
            "mail.folders.list",
            "mail.folders.refresh",
            "mail.messages.search",
            "mail.sync.page",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert_eq!(default_cli_metadata_tools(), metadata);
        let sets = [
            default_cli_legacy_tools(),
            default_cli_folder_tools(),
            default_cli_uid_tools(),
            metadata.clone(),
        ];
        assert!(is_supported_default_cli_toolset(&metadata));
        for (old_index, old) in sets.iter().enumerate() {
            let scope = RequestedScope::new(vec!["acct".into()], old.clone()).unwrap();
            assert_eq!(
                scope.permits_account("acct", "mail.messages.search"),
                old_index == 3
            );
            assert!(!scope.permits_account("other", "mail.messages.search"));
            for (target_index, target) in sets.iter().enumerate() {
                assert_eq!(
                    is_supported_default_cli_toolset_transition(old, target),
                    old_index <= target_index
                );
            }
        }
        for tools in [
            vec!["mail.messages.search".into()],
            vec!["mail.accounts.list".into(), "mail.messages.search".into()],
            metadata.iter().rev().cloned().collect(),
            metadata
                .iter()
                .cloned()
                .chain(["mail.send.execute".into()])
                .collect(),
            metadata
                .iter()
                .cloned()
                .chain(["mail.sync.page".into()])
                .collect(),
        ] {
            assert!(!is_supported_default_cli_toolset(&tools));
        }
    }
    #[test]
    fn cli15_uid_scope_transition_is_monotonic_for_every_supported_pair() {
        let sets = [
            default_cli_legacy_tools(),
            default_cli_folder_tools(),
            default_cli_uid_tools(),
        ];
        for (old_index, old) in sets.iter().enumerate() {
            for (new_index, new) in sets.iter().enumerate() {
                assert_eq!(
                    is_supported_default_cli_toolset_transition(old, new),
                    old_index <= new_index
                );
            }
        }
        assert!(!is_supported_default_cli_toolset_transition(&[], &sets[2]));
        assert!(!is_supported_default_cli_toolset_transition(
            &sets[0],
            &["mail.sync.page".into()]
        ));
    }
    #[test]
    fn cli15_uid_scope_accepts_exact_tools_without_widening_old_grants() {
        let mut uid = default_cli_folder_tools();
        uid.push("mail.sync.page".into());
        assert!(is_supported_default_cli_toolset(&uid));
        for tools in [default_cli_legacy_tools(), default_cli_folder_tools()] {
            let scope = RequestedScope::new(vec!["acct".into()], tools).unwrap();
            assert!(!scope.permits_account("acct", "mail.sync.page"));
        }
        let scope = RequestedScope::new(vec!["acct".into()], uid.clone()).unwrap();
        assert!(scope.permits_account("acct", "mail.sync.page"));
        assert!(!scope.permits_account("other", "mail.sync.page"));
        uid.swap(2, 3);
        assert!(!is_supported_default_cli_toolset(&uid));
    }
    #[test]
    fn cli15_default_toolsets_reject_partial_duplicate_and_send_grants() {
        assert!(is_supported_default_cli_toolset(&default_cli_legacy_tools()));
        assert!(is_supported_default_cli_toolset(&default_cli_folder_tools()));
        for tools in [
            vec![],
            vec!["mail.folders.list".into()],
            vec!["mail.accounts.list".into(), "mail.send.execute".into()],
            vec!["mail.accounts.list".into(), "mail.accounts.list".into()],
        ] {
            assert!(!is_supported_default_cli_toolset(&tools));
        }
        let mut reversed = default_cli_folder_tools();
        reversed.reverse();
        assert!(!is_supported_default_cli_toolset(&reversed));
    }
}
