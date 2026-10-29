use super::*;
use hantong_approval::ScopeRefreshReview;
use hantong_approval::registration_review::ReviewSnapshot;
use hantong_contracts::auth::PeerContext;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(super) fn utc_ms() -> Result<i64, ErrorCode> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ErrorCode::InternalError)?
            .as_millis(),
    )
    .map_err(|_| ErrorCode::InternalError)
}
pub(super) struct CapturedReview<R: ReviewSnapshot = RegistrationReview> {
    pub(super) review: R,
    pub(super) started: Instant,
    captured_at_utc_ms: i64,
    user: PeerContext,
    session: [u8; 32],
    nonce: [u8; 32],
}
impl<R: ReviewSnapshot> CapturedReview<R> {
    pub(super) fn capture(review: R) -> Result<Self, ErrorCode> {
        let value = Self {
            review,
            started: Instant::now(),
            captured_at_utc_ms: utc_ms()?,
            user: hantong_platform::current_peer_context()?,
            session: hantong_platform::random_nonce()?,
            nonce: hantong_platform::random_nonce()?,
        };
        value.validate(&value.review)?;
        Ok(value)
    }
    pub(super) fn validate(&self, expected: &R) -> Result<(), ErrorCode> {
        self.validate_at(
            expected,
            utc_ms()?,
            self.started.elapsed(),
            &hantong_platform::current_peer_context()?,
        )
    }
    fn validate_at(
        &self,
        expected: &R,
        now: i64,
        elapsed: Duration,
        user: &PeerContext,
    ) -> Result<(), ErrorCode> {
        if expected != &self.review
            || user != &self.user
            || self.session == [0; 32]
            || self.nonce == [0; 32]
        {
            return Err(ErrorCode::ScopeDenied);
        }
        if now < self.review.created_at_utc_ms()
            || now < self.captured_at_utc_ms
            || now >= self.review.expires_at_utc_ms()
            || elapsed >= Duration::from_secs(300)
        {
            return Err(ErrorCode::ApprovalExpired);
        }
        Ok(())
    }
    fn verification_time(&self) -> Result<i64, ErrorCode> {
        let verified_at_utc_ms = utc_ms()?;
        self.validate_at(
            &self.review,
            verified_at_utc_ms,
            self.started.elapsed(),
            &hantong_platform::current_peer_context()?,
        )?;
        Ok(verified_at_utc_ms)
    }
    pub(super) fn consume(self, expected: &R, verified_at: i64) -> Result<R, ErrorCode> {
        let now = utc_ms()?;
        let elapsed = self.started.elapsed();
        self.consume_at(
            expected,
            verified_at,
            now,
            elapsed,
            &hantong_platform::current_peer_context()?,
        )
    }
    fn consume_at(
        self,
        expected: &R,
        verified_at: i64,
        now: i64,
        elapsed: Duration,
        user: &PeerContext,
    ) -> Result<R, ErrorCode> {
        self.validate_at(expected, now, elapsed, user)?;
        if verified_at < self.captured_at_utc_ms
            || verified_at >= self.review.expires_at_utc_ms()
            || now < verified_at
        {
            return Err(ErrorCode::ApprovalExpired);
        }
        Ok(self.review)
    }
}

impl CapturedReview<RegistrationReview> {
    // Called only from the owned window's successful OS verification branch.
    pub(super) fn into_verified(self) -> Result<VerifiedRegistration, ErrorCode> {
        let verified_at_utc_ms = self.verification_time()?;
        Ok(VerifiedRegistration {
            binding: self,
            verified_at_utc_ms,
        })
    }
}

impl CapturedReview<AccountSetupReview> {
    // The pending owner travels inside the result; it cannot be substituted later.
    pub(super) fn into_verified_setup(
        self,
        pending: hantong_secrets::PendingImapCredential,
    ) -> Result<VerifiedImapSetup, ErrorCode> {
        crate::setup_binding::validate_pending(&self.review, &pending)?;
        let verified_at_utc_ms = self.verification_time()?;
        Ok(VerifiedImapSetup {
            binding: self,
            verified_at_utc_ms,
            pending,
        })
    }
}

