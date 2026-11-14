//! Prespecified OOS perturbations; never fit to, or evaluate on, protected-final quotes.
use crate::{
    Result, data,
    research::{self, Evaluation, Input},
    science::{self, DescriptiveStats, Evidence},
    storage::{Registry, Stage},
    strategies::Strategy,
    tick_engine::Costs,
    validation::Observation,
};
use serde::{Deserialize, Serialize};
pub const VERSION: &str = "fixed-cost-stop-rule-neighborhood-v3-atr";
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Case {
    pub name: String,
    pub strategy: Strategy,
    pub costs: Costs,
}
/// Fixed recipe, computed and bound before any OOS experiment, without performance input.
pub fn cases(strategy: &Strategy, costs: &Costs) -> Result<Vec<Case>> {
    let strategy = strategy.canonical()?;
    costs.validate()?;
    let mut adverse = costs.clone();
    adverse.spread_floor_pips = adverse
        .spread_floor_pips
        .checked_add(1)
        .ok_or("stress cost overflow")?;
    adverse.slippage_pips = adverse
        .slippage_pips
        .checked_add(1)
        .ok_or("stress cost overflow")?;
    adverse.validate()?;
    let mut cases = vec![Case {
        name: "spread-and-slippage-plus-one-pip".into(),
        strategy: strategy.clone(),
        costs: adverse,
    }];
    cases.extend(strategy_cases(&strategy, costs)?);
    Ok(cases)
}
/// Strategy-only probes preserve execution costs, including at the cost-domain ceiling.
pub fn strategy_cases(strategy: &Strategy, costs: &Costs) -> Result<Vec<Case>> {
    let strategy = strategy.canonical()?;
    costs.validate()?;
    let mut cases = Vec::new();
    let step = (strategy.stop_ticks / 10).max(1);
    for (name, stop) in [
        ("stop-minus-ten-percent", strategy.stop_ticks - step),
        ("stop-plus-ten-percent", strategy.stop_ticks + step),
    ] {
        if (1..=1_000_000).contains(&stop) {
            let mut neighbor = strategy.clone();
            neighbor.stop_ticks = stop;
            cases.push(Case {
                name: name.into(),
                strategy: neighbor.canonical()?,
                costs: costs.clone(),
            });
        }
    }
    if let Some(crate::stops::Policy::Atr {
        window,
        multiple_bps,
    }) = &strategy.stop_policy
    {
        let step = (*window / 10).max(1);
        for (name, next) in [
            ("atr-window-lower", window.saturating_sub(step)),
            ("atr-window-higher", window + step),
        ] {
            let mut neighbor = strategy.clone();
            neighbor.stop_policy = Some(crate::stops::Policy::Atr {
                window: next,
                multiple_bps: *multiple_bps,
            });
            if let Ok(neighbor) = neighbor.canonical() {
                cases.push(Case {
                    name: name.into(),
                    strategy: neighbor,
                    costs: costs.clone(),
                });
            }
        }
        let step = (*multiple_bps / 10).max(1);
        for (name, next) in [
            ("atr-multiple-lower", multiple_bps.saturating_sub(step)),
            ("atr-multiple-higher", multiple_bps + step),
        ] {
            let mut neighbor = strategy.clone();
            neighbor.stop_policy = Some(crate::stops::Policy::Atr {
                window: *window,
                multiple_bps: next,
            });
            if let Ok(neighbor) = neighbor.canonical() {
                cases.push(Case {
                    name: name.into(),
                    strategy: neighbor,
                    costs: costs.clone(),
                });
            }
        }
    }
    for (i, rule) in crate::neighborhood::rules(&strategy.rule)?
        .into_iter()
        .enumerate()
    {
        let mut neighbor = strategy.clone();
        neighbor.rule = rule;
        cases.push(Case {
            name: format!("rule-neighbor-{i}"),
            strategy: neighbor.canonical()?,
            costs: costs.clone(),
        });
    }
    Ok(cases)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceSet {
    pub method: String,
    pub case: Case,
    pub concept_id: String,
    pub cost_hash: String,
    pub experiments: Vec<Evidence>,
    pub pooled_reset_fold_stats: DescriptiveStats,
    pub unresolved_positions: usize,
    pub failed_windows: usize,
    pub status: String,
}
pub fn execute(
    registry: &mut Registry,
    input: &Input,
    strategy: &Strategy,
    plan: &science::Plan,
    protocol: &str,
    revision: &str,
    budget: &science::Budget,
) -> Result<Vec<EvidenceSet>> {
    let mut evidence = Vec::new();
    for case in cases(strategy, &plan.costs)? {
        let mut observations = Vec::new();
        let mut unresolved = 0;
        let mut failed = 0;
        let mut experiments = Vec::new();
        for (i, fold) in plan.folds.iter().enumerate() {
            research::check_state_budget(registry, budget.state_bytes)?;
            let result = research::evaluate_window(
                registry,
                input,
                &case.strategy,
                &case.costs,
                revision,
                budget.feature_cells,
                &Evaluation {
                    window: fold.test.clone(),
                    stage: Stage::TickVerification,
                    protocol_hash: protocol.into(),
                },
            )?;
            failed += usize::from(result.outcome == "failed");
            if let Some(trades) = result.result["tick_replay"]["trades"].as_array() {
                if observations.len().saturating_add(trades.len()) > 1_000_000 {
                    return Err("robustness observation budget exceeded; receipts retained".into());
                }
                observations.extend(
                    serde_json::from_value::<Vec<Observation>>(serde_json::Value::Array(
                        trades.clone(),
                    ))
                    .map_err(|e| e.to_string())?,
                );
                unresolved += usize::from(!result.result["tick_replay"]["unresolved"].is_null());
            }
            experiments.push(Evidence {
                phase: format!("robustness-{}-fold-{i}-out-of-sample", case.name),
                experiment_id: result.experiment_id,
                outcome: result.outcome,
                reused: result.reused,
            });
        }
        evidence.push(EvidenceSet {
            method: VERSION.into(),
            concept_id: case.strategy.identity()?,
            cost_hash: data::hash(&case.costs)?,
            case,
            experiments,
            pooled_reset_fold_stats: DescriptiveStats::calculate(&observations)?,
            unresolved_positions: unresolved,
            failed_windows: failed,
            status: "DESCRIPTIVE_ONLY".into(),
        });
    }
    Ok(evidence)
}

pub const ASSESSMENT_VERSION: &str = "frozen-descriptive-probe-policy-v1";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProbePolicy {
    pub minimum_resolved_per_probe: usize,
    pub minimum_net_win_rate_bps: u16,
    pub minimum_sum_net_micro_usd: i128,
    pub maximum_consecutive_losses: usize,
}
impl ProbePolicy {
    pub fn validate(&self) -> Result<()> {
        if !(1..=1_000_000).contains(&self.minimum_resolved_per_probe)
            || !(5000..=10000).contains(&self.minimum_net_win_rate_bps)
            || self.minimum_sum_net_micro_usd < 0
            || self.maximum_consecutive_losses > 5
        {
            return Err("invalid frozen robustness policy".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeDecision {
    pub case_name: String,
    pub state: String,
    pub reasons: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeAssessment {
    pub method: String,
    pub policy: ProbePolicy,
    pub state: String,
    pub cases: Vec<ProbeDecision>,
    pub qualification_granted: bool,
    pub limitations: String,
}
/// A prespecified descriptive decision over every frozen OOS probe. It cannot
/// promote a candidate or replace full-period/annual/scientific/source gates.
pub fn assess_probes(
    strategy: &Strategy,
    costs: &Costs,
    folds: usize,
    evidence: &[EvidenceSet],
    policy: &ProbePolicy,
) -> Result<ProbeAssessment> {
    policy.validate()?;
    if !(1..=16).contains(&folds) || evidence.len() > 32 {
        return Err("invalid robustness evidence budget".into());
    }
    let expected = cases(strategy, costs)?;
    let mut observed = std::collections::BTreeMap::new();
    for e in evidence {
        let Some(case) = expected.iter().find(|c| c.name == e.case.name) else {
            return Err("unexpected robustness case".into());
        };
        if e.method != VERSION
            || e.status != "DESCRIPTIVE_ONLY"
            || data::hash(&e.case)? != data::hash(case)?
            || e.concept_id != case.strategy.identity()?
            || e.cost_hash != data::hash(&case.costs)?
            || observed.insert(e.case.name.clone(), e).is_some()
        {
            return Err("robustness evidence identity mismatch or duplicate".into());
        }
        let stats = &e.pooled_reset_fold_stats;
        if e.failed_windows > folds
            || e.unresolved_positions > folds
            || stats.closed > 1_000_000
            || stats
                .wins
                .checked_add(stats.losses)
                .and_then(|n| n.checked_add(stats.breakeven))
                != Some(stats.closed)
            || stats.maximum_losing_streak > stats.losses
        {
            return Err("inconsistent robustness statistics".into());
        }
    }
    let mut decisions = Vec::new();
    for case in expected {
        let mut reasons = Vec::new();
        let mut state = "UNKNOWN";
        if let Some(e) = observed.get(&case.name) {
            let complete = e.experiments.len() == folds
                && e.failed_windows == 0
                && e.experiments.iter().enumerate().all(|(i, x)| {
                    x.outcome == "completed"
                        && x.phase == format!("robustness-{}-fold-{i}-out-of-sample", case.name)
                        && crate::machine_evidence::hex(&x.experiment_id)
                })
                && e.experiments
                    .iter()
                    .map(|x| &x.experiment_id)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    == folds;
            if !complete {
                reasons.push("incomplete-or-failed-fold-evidence".into());
            }
            if e.unresolved_positions > 0 {
                reasons.push("unresolved-positions".into());
            }
            if complete && e.unresolved_positions == 0 {
                let s = &e.pooled_reset_fold_stats;
                if s.closed < policy.minimum_resolved_per_probe {
                    reasons.push("insufficient-resolved-trades".into());
                }
                if s.closed == 0
                    || (s.wins as u128) * 10000
                        < (s.closed as u128) * u128::from(policy.minimum_net_win_rate_bps)
                {
                    reasons.push("net-win-rate-below-policy".into());
                }
                if s.sum_net_micro_usd < policy.minimum_sum_net_micro_usd {
                    reasons.push("reset-fold-net-sum-below-policy".into());
                }
                if s.maximum_losing_streak > policy.maximum_consecutive_losses {
                    reasons.push("losing-streak-above-policy".into());
                }
                state = if reasons.is_empty() {
                    "PASS_DECLARED_PROBES"
                } else {
                    "REJECTED"
                };
            }
        } else {
            reasons.push("missing-frozen-case".into());
        }
        decisions.push(ProbeDecision {
            case_name: case.name,
            state: state.into(),
            reasons,
        });
    }
    let state = if decisions.iter().any(|d| d.state == "REJECTED") {
        "REJECTED"
    } else if decisions.iter().any(|d| d.state == "UNKNOWN") {
        "UNKNOWN"
    } else {
        "PASS_DECLARED_PROBES"
    };
    Ok(ProbeAssessment{method:ASSESSMENT_VERSION.into(),policy:policy.clone(),state:state.into(),cases:decisions,qualification_granted:false,limitations:"Descriptive frozen reset-fold probe policy only; annual/full-period equity, source coverage, dependence/multiplicity, broader stability and independent qualification remain separate gates.".into()})
}
