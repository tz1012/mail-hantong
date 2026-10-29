use super::*;
use hantong_contracts::{
    auth::RequestedScope,
    imap_setup::{ImapSetupInput, ImapSetupTls},
};
use hantong_store::{
    CliRegistrationStage, ClientKind, DEFAULT_CLI_LABEL, RegistrationState, Store,
};
use std::{
    cell::{Cell, RefCell},
    fs,
    path::PathBuf,
};
fn public() -> [u8; 72] {
    let mut key = [0; 72];
    key[..8].copy_from_slice(b"ECS1\x20\0\0\0");
    key[8..40].copy_from_slice(
        &hantong_contracts::wire_hex::decode_fixed::<32>(
            "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
        )
        .unwrap(),
    );
    key[40..].copy_from_slice(
        &hantong_contracts::wire_hex::decode_fixed::<32>(
            "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
        )
        .unwrap(),
    );
    key
}
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
struct Fixture {
    store: Store,
    _root: Cleanup,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "hantong-scope-core-{}",
            hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
        ));
        fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        store.stage_server_identity([7; 32]).unwrap();
        store
            .activate_server_identity([7; 32], public(), 1)
            .unwrap();
        let req = store
            .create_registration_request(
                [1; 32],
                ClientKind::Cli,
                DEFAULT_CLI_LABEL,
                &RequestedScope::new(vec![], vec!["mail.accounts.list".into()]).unwrap(),
            )
            .unwrap();
        let id = hantong_platform::registration_id_for_request([1; 32]).unwrap();
        let hash = hantong_platform::sha256(&public()).unwrap();
        store
            .stage_cli_registration(&CliRegistrationStage::new(req, id, public(), hash).unwrap())
            .unwrap();
        store.activate_cli_registration(id, 1, hash).unwrap();
        let setup = crate::setup_imap::create(
            &store,
            &ImapSetupInput::new(
                None,
                None,
                "imap.example".into(),
                993,
                ImapSetupTls::ImplicitTls,
                "synthetic".into(),
            )
            .unwrap(),
        )
        .unwrap();
        store.stage_imap_setup_request(&setup).unwrap();
        Self {
            store,
            _root: Cleanup(root),
        }
    }
    fn pending(&self) -> CliScopeRefresh {
        self.store
            .create_cli_scope_refresh(
                [9; 32],
                self.store.list_registrable_account_ids().unwrap(),
                hantong_platform::sha256(&public()).unwrap(),
            )
            .unwrap()
    }
    fn pending_with_scope(&self, tools: Vec<String>) -> CliScopeRefresh {
        self.store
            .create_cli_scope_refresh_with_scope(
                [10; 32],
                RequestedScope::new(self.store.list_registrable_account_ids().unwrap(), tools)
                    .unwrap(),
                hantong_platform::sha256(&public()).unwrap(),
            )
            .unwrap()
    }
}
struct Fake<'a> {
    store: &'a Store,
    events: RefCell<Vec<&'static str>>,
    fail: Option<&'static str>,
    uncertain: bool,
    wrong_key: bool,
    held: std::rc::Rc<Cell<bool>>,
    diagnostics: RefCell<Vec<ErrorCode>>,
    published: RefCell<Option<(Vec<u8>, Vec<u8>)>>,
}
struct Publication(std::rc::Rc<Cell<bool>>);
impl Drop for Publication {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
impl<'a> Fake<'a> {
    fn new(store: &'a Store) -> Self {
        Self {
            store,
            events: RefCell::new(vec![]),
            fail: None,
            uncertain: false,
            wrong_key: false,
            held: std::rc::Rc::new(Cell::new(false)),
            diagnostics: RefCell::new(vec![]),
            published: RefCell::new(None),
        }
    }
    fn hit(&self, name: &'static str) -> Result<(), ErrorCode> {
        self.events.borrow_mut().push(name);
        if self.fail == Some(name) && !self.uncertain {
            Err(ErrorCode::InternalError)
        } else {
            Ok(())
        }
    }
}
// The guard is owned for the activation boundary; this fake records lifetime.
impl Effects for Fake<'_> {
    type Key = ();
    type Publication = Publication;
    fn load(&self, id: [u8; 32]) -> Result<CliScopeRefresh, ErrorCode> {
        self.hit("load")?;
        self.store
            .load_cli_scope_refresh(id)?
            .ok_or(ErrorCode::NotFound)
    }
    fn server_public(&self) -> Result<[u8; 72], ErrorCode> {
        self.hit("server")?;
        Ok(public())
    }
    fn open_key(&self, _: [u8; 16]) -> Result<(), ErrorCode> {
        self.hit("open")
    }
    fn key_public(&self, _: &()) -> Result<[u8; 72], ErrorCode> {
        self.hit("key")?;
        Ok(if self.wrong_key { [0; 72] } else { public() })
    }
    fn stage(&self, r: &CliScopeRefresh) -> Result<CliScopeRefresh, ErrorCode> {
        self.hit("stage")?;
        let r = self.store.stage_cli_scope_refresh(r)?;
        if self.fail == Some("stage") && self.uncertain {
            Err(ErrorCode::InternalError)
        } else {
            Ok(r)
        }
    }
    fn publish(&self, id: [u8; 32], old: &[u8], new: &[u8]) -> Result<Publication, ErrorCode> {
        self.hit("publish")?;
        let row = self.store.load_cli_scope_refresh(id)?.unwrap();
        assert_eq!(row.phase(), ScopeRefreshPhase::Staged);
        let a = ParsedClientProfile::from_json(old)?;
        let b = ParsedClientProfile::from_json(new)?;
        self.published
            .borrow_mut()
            .replace((old.to_vec(), new.to_vec()));
        assert_eq!(a.registration_id(), b.registration_id());
        assert_eq!(a.client_public_key(), b.client_public_key());
        assert_eq!(a.client_key_id(), b.client_key_id());
        assert_eq!(a.server_public_key(), b.server_public_key());
        assert_eq!(a.profile_epoch() + 1, b.profile_epoch());
        assert_eq!(a.scope(), &row.base_registration().scope);
        assert_eq!(b.scope(), &row.target_scope());
        self.held.set(true);
        Ok(Publication(self.held.clone()))
    }
    fn activate(&self, r: &CliScopeRefresh) -> Result<RegistrationRecord, ErrorCode> {
        self.hit("activate")?;
        assert!(self.held.get());
        let record = self.store.activate_cli_scope_refresh(r)?;
        if self.fail == Some("activate") && self.uncertain {
            Err(ErrorCode::InternalError)
        } else {
            Ok(record)
        }
    }
    fn cleanup(&self, _: &mut Publication) -> Result<(), ErrorCode> {
        assert_eq!(
            self.store.load_default_cli_registration()?.unwrap().state,
            RegistrationState::Active
        );
        self.held.set(false);
        self.hit("cleanup")
    }
    fn report(&self, e: ErrorCode) {
        self.diagnostics.borrow_mut().push(e);
    }
}

