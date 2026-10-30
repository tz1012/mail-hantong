use super::*;

fn plan() -> InitialSyncPlan {
    InitialSyncPlan::new(29, 194, 3).unwrap()
}
fn unchanged<T: fmt::Debug>(
    plan: &mut InitialSyncPlan,
    action: impl FnOnce(&mut InitialSyncPlan) -> Result<T, InitialSyncPlanError>,
) {
    let before = plan.clone();
    assert!(matches!(action(plan), Err(InitialSyncPlanError::Conflict)));
    assert_eq!(*plan, before);
}
fn progress(plan: &InitialSyncPlan) -> (u32, Option<u32>, u8, u8, u8, u32, Option<u32>) {
    (
        plan.target_uid_ceiling(),
        plan.selected_lower_uid(),
        plan.selected_page_count(),
        plan.completed_page_count(),
        plan.remaining_page_count(),
        plan.remaining_uid_span(),
        plan.not_selected_before_uid(),
    )
}

#[test]
fn sync01_empty_is_observed_complete_and_invalid_inputs_are_rejected() {
    for recent in 1..=8 {
        let mut p = InitialSyncPlan::new(1, 1, recent).unwrap();
        assert_eq!(p.state(), InitialSyncState::Complete);
        assert_eq!(p.version(), 1);
        assert_eq!(progress(&p), (0, None, 0, 0, 0, 0, None));
        unchanged(&mut p, |p| p.begin(1));
        unchanged(&mut p, |p| p.pause(1));
        unchanged(&mut p, |p| p.invalidate(1, true));
    }
    for (validity, next, recent) in [(0, 1, 1), (1, 0, 1), (1, 1, 0), (1, 1, 9), (1, 1, u8::MAX)] {
        assert_eq!(
            InitialSyncPlan::new(validity, next, recent),
            Err(InitialSyncPlanError::InvalidInput)
        );
    }
}

#[test]
fn sync01_recent_blocks_descend_with_exact_partial_span_and_fixed_ceiling() {
    for next in [2, 64, 65, 66, 128, 129, 130, 194, 513, 514, 1025, u32::MAX] {
        for recent in 1..=8 {
            let mut p = InitialSyncPlan::new(u32::MAX, next, recent).unwrap();
            let ceiling = u64::from(next) - 1;
            let selected = u64::from(recent).min((ceiling - 1) / 64 + 1);
            let lower = (((ceiling - 1) / 64 + 1 - selected) * 64 + 1) as u32;
            assert_eq!(p.selected_page_count(), selected as u8);
            assert_eq!(p.selected_lower_uid(), Some(lower));
            assert_eq!(
                p.remaining_uid_span(),
                (ceiling - u64::from(lower) + 1) as u32
            );
            assert_eq!(
                p.not_selected_before_uid(),
                if lower == 1 { None } else { Some(lower - 1) }
            );
            let initial_span = p.remaining_uid_span();
            let mut completed_span = 0;
            let mut previous_after = None;
            for ordinal in 0..p.selected_page_count() {
                let before = p.version();
                let step = p.begin(before).unwrap();
                assert_eq!(step.plan_version(), before + 1);
                assert_eq!(step.ordinal(), ordinal);
                assert_eq!(step.uid_validity(), u32::MAX);
                assert_eq!(step.after_uid() % 64, 0);
                assert!((1..=64).contains(&(step.covered_through_uid() - step.after_uid())));
                if let Some(after) = previous_after {
                    assert_eq!(step.covered_through_uid(), after);
                }
                previous_after = Some(step.after_uid());
                completed_span += step.covered_through_uid() - step.after_uid();
                // No message-count input exists: even an empty provider page
                // covering these sparse UIDs completes exactly one range.
                p.complete_step(&step, u32::MAX).unwrap();
                assert_eq!(p.completed_page_count(), ordinal + 1);
                assert_eq!(p.remaining_uid_span() + completed_span, initial_span);
                assert_eq!(p.target_uid_ceiling(), next - 1);
                assert_eq!(p.selected_lower_uid(), Some(lower));
                assert_eq!(
                    p.state(),
                    if ordinal + 1 == selected as u8 {
                        InitialSyncState::Complete
                    } else {
                        InitialSyncState::PausedBudget
                    }
                );
                unchanged(&mut p, |p| p.complete_step(&step, u32::MAX));
            }
            assert_eq!(p.remaining_page_count(), 0);
            assert_eq!(p.remaining_uid_span(), 0);
            let version = p.version();
            unchanged(&mut p, |p| p.begin(version));
        }
    }
    let p = InitialSyncPlan::new(1, u32::MAX, 8).unwrap();
    assert_eq!(p.target_uid_ceiling(), u32::MAX - 1);
    assert_eq!(p.remaining_uid_span(), 510);
}

