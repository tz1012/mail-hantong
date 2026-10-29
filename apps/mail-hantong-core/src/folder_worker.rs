//! One-shot folder discovery behind durable admission and a single reserved slot.
use hantong_contracts::{ErrorCode, folders::FolderRefreshRequest};
use hantong_domain::mailboxes::ImapMailboxSnapshot;
use hantong_store::{
    Store,
    folder_refresh::{
        FolderQueueAdmission, FolderRefreshAdmission, FolderRefreshJob, FolderRefreshStart,
    },
};
use std::{cell::Cell, future::Future, path::Path, rc::Rc, time::Duration};
use tokio::sync::mpsc;

type Job = [u8; 32];
struct Queue {
    sender: mpsc::Sender<Job>,
    scheduled: Rc<Cell<Option<Job>>>,
}
struct Slot {
    permit: Option<mpsc::OwnedPermit<Job>>,
    scheduled: Rc<Cell<Option<Job>>>,
    id: Job,
}
impl Drop for Slot {
    fn drop(&mut self) {
        if self.permit.is_some() && self.scheduled.get() == Some(self.id) {
            self.scheduled.set(None);
        }
    }
}
impl Slot {
    fn submit(mut self) {
        self.permit
            .take()
            .expect("owned queue permit")
            .send(self.id);
    }
}
impl Queue {
    fn reserve(&self, id: Job) -> Result<Slot, FolderQueueAdmission> {
        if self.sender.is_closed() {
            return Err(FolderQueueAdmission::Closed);
        }
        if self.scheduled.get().is_some() {
            return Err(FolderQueueAdmission::Full);
        }
        let permit = self
            .sender
            .clone()
            .try_reserve_owned()
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => FolderQueueAdmission::Full,
                mpsc::error::TrySendError::Closed(_) => FolderQueueAdmission::Closed,
            })?;
        self.scheduled.set(Some(id));
        Ok(Slot {
            permit: Some(permit),
            scheduled: self.scheduled.clone(),
            id,
        })
    }
}
fn queue() -> (Queue, mpsc::Receiver<Job>) {
    let (sender, receiver) = mpsc::channel(1);
    (
        Queue {
            sender,
            scheduled: Rc::new(Cell::new(None)),
        },
        receiver,
    )
}

