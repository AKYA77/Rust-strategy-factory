//! Immutable training-only diagnostics. Proposals are unevaluated; no qualification or OOS adaptation.
use crate::{
    Result, data, research, robustness,
    storage::{ExperimentKey, Registry, Stage},
    strategies::Strategy,
    tick_engine::{Costs, Replay},
    validation::Interval,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub const VERSION: &str = "training-proposals-v1";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub seed: u64,
    pub max_children: usize,
    pub state_budget_bytes: u64,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            seed: 42,
            max_children: 8,
            state_budget_bytes: 256 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub edge_id: String,
    pub concept_id: String,
    pub strategy: Strategy,
    pub operator: String,
    pub rationale: String,
    pub status: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub id: String,
    pub version: String,
    pub parent_id: String,
    pub parent_key: ExperimentKey,
    pub parent_result_hash: String,
    pub parent_outcome: String,
    pub parent_data_class: Option<data::DataClass>,
    pub compute_revision: String,
    pub options: Options,
    pub reasons: Vec<String>,
    pub proposals: Vec<Proposal>,
    pub status: String,
}
#[derive(Debug, Serialize)]
pub struct Execution {
    pub reused: bool,
    pub report: Report,
}
fn identity(r: &Report) -> Result<String> {
    data::hash(&(
        &r.version,
        &r.parent_id,
        &r.parent_result_hash,
        &r.compute_revision,
        &r.options,
    ))
}
fn edge(r: &Report, concept: &str, operator: &str) -> Result<String> {
    data::hash(&(&r.id, concept, operator, r.options.seed))
}
fn authorize(key: &ExperimentKey) -> Result<()> {
    let window = Interval {
        start_ms: key.start_ms,
        end_exclusive_ms: key.end_exclusive_ms,
    };
    if key.stage != Stage::TickVerification
        || (key.protocol_hash != data::hash(&"training-only-conservative-signals-v1")?
            && key.protocol_hash != data::hash(&("training-window-v1", &window))?)
    {
        return Err("adaptation requires explicit training protocol; scientific/OOS/final evidence is forbidden".into());
    }
    if key.engine_version != crate::ENGINE_VERSION
        || key.indicator_version != crate::INDICATOR_VERSION
        || key.signal_version != crate::SIGNAL_VERSION
    {
        return Err("new diagnostics require current model versions".into());
    }
    Ok(())
}
pub fn read_report(registry: &Registry, id: &str) -> Result<Option<Report>> {
    let row: Option<(String, String, String)> = registry
        .connection
        .query_row(
            "SELECT parent_id,report_json,report_hash FROM training_diagnoses WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    row.map(|(parent, json, expected)| {
        let report: Report = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        if data::hash(&report)? != expected
            || report.id != id
            || identity(&report)? != id
            || report.parent_id != parent
            || report.options.max_children > 16
            || report.proposals.len() > report.options.max_children
        {
            return Err("training diagnosis integrity mismatch".into());
        }
        if registry.experiment_key(&parent)?.as_ref() != Some(&report.parent_key)
            || registry
                .result(&parent)?
                .ok_or("diagnosis parent result missing")?
                .result_hash
                != report.parent_result_hash
        {
            return Err("training diagnosis parent mismatch".into());
        }
        let mut seen = BTreeSet::new();
        for p in &report.proposals {
            if p.strategy.identity()? != p.concept_id
                || p.edge_id != edge(&report, &p.concept_id, &p.operator)?
                || p.status != "UNEVALUATED"
                || !seen.insert(&p.concept_id)
            {
                return Err("training proposal integrity mismatch".into());
            }
        }
        Ok(report)
    })
    .transpose()
}
pub fn diagnose(registry: &mut Registry, parent_id: &str, options: &Options) -> Result<Execution> {
    if options.max_children > 16
        || !(32 * 1024 * 1024..=8 * 1024 * 1024 * 1024).contains(&options.state_budget_bytes)
    {
        return Err("diagnosis budget out of bounds".into());
    }
    let key = registry
        .experiment_key(parent_id)?
        .ok_or("parent experiment missing")?;
    authorize(&key)?;
    let parent = registry
        .result(parent_id)?
        .ok_or("parent must have a terminal result")?;
    let mut report = Report {
        id: String::new(),
        version: VERSION.into(),
        parent_id: parent_id.into(),
        parent_key: key.clone(),
        parent_result_hash: parent.result_hash,
        parent_outcome: parent.outcome.clone(),
        parent_data_class: None,
        compute_revision: research::compute_revision()?,
        options: options.clone(),
        reasons: vec![],
        proposals: vec![],
        status: "TRAINING_ONLY_UNEVALUATED".into(),
    };
    report.id = identity(&report)?;
    if let Some(report) = read_report(registry, &report.id)? {
        return Ok(Execution {
            reused: true,
            report,
        });
    }
    research::check_state_budget(registry, options.state_budget_bytes)?;
    if parent.outcome == "failed" {
        report.reasons.push("execution_failed_no_proposals".into());
    } else {
        let strategy: Strategy = serde_json::from_value(parent.result["strategy"].clone())
            .map_err(|e| format!("parent strategy unavailable: {e}"))?;
        let costs: Costs = serde_json::from_value(parent.result["cost_profile"].clone())
            .map_err(|e| e.to_string())?;
        let profile: crate::contracts::Profile =
            serde_json::from_value(parent.result["instrument_profile"].clone())
                .map_err(|e| e.to_string())?;
        let manifest: data::Manifest =
            serde_json::from_value(parent.result["data_manifest"].clone())
                .map_err(|e| e.to_string())?;
        report.parent_data_class = Some(manifest.class);
        if profile.symbol != key.instrument
            || manifest.content_hash != key.dataset_hash
            || manifest.profile_hash != key.profile_hash
        {
            return Err("parent dataset/profile mismatch".into());
        }
        if strategy.identity()? != key.concept_hash
            || data::hash(&costs)? != key.cost_hash
            || data::hash(&profile)? != key.profile_hash
        {
            return Err("parent specification identity mismatch".into());
        }
        let mut eligible = true;
        if parent.result["tick_replay"].is_null() {
            if parent.result["screen"]["signals"].as_u64() != Some(0) {
                return Err("missing replay without proven zero signals".into());
            }
            report.reasons.push("no_signals".into());
        } else {
            let replay: Replay = serde_json::from_value(parent.result["tick_replay"].clone())
                .map_err(|e| e.to_string())?;
            if replay.strategy_id != key.concept_hash
                || replay.dataset_id != key.dataset_hash
                || replay.profile_id != key.profile_hash
                || replay.cost_id != key.cost_hash
                || replay.version != key.engine_version
            {
                return Err("parent replay identity mismatch".into());
            }
            if replay.unresolved.is_some() || replay.pending_entry.is_some() {
                eligible = false;
                report
                    .reasons
                    .push("unresolved_outcome_no_proposals".into());
            }
            if replay.trades.is_empty() {
                report.reasons.push("no_closed_trades".into());
            }
            let net = replay.trades.iter().try_fold(0i128, |sum, t| {
                sum.checked_add(t.net_micro_usd)
                    .ok_or("diagnosis net overflow")
            })?;
            if net < 0 {
                report.reasons.push("recorded_net_loss".into());
            }
            if !replay.rejections.is_empty() {
                report.reasons.push("entry_rejections".into());
            }
        }
        if eligible {
            let mut candidates = robustness::strategy_cases(&strategy, &costs)?
                .into_iter()
                .map(|c| {
                    Ok((
                        data::hash(&(options.seed, c.strategy.identity()?, &c.name))?,
                        c,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            candidates.sort_by(|a, b| a.0.cmp(&b.0));
            let mut seen = BTreeSet::from([strategy.identity()?]);
            for (_, case) in candidates {
                let concept_id = case.strategy.identity()?;
                if report.proposals.len() >= options.max_children {
                    break;
                }
                if seen.insert(concept_id.clone()) {
                    report.proposals.push(Proposal {
                        edge_id: edge(&report, &concept_id, &case.name)?,
                        concept_id,
                        strategy: case.strategy,
                        operator: case.name,
                        rationale: "Bounded training neighborhood probe; performance effect unknown".into(),
                        status: "UNEVALUATED".into(),
                    });
                }
            }
        }
    }
    if report.reasons.is_empty() {
        report
            .reasons
            .push("bounded_neighborhood_review_no_improvement_claim".into());
    }
    let json = serde_json::to_string(&report).map_err(|e| e.to_string())?;
    if json.len() > 1024 * 1024 {
        return Err("diagnosis report budget exceeded".into());
    }
    let inserted = registry.connection.execute("INSERT OR IGNORE INTO training_diagnoses(id,parent_id,report_json,report_hash) VALUES(?1,?2,?3,?4)", params![report.id, parent_id, json, data::hash(&report)?]).map_err(|e|e.to_string())?;
    let stored = read_report(registry, &report.id)?.ok_or("diagnosis publication missing")?;
    if data::hash(&stored)? != data::hash(&report)? {
        return Err("conflicting immutable diagnosis".into());
    }
    Ok(Execution {
        reused: inserted == 0,
        report: stored,
    })
}
