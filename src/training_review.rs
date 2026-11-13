//! Bounded comparable training replays of immutable proposals; descriptive evidence only.
use crate::{
    Result, data, improvement,
    research::{self, Evaluation, Input},
    storage::{ExperimentKey, Registry},
    strategies::Strategy,
    tick_engine::{Costs, Replay},
    validation::Interval,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
pub const VERSION: &str = "comparable-training-review-v1";
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub experiment_id: String,
    pub result_hash: String,
    pub outcome: String,
    pub resolved_net_micro_usd: Option<i128>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_basis: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Child {
    pub proposal_edge_id: String,
    pub evidence: Evidence,
    pub net_delta_micro_usd: Option<i128>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub id: String,
    pub version: String,
    pub diagnosis_id: String,
    pub compute_revision: String,
    pub feature_cells: usize,
    pub baseline: Evidence,
    pub children: Vec<Child>,
    pub status: String,
}
#[derive(Debug, Serialize)]
pub struct Execution {
    pub reused: bool,
    pub report: Report,
}
fn identity(diagnosis: &str, revision: &str, cells: usize, version: &str) -> Result<String> {
    data::hash(&(version, diagnosis, revision, cells))
}
fn evidence(registry: &Registry, id: &str) -> Result<Evidence> {
    let stored = registry.result(id)?.ok_or("review result missing")?;
    let proof = stored.outcome == "rejected"
        && stored.result["proof_rejection"] == true
        && crate::proof_screen::verified_rejection(
            &stored.result,
            &registry.experiment_key(id)?.ok_or("review key missing")?,
        )?
        .is_some();
    let proven_empty = stored.outcome == "rejected"
        && (stored.result["screen"]["signals"].as_u64() == Some(0) || proof);
    let net = if proven_empty {
        Some(0)
    } else if stored.outcome == "completed" {
        let replay: Replay = serde_json::from_value(stored.result["tick_replay"].clone())
            .map_err(|e| e.to_string())?;
        if replay.unresolved.is_some() || replay.pending_entry.is_some() {
            None
        } else {
            Some(replay.trades.iter().try_fold(0i128, |sum, t| {
                sum.checked_add(t.net_micro_usd)
                    .ok_or("review net overflow")
            })?)
        }
    } else {
        None
    };
    Ok(Evidence {
        experiment_id: id.into(),
        result_hash: stored.result_hash,
        outcome: stored.outcome,
        resolved_net_micro_usd: net,
        net_basis: proof
            .then(|| "zero from verified no-entry proof; no executed zero-return trade".into()),
    })
}
fn compatible(key: &ExperimentKey, parent: &ExperimentKey, concept: &str, revision: &str) -> bool {
    let mut expected = parent.clone();
    expected.concept_hash = concept.into();
    expected.code_revision = revision.into();
    key == &expected
}
fn verify_evidence(
    registry: &Registry,
    e: &Evidence,
    parent: &ExperimentKey,
    concept: &str,
    revision: &str,
) -> Result<()> {
    let key = registry
        .experiment_key(&e.experiment_id)?
        .ok_or("review experiment missing")?;
    if !compatible(&key, parent, concept, revision)
        || data::hash(&evidence(registry, &e.experiment_id)?)? != data::hash(e)?
    {
        return Err("training review evidence mismatch".into());
    }
    Ok(())
}
fn delta(child: &Evidence, baseline: &Evidence) -> Result<Option<i128>> {
    match (
        child.resolved_net_micro_usd,
        baseline.resolved_net_micro_usd,
    ) {
        (Some(c), Some(b)) => Ok(Some(c.checked_sub(b).ok_or("review delta overflow")?)),
        _ => Ok(None),
    }
}
pub fn read_report(registry: &Registry, id: &str) -> Result<Option<Report>> {
    let row: Option<(String, String, String)> = registry
        .connection
        .query_row(
            "SELECT diagnosis_id,report_json,report_hash FROM training_reviews WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    row.map(|(diagnosis, json, expected)| {
        let report: Report = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        if report.id != id
            || report.diagnosis_id != diagnosis
            || data::hash(&report)? != expected
            || identity(
                &diagnosis,
                &report.compute_revision,
                report.feature_cells,
                &report.version,
            )? != id
        {
            return Err("training review integrity mismatch".into());
        }
        let source =
            improvement::read_report(registry, &diagnosis)?.ok_or("review diagnosis missing")?;
        verify_evidence(
            registry,
            &report.baseline,
            &source.parent_key,
            &source.parent_key.concept_hash,
            &report.compute_revision,
        )?;
        if report.children.len() != source.proposals.len() {
            return Err("incomplete training review lineage".into());
        }
        for (child, proposal) in report.children.iter().zip(&source.proposals) {
            if child.proposal_edge_id != proposal.edge_id
                || child.net_delta_micro_usd != delta(&child.evidence, &report.baseline)?
            {
                return Err("training review edge/delta mismatch".into());
            }
            verify_evidence(
                registry,
                &child.evidence,
                &source.parent_key,
                &proposal.concept_id,
                &report.compute_revision,
            )?;
        }
        Ok(report)
    })
    .transpose()
}
/// Explicit bounded review; finishes all missing experiments before returning.
pub fn execute(
    registry: &mut Registry,
    input: &Input,
    diagnosis_id: &str,
    feature_cells: usize,
    state_budget_bytes: u64,
) -> Result<Execution> {
    run(
        registry,
        input,
        diagnosis_id,
        feature_cells,
        state_budget_bytes,
        false,
    )?
    .ok_or("complete review unexpectedly yielded".into())
}
/// Performs at most one missing experiment. None means committed progress; retry to finish.
pub fn advance(
    registry: &mut Registry,
    input: &Input,
    diagnosis_id: &str,
    feature_cells: usize,
    state_budget_bytes: u64,
) -> Result<Option<Execution>> {
    run(
        registry,
        input,
        diagnosis_id,
        feature_cells,
        state_budget_bytes,
        true,
    )
}
fn run(
    registry: &mut Registry,
    input: &Input,
    diagnosis_id: &str,
    feature_cells: usize,
    state_budget_bytes: u64,
    incremental: bool,
) -> Result<Option<Execution>> {
    if feature_cells == 0 || feature_cells > 4_000_000 || input.dataset.ticks.len() > 2_000_000 {
        return Err("training review input/feature budget exceeded".into());
    }
    let source = improvement::read_report(registry, diagnosis_id)?.ok_or("diagnosis missing")?;
    improvement::authorize(&source.parent_key)?;
    if source.proposals.is_empty() {
        return Err("diagnosis has no child proposals".into());
    }
    input.dataset.verify()?;
    let key = &source.parent_key;
    let parent = registry
        .result(&source.parent_id)?
        .ok_or("parent result missing")?;
    let strategy: Strategy =
        serde_json::from_value(parent.result["strategy"].clone()).map_err(|e| e.to_string())?;
    let costs: Costs =
        serde_json::from_value(parent.result["cost_profile"].clone()).map_err(|e| e.to_string())?;
    let shadow_policy: Option<crate::candidate_screen::ShadowPolicy> =
        serde_json::from_value(parent.result["shadow_policy"].clone())
            .map_err(|e| e.to_string())?;
    let proof_policy: Option<crate::proof_screen::Policy> =
        serde_json::from_value(parent.result["proof_policy"].clone()).map_err(|e| e.to_string())?;
    if input.conversions.len() != input.dataset.ticks.len()
        || input.dataset.manifest.content_hash != key.dataset_hash
        || input.dataset.manifest.profile_hash != key.profile_hash
        || input.dataset.profile.symbol != key.instrument
        || data::hash(&costs)? != key.cost_hash
        || strategy.identity()? != key.concept_hash
        || research::configuration_hash(
            input,
            &costs,
            feature_cells,
            shadow_policy.as_ref(),
            proof_policy.as_ref(),
        )? != key.configuration_hash
    {
        return Err("review requires exact parent data, profile, provenance, costs, conversions and feature configuration".into());
    }
    if !(32 * 1024 * 1024..=8 * 1024 * 1024 * 1024).contains(&state_budget_bytes) {
        return Err("review state budget out of bounds".into());
    }
    let revision = research::compute_revision()?;
    let id = identity(diagnosis_id, &revision, feature_cells, VERSION)?;
    if let Some(report) = read_report(registry, &id)? {
        return Ok(Some(Execution {
            reused: true,
            report,
        }));
    }
    let window = Interval {
        start_ms: key.start_ms,
        end_exclusive_ms: key.end_exclusive_ms,
    };
    let whole = key.protocol_hash == data::hash(&"training-only-conservative-signals-v1")?;
    if whole
        && (window.start_ms != input.dataset.manifest.first_ms
            || Some(window.end_exclusive_ms) != input.dataset.manifest.last_ms.checked_add(1))
    {
        return Err("whole-history training scope mismatch".into());
    }
    let evaluate = |registry: &mut Registry, strategy: &Strategy| -> Result<Evidence> {
        research::check_state_budget(registry, state_budget_bytes)?;
        let evaluation = (!whole).then(|| Evaluation {
            window: window.clone(),
            stage: key.stage.clone(),
            protocol_hash: key.protocol_hash.clone(),
        });
        let execution = research::evaluate_configured(
            registry,
            input,
            strategy,
            &costs,
            &revision,
            research::EvaluationOptions {
                feature_cells,
                evaluation: evaluation.as_ref(),
                shadow_policy: shadow_policy.as_ref(),
                proof_policy: proof_policy.as_ref(),
            },
        )?;
        evidence(registry, &execution.experiment_id)
    };
    let missing = |registry: &Registry, strategy: &Strategy| -> Result<bool> {
        let mut expected = key.clone();
        expected.code_revision = revision.clone();
        expected.concept_hash = strategy.identity()?;
        Ok(registry.result(&expected.identity()?)?.is_none())
    };
    let new_baseline = missing(registry, &strategy)?;
    let baseline = evaluate(registry, &strategy)?;
    if incremental && new_baseline {
        return Ok(None);
    }
    let mut children = Vec::new();
    for proposal in &source.proposals {
        let new_child = missing(registry, &proposal.strategy)?;
        let result = evaluate(registry, &proposal.strategy)?;
        children.push(Child {
            proposal_edge_id: proposal.edge_id.clone(),
            net_delta_micro_usd: delta(&result, &baseline)?,
            evidence: result,
        });
        if incremental && new_child {
            return Ok(None);
        }
    }
    let report = Report {
        id,
        version: VERSION.into(),
        diagnosis_id: diagnosis_id.into(),
        compute_revision: revision,
        feature_cells,
        baseline,
        children,
        status: "DESCRIPTIVE_TRAINING_ONLY".into(),
    };
    research::check_state_budget(registry, state_budget_bytes)?;
    let json = serde_json::to_string(&report).map_err(|e| e.to_string())?;
    if json.len() > 1024 * 1024 {
        return Err("review report budget exceeded".into());
    }
    let inserted = registry.connection.execute("INSERT OR IGNORE INTO training_reviews(id,diagnosis_id,report_json,report_hash) VALUES(?1,?2,?3,?4)",params![report.id,diagnosis_id,json,data::hash(&report)?]).map_err(|e|e.to_string())?;
    let stored = read_report(registry, &report.id)?.ok_or("review publication missing")?;
    if data::hash(&stored)? != data::hash(&report)? {
        return Err("conflicting immutable training review".into());
    }
    Ok(Some(Execution {
        reused: inserted == 0,
        report: stored,
    }))
}
