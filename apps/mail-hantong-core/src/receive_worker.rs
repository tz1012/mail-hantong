//! Opt-in background receive observations. No message content or write operation is fetched.
use hantong_contracts::{
    ErrorCode,
    audit::{AuditBegin, AuditTerminal, AuditTool},
    google::GoogleAccountBinding,
};
use hantong_domain::{
    accounts::AccountProvider,
    imap_observation::ImapUidObservationRequest,
    mail_connection::ImapEndpointConfig,
    mailboxes::{ImapMailboxName, MailboxEncoding},
};
use hantong_policy::ReadPolicy;
use hantong_store::{RegistrationRecord, RegistrationState, Store};
use std::{
    cell::{Cell, RefCell},
    path::Path,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Eq, PartialEq)]
enum Binding {
    Google(GoogleAccountBinding),
    Imap(ImapEndpointConfig),
}

struct ProviderLease {
    slot: Rc<Cell<Option<[u8; 32]>>>,
    id: [u8; 32],
    reaped: bool,
}
impl Drop for ProviderLease {
    fn drop(&mut self) {
        // Cancellation may detach a bounded blocking read. Fail closed until Core restarts.
        if self.reaped && self.slot.get() == Some(self.id) {
            self.slot.set(None);
        }
    }
}

struct AuditLease<'a> {
    store: &'a Store,
    id: [u8; 32],
    finished: bool,
}
impl Drop for AuditLease<'_> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self
                .store
                .finish_audit(self.id, AuditTerminal::failed(ErrorCode::CoreUnavailable));
        }
    }
}

fn binding(store: &Store, account: &str) -> Result<Binding, ErrorCode> {
    let snapshot = store.list_account_summaries(&[account.to_owned()], false)?;
    let provider = snapshot.items.first().ok_or(ErrorCode::NotFound)?.provider;
    // Display metadata only selects the adapter; the typed binding still validates its identity.
    match provider {
        AccountProvider::Gmail => Ok(Binding::Google(store.google_account_binding(account)?)),
        AccountProvider::Imap => Ok(Binding::Imap(
            store
                .load_mail_connection(account)?
                .ok_or(ErrorCode::NotFound)?
                .imap()
                .clone(),
        )),
        AccountProvider::Graph | AccountProvider::Archive => Err(ErrorCode::CapabilityUnsupported),
    }
}

fn current(store: &Store, owner: &RegistrationRecord, account: &str) -> Result<(), ErrorCode> {
    let now = store
        .load_registration(owner.registration_id)?
        .ok_or(ErrorCode::ClientNotRegistered)?;
    if now.state != RegistrationState::Active {
        return Err(ErrorCode::ClientSuspended);
    }
    if now.profile_epoch != owner.profile_epoch {
        return Err(ErrorCode::AuthRequired);
    }
    if !now.scope.permits_account(account, "mail.events.read")
        || !now.scope.permits_account(account, "mail.sync.page")
    {
        return Err(ErrorCode::ScopeDenied);
    }
    store.check_mcp_ai_consent(
        now.registration_id,
        now.profile_epoch,
        &[account.to_owned()],
    )?;
    let enabled = store.list_mail_event_tracking_targets(now.registration_id, now.profile_epoch)?;
    if !enabled
        .iter()
        .any(|target| target.target.account_id == account)
    {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(())
}

async fn observe(root: &Path, account: &str, source: &Binding) -> Result<[u8; 32], ErrorCode> {
    let root = root.to_path_buf();
    let account = account.to_owned();
    let source = source.clone();
    let stop = Arc::new(AtomicBool::new(false));
    struct Cancel(Arc<AtomicBool>);
    impl Drop for Cancel {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let _cancel = Cancel(stop.clone());
    let task = tokio::task::spawn_blocking(move || {
        let secrets =
            hantong_secrets::SecretStore::open(&root).map_err(|_| ErrorCode::InternalError)?;
        match source {
            Binding::Google(source) => secrets.fetch_google_inbox_observation(&source, &stop),
            Binding::Imap(endpoint) => {
                let mailbox = ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::Utf8)
                    .map_err(|_| ErrorCode::InvalidInput)?;
                let request =
                    ImapUidObservationRequest::new(&account, endpoint.settings_version(), mailbox)
                        .map_err(|_| ErrorCode::InvalidInput)?;
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|_| ErrorCode::CoreUnavailable)?
                    .block_on(async {
                        let result = secrets
                            .fetch_imap_uid_observation(&request, &endpoint)
                            .await
                            .map_err(crate::uid_worker::uid_error)?;
                        let mut bytes = b"mail-hantong.receive-imap.v1\0".to_vec();
                        bytes.extend_from_slice(&result.uid_validity().to_be_bytes());
                        bytes.extend_from_slice(&result.uid_next().to_be_bytes());
                        hantong_platform::sha256(&bytes)
                    })
            }
        }
    });
    // Secrets bounds provider I/O. Keep the shared slot until its blocking task is reaped.
    task.await.map_err(|_| ErrorCode::InternalError)?
}

