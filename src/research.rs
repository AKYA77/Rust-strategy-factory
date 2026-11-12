//! One bounded worker and one in-flight task; persistent control/cursors and fenced leases.
use crate::{
    Result, candles,
    contracts::Conversion,
    data::{self, Dataset},
    storage::{Claim, ExperimentKey, Outcome, Registry, Stage},
    strategies::{self, EntryPlan, Features, Rule, Strategy},
    tick_engine::{self, Costs},
};
use rusqlite::{TransactionBehavior, params};
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
        }
    }
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
pub fn evaluate(
    registry: &mut Registry,
    input: &Input,
    strategy: &Strategy,
    costs: &Costs,
    revision: &str,
    feature_cells: usize,
) -> Result<Execution> {
    evaluate_internal(
        registry,
        input,
        strategy,
        costs,
        revision,
        feature_cells,
        None,
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
    evaluation.window.validate()?;
    if evaluation.stage == Stage::CandleScreen {
        return Err(
            "tick evaluation requires tick verification or independent-validation stage".into(),
        );
    }
    evaluate_internal(
        registry,
        input,
        strategy,
        costs,
        revision,
        feature_cells,
        Some(evaluation),
    )
}
fn evaluate_internal(
    registry: &mut Registry,
    input: &Input,
    strategy: &Strategy,
    costs: &Costs,
    revision: &str,
    feature_cells: usize,
    evaluation: Option<&Evaluation>,
) -> Result<Execution> {
    let d = &input.dataset;
    d.verify()?;
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
        fill_policy: "full-l1-assumption-target-at-limit-preflight-v2".into(),
        profile_hash: d.manifest.profile_hash.clone(),
        engine_version: crate::ENGINE_VERSION.into(),
        protocol_hash: match evaluation {
            Some(e) => e.protocol_hash.clone(),
            None => data::hash(&"training-only-conservative-signals-v1")?,
        },
        code_revision: revision.into(),
        configuration_hash: data::hash(&(
            feature_cells,
            costs,
            data::hash(&input.conversions)?,
            data::dataset_identity(&d.manifest)?,
        ))?,
        stage: evaluation.map_or(Stage::TickVerification, |e| e.stage.clone()),
    };
    let id = registry.register(&key)?;
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
                let c = candles::build(d, strategy.timeframe)?;
                let mut features = Features::new(&c, feature_cells)?;
                let mut screen = strategies::screen(&mut features, strategy)?;
                let mask = features.signal(&strategy.rule)?;
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
                    screen.version = "conservative-window-signals-v2".into();
                }
                if !screen.requires_tick_verification {
                    return Ok(
                        serde_json::json!({"screen":screen,"tick_replay":null,"strategy":strategy,"data_manifest":d.manifest,"instrument_profile":d.profile,"cost_profile":costs,"evaluation":evaluation}),
                    );
                }
                let replay = if let Some(e) = evaluation {
                    tick_engine::replay_window(
                        d,
                        &c,
                        &mask,
                        strategy,
                        costs,
                        &input.conversions,
                        &e.window,
                    )?
                } else {
                    tick_engine::replay(d, &c, &mask, strategy, costs, &input.conversions)?
                };
                let screen_audit = crate::screen::audit_replay(d, costs, &replay)?;
                Ok(
                    serde_json::json!({"screen":screen,"screen_audit":screen_audit,"tick_replay":replay,"strategy":strategy,"data_manifest":d.manifest,"instrument_profile":d.profile,"cost_profile":costs,"evaluation":evaluation}),
                )
            };
            let (outcome, result) = match compute() {
                Ok(result) => (
                    if result["tick_replay"].is_null() {
                        Outcome::Rejected
                    } else {
                        Outcome::Completed
                    },
                    result,
                ),
                Err(error) => (Outcome::Failed, serde_json::json!({"error":error})),
            };
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
/// Bounded novel parameter regions; no cycling through already completed template ranges.
pub fn candidate_at(index: u64, seed: u64) -> Result<(strategies::Family, Strategy)> {
    static TEMPLATES: std::sync::OnceLock<Vec<strategies::Candidate>> = std::sync::OnceLock::new();
    let templates = TEMPLATES.get_or_init(strategies::family_templates);
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
            | Rule::RangeExpansion { window, .. } => *window += shift,
            Rule::Rising { lookback } | Rule::Falling { lookback } => {
                if *lookback > 1 {
                    *lookback += shift;
                }
            }
            Rule::MeanTrend { fast, slow, .. } => {
                *fast += shift;
                *slow += shift;
            }
            Rule::All(v) | Rule::Any(v) => {
                for r in v {
                    mutate(r, shift);
                }
            }
            _ => {}
        }
    }
    mutate(&mut s.rule, shift);
    s.stop_ticks += (region / 300) as i64 * 200;
    s.entry = match region % 3 {
        0 => None,
        1 => Some(EntryPlan::StopMarket {
            offset_ticks: 30,
            ttl_ms: 1800000,
            cancel_on_signal_false: true,
        }),
        _ => Some(EntryPlan::Limit {
            offset_ticks: 30,
            ttl_ms: 1800000,
            cancel_on_signal_false: true,
        }),
    };
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
}
pub fn status(registry: &Registry, id: &str, now: i64) -> Result<Status> {
    registry.connection.query_row("SELECT state,cursor,executed,reused,rejected,failed,active,expires_ms,latest,last_error FROM research_runs WHERE id=?1",[id],|r|{
        let active:i64=r.get(6)?;let expires:Option<i64>=r.get(7)?;
        Ok(Status {id:id.into(),state:r.get(0)?,cursor:r.get::<_,i64>(1)? as u64,executed:r.get(2)?,reused:r.get(3)?,rejected:r.get(4)?,failed:r.get(5)?,active:active==1 && expires.is_some_and(|t|t>now),latest:r.get(8)?,last_error:r.get(9)?})
    }).map_err(|e|e.to_string())
}
pub fn control(registry: &Registry, id: &str, state: &str) -> Result<()> {
    if !["running", "paused", "stopped"].contains(&state) {
        return Err("invalid research control state".into());
    }
    let n = registry
        .connection
        .execute(
            "UPDATE research_runs SET state=?2 WHERE id=?1",
            params![id, state],
        )
        .map_err(|e| e.to_string())?;
    if n != 1 {
        return Err("unknown research run".into());
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
    database_path: std::path::PathBuf,
}
impl Driver {
    pub fn open(path: &Path, inputs: Vec<Input>, config: Config, revision: String) -> Result<Self> {
        if inputs.is_empty()
            || inputs.len() > 8
            || config.feature_cells == 0
            || config.feature_cells > 4_000_000
            || config.max_total_events == 0
            || config.max_total_events > 2_000_000
            || config.max_regions == 0
            || config.max_regions > 1_000_000
            || !(5000..=300000).contains(&config.lease_ms)
            || !(32 * 1024 * 1024..=8 * 1024 * 1024 * 1024).contains(&config.state_budget_bytes)
        {
            return Err("research configuration exceeds worker/cache/input/region limits".into());
        }
        let total = inputs.iter().try_fold(0_usize, |n, input| {
            n.checked_add(input.dataset.ticks.len())
                .ok_or("event count overflow")
        })?;
        if total > config.max_total_events {
            return Err("loaded inputs exceed configured event budget".into());
        }
        if let Some(window) = &config.training_window {
            window.validate()?;
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
        registry
            .connection
            .execute(
                "INSERT OR IGNORE INTO research_runs(id,configuration) VALUES (?1,?2)",
                params![id, configuration],
            )
            .map_err(|e| e.to_string())?;
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
            database_path: path.to_path_buf(),
        })
    }
    pub fn acquire(&mut self, now: i64) -> Result<()> {
        let expires = now
            .checked_add(self.config.lease_ms)
            .ok_or("lease overflow")?;
        let tx = self
            .registry
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
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
            .checked_mul(strategies::family_templates().len() as u64)
            .and_then(|v| v.checked_mul(self.inputs.len() as u64))
            .ok_or("research budget overflow")?;
        let expires = now
            .checked_add(self.config.lease_ms)
            .ok_or("lease overflow")?;
        let n=self.registry.connection.execute("UPDATE research_runs SET expires_ms=?3,active=?4 WHERE id=?1 AND token=?2 AND expires_ms>?5",params![self.id,token,expires,i64::from(s.state=="running" && s.cursor<limit),now]).map_err(|e|e.to_string())?;
        if n != 1 {
            return Err("coordinator lease expired or fenced".into());
        }
        if s.state != "running" {
            return Ok(false);
        }
        let mut state_bytes = 0_u64;
        for suffix in ["", "-wal", "-shm"] {
            let mut path = self.database_path.as_os_str().to_os_string();
            path.push(suffix);
            match std::fs::metadata(std::path::PathBuf::from(path)) {
                Ok(metadata) => {
                    state_bytes = state_bytes
                        .checked_add(metadata.len())
                        .ok_or("state byte count overflow")?
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        // Reserve headroom for one bounded result and WAL publication. Never delete evidence.
        if state_bytes.saturating_add(16 * 1024 * 1024) > self.config.state_budget_bytes {
            self.registry.connection.execute("UPDATE research_runs SET state='paused',active=0,last_error='state file budget requires operator action' WHERE id=?1 AND token=?2",params![self.id,token]).map_err(|e|e.to_string())?;
            return Ok(false);
        }

        if s.cursor >= limit {
            control(&self.registry, &self.id, "stopped")?;
            return Ok(false);
        }
        // Round-robin inputs; candidate cursor is independent per input and never resets on restart.
        let input_index = s.cursor as usize % self.inputs.len();
        let candidate_index = s.cursor / self.inputs.len() as u64;
        let (_, strategy) = candidate_at(candidate_index, self.config.seed)?;
        let execution = if let Some(window) = &self.config.training_window {
            evaluate_window(
                &mut self.registry,
                &self.inputs[input_index],
                &strategy,
                &Costs::default(),
                &self.revision,
                self.config.feature_cells,
                &Evaluation {
                    window: window.clone(),
                    stage: Stage::TickVerification,
                    protocol_hash: data::hash(&("training-window-v1", window))?,
                },
            )
        } else {
            evaluate(
                &mut self.registry,
                &self.inputs[input_index],
                &strategy,
                &Costs::default(),
                &self.revision,
                self.config.feature_cells,
            )
        };
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
        let tx = self
            .registry
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let n=tx.execute("UPDATE research_runs SET cursor=cursor+1,active=0,executed=executed+?4,reused=reused+?5,rejected=rejected+?6,failed=failed+?7,latest=?8,last_error=?9 WHERE id=?1 AND token=?2 AND expires_ms>?3 AND cursor=?10",params![self.id,token,finished,i64::from(!reused && outcome=="completed"),i64::from(reused),i64::from(!reused && outcome=="rejected"),i64::from(!reused && outcome=="failed"),id,error,s.cursor as i64]).map_err(|e|e.to_string())?;
        if n != 1 {
            return Err(
                "completion fenced; committed experiment will be reused after recovery".into(),
            );
        }
        tx.execute("INSERT INTO research_steps(run_id,cursor,experiment_id,outcome,reused,error) VALUES (?1,?2,?3,?4,?5,?6)",params![self.id,s.cursor as i64,id,outcome,i64::from(reused),error]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(true)
    }
}
impl Drop for Driver {
    fn drop(&mut self) {
        let _ = self.release();
    }
}
