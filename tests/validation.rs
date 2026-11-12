use rust_strategy_factory::{
    contracts::MAX_HOLD_MS,
    validation::{self, Observation},
};
fn time(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .unwrap()
        .timestamp_millis()
}
fn observations() -> Vec<Observation> {
    (0..100)
        .map(|i| Observation {
            exit_ms: time(if i < 50 {
                "2025-06-01T00:00:00Z"
            } else {
                "2026-06-01T00:00:00Z"
            }) + i,
            net_micro_usd: if i % 2 == 0 { 11_000_000 } else { -1_000_000 },
        })
        .collect()
}
const CUTOFF: &str = "2026-11-12T00:00:00Z";
#[test]
fn exact_fifty_percent_and_fifty_per_year_pass_numeric_only() {
    let a = validation::assess(&observations(), time(CUTOFF), 0).unwrap();
    assert_eq!((a.pooled.wins, a.pooled.resolved), (50, 100));
    assert_eq!((a.year_2025.resolved, a.year_2026.resolved), (50, 50));
    assert!(a.numeric_gates_passed);
    assert_eq!(a.status, "INSUFFICIENT_EVIDENCE");
    assert_eq!(a.ending_micro_usd, 520_000_000);
}
#[test]
fn below_threshold_break_even_and_unresolved_do_not_qualify() {
    let mut obs = observations();
    obs[0].net_micro_usd = 0;
    let a = validation::assess(&obs, time(CUTOFF), 0).unwrap();
    assert_eq!(
        (a.pooled.wins, a.pooled.breakeven, a.pooled.resolved),
        (49, 1, 100)
    );
    assert!(!a.numeric_gates_passed);
    assert!(
        !validation::assess(&observations()[1..], time(CUTOFF), 0)
            .unwrap()
            .numeric_gates_passed
    );
    assert!(
        !validation::assess(&observations()[..99], time(CUTOFF), 0)
            .unwrap()
            .numeric_gates_passed
    );
    assert!(
        !validation::assess(&observations(), time(CUTOFF), 1)
            .unwrap()
            .numeric_gates_passed
    );
}
#[test]
fn streak_crosses_calendar_boundary_and_break_even_does_not_reset_it() {
    let mut obs = observations();
    for t in &mut obs[47..53] {
        t.net_micro_usd = -1;
    }
    obs[49].net_micro_usd = 0;
    obs[53].net_micro_usd = 11_000_000;
    let a = validation::assess(&obs, time(CUTOFF), 0).unwrap();
    assert_eq!(a.pooled.max_losing_streak, 5);
    obs[49].net_micro_usd = -1;
    let a = validation::assess(&obs, time(CUTOFF), 0).unwrap();
    assert_eq!(a.pooled.max_losing_streak, 6);
    assert!(!a.numeric_gates_passed);
}
#[test]
fn timestamps_and_future_outcomes_are_not_silently_sorted_or_accepted() {
    let mut obs = observations();
    obs.swap(0, 99);
    assert!(validation::assess(&obs, time(CUTOFF), 0).is_err());
    let mut obs = observations();
    obs[99].exit_ms = time("2026-12-01T00:00:00Z");
    assert!(validation::assess(&obs, time(CUTOFF), 0).is_err());
    assert!(validation::assess(&[], time("2027-01-01T00:00:00Z"), 0).is_err());
}
#[test]
fn folds_are_calendar_quarters_prior_only_with_purge_embargo() {
    let folds = validation::walk_forward(
        time("2025-01-01T00:00:00Z"),
        time("2025-07-01T00:00:00Z"),
        time("2026-07-01T00:00:00Z"),
        MAX_HOLD_MS,
    )
    .unwrap();
    assert_eq!(folds.len(), 4);
    assert_eq!(folds[0].test.end_exclusive_ms, time("2025-10-01T00:00:00Z"));
    let f = &folds[0];
    assert_eq!(f.train.end_exclusive_ms, time("2025-06-30T00:00:00Z"));
    assert!(!f.training_observation(time("2025-06-29T23:00:00Z"), time("2025-06-30T00:00:00Z")));
    assert!(f.training_observation(time("2025-06-29T00:00:00Z"), time("2025-06-29T23:00:00Z")));
    assert!(
        validation::walk_forward(
            f.train.start_ms,
            f.test.start_ms,
            f.test.end_exclusive_ms,
            MAX_HOLD_MS - 1
        )
        .is_err()
    );
}
