//! Private, non-authoritative matching of review and secret-owned capture metadata.
use hantong_approval::AccountSetupReview;
use hantong_contracts::ErrorCode;
use hantong_domain::mail_connection::ImapEndpointConfig;
use hantong_secrets::{ImapPasswordCapturePlan, PendingImapCredential, SecretError};

fn validate_target(
    review: &AccountSetupReview,
    account: &str,
    endpoint: &ImapEndpointConfig,
    request: &[u8; 32],
    hash: &[u8; 32],
    nonce: &[u8; 32],
) -> Result<(), ErrorCode> {
    if account != review.draft().account_id()
        || endpoint != review.endpoint()
        || request != review.request_id()
        || hash != review.payload_hash()
        || nonce != review.credential_nonce()
    {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(())
}

pub(crate) fn validate_plan(
    review: &AccountSetupReview,
    plan: &ImapPasswordCapturePlan,
) -> Result<(), ErrorCode> {
    validate_target(
        review,
        plan.account_id(),
        plan.endpoint(),
        plan.setup_request_id(),
        plan.setup_payload_hash(),
        plan.nonce(),
    )
}
pub(crate) fn validate_pending(
    review: &AccountSetupReview,
    pending: &PendingImapCredential,
) -> Result<(), ErrorCode> {
    validate_target(
        review,
        pending.account_id(),
        pending.endpoint(),
        pending.setup_request_id(),
        pending.setup_payload_hash(),
        pending.nonce(),
    )
}
pub(crate) fn input_error(error: SecretError) -> ErrorCode {
    match error {
        SecretError::InputExpired => ErrorCode::ApprovalExpired,
        SecretError::InvalidAccountId
        | SecretError::InvalidCredential
        | SecretError::InvalidInputWindow
        | SecretError::EmptySecret
        | SecretError::SecretTooLarge => ErrorCode::InvalidInput,
        SecretError::CredentialMismatch => ErrorCode::ScopeDenied,
        SecretError::UnsupportedPlatform => ErrorCode::CapabilityUnsupported,
        _ => ErrorCode::InternalError,
    }
}

/// Keeps a captured value owned until OS verification has successfully started.
/// This only orders effects; it issues no approval result.
pub(crate) fn capture_before_verification<P, O>(
    capture: impl FnOnce() -> Result<P, ErrorCode>,
    start: impl FnOnce() -> Result<O, ErrorCode>,
) -> Result<(P, O), ErrorCode> {
    let pending = capture()?;
    let operation = start()?;
    Ok((pending, operation))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_domain::{
        account_provisioning::NewImapAccountDraft, mail_connection::MailConnectionConfig,
    };
    use std::cell::Cell;

    fn endpoint(host: &str) -> ImapEndpointConfig {
        let raw = format!(
            r#"{{"schema_version":1,"account_id":"acct","imap":{{"settings_version":1,"host":"{host}","port":993,"tls":"implicit_tls","login_name":"user","authentication":"app_password"}}}}"#
        );
        MailConnectionConfig::from_json(raw.as_bytes())
            .unwrap()
            .imap()
            .clone()
    }
    fn review() -> AccountSetupReview {
        AccountSetupReview::new(
            NewImapAccountDraft::new("acct".into(), None, None).unwrap(),
            endpoint("imap.example"),
            [1; 32],
            [2; 32],
            [3; 32],
            1000,
            301000,
        )
        .unwrap()
    }
    #[test]
    fn acc03_setup_requires_exact_account_endpoint_request_hash_and_capture_nonce() {
        let r = review();
        assert_eq!(
            validate_target(&r, "acct", r.endpoint(), &[1; 32], &[2; 32], &[3; 32]),
            Ok(())
        );
        for (account, endpoint, request, hash, nonce) in [
            ("other", r.endpoint().clone(), [1; 32], [2; 32], [3; 32]),
            ("acct", endpoint("other.example"), [1; 32], [2; 32], [3; 32]),
            ("acct", r.endpoint().clone(), [9; 32], [2; 32], [3; 32]),
            ("acct", r.endpoint().clone(), [1; 32], [9; 32], [3; 32]),
            ("acct", r.endpoint().clone(), [1; 32], [2; 32], [9; 32]),
            ("acct", r.endpoint().clone(), [1; 32], [2; 32], [0; 32]),
        ] {
            assert_eq!(
                validate_target(&r, account, &endpoint, &request, &hash, &nonce),
                Err(ErrorCode::ScopeDenied)
            );
        }
    }
    #[test]
    fn inst05_setup_captures_before_os_verification_and_capture_failure_cannot_start_it() {
        let phase = Cell::new(0);
        let result = capture_before_verification(
            || {
                assert_eq!(phase.get(), 0);
                phase.set(1);
                Ok(42)
            },
            || {
                assert_eq!(phase.get(), 1);
                phase.set(2);
                Ok(17)
            },
        );
        assert_eq!(result, Ok((42, 17)));
        assert_eq!(phase.get(), 2);
        let result: Result<((), ()), _> = capture_before_verification(
            || Err(ErrorCode::InvalidInput),
            || panic!("failed capture must not start verification"),
        );
        assert_eq!(result, Err(ErrorCode::InvalidInput));
    }
    #[test]
    fn inst05_setup_os_start_failure_discards_the_captured_value() {
        struct DropProbe<'a>(&'a Cell<u32>);
        impl Drop for DropProbe<'_> {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let dropped = Cell::new(0);
        let result: Result<(_, ()), _> = capture_before_verification(
            || Ok(DropProbe(&dropped)),
            || Err(ErrorCode::HumanStepRequired),
        );
        assert!(matches!(result, Err(ErrorCode::HumanStepRequired)));
        assert_eq!(dropped.get(), 1);
    }
}
