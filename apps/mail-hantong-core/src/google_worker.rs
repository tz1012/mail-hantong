//! Per-account read workers. No Google credential is visible in this module.
use hantong_application::AuditHandoff;
use hantong_contracts::{
    ErrorCode,
    google::{GoogleAccountBinding, GooglePage, GoogleSyncReceipt, GoogleSyncRequest},
    google_mail::GoogleMailRequest,
};
use hantong_store::{RegistrationState, Store, google::PendingGoogleSync};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::Path,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::mpsc;
type Fetcher = Arc<
    dyn Fn(
            GoogleAccountBinding,
            GoogleSyncRequest,
            Arc<AtomicBool>,
        ) -> Result<GooglePage, ErrorCode>
        + Send
        + Sync,
>;
struct Active {
    pending: PendingGoogleSync,
    cancel: Arc<AtomicBool>,
    error: Option<ErrorCode>,
}
struct Response {
    id: [u8; 32],
    page: Result<GooglePage, ErrorCode>,
}
struct Shared<'a> {
    store: &'a Store,
    active: RefCell<BTreeMap<[u8; 32], Active>>,
    sender: mpsc::Sender<Response>,
    fetch: Fetcher,
}
pub(crate) struct GoogleHandle<'a> {
    state: Rc<Shared<'a>>,
}
pub(crate) struct GoogleWorker<'a> {
    state: Rc<Shared<'a>>,
    receiver: mpsc::Receiver<Response>,
}
pub(crate) fn coordinator<'a>(
    store: &'a Store,
    root: &Path,
) -> Result<(GoogleHandle<'a>, GoogleWorker<'a>), ErrorCode> {
    store.recover_google_sync()?;
    let root = root.to_path_buf();
    let fetch = Arc::new(
        move |binding: GoogleAccountBinding,
              request: GoogleSyncRequest,
              cancel: Arc<AtomicBool>| {
            let secrets =
                hantong_secrets::SecretStore::open(&root).map_err(|_| ErrorCode::InternalError)?;
            secrets.fetch_google_page(&binding, &request, &cancel)
        },
    );
    Ok(with_fetcher(store, fetch))
}
pub(crate) fn with_fetcher(store: &Store, fetch: Fetcher) -> (GoogleHandle<'_>, GoogleWorker<'_>) {
    let (sender, receiver) = mpsc::channel(8);
    let state = Rc::new(Shared {
        store,
        active: RefCell::new(BTreeMap::new()),
        sender,
        fetch,
    });
    (
        GoogleHandle {
            state: state.clone(),
        },
        GoogleWorker { state, receiver },
    )
}
impl GoogleHandle<'_> {
    pub(crate) fn request(
        &self,
        audit: [u8; 32],
        owner: [u8; 16],
        epoch: u64,
        r: &GoogleSyncRequest,
    ) -> Result<AuditHandoff<GoogleSyncReceipt>, ErrorCode> {
        if self.state.sender.is_closed() {
            return Err(ErrorCode::CoreUnavailable);
        }
        if self.state.active.borrow().len() >= 8 {
            return Err(ErrorCode::RateLimited);
        }
        let receipt = match self.state.store.admit_google_sync(audit, owner, epoch, r) {
            Ok(r) => r,
            Err(ErrorCode::InternalError) => {
                return Ok(AuditHandoff::error(ErrorCode::InternalError));
            }
            Err(e) => return Err(e),
        };
        if receipt.execution_id != audit || receipt.state != "pending" {
            return Ok(AuditHandoff::response(receipt));
        }
        let pending = match self.state.store.claim_google_sync(audit) {
            Ok(p) => p,
            Err(e) => {
                let failed = self.state.store.fail_google_sync(audit, e);
                return Ok(AuditHandoff::error(failed.err().unwrap_or(e)));
            }
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let sender = self.state.sender.clone();
        let fetch = self.state.fetch.clone();
        let binding = pending.binding.clone();
        let request = pending.request.clone();
        let stop = cancel.clone();
        self.state.active.borrow_mut().insert(
            audit,
            Active {
                pending,
                cancel,
                error: None,
            },
        );
        let spawned = std::thread::Builder::new()
            .name("hantong-google-read".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    fetch(binding, request, stop)
                }))
                .unwrap_or(Err(ErrorCode::InternalError));
                let _ = sender.blocking_send(Response {
                    id: audit,
                    page: result,
                });
            });
        if spawned.is_err() {
            self.state.active.borrow_mut().remove(&audit);
            let failed = self
                .state
                .store
                .fail_google_sync(audit, ErrorCode::CoreUnavailable);
            return Ok(AuditHandoff::error(
                failed.err().unwrap_or(ErrorCode::CoreUnavailable),
            ));
        }
        Ok(AuditHandoff::response(receipt))
    }
}
fn current(store: &Store, p: &PendingGoogleSync) -> Result<(), ErrorCode> {
    let r = store
        .load_registration(p.owner)?
        .ok_or(ErrorCode::ClientNotRegistered)?;
    if r.state != RegistrationState::Active {
        return Err(ErrorCode::ClientSuspended);
    }
    if r.profile_epoch != p.profile_epoch {
        return Err(ErrorCode::AuthRequired);
    }
    if !r
        .scope
        .permits_account(&p.binding.account_id, "mail.sync.page")
    {
        return Err(ErrorCode::ScopeDenied);
    }
    if r.client_kind == hantong_contracts::client::ClientKind::Mcp {
        store.check_mcp_ai_consent(
            p.owner,
            p.profile_epoch,
            std::slice::from_ref(&p.binding.account_id),
        )?
    }
    if store.google_account_binding(&p.binding.account_id)? != p.binding {
        return Err(ErrorCode::Conflict);
    }
    let accounts =
        store.list_account_summaries(std::slice::from_ref(&p.binding.account_id), false)?;
    if !accounts
        .items
        .iter()
        .any(|a| a.status == hantong_domain::accounts::AccountStatus::Ready)
    {
        return Err(ErrorCode::AuthRequired);
    }
    Ok(())
}
impl GoogleWorker<'_> {
    pub(crate) async fn run(mut self) -> Result<(), ErrorCode> {
        let mut timer = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            tokio::select! {
                response=self.receiver.recv()=>{let Some(response)=response else{return Err(ErrorCode::CoreUnavailable)};self.complete(response)?},
                _=timer.tick()=>{for active in self.state.active.borrow_mut().values_mut(){if active.error.is_none()&&let Err(e)=current(self.state.store,&active.pending){active.error=Some(e);active.cancel.store(true,Ordering::SeqCst)}}}
            }
        }
    }
    fn complete(&self, response: Response) -> Result<(), ErrorCode> {
        let Some(active) = self.state.active.borrow_mut().remove(&response.id) else {
            return Ok(());
        };
        let provider_auth_failure = active.error.is_none()
            && current(self.state.store, &active.pending).is_ok()
            && matches!(
                &response.page,
                Err(ErrorCode::AuthRequired | ErrorCode::ScopeDenied)
            );
        let result = active.error.map_or_else(
            || current(self.state.store, &active.pending).and(response.page),
            Err,
        );
        match result {
            Ok(page) => {
                if let Err(e) = self.state.store.complete_google_sync(response.id, &page) {
                    self.state.store.fail_google_sync(response.id, e)?;
                }
            }
            Err(e) => {
                if provider_auth_failure {
                    self.state.store.mark_google_auth_required(
                        &active.pending.binding.account_id,
                        active.pending.binding.credential_version,
                        e,
                    )?
                }
                self.state.store.fail_google_sync(response.id, e)?;
            }
        }
        Ok(())
    }
}

