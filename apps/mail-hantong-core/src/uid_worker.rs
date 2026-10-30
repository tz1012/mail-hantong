//! One-shot UID observation sharing the global IMAP operation slot.
use hantong_contracts::uid_reconcile::UidReconcileRequest;
use hantong_contracts::{ErrorCode, uid_page::UidPageRequest};
use hantong_domain::imap_reconcile::ImapUidReconcilePage;
use hantong_domain::imap_sync::ImapUidPage;
use hantong_store::uid_reconcile_jobs::{
    UidReconcileAdmission, UidReconcileJob, UidReconcileStart,
};
use hantong_store::{
    Store,
    uid_page_jobs::{UidPageAdmission, UidPageJob, UidPageStart, UidQueueAdmission},
};
use std::{cell::Cell, future::Future, path::Path, rc::Rc, time::Duration};
use tokio::sync::mpsc;

type Job = [u8; 32];
#[derive(Debug, Eq, PartialEq)]
enum QueuedJob {
    Page(Job),
    Reconcile(Job),
}
struct Queue {
    sender: mpsc::Sender<QueuedJob>,
    scheduled: Rc<Cell<Option<Job>>>,
    owned: Rc<Cell<Option<Job>>>,
}
struct Slot {
    permit: Option<mpsc::OwnedPermit<QueuedJob>>,
    scheduled: Rc<Cell<Option<Job>>>,
    owned: Rc<Cell<Option<Job>>>,
    id: Job,
}
impl Drop for Slot {
    fn drop(&mut self) {
        if self.permit.is_some() && self.scheduled.get() == Some(self.id) {
            self.scheduled.set(None);
            self.owned.set(None);
        }
    }
}
impl Slot {
    fn submit(mut self) {
        self.permit
            .take()
            .expect("owned queue permit")
            .send(QueuedJob::Page(self.id));
    }
    fn submit_reconcile(mut self) {
        self.permit
            .take()
            .expect("owned queue permit")
            .send(QueuedJob::Reconcile(self.id));
    }
}
impl Queue {
    fn reserve(&self, id: Job) -> Result<Slot, UidQueueAdmission> {
        if self.sender.is_closed() {
            return Err(UidQueueAdmission::Closed);
        }
        if self.scheduled.get().is_some() {
            return Err(UidQueueAdmission::Full);
        }
        let permit = self
            .sender
            .clone()
            .try_reserve_owned()
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => UidQueueAdmission::Full,
                mpsc::error::TrySendError::Closed(_) => UidQueueAdmission::Closed,
            })?;
        self.scheduled.set(Some(id));
        self.owned.set(Some(id));
        Ok(Slot {
            permit: Some(permit),
            scheduled: self.scheduled.clone(),
            owned: self.owned.clone(),
            id,
        })
    }
}
fn queue(shared: Rc<Cell<Option<Job>>>) -> (Queue, mpsc::Receiver<QueuedJob>) {
    let (sender, receiver) = mpsc::channel(1);
    (
        Queue {
            sender,
            scheduled: shared,
            owned: Rc::new(Cell::new(None)),
        },
        receiver,
    )
}

