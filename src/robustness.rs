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