impl Drop for GoogleWorker<'_> {
    fn drop(&mut self) {
        self.receiver.close();
        for active in self.state.active.borrow().values() {
            active.cancel.store(true, Ordering::SeqCst)
        }
    }
}
pub(crate) fn receipt(r: &GoogleSyncReceipt) -> Value {
    json!({"account_id":r.account_id,"execution_id":hantong_contracts::wire_hex::encode(&r.execution_id),"state":r.state,"version":r.version,"next_page_token":r.next_page_token,"error_code":r.error_code.map(ErrorCode::as_str),"downloaded_count":r.downloaded_count})
}
fn meta(m: &hantong_contracts::google::GoogleMessage) -> Value {
    json!({"id":m.id,"thread_id":m.thread_id,"internal_date_ms":m.internal_date_ms,"label_ids":m.label_ids,"subject":m.subject,"sender":m.sender,"snippet":m.snippet,"size":m.size})
}
pub(crate) fn read(
    store: &Store,
    owner: [u8; 16],
    epoch: u64,
    r: &GoogleMailRequest,
) -> Result<(Value, u64), ErrorCode> {
    let account = &r.account_ids()[0];
    let mut out = json!({"schema_version":1,"ok":true,"provider":"gmail","read_only":true,"account_id":account});
    let count = match r {
        GoogleMailRequest::Labels { .. } => {
            let labels = store.google_labels(account)?;
            let count = labels.len() as u64;
            out["labels"] = json!(
                labels
                    .iter()
                    .map(|l| json!({"id":l.id,"name":l.name,"kind":l.kind}))
                    .collect::<Vec<_>>()
            );
            out["sync_version"] = json!(store.google_sync_version(account)?);
            count
        }
        GoogleMailRequest::Messages {
            label_id,
            query,
            limit,
            after,
            ..
        } => {
            let list = store.google_messages(
                account,
                label_id.as_deref(),
                query.as_deref(),
                *limit,
                after.as_deref(),
            )?;
            let count = list.messages.len() as u64;
            out["messages"] = json!(list.messages.iter().map(meta).collect::<Vec<_>>());
            out["next_cursor"] = json!(list.next_cursor);
            out["sync_version"] = json!(store.google_sync_version(account)?);
            count
        }
        GoogleMailRequest::Body {
            message_id,
            max_bytes,
            ..
        } => {
            let m = store.google_message(account, message_id)?;
            let text = m.body_text.ok_or_else(|| {
                m.body_error
                    .as_deref()
                    .and_then(|s| ErrorCode::ALL.into_iter().find(|e| e.as_str() == s))
                    .unwrap_or(ErrorCode::CapabilityUnsupported)
            })?;
            let mut text = hantong_contracts::message_body::sanitize_terminal_text(&text);
            let mut cut = (*max_bytes).min(text.len());
            while !text.is_char_boundary(cut) {
                cut -= 1
            }
            let truncated = m.body_truncated || cut < text.len();
            text.truncate(cut);
            out["body"] = json!({"message_id":m.id,"text":text,"truncated":truncated});
            1
        }
        GoogleMailRequest::Status { execution_id, .. } => {
            out["sync"] =
                receipt(&store.google_sync_status(owner, epoch, account, *execution_id)?);
            0
        }
        GoogleMailRequest::Sync(_) => return Err(ErrorCode::InvalidInput),
    };
    if out.to_string().len() > 512 * 1024 {
        return Err(ErrorCode::QuotaExceeded);
    }
    Ok((out, count))
}

#[cfg(test)]
#[path = "google_worker_tests.rs"]
mod tests;
