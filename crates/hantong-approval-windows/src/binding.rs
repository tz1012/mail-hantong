use super::*;
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
pub(super) struct CapturedReview {
    pub(super) review: RegistrationReview,
    pub(super) started: Instant,
    captured_at_utc_ms: i64,
    user: PeerContext,
    session: [u8; 32],
    nonce: [u8; 32],
}
impl CapturedReview {
    pub(super) fn capture(review: RegistrationReview) -> Result<Self, ErrorCode> {
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
    pub(super) fn validate(&self, expected: &RegistrationReview) -> Result<(), ErrorCode> {
        self.validate_at(
            expected,
            utc_ms()?,
            self.started.elapsed(),
            &hantong_platform::current_peer_context()?,
        )
    }
    fn validate_at(
        &self,
        expected: &RegistrationReview,
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
    // Called only from the owned window's successful OS verification branch.
    pub(super) fn into_verified(self) -> Result<VerifiedRegistration, ErrorCode> {
        let verified_at_utc_ms = utc_ms()?;
        self.validate_at(
            &self.review,
            verified_at_utc_ms,
            self.started.elapsed(),
            &hantong_platform::current_peer_context()?,
        )?;
        Ok(VerifiedRegistration {
            binding: self,
            verified_at_utc_ms,
        })
    }
    pub(super) fn consume(
        self,
        expected: &RegistrationReview,
        verified_at: i64,
    ) -> Result<RegistrationReview, ErrorCode> {
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
        expected: &RegistrationReview,
        verified_at: i64,
        now: i64,
        elapsed: Duration,
        user: &PeerContext,
    ) -> Result<RegistrationReview, ErrorCode> {
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