pub(crate) struct FolderWorkerHandle<'a> {
    store: &'a Store,
    queue: Queue,
}
pub(crate) struct FolderWorker<'a, D> {
    store: &'a Store,
    receiver: mpsc::Receiver<Job>,
    scheduled: Rc<Cell<Option<Job>>>,
    discovery: D,
}
pub(crate) trait Discovery {
    fn discover(
        &self,
        job: &FolderRefreshJob,
    ) -> impl Future<Output = Result<ImapMailboxSnapshot, ErrorCode>>;
}
pub(crate) struct NativeDiscovery<'a> {
    secrets_root: &'a Path,
}
impl Discovery for NativeDiscovery<'_> {
    async fn discover(&self, job: &FolderRefreshJob) -> Result<ImapMailboxSnapshot, ErrorCode> {
        let secrets = hantong_secrets::SecretStore::open(self.secrets_root)
            .map_err(|_| ErrorCode::InternalError)?;
        secrets
            .discover_imap_mailboxes(job.request().account_id(), job.endpoint())
            .await
            .map_err(probe_error)
    }
}
fn probe_error(error: hantong_secrets::MailProbeError) -> ErrorCode {
    use hantong_secrets::{MailProbeError as E, SecretError as S};
    match error {
        E::InvalidAccount => ErrorCode::InvalidInput,
        E::UnsupportedPlatform | E::MechanismUnsupported => ErrorCode::CapabilityUnsupported,
        E::Authentication | E::Credential(S::NotFound | S::CredentialMismatch) => {
            ErrorCode::AuthRequired
        }
        E::Connection | E::Timeout | E::Tls => ErrorCode::Offline,
        E::Protocol | E::Credential(_) => ErrorCode::InternalError,
    }
}
pub(crate) fn coordinator_for_store<'a>(
    store: &'a Store,
    secrets_root: &'a Path,
) -> (
    FolderWorkerHandle<'a>,
    FolderWorker<'a, NativeDiscovery<'a>>,
) {
    coordinator_with_discovery(store, NativeDiscovery { secrets_root })
}
pub(crate) fn coordinator_with_discovery<D: Discovery>(
    store: &Store,
    discovery: D,
) -> (FolderWorkerHandle<'_>, FolderWorker<'_, D>) {
    let (queue, receiver) = queue();
    let scheduled = queue.scheduled.clone();
    (
        FolderWorkerHandle { store, queue },
        FolderWorker {
            store,
            receiver,
            scheduled,
            discovery,
        },
    )
}
impl FolderWorkerHandle<'_> {
    pub(crate) fn admit(
        &self,
        id: Job,
        request: &FolderRefreshRequest,
    ) -> Result<FolderRefreshAdmission, ErrorCode> {
        let (slot, queue) = match self.queue.reserve(id) {
            Ok(slot) => (Some(slot), FolderQueueAdmission::Reserved),
            Err(queue) => (None, queue),
        };
        let outcome = self.store.admit_folder_refresh(id, request, queue)?;
        if matches!(outcome, FolderRefreshAdmission::Admitted(_)) {
            // There is no await or fallible step between admission and sending a reserved slot.
            slot.expect("Store admits only a reserved queue slot")
                .submit();
        }
        Ok(outcome)
    }
}
impl<D> Drop for FolderWorker<'_, D> {
    fn drop(&mut self) {
        self.scheduled.set(None);
    }
}
impl<D: Discovery> FolderWorker<'_, D> {
    pub(crate) async fn run(mut self) -> Result<(), ErrorCode> {
        while let Some(id) = self.receiver.recv().await {
            self.run_one(id).await?;
            if self.scheduled.get() == Some(id) {
                self.scheduled.set(None);
            }
        }
        Ok(())
    }
    async fn run_one(&self, id: Job) -> Result<(), ErrorCode> {
        let job = match self.store.start_folder_refresh(id)? {
            FolderRefreshStart::Ready(job) => job,
            FolderRefreshStart::Finished(_) => return Ok(()),
        };
        let outcome = if let Err(error) = self.store.check_folder_refresh(id) {
            if error == ErrorCode::InternalError {
                return Err(error);
            }
            Err(error)
        } else {
            // Leaving this scope drops a pending network future before any terminal write.
            let discovery = self.discovery.discover(&job);
            tokio::pin!(discovery);
            let mut ticker = tokio::time::interval_at(
                tokio::time::Instant::now() + Duration::from_millis(500),
                Duration::from_millis(500),
            );
            loop {
                tokio::select! {
                    biased;
                    _=ticker.tick()=>{if let Err(error)=self.store.check_folder_refresh(id){if error==ErrorCode::InternalError{return Err(error)} break Err(error)}},
                    result=&mut discovery=>break result,
                }
            }
        };
        match outcome {
            // Stable rejections are already committed by Store. A remaining error
            // may be an uncertain commit: stop, never issue a second terminal mutation.
            Ok(snapshot) => self
                .store
                .complete_folder_refresh_success(id, &snapshot)
                .map(|_| ()),
            Err(error) => self
                .store
                .complete_folder_refresh_failure(id, error)
                .map(|_| ()),
        }
    }
}

#[cfg(test)]
pub(crate) struct NoDiscovery;
#[cfg(test)]
impl Discovery for NoDiscovery {
    async fn discover(&self, _: &FolderRefreshJob) -> Result<ImapMailboxSnapshot, ErrorCode> {
        panic!("unexpected discovery in non-folder test")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor = "current_thread")]
    async fn m203_capacity_one_remains_reserved_during_execution() {
        let (q, mut receiver) = queue();
        let slot = q.reserve([1; 32]).unwrap();
        assert!(matches!(
            q.reserve([2; 32]),
            Err(FolderQueueAdmission::Full)
        ));
        slot.submit();
        assert_eq!(receiver.recv().await, Some([1; 32]));
        assert!(matches!(
            q.reserve([2; 32]),
            Err(FolderQueueAdmission::Full)
        ));
        q.scheduled.set(None);
        assert!(q.reserve([2; 32]).is_ok());
    }
    #[tokio::test(flavor = "current_thread")]
    async fn m203_unsubmitted_reservation_releases_without_effect() {
        let (q, mut receiver) = queue();
        drop(q.reserve([3; 32]).unwrap());
        assert_eq!(q.scheduled.get(), None);
        assert!(receiver.try_recv().is_err());
        drop(receiver);
        assert!(matches!(
            q.reserve([4; 32]),
            Err(FolderQueueAdmission::Closed)
        ));
    }
    #[test]
    fn ext03_folder_probe_errors_are_stable_without_secret_details() {
        use hantong_secrets::MailProbeError as E;
        assert_eq!(probe_error(E::Authentication), ErrorCode::AuthRequired);
        assert_eq!(
            probe_error(E::Credential(
                hantong_secrets::SecretError::CredentialMismatch
            )),
            ErrorCode::AuthRequired
        );
        assert_eq!(probe_error(E::Tls), ErrorCode::Offline);
        assert_eq!(probe_error(E::Protocol), ErrorCode::InternalError);
        assert_eq!(
            probe_error(E::MechanismUnsupported),
            ErrorCode::CapabilityUnsupported
        );
    }
}
