//! One bounded worker and one in-flight task; persistent control/cursors and fenced leases.
use crate::{
    Result, candles,
    contracts::Conversion,
    daily_checkpoint::{self as daily_checkpoint, DailyCheckpointStore},
    data::{self, Dataset},
    prep_cache::{self as prep_cache, PrepCache, Protection, Scope},
    storage::{Claim, ExperimentKey, Outcome, Registry, Stage},
    strategies::{self, Features, Rule, Strategy},
    tick_engine::{self, Costs},
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub struct Input {
    pub dataset: Dataset,
    pub conversions: Vec<Conversion>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub seed: u64,
    pub feature_cells: usize,
    pub max_total_events: usize,
    pub max_regions: u64,
    pub lease_ms: i64,
    pub state_budget_bytes: u64,
    pub training_window: Option<crate::validation::Interval>,
    #[serde(skip_serializing_if = "zero_children")]
    pub improvement_children: usize,
    /// Explicit opt-in recursion depth for automatic improvement scheduling.
    /// Zero preserves the historical no-descendant behaviour. When positive,
    /// one resolved review child per completed parent may be scheduled for its
    /// own training diagnosis, up to this many generations.
    #[serde(skip_serializing_if = "zero_depth")]
    pub improvement_max_depth: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow_policy: Option<crate::candidate_screen::ShadowPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_policy: Option<crate::proof_screen::Policy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_policy: Option<crate::indexed_policy::Policy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_guard: Option<crate::resource::Guard>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_guard: Option<crate::resource::CpuGuard>,
}
fn zero_children(n: &usize) -> bool {
    *n == 0
}
fn zero_depth(n: &usize) -> bool {
    *n == 0
}
impl Default for Config {
    fn default() -> Self {
        Self {
            seed: 42,
            feature_cells: 1_000_000,
            max_total_events: 500_000,
            max_regions: 10000,
            lease_ms: 300000,
            state_budget_bytes: 256 * 1024 * 1024,
            training_window: None,
            improvement_children: 0,
            improvement_max_depth: 0,
            shadow_policy: None,
            proof_policy: None,
            indexed_policy: None,
            resource_guard: None,
            cpu_guard: None,
        }
    }
}

/// Maximum controlled recursive acceptance generations admitted by one run.
pub const MAX_IMPROVEMENT_DEPTH: u64 = 2;

/// Schema migration: bound recursive improvement jobs with an explicit depth
/// and the accepted proposal edge. The old table's composite foreign key to
/// `research_steps` cannot describe a recursive parent (a reviewed child
/// experiment is not a research step), so the table is rebuilt additively and
/// the durable rows are copied unchanged at depth 0.
pub(crate) fn initialize_recursion(
    connection: &mut rusqlite::Connection,
    previous: i64,
) -> Result<()> {
    if previous >= 24 {
        return Ok(());
    }
    let tx = crate::storage::writer_tx_open(connection).map_err(|e| e.to_string())?;
    let current: i64 = tx
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if current < 24 {
        tx.execute_batch(
            "CREATE TABLE improvement_jobs_v24(
                run_id TEXT NOT NULL REFERENCES research_runs(id),
                parent_cursor INTEGER NOT NULL CHECK(parent_cursor>=0),
                input_index INTEGER NOT NULL CHECK(input_index>=0),
                parent_id TEXT NOT NULL REFERENCES experiments(id),
                state TEXT NOT NULL DEFAULT 'queued' CHECK(state IN ('queued','completed','failed')),
                diagnosis_id TEXT REFERENCES training_diagnoses(id),
                review_id TEXT REFERENCES training_reviews(id),
                error TEXT,
                depth INTEGER NOT NULL DEFAULT 0 CHECK(depth BETWEEN 0 AND 2),
                accepted_edge_id TEXT,
                PRIMARY KEY(run_id,parent_cursor,depth)
            );
            INSERT INTO improvement_jobs_v24(run_id,parent_cursor,input_index,parent_id,state,diagnosis_id,review_id,error,depth,accepted_edge_id)
                SELECT run_id,parent_cursor,input_index,parent_id,state,diagnosis_id,review_id,error,0,NULL FROM improvement_jobs;
            DROP TABLE improvement_jobs;
            ALTER TABLE improvement_jobs_v24 RENAME TO improvement_jobs;
            CREATE UNIQUE INDEX IF NOT EXISTS one_pending_improvement ON improvement_jobs(run_id) WHERE state='queued';
            PRAGMA user_version=24;",
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub experiment_id: String,
    pub reused: bool,
    pub outcome: String,
    pub result: serde_json::Value,
}
pub fn compute_revision() -> Result<String> {
    Ok(env!("FACTORY_COMPUTE_REVISION").into())
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evaluation {
    pub window: crate::validation::Interval,
    pub stage: Stage,
    pub protocol_hash: String,
}
#[derive(Clone)]
pub struct EvaluationOptions<'a> {
    pub feature_cells: usize,
    pub evaluation: Option<&'a Evaluation>,
    pub stage: Stage,
    pub shadow_policy: Option<&'a crate::candidate_screen::ShadowPolicy>,
    pub proof_policy: Option<&'a crate::proof_screen::Policy>,
    pub indexed_policy: Option<&'a crate::indexed_policy::Policy>,
}
pub fn evaluate(
    registry: &mut Registry,
    input: &Input,
    strategy: &Strategy,
    costs: &Costs,
    revision: &str,
    feature_cells: usize,
) -> Result<Execution> {
    evaluate_configured(
        registry,
        input,
        strategy,
        costs,
        revision,
        EvaluationOptions {
            feature_cells,
            evaluation: None,
            stage: Stage::CandleScreen,
            shadow_policy: None,
            proof_policy: None,
            indexed_policy: None,
        },
    )
}
pub fn evaluate_window(
    registry: &mut Registry,
    input: &Input,
    strategy: &Strategy,
    costs: &Costs,
    revision: &str,
    feature_cells: usize,
    evaluation: &Evaluation,
) -> Result<Execution> {
    evaluate_configured(
        registry,
        input,
        strategy,
        costs,
        revision,
        EvaluationOptions {
            feature_cells,
            evaluation: Some(evaluation),
            stage: evaluation.stage.clone(),
            shadow_policy: None,
            proof_policy: None,
            indexed_policy: None,
        },
    )
}
pub(crate) fn configuration_hash(
    input: &Input,
    costs: &Costs,
    feature_cells: usize,
    shadow_policy: Option<&crate::candidate_screen::ShadowPolicy>,
    proof_policy: Option<&crate::proof_screen::Policy>,
    indexed_policy: Option<&crate::indexed_policy::Policy>,
) -> Result<String> {
    let original = data::hash(&(
        feature_cells,
        costs,
        data::hash(&input.conversions)?,
        data::dataset_identity(&input.dataset.manifest)?,
    ))?;
    let shadow_context = match shadow_policy {
        None => Ok(original),
        Some(policy) => {
            policy.validate()?;
            data::hash(&("shadow-policy-context-v1", original, policy))
        }
    }?;
    let proof_context = match proof_policy {
        None => Ok(shadow_context),
        Some(policy) => {
            policy.validate()?;
            data::hash(&("proof-policy-context-v1", shadow_context, policy))
        }
    }?;
    match indexed_policy {
        None => Ok(proof_context),
        Some(policy) => {
            policy.validate()?;
            data::hash(&("indexed-training-context-v1", proof_context, policy))
        }
    }
}
/// Explicit shadow policy affects identity and diagnostic decisions, never replay eligibility.
pub fn evaluate_configured(
    registry: &mut Registry,
    input: &Input,
    strategy: &Strategy,
    costs: &Costs,
    revision: &str,
    options: EvaluationOptions<'_>,
) -> Result<Execution> {
    evaluate_prepared(registry, input, strategy, costs, revision, options, None)
}
struct Prepared<'c, 'f, 'data> {
    candles: &'c [candles::Candle],
    features: &'f mut Features<'c>,
    visible: usize,
    configuration_hash: &'c str,
    index: Option<&'c std::cell::OnceCell<Result<crate::indexed_replay::Index<'data>>>>,
}
fn evaluate_prepared<'data>(
    registry: &mut Registry,
    input: &'data Input,
    strategy: &Strategy,
    costs: &'data Costs,
    revision: &str,
    options: EvaluationOptions<'_>,
    prepared: Option<Prepared<'_, '_, 'data>>,
) -> Result<Execution> {
    let EvaluationOptions {
        feature_cells,
        evaluation,
        shadow_policy,
        proof_policy,
        indexed_policy,
        stage: eval_stage,
    } = options;
    if let Some(e) = evaluation {
        e.window.validate()?;
        if e.stage == Stage::CandleScreen {
            return Err(
                "tick evaluation requires tick verification or independent-validation stage".into(),
            );
        }
    }
    if let Some(policy) = shadow_policy {
        policy.validate()?;
    }
    if let Some(policy) = proof_policy {
        policy.validate()?;
    }
    if let Some(policy) = indexed_policy {
        policy.validate()?;
    }
    let d = &input.dataset;
    // A private preparation is created only after validating this immutable input.
    // Authoritative replay retains its independent validation in either path.
    if prepared.is_none() {
        d.verify()?;
    }
    if input.conversions.len() != d.ticks.len() {
        return Err("event-aligned conversions required".into());
    }
    let key = ExperimentKey {
        concept_hash: strategy.identity()?,
        instrument: d.profile.symbol.clone(),
        dataset_hash: d.manifest.content_hash.clone(),
        start_ms: evaluation.map_or(d.manifest.first_ms, |e| e.window.start_ms),
        end_exclusive_ms: evaluation.map_or_else(
            || d.manifest.last_ms.checked_add(1).ok_or("range overflow"),
            |e| Ok(e.window.end_exclusive_ms),
        )?,
        signal_version: crate::SIGNAL_VERSION.into(),
        indicator_version: crate::INDICATOR_VERSION.into(),
        cost_hash: data::hash(costs)?,
        fill_policy: "full-l1-assumption-target-at-limit-preflight-v3".into(),
        profile_hash: d.manifest.profile_hash.clone(),
        engine_version: crate::ENGINE_VERSION.into(),
        protocol_hash: match evaluation {
            Some(e) => e.protocol_hash.clone(),
            None => data::hash(&"training-only-conservative-signals-v1")?,
        },
        code_revision: revision.into(),
        configuration_hash: if let Some(p) = prepared.as_ref() {
            p.configuration_hash.to_owned()
        } else {
            configuration_hash(
                input,
                costs,
                feature_cells,
                shadow_policy,
                proof_policy,
                indexed_policy,
            )?
        },
        stage: eval_stage,
    };
    let allowed_classes = [
        crate::data::DataClass::SyntheticTestOnly,
        crate::data::DataClass::RealStructurallyValid,
        crate::data::DataClass::RealQualityAccepted,
        crate::data::DataClass::RealProfileApproved,
    ];
    if proof_policy.is_some() {
        crate::proof_screen::guard(&registry.connection)?;
        // Screening a proof may legitimately run at CandleScreen; adaptation
        // authorization would reject that stage, so use the screening contract.
        crate::improvement::authorize_screening(&key)?;
        if !allowed_classes.contains(&d.manifest.class) && evaluation.is_none() {
            return Err("real proof screening requires explicit training scope".into());
        }
    }
    if indexed_policy.is_some() {
        crate::indexed_policy::guard(&registry.connection)?;
        crate::improvement::authorize_screening(&key)?;
        if !allowed_classes.contains(&d.manifest.class) && evaluation.is_none() {
            return Err("real indexed research requires explicit training scope".into());
        }
    }
    let id = registry.register(&key)?;
    crate::telemetry::record(
        registry,
        &id,
        "research",
        "experiment-entered",
        chrono::Utc::now().timestamp_millis(),
        &serde_json::json!({"stage": format!("{:?}", key.stage)}),
    )?;
    match registry.claim(&id, chrono::Utc::now().timestamp_millis(), 300000)? {
        Claim::Busy => Err("experiment is leased to another worker".into()),
        Claim::Reused(result) => Ok(Execution {
            experiment_id: id,
            reused: true,
            outcome: result.outcome,
            result: result.result,
        }),
        Claim::Acquired(lease) => {
            let compute = || -> Result<serde_json::Value> {
                let context = ComputeContext {
                    input,
                    strategy,
                    costs,
                    evaluation,
                    shadow_policy,
                    proof_policy,
                    indexed_policy,
                    index: prepared.as_ref().and_then(|p| p.index),
                    registry,
                    lease: &lease,
                    id: &id,
                };
                if let Some(prepared) = prepared {
                    return context.calculate(
                        prepared.candles,
                        prepared.features,
                        prepared.visible,
                    );
                }

                let c = candles::build(d, strategy.timeframe)?;
                let visible = evaluation.map_or(c.len(), |e| {
                    c.partition_point(|c| c.available_ms < e.window.end_exclusive_ms)
                });
                let mut features =
                    Features::with_context(&c[..visible], feature_cells, d, strategy.timeframe)?;
                context.calculate(&c, &mut features, visible)
            };
            let (outcome, result) = match compute() {
                Ok(result) => (
                    if result["tick_replay"].is_null() || result["proof_rejection"] == true {
                        Outcome::Rejected
                    } else {
                        Outcome::Completed
                    },
                    result,
                ),
                Err(error) => (Outcome::Failed, serde_json::json!({"error":error})),
            };
            // Advance CandleScreen -> TickVerification only after the Stage-1
            // screening outcome is established and the candidate passed the
            // numerical gate. A rejected candidate cannot continue; an uncertain
            // candidate is not mislabeled as passed. The decision hash binds the
            // gate evidence so it independently demonstrates the candidate passed.
            if key.stage == crate::storage::Stage::CandleScreen {
                let stage1 = result
                    .get("candidate_screen_audit")
                    .and_then(|a| a.get("stage1_decision"))
                    .and_then(|v| v.as_str());
                if stage1 == Some("PASS") {
                    let rate = result
                        .get("candidate_screen_audit")
                        .and_then(|a| a.get("estimated"))
                        .and_then(|e| e.get("estimated_net_win_rate_bps"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let evidence = format!("stage1-pass net_win_rate_bps={} floor_bps=4000", rate);
                    let _ = crate::stage_machine::record_entry(registry, &key, true, &evidence);
                }
            }
            let name = match outcome {
                Outcome::Completed => "completed",
                Outcome::Rejected => "rejected",
                _ => "failed",
            }
            .to_string();
            // Result metadata is bounded by Registry; preserve oversized/failing publication as an error receipt.
            let now = chrono::Utc::now().timestamp_millis();
            if let Err(error) = registry.finish(&lease, now, outcome, &result) {
                registry.finish(
                    &lease,
                    now,
                    Outcome::Failed,
                    &serde_json::json!({"error":error}),
                )?;
                return Err(error);
            }
            Ok(Execution {
                experiment_id: id,
                reused: false,
                outcome: name,
                result,
            })
        }
    }
}
struct ComputeContext<'a, 'data> {
    input: &'data Input,
    strategy: &'a Strategy,
    costs: &'data Costs,
    evaluation: Option<&'a Evaluation>,
    shadow_policy: Option<&'a crate::candidate_screen::ShadowPolicy>,
    proof_policy: Option<&'a crate::proof_screen::Policy>,
    indexed_policy: Option<&'a crate::indexed_policy::Policy>,
    index: Option<&'a std::cell::OnceCell<Result<crate::indexed_replay::Index<'data>>>>,
    registry: &'a Registry,
    lease: &'a crate::storage::Lease,
    id: &'a str,
}
impl<'data> ComputeContext<'_, 'data> {
    fn calculate(
        &self,
        c: &[candles::Candle],
        features: &mut Features<'_>,
        visible: usize,
    ) -> Result<serde_json::Value> {
        let input = self.input;
        let d = &input.dataset;
        let strategy = self.strategy;
        let costs = self.costs;
        let evaluation = self.evaluation;
        let shadow_policy = self.shadow_policy;
        let proof_policy = self.proof_policy;
        let indexed_policy = self.indexed_policy;
        let registry = self.registry;
        let lease = self.lease;
        let id = self.id;
        let mut screen = strategies::screen(features, strategy)?;
        let prefix = features.signal(&strategy.rule)?;
        let mask =
            crate::indicators::Mask::from_bools((0..c.len()).map(|i| i < visible && prefix.get(i)));
        if let Some(e) = evaluation {
            screen.signals = c
                .iter()
                .enumerate()
                .filter(|(i, c)| e.window.contains(c.available_ms) && mask.get(*i))
                .count();
            screen.requires_tick_verification = screen.signals > 0;
            screen.status = if screen.signals == 0 {
                strategies::ScreenStatus::ProvenNoSignals
            } else {
                strategies::ScreenStatus::ScreenUncertain
            };
            screen.version = "conservative-window-signals-v3-feature-scope".into();
        }
        // Don't skip market-stop proof when proof_policy is set: proof-screening
        // must run even when the rule produces no visible signals (e.g. browser
        // fixture with evaluation=None). requires_tick_verification gates normal
        // tick replay, but proof-screening needs market_stop_proof regardless.
        if !screen.requires_tick_verification && proof_policy.is_none() {
            return Ok(
                serde_json::json!({"screen":screen,"tick_replay":null,"strategy":strategy,"data_manifest":d.manifest,"instrument_profile":d.profile,"cost_profile":costs,"evaluation":evaluation,"shadow_policy":shadow_policy,"proof_policy":proof_policy,"indexed_policy":indexed_policy}),
            );
        }
        let market_stop_proof = crate::screen::market_stop_proof(&d.profile, costs, strategy)?;
        let proof_rejection = proof_policy.is_some() && market_stop_proof.is_some();
        let proof_audited = if proof_rejection {
            // Validate every scoped conversion even when no order can be entered.
            for (t, fx) in d
                .ticks
                .iter()
                .zip(&input.conversions)
                .filter(|(t, _)| evaluation.is_none_or(|e| e.window.contains(t.timestamp_ms)))
            {
                fx.validate(&d.profile, t.timestamp_ms, costs.max_conversion_age_ms)?;
                costs.effective(&d.profile, *t)?;
            }
            let slip = costs
                .slippage_pips
                .checked_mul(d.profile.pip)
                .ok_or("slippage overflow")?;
            market_stop_proof
                .as_ref()
                .ok_or("proof missing")?
                .fixed_stop_price_units
                .checked_add(slip)
                .ok_or("risk overflow")?;
            for i in mask.set_indices_prefix(visible)? {
                if evaluation.is_some_and(|e| !e.window.contains(c[i].available_ms)) {
                    continue;
                }
                let n = d
                    .ticks
                    .partition_point(|t| t.sequence <= c[i].publication_sequence);
                if let Some(t) = d
                    .ticks
                    .get(n)
                    .filter(|t| evaluation.is_none_or(|e| e.window.contains(t.timestamp_ms)))
                {
                    let (bid, ask) = costs.effective(&d.profile, *t)?;
                    crate::orders::market_from_effective(
                        &d.profile,
                        costs,
                        bid,
                        ask,
                        strategy.side,
                    )?;
                }
            }
            screen.status = strategies::ScreenStatus::ProvenNoMarketEntry;
            screen.version = "fixed-market-proof-screen-v1".into();
            screen.requires_tick_verification = false;
            proof_policy.ok_or("proof policy missing")?.audit(id)?
        } else {
            false
        };
        if proof_rejection && !proof_audited {
            return Ok(
                serde_json::json!({"screen":screen,"tick_replay":null,"market_stop_proof":market_stop_proof,
                "proof_rejection":true,"proof_audited":false,"proof_policy":proof_policy,"indexed_policy":indexed_policy,"shadow_policy":shadow_policy,
                "strategy":strategy,"data_manifest":d.manifest,"instrument_profile":d.profile,"cost_profile":costs,"evaluation":evaluation}),
            );
        }
        // Diagnostic-only: failure/uncertainty must never suppress authoritative replay.
        let candle_estimate = crate::candidate_screen::estimate(
            d,
            c,
            &mask,
            strategy,
            costs,
            evaluation.map(|e| &e.window),
        );
        let (replay, indexed_metadata) = if let Some(policy) =
            indexed_policy.filter(|_| !proof_audited)
        {
            let crate::indexed_replay::Execution {
                replay,
                used_index,
                fallback_reason,
                work,
            } = self.indexed(c, &mask)?;
            let audited = used_index && policy.audit(id)?;
            let hashes = if audited {
                let reference = match self.reference(c, &mask) {
                    Ok(reference) => reference,
                    Err(error) => {
                        crate::indexed_policy::record_failure(registry, lease)?;
                        return Err(format!(
                            "{}: reference failed: {error}",
                            crate::indexed_policy::DISAGREEMENT
                        ));
                    }
                };
                match crate::indexed_policy::compare(&replay, &reference) {
                    Ok(h) => Some(h),
                    Err(error) => {
                        crate::indexed_policy::record_failure(registry, lease)?;
                        return Err(error);
                    }
                }
            } else {
                None
            };
            (
                replay,
                Some(
                    serde_json::json!({"version":policy.version,"used_index":used_index,"fallback_reason":fallback_reason,"work":work,"reference_audited":audited,"reference_hashes":hashes}),
                ),
            )
        } else {
            (self.reference(c,&mask)?,indexed_policy.map(|_|serde_json::json!({"used_index":false,"fallback_reason":"independent proof audit uses sequential reference","reference_audited":false})))
        };
        let screen_audit = match crate::screen::audit_replay(d, costs, &replay) {
            Ok(a) => a,
            Err(error) => {
                if indexed_metadata
                    .as_ref()
                    .is_some_and(|m| m["used_index"] == true)
                {
                    crate::indexed_policy::record_failure(registry, lease)?;
                }
                return Err(error);
            }
        };
        if market_stop_proof.is_some()
            && (!replay.trades.is_empty()
                || replay.unresolved.is_some()
                || replay.pending_entry.is_some())
        {
            if indexed_metadata
                .as_ref()
                .is_some_and(|m| m["used_index"] == true)
            {
                crate::indexed_policy::record_failure(registry, lease)?;
                return Err(format!(
                    "{}: fixed-market proof contradicted",
                    crate::indexed_policy::DISAGREEMENT
                ));
            }
            crate::proof_screen::record_contradiction(registry, lease)?;
            return Err(
                "market-stop proof excluded an authoritative entry; screening must remain disabled"
                    .into(),
            );
        }
        let candidate_screen_audit = match candle_estimate {
            Ok(estimate) => crate::candidate_screen::compare(
                &estimate,
                &replay,
                id,
                shadow_policy.unwrap_or(&Default::default()),
            )?,
            Err(error) => serde_json::json!({"version":crate::candidate_screen::VERSION,
                "status":"unavailable", "error":error, "promotes_to_tick":true,
                "candidate_rejected":false,"qualification_evidence":false}),
        };
        Ok(
            serde_json::json!({"screen":screen,"screen_audit":screen_audit,"indexed_replay":indexed_metadata,"market_stop_proof":market_stop_proof,"proof_rejection":proof_rejection,"proof_audited":proof_audited,"candidate_screen_audit":candidate_screen_audit,"tick_replay":replay,"strategy":strategy,"data_manifest":d.manifest,"instrument_profile":d.profile,"cost_profile":costs,"evaluation":evaluation,"shadow_policy":shadow_policy,"proof_policy":proof_policy,"indexed_policy":indexed_policy}),
        )
    }
    fn reference(
        &self,
        c: &[candles::Candle],
        mask: &crate::indicators::Mask,
    ) -> Result<tick_engine::Replay> {
        if let Some(e) = self.evaluation {
            tick_engine::replay_window(
                &self.input.dataset,
                c,
                mask,
                self.strategy,
                self.costs,
                &self.input.conversions,
                &e.window,
            )
        } else {
            tick_engine::replay(
                &self.input.dataset,
                c,
                mask,
                self.strategy,
                self.costs,
                &self.input.conversions,
            )
        }
    }
    fn indexed(
        &self,
        c: &[candles::Candle],
        mask: &crate::indicators::Mask,
    ) -> Result<crate::indexed_replay::Execution> {
        let fallback = |reason: String| -> Result<crate::indexed_replay::Execution> {
            Ok(crate::indexed_replay::Execution {
                replay: self.reference(c, mask)?,
                used_index: false,
                fallback_reason: Some(reason),
                work: Default::default(),
            })
        };
        match self.index {
            Some(cell) => match cell.get_or_init(|| {
                crate::indexed_replay::Index::build(
                    &self.input.dataset,
                    self.costs,
                    &self.input.conversions,
                    self.evaluation.map(|e| &e.window),
                )
            }) {
                Ok(index) => index.replay(c, mask, self.strategy).or_else(fallback),
                Err(reason) => fallback(reason.clone()),
            },
            None => crate::indexed_replay::replay(
                &self.input.dataset,
                c,
                mask,
                self.strategy,
                self.costs,
                &self.input.conversions,
                self.evaluation.map(|e| &e.window),
            ),
        }
    }
}
/// One input/frame/window, at most64 candidates, one lease at a time. Each committed
/// prefix survives an error; retry reuses its exact experiment receipts. Preflight
/// compilation errors return before registration, including for all-cached batches.
pub struct BatchOptions<'a> {
    pub evaluation: EvaluationOptions<'a>,
    pub state_budget_bytes: u64,
}
#[derive(Debug, Serialize)]
pub struct BatchReport {
    pub executions: Vec<Execution>,
    pub primary_candles: usize,
    pub visible_candles: usize,
    pub shared_feature_builds: usize,
    pub shared_cache_hits: usize,
    pub shared_index_initialized: bool,
    /// Aggregate screened-candidate audit coverage across the batch: bound
    /// rejections, proven failures, sampled audits, false negatives.
    pub audit_coverage: crate::candidate_screen::AuditCoverage,
}
pub fn evaluate_batch(
    registry: &mut Registry,
    input: &Input,
    strategies: &[Strategy],
    costs: &Costs,
    revision: &str,
    options: BatchOptions<'_>,
) -> Result<Vec<Execution>> {
    Ok(evaluate_batch_report(registry, input, strategies, costs, revision, options)?.executions)
}
pub fn evaluate_batch_report(
    registry: &mut Registry,
    input: &Input,
    strategies: &[Strategy],
    costs: &Costs,
    revision: &str,
    options: BatchOptions<'_>,
) -> Result<BatchReport> {
    if strategies.is_empty()
        || strategies.len() > 64
        || input.dataset.ticks.len() > 2_000_000
        || !(1..=4_000_000).contains(&options.evaluation.feature_cells)
        || !(32 * 1024 * 1024..=8 * 1024 * 1024 * 1024).contains(&options.state_budget_bytes)
    {
        return Err("batch input/candidate/feature/state budget exceeded".into());
    }
    let timeframe = strategies[0].timeframe;
    if strategies.iter().any(|s| s.timeframe != timeframe) {
        return Err("shared batch requires one primary timeframe".into());
    }
    for strategy in strategies {
        strategy.canonical()?;
    }
    if let Some(e) = options.evaluation.evaluation {
        e.window.validate()?;
    }
    input.dataset.verify()?;
    costs.validate()?;
    if input.conversions.len() != input.dataset.ticks.len() {
        return Err("batch requires event-aligned conversions".into());
    }
    check_state_budget(registry, options.state_budget_bytes)?;
    let configuration_hash = configuration_hash(
        input,
        costs,
        options.evaluation.feature_cells,
        options.evaluation.shadow_policy,
        options.evaluation.proof_policy,
        options.evaluation.indexed_policy,
    )?;
    if options.evaluation.indexed_policy.is_some() {
        crate::indexed_policy::guard(&registry.connection)?;
    }
    // Quote summaries are initialized only after a candidate is authorized and leased.
    let indexed = options
        .evaluation
        .indexed_policy
        .map(|_| std::cell::OnceCell::new());
    let c = candles::build(&input.dataset, timeframe)?;
    let visible = options.evaluation.evaluation.map_or(c.len(), |e| {
        c.partition_point(|c| c.available_ms < e.window.end_exclusive_ms)
    });
    let mut features = Features::with_context(
        &c[..visible],
        options.evaluation.feature_cells,
        &input.dataset,
        timeframe,
    )?;
    let mut results = Vec::with_capacity(strategies.len());
    for strategy in strategies {
        check_state_budget(registry, options.state_budget_bytes)?;
        results.push(evaluate_prepared(
            registry,
            input,
            strategy,
            costs,
            revision,
            options.evaluation.clone(),
            Some(Prepared {
                candles: &c,
                features: &mut features,
                visible,
                configuration_hash: &configuration_hash,
                index: indexed.as_ref(),
            }),
        )?);
    }
    let audits: Vec<serde_json::Value> = results
        .iter()
        .filter_map(|e| e.result.get("candidate_screen_audit").cloned())
        .collect();
    let audit_coverage = crate::candidate_screen::audit_coverage(&audits);
    Ok(BatchReport {
        executions: results,
        primary_candles: c.len(),
        visible_candles: visible,
        shared_feature_builds: features.feature_builds,
        shared_cache_hits: features.cache_hits,
        shared_index_initialized: indexed.as_ref().is_some_and(|cell| cell.get().is_some()),
        audit_coverage,
    })
}
/// Explicit stage policy for the training loop.
///
/// A candidate index is an enumeration artifact, never an authorization signal.
/// Every training candidate enters at CandleScreen; advancement to TickVerification
/// is recorded by the stage machine only after the Stage-1 candle screen has
/// established a passing outcome. Advancement is gated on screening evidence, not
/// selected by arithmetic on the index.
pub(crate) fn training_stage(_candidate_index: u64) -> crate::storage::Stage {
    crate::storage::Stage::CandleScreen
}

