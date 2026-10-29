#![cfg(windows)]

use std::sync::mpsc::Sender;

use windows::{
    core::{factory, HSTRING},
    Security::Credentials::UI::{
        UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
    },
    Win32::{
        Foundation::HWND,
        System::WinRT::{
            IUserConsentVerifierInterop, RoInitialize, RoUninitialize, RO_INIT_SINGLETHREADED,
        },
        UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, TranslateMessage, MSG},
    },
};
use windows_future::{AsyncOperationCompletedHandler, AsyncStatus, IAsyncOperation};

pub type VerificationOperation = IAsyncOperation<UserConsentVerificationResult>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationDisposition {
    Verified,
    DeviceNotPresent,
    NotConfiguredForUser,
    DisabledByPolicy,
    DeviceBusy,
    RetriesExhausted,
    Canceled,
    Unknown(i32),
}

pub fn classify_result(result: UserConsentVerificationResult) -> VerificationDisposition {
    match result {
        UserConsentVerificationResult::Verified => VerificationDisposition::Verified,
        UserConsentVerificationResult::DeviceNotPresent => {
            VerificationDisposition::DeviceNotPresent
        }
        UserConsentVerificationResult::NotConfiguredForUser => {
            VerificationDisposition::NotConfiguredForUser
        }
        UserConsentVerificationResult::DisabledByPolicy => {
            VerificationDisposition::DisabledByPolicy
        }
        UserConsentVerificationResult::DeviceBusy => VerificationDisposition::DeviceBusy,
        UserConsentVerificationResult::RetriesExhausted => {
            VerificationDisposition::RetriesExhausted
        }
        UserConsentVerificationResult::Canceled => VerificationDisposition::Canceled,
        other => VerificationDisposition::Unknown(other.0),
    }
}

// Compile-only safe boundary: the caller supplies an owned UI HWND, while the
// unsafe ABI call remains contained inside this Windows-only platform module.
#[allow(dead_code)]
fn request_verification(
    hwnd: HWND,
    message: &HSTRING,
) -> windows::core::Result<VerificationOperation> {
    let verifier = factory::<UserConsentVerifier, IUserConsentVerifierInterop>()?;
    unsafe { verifier.RequestVerificationForWindowAsync::<VerificationOperation>(hwnd, message) }
}

#[allow(dead_code)]
fn availability_operation(
) -> windows::core::Result<IAsyncOperation<UserConsentVerifierAvailability>> {
    UserConsentVerifier::CheckAvailabilityAsync()
}

// Compile-only callback/cancellation surface. Neither this helper nor the
// request helper is called by tests or a binary in this experiment.
#[allow(dead_code)]
fn completion_and_cancellation(
    operation: VerificationOperation,
    sender: Sender<windows::core::Result<UserConsentVerificationResult>>,
) -> windows::core::Result<()> {
    let handler = AsyncOperationCompletedHandler::new(move |info, status| {
        if status == AsyncStatus::Completed {
            let _ = sender.send(info.ok()?.GetResults());
        }
        Ok(())
    });
    operation.SetCompleted(&handler)?;
    operation.Cancel()?;
    operation.Close()
}

#[allow(dead_code)]
fn sta_initialization_symbols() -> windows::core::Result<()> {
    unsafe {
        RoInitialize(RO_INIT_SINGLETHREADED)?;
        RoUninitialize();
    }
    Ok(())
}

#[allow(dead_code)]
fn message_loop_symbols(message: &mut MSG) {
    unsafe {
        let _ = GetMessageW(message, None, 0, 0);
        let _ = TranslateMessage(message);
        let _ = DispatchMessageW(message);
    }
}