impl CapturedReview<ScopeRefreshReview> {
    pub(super) fn into_verified(self) -> Result<VerifiedScopeRefresh, ErrorCode> {
        let verified_at_utc_ms = self.verification_time()?;
        Ok(VerifiedScopeRefresh {
            binding: self,
            verified_at_utc_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn review(id: u8) -> RegistrationReview {
        RegistrationReview::new([id; 32], 1, vec!["acct".into()], [3; 32], 1000, 301000).unwrap()
    }
    fn context(logon: u8) -> PeerContext {
        PeerContext::new("S-1-5-21-1-2-3-1001", &format!("S-1-5-5-7-{logon}")).unwrap()
    }
    fn captured() -> CapturedReview {
        CapturedReview {
            review: review(1),
            started: Instant::now(),
            captured_at_utc_ms: 1500,
            user: context(8),
            session: [4; 32],
            nonce: [5; 32],
        }
    }
    fn setup_review(name: Option<&str>) -> AccountSetupReview {
        let draft = hantong_domain::account_provisioning::NewImapAccountDraft::new(
            "acct".into(),
            name.map(str::to_owned),
            None,
        )
        .unwrap();
        let endpoint = hantong_domain::mail_connection::MailConnectionConfig::from_json(
            br#"{"schema_version":1,"account_id":"acct","imap":{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"user","authentication":"app_password"}}"#,
        ).unwrap().imap().clone();
        AccountSetupReview::new(draft, endpoint, [1; 32], [2; 32], [3; 32], 1000, 301000).unwrap()
    }
    fn captured_setup() -> CapturedReview<AccountSetupReview> {
        CapturedReview {
            review: setup_review(None),
            started: Instant::now(),
            captured_at_utc_ms: 1500,
            user: context(8),
            session: [4; 32],
            nonce: [5; 32],
        }
    }
    fn scope_review(epoch: u64, client_hash: u8) -> ScopeRefreshReview {
        ScopeRefreshReview::new(
            [1; 32],
            1,
            [2; 16],
            [3; 32],
            4,
            epoch,
            [client_hash; 32],
            [7; 32],
            vec!["acct_a".into()],
            vec!["acct_b".into()],
            1000,
            301000,
        )
        .unwrap()
    }
    fn captured_scope() -> CapturedReview<ScopeRefreshReview> {
        CapturedReview {
            review: scope_review(5, 6),
            started: Instant::now(),
            captured_at_utc_ms: 1500,
            user: context(8),
            session: [4; 32],
            nonce: [5; 32],
        }
    }
    #[test]
    fn inst05_setup_binding_requires_exact_display_and_current_logon() {
        let captured = captured_setup();
        assert_eq!(
            captured.validate_at(
                &setup_review(None),
                2000,
                Duration::from_secs(1),
                &context(8)
            ),
            Ok(())
        );
        assert_eq!(
            captured.validate_at(
                &setup_review(Some("Changed display")),
                2000,
                Duration::from_secs(1),
                &context(8)
            ),
            Err(ErrorCode::ScopeDenied)
        );
        assert_eq!(
            captured.validate_at(
                &setup_review(None),
                2000,
                Duration::from_secs(1),
                &context(9)
            ),
            Err(ErrorCode::ScopeDenied)
        );
    }
    #[test]
    fn inst05_setup_binding_rejects_missing_nonce_and_expiry_at_consumption() {
        for (now, elapsed) in [
            (1499, Duration::ZERO),
            (301000, Duration::ZERO),
            (2000, Duration::from_secs(300)),
        ] {
            assert_eq!(
                captured_setup().validate_at(&setup_review(None), now, elapsed, &context(8)),
                Err(ErrorCode::ApprovalExpired)
            );
        }
        for corrupt_session in [false, true] {
            let mut captured = captured_setup();
            if corrupt_session {
                captured.session = [0; 32];
            } else {
                captured.nonce = [0; 32];
            }
            assert_eq!(
                captured.validate_at(&setup_review(None), 2000, Duration::ZERO, &context(8)),
                Err(ErrorCode::ScopeDenied)
            );
        }
        assert_eq!(
            captured_setup().consume_at(
                &setup_review(None),
                2000,
                1999,
                Duration::ZERO,
                &context(8)
            ),
            Err(ErrorCode::ApprovalExpired)
        );
        assert_eq!(
            captured_setup().consume_at(
                &setup_review(None),
                2000,
                2001,
                Duration::from_millis(501),
                &context(8)
            ),
            Ok(setup_review(None))
        );
    }

    #[test]
    fn cli15_scope_binding_requires_exact_snapshot_user_nonce_and_live_time() {
        let captured = captured_scope();
        assert_eq!(
            captured.validate_at(
                &scope_review(5, 6),
                2000,
                Duration::from_secs(1),
                &context(8)
            ),
            Ok(())
        );
        for expected in [
            scope_review(5, 8),
            ScopeRefreshReview::new(
                [1; 32],
                1,
                [2; 16],
                [3; 32],
                4,
                5,
                [6; 32],
                [7; 32],
                vec!["acct_b".into()],
                vec!["acct_c".into()],
                1000,
                301000,
            )
            .unwrap(),
        ] {
            assert_eq!(
                captured.validate_at(&expected, 2000, Duration::from_secs(1), &context(8)),
                Err(ErrorCode::ScopeDenied)
            );
        }
        assert_eq!(
            captured.validate_at(
                &scope_review(5, 6),
                2000,
                Duration::from_secs(1),
                &context(9)
            ),
            Err(ErrorCode::ScopeDenied)
        );
        let mut corrupt = captured_scope();
        corrupt.nonce = [0; 32];
        assert_eq!(
            corrupt.validate_at(&scope_review(5, 6), 2000, Duration::ZERO, &context(8)),
            Err(ErrorCode::ScopeDenied)
        );
        for (now, elapsed) in [
            (1499, Duration::ZERO),
            (301000, Duration::ZERO),
            (2000, Duration::from_secs(300)),
        ] {
            assert_eq!(
                captured_scope().validate_at(&scope_review(5, 6), now, elapsed, &context(8)),
                Err(ErrorCode::ApprovalExpired)
            );
        }
    }
    #[test]
    fn inst04_current_request_and_context_are_required_for_consumption() {
        let captured = captured();
        assert_eq!(
            captured.validate_at(&review(1), 2000, Duration::from_secs(1), &context(8)),
            Ok(())
        );
        assert_eq!(
            captured.validate_at(&review(2), 2000, Duration::from_secs(1), &context(8)),
            Err(ErrorCode::ScopeDenied)
        );
        assert_eq!(
            captured.validate_at(&review(1), 2000, Duration::from_secs(1), &context(9)),
            Err(ErrorCode::ScopeDenied)
        );
    }
    #[test]
    fn inst04_wall_and_monotonic_expiry_and_nonce_failure_never_validate() {
        let mut captured = captured();
        for (utc, elapsed) in [
            (999, Duration::ZERO),
            (1499, Duration::ZERO),
            (301000, Duration::ZERO),
            (2000, Duration::from_secs(300)),
        ] {
            assert_eq!(
                captured.validate_at(&review(1), utc, elapsed, &context(8)),
                Err(ErrorCode::ApprovalExpired)
            );
        }
        captured.nonce = [0; 32];
        assert!(
            captured
                .validate_at(&review(1), 2000, Duration::ZERO, &context(8))
                .is_err()
        );
        captured.nonce = [5; 32];
        captured.session = [0; 32];
        assert!(
            captured
                .validate_at(&review(1), 2000, Duration::ZERO, &context(8))
                .is_err()
        );
    }
    #[test]
    fn sec08_verified_registration_debug_contains_no_binding_fields() {
        let token = VerifiedRegistration {
            binding: captured(),
            verified_at_utc_ms: 2000,
        };
        assert_eq!(format!("{token:?}"), "VerifiedRegistration { <redacted> }");
    }
    #[test]
    fn inst04_consumption_rejects_clock_rollback_since_os_verification() {
        for (verified_at, now) in [(2000, 1999), (1499, 2000), (301000, 301000)] {
            assert_eq!(
                captured().consume_at(&review(1), verified_at, now, Duration::ZERO, &context(8)),
                Err(ErrorCode::ApprovalExpired)
            );
        }
        assert_eq!(
            captured().consume_at(
                &review(1),
                2000,
                2001,
                Duration::from_millis(501),
                &context(8)
            ),
            Ok(review(1))
        );
    }
}
