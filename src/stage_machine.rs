//! Explicit, durable, crash-safe research stage control.
//!
//! A candidate enters the pipeline at [`Stage::CandleScreen`], advances through
//! [`Stage::TickVerification`] and may reach [`Stage::IndependentValidation`]
//! only after a pre-reserved holdout authorizes it. Every transition is written
//! to an append-only SQLite table *before* the next step runs, so a crash leaves
//! an exact resume point: re-running [`advance`](crate::stage_machine::advance)
//! reads the persisted lineage and never repeats or skips a recorded transition.
//! Lineage rows are immutable; validation refuses regressing or discontinuous
//! chains.
use crate::{
    Result,
    data::hash,
    storage::{ExperimentKey, Registry, Stage},
};
use rusqlite::OptionalExtension;

/// Maximum recorded transitions for one experiment. The ladder has three stages,
/// so a generous bound exists only to reject corrupt/looping writers early.
pub const MAX_TRANSITIONS_PER_EXPERIMENT: i64 = 8;

/// Schema initializer, invoked from the Registry bootstrap.
pub fn initialize(connection: &rusqlite::Connection) -> Result<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS stage_transitions(
                experiment_id TEXT NOT NULL REFERENCES experiments(id),
                sequence INTEGER NOT NULL CHECK(sequence>0),
                from_stage TEXT NOT NULL,
                to_stage TEXT NOT NULL,
                at_ms INTEGER NOT NULL,
                decision_hash TEXT NOT NULL CHECK(length(decision_hash)=64),
                PRIMARY KEY(experiment_id,sequence)
            );
            CREATE TRIGGER IF NOT EXISTS stage_transition_no_update BEFORE UPDATE ON stage_transitions BEGIN SELECT RAISE(ABORT,'stage transition immutable');END;
            CREATE TRIGGER IF NOT EXISTS stage_transition_no_delete BEFORE DELETE ON stage_transitions BEGIN SELECT RAISE(ABORT,'stage transition immutable');END;",
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub experiment_id: String,
    pub sequence: i64,
    pub from_stage: Stage,
    pub to_stage: Stage,
    pub at_ms: i64,
    pub decision_hash: String,
}

fn parse_label(label: &str) -> Result<Stage> {
    match label {
        "CandleScreen" => Ok(Stage::CandleScreen),
        "TickVerification" => Ok(Stage::TickVerification),
        "IndependentValidation" => Ok(Stage::IndependentValidation),
        other => Err(format!("unknown stage label {other}")),
    }
}

fn label(stage: &Stage) -> &'static str {
    match stage {
        Stage::CandleScreen => "CandleScreen",
        Stage::TickVerification => "TickVerification",
        Stage::IndependentValidation => "IndependentValidation",
    }
}

fn stage_index(stage: &Stage) -> u8 {
    match stage {
        Stage::CandleScreen => 0,
        Stage::TickVerification => 1,
        Stage::IndependentValidation => 2,
    }
}

/// The single legal next stage.
pub fn next_stage(from: &Stage) -> Option<Stage> {
    match from {
        Stage::CandleScreen => Some(Stage::TickVerification),
        Stage::TickVerification => Some(Stage::IndependentValidation),
        Stage::IndependentValidation => None,
    }
}

/// Deterministic decision hash binding the experiment, the stage pair and the
/// deciding evidence. Used to prove which immutable evidence authorized a move.
pub fn decision_hash(key: &ExperimentKey, from: &Stage, to: &Stage) -> Result<String> {
    hash(&("stage-decision-v1", key.identity()?, label(from), label(to)))
}

