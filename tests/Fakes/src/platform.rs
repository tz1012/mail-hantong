//! In-memory platform fixtures. No files, network, process, or shell access.
use hantong_contracts::platform::{
    AutoStart, AutoStartState, IpcTransport, MAX_IPC_FRAME, Notifier, PlatformError, SecretStore,
    UnverifiedEvidence, UserVerifier, VerificationChallenge,
};
use std::collections::BTreeMap;
use std::io::{Cursor, Read};

#[derive(Clone, Eq, PartialEq)]
pub struct FixtureSecretHandle(u64);

impl std::fmt::Debug for FixtureSecretHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("FixtureSecretHandle(<opaque>)")
    }
}

#[derive(Default)]
pub struct FakeSecretStore {
    next: u64,
    values: BTreeMap<u64, (String, String, Vec<u8>)>,
}

impl SecretStore for FakeSecretStore {
    type Handle = FixtureSecretHandle;

    fn store_from_reader(
        &mut self,
        account_id: &str,
        kind: &str,
        reader: &mut dyn Read,
    ) -> Result<Self::Handle, PlatformError> {
        if account_id.is_empty() || kind.is_empty() {
            return Err(PlatformError::InvalidInput);
        }
        let mut value = Vec::new();
        reader
            .take(65_537)
            .read_to_end(&mut value)
            .map_err(|_| PlatformError::InvalidInput)?;
        if value.is_empty() || value.len() > 65_536 {
            return Err(PlatformError::InvalidInput);
        }
        let id = self.next;
        self.next = self
            .next
            .checked_add(1)
            .ok_or(PlatformError::InvalidInput)?;
        self.values
            .insert(id, (account_id.to_owned(), kind.to_owned(), value));
        Ok(FixtureSecretHandle(id))
    }
    fn contains(&self, handle: &Self::Handle) -> bool {
        self.values.contains_key(&handle.0)
    }
    fn verify(&mut self, handle: &Self::Handle) -> Result<(), PlatformError> {
        self.values
            .contains_key(&handle.0)
            .then_some(())
            .ok_or(PlatformError::NotFound)
    }
    fn delete(&mut self, handle: &Self::Handle) -> Result<(), PlatformError> {
        self.values
            .remove(&handle.0)
            .map(|_| ())
            .ok_or(PlatformError::NotFound)
    }
}

impl FakeSecretStore {
    pub fn verify_for_account(
        &self,
        account_id: &str,
        handle: &FixtureSecretHandle,
    ) -> Result<(), PlatformError> {
        match self.values.get(&handle.0) {
            Some((stored_account, _, _)) if stored_account == account_id => Ok(()),
            Some(_) => Err(PlatformError::Denied),
            None => Err(PlatformError::NotFound),
        }
    }
}

#[derive(Default)]
pub struct FakeIpcTransport {
    pub frames: Vec<Vec<u8>>,
    pub response: Vec<u8>,
}
impl IpcTransport for FakeIpcTransport {
    fn send_frame(&mut self, frame: &[u8]) -> Result<Vec<u8>, PlatformError> {
        if frame.is_empty() || frame.len() > MAX_IPC_FRAME {
            return Err(PlatformError::FrameTooLarge);
        }
        self.frames.push(frame.to_vec());
        Ok(self.response.clone())
    }
}

