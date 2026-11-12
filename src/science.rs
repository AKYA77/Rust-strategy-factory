//! Frozen chronological protocol execution. Executing evidence is distinct from qualification.
use crate::{
    Result,
    contracts::MAX_HOLD_MS,
    data,
    research::{self, Evaluation, Input},
    storage::{ExperimentKey, Registry, Stage},
    strategies::Strategy,
    tick_engine::Costs,
    validation::{Fold, Interval, Observation},
};
use chrono::{Datelike, Months};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub training: Interval,
    pub folds: Vec<Fold>,
    pub holdout: Interval,
    pub candidates: Vec<Strategy>,
    pub costs: Costs,
    /// Preregistered total selection/testing budget; not an assertion of independent trials.
    pub trial_budget: u64,
}
impl Plan {
    pub fn canonical(&self) -> Result<Self> {
        self.training.validate()?;
        self.holdout.validate()?;
        if self.folds.is_empty()
            || self.folds.len() > 16
            || self.candidates.is_empty()
            || self.candidates.len() > 32
            || self.trial_budget < self.candidates.len() as u64
        {
            return Err("scientific plan exceeds or omits candidate/fold/trial budget".into());
        }
        let mut candidates = self
            .candidates
            .iter()
            .map(|s| Ok((s.identity()?, s.canonical()?)))
            .collect::<Result<Vec<_>>>()?;
        let required_trials = candidates.iter().try_fold(0u64, |n, (_, strategy)| {
            Ok::<_, String>(n + 1 + crate::robustness::cases(strategy, &self.costs)?.len() as u64)
        })?;
        if self.trial_budget < required_trials {
            return Err("trial budget omits frozen robustness cases".into());
        }
        candidates.sort_by(|a, b| a.0.cmp(&b.0));
        if candidates.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err("duplicate frozen concept".into());
        }
        let mut end = self.training.end_exclusive_ms;
        let mut prior_train_end = self.training.end_exclusive_ms;
        for fold in &self.folds {
            fold.validate()?;
            if fold.train.end_exclusive_ms < prior_train_end
                || fold.train.start_ms != self.training.start_ms
                || fold.train.end_exclusive_ms < self.training.end_exclusive_ms
                || fold.test.start_ms < end
            {
                return Err("nonchronological or inconsistent prior-only folds".into());
            }
            let start = chrono::DateTime::from_timestamp_millis(fold.test.start_ms)
                .ok_or("invalid fold start")?;
            if start.day() != 1
                || start.time() != chrono::NaiveTime::MIN
                || start
                    .checked_add_months(Months::new(3))
                    .ok_or("fold calendar overflow")?
                    .timestamp_millis()
                    != fold.test.end_exclusive_ms
            {
                return Err(
                    "walk-forward tests must span three complete UTC calendar months".into(),
                );
            }
            prior_train_end = fold.train.end_exclusive_ms;
            end = fold.test.end_exclusive_ms;
        }
        if end
            .checked_add(MAX_HOLD_MS)
            .ok_or("holdout embargo overflow")?
            > self.holdout.start_ms
        {
            return Err("final holdout overlaps OOS or embargo".into());
        }
        Ok(Self {
            candidates: candidates.into_iter().map(|(_, s)| s).collect(),
            ..self.clone()
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub phase: String,
    pub experiment_id: String,
    pub outcome: String,
    pub reused: bool,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DescriptiveStats {
    pub closed: usize,
    pub wins: usize,
    pub losses: usize,
    pub breakeven: usize,
    pub maximum_losing_streak: usize,
    pub sum_net_micro_usd: i128,
}
impl DescriptiveStats {
    pub(crate) fn calculate(observations: &[Observation]) -> Result<Self> {
        let mut stats = Self::default();
        let mut streak = 0;
        for observation in observations {
            stats.closed += 1;
            stats.sum_net_micro_usd = stats
                .sum_net_micro_usd
                .checked_add(observation.net_micro_usd)
                .ok_or("pooled P&L overflow")?;
            if observation.net_micro_usd > 0 {
                stats.wins += 1;
                streak = 0;
            } else if observation.net_micro_usd < 0 {
                stats.losses += 1;
                streak += 1;
            } else {
                stats.breakeven += 1;
            }
            stats.maximum_losing_streak = stats.maximum_losing_streak.max(streak);
        }
        Ok(stats)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateEvidence {
    pub concept_id: String,
    pub experiments: Vec<Evidence>,
    pub pooled_oos_reset_fold_stats: DescriptiveStats,
    pub unresolved_oos_positions: usize,
    pub failed_windows: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncertainty: Option<crate::uncertainty::Diagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub robustness: Vec<crate::robustness::EvidenceSet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breakdown: Option<crate::diagnostics::Breakdown>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub protocol_id: String,
    pub dataset_hash: String,
    pub dataset_identity: String,
    pub compute_revision: String,
    pub status: String,
    pub data_class: data::DataClass,
    pub trial_budget: u64,
    pub candidates: Vec<CandidateEvidence>,
    pub missing_requirements: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolResult {
    pub reused: bool,
    pub report: Report,
}
fn definition(plan: &Plan, input: &Input, revision: &str) -> Result<serde_json::Value> {
    Ok(
        serde_json::json!({"version":"frozen-chronological-protocol-v5","regime_method":crate::regimes::VERSION,"breakdown_method":crate::diagnostics::VERSION,"robustness_method":crate::robustness::VERSION,"robustness_cases":plan.candidates.iter().map(|s|crate::robustness::cases(s,&plan.costs)).collect::<Result<Vec<_>>>()?,"uncertainty_method":crate::uncertainty::VERSION,"plan":plan,"dataset_hash":input.dataset.manifest.content_hash,"dataset_identity":data::dataset_identity(&input.dataset.manifest)?,"profile_hash":input.dataset.manifest.profile_hash,"conversions_hash":data::hash(&input.conversions)?,"revision":revision,"engine":crate::ENGINE_VERSION,"indicators":crate::INDICATOR_VERSION,"signals":crate::SIGNAL_VERSION}),
    )
}
/// Immutable report lookup verifies stored scientific evidence integrity.
pub fn read_report(registry: &Registry, id: &str) -> Result<Option<Report>> {
    let row:Option<(String,String)>=registry.connection.query_row("SELECT report_json,report_hash FROM scientific_protocols WHERE id=?1 AND report_json IS NOT NULL",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
    row.map(|(json, hash)| {
        let r: Report = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        if r.protocol_id != id || data::hash(&r)? != hash {
            return Err("scientific report integrity mismatch".into());
        }
        Ok(r)
    })
    .transpose()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Budget {
    pub feature_cells: usize,
    pub state_bytes: u64,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            feature_cells: 1_000_000,
            state_bytes: 256 * 1024 * 1024,
        }
    }
}
/// All definitions are frozen before any OOS or final evaluation. Retry reuses exact receipts.
pub fn execute(
    registry: &mut Registry,
    input: &Input,
    plan: &Plan,
    revision: &str,
    budget: &Budget,
) -> Result<ProtocolResult> {
    input.dataset.verify()?;
    if input.dataset.manifest.class != data::DataClass::SyntheticTestOnly {
        return Err("real protected-final execution blocked until independent qualification prerequisites are implemented".into());
    }
    if budget.feature_cells == 0 || budget.feature_cells > 4_000_000 {
        return Err("invalid protocol feature budget".into());
    }
    research::check_state_budget(registry, budget.state_bytes)?;
    let plan = plan.canonical()?;
    if input.conversions.len() != input.dataset.ticks.len() {
        return Err("event-aligned conversion coverage required".into());
    }
    for (tick, fx) in input.dataset.ticks.iter().zip(&input.conversions) {
        fx.validate(
            &input.dataset.profile,
            tick.timestamp_ms,
            plan.costs.max_conversion_age_ms,
        )?;
    }
    plan.costs
        .effective(&input.dataset.profile, input.dataset.ticks[0])?;
    let definition = definition(&plan, input, revision)?;
    let id = data::hash(&definition)?;
    let previous_definition: Option<String> = registry
        .connection
        .query_row(
            "SELECT definition_json FROM scientific_protocols WHERE id=?1",
            [&id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(previous) = &previous_definition
        && *previous != serde_json::to_string(&definition).map_err(|e| e.to_string())?
    {
        return Err("immutable scientific definition mismatch".into());
    }
    if let Some(report) = read_report(registry, &id)? {
        return Ok(ProtocolResult {
            reused: true,
            report,
        });
    }
    let ids = plan
        .candidates
        .iter()
        .map(|s| s.identity())
        .collect::<Result<Vec<_>>>()?;
    let mut allowed = Vec::new();
    for candidate in &plan.candidates {
        allowed.push((candidate.identity()?, data::hash(&plan.costs)?));
        for case in crate::robustness::cases(candidate, &plan.costs)? {
            allowed.push((case.strategy.identity()?, data::hash(&case.costs)?));
        }
    }
    // Prevent a retrospective OOS claim. Same-plan resumed tasks must match the frozen setup.
    {
        let mut q = registry
            .connection
            .prepare(
                "SELECT key_json FROM experiments WHERE json_extract(key_json,'$.instrument')=?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = q
            .query_map(params![input.dataset.profile.symbol], |r| {
                r.get::<_, String>(0)
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let key: ExperimentKey = serde_json::from_str(&row.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            let exposed = plan
                .folds
                .iter()
                .map(|f| &f.test)
                .chain(std::iter::once(&plan.holdout))
                .any(|r| key.start_ms < r.end_exclusive_ms && key.end_exclusive_ms > r.start_ms);
            if exposed
                && (previous_definition.is_none()
                    || key.protocol_hash != id
                    || !allowed.contains(&(key.concept_hash.clone(), key.cost_hash.clone()))
                    || key.code_revision != revision
                    || key.profile_hash != input.dataset.manifest.profile_hash)
            {
                return Err(
                    "independent range already exposed outside this frozen protocol".into(),
                );
            }
        }
    }
    // Idempotent identical reservation also permits recovery after reservation-before-definition crash.
    registry.reserve_instrument_holdout(
        &input.dataset.manifest.content_hash,
        &input.dataset.profile.symbol,
        &plan.holdout,
        &id,
        &ids,
    )?;
    let json = serde_json::to_string(&definition).map_err(|e| e.to_string())?;
    registry.connection.execute("INSERT OR IGNORE INTO scientific_protocols(id,definition_json,definition_hash) VALUES (?1,?2,?3)",params![id,json,data::hash(&definition)?]).map_err(|e|e.to_string())?;
    let stored: String = registry
        .connection
        .query_row(
            "SELECT definition_json FROM scientific_protocols WHERE id=?1",
            [&id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if stored != json {
        return Err("immutable scientific definition mismatch".into());
    }
    registry.reserve_protocol_ranges(
        &input.dataset.manifest.content_hash,
        &input.dataset.profile.symbol,
        &id,
        &plan
            .folds
            .iter()
            .map(|f| f.test.clone())
            .collect::<Vec<_>>(),
    )?;
    let regimes = crate::regimes::Classifier::new(&input.dataset)?;
    let mut candidates = Vec::new();
    for strategy in &plan.candidates {
        let mut experiments = Vec::new();
        let mut observations = Vec::new();
        let mut breakdown = crate::diagnostics::Breakdown::default();
        let mut unresolved = 0;
        let mut failed = 0;
        let mut windows = vec![(
            "selection-training".to_string(),
            plan.training.clone(),
            Stage::TickVerification,
            false,
        )];
        for (i, fold) in plan.folds.iter().enumerate() {
            windows.push((
                format!("fold-{i}-prior-training"),
                fold.train.clone(),
                Stage::TickVerification,
                false,
            ));
            windows.push((
                format!("fold-{i}-out-of-sample"),
                fold.test.clone(),
                Stage::TickVerification,
                true,
            ));
        }
        windows.push((
            "protected-final".into(),
            plan.holdout.clone(),
            Stage::IndependentValidation,
            false,
        ));
        for (phase, window, stage, is_oos) in windows {
            research::check_state_budget(registry, budget.state_bytes)?;
            let result = research::evaluate_window(
                registry,
                input,
                strategy,
                &plan.costs,
                revision,
                budget.feature_cells,
                &Evaluation {
                    window,
                    stage,
                    protocol_hash: id.clone(),
                },
            )?;
            if result.outcome == "failed" {
                failed += 1;
            }
            if is_oos {
                let mut fold = crate::diagnostics::FoldRisk::new(phase.clone());
                if let Some(trades) = result.result["tick_replay"]["trades"].as_array() {
                    if observations.len().saturating_add(trades.len()) > 1_000_000 {
                        return Err(
                            "protocol observation budget exceeded; committed evidence retained"
                                .into(),
                        );
                    }
                    fold.replay_present = true;
                    fold.unresolved = !result.result["tick_replay"]["unresolved"].is_null();
                    unresolved += usize::from(fold.unresolved);
                    for raw in trades {
                        let trade: crate::tick_engine::Trade =
                            serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
                        breakdown.record(&trade, &mut fold)?;
                        breakdown.record_regime(
                            regimes.at(trade.entry_event, trade.entry_ms),
                            trade.net_micro_usd,
                        )?;
                        observations.push(Observation {
                            exit_ms: trade.exit_ms,
                            net_micro_usd: trade.net_micro_usd,
                        });
                    }
                }
                breakdown.folds.push(fold);
            }
            experiments.push(Evidence {
                phase,
                experiment_id: result.experiment_id,
                outcome: result.outcome,
                reused: result.reused,
            });
        }
        candidates.push(CandidateEvidence {
            concept_id: strategy.identity()?,
            experiments,
            pooled_oos_reset_fold_stats: DescriptiveStats::calculate(&observations)?,
            unresolved_oos_positions: unresolved,
            failed_windows: failed,
            breakdown: Some(breakdown),
            robustness: crate::robustness::execute(
                registry, input, strategy, &plan, &id, revision, budget,
            )?,
            uncertainty: Some(crate::uncertainty::diagnose(
                &observations,
                plan.trial_budget,
            )?),
        });
    }
    // Numerical/descriptive execution alone cannot certify independent research qualification.
    let report=Report {protocol_id:id.clone(),dataset_hash:input.dataset.manifest.content_hash.clone(),dataset_identity:data::dataset_identity(&input.dataset.manifest)?,compute_revision:revision.into(),status:"INSUFFICIENT_EVIDENCE".into(),data_class:input.dataset.manifest.class,trial_budget:plan.trial_budget,candidates,
        missing_requirements:vec!["corpus-wide source-revision/alias exposure proof".into(),"approved full-history/profile/calendar evidence".into(),"dependence-aware uncertainty, regime validation and prespecified robustness acceptance gates".into(),"frozen full-period financial qualification and diversity ranking".into()]};
    let json = serde_json::to_string(&report).map_err(|e| e.to_string())?;
    if json.len() > 8 * 1024 * 1024 {
        return Err("scientific report exceeds metadata budget".into());
    }
    let n=registry.connection.execute("UPDATE scientific_protocols SET report_json=?2,report_hash=?3 WHERE id=?1 AND report_json IS NULL",params![id,json,data::hash(&report)?]).map_err(|e|e.to_string())?;
    if n != 1 {
        let stored = read_report(registry, &id)?.ok_or("concurrent report publication failed")?;
        return Ok(ProtocolResult {
            reused: true,
            report: stored,
        });
    }
    Ok(ProtocolResult {
        reused: false,
        report,
    })
}