#[test]
fn sync01_pause_wins_commit_and_resume_rejects_the_old_identical_range() {
    let mut p = plan();
    let old = p.begin(1).unwrap();
    let before = progress(&p);
    p.pause(2).unwrap();
    assert_eq!(p.state(), InitialSyncState::Pausing);
    assert_eq!(progress(&p), before);
    unchanged(&mut p, |p| p.complete_step(&old, 29));
    unchanged(&mut p, |p| p.begin(3));
    unchanged(&mut p, |p| p.acknowledge_pause(2));
    p.acknowledge_pause(3).unwrap();
    assert_eq!(p.state(), InitialSyncState::PausedUser);
    let resumed = p.begin(4).unwrap();
    assert_eq!(
        (
            resumed.ordinal(),
            resumed.after_uid(),
            resumed.covered_through_uid()
        ),
        (old.ordinal(), old.after_uid(), old.covered_through_uid())
    );
    assert_ne!(resumed.plan_version(), old.plan_version());
    unchanged(&mut p, |p| p.complete_step(&old, 29));
    p.complete_step(&resumed, 29).unwrap();
    assert_eq!(p.completed_page_count(), 1);
    assert_eq!(p.state(), InitialSyncState::PausedBudget);
}

#[test]
fn sync01_commit_wins_pause_without_starting_the_next_page() {
    let mut p = plan();
    let step = p.begin(1).unwrap();
    p.complete_step(&step, 29).unwrap();
    assert_eq!(p.state(), InitialSyncState::PausedBudget);
    unchanged(&mut p, |p| p.pause(step.plan_version()));
    p.pause(3).unwrap();
    assert_eq!(p.state(), InitialSyncState::PausedUser);
    assert_eq!(p.completed_page_count(), 1);
    let next = p.begin(4).unwrap();
    assert_eq!(next.ordinal(), 1);
    assert_eq!(next.covered_through_uid(), step.after_uid());
    let mut last = InitialSyncPlan::new(29, 2, 1).unwrap();
    let step = last.begin(1).unwrap();
    last.complete_step(&step, 29).unwrap();
    unchanged(&mut last, |p| p.pause(3));
}

#[test]
fn sync01_pause_noop_requires_current_version_even_when_version_is_exhausted() {
    let mut p = plan();
    p.pause(1).unwrap();
    let before = p.clone();
    p.pause(2).unwrap();
    assert_eq!(p, before);
    unchanged(&mut p, |p| p.pause(1));
    p.begin(2).unwrap();
    p.pause(3).unwrap();
    let before = p.clone();
    p.pause(4).unwrap();
    assert_eq!(p, before);
    p.version = MAX_VERSION;
    let before = p.clone();
    p.pause(MAX_VERSION).unwrap();
    assert_eq!(p, before);
}

#[test]
fn sync01_failure_and_interruption_preserve_progress_until_explicit_resume() {
    for kind in 0..3 {
        let mut p = plan();
        let old = p.begin(1).unwrap();
        let before = progress(&p);
        let mut failed_expected = p.clone();
        failed_expected.version += 1;
        failed_expected.state = InitialSyncState::Failed;
        match kind {
            0 => {
                p.fail(2).unwrap();
                assert_eq!(p, failed_expected);
            }
            1 => {
                p.interrupt(2).unwrap();
                assert_eq!(p.state(), InitialSyncState::Interrupted);
            }
            _ => {
                p.pause(2).unwrap();
                p.interrupt(3).unwrap();
                assert_eq!(p.state(), InitialSyncState::PausedUser);
            }
        }
        assert_eq!(progress(&p), before);
        unchanged(&mut p, |p| p.complete_step(&old, 29));
        let resumed = p.begin(p.version()).unwrap();
        assert_eq!(resumed.ordinal(), old.ordinal());
        assert_eq!(resumed.after_uid(), old.after_uid());
        assert_eq!(resumed.covered_through_uid(), old.covered_through_uid());
        p.complete_step(&resumed, 29).unwrap();
        assert_eq!(p.completed_page_count(), 1);
    }
}