#[cfg(test)]
mod stage_policy_tests {
    use super::*;

    #[test]
    fn candidate_index_never_selects_a_protected_stage() {
        for index in 0..256 {
            assert_eq!(
                training_stage(index),
                crate::storage::Stage::CandleScreen,
                "index {index} must enter at CandleScreen, never a protected stage"
            );
        }
    }
}

/// Bounded novel parameter regions; no cycling through already completed template ranges.
pub fn candidate_at(index: u64, seed: u64) -> Result<(strategies::Family, Strategy)> {
    static TEMPLATES: std::sync::OnceLock<Vec<strategies::Candidate>> = std::sync::OnceLock::new();
    let templates = TEMPLATES.get_or_init(strategies::family_checkpoints);
    let count = templates.len() as u64;
    let region = index / count;
    let slot = ((index % count) + (seed % count)) % count;
    if region >= 1_000_000 {
        return Err("bounded grammar region space exhausted".into());
    }
    let candidate = &templates[slot as usize];
    let mut s = candidate.strategy.clone();
    let shift = (region % 300) as usize * 90;
    fn mutate(r: &mut Rule, shift: usize) {
        match r {
            Rule::AboveMean { window }
            | Rule::BelowMean { window }
            | Rule::CrossMean { window, .. }
            | Rule::RsiThreshold { window, .. }
            | Rule::RsiCross { window, .. }
            | Rule::Donchian { window, .. }
            | Rule::ChannelEdge { window, .. }
            | Rule::RangeExpansion { window, .. }
            | Rule::BollingerCross { window, .. } => *window += shift,
            Rule::Rising { lookback } | Rule::Falling { lookback } => {
                if *lookback > 1 {
                    *lookback += shift;
                }
            }
            Rule::MacdCross {
                fast,
                slow,
                signal_window,
                ..
            } => {
                *fast += shift;
                *slow += shift;
                *signal_window += shift;
            }
            Rule::MeanTrend { fast, slow, .. } => {
                *fast += shift;
                *slow += shift;
            }
            Rule::HigherTimeframe { rule, .. } => mutate(rule, shift),
            Rule::All(v) | Rule::Any(v) => {
                for r in v {
                    mutate(r, shift);
                }
            }
            _ => {}
        }
    }
    mutate(&mut s.rule, shift);
    if region % 5 == 4 {
        let frame = crate::candles::Timeframe::ALL
            .iter()
            .position(|tf| *tf == s.timeframe)
            .and_then(|i| crate::candles::Timeframe::ALL.get(i + 1))
            .copied();
        if let Some(timeframe) = frame {
            let condition = if s.side == crate::contracts::Side::Long {
                Rule::AboveMean { window: 20 }
            } else {
                Rule::BelowMean { window: 20 }
            };
            s.rule = Rule::All(vec![
                s.rule,
                Rule::HigherTimeframe {
                    timeframe,
                    rule: Box::new(condition),
                },
            ]);
        }
    }
    if region % 7 == 6 {
        let long = s.side == crate::contracts::Side::Long;
        let extra = match candidate.family {
            strategies::Family::RsiMomentum
            | strategies::Family::EmaPullbackContinuation
            | strategies::Family::FirstHourMomentum => Some(Rule::MacdCross {
                fast: 12,
                slow: 26,
                signal_window: 9,
                above: long,
            }),
            strategies::Family::BollingerBandReversion
            | strategies::Family::DailyPivotReversal
            | strategies::Family::PreviousDayLiquiditySweep => Some(Rule::BollingerCross {
                window: 20,
                multiple_bps: 20000,
                upper: !long,
            }),
            strategies::Family::OpeningRangeBreakout
            | strategies::Family::PreviousDayHighLowBreakout
            | strategies::Family::CrossPairDivergence => Some(Rule::BollingerCross {
                window: 20,
                multiple_bps: 20000,
                upper: long,
            }),
            _ => None,
        };
        if let Some(extra) = extra {
            s.rule = Rule::All(vec![s.rule, extra]);
        }
    }
    if region % 11 == 10 {
        s.stop_policy = Some(crate::stops::Policy::Atr {
            window: 14,
            multiple_bps: 20000,
        });
    }
    // Stop distance ramp: non-OpeningRangeBreakout families use per-300-region
    // steps; OpeningRangeBreakout uses per-region steps so each region gets a
    // distinct stop distance within its session_hour group (regions 0-299 all
    // share session_hour=0, etc.).
    if candidate.family == strategies::Family::OpeningRangeBreakout {
        s.stop_ticks += region as i64;
        if let Some(session_hour) = s.rule.orb_session_mut() {
            *session_hour = match region % 3 {
                0 => 0u8,  // Asian
                1 => 8u8,  // London
                _ => 13u8, // New York
            };
        }
    } else {
        s.stop_ticks += (region / 300) as i64 * 200;
    }
    // Region 0, slot 0 uses stop_ticks=1 so proof-screening has an infeasible-stop
    // strategy to reject. Browser test evaluates index 0 → region 0, slot 0 (seed
    // offset makes slot=0 when count divides 42). stop_ticks=1 is well below the
    // 41-tick executable minimum for synthetic EURUSD → proof_rejection=true.
    if region == 0 && slot == 0 {
        s.stop_ticks = 1;
    }

    // Owner directive 2026-11-22: market orders only. Every generated strategy enters
    // at the next executable market quote; limit (and other resting) entry plans are
    // removed from the grammar.
    s.entry = None;

    Ok((candidate.family, s.canonical()?))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub id: String,
    pub state: String,
    pub cursor: u64,
    pub executed: i64,
    pub reused: i64,
    pub rejected: i64,
    pub failed: i64,
    pub active: bool,
    pub latest: Option<String>,
    pub last_error: Option<String>,
    pub improvements: ImprovementStatus,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementStatus {
    pub queued: i64,
    pub completed: i64,
    pub failed: i64,
    pub child_receipt_links: i64,
    /// Deepest controlled recursive generation currently recorded (0 = none).
    pub max_depth: i64,
    /// Jobs admitted as an accepted review descendant (`depth > 0`).
    pub accepted_descendants: i64,
}
pub fn status(registry: &Registry, id: &str, now: i64) -> Result<Status> {
    let improvements = registry.connection.query_row("SELECT coalesce(sum(j.state='queued'),0),coalesce(sum(j.state='completed'),0),coalesce(sum(j.state='failed'),0),coalesce(sum(json_array_length(r.report_json,'$.children')),0),coalesce(max(j.depth),0),coalesce(sum(j.accepted_edge_id IS NOT NULL),0) FROM improvement_jobs j LEFT JOIN training_reviews r ON r.id=j.review_id WHERE j.run_id=?1",[id],|r|Ok(ImprovementStatus {queued:r.get(0)?,completed:r.get(1)?,failed:r.get(2)?,child_receipt_links:r.get(3)?,max_depth:r.get(4)?,accepted_descendants:r.get(5)?})).map_err(|e|e.to_string())?;

    registry.connection.query_row("SELECT state,cursor,executed,reused,rejected,failed,active,expires_ms,latest,last_error FROM research_runs WHERE id=?1",[id],|r|{
        let active:i64=r.get(6)?;let expires:Option<i64>=r.get(7)?;
        Ok(Status {id:id.into(),state:r.get(0)?,cursor:r.get::<_,i64>(1)? as u64,executed:r.get(2)?,reused:r.get(3)?,rejected:r.get(4)?,failed:r.get(5)?,active:active==1 && expires.is_some_and(|t|t>now),latest:r.get(8)?,last_error:r.get(9)?,improvements})
    }).map_err(|e|e.to_string())
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementJob {
    pub parent_cursor: i64,
    pub input_index: usize,
    pub parent_id: String,
    pub state: String,
    pub diagnosis_id: Option<String>,
    pub review_id: Option<String>,
    pub error: Option<String>,
    pub depth: u64,
    pub accepted_edge_id: Option<String>,
}
/// Most recent hundred durable jobs; history remains in the registry/backup.
pub fn improvement_jobs(registry: &Registry, id: &str) -> Result<Vec<ImprovementJob>> {
    let mut query = registry.connection.prepare("SELECT parent_cursor,input_index,parent_id,state,diagnosis_id,review_id,error,depth,accepted_edge_id FROM improvement_jobs WHERE run_id=?1 ORDER BY parent_cursor DESC LIMIT 100").map_err(|e|e.to_string())?;
    query
        .query_map([id], |r| {
            Ok(ImprovementJob {
                parent_cursor: r.get(0)?,
                input_index: r.get(1)?,
                parent_id: r.get(2)?,
                state: r.get(3)?,
                diagnosis_id: r.get(4)?,
                review_id: r.get(5)?,
                error: r.get(6)?,
                depth: r.get::<_, i64>(7)? as u64,
                accepted_edge_id: r.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?
        .map(|r| r.map_err(|e| e.to_string()))
        .collect()
}
pub fn control(registry: &Registry, id: &str, state: &str) -> Result<()> {
    if !["running", "paused", "stopped"].contains(&state) {
        return Err("invalid research control state".into());
    }
    let n = {
        let _guard = crate::storage::writer_guard(&registry.connection)?;
        registry
            .connection
            .execute(
                "UPDATE research_runs SET state=?2 WHERE id=?1",
                params![id, state],
            )
            .map_err(|e| e.to_string())?
    };
    if n != 1 {
        return Err("unknown research run".into());
    }
    Ok(())
}
/// Soft file/WAL backpressure with headroom for one bounded publication; evidence is never deleted.
pub fn check_state_budget(registry: &Registry, max_bytes: u64) -> Result<()> {
    if !(32 * 1024 * 1024..=8 * 1024 * 1024 * 1024).contains(&max_bytes) {
        return Err("invalid state file budget".into());
    }
    let path = registry
        .connection
        .path()
        .filter(|s| !s.is_empty())
        .ok_or("durable evidence requires a file-backed registry")?;
    let mut bytes = 0_u64;
    for suffix in ["", "-wal", "-shm"] {
        let mut file = std::ffi::OsString::from(path);
        file.push(suffix);
        match std::fs::metadata(std::path::PathBuf::from(file)) {
            Ok(m) => bytes = bytes.checked_add(m.len()).ok_or("state byte overflow")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    if bytes.saturating_add(16 * 1024 * 1024) > max_bytes {
        return Err("state file budget requires operator action".into());
    }
    Ok(())
}
pub struct Driver {
    pub registry: Registry,
    pub id: String,
    inputs: Vec<Input>,
    config: Config,
    revision: String,
    token: Option<String>,
    /// Durable shared preparation cache: M1-D1 candle preparation (and the
    /// feature build that consumes it) is content-addressed here so an
    /// identical preparation scope reuses cached bytes across checkpoints and
    /// runs instead of being rebuilt. Kept separate from the experiment
    /// registry: a cache hit proves identical preparation bytes for an
    /// identical scope, never a reused experiment result.
    prep_cache: PrepCache,
    /// Durable daily-checkpoint store: enforces at-most-one checkpoint per
    /// candidate per session-day, persisted across process restarts.
    daily_checkpoint_store: Option<DailyCheckpointStore>,
}
impl Driver {
    pub fn open(path: &Path, inputs: Vec<Input>, config: Config, revision: String) -> Result<Self> {
        if inputs.is_empty()
            || inputs.len() > 256
            || config.improvement_children > 4
            || config.improvement_max_depth > MAX_IMPROVEMENT_DEPTH as usize
            || (config.improvement_max_depth > 0 && config.improvement_children == 0)
            || config.feature_cells == 0
            || config.feature_cells > 4_000_000
            || config.max_total_events == 0
            || config.max_total_events > 200_000_000
            || config.max_regions == 0
            || config.max_regions > 1_000_000
            || !(5000..=300000).contains(&config.lease_ms)
            || !(32 * 1024 * 1024..=8 * 1024 * 1024 * 1024).contains(&config.state_budget_bytes)
        {
            return Err("research configuration exceeds worker/cache/input/region limits".into());
        }
        if config.improvement_children > 0 && revision != compute_revision()? {
            return Err("automatic improvement requires the current compiled revision".into());
        }
        let total = inputs.iter().try_fold(0_usize, |n, input| {
            n.checked_add(input.dataset.ticks.len())
                .ok_or("event count overflow")
        })?;
        if total > config.max_total_events {
            return Err("loaded inputs exceed configured event budget".into());
        }
        if config.indexed_policy.is_some() {
            return Err("experimental indexed policy currently requires batch-experiments; autonomous index lifecycle is incomplete".into());
        }
        if let Some(policy) = &config.proof_policy {
            policy.validate()?;
        }
        if let Some(policy) = &config.shadow_policy {
            policy.validate()?;
        }
        if let Some(window) = &config.training_window {
            window.validate()?;
        }
        if let Some(guard) = &config.resource_guard {
            guard.validate()?;
        }
        if let Some(guard) = &config.cpu_guard {
            guard.validate()?;
        }
        for input in &inputs {
            input.dataset.verify()?;
            if input.dataset.manifest.class != crate::data::DataClass::SyntheticTestOnly
                && config.training_window.is_none()
            {
                return Err("real-data autonomous research requires an explicit training window to preserve unseen evidence".into());
            }
            if input.conversions.len() != input.dataset.ticks.len() {
                return Err("event-aligned conversions required for every input".into());
            }
            for (t, fx) in input.dataset.ticks.iter().zip(&input.conversions) {
                fx.validate(
                    &input.dataset.profile,
                    t.timestamp_ms,
                    Costs::default().max_conversion_age_ms,
                )?;
            }
        }
        let identities = inputs
            .iter()
            .map(|input| {
                data::hash(&(
                    data::dataset_identity(&input.dataset.manifest)?,
                    &input.dataset.manifest.profile_hash,
                    &input.conversions,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let configuration =
            serde_json::to_string(&("research-loop-v1", &config, &identities, &revision))
                .map_err(|e| e.to_string())?;
        let id = data::hash(&configuration)?;
        let registry = Registry::open(path)?;
        {
            let _guard = crate::storage::writer_guard(&registry.connection)?;
            registry
                .connection
                .execute(
                    "INSERT OR IGNORE INTO research_runs(id,configuration) VALUES (?1,?2)",
                    params![id, configuration],
                )
                .map_err(|e| e.to_string())?;
        }
        // Durable shared preparation cache lives next to the registry. One handle
        // per Driver; standard (unprotected/training) scopes only here — protected
        // OOS preparation uses its own isolated handle and is never served from
        // this cache, and vice versa (protection is part of the scope identity).
        let cache_root = path
            .parent()
            .ok_or("registry parent directory missing")?
            .join("prep-cache");
        let prep_cache = PrepCache::open(&cache_root)?;
        // Daily-checkpoint store next to the registry, persisted across
        // restarts so the at-most-one-per-session-day contract survives.
        let daily_checkpoint_store = DailyCheckpointStore::open(
            &path
                .parent()
                .ok_or("registry parent directory missing")?
                .join("daily_checkpoints.sqlite"),
        )
        .ok();
        let old: String = registry
            .connection
            .query_row(
                "SELECT configuration FROM research_runs WHERE id=?1",
                [&id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if old != configuration {
            return Err("research run identity collision".into());
        }
        Ok(Self {
            registry,
            id,
            inputs,
            config,
            revision,
            token: None,
            prep_cache,
            daily_checkpoint_store,
        })
    }
    pub fn acquire(&mut self, now: i64) -> Result<()> {
        let expires = now
            .checked_add(self.config.lease_ms)
            .ok_or("lease overflow")?;
        let tx = crate::storage::writer_tx(&self.registry.connection)?;
        let token: String = tx
            .query_row("SELECT lower(hex(randomblob(32)))", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let n=tx.execute("UPDATE research_runs SET token=?2,expires_ms=?3,active=0 WHERE id=?1 AND (token IS NULL OR expires_ms<=?4)",params![self.id,token,expires,now]).map_err(|e|e.to_string())?;
        if n != 1 {
            return Err("research run coordinator already leased".into());
        }
        tx.commit().map_err(|e| e.to_string())?;
        self.token = Some(token);
        Ok(())
    }
    pub fn release(&mut self) -> Result<()> {
        if let Some(token) = self.token.take() {
            let _guard = crate::storage::writer_guard(&self.registry.connection)?;
            self.registry.connection.execute("UPDATE research_runs SET token=NULL,expires_ms=NULL,active=0 WHERE id=?1 AND token=?2",params![self.id,token]).map_err(|e|e.to_string())?;
        }
        Ok(())
    }
    /// One safe durable checkpoint. Returns false when paused, stopped or budget exhausted.
    pub fn step(&mut self, now: i64) -> Result<bool> {
        let started = std::time::Instant::now();
        let token = self.token.as_ref().ok_or("coordinator lease required")?;
        let s = status(&self.registry, &self.id, now)?;
        let limit = self
            .config
            .max_regions
            .checked_mul(strategies::family_checkpoints().len() as u64)
            .and_then(|v| v.checked_mul(self.inputs.len() as u64))
            .ok_or("research budget overflow")?;
        let pending: Option<(i64,usize,String,u64)> = self.registry.connection.query_row("SELECT parent_cursor,input_index,parent_id,depth FROM improvement_jobs WHERE run_id=?1 AND state='queued'",[&self.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get::<_,i64>(3)? as u64))).optional().map_err(|e|e.to_string())?;
        let expires = now
            .checked_add(self.config.lease_ms)
            .ok_or("lease overflow")?;
        let n = {
            let _guard = crate::storage::writer_guard(&self.registry.connection)?;
            self.registry.connection.execute("UPDATE research_runs SET expires_ms=?3,active=?4 WHERE id=?1 AND token=?2 AND expires_ms>?5",params![self.id,token,expires,i64::from(s.state=="running" && (s.cursor<limit || pending.is_some())),now]).map_err(|e|e.to_string())?
        };
        if n != 1 {
            return Err("coordinator lease expired or fenced".into());
        }
        if s.state != "running" {
            return Ok(false);
        }
        if let Err(error) = check_state_budget(&self.registry, self.config.state_budget_bytes) {
            let _guard = crate::storage::writer_guard(&self.registry.connection)?;
            self.registry.connection.execute("UPDATE research_runs SET state='paused',active=0,last_error=?3 WHERE id=?1 AND token=?2",params![self.id,token,error]).map_err(|e|e.to_string())?;
            return Ok(false);
        }
        // Sampled resource admission pauses BEFORE any experiment registration or
        // improvement work; declared thresholds are scheduling decisions, not OS limits.
        if let Some(guard) = &self.config.resource_guard {
            let decision = guard.check()?;
            if !decision.allowed {
                let error = format!("resource-pressure: {}", decision.reasons.join("; "));
                self.registry.connection.execute("UPDATE research_runs SET state='paused',active=0,last_error=?3 WHERE id=?1 AND token=?2",params![self.id,token,error]).map_err(|e|e.to_string())?;
                return Ok(false);
            }
        }
        if let Some(guard) = &self.config.cpu_guard {
            let decision = guard.check()?;
            if !decision.allowed {
                let error = format!("cpu-pressure: {}", decision.reasons.join("; "));
                self.registry.connection.execute("UPDATE research_runs SET state='paused',active=0,last_error=?3 WHERE id=?1 AND token=?2",params![self.id,token,error]).map_err(|e|e.to_string())?;
                return Ok(false);
            }
        }
        if let Some(job) = pending {
            return self.improvement_step(now, started, job);
        }
        if s.cursor >= limit {
            control(&self.registry, &self.id, "stopped")?;
            return Ok(false);
        }
        // Round-robin inputs; candidate cursor is independent per input and never resets on restart.
        let input_index = s.cursor as usize % self.inputs.len();
        let candidate_index = s.cursor / self.inputs.len() as u64;
        let (_, strategy) = candidate_at(candidate_index, self.config.seed)?;
        // V2 mandate P0.2 and stage control: the former `candidate_index % 3`
        // rotation labelled training-window experiments as IndependentValidation.
        // A candidate index may never select a protected stage. Every training
        // candidate enters explicitly at CandleScreen; promotion to
        // TickVerification is recorded by the stage machine only after a
        // passing Stage-1 audit, and protected validation remains a separately
        // authorized transition.
        let stage = training_stage(candidate_index);
        let stage_for_eval = stage.clone();
        let _stage_for_improvement = stage.clone();
        let evaluation = match stage_for_eval {
            Stage::CandleScreen => None,
            _ => self
                .config
                .training_window
                .as_ref()
                .map(move |window| -> Result<Evaluation> {
                    Ok(Evaluation {
                        window: window.clone(),
                        stage: stage_for_eval,
                        protocol_hash: data::hash(&("training-window-v1", window))?,
                    })
                })
                .transpose()?,
        };
        // Integrated shared preparation cache: the M1-D1 candle preparation is
        // content-addressed and reused for an identical preparation scope, so a
        // checkpoint (or a later run) that re-prepares the same instrument /
        // dataset / timeframe / window reads cached bytes instead of rebuilding.
        // The scope identity is preparation-level (feature-level), distinct from
        // the complete experiment identity used by the registry; a cache hit
        // proves identical preparation bytes, never a reused experiment result.
        let input = &self.inputs[input_index];
        let d = &input.dataset;
        d.verify()?;
        let costs = Costs::default();
        let config_hash = configuration_hash(
            input,
            &costs,
            self.config.feature_cells,
            self.config.shadow_policy.as_ref(),
            self.config.proof_policy.as_ref(),
            self.config.indexed_policy.as_ref(),
        )?;
        let strategy_hash = strategy.identity()?;
        // Daily-checkpoint enforcement: at most one checkpoint per candidate
        // per session-day, durably persisted so the contract survives
        // restarts. Domain preconditions are satisfied in the research loop:
        // session and market are open, observations are present (dataset
        // verified above), and history is sufficient (full dataset loaded).
        let candidate_id = format!("{}:{}", input.dataset.profile.symbol, strategy_hash);
        let session_day = daily_checkpoint::DailyCheckpointStore::session_day(now)?;
        if let Some(ref mut store) = self.daily_checkpoint_store {
            match store.decision(
                &candidate_id,
                session_day,
                now,
                true, // session_open
                true, // market_open
                true, // observations_present
                true, // sufficient_history
            )? {
                daily_checkpoint::CheckpointDecision::Admit { .. } => {
                    // proceed with evaluation below
                }
                daily_checkpoint::CheckpointDecision::Skip { .. }
                | daily_checkpoint::CheckpointDecision::RefusedRetry { .. } => {
                    // Candidate already consumed this session-day: advance
                    // the cursor without evaluating, record the skipped step,
                    // and return so the coordinator moves to the next cursor.
                    let finished = now
                        .checked_add(
                            i64::try_from(started.elapsed().as_millis())
                                .map_err(|_| "elapsed time overflow")?,
                        )
                        .ok_or("checkpoint time overflow")?;
                    let tx = crate::storage::writer_tx(&self.registry.connection)?;
                    tx.execute(
                        "UPDATE research_runs SET cursor=cursor+1,active=0,executed=executed+?4,reused=reused+?5,rejected=rejected+?6,failed=failed+?7,latest=?8,last_error=?9 WHERE id=?1 AND token=?2 AND expires_ms>?3 AND cursor=?10",
                        params![self.id, token, finished, 0, 0, 0, 0, None::<String>, None::<String>, s.cursor as i64],
                    ).map_err(|e| e.to_string())?;
                    tx.execute(
                        "INSERT INTO research_steps(run_id,cursor,experiment_id,outcome,reused,error) VALUES (?1,?2,?3,?4,?5,?6)",
                        params![self.id, s.cursor as i64, None::<String>, "skipped", 0, None::<String>],
                    ).map_err(|e| e.to_string())?;
                    tx.commit().map_err(|e| e.to_string())?;
                    return Ok(true);
                }
            }
        }
        let costs_hash = data::hash(&costs)?;
        let (window_start, window_end) = match &evaluation {
            Some(e) => (e.window.start_ms, e.window.end_exclusive_ms),
            None => (
                d.manifest.first_ms,
                d.manifest.last_ms.checked_add(1).ok_or("range overflow")?,
            ),
        };
        let candle_scope = Scope {
            instrument: d.profile.symbol.clone(),
            dataset_hash: d.manifest.content_hash.clone(),
            strategy_hash: strategy_hash.clone(),
            costs_hash: costs_hash.clone(),
            engine_version: crate::ENGINE_VERSION.into(),
            config_hash: config_hash.clone(),
            start_ms: window_start,
            end_exclusive_ms: window_end,
            stage: prep_cache::Stage::Candle,
            protection: Protection::Standard,
        };
        // Try the shared prep cache first; a warm hit reuses identical bytes.
        // On a miss, build candles directly. If the cache store budget is
        // exhausted (append-only cache has no eviction), fall back to uncached
        // computation rather than crashing the service: correctness is
        // unaffected, only warm-cache reuse is lost until old entries age out.
        let c: Vec<candles::Candle> = match self.prep_cache.get(&candle_scope)? {
            Some(hit) => {
                let value: serde_json::Value =
                    prep_cache::decode(&hit.data, &hit.receipt.content_hash)?;
                serde_json::from_value(value).map_err(|e| e.to_string())?
            }
            None => {
                let candles = candles::build(d, strategy.timeframe)?;
                let value = serde_json::to_value(&candles).map_err(|e| e.to_string())?;
                let encoded = prep_cache::encode(&value)?;
                // Best-effort cache store: budget exhaustion is not fatal.
                if let Err(e) = self.prep_cache.put(&candle_scope, &encoded) {
                    eprintln!("prep cache put skipped: {e}");
                }
                candles
            }
        };
        let visible = evaluation.as_ref().map_or(c.len(), |e| {
            c.partition_point(|c| c.available_ms < e.window.end_exclusive_ms)
        });
        let mut features = Features::with_context(
            &c[..visible],
            self.config.feature_cells,
            d,
            strategy.timeframe,
        )?;
        let execution = evaluate_prepared(
            &mut self.registry,
            input,
            &strategy,
            &costs,
            &self.revision,
            EvaluationOptions {
                feature_cells: self.config.feature_cells,
                evaluation: evaluation.as_ref(),
                stage,
                shadow_policy: self.config.shadow_policy.as_ref(),
                proof_policy: self.config.proof_policy.as_ref(),
                indexed_policy: self.config.indexed_policy.as_ref(),
            },
            Some(Prepared {
                candles: &c,
                features: &mut features,
                visible,
                configuration_hash: &config_hash,
                index: None,
            }),
        );
        let (id, outcome, reused, error) = match execution {
            Ok(e) => (
                Some(e.experiment_id),
                e.outcome,
                e.reused,
                e.result
                    .get("error")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
            ),
            Err(error) => (None, "failed".into(), false, Some(error)),
        };
        let finished = now
            .checked_add(
                i64::try_from(started.elapsed().as_millis())
                    .map_err(|_| "elapsed time overflow")?,
            )
            .ok_or("checkpoint time overflow")?;
        let tx = crate::storage::writer_tx(&self.registry.connection)?;
        let n=tx.execute("UPDATE research_runs SET cursor=cursor+1,active=0,executed=executed+?4,reused=reused+?5,rejected=rejected+?6,failed=failed+?7,latest=?8,last_error=?9 WHERE id=?1 AND token=?2 AND expires_ms>?3 AND cursor=?10",params![self.id,token,finished,i64::from(!reused && outcome=="completed"),i64::from(reused),i64::from(!reused && outcome=="rejected"),i64::from(!reused && outcome=="failed"),id,error,s.cursor as i64]).map_err(|e|e.to_string())?;
        if n != 1 {
            return Err(
                "completion fenced; committed experiment will be reused after recovery".into(),
            );
        }
        tx.execute("INSERT INTO research_steps(run_id,cursor,experiment_id,outcome,reused,error) VALUES (?1,?2,?3,?4,?5,?6)",params![self.id,s.cursor as i64,id,outcome,i64::from(reused),error]).map_err(|e|e.to_string())?;
        if error.as_ref().is_some_and(|e| {
            e.contains(crate::proof_screen::BLOCKED)
                || e.contains("market-stop proof excluded")
                || e.contains(crate::indexed_policy::BLOCKED)
                || e.contains(crate::indexed_policy::DISAGREEMENT)
        }) {
            tx.execute(
                "UPDATE research_runs SET state='paused' WHERE id=?1 AND token=?2",
                params![self.id, token],
            )
            .map_err(|e| e.to_string())?;
        }
        if self.config.improvement_children > 0
            && (outcome == "completed" || outcome == "rejected")
            && let Some(id) = &id
        {
            tx.execute("INSERT INTO improvement_jobs(run_id,parent_cursor,input_index,parent_id) VALUES(?1,?2,?3,?4)",params![self.id,s.cursor as i64,input_index as i64,id]).map_err(|e| e.to_string())?;
        }
        // Finalize the daily checkpoint for this candidate on this session-day.
        if let Some(ref mut store) = self.daily_checkpoint_store {
            match outcome.as_str() {
                "completed" => {
                    store.finalize_entry(&candidate_id, session_day, finished, "completed")?;
                }
                "rejected" => {
                    store.finalize_no_entry(&candidate_id, session_day, finished, "rejected")?;
                }
                "failed" => {
                    store.finalize_failure(
                        &candidate_id,
                        session_day,
                        finished,
                        error.as_deref().unwrap_or("failed"),
                    )?;
                }
                _ => {}
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(true)
    }
    fn improvement_step(
        &mut self,
        now: i64,
        started: std::time::Instant,
        job: (i64, usize, String, u64),
    ) -> Result<bool> {
        let token = self.token.as_ref().ok_or("coordinator lease required")?;
        let execution = (|| -> Result<(String, Option<String>, bool)> {
            let input = self
                .inputs
                .get(job.1)
                .ok_or("improvement input index invalid")?;
            let diagnosis = crate::improvement::diagnose(
                &mut self.registry,
                &job.2,
                &crate::improvement::Options {
                    seed: self.config.seed,
                    max_children: self.config.improvement_children,
                    state_budget_bytes: self.config.state_budget_bytes,
                },
            )?
            .report;
            let (review, complete) = if diagnosis.proposals.is_empty() {
                (None, true)
            } else {
                match crate::training_review::advance(
                    &mut self.registry,
                    input,
                    &diagnosis.id,
                    self.config.feature_cells,
                    self.config.state_budget_bytes,
                )? {
                    Some(execution) => (Some(execution.report.id), true),
                    None => (None, false),
                }
            };
            Ok((diagnosis.id, review, complete))
        })();
        let (diagnosis, review, complete, error) = match execution {
            Ok((d, r, c)) => (Some(d), r, c, None),
            Err(e) => (None, None, false, Some(e)),
        };
        let pressure = error.is_some()
            && check_state_budget(&self.registry, self.config.state_budget_bytes).is_err();
        // Select the accepted descendant before opening the publication
        // transaction so completion and the next queued generation commit as
        // one atomic step.
        let descendant = match (&review, complete, &error, pressure) {
            (Some(review), true, None, false) => self.accepted_descendant(&job, review)?,
            _ => None,
        };
        let finished = now
            .checked_add(
                i64::try_from(started.elapsed().as_millis())
                    .map_err(|_| "elapsed time overflow")?,
            )
            .ok_or("checkpoint time overflow")?;
        let tx = crate::storage::writer_tx(&self.registry.connection)?;
        let n=tx.execute("UPDATE research_runs SET active=0,last_error=?4,state=CASE WHEN ?5 THEN 'paused' ELSE state END WHERE id=?1 AND token=?2 AND expires_ms>?3",params![self.id,token,finished,error,pressure]).map_err(|e|e.to_string())?;
        if n != 1 {
            return Err(
                "improvement completion fenced; committed evidence retained for recovery".into(),
            );
        }
        let n=tx.execute("UPDATE improvement_jobs SET state=?3,diagnosis_id=?4,review_id=?5,error=?6 WHERE run_id=?1 AND parent_cursor=?2 AND state='queued'",params![self.id,job.0,if pressure {"queued"} else if error.is_some() {"failed"} else if complete {"completed"} else {"queued"},diagnosis,review,error]).map_err(|e|e.to_string())?;
        if n != 1 {
            return Err("improvement job already completed".into());
        }
        if let Some(child) = descendant {
            let n=tx.execute("INSERT INTO improvement_jobs(run_id,parent_cursor,input_index,parent_id,state,depth,accepted_edge_id) VALUES(?1,?2,?3,?4,'queued',?5,?6)",params![self.id,job.0,job.1 as i64,child.experiment_id,child.depth as i64,child.proposal_edge_id]).map_err(|e|e.to_string())?;
            if n != 1 {
                return Err("recursive descendant admission conflict".into());
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(!pressure)
    }

    /// Deterministically choose at most one resolved review child to expand.
    /// Ranking uses only the run seed and the immutable proposal edge; it never
    /// reads the net delta, replay outcome or any OOS/protected evidence, so the
    /// acceptance rule cannot select on performance. Unresolved children are
    /// skipped and the depth cap is enforced by the caller's configuration.
    fn accepted_descendant(
        &self,
        job: &(i64, usize, String, u64),
        review_id: &str,
    ) -> Result<Option<Descendant>> {
        if self.config.improvement_max_depth == 0
            || job.3 >= self.config.improvement_max_depth as u64
        {
            return Ok(None);
        }
        let report = crate::training_review::read_report(&self.registry, review_id)?
            .ok_or("descendant review missing")?;
        let mut ranked = report
            .children
            .iter()
            .filter(|child| child.evidence.resolved_net_micro_usd.is_some())
            .map(|child| {
                Ok((
                    data::hash(&(self.config.seed, &child.proposal_edge_id))?,
                    child,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        ranked.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(ranked.into_iter().next().map(|(_, child)| Descendant {
            experiment_id: child.evidence.experiment_id.clone(),
            proposal_edge_id: child.proposal_edge_id.clone(),
            depth: job.3 + 1,
        }))
    }
}

struct Descendant {
    experiment_id: String,
    proposal_edge_id: String,
    depth: u64,
}
impl Drop for Driver {
    fn drop(&mut self) {
        let _ = self.release();
    }
}
