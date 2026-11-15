//! Prespecified conditional-null evidence with arbitrary outcome dependence; not qualification.
use crate::{Result, validation::Observation};
use serde::{Deserialize, Serialize};
pub const VERSION: &str = "prespecified-conditional-win-e-process-v1";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub stake_basis_points: u16,
    pub family_error_basis_points: u16,
    pub minimum_resolved: usize,
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        if !(1..=9999).contains(&self.stake_basis_points)
            || !(1..=500).contains(&self.family_error_basis_points)
            || !(100..=1000000).contains(&self.minimum_resolved)
        {
            return Err("win evidence requires1..9999 stake bps,1..500 family error bps and100..1000000 minimum observations".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Diagnostic {
    pub method: String,
    pub policy: Policy,
    pub observations: usize,
    pub net_wins: usize,
    pub declared_trials: u64,
    pub log_final_evidence: f64,
    pub log_maximum_evidence: f64,
    pub log_family_threshold: f64,
    pub numerical_margin: f64,
    pub final_evidence_if_representable: Option<f64>,
    pub anytime_crossed: bool,
    pub fixed_horizon_gate_passed: bool,
    pub qualification_granted: bool,
    pub warnings: Vec<String>,
}
/// Under H0: P(net_win_t | all prior information) <= 1/2, the wealth process
/// product(1 + lambda*(2*X_t-1)) is a nonnegative supermartingale. No trade or
/// candidate independence is needed for Ville + a union bound over a complete
/// frozen family. Rejecting this null is not a stationary win-rate CI, positive
/// expectancy proof, future performance guarantee or independent qualification.
/// Only final evidence at the frozen horizon enters the software gate.
pub fn diagnose(
    trades: &[Observation],
    declared_trials: u64,
    policy: &Policy,
) -> Result<Diagnostic> {
    policy.validate()?;
    if declared_trials == 0 || trades.len() > 1000000 {
        return Err("invalid win evidence trial/observation budget".into());
    }
    if trades
        .iter()
        .any(|t| chrono::DateTime::from_timestamp_millis(t.exit_ms).is_none())
        || trades.windows(2).any(|w| w[0].exit_ms > w[1].exit_ms)
    {
        return Err("win evidence requires valid chronological resolved outcomes".into());
    }
    let lambda = f64::from(policy.stake_basis_points) / 10000.0;
    let threshold = (declared_trials as f64).ln()
        - (f64::from(policy.family_error_basis_points) / 10000.0).ln();
    let mut score = 0.0;
    let mut maximum = 0.0f64;
    let mut wins = 0;
    let positive = lambda.ln_1p();
    let negative = (-lambda).ln_1p();
    for (index, trade) in trades.iter().enumerate() {
        wins += usize::from(trade.net_micro_usd > 0);
        score = wins as f64 * positive + (index + 1 - wins) as f64 * negative;
        maximum = maximum.max(score);
    }
    let margin = 64.0
        * f64::EPSILON
        * (trades.len() as f64 * (positive.abs() + negative.abs()) + threshold.abs() + 1.0);
    let value = score.exp();
    Ok(Diagnostic{method:VERSION.into(),policy:policy.clone(),observations:trades.len(),net_wins:wins,declared_trials,log_final_evidence:score,log_maximum_evidence:maximum,log_family_threshold:threshold,numerical_margin:margin,final_evidence_if_representable:(value.is_finite()&&value>0.0).then_some(value),anytime_crossed:maximum-margin>=threshold,fixed_horizon_gate_passed:trades.len()>=policy.minimum_resolved&&score-margin>=threshold,qualification_granted:false,warnings:vec!["Conditional null: each resolved outcome's success probability given all prior information is at most1/2. This differs from an IID/common-probability win-rate claim.".into(),"Candidate, stake, outcome definition, horizon and complete multiplicity family must be frozen before exposure. A declared budget alone does not prove complete search accounting.".into(),"Breakevens are nonwins; unresolved positions, failed windows, annual samples, chronological coverage and full-period financial gates remain separate.".into(),"Final-horizon evidence alone is used; an earlier crossing cannot rescue a failed final gate. Synthetic evidence never qualifies a strategy.".into()]})
}

/// Check final evidence from exact counts; no earlier crossing can authorize a final gate.
pub fn verify_final(d: &Diagnostic) -> Result<()> {
    d.policy.validate()?;
    if d.method != VERSION
        || d.observations > 1000000
        || d.net_wins > d.observations
        || d.declared_trials == 0
        || d.qualification_granted
    {
        return Err("conditional evidence scope/counters invalid".into());
    }
    let lambda = f64::from(d.policy.stake_basis_points) / 10000.0;
    let positive = lambda.ln_1p();
    let negative = (-lambda).ln_1p();
    let score = d.net_wins as f64 * positive + (d.observations - d.net_wins) as f64 * negative;
    let threshold = (d.declared_trials as f64).ln()
        - (f64::from(d.policy.family_error_basis_points) / 10000.0).ln();
    let margin = 64.0
        * f64::EPSILON
        * (d.observations as f64 * (positive.abs() + negative.abs()) + threshold.abs() + 1.0);
    if !d.log_final_evidence.is_finite()
        || !d.log_maximum_evidence.is_finite()
        || !d.log_family_threshold.is_finite()
        || !d.numerical_margin.is_finite()
        || (d.log_final_evidence - score).abs() > margin
        || (d.log_family_threshold - threshold).abs() > margin
        || (d.numerical_margin - margin).abs() > 64.0 * f64::EPSILON * margin
        || d.log_maximum_evidence < 0.0
        || d.log_maximum_evidence + margin < score
        || d.log_maximum_evidence > d.net_wins as f64 * positive + margin
        || d.fixed_horizon_gate_passed
            != (d.observations >= d.policy.minimum_resolved && score - margin >= threshold)
        || d.anytime_crossed
            != (d.log_maximum_evidence - d.numerical_margin >= d.log_family_threshold)
    {
        return Err("conditional final evidence consistency mismatch".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> Policy {
        Policy {
            stake_basis_points: 5000,
            family_error_basis_points: 500,
            minimum_resolved: 100,
        }
    }
    fn observations(wins: usize, losses: usize) -> Vec<Observation> {
        (0..wins + losses)
            .map(|i| Observation {
                exit_ms: i as i64,
                net_micro_usd: if i < wins { 1 } else { -1 },
            })
            .collect()
    }
    #[test]
    fn exact_finite_null_trees_obey_wealth_and_anytime_multiplicity_bounds() {
        for dependent in [false, true] {
            let mut wealth = 0.0;
            let mut crossed = 0.0;
            for bits in 0u32..4096 {
                let mut probability = 1.0;
                let mut last = false;
                let mut trades = Vec::new();
                for step in 0..12 {
                    let win = bits & (1 << step) != 0;
                    let p = if dependent {
                        if last { 0.1 } else { 0.49 }
                    } else {
                        0.5
                    };
                    probability *= if win { p } else { 1.0 - p };
                    last = win;
                    trades.push(Observation {
                        exit_ms: step,
                        net_micro_usd: if win { 1 } else { 0 },
                    });
                }
                let d = diagnose(&trades, 1, &policy()).unwrap();
                wealth += probability * d.log_final_evidence.exp();
                crossed += if d.anytime_crossed { probability } else { 0.0 };
            }
            assert!(wealth <= 1.0 + 1e-12);
            assert!(crossed <= 0.05 + 1e-12);
            if !dependent {
                assert!((wealth - 1.0).abs() < 1e-12);
            }
        }
    }
    #[test]
    fn final_gate_never_uses_early_crossing_or_weaker_trial_correction() {
        let trades = observations(200, 200);
        let d = diagnose(&trades, 1, &policy()).unwrap();
        assert!(d.anytime_crossed);
        assert!(!d.fixed_horizon_gate_passed);
        let short = diagnose(&observations(99, 0), 1, &policy()).unwrap();
        assert!(!short.fixed_horizon_gate_passed);
        let passing = diagnose(&observations(200, 0), 1, &policy()).unwrap();
        assert!(passing.fixed_horizon_gate_passed);
        assert!(!passing.qualification_granted);
        let many = diagnose(&observations(200, 0), u64::MAX, &policy()).unwrap();
        assert!(many.log_family_threshold > passing.log_family_threshold);
    }
    #[test]
    fn extreme_scores_keep_finite_logs_and_refuse_invalid_policy_clock_and_order() {
        let d = diagnose(&observations(100000, 0), u64::MAX, &policy()).unwrap();
        assert!(d.log_final_evidence.is_finite());
        assert!(d.final_evidence_if_representable.is_none());
        let d = diagnose(&observations(0, 100000), 1, &policy()).unwrap();
        assert!(d.log_final_evidence.is_finite());
        assert!(d.final_evidence_if_representable.is_none());
        for bad in [
            Policy {
                stake_basis_points: 0,
                ..policy()
            },
            Policy {
                stake_basis_points: 10000,
                ..policy()
            },
            Policy {
                family_error_basis_points: 0,
                ..policy()
            },
            Policy {
                minimum_resolved: 99,
                ..policy()
            },
        ] {
            assert!(diagnose(&[], 1, &bad).is_err());
        }
        assert!(diagnose(&[], 0, &policy()).is_err());
        assert!(
            diagnose(
                &[Observation {
                    exit_ms: i64::MAX,
                    net_micro_usd: 1
                }],
                1,
                &policy()
            )
            .is_err()
        );
        assert!(
            diagnose(
                &[
                    Observation {
                        exit_ms: 1,
                        net_micro_usd: 1
                    },
                    Observation {
                        exit_ms: 0,
                        net_micro_usd: 1
                    }
                ],
                1,
                &policy()
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod roundtrip_tests {
    use super::*;
    #[test]
    fn log_evidence_and_numerical_margin_preserve_exact_report_hash_on_json_roundtrip() {
        let p = Policy {
            stake_basis_points: 1000,
            family_error_basis_points: 500,
            minimum_resolved: 100,
        };
        for count in [0, 1, 13, 101, 1000] {
            let trades = (0..count)
                .map(|i| Observation {
                    exit_ms: i,
                    net_micro_usd: if i % 3 == 0 { 1 } else { -1 },
                })
                .collect::<Vec<_>>();
            let d = diagnose(&trades, 1000, &p).unwrap();
            let raw = serde_json::to_string(&d).unwrap();
            let restored: Diagnostic = serde_json::from_str(&raw).unwrap();
            assert_eq!(d, restored);
            assert_eq!(
                crate::data::hash(&d).unwrap(),
                crate::data::hash(&restored).unwrap()
            );
        }
    }
}