#[test]
fn sync01_step_fields_uidvalidity_and_versions_are_bound_without_mutation_on_conflict() {
    let mut p = plan();
    unchanged(&mut p, |p| p.begin(0));
    unchanged(&mut p, |p| p.begin(u64::MAX));
    let step = p.begin(1).unwrap();
    unchanged(&mut p, |p| p.begin(2));
    for validity in [0, 28, 30, u32::MAX] {
        unchanged(&mut p, |p| p.complete_step(&step, validity));
    }
    for field in 0..5 {
        let mut wrong = step.clone();
        match field {
            0 => wrong.plan_version += 1,
            1 => wrong.ordinal += 1,
            2 => wrong.uid_validity += 1,
            3 => wrong.after_uid -= 1,
            _ => wrong.covered_through_uid += 1,
        }
        unchanged(&mut p, |p| p.complete_step(&wrong, 29));
    }
    unchanged(&mut p, |p| p.pause(1));
    unchanged(&mut p, |p| p.fail(1));
    unchanged(&mut p, |p| p.interrupt(1));
    unchanged(&mut p, |p| p.invalidate(1, true));
    p.complete_step(&step, 29).unwrap();
}

const STATES: [InitialSyncState; 10] = [
    InitialSyncState::Ready,
    InitialSyncState::Running,
    InitialSyncState::Pausing,
    InitialSyncState::PausedUser,
    InitialSyncState::PausedBudget,
    InitialSyncState::Interrupted,
    InitialSyncState::Complete,
    InitialSyncState::Failed,
    InitialSyncState::StaleBinding,
    InitialSyncState::NeedsRecovery,
];

#[test]
fn sync01_invalidation_is_terminal_and_preserves_every_progress_value() {
    for state in STATES {
        for recovery in [false, true] {
            let mut p = plan();
            p.state = state;
            let before = progress(&p);
            if matches!(
                state,
                InitialSyncState::Complete
                    | InitialSyncState::StaleBinding
                    | InitialSyncState::NeedsRecovery
            ) {
                unchanged(&mut p, |p| p.invalidate(1, recovery));
            } else {
                p.invalidate(1, recovery).unwrap();
                assert_eq!(progress(&p), before);
                assert_eq!(
                    p.state(),
                    if recovery {
                        InitialSyncState::NeedsRecovery
                    } else {
                        InitialSyncState::StaleBinding
                    }
                );
                for operation in 0..6 {
                    unchanged(&mut p, |p| match operation {
                        0 => p.pause(2),
                        1 => p.acknowledge_pause(2),
                        2 => p.interrupt(2),
                        3 => p.fail(2),
                        4 => p.invalidate(2, !recovery),
                        _ => p.begin(2).map(|_| ()),
                    });
                }
            }
        }
    }
}

#[test]
fn sync01_invalid_state_transitions_and_all_version_overflows_are_atomic() {
    for state in STATES {
        let mut p = plan();
        p.state = state;
        if state != InitialSyncState::Running {
            unchanged(&mut p, |p| p.fail(1));
        }
        if state != InitialSyncState::Pausing {
            unchanged(&mut p, |p| p.acknowledge_pause(1));
        }
        if !matches!(state, InitialSyncState::Running | InitialSyncState::Pausing) {
            unchanged(&mut p, |p| p.interrupt(1));
        }
    }
    for operation in 0..7 {
        let mut p = plan();
        let mut step = p.begin(1).unwrap();
        p.version = MAX_VERSION;
        step.plan_version = MAX_VERSION;
        if operation == 0 {
            p.state = InitialSyncState::Ready;
        }
        if operation == 3 {
            p.state = InitialSyncState::Pausing;
        }
        unchanged(&mut p, |p| match operation {
            0 => p.begin(MAX_VERSION).map(|_| ()),
            1 => p.complete_step(&step, 29),
            2 => p.pause(MAX_VERSION),
            3 => p.acknowledge_pause(MAX_VERSION),
            4 => p.interrupt(MAX_VERSION),
            5 => p.fail(MAX_VERSION),
            _ => p.invalidate(MAX_VERSION, true),
        });
    }
}

#[test]
fn sync01_public_debug_and_errors_are_fixed_and_redacted() {
    let mut p = plan();
    let step = p.begin(1).unwrap();
    assert_eq!(format!("{p:?}"), "InitialSyncPlan(<redacted>)");
    assert_eq!(format!("{step:?}"), "InitialSyncStep(<redacted>)");
    for (error, text) in [
        (
            InitialSyncPlanError::InvalidInput,
            "invalid initial sync plan input",
        ),
        (InitialSyncPlanError::Conflict, "initial sync plan conflict"),
    ] {
        assert_eq!(error.to_string(), text);
        let dynamic: &dyn std::error::Error = &error;
        assert!(dynamic.source().is_none());
    }
}
