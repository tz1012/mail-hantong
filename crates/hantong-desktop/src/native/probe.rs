use super::*;
use crate::{MailAction, MailActionResult};
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// A window owns one bounded response slot. Closing it discards late results.
pub(super) struct StatusProbe {
    callback: Arc<dyn Fn() -> CoreStatus + Send + Sync>,
    response: Option<Receiver<CoreStatus>>,
}

pub(super) struct ActionProbe {
    callback: Option<crate::ActionCallback>,
    response: Option<Receiver<Result<MailActionResult, ErrorCode>>>,
    action: Option<MailAction>,
}
impl ActionProbe {
    pub(super) fn new(callback: Option<crate::ActionCallback>) -> Self {
        Self {
            callback,
            response: None,
            action: None,
        }
    }
    pub(super) fn available(&self) -> bool {
        self.callback.is_some()
    }
    pub(super) fn close(&mut self) {
        self.response = None;
        self.action = None;
    }
    pub(super) fn pending(&self) -> bool {
        self.response.is_some()
    }
    pub(super) fn begin(&mut self, action: MailAction) -> Result<bool, ErrorCode> {
        if self.pending() {
            return Ok(false);
        }
        let Some(callback) = self.callback.clone() else {
            return Ok(false);
        };
        let pending = action.clone();
        let (sender, response) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("hantong-gui-action".into())
            .spawn(move || {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(action)))
                        .unwrap_or(Err(ErrorCode::InternalError));
                let _ = sender.try_send(result);
            })
            .map_err(|_| ErrorCode::InternalError)?;
        self.response = Some(response);
        self.action = Some(pending);
        Ok(true)
    }
    pub(super) fn poll(&mut self) -> Option<(MailAction, Result<MailActionResult, ErrorCode>)> {
        let result = match self.response.as_ref()?.try_recv() {
            Ok(v) => v,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(ErrorCode::InternalError),
        };
        self.response = None;
        Some((self.action.take()?, result))
    }
}
impl StatusProbe {
    pub(super) fn new(callback: Arc<dyn Fn() -> CoreStatus + Send + Sync>) -> Self {
        Self {
            callback,
            response: None,
        }
    }
    pub(super) fn pending(&self) -> bool {
        self.response.is_some()
    }
    pub(super) fn close(&mut self) {
        self.response = None;
    }
    pub(super) fn begin(&mut self) -> Result<bool, ErrorCode> {
        if self.pending() {
            return Ok(false);
        }
        let (sender, response) = mpsc::sync_channel(1);
        let callback = Arc::clone(&self.callback);
        std::thread::Builder::new()
            .name("hantong-gui-status".into())
            .spawn(move || {
                let status = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback()))
                    .unwrap_or(CoreStatus::Unavailable(ErrorCode::InternalError));
                let _ = sender.try_send(status);
            })
            .map_err(|_| ErrorCode::InternalError)?;
        self.response = Some(response);
        Ok(true)
    }
    pub(super) fn poll(&mut self) -> Option<CoreStatus> {
        let status = match self.response.as_ref()?.try_recv() {
            Ok(CoreStatus::Checking) => CoreStatus::Unavailable(ErrorCode::InternalError),
            Ok(status) => status,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => CoreStatus::Unavailable(ErrorCode::InternalError),
        };
        self.response = None;
        Some(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::{Duration, Instant};
    #[test]
    fn ops02_gui_action_keeps_request_binding_and_drops_closed_window_results() {
        let (release, wait) = mpsc::sync_channel(1);
        let wait = Arc::new(Mutex::new(wait));
        let mut action = ActionProbe::new(Some(Arc::new(move |value| {
            assert_eq!(value, MailAction::Connect);
            wait.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            Ok(MailActionResult::Registration(
                hantong_contracts::registration::RegistrationProgress::Pending,
            ))
        })));
        assert!(action.begin(MailAction::Connect).unwrap());
        assert!(!action.begin(MailAction::Accounts).unwrap());
        action.close();
        release.send(()).unwrap();
        assert!(action.poll().is_none());
        assert!(!action.pending());
    }
    #[test]
    fn ops02_gui_status_probe_has_one_inflight_request_and_discards_late_result() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let g = Arc::clone(&gate);
        let c = Arc::clone(&calls);
        let mut probe = StatusProbe::new(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
            let (lock, cv) = &*g;
            let guard = lock.lock().unwrap();
            let _guard = cv
                .wait_timeout_while(guard, Duration::from_secs(2), |released| !*released)
                .unwrap();
            CoreStatus::Available { mail_ready: true }
        }));
        assert!(probe.begin().unwrap());
        for _ in 0..20 {
            assert!(!probe.begin().unwrap());
        }
        assert!(probe.poll().is_none());
        let start = Instant::now();
        while calls.load(Ordering::SeqCst) == 0 && start.elapsed() < Duration::from_secs(2) {
            std::thread::yield_now();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        drop(probe);
        let (lock, cv) = &*gate;
        *lock.lock().unwrap() = true;
        cv.notify_one();
    }
    #[test]
    fn ops02_gui_status_probe_panic_becomes_stable_error_and_releases_slot() {
        let mut probe = StatusProbe::new(Arc::new(|| panic!("synthetic probe failure")));
        assert!(probe.begin().unwrap());
        let start = Instant::now();
        let status = loop {
            if let Some(status) = probe.poll() {
                break status;
            }
            assert!(start.elapsed() < Duration::from_secs(2));
            std::thread::yield_now();
        };
        assert_eq!(status, CoreStatus::Unavailable(ErrorCode::InternalError));
        assert!(!probe.pending());
    }
}
