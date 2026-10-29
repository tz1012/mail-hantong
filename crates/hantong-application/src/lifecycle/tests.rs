use super::*;

#[test]
fn inst13_crashes_back_off_five_times_then_pause_once() {
    let mut budget = RestartBudget::default();
    for seconds in [1, 2, 4, 8, 16] {
        assert_eq!(
            budget.child_exited(ChildExit::Abnormal, Duration::from_secs(1)),
            RestartDecision::RestartAfter(Duration::from_secs(seconds))
        );
    }
    assert_eq!(
        budget.child_exited(ChildExit::Abnormal, Duration::ZERO),
        RestartDecision::PauseAndNotify
    );
    assert_eq!(
        budget.child_exited(ChildExit::Abnormal, Duration::from_secs(600)),
        RestartDecision::Stop
    );
}

#[test]
fn inst13_stable_run_resets_only_active_budget_at_five_minutes() {
    let mut budget = RestartBudget::default();
    assert_eq!(
        budget.child_exited(ChildExit::Abnormal, Duration::ZERO),
        RestartDecision::RestartAfter(Duration::from_secs(1))
    );
    assert_eq!(
        budget.child_exited(ChildExit::Abnormal, Duration::from_millis(299_999)),
        RestartDecision::RestartAfter(Duration::from_secs(2))
    );
    assert_eq!(
        budget.child_exited(ChildExit::Abnormal, Duration::from_secs(300)),
        RestartDecision::RestartAfter(Duration::from_secs(1))
    );
}

#[test]
fn inst13_normal_stop_maintenance_and_existing_core_never_restart() {
    for reason in [
        ChildExit::Clean,
        ChildExit::RequestedStop,
        ChildExit::Maintenance,
        ChildExit::AlreadyRunning,
    ] {
        let mut budget = RestartBudget::default();
        assert_eq!(
            budget.child_exited(reason, Duration::ZERO),
            RestartDecision::Stop
        );
        assert_eq!(
            budget.child_exited(ChildExit::Abnormal, Duration::from_secs(600)),
            RestartDecision::Stop
        );
    }
}

#[test]
fn inst13_stop_during_crash_sequence_cancels_future_retries() {
    let mut budget = RestartBudget::default();
    assert_eq!(
        budget.child_exited(ChildExit::Abnormal, Duration::ZERO),
        RestartDecision::RestartAfter(Duration::from_secs(1))
    );
    assert_eq!(
        budget.child_exited(ChildExit::RequestedStop, Duration::ZERO),
        RestartDecision::Stop
    );
    assert_eq!(
        budget.child_exited(ChildExit::Abnormal, Duration::ZERO),
        RestartDecision::Stop
    );
}