pub(crate) struct UidWorkerHandle<'a> {
    store: &'a Store,
    queue: Queue,
}
pub(crate) struct UidWorker<'a, D> {
    store: &'a Store,
    receiver: mpsc::Receiver<QueuedJob>,
    scheduled: Rc<Cell<Option<Job>>>,
    owned: Rc<Cell<Option<Job>>>,
    discovery: D,
}
pub(crate) trait Discovery {
    fn discover(&self, job: &UidPageJob) -> impl Future<Output = Result<ImapUidPage, ErrorCode>>;
    async fn reconcile(&self, _job: &UidReconcileJob) -> Result<ImapUidReconcilePage, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
}
pub(crate) struct NativeDiscovery<'a> {
    secrets_root: &'a Path,
}
impl Discovery for NativeDiscovery<'_> {
    async fn reconcile(&self, job: &UidReconcileJob) -> Result<ImapUidReconcilePage, ErrorCode> {
        let secrets = hantong_secrets::SecretStore::open(self.secrets_root)
            .map_err(|_| ErrorCode::InternalError)?;
        secrets
            .fetch_imap_uid_reconcile_page(job.fetch_request(), job.endpoint())
            .await
            .map_err(uid_error)
    }
    async fn discover(&self, job: &UidPageJob) -> Result<ImapUidPage, ErrorCode> {
        let secrets = hantong_secrets::SecretStore::open(self.secrets_root)
            .map_err(|_| ErrorCode::InternalError)?;
        secrets
            .fetch_imap_uid_page(job.fetch_request(), job.endpoint())
            .await
            .map_err(uid_error)
    }
}
fn uid_error(error: hantong_secrets::ImapUidPageError) -> ErrorCode {
    match error {
        hantong_secrets::ImapUidPageError::Probe(error) => probe_error(error),
        hantong_secrets::ImapUidPageError::UidValidityChanged
        | hantong_secrets::ImapUidPageError::CursorInvalid => ErrorCode::Conflict,
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
    shared: Rc<Cell<Option<Job>>>,
) -> (UidWorkerHandle<'a>, UidWorker<'a, NativeDiscovery<'a>>) {
    coordinator_with_discovery(store, NativeDiscovery { secrets_root }, shared)
}
pub(crate) fn coordinator_with_discovery<D: Discovery>(
    store: &Store,
    discovery: D,
    shared: Rc<Cell<Option<Job>>>,
) -> (UidWorkerHandle<'_>, UidWorker<'_, D>) {
    let (queue, receiver) = queue(shared);
    let scheduled = queue.scheduled.clone();
    let owned = queue.owned.clone();
    (
        UidWorkerHandle { store, queue },
        UidWorker {
            store,
            receiver,
            scheduled,
            owned,
            discovery,
        },
    )
}
impl UidWorkerHandle<'_> {
    pub(crate) fn admit_reconcile(
        &self,
        id: Job,
        request: &UidReconcileRequest,
    ) -> Result<UidReconcileAdmission, ErrorCode> {
        let (slot, queue) = match self.queue.reserve(id) {
            Ok(slot) => (Some(slot), UidQueueAdmission::Reserved),
            Err(queue) => (None, queue),
        };
        let outcome = self.store.admit_uid_reconcile(id, request, queue)?;
        if matches!(outcome, UidReconcileAdmission::Admitted(_)) {
            slot.expect("Store admits only a reserved queue slot")
                .submit_reconcile();
        }
        Ok(outcome)
    }
    pub(crate) fn admit(
        &self,
        id: Job,
        request: &UidPageRequest,
    ) -> Result<UidPageAdmission, ErrorCode> {
        let (slot, queue) = match self.queue.reserve(id) {
            Ok(slot) => (Some(slot), UidQueueAdmission::Reserved),
            Err(queue) => (None, queue),
        };
        let outcome = self.store.admit_uid_page(id, request, queue)?;
        if matches!(outcome, UidPageAdmission::Admitted(_)) {
            // There is no await or fallible step between admission and sending a reserved slot.
            slot.expect("Store admits only a reserved queue slot")
                .submit();
        }
        Ok(outcome)
    }
}
impl<D> Drop for UidWorker<'_, D> {
    fn drop(&mut self) {
        if let Some(id) = self.owned.get()
            && self.scheduled.get() == Some(id)
        {
            self.scheduled.set(None);
        }
        self.owned.set(None);
    }
}
impl<D: Discovery> UidWorker<'_, D> {
    pub(crate) async fn run(mut self) -> Result<(), ErrorCode> {
        while let Some(queued) = self.receiver.recv().await {
            let id = match queued {
                QueuedJob::Page(id) => {
                    self.run_one(id).await?;
                    id
                }
                QueuedJob::Reconcile(id) => {
                    self.run_reconcile(id).await?;
                    id
                }
            };
            if self.scheduled.get() == Some(id) {
                self.scheduled.set(None);
            }
            self.owned.set(None);
        }
        Ok(())
    }
    async fn run_reconcile(&self, id: Job) -> Result<(), ErrorCode> {
        let job = match self.store.start_uid_reconcile(id)? {
            UidReconcileStart::Ready(job) => job,
            UidReconcileStart::Finished(_) => return Ok(()),
        };
        let outcome = if let Err(error) = self.store.check_uid_reconcile(id) {
            if error == ErrorCode::InternalError {
                return Err(error);
            }
            Err(error)
        } else {
            let discovery = self.discovery.reconcile(&job);
            tokio::pin!(discovery);
            let mut ticker = tokio::time::interval_at(
                tokio::time::Instant::now() + Duration::from_millis(500),
                Duration::from_millis(500),
            );
            loop {
                tokio::select! {
                    biased;
                    _=ticker.tick()=>{if let Err(error)=self.store.check_uid_reconcile(id){if error==ErrorCode::InternalError{return Err(error)} break Err(error)}},
                    result=&mut discovery=>break result,
                }
            }
        };
        // As in append, an uncertain commit never triggers a second terminal write.
        match outcome {
            Ok(page) => self
                .store
                .complete_uid_reconcile_success(id, &page)
                .map(|_| ()),
            Err(error) => self
                .store
                .complete_uid_reconcile_failure(id, error)
                .map(|_| ()),
        }
    }
    async fn run_one(&self, id: Job) -> Result<(), ErrorCode> {
        let job = match self.store.start_uid_page(id)? {
            UidPageStart::Ready(job) => job,
            UidPageStart::Finished(_) => return Ok(()),
        };
        let outcome = if let Err(error) = self.store.check_uid_page(id) {
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
                    _=ticker.tick()=>{if let Err(error)=self.store.check_uid_page(id){if error==ErrorCode::InternalError{return Err(error)} break Err(error)}},
                    result=&mut discovery=>break result,
                }
            }
        };
        match outcome {
            // Stable rejections are already committed by Store. A remaining error
            // may be an uncertain commit: stop, never issue a second terminal mutation.
            Ok(snapshot) => self
                .store
                .complete_uid_page_success(id, &snapshot)
                .map(|_| ()),
            Err(error) => self.store.complete_uid_page_failure(id, error).map(|_| ()),
        }
    }
}

