use hantong_contracts::ErrorCode;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};

pub(super) struct WindowPermit(&'static AtomicBool);
impl WindowPermit {
    pub(super) fn claim(slot: &'static AtomicBool) -> Result<Self, ErrorCode> {
        slot.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self(slot))
            .map_err(|_| ErrorCode::ApprovalRateLimited)
    }
}
impl Drop for WindowPermit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
pub(super) struct CancelOnDrop(pub(super) Sender<()>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inst04_only_one_window_per_slot_and_drop_releases_it() {
        static SLOT: AtomicBool = AtomicBool::new(false);
        let first = WindowPermit::claim(&SLOT).unwrap();
        assert!(matches!(
            WindowPermit::claim(&SLOT),
            Err(ErrorCode::ApprovalRateLimited)
        ));
        drop(first);
        assert!(WindowPermit::claim(&SLOT).is_ok());
    }
    #[test]
    fn inst04_abandoned_review_future_notifies_its_own_window() {
        let (sender, receiver) = std::sync::mpsc::channel();
        {
            let _cancel = CancelOnDrop(sender);
        }
        assert_eq!(receiver.try_recv(), Ok(()));
        assert!(receiver.try_recv().is_err());
        let (sender, receiver) = std::sync::mpsc::channel();
        drop(receiver);
        drop(CancelOnDrop(sender));
    }
}