#[test]
fn cli15_folder_scope_refresh_activates_full_scope_and_recovery_reuses_same_profile_bytes() {
    let fixture = Fixture::new();
    let tools = hantong_contracts::auth::default_cli_folder_tools();
    let r = fixture.pending_with_scope(tools.clone());
    assert_eq!(r.target_scope().tools(), tools.as_slice());
    let mut f = Fake::new(&fixture.store);
    f.fail = Some("activate");
    assert_eq!(
        commit_consumed_review(&f, &r, &review_snapshot(&r).unwrap()),
        Err(ErrorCode::InternalError)
    );
    let bytes = f.published.borrow().clone().expect("publish bytes");
    let staged = fixture
        .store
        .load_cli_scope_refresh(*r.refresh_id())
        .unwrap()
        .unwrap();
    assert_eq!(staged.phase(), ScopeRefreshPhase::Staged);
    assert_eq!(
        fixture
            .store
            .load_default_cli_registration()
            .unwrap()
            .unwrap()
            .scope,
        r.base_registration().scope
    );
    let f = Fake::new(&fixture.store);
    assert_eq!(recover_staged(&f, &staged), Ok(()));
    let recovered = f
        .published
        .borrow()
        .clone()
        .expect("recovery publish bytes");
    assert_eq!(recovered, bytes);
    let active = fixture
        .store
        .load_default_cli_registration()
        .unwrap()
        .unwrap();
    assert_eq!(active.scope.tools(), tools.as_slice());
    assert_eq!(active.scope.accounts(), r.target_scope().accounts());
}
#[test]
fn cli15_scope_refresh_orders_stage_publish_activate_then_cleanup_without_new_identity() {
    let fixture = Fixture::new();
    let r = fixture.pending();
    let f = Fake::new(&fixture.store);
    assert_eq!(
        commit_consumed_review(&f, &r, &review_snapshot(&r).unwrap()),
        Ok(())
    );
    assert_eq!(
        fixture
            .store
            .load_cli_scope_refresh(*r.refresh_id())
            .unwrap()
            .unwrap()
            .phase(),
        ScopeRefreshPhase::Active
    );
    let events = f.events.borrow();
    for pair in [
        ("stage", "open"),
        ("open", "publish"),
        ("publish", "activate"),
        ("activate", "cleanup"),
    ] {
        assert!(
            events.iter().position(|x| *x == pair.0).unwrap()
                < events.iter().position(|x| *x == pair.1).unwrap()
        );
    }
    assert_eq!(
        fixture
            .store
            .load_default_cli_registration()
            .unwrap()
            .unwrap()
            .profile_epoch,
        2
    );
}
#[test]
fn cli15_scope_refresh_failures_never_advance_to_next_effect() {
    for (failed, next) in [
        ("stage", "open"),
        ("open", "publish"),
        ("key", "publish"),
        ("publish", "activate"),
        ("activate", "cleanup"),
    ] {
        for uncertain in [false, true] {
            if uncertain && !["stage", "activate"].contains(&failed) {
                continue;
            }
            let fixture = Fixture::new();
            let r = fixture.pending();
            let mut f = Fake::new(&fixture.store);
            f.fail = Some(failed);
            f.uncertain = uncertain;
            assert_eq!(
                commit_consumed_review(&f, &r, &review_snapshot(&r).unwrap()),
                Err(ErrorCode::InternalError)
            );
            assert!(f.events.borrow().contains(&failed));
            assert!(!f.events.borrow().contains(&next));
            assert!(!f.held.get());
        }
    }
}
#[test]
fn cli15_scope_refresh_recovery_uses_staged_decision_and_reopens_same_key() {
    let fixture = Fixture::new();
    let r = fixture.pending();
    let f = Fake::new(&fixture.store);
    assert_eq!(recover_staged(&f, &r), Err(ErrorCode::Conflict));
    assert!(!f.events.borrow().contains(&"publish"));
    let staged = fixture.store.stage_cli_scope_refresh(&r).unwrap();
    assert_eq!(recover_staged(&f, &staged), Ok(()));
    assert!(!f.events.borrow().contains(&"stage"));
    assert_eq!(recover_staged(&f, &staged), Err(ErrorCode::Conflict));
}
#[test]
fn cli15_scope_refresh_stale_or_changed_review_and_wrong_key_cannot_publish() {
    let fixture = Fixture::new();
    let r = fixture.pending();
    let f = Fake::new(&fixture.store);
    let b = r.base_registration();
    let wrong = ScopeRefreshReview::new(
        [8; 32],
        1,
        b.registration_id,
        b.request_id,
        b.version,
        b.profile_epoch,
        *r.client_public_key_hash(),
        *r.server_public_key_hash(),
        b.scope.accounts().to_vec(),
        r.target_accounts().to_vec(),
        r.created_at_utc_ms(),
        r.expires_at_utc_ms(),
    )
    .unwrap();
    assert_eq!(
        commit_consumed_review(&f, &r, &wrong),
        Err(ErrorCode::Conflict)
    );
    assert!(!f.events.borrow().contains(&"stage"));
    let mut f = Fake::new(&fixture.store);
    f.wrong_key = true;
    assert_eq!(
        commit_consumed_review(&f, &r, &review_snapshot(&r).unwrap()),
        Err(ErrorCode::Conflict)
    );
    assert!(!f.events.borrow().contains(&"publish"));
    assert_eq!(
        commit_consumed_review(&f, &r, &review_snapshot(&r).unwrap()),
        Err(ErrorCode::Conflict)
    );
}
#[test]
fn cli15_scope_refresh_cleanup_failure_does_not_revoke_successful_activation() {
    let fixture = Fixture::new();
    let r = fixture.pending();
    let mut f = Fake::new(&fixture.store);
    f.fail = Some("cleanup");
    assert_eq!(
        commit_consumed_review(&f, &r, &review_snapshot(&r).unwrap()),
        Ok(())
    );
    assert_eq!(
        f.diagnostics.borrow().as_slice(),
        [ErrorCode::InternalError]
    );
    assert_eq!(
        fixture
            .store
            .load_default_cli_registration()
            .unwrap()
            .unwrap()
            .state,
        RegistrationState::Active
    );
}