pub(crate) async fn run(
    store: &Store,
    root: &Path,
    slot: Rc<Cell<Option<[u8; 32]>>>,
    policy: &RefCell<ReadPolicy>,
) -> Result<(), ErrorCode> {
    let mut timer = tokio::time::interval(Duration::from_secs(60));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        timer.tick().await;
        let Some(owner) = store.load_mcp_registration()? else {
            continue;
        };
        let targets = match store
            .list_mail_event_tracking_targets(owner.registration_id, owner.profile_epoch)
        {
            Ok(targets) => targets,
            Err(
                ErrorCode::ClientSuspended
                | ErrorCode::AuthRequired
                | ErrorCode::ScopeDenied
                | ErrorCode::PolicyBlocked,
            ) => continue,
            Err(error) => return Err(error),
        };
        for target in targets {
            if slot.get().is_some() {
                break;
            }
            let account = &target.target.account_id;
            if current(store, &owner, account).is_err() {
                continue;
            }
            let source = match binding(store, account) {
                Ok(source) => source,
                Err(_) => continue,
            };
            if policy
                .borrow_mut()
                .check(
                    owner.registration_id,
                    owner.read_only,
                    owner.requests_per_minute,
                    AuditTool::EventsRead,
                    Instant::now(),
                )
                .is_err()
            {
                break;
            }
            let id = hantong_platform::random_nonce()?;
            store.begin_audit(&AuditBegin::new(
                id,
                owner.registration_id,
                owner.profile_epoch,
                AuditTool::EventsRead,
                vec![account.clone()],
            )?)?;
            slot.set(Some(id));
            let mut lease = ProviderLease {
                slot: slot.clone(),
                id,
                reaped: false,
            };
            let mut audit = AuditLease {
                store,
                id,
                finished: false,
            };
            let observed = observe(root, account, &source).await;
            lease.reaped = true;
            let result = match observed {
                Ok(hash) => current(store, &owner, account).and_then(|()| {
                    if binding(store, account)? != source {
                        return Err(ErrorCode::Conflict);
                    }
                    store.commit_mail_event_observation(
                        owner.registration_id,
                        owner.profile_epoch,
                        account,
                        &target.target.mailbox_key,
                        &target.target.mailbox_encoding,
                        hash,
                    )
                }),
                Err(error) => Err(error),
            };
            let terminal = match result {
                Ok(event) => AuditTerminal::succeeded(Some(u64::from(event.is_some())))?,
                Err(error) => AuditTerminal::failed(error),
            };
            audit.finished = true; // A commit with an uncertain outcome must not be retried by Drop.
            store.finish_audit(id, terminal)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mcp07_cancelled_provider_read_does_not_release_slot_before_reap() {
        let slot = Rc::new(Cell::new(Some([1; 32])));
        drop(ProviderLease {
            slot: slot.clone(),
            id: [1; 32],
            reaped: false,
        });
        assert_eq!(slot.get(), Some([1; 32]));
        drop(ProviderLease {
            slot: slot.clone(),
            id: [1; 32],
            reaped: true,
        });
        assert_eq!(slot.get(), None);
        slot.set(Some([2; 32]));
        drop(ProviderLease {
            slot: slot.clone(),
            id: [1; 32],
            reaped: true,
        });
        assert_eq!(slot.get(), Some([2; 32]));
    }
}
