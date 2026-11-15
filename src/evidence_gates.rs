//! Frozen OOS evidence criteria. Passing these criteria never grants independent qualification.
use crate::{
    Result, data, diagnostics::Breakdown, robustness, science::CandidateEvidence, win_evidence,
};
use serde::{Deserialize, Serialize};
pub const VERSION: &str = "frozen-oos-evidence-gates-v1";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub minimum_folds: usize,
    pub minimum_resolved_per_fold: usize,
    pub minimum_resolved_per_year: usize,
    pub minimum_net_win_rate_bps: u16,
    pub maximum_consecutive_losses: usize,
    pub minimum_net_micro_usd: i128,
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        if !(2..=16).contains(&self.minimum_folds)
            || !(20..=1000000).contains(&self.minimum_resolved_per_fold)
            || !(50..=1000000).contains(&self.minimum_resolved_per_year)
            || !(5000..=10000).contains(&self.minimum_net_win_rate_bps)
            || self.maximum_consecutive_losses > 5
            || self.minimum_net_micro_usd < 1
        {
            return Err("invalid frozen OOS evidence sample/win/loss/net policy".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Decision {
    pub criterion: String,
    pub passed: bool,
    pub explanation: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Assessment {
    pub method: String,
    pub policy: Policy,
    pub state: String,
    pub criteria_passed: bool,
    pub qualification_granted: bool,
    pub decisions: Vec<Decision>,
    pub remaining_blockers: Vec<String>,
}
fn count_ok(closed: usize, wins: usize, losses: usize, even: usize) -> bool {
    closed <= 1000000 && wins.checked_add(losses).and_then(|n| n.checked_add(even)) == Some(closed)
}
fn annual(b: &Breakdown, year: i32, p: &Policy) -> bool {
    b.exit_years.get(&year).is_some_and(|s| {
        count_ok(s.resolved, s.wins, s.losses, s.breakevens)
            && s.resolved >= p.minimum_resolved_per_year
            && (s.wins as u128) * 10000
                >= (s.resolved as u128) * u128::from(p.minimum_net_win_rate_bps)
            && s.sum_net_micro_usd >= p.minimum_net_micro_usd
    })
}
pub fn assess(
    candidate: &CandidateEvidence,
    expected_folds: usize,
    declared_trials: u64,
    policy: &Policy,
    win_policy: &win_evidence::Policy,
    probe_policy: &robustness::ProbePolicy,
) -> Result<Assessment> {
    policy.validate()?;
    win_policy.validate()?;
    probe_policy.validate()?;
    if !(policy.minimum_folds..=16).contains(&expected_folds) || declared_trials == 0 {
        return Err("invalid frozen evidence fold/trial scope".into());
    }
    let s = &candidate.pooled_oos_reset_fold_stats;
    if !count_ok(s.closed, s.wins, s.losses, s.breakeven) || s.maximum_losing_streak > s.losses {
        return Err("inconsistent OOS outcome counters".into());
    }
    let mut decisions = Vec::new();
    let mut add = |name: &str, passed: bool, explanation: &str| {
        decisions.push(Decision {
            criterion: name.into(),
            passed,
            explanation: explanation.into(),
        })
    };
    let mut phases = std::collections::BTreeSet::from([
        "selection-training".to_owned(),
        "protected-final".to_owned(),
    ]);
    for i in 0..expected_folds {
        phases.insert(format!("fold-{i}-prior-training"));
        phases.insert(format!("fold-{i}-out-of-sample"));
    }
    let mut protected_ids = std::collections::BTreeSet::new();
    let training_ids = candidate
        .experiments
        .iter()
        .filter(|e| !e.phase.ends_with("out-of-sample") && e.phase != "protected-final")
        .map(|e| e.experiment_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut seen = std::collections::BTreeSet::new();
    let complete = candidate.experiments.len() == phases.len()
        && candidate.experiments.iter().all(|e| {
            crate::machine_evidence::hex(&e.experiment_id)
                && (!(e.phase.ends_with("out-of-sample") || e.phase == "protected-final")
                    || (!training_ids.contains(e.experiment_id.as_str())
                        && protected_ids.insert(e.experiment_id.as_str())))
                && phases.contains(&e.phase)
                && seen.insert(e.phase.clone())
                && e.outcome == "completed"
        })
        && candidate.failed_windows == 0;
    add(
        "complete-frozen-windows",
        complete,
        "Every declared training/OOS/final window must have one completed immutable replay; completion does not certify independent final performance.",
    );
    add(
        "no-unresolved-oos",
        candidate.unresolved_oos_positions == 0,
        "Unresolved OOS positions cannot be silently excluded.",
    );
    add(
        "pooled-net-win-rate",
        s.closed > 0
            && (s.wins as u128) * 10000
                >= (s.closed as u128) * u128::from(policy.minimum_net_win_rate_bps),
        "Breakevens remain nonwins; exact integer counts determine the threshold.",
    );
    add(
        "pooled-net-and-loss-streak",
        s.sum_net_micro_usd >= policy.minimum_net_micro_usd
            && s.maximum_losing_streak <= policy.maximum_consecutive_losses,
        "Reset-fold net sum and losing streak are evidence criteria, not full-period compounded equity.",
    );
    let folds = candidate.breakdown.as_ref().is_some_and(|b| {
        b.folds.len() == expected_folds
            && b.folds.iter().all(|f| {
                f.replay_present
                    && !f.unresolved
                    && !f.negative_realized_cash
                    && f.closed >= policy.minimum_resolved_per_fold
                    && f.ending_realized_cash_micro_usd >= crate::contracts::INITIAL_EQUITY
            })
            && b.folds
                .iter()
                .try_fold(0usize, |n, f| n.checked_add(f.closed))
                == Some(s.closed)
    });
    add(
        "fold-samples-and-replay",
        folds,
        "All frozen folds require sufficient resolved samples, valid replay, no unresolved/negative cash and consistent total counts.",
    );
    for year in [2025, 2026] {
        add(
            &format!("year-{year}-samples-net-and-win-rate"),
            candidate
                .breakdown
                .as_ref()
                .is_some_and(|b| annual(b, year, policy)),
            "Both research years need the frozen resolved sample, exact net-win threshold and positive reset-fold net evidence; missing years hold.",
        );
    }
    let win = candidate.win_evidence.as_ref().is_some_and(|d| {
        d.method == win_evidence::VERSION
            && d.policy == *win_policy
            && d.declared_trials == declared_trials
            && d.observations == s.closed
            && d.net_wins == s.wins
            && !d.qualification_granted
            && win_evidence::verify_final(d).is_ok()
            && d.fixed_horizon_gate_passed
    });
    add(
        "conditional-final-horizon-evidence",
        win,
        "The frozen conditional-null method, family and counts must match; early crossings do not substitute for the final gate.",
    );
    let probes = if let Some(a) = &candidate.robustness_assessment {
        probe_policy.minimum_resolved_per_probe >= policy.minimum_resolved_per_fold
            && probe_policy.minimum_net_win_rate_bps >= policy.minimum_net_win_rate_bps
            && probe_policy.maximum_consecutive_losses <= policy.maximum_consecutive_losses
            && probe_policy.minimum_sum_net_micro_usd >= policy.minimum_net_micro_usd
            && a.method == robustness::ASSESSMENT_VERSION
            && data::hash(&a.policy)? == data::hash(probe_policy)?
            && !a.qualification_granted
            && a.state == "PASS_DECLARED_PROBES"
            && !a.cases.is_empty()
            && a.cases
                .iter()
                .all(|c| c.state == "PASS_DECLARED_PROBES" && c.reasons.is_empty())
    } else {
        false
    };
    add(
        "frozen-robustness",
        probes,
        "Every frozen perturbation must satisfy its prespecified OOS policy; unknown or missing evidence holds.",
    );
    let passed = decisions.iter().all(|d| d.passed);
    Ok(Assessment{method:VERSION.into(),policy:policy.clone(),state:if passed{"NUMERICAL_EVIDENCE_ONLY"}else{"HELD"}.into(),criteria_passed:passed,qualification_granted:false,decisions,remaining_blockers:vec!["Independent approved source/profile/calendar/conversion and complete chronological coverage evidence".into(),"Complete corpus-wide search, revision/alias exposure and multiplicity accounting".into(),"Single-account full2025/2026-YTD financial replay; reset-fold sums cannot replace compounding/marked risk".into(),"Independent protected-final confirmation, broader regime stability and diversity/ranking acceptance".into(),"Synthetic data never qualifies regardless of numerical evidence".into()]})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        diagnostics::{Bucket, FoldRisk},
        science::{DescriptiveStats, Evidence},
        validation::Observation,
    };
    fn fixture() -> (
        CandidateEvidence,
        Policy,
        win_evidence::Policy,
        robustness::ProbePolicy,
    ) {
        let policy = Policy {
            minimum_folds: 2,
            minimum_resolved_per_fold: 20,
            minimum_resolved_per_year: 50,
            minimum_net_win_rate_bps: 5000,
            maximum_consecutive_losses: 5,
            minimum_net_micro_usd: 1,
        };
        let win = win_evidence::Policy {
            stake_basis_points: 5000,
            family_error_basis_points: 500,
            minimum_resolved: 100,
        };
        let probes = robustness::ProbePolicy {
            minimum_resolved_per_probe: 20,
            minimum_net_win_rate_bps: 5000,
            minimum_sum_net_micro_usd: 1,
            maximum_consecutive_losses: 5,
        };
        let mut breakdown = Breakdown::default();
        for year in [2025, 2026] {
            breakdown.exit_years.insert(
                year,
                Bucket {
                    resolved: 100,
                    wins: 100,
                    losses: 0,
                    breakevens: 0,
                    sum_net_micro_usd: 100,
                },
            );
        }
        let mut phases = vec![
            "selection-training".to_owned(),
            "protected-final".to_owned(),
        ];
        for i in 0..2 {
            phases.push(format!("fold-{i}-prior-training"));
            phases.push(format!("fold-{i}-out-of-sample"));
            let mut f = FoldRisk::new(format!("fold-{i}-out-of-sample"));
            f.replay_present = true;
            f.closed = 100;
            f.ending_realized_cash_micro_usd += 100;
            f.peak_realized_cash_micro_usd += 100;
            breakdown.folds.push(f);
        }
        let candidate = CandidateEvidence {
            concept_id: "a".repeat(64),
            experiments: phases
                .into_iter()
                .enumerate()
                .map(|(i, phase)| Evidence {
                    phase,
                    experiment_id: format!("{i:064x}"),
                    outcome: "completed".into(),
                    reused: false,
                })
                .collect(),
            pooled_oos_reset_fold_stats: DescriptiveStats {
                closed: 200,
                wins: 200,
                losses: 0,
                breakeven: 0,
                maximum_losing_streak: 0,
                sum_net_micro_usd: 200,
            },
            unresolved_oos_positions: 0,
            failed_windows: 0,
            uncertainty: None,
            block_uncertainty: None,
            robustness: Vec::new(),
            breakdown: Some(breakdown),
            robustness_assessment: Some(robustness::ProbeAssessment {
                method: robustness::ASSESSMENT_VERSION.into(),
                policy: probes.clone(),
                state: "PASS_DECLARED_PROBES".into(),
                cases: vec![robustness::ProbeDecision {
                    case_name: "abstract-frozen-case".into(),
                    state: "PASS_DECLARED_PROBES".into(),
                    reasons: Vec::new(),
                }],
                qualification_granted: false,
                limitations: "Abstract unit fixture, no market/source evidence".into(),
            }),
            win_evidence: Some(
                win_evidence::diagnose(
                    &(0..200)
                        .map(|i| Observation {
                            exit_ms: i,
                            net_micro_usd: 1,
                        })
                        .collect::<Vec<_>>(),
                    1000,
                    &win,
                )
                .unwrap(),
            ),
            evidence_gate: None,
        };
        (candidate, policy, win, probes)
    }
    #[test]
    fn all_abstract_numeric_criteria_still_never_qualify_or_remove_independent_blockers() {
        let (c, p, w, r) = fixture();
        let a = assess(&c, 2, 1000, &p, &w, &r).unwrap();
        assert!(a.criteria_passed);
        assert_eq!(a.state, "NUMERICAL_EVIDENCE_ONLY");
        assert!(!a.qualification_granted);
        assert!(!a.remaining_blockers.is_empty());
    }
    #[test]
    fn absent_annual_fold_unresolved_and_execution_evidence_hold_independently() {
        let (c, p, w, r) = fixture();
        for mode in 0..5 {
            let mut bad = c.clone();
            match mode {
                0 => {
                    bad.breakdown.as_mut().unwrap().exit_years.remove(&2026);
                }
                1 => bad.breakdown.as_mut().unwrap().folds[0].closed = 19,
                2 => bad.unresolved_oos_positions = 1,
                3 => bad.experiments[0].outcome = "failed".into(),
                _ => bad.experiments[1].experiment_id = bad.experiments[0].experiment_id.clone(),
            };
            let a = assess(&bad, 2, 1000, &p, &w, &r).unwrap();
            assert!(!a.criteria_passed);
            assert!(!a.qualification_granted);
        }
    }
    #[test]
    fn forged_final_pass_unknown_probes_and_inconsistent_counts_cannot_pass() {
        let (c, p, w, r) = fixture();
        let mut bad = c.clone();
        bad.win_evidence.as_mut().unwrap().log_final_evidence = -100.0;
        assert!(!assess(&bad, 2, 1000, &p, &w, &r).unwrap().criteria_passed);
        let mut bad = c.clone();
        bad.robustness_assessment.as_mut().unwrap().state = "UNKNOWN".into();
        assert!(!assess(&bad, 2, 1000, &p, &w, &r).unwrap().criteria_passed);
        let mut bad = c;
        bad.pooled_oos_reset_fold_stats.wins = 199;
        assert!(assess(&bad, 2, 1000, &p, &w, &r).is_err());
    }
    #[test]
    fn policies_refuse_weaker_contracts_and_multiplicity_or_method_mismatch_holds() {
        let (c, p, w, r) = fixture();
        for bad in [
            Policy {
                minimum_resolved_per_year: 49,
                ..p.clone()
            },
            Policy {
                minimum_resolved_per_fold: 19,
                ..p.clone()
            },
            Policy {
                minimum_net_win_rate_bps: 4999,
                ..p.clone()
            },
            Policy {
                maximum_consecutive_losses: 6,
                ..p.clone()
            },
            Policy {
                minimum_net_micro_usd: 0,
                ..p.clone()
            },
        ] {
            assert!(bad.validate().is_err());
        }
        assert!(!assess(&c, 2, 1001, &p, &w, &r).unwrap().criteria_passed);
        let mut bad = c;
        bad.win_evidence.as_mut().unwrap().method = "unknown".into();
        assert!(!assess(&bad, 2, 1000, &p, &w, &r).unwrap().criteria_passed);
    }
}
