//! Bounded deterministic adaptive worker-count selection from measured CPU observations.
//! Declared caps remain declared caps; unknown observations never justify an increase.
//! The policy is a pure function of its inputs: no clock, no randomness, no iteration
//! order dependence. Hysteresis streaks bound flapping; the floor is always >= 1 so
//! sustained pressure can never starve progress entirely.
use crate::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub minimum_workers: usize,
    pub maximum_workers: usize,
    pub scale_up_busy_below_numerator: u64,
    pub scale_up_busy_below_denominator: u64,
    pub scale_down_busy_at_or_above_numerator: u64,
    pub scale_down_busy_at_or_above_denominator: u64,
    pub hysteresis_windows: u64,
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        if !(1..=16).contains(&self.minimum_workers)
            || !(1..=16).contains(&self.maximum_workers)
            || self.minimum_workers > self.maximum_workers
            || self.scale_up_busy_below_denominator == 0
            || self.scale_down_busy_at_or_above_denominator == 0
            || self.scale_up_busy_below_numerator > self.scale_up_busy_below_denominator
            || self.scale_down_busy_at_or_above_numerator
                > self.scale_down_busy_at_or_above_denominator
            || !(1..=8).contains(&self.hysteresis_windows)
        {
            return Err("invalid worker selection policy bounds".into());
        }
        // The down threshold must sit strictly above the up threshold so a real
        // dead band exists between scaling directions.
        if (self.scale_down_busy_at_or_above_numerator as u128)
            * (self.scale_up_busy_below_denominator as u128)
            <= (self.scale_up_busy_below_numerator as u128)
                * (self.scale_down_busy_at_or_above_denominator as u128)
        {
            return Err("worker selection thresholds require a dead band".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub declared_workers: usize,
    pub current_workers: usize,
    pub up_streak: u64,
    pub down_streak: u64,
}
#[derive(Debug, Clone)]
pub struct Observation {
    pub host_busy_delta: Option<u64>,
    pub host_total_delta: Option<u64>,
    pub available_parallelism: Option<usize>,
    pub memory_allowed: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct Selection {
    pub workers: usize,
    pub reasons: Vec<String>,
    pub next_state: State,
    pub busy_share: Option<(u64, u64)>,
}
pub fn initial_state(policy: &Policy, declared_workers: usize) -> Result<State> {
    policy.validate()?;
    if !(1..=16).contains(&declared_workers) {
        return Err("invalid declared worker count".into());
    }
    let upper = policy.maximum_workers.min(declared_workers);
    let lower = policy.minimum_workers.min(declared_workers);
    Ok(State {
        declared_workers,
        current_workers: declared_workers.clamp(lower, upper),
        up_streak: 0,
        down_streak: 0,
    })
}
pub fn select(policy: &Policy, state: &State, observation: &Observation) -> Result<Selection> {
    policy.validate()?;
    if !(1..=16).contains(&state.declared_workers) {
        return Err("invalid selection state".into());
    }
    let upper = policy.maximum_workers.min(state.declared_workers);
    let lower = policy.minimum_workers.min(state.declared_workers).max(1);
    if state.current_workers < lower
        || state.current_workers > upper
        || state.up_streak > policy.hysteresis_windows
        || state.down_streak > policy.hysteresis_windows
    {
        return Err("invalid selection state".into());
    }
    let mut reasons = Vec::new();
    let mut workers = state.current_workers;
    let mut up = state.up_streak;
    let mut down = state.down_streak;
    let mut busy_share = None;
    if !observation.memory_allowed {
        workers = lower;
        up = 0;
        down = 0;
        reasons.push("memory-pressure".into());
    } else {
        match (observation.host_busy_delta, observation.host_total_delta) {
            (Some(busy), Some(total)) if total > 0 => {
                busy_share = Some((busy, total));
                let b = busy as u128;
                let t = total as u128;
                let down_hit = b * (policy.scale_down_busy_at_or_above_denominator as u128)
                    >= t * (policy.scale_down_busy_at_or_above_numerator as u128);
                let up_hit = b * (policy.scale_up_busy_below_denominator as u128)
                    < t * (policy.scale_up_busy_below_numerator as u128);
                if down_hit {
                    up = 0;
                    down = (down + 1).min(policy.hysteresis_windows);
                    if down >= policy.hysteresis_windows {
                        down = 0;
                        if workers > lower {
                            workers -= 1;
                            reasons.push("sustained-cpu-pressure-scale-down".into());
                        } else {
                            reasons.push("cpu-pressure-at-floor".into());
                        }
                    } else {
                        reasons.push("cpu-pressure-streak".into());
                    }
                } else if up_hit {
                    down = 0;
                    up = (up + 1).min(policy.hysteresis_windows);
                    if up >= policy.hysteresis_windows {
                        up = 0;
                        let cap = match observation.available_parallelism {
                            Some(parallelism) => upper.min(parallelism.max(1)),
                            None => upper,
                        };
                        if workers < cap {
                            workers += 1;
                            reasons.push("sustained-cpu-headroom-scale-up".into());
                        } else {
                            reasons.push("cpu-headroom-at-cap".into());
                        }
                    } else {
                        reasons.push("cpu-headroom-streak".into());
                    }
                } else {
                    up = 0;
                    down = 0;
                    reasons.push("cpu-share-in-dead-band".into());
                }
            }
            (Some(_), Some(0)) => {
                up = 0;
                down = 0;
                reasons.push("host-window-zero-hold".into());
            }
            _ => {
                up = 0;
                down = 0;
                reasons.push("observation-unknown-hold".into());
            }
        }
    }
    Ok(Selection {
        workers,
        reasons,
        next_state: State {
            declared_workers: state.declared_workers,
            current_workers: workers,
            up_streak: up,
            down_streak: down,
        },
        busy_share,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> Policy {
        Policy {
            minimum_workers: 1,
            maximum_workers: 4,
            scale_up_busy_below_numerator: 1,
            scale_up_busy_below_denominator: 4,
            scale_down_busy_at_or_above_numerator: 3,
            scale_down_busy_at_or_above_denominator: 4,
            hysteresis_windows: 2,
        }
    }
    fn obs(busy: u64, total: u64) -> Observation {
        Observation {
            host_busy_delta: Some(busy),
            host_total_delta: Some(total),
            available_parallelism: Some(6),
            memory_allowed: true,
        }
    }
    fn state(workers: usize) -> State {
        State {
            declared_workers: 4,
            current_workers: workers,
            up_streak: 0,
            down_streak: 0,
        }
    }
    #[test]
    fn policy_validation_rejects_unbounded_or_inverted_thresholds() {
        assert!(policy().validate().is_ok());
        let mut bad = policy();
        bad.minimum_workers = 0;
        assert!(bad.validate().is_err());
        let mut bad = policy();
        bad.maximum_workers = 17;
        assert!(bad.validate().is_err());
        let mut bad = policy();
        bad.minimum_workers = 5;
        assert!(bad.validate().is_err());
        let mut bad = policy();
        bad.scale_up_busy_below_denominator = 0;
        assert!(bad.validate().is_err());
        let mut bad = policy();
        bad.scale_down_busy_at_or_above_denominator = 0;
        assert!(bad.validate().is_err());
        let mut bad = policy();
        bad.scale_up_busy_below_numerator = 5;
        bad.scale_up_busy_below_denominator = 4;
        assert!(bad.validate().is_err());
        // down threshold must be strictly above up threshold (a real dead band).
        let mut bad = policy();
        bad.scale_down_busy_at_or_above_numerator = 1;
        bad.scale_down_busy_at_or_above_denominator = 4;
        assert!(bad.validate().is_err());
        let mut bad = policy();
        bad.scale_down_busy_at_or_above_numerator = 1;
        bad.scale_down_busy_at_or_above_denominator = 2;
        bad.scale_up_busy_below_numerator = 1;
        bad.scale_up_busy_below_denominator = 2;
        assert!(bad.validate().is_err());
        let mut bad = policy();
        bad.hysteresis_windows = 0;
        assert!(bad.validate().is_err());
        let mut bad = policy();
        bad.hysteresis_windows = 9;
        assert!(bad.validate().is_err());
    }
    #[test]
    fn initial_state_starts_at_declared_cap_clamped_to_policy() {
        let p = policy();
        assert_eq!(initial_state(&p, 4).unwrap().current_workers, 4);
        assert_eq!(initial_state(&p, 8).unwrap().current_workers, 4);
        assert_eq!(initial_state(&p, 2).unwrap().current_workers, 2);
        assert_eq!(initial_state(&p, 2).unwrap().declared_workers, 2);
        assert!(initial_state(&p, 0).is_err());
        assert!(initial_state(&p, 17).is_err());
    }
    #[test]
    fn sustained_pressure_steps_down_to_floor_and_never_below() {
        let p = policy();
        let mut s = initial_state(&p, 4).unwrap();
        let mut counts = Vec::new();
        for _ in 0..12 {
            let selection = select(&p, &s, &obs(90, 100)).unwrap();
            counts.push(selection.workers);
            s = selection.next_state;
        }
        // Hysteresis 2: hold, step, hold, step, hold, step, then floor forever.
        assert_eq!(counts, vec![4, 3, 3, 2, 2, 1, 1, 1, 1, 1, 1, 1]);
        assert!(counts.iter().all(|n| *n >= 1));
        let last = select(&p, &s, &obs(90, 100)).unwrap();
        assert_eq!(last.workers, 1);
        assert!(
            last.reasons
                .iter()
                .any(|r| r.contains("floor") || r.contains("pressure"))
        );
    }
    #[test]
    fn sustained_idle_steps_up_to_declared_policy_and_parallelism_caps() {
        let p = policy();
        let mut s = initial_state(&p, 4).unwrap();
        s.current_workers = 1;
        let mut counts = Vec::new();
        for _ in 0..8 {
            let selection = select(&p, &s, &obs(10, 100)).unwrap();
            counts.push(selection.workers);
            s = selection.next_state;
        }
        assert_eq!(counts, vec![1, 2, 2, 3, 3, 4, 4, 4]);
        // Observed parallelism caps upscale even when the policy allows more.
        let mut s = initial_state(&p, 4).unwrap();
        s.current_workers = 1;
        let mut low = obs(10, 100);
        low.available_parallelism = Some(2);
        for _ in 0..8 {
            let selection = select(&p, &s, &low).unwrap();
            s = selection.next_state;
        }
        assert_eq!(s.current_workers, 2);
        // Unknown parallelism still allows upscale to the declared/policy cap.
        let mut s = initial_state(&p, 4).unwrap();
        s.current_workers = 1;
        let mut unknown = obs(10, 100);
        unknown.available_parallelism = None;
        for _ in 0..8 {
            let selection = select(&p, &s, &unknown).unwrap();
            s = selection.next_state;
        }
        assert_eq!(s.current_workers, 4);
        // Declared cap below policy maximum is never exceeded.
        let mut s = initial_state(&p, 2).unwrap();
        s.current_workers = 1;
        for _ in 0..8 {
            let selection = select(&p, &s, &obs(10, 100)).unwrap();
            assert!(selection.workers <= 2);
            s = selection.next_state;
        }
        assert_eq!(s.current_workers, 2);
    }
    #[test]
    fn alternating_windows_never_oscillate_under_hysteresis() {
        let p = policy();
        let mut s = initial_state(&p, 4).unwrap();
        s.current_workers = 2;
        for i in 0..40 {
            let observation = if i % 2 == 0 {
                obs(95, 100)
            } else {
                obs(5, 100)
            };
            let selection = select(&p, &s, &observation).unwrap();
            assert_eq!(selection.workers, 2, "flapped at window {i}");
            s = selection.next_state;
            assert!(s.up_streak < p.hysteresis_windows);
            assert!(s.down_streak < p.hysteresis_windows);
        }
    }
    #[test]
    fn unknown_or_empty_observations_hold_and_memory_pressure_floors() {
        let p = policy();
        let s = state(3);
        let mut unknown = obs(10, 100);
        unknown.host_busy_delta = None;
        let selection = select(&p, &s, &unknown).unwrap();
        assert_eq!(selection.workers, 3);
        assert!(
            selection
                .reasons
                .contains(&"observation-unknown-hold".to_string())
        );
        assert_eq!(selection.next_state.up_streak, 0);
        assert_eq!(selection.next_state.down_streak, 0);
        let mut unknown = obs(10, 100);
        unknown.host_total_delta = None;
        assert_eq!(select(&p, &s, &unknown).unwrap().workers, 3);
        let empty = obs(0, 0);
        let selection = select(&p, &s, &empty).unwrap();
        assert_eq!(selection.workers, 3);
        assert!(
            selection
                .reasons
                .contains(&"host-window-zero-hold".to_string())
        );
        let mut pressured = obs(10, 100);
        pressured.memory_allowed = false;
        let selection = select(&p, &s, &pressured).unwrap();
        assert_eq!(selection.workers, 1);
        assert!(selection.reasons.contains(&"memory-pressure".to_string()));
        // Unknown can never raise the worker count, even from the floor.
        let floor = state(1);
        let mut unknown = obs(10, 100);
        unknown.host_busy_delta = None;
        assert_eq!(select(&p, &floor, &unknown).unwrap().workers, 1);
    }
    #[test]
    fn dead_band_holds_and_resets_streaks() {
        let p = policy();
        let mut s = state(2);
        s.down_streak = 1;
        // busy 1/2 is between 1/4 and 3/4.
        let selection = select(&p, &s, &obs(50, 100)).unwrap();
        assert_eq!(selection.workers, 2);
        assert!(
            selection
                .reasons
                .contains(&"cpu-share-in-dead-band".to_string())
        );
        assert_eq!(selection.next_state.down_streak, 0);
        assert_eq!(selection.next_state.up_streak, 0);
        assert_eq!(selection.busy_share, Some((50, 100)));
    }
    #[test]
    fn selection_is_deterministic_and_pure() {
        let p = policy();
        let s = state(2);
        let observation = obs(90, 100);
        let a = select(&p, &s, &observation).unwrap();
        let b = select(&p, &s, &observation).unwrap();
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
        // The input state is untouched.
        assert_eq!(s.current_workers, 2);
        assert_eq!(s.down_streak, 0);
        assert_eq!(a.next_state.down_streak, 1);
        assert_eq!(a.workers, 2);
    }
    #[test]
    fn invalid_state_is_refused() {
        let p = policy();
        let mut s = state(2);
        s.current_workers = 5;
        assert!(select(&p, &s, &obs(50, 100)).is_err());
        let mut s = state(2);
        s.current_workers = 0;
        assert!(select(&p, &s, &obs(50, 100)).is_err());
        let mut s = state(2);
        s.declared_workers = 0;
        assert!(select(&p, &s, &obs(50, 100)).is_err());
        let mut s = state(2);
        s.up_streak = 9;
        assert!(select(&p, &s, &obs(50, 100)).is_err());
    }
    #[test]
    fn exact_boundary_comparisons_use_checked_cross_products() {
        let p = policy();
        let s = state(2);
        // Exactly at the down threshold (3/4) counts as pressure.
        let selection = select(&p, &s, &obs(75, 100)).unwrap();
        assert_eq!(selection.next_state.down_streak, 1);
        assert_eq!(selection.next_state.up_streak, 0);
        // Exactly at the up threshold (1/4) is NOT below it: dead band.
        let selection = select(&p, &s, &obs(25, 100)).unwrap();
        assert!(
            selection
                .reasons
                .contains(&"cpu-share-in-dead-band".to_string())
        );
        // Near-u64-MAX deltas must not overflow.
        let huge = obs(u64::MAX - 1, u64::MAX);
        let selection = select(&p, &s, &huge).unwrap();
        assert_eq!(selection.next_state.down_streak, 1);
        let huge_idle = obs(1, u64::MAX);
        let selection = select(&p, &s, &huge_idle).unwrap();
        assert_eq!(selection.next_state.up_streak, 1);
    }
}
