use super::*;
use std::time::Duration;
fn read(p: &mut ReadPolicy, id: u128, limit: u32, now: Instant) -> Result<(), ErrorCode> {
    p.check(id.to_be_bytes(), true, limit, AuditTool::AccountsList, now)
}

#[test]
fn ai05_read_is_allowed_for_both_read_only_values_but_send_always_consumes_then_denies() {
    let now = Instant::now();
    for read_only in [false, true] {
        let mut p = ReadPolicy::new();
        assert_eq!(
            p.check([1; 16], read_only, 2, AuditTool::AccountsList, now),
            Ok(())
        );
        assert_eq!(
            p.check([1; 16], read_only, 2, AuditTool::SendExecute, now),
            Err(ErrorCode::PolicyBlocked)
        );
        assert_eq!(
            p.check([1; 16], read_only, 2, AuditTool::SendExecute, now),
            Err(ErrorCode::RateLimited)
        );
        assert_eq!(p.buckets[&[1; 16]].len(), 2);
    }
}
#[test]
fn ai05_exact_sixty_seconds_expires_and_rate_denial_does_not_extend_window() {
    let now = Instant::now();
    let mut p = ReadPolicy::new();
    assert_eq!(read(&mut p, 1, 1, now), Ok(()));
    assert_eq!(
        read(&mut p, 1, 1, now + Duration::from_millis(59999)),
        Err(ErrorCode::RateLimited)
    );
    assert_eq!(read(&mut p, 1, 1, now + Duration::from_secs(60)), Ok(()));
    assert_eq!(p.buckets[&1_u128.to_be_bytes()].len(), 1);
}
#[test]
fn ai05_latest_limit_applies_without_reset_and_registration_windows_are_independent() {
    let now = Instant::now();
    let mut p = ReadPolicy::new();
    assert_eq!(read(&mut p, 1, 3, now), Ok(()));
    assert_eq!(read(&mut p, 1, 3, now), Ok(()));
    assert_eq!(read(&mut p, 1, 1, now), Err(ErrorCode::RateLimited));
    assert_eq!(read(&mut p, 2, 1, now), Ok(()));
    assert_eq!(read(&mut p, 1, 3, now), Ok(()));
    assert_eq!(read(&mut p, 1, 3, now), Err(ErrorCode::RateLimited));
}
#[test]
fn ai05_monotonic_rollback_rejects_without_mutation_even_after_rate_denial() {
    let start = Instant::now();
    let mut p = ReadPolicy::new();
    read(&mut p, 1, 1, start).unwrap();
    assert_eq!(
        read(&mut p, 1, 1, start + Duration::from_secs(10)),
        Err(ErrorCode::RateLimited)
    );
    let buckets = p.buckets.clone();
    let observed = p.last_observed;
    assert_eq!(
        read(&mut p, 2, 1, start + Duration::from_secs(9)),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(p.buckets, buckets);
    assert_eq!(p.last_observed, observed);
}
#[test]
fn ai05_invalid_persisted_identity_and_rate_do_not_consume_or_observe() {
    let now = Instant::now();
    let mut p = ReadPolicy::new();
    for (id, limit) in [(0, 1), (1, 0), (1, 601), (1, u32::MAX)] {
        assert_eq!(read(&mut p, id, limit, now), Err(ErrorCode::InternalError));
    }
    assert!(p.buckets.is_empty());
    assert_eq!(p.last_observed, None);
}
#[test]
fn ai05_bucket_capacity_preserves_live_registrations_and_reclaims_expired_buckets() {
    let now = Instant::now();
    let mut p = ReadPolicy::new();
    for id in 1..=1024 {
        read(&mut p, id, 2, now).unwrap();
    }
    assert_eq!(read(&mut p, 1025, 2, now), Err(ErrorCode::QuotaExceeded));
    assert_eq!(p.buckets.len(), 1024);
    assert_eq!(read(&mut p, 1, 2, now), Ok(()));
    assert_eq!(read(&mut p, 1, 2, now), Err(ErrorCode::RateLimited));
    assert_eq!(read(&mut p, 1025, 2, now + Duration::from_secs(60)), Ok(()));
    assert_eq!(p.buckets.len(), 1);
}
#[test]
fn ai05_maximum_bucket_is_bounded_and_restart_resets_only_memory() {
    let now = Instant::now();
    let mut p = ReadPolicy::new();
    for _ in 0..600 {
        read(&mut p, 1, 600, now).unwrap();
    }
    assert_eq!(read(&mut p, 1, 600, now), Err(ErrorCode::RateLimited));
    assert_eq!(p.buckets[&1_u128.to_be_bytes()].len(), 600);
    let mut restarted = ReadPolicy::new();
    assert_eq!(read(&mut restarted, 1, 600, now), Ok(()));
}