#[derive(Default)]
pub struct FakeNotifier {
    pub events: Vec<(String, String)>,
}
impl Notifier for FakeNotifier {
    fn notify(&mut self, message_key: &str, object_ref: &str) -> Result<(), PlatformError> {
        if message_key.is_empty()
            || object_ref.is_empty()
            || message_key.contains('\n')
            || object_ref.contains('\n')
        {
            return Err(PlatformError::InvalidInput);
        }
        self.events
            .push((message_key.to_owned(), object_ref.to_owned()));
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeUserVerifier {
    pub allow_fixture: bool,
}
impl UserVerifier for FakeUserVerifier {
    fn verify(&mut self, c: &VerificationChallenge) -> Result<UnverifiedEvidence, PlatformError> {
        if c.request_id.is_empty()
            || c.account_id.is_empty()
            || c.expires_at_unix == 0
            || !self.allow_fixture
        {
            return Err(PlatformError::Denied);
        }
        Ok(UnverifiedEvidence {
            request_id: c.request_id.clone(),
            account_id: c.account_id.clone(),
            operation_hash: c.operation_hash,
            nonce: c.nonce,
            expires_at_unix: c.expires_at_unix,
            profile_epoch: c.profile_epoch,
            assertion: b"fake-unverified-evidence".to_vec(),
        })
    }
}

pub struct FakeAutoStart {
    state: AutoStartState,
}
impl Default for FakeAutoStart {
    fn default() -> Self {
        Self {
            state: AutoStartState::Disabled,
        }
    }
}
impl AutoStart for FakeAutoStart {
    fn set_enabled(&mut self, enabled: bool) -> Result<(), PlatformError> {
        self.state = if enabled {
            AutoStartState::Enabled
        } else {
            AutoStartState::Disabled
        };
        Ok(())
    }
    fn state(&self) -> Result<AutoStartState, PlatformError> {
        Ok(self.state)
    }
}

pub fn reader(value: &[u8]) -> Cursor<Vec<u8>> {
    Cursor::new(value.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sec01_secret_fixture_is_in_memory_and_bounded() {
        let mut s = FakeSecretStore::default();
        let mut r = reader(b"fixture");
        let h = s.store_from_reader("account-a", "oauth", &mut r).unwrap();
        assert_eq!(
            s.verify_for_account("account-b", &h),
            Err(PlatformError::Denied)
        );
        assert_eq!(s.verify_for_account("account-a", &h), Ok(()));
        assert!(s.contains(&h));
        s.delete(&h).unwrap();
        assert!(!s.contains(&h));
    }
    #[test]
    fn sec03_ipc_rejects_empty_and_oversized_frames() {
        let mut i = FakeIpcTransport::default();
        assert_eq!(i.send_frame(&[]), Err(PlatformError::FrameTooLarge));
        assert_eq!(
            i.send_frame(&vec![0; MAX_IPC_FRAME + 1]),
            Err(PlatformError::FrameTooLarge)
        );
    }
    #[test]
    fn sec08_notifications_are_keys_and_refs_only() {
        let mut n = FakeNotifier::default();
        n.notify("mail.new", "message-1").unwrap();
        assert_eq!(n.events.len(), 1);
        assert_eq!(
            n.notify("body\nsecret", "message-2"),
            Err(PlatformError::InvalidInput)
        );
    }
    #[test]
    fn apr01_verification_defaults_to_denied_and_returns_unverified_evidence() {
        let c = VerificationChallenge {
            request_id: "r1".into(),
            account_id: "account-a".into(),
            operation_hash: [1; 32],
            nonce: [2; 32],
            expires_at_unix: 1,
            profile_epoch: 1,
        };
        let mut v = FakeUserVerifier::default();
        assert_eq!(v.verify(&c), Err(PlatformError::Denied));
        v.allow_fixture = true;
        let e = v.verify(&c).unwrap();
        assert_eq!(e.assertion, b"fake-unverified-evidence");
        assert_eq!(e.operation_hash, c.operation_hash);
    }

    #[test]
    fn platform_debug_redacts_scope_and_secret_handle_contents() {
        let challenge = VerificationChallenge {
            request_id: "request-secret".into(),
            account_id: "account-secret".into(),
            operation_hash: [7; 32],
            nonce: [8; 32],
            expires_at_unix: 1,
            profile_epoch: 1,
        };
        let challenge_debug = format!("{challenge:?}");
        assert!(!challenge_debug.contains("request-secret"));
        assert!(!challenge_debug.contains("account-secret"));

        let handle = FixtureSecretHandle(42);
        assert!(!format!("{handle:?}").contains("42"));
    }

    #[test]
    fn inst13_autostart_is_explicit_state_only() {
        let mut a = FakeAutoStart::default();
        assert_eq!(a.state(), Ok(AutoStartState::Disabled));
        a.set_enabled(true).unwrap();
        assert_eq!(a.state(), Ok(AutoStartState::Enabled));
    }
}