#[cfg(test)]
pub(crate) struct NoDiscovery;
#[cfg(test)]
impl Discovery for NoDiscovery {
    async fn discover(&self, _: &UidPageJob) -> Result<ImapUidPage, ErrorCode> {
        panic!("unexpected discovery in non-UID test")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor = "current_thread")]
    async fn m204_reconcile_uses_the_same_owned_network_slot_as_append() {
        let (q, mut receiver) = queue(Rc::new(Cell::new(None)));
        q.reserve([5; 32]).unwrap().submit_reconcile();
        assert!(matches!(q.reserve([6; 32]), Err(UidQueueAdmission::Full)));
        assert_eq!(receiver.recv().await, Some(QueuedJob::Reconcile([5; 32])));
        assert!(matches!(q.reserve([6; 32]), Err(UidQueueAdmission::Full)));
    }
    #[tokio::test(flavor = "current_thread")]
    async fn m204_uid_capacity_one_remains_reserved_during_execution() {
        let (q, mut receiver) = queue(Rc::new(Cell::new(None)));
        let slot = q.reserve([1; 32]).unwrap();
        assert!(matches!(q.reserve([2; 32]), Err(UidQueueAdmission::Full)));
        slot.submit();
        assert_eq!(receiver.recv().await, Some(QueuedJob::Page([1; 32])));
        assert!(matches!(q.reserve([2; 32]), Err(UidQueueAdmission::Full)));
        q.scheduled.set(None);
        assert!(q.reserve([2; 32]).is_ok());
    }
    #[tokio::test(flavor = "current_thread")]
    async fn m204_uid_unsubmitted_reservation_releases_without_effect() {
        let (q, mut receiver) = queue(Rc::new(Cell::new(None)));
        drop(q.reserve([3; 32]).unwrap());
        assert_eq!(q.scheduled.get(), None);
        assert!(receiver.try_recv().is_err());
        drop(receiver);
        assert!(matches!(q.reserve([4; 32]), Err(UidQueueAdmission::Closed)));
    }
    #[test]
    fn ext03_uid_probe_errors_are_stable_without_secret_details() {
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
        assert_eq!(
            uid_error(hantong_secrets::ImapUidPageError::UidValidityChanged),
            ErrorCode::Conflict
        );
        assert_eq!(
            uid_error(hantong_secrets::ImapUidPageError::CursorInvalid),
            ErrorCode::Conflict
        );
    }
}

#[cfg(test)]
#[path = "uid_worker_tests.rs"]
mod integration_tests;
