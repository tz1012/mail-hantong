use hantong_contracts::ErrorCode;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    time::Duration,
};
use windows::{
    Security::Credentials::UI::{UserConsentVerificationResult, UserConsentVerifier},
    Win32::{
        Foundation::HWND, System::WinRT::IUserConsentVerifierInterop,
        UI::WindowsAndMessaging::GetForegroundWindow,
    },
    core::{HSTRING, factory},
};
use windows_future::{AsyncOperationCompletedHandler, AsyncStatus, IAsyncOperation};

type Operation = IAsyncOperation<UserConsentVerificationResult>;
const CLEANUP_LIMIT: Duration = Duration::from_secs(5);
#[derive(Debug, Eq, PartialEq)]
enum CleanupAction {
    Wait,
    Close,
    Finished,
    Abandon,
}
fn is_terminal(status: AsyncStatus) -> bool {
    matches!(
        status,
        AsyncStatus::Completed | AsyncStatus::Canceled | AsyncStatus::Error
    )
}
fn cleanup_action(
    status: Option<AsyncStatus>,
    close_phase: u8,
    elapsed: Duration,
) -> CleanupAction {
    if close_phase == 2 {
        return CleanupAction::Finished;
    }
    if close_phase == 0 && status.is_some_and(is_terminal) {
        return CleanupAction::Close;
    }
    if elapsed >= CLEANUP_LIMIT {
        CleanupAction::Abandon
    } else {
        CleanupAction::Wait
    }
}
fn close_once(operation: &Operation, phase: &AtomicU8) -> Result<(), ErrorCode> {
    phase
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| ErrorCode::InternalError)?;
    let result = operation.Close().map_err(|_| ErrorCode::InternalError);
    // 2 means the terminal Close attempt finished, including an API error.
    // A Close failure is never converted to a successful verification result.
    phase.store(2, Ordering::Release);
    result
}
pub(super) struct ConsentOperation {
    operation: Operation,
    completion: Receiver<Result<UserConsentVerificationResult, ErrorCode>>,
    close_phase: Arc<AtomicU8>,
    setup_failed: bool,
}
impl ConsentOperation {
    // This private seam is called only with the window owned by this UI thread.
    pub(super) fn start(hwnd: HWND, message: &str) -> Result<Self, ErrorCode> {
        let verifier = factory::<UserConsentVerifier, IUserConsentVerifierInterop>()
            .map_err(|_| ErrorCode::CapabilityUnsupported)?;
        // WS_VISIBLE also permits obscured/background windows. Check after
        // factory activation, immediately before entering the OS prompt API.
        if unsafe { GetForegroundWindow() } != hwnd {
            return Err(ErrorCode::ApprovalRequired);
        }
        let operation = unsafe {
            verifier.RequestVerificationForWindowAsync::<Operation>(hwnd, &HSTRING::from(message))
        }
        .map_err(|_| ErrorCode::InternalError)?;
        let (sender, completion) = mpsc::channel();
        let close_phase = Arc::new(AtomicU8::new(0));
        let callback_phase = Arc::clone(&close_phase);
        let handler = AsyncOperationCompletedHandler::new(move |info, status| {
            // Callback can run on another thread. It owns no HWND/State pointer.
            // Only terminal callbacks close the operation. Cancellation alone
            // is a request, not proof that the operation has finished.
            let result = match status {
                AsyncStatus::Completed => info
                    .ok()
                    .and_then(|op| op.GetResults())
                    .map_err(|_| ErrorCode::InternalError),
                AsyncStatus::Canceled => Err(ErrorCode::ApprovalRequired),
                _ => Err(ErrorCode::InternalError),
            };
            let result = if is_terminal(status) {
                match info
                    .ok()
                    .map_err(|_| ErrorCode::InternalError)
                    .and_then(|op| close_once(op, &callback_phase))
                {
                    Ok(()) => result,
                    Err(_) => Err(ErrorCode::InternalError),
                }
            } else {
                Err(ErrorCode::InternalError)
            };
            let _ = sender.send(result);
            Ok(())
        });
        let setup_failed = operation.SetCompleted(&handler).is_err();
        if setup_failed {
            let _ = operation.Cancel();
        }
        // Even setup failure retains the operation. The owning STA closes its
        // review, then pumps a bounded cleanup phase until this is terminal.
        Ok(Self {
            operation,
            completion,
            close_phase,
            setup_failed,
        })
    }
    pub(super) fn setup_failed(&self) -> bool {
        self.setup_failed
    }
    pub(super) fn cancel(&self) {
        if self.close_phase.load(Ordering::Acquire) == 0 {
            // Status can fail. Still request cancellation; never infer that a
            // failed status query means there is no outstanding operation.
            let _ = self.operation.Cancel();
        }
    }
    pub(super) fn cleanup_finished(&self, elapsed: Duration) -> bool {
        let phase = self.close_phase.load(Ordering::Acquire);
        let status = if phase == 0 {
            self.operation.Status().ok()
        } else {
            None
        };
        match cleanup_action(status, phase, elapsed) {
            CleanupAction::Close => {
                let _ = close_once(&self.operation, &self.close_phase);
                self.close_phase.load(Ordering::Acquire) == 2
            }
            CleanupAction::Wait => false,
            CleanupAction::Finished | CleanupAction::Abandon => true,
        }
    }
    pub(super) fn poll(&self) -> Option<Result<UserConsentVerificationResult, ErrorCode>> {
        match self.completion.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(ErrorCode::InternalError)),
        }
    }
}
impl Drop for ConsentOperation {
    fn drop(&mut self) {
        self.cancel();
        // A registered terminal callback calls Close even when its receiver has
        // already been dropped. Do not Close a still-started operation here.
    }
}

pub(super) fn non_verified_error(result: UserConsentVerificationResult) -> ErrorCode {
    match result {
        UserConsentVerificationResult::Canceled => ErrorCode::ApprovalRequired,
        UserConsentVerificationResult::DeviceNotPresent
        | UserConsentVerificationResult::NotConfiguredForUser
        | UserConsentVerificationResult::DisabledByPolicy
        | UserConsentVerificationResult::DeviceBusy
        | UserConsentVerificationResult::RetriesExhausted => ErrorCode::HumanStepRequired,
        _ => ErrorCode::InternalError,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn inst04_cleanup_closes_only_known_terminal_operations() {
        for status in [
            AsyncStatus::Completed,
            AsyncStatus::Canceled,
            AsyncStatus::Error,
        ] {
            assert_eq!(
                cleanup_action(Some(status), 0, Duration::ZERO),
                CleanupAction::Close
            );
        }
        for status in [Some(AsyncStatus::Started), Some(AsyncStatus(99)), None] {
            assert_eq!(
                cleanup_action(status, 0, Duration::ZERO),
                CleanupAction::Wait
            );
            assert_eq!(
                cleanup_action(status, 0, CLEANUP_LIMIT),
                CleanupAction::Abandon
            );
        }
    }
    #[test]
    fn inst04_cleanup_does_not_close_twice_and_has_a_finite_deadline() {
        assert_eq!(
            cleanup_action(Some(AsyncStatus::Completed), 1, Duration::ZERO),
            CleanupAction::Wait
        );
        assert_eq!(
            cleanup_action(Some(AsyncStatus::Completed), 1, CLEANUP_LIMIT),
            CleanupAction::Abandon
        );
        assert_eq!(
            cleanup_action(None, 2, Duration::ZERO),
            CleanupAction::Finished
        );
    }
}