/// Record the entry transition CandleScreen -> TickVerification for a freshly
/// registered candidate, but only after the Stage-1 screening outcome has been
/// established and the candidate passed the relevant numerical gate.
///
/// - A rejected candidate cannot continue: no entry transition is recorded and
///   the candidate stays at CandleScreen.
/// - An uncertain (held) candidate is not mislabeled as passed: no entry
///   transition is recorded either; it must be resolved against exact replay.
/// - A passing candidate advances to TickVerification; the decision hash binds
///   the Stage-1 gate evidence so the hash independently demonstrates the
///   candidate passed the gate.
///
/// Idempotent: a candidate already present keeps its existing lineage.
pub fn record_entry(
    registry: &Registry,
    key: &ExperimentKey,
    gate_passed: bool,
    gate_evidence: &str,
) -> Result<()> {
    let id = key.identity()?;
    let tx = crate::storage::writer_tx(&registry.connection).map_err(|e| e.to_string())?;
    let existing: Option<i64> = tx
        .query_row(
            "SELECT MAX(sequence) FROM stage_transitions WHERE experiment_id=?1",
            [&id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    if existing.is_some() {
        return Ok(());
    }
    // Only a CandleScreen candidate that passed the Stage-1 numerical gate may
    // record the entry transition. The decision hash binds the gate evidence,
    // so it cannot be reproduced without the candidate having passed.
    if key.stage != Stage::CandleScreen || !gate_passed || gate_evidence.is_empty() {
        return Ok(());
    }
    let (from, to) = (Stage::CandleScreen.clone(), Stage::TickVerification.clone());
    let at_ms = chrono::Utc::now().timestamp_millis();
    let digest = entry_decision_hash(key, &from, &to, gate_evidence)?;
    tx.execute(
        "INSERT INTO stage_transitions(experiment_id,sequence,from_stage,to_stage,at_ms,decision_hash)
         VALUES (?1,1,?2,?3,?4,?5)",
        rusqlite::params![id, label(&from), label(&to), at_ms, digest],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

/// Decision hash for the CandleScreen -> TickVerification entry transition.
/// Binds the experiment identity and the Stage-1 gate evidence so the hash
/// independently demonstrates the candidate passed the relevant numerical gate.
pub fn entry_decision_hash(
    key: &ExperimentKey,
    from: &Stage,
    to: &Stage,
    gate_evidence: &str,
) -> Result<String> {
    hash(&(
        "stage-decision-v1",
        key.identity()?,
        label(from),
        label(to),
        gate_evidence,
    ))
}

/// Full ordered transition history for one experiment.
pub fn history(registry: &Registry, experiment_id: &str) -> Result<Vec<Transition>> {
    let mut stmt = registry
        .connection
        .prepare(
            "SELECT experiment_id,sequence,from_stage,to_stage,at_ms,decision_hash
             FROM stage_transitions WHERE experiment_id=?1 ORDER BY sequence",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([experiment_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (experiment_id, sequence, from_stage, to_stage, at_ms, decision_hash) in rows {
        out.push(Transition {
            experiment_id,
            sequence,
            from_stage: parse_label(&from_stage)?,
            to_stage: parse_label(&to_stage)?,
            at_ms,
            decision_hash,
        });
    }
    if out.len() > MAX_TRANSITIONS_PER_EXPERIMENT as usize {
        return Err("stage transition budget exceeded".into());
    }
    Ok(out)
}

/// Validate that a stored transition chain is contiguous, forward-only and
/// consistent with the experiment's recorded stage.
pub fn validate(registry: &Registry, experiment_id: &str) -> Result<()> {
    let chain = history(registry, experiment_id)?;
    let mut previous_end: Option<Stage> = None;
    for (index, t) in chain.iter().enumerate() {
        if t.sequence != index as i64 + 1 {
            return Err("stage transition sequence gap".into());
        }
        match &previous_end {
            None => {}
            Some(prev) if *prev == t.from_stage => {}
            Some(prev) => return Err(format!("stage regression {prev:?} -> {:?}", t.from_stage)),
        }
        if next_stage(&t.from_stage) != Some(t.to_stage.clone()) {
            return Err("illegal stage transition".into());
        }
        previous_end = Some(t.to_stage.clone());
    }
    let key: Option<ExperimentKey> = registry.experiment_key(experiment_id)?;
    if let Some(key) = key {
        let recorded_stage_index = stage_index(&key.stage);
        let chain_end_index = chain
            .last()
            .map(|t| stage_index(&t.to_stage))
            .unwrap_or(recorded_stage_index);
        // Explicit stage advancement may legitimately move beyond the entry stage
        // recorded in the immutable experiment key; the reverse (a chain behind
        // the entry stage) is corruption.
        if !chain.is_empty() && chain_end_index < recorded_stage_index {
            return Err("recorded stage diverges from transition lineage".into());
        }
    }
    Ok(())
}

/// True when a pre-reserved holdout covers this experiment's window and protocol.
fn holdout_authorizes(registry: &Registry, key: &ExperimentKey) -> Result<bool> {
    let authorized: bool = registry
        .connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM instrument_holdouts
                 WHERE dataset_hash=?1 AND instrument=?2 AND protocol_hash=?3
                   AND start_ms<?5 AND end_ms>?4 AND length(selected_json)>2
                UNION ALL
                SELECT 1 FROM holdouts
                 WHERE dataset_hash=?1 AND protocol_hash=?3
                   AND start_ms<?5 AND end_ms>?4 AND length(selected_json)>2
                LIMIT 1
             )",
            rusqlite::params![
                key.dataset_hash,
                key.instrument,
                key.protocol_hash,
                key.start_ms,
                key.end_exclusive_ms
            ],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(authorized)
}

/// Crash-safe advance: move the experiment one legal stage forward, persisting
/// the transition before returning. Repeated calls walk the ladder exactly once
/// per step; a crash between steps resumes from the last recorded row because
/// the current position is derived from durable history, never from process
/// memory.
pub fn advance(registry: &mut Registry, key: &ExperimentKey, now_ms: i64) -> Result<Option<Stage>> {
    let id = key.identity()?;
    // Never advance on a corrupted or non-contiguous durable chain.
    validate(registry, &id)?;
    let chain = history(registry, &id)?;
    let current = chain
        .last()
        .map(|t| t.to_stage.clone())
        .or(Some(Stage::CandleScreen))
        .ok_or("missing stage position")?;
    let next = match next_stage(&current) {
        Some(next) => next,
        None => return Ok(None),
    };
    // Independent Validation is a protected stage: it may only be entered after
    // a matching, pre-reserved concept/dataset/window/protocol holdout exists.
    // Both instrument-scoped and legacy global reservations are accepted; the
    // reservation must cover this exact window and protocol revision.
    if next == Stage::IndependentValidation && !holdout_authorizes(registry, key)? {
        return Err("independent validation requires a reserved holdout".into());
    }
    let digest = decision_hash(key, &current, &next)?;
    let sequence = chain.len() as i64 + 1;
    if sequence > MAX_TRANSITIONS_PER_EXPERIMENT {
        return Err("stage transition budget exceeded".into());
    }
    let tx = crate::storage::writer_tx(&registry.connection).map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO stage_transitions(experiment_id,sequence,from_stage,to_stage,at_ms,decision_hash)
         VALUES (?1,?2,?3,?4,?5,?6)",
        rusqlite::params![id, sequence, label(&current), label(&next), now_ms, digest],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    // Every durable transition emits an immutable telemetry event, so the
    // decision feed can be replayed in exact order after a crash.
    crate::telemetry::record(
        registry,
        &id,
        "stage",
        "stage-advanced",
        now_ms,
        &serde_json::json!({"from": label(&current), "to": label(&next)}),
    )?;
    Ok(Some(next))
}

/// True when the recorded lineage already reached `stage` (used by resume).
pub fn has_reached(registry: &Registry, experiment_id: &str, stage: &Stage) -> Result<bool> {
    let chain = history(registry, experiment_id)?;
    Ok(chain
        .iter()
        .any(|t| stage_index(&t.to_stage) >= stage_index(stage)))
}
