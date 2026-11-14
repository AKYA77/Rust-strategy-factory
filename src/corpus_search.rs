//! Bounded synthetic corpus candidate generation and fenced durable queue progress.
use crate::{
    Result, data, research,
    storage::{ExperimentKey, Registry},
    strategies::Strategy,
    tick_engine::{self, CorpusBatchLimits, CorpusBudget, CorpusProgress, CorpusRun, Costs},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
const VERSION: &str = "synthetic-corpus-search-v1";
const GENERATOR: &str = "candidate-at-regions-v1";
pub(crate) fn initialize(connection: &Connection) -> Result<()> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS corpus_searches(id TEXT PRIMARY KEY,configuration TEXT NOT NULL,cursor INTEGER NOT NULL DEFAULT 0 CHECK(cursor>=0),state TEXT NOT NULL DEFAULT 'running' CHECK(state IN ('running','paused','stopped','exhausted')),token TEXT,expires_ms INTEGER,attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt>=0));
    CREATE TABLE IF NOT EXISTS corpus_search_jobs(search_id TEXT NOT NULL REFERENCES corpus_searches(id),slot INTEGER NOT NULL CHECK(slot>=0),body TEXT NOT NULL,hash TEXT NOT NULL,state TEXT NOT NULL CHECK(state IN ('queued','pending','completed','terminal','error')),error TEXT,PRIMARY KEY(search_id,slot));
    CREATE TRIGGER IF NOT EXISTS corpus_search_definition_immutable BEFORE UPDATE OF id,configuration ON corpus_searches BEGIN SELECT RAISE(ABORT,'search definition immutable');END;
    CREATE TRIGGER IF NOT EXISTS corpus_search_job_immutable BEFORE UPDATE OF search_id,slot,body,hash ON corpus_search_jobs BEGIN SELECT RAISE(ABORT,'search job immutable');END;
    CREATE TRIGGER IF NOT EXISTS corpus_search_no_delete BEFORE DELETE ON corpus_searches BEGIN SELECT RAISE(ABORT,'search immutable');END;
    CREATE TRIGGER IF NOT EXISTS corpus_search_job_no_delete BEFORE DELETE ON corpus_search_jobs BEGIN SELECT RAISE(ABORT,'search job immutable');END;").map_err(|e|e.to_string())?;
    connection.execute_batch("CREATE TABLE IF NOT EXISTS corpus_search_requeues(event_id INTEGER PRIMARY KEY,search_id TEXT NOT NULL,slot INTEGER NOT NULL,sequence INTEGER NOT NULL CHECK(sequence BETWEEN 1 AND 3),body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=4096),hash TEXT NOT NULL CHECK(length(hash)=64),UNIQUE(search_id,slot,sequence),FOREIGN KEY(search_id,slot) REFERENCES corpus_search_jobs(search_id,slot));CREATE TRIGGER IF NOT EXISTS corpus_search_requeue_no_update BEFORE UPDATE ON corpus_search_requeues BEGIN SELECT RAISE(ABORT,'requeue history immutable');END;CREATE TRIGGER IF NOT EXISTS corpus_search_requeue_no_delete BEFORE DELETE ON corpus_search_requeues BEGIN SELECT RAISE(ABORT,'requeue history immutable');END;").map_err(|e|e.to_string())
}
pub struct SearchRun<'a> {
    pub base: &'a Path,
    pub specification: &'a crate::corpus::Specification,
    pub costs: &'a Costs,
    pub budget: CorpusBudget,
    pub seed: u64,
    pub maximum_candidates: usize,
    pub queue_width: usize,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    version: String,
    generator: String,
    seed: u64,
    maximum_candidates: usize,
    queue_width: usize,
    source_identity: String,
    source_key: ExperimentKey,
    costs: Costs,
    budget: CorpusBudget,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    slot: usize,
    family: String,
    strategy: Strategy,
    key: ExperimentKey,
}
struct Job {
    definition: Definition,
    state: String,
    error: Option<String>,
}
type Lease = crate::coordinator::Lease;
#[derive(Serialize)]
pub struct Step {
    pub kind: String,
    pub status: serde_json::Value,
}
fn configuration(run: &SearchRun<'_>) -> Result<Configuration> {
    if !(1..=4096).contains(&run.maximum_candidates) || !(1..=32).contains(&run.queue_width) {
        return Err("invalid corpus search candidate/queue limits".into());
    }
    let (_, first) = research::candidate_at(0, run.seed)?;
    Ok(Configuration {
        version: VERSION.into(),
        generator: GENERATOR.into(),
        seed: run.seed,
        maximum_candidates: run.maximum_candidates,
        queue_width: run.queue_width,
        source_identity: run.specification.identity()?,
        source_key: tick_engine::corpus_key(
            run.base,
            run.specification,
            &first,
            run.costs,
            run.budget,
        )?,
        costs: run.costs.clone(),
        budget: run.budget,
    })
}
fn frozen_key(config: &Configuration, strategy: &Strategy) -> Result<ExperimentKey> {
    let mut key = config.source_key.clone();
    key.concept_hash = strategy.identity()?;
    key.configuration_hash = tick_engine::corpus_configuration_hash(
        &config.source_identity,
        strategy,
        &config.costs,
        config.budget,
        &config.source_key.code_revision,
    )?;
    Ok(key)
}
fn definition(config: &Configuration, slot: usize) -> Result<Definition> {
    let (family, strategy) = research::candidate_at(slot as u64, config.seed)?;
    let key = frozen_key(config, &strategy)?;
    Ok(Definition {
        slot,
        family: format!("{family:?}"),
        strategy,
        key,
    })
}
fn register(registry: &mut Registry, config: &Configuration) -> Result<String> {
    let id = data::hash(config)?;
    let body = serde_json::to_string(config).map_err(|e| e.to_string())?;
    let exists: bool = registry
        .connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM corpus_searches WHERE id=?1)",
            [&id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        for slot in 0..config.maximum_candidates {
            research::candidate_at(slot as u64, config.seed)
                .map_err(|error| format!("frozen candidate slot {slot} refused: {error}"))?;
        }
    }
    let tx = registry
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let stored_bytes: Option<usize> = tx
        .query_row(
            "SELECT length(CAST(configuration AS BLOB)) FROM corpus_searches WHERE id=?1",
            [&id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if stored_bytes.is_some_and(|n| n > 1024 * 1024) {
        return Err("stored corpus search definition exceeds metadata cap".into());
    }
    let previous: Option<String> = tx
        .query_row(
            "SELECT configuration FROM corpus_searches WHERE id=?1",
            [&id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(previous) = previous {
        if previous != body {
            return Err("immutable corpus search configuration mismatch".into());
        }
    } else {
        let count: i64 = tx
            .query_row("SELECT count(*) FROM corpus_searches", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if count >= 64 {
            return Err("corpus search count quota reached".into());
        }
        tx.execute(
            "INSERT INTO corpus_searches(id,configuration) VALUES(?1,?2)",
            params![id, body],
        )
        .map_err(|e| e.to_string())?;
        crate::search_control::register(&tx, &id, "running")?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(id)
}
fn claim(registry: &mut Registry, id: &str, now: i64, duration: i64) -> Result<Option<Lease>> {
    crate::coordinator::claim(
        registry,
        crate::coordinator::Kind::Search,
        id,
        now,
        duration,
    )
}
fn fence(connection: &Connection, lease: &Lease, now: i64) -> Result<()> {
    crate::coordinator::fence(connection, lease, now)
}
fn release(registry: &Registry, lease: &Lease) -> Result<()> {
    crate::coordinator::release(registry, lease)
}
type SearchRow = (Option<String>, usize, String, i64, Option<i64>);
type SearchSnapshot = (Configuration, usize, String, i64, Option<i64>, Vec<Job>);
fn read(connection: &Connection, id: &str) -> Result<SearchSnapshot> {
    let (body, cursor, state, attempt, expiry): SearchRow = connection
        .query_row(
            "SELECT CASE WHEN length(CAST(configuration AS BLOB))<=1048576 THEN configuration ELSE NULL END,cursor,state,attempt,expires_ms FROM corpus_searches WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .map_err(|e| e.to_string())?;
    let body = body.ok_or("corpus search definition exceeds metadata cap")?;
    if body.len() > 1024 * 1024 {
        return Err("corpus search definition budget exceeded".into());
    }
    let config: Configuration = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    if data::hash(&config)? != id
        || config.version != VERSION
        || config.generator != GENERATOR
        || !(1..=4096).contains(&config.maximum_candidates)
        || !(1..=32).contains(&config.queue_width)
        || cursor > config.maximum_candidates
    {
        return Err("corpus search identity/cursor integrity failure".into());
    }
    let (rows,bytes):(usize,usize)=connection.query_row("SELECT count(*),coalesce(sum(length(CAST(body AS BLOB))),0) FROM corpus_search_jobs WHERE search_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
    if rows != cursor || bytes > 16 * 1024 * 1024 {
        return Err("corpus search persisted queue budget/cursor mismatch".into());
    }
    config.source_key.identity()?;
    config.costs.validate()?;
    if config.source_key.cost_hash != data::hash(&config.costs)? {
        return Err("search frozen cost identity mismatch".into());
    }
    let mut q=connection.prepare("SELECT slot,body,hash,state,error FROM corpus_search_jobs WHERE search_id=?1 ORDER BY slot").map_err(|e|e.to_string())?;
    let rows = q
        .query_map([id], |r| {
            Ok((
                r.get::<_, usize>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut jobs = Vec::new();
    for row in rows {
        let (slot, body, hash, state, error) = row.map_err(|e| e.to_string())?;
        if body.len() > 65536 || error.as_ref().is_some_and(|s| s.chars().count() > 512) {
            return Err("corpus search job metadata budget exceeded".into());
        }
        let definition: Definition = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        definition.key.identity()?;
        if slot != jobs.len()
            || definition.slot != slot
            || data::hash(&definition)? != hash
            || definition.strategy.identity()? != definition.key.concept_hash
            || data::hash(&definition.key)?
                != data::hash(&frozen_key(&config, &definition.strategy)?)?
            || definition.key.protocol_hash != "synthetic-corpus-training-v1"
        {
            return Err("corpus search job integrity failure".into());
        }
        if state == "completed" {
            let completed: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM experiments WHERE id=?1 AND state='completed')",
                    [definition.key.identity()?],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !completed {
                return Err("search completed slot lacks committed experiment".into());
            }
        }
        jobs.push(Job {
            definition,
            state,
            error,
        });
    }
    if jobs
        .iter()
        .filter(|j| j.state == "queued" || j.state == "pending")
        .count()
        > config.queue_width
    {
        return Err("corpus search active queue exceeds frozen width".into());
    }
    if jobs.len() != cursor {
        return Err("corpus search queue/cursor mismatch".into());
    }
    crate::search_control::read(connection, id)?;
    Ok((config, cursor, state, attempt, expiry, jobs))
}

const REQUEUE_VERSION: &str = "corpus-search-error-requeue-v1";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequeueRecord {
    version: String,
    search_id: String,
    slot: usize,
    sequence: usize,
    at_ms: i64,
    prior_error: String,
    previous_hash: String,
}
fn requeue_history(
    connection: &Connection,
    id: &str,
    slots: usize,
) -> Result<Vec<(RequeueRecord, String)>> {
    let mut query=connection.prepare("SELECT slot,sequence,body,hash FROM corpus_search_requeues WHERE search_id=?1 ORDER BY event_id").map_err(|e|e.to_string())?;
    let rows = query
        .query_map([id], |r| {
            Ok((
                r.get::<_, usize>(0)?,
                r.get::<_, usize>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    let mut heads = std::collections::BTreeMap::<usize, (usize, String)>::new();
    for row in rows {
        let (slot, sequence, body, hash) = row.map_err(|e| e.to_string())?;
        if result.len() >= 10000 || body.len() > 4096 {
            return Err("requeue history budget exceeded".into());
        }
        let record: RequeueRecord = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let previous = heads.get(&slot);
        let expected_sequence = previous.map_or(1, |(n, _)| n + 1);
        let expected_hash = previous.map_or("", |(_, hash)| hash.as_str());
        if record.version != REQUEUE_VERSION
            || record.search_id != id
            || record.slot != slot
            || slot >= slots
            || record.sequence != sequence
            || sequence != expected_sequence
            || sequence > 3
            || record.prior_error.chars().count() > 512
            || record.previous_hash != expected_hash
            || chrono::DateTime::from_timestamp_millis(record.at_ms).is_none()
            || data::hash(&record)? != hash
        {
            return Err("requeue history integrity mismatch".into());
        }
        heads.insert(slot, (sequence, hash.clone()));
        result.push((record, hash));
    }
    Ok(result)
}
/// Requeue only uncommitted error slots under the same frozen keys. Child leases
/// and terminal results remain untouched; an operator process must run the work.
pub fn requeue_errors(
    registry: &mut Registry,
    id: &str,
    maximum_slots: usize,
) -> Result<serde_json::Value> {
    if !(1..=32).contains(&maximum_slots) {
        return Err("requeue request must contain1..32 slots".into());
    }
    let tx = registry
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let (config, cursor, state, _, expiry, jobs) = read(&tx, id)?;
    if state != "paused" || expiry.is_some_and(|t| t > chrono::Utc::now().timestamp_millis()) {
        return Err("requeue requires a paused search without an active coordinator".into());
    }
    let history = requeue_history(&tx, id, cursor)?;
    let mut heads = std::collections::BTreeMap::<usize, (usize, String)>::new();
    for (record, hash) in history {
        heads.insert(record.slot, (record.sequence, hash));
    }
    let active = jobs
        .iter()
        .filter(|j| j.state == "queued" || j.state == "pending")
        .count();
    let capacity = config.queue_width.saturating_sub(active).min(maximum_slots);
    if capacity == 0 {
        return Err("no retry queue capacity; reconcile pending slots first".into());
    }
    let eligible: Vec<_> = jobs
        .iter()
        .filter(|j| j.state == "error" && heads.get(&j.definition.slot).map_or(0, |(n, _)| *n) < 3)
        .take(capacity)
        .collect();
    if eligible.is_empty() {
        return Err("no eligible error slots; bounded requeue budget may be exhausted".into());
    }
    let (count,size):(usize,usize)=tx.query_row("SELECT count(*),coalesce(sum(length(CAST(body AS BLOB))),0) FROM corpus_search_requeues",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
    let mut incoming = Vec::new();
    let mut bytes = 0usize;
    for job in eligible {
        let prior_error = job
            .error
            .clone()
            .ok_or("error slot lacks prior error evidence")?;
        let previous = heads.get(&job.definition.slot);
        let record = RequeueRecord {
            version: REQUEUE_VERSION.into(),
            search_id: id.into(),
            slot: job.definition.slot,
            sequence: previous.map_or(1, |(n, _)| n + 1),
            at_ms: chrono::Utc::now().timestamp_millis(),
            prior_error,
            previous_hash: previous.map_or_else(String::new, |(_, h)| h.clone()),
        };
        let body = serde_json::to_string(&record).map_err(|e| e.to_string())?;
        if body.len() > 4096 {
            return Err("requeue record exceeds metadata budget".into());
        }
        bytes += body.len();
        incoming.push((record, body));
    }
    if count + incoming.len() > 10000 || size + bytes > 40 * 1024 * 1024 {
        return Err("global requeue history quota reached".into());
    }
    let mut slots = Vec::new();
    for (record, body) in incoming {
        tx.execute("INSERT INTO corpus_search_requeues(search_id,slot,sequence,body,hash) VALUES(?1,?2,?3,?4,?5)",params![id,record.slot,record.sequence,body,data::hash(&record)?]).map_err(|e|e.to_string())?;
        let n=tx.execute("UPDATE corpus_search_jobs SET state='queued',error=NULL WHERE search_id=?1 AND slot=?2 AND state='error'",params![id,record.slot]).map_err(|e|e.to_string())?;
        if n != 1 {
            return Err("error slot changed during requeue".into());
        }
        slots.push(record.slot.to_string());
    }
    crate::search_control::transition(
        &tx,
        id,
        "running",
        crate::search_control::Cause::OperatorRequeue,
        chrono::Utc::now().timestamp_millis(),
    )?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(
        serde_json::json!({"search_id":id,"requeued_slots":slots,"state":"running","qualification_granted":false,"limitations":"Explicit bounded requeue under unchanged keys; original errors remain immutable. Child leases and committed results are never reset. Start an operator process to execute queued work."}),
    )
}
pub fn status(registry: &Registry, id: &str) -> Result<serde_json::Value> {
    let tx = registry
        .connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let (config, cursor, state, attempt, expiry, jobs) = read(&tx, id)?;
    let history = requeue_history(&tx, id, cursor)?;
    let retry_counts: std::collections::BTreeMap<usize, usize> =
        history.iter().map(|(r, _)| (r.slot, r.sequence)).collect();
    let retry_eligible = jobs
        .iter()
        .filter(|j| {
            j.state == "error" && retry_counts.get(&j.definition.slot).copied().unwrap_or(0) < 3
        })
        .count();
    let retry_capacity = config.queue_width.saturating_sub(
        jobs.iter()
            .filter(|j| j.state == "queued" || j.state == "pending")
            .count(),
    );
    let recent_requeues:Vec<_>=history.iter().rev().take(32).map(|(record,hash)|serde_json::json!({"candidate_index":record.slot.to_string(),"requeue_number":record.sequence,"at_ms":record.at_ms,"prior_error":record.prior_error,"hash":hash})).collect();
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for job in &jobs {
        *counts.entry(job.state.clone()).or_default() += 1;
    }
    let recent:Vec<_>=jobs.iter().rev().take(32).map(|j|serde_json::json!({"candidate_index":j.definition.slot.to_string(),"family":j.definition.family,"experiment_id":j.definition.key.identity().ok(),"state":j.state,"error":j.error})).collect();
    let control = crate::search_control::status(&tx, id)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(
        serde_json::json!({"control":control,"search_id":id,"state":state,"generated_slots":cursor.to_string(),"maximum_candidates":config.maximum_candidates.to_string(),"attempt":attempt.to_string(),"lease_active":expiry.is_some_and(|t|t>chrono::Utc::now().timestamp_millis()),"slot_states":counts,"recent_slots":recent,"requeue_records":history.len().to_string(),"retry_eligible_slots":retry_eligible.to_string(),"retry_capacity":retry_capacity.to_string(),"maximum_requeues_per_slot":3,"recent_requeues":recent_requeues,"qualification_granted":false,"limitations":"Generated slots are not new tick simulations; canonical duplicates can share immutable experiment receipts. Synthetic USD training search only."}),
    )
}
pub(crate) fn integrity(registry: &Registry) -> Result<()> {
    let tx = registry
        .connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let ids = {
        let mut q = tx
            .prepare("SELECT id FROM corpus_searches")
            .map_err(|e| e.to_string())?;
        q.query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    let (jobs, bytes): (usize, usize) = tx
        .query_row(
            "SELECT count(*),coalesce(sum(length(CAST(body AS BLOB))),0) FROM corpus_search_jobs",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    if ids.len() > 64 || jobs > 4096 || bytes > 16 * 1024 * 1024 {
        return Err("corpus search persisted quotas exceeded".into());
    }
    let (count,bytes):(usize,usize)=tx.query_row("SELECT count(*),coalesce(sum(length(CAST(body AS BLOB))),0) FROM corpus_search_requeues",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
    if count > 10000 || bytes > 40 * 1024 * 1024 {
        return Err("persisted requeue quota exceeded".into());
    }
    crate::search_control::integrity(&tx)?;
    for id in ids {
        let (_, cursor, _, _, _, _) = read(&tx, &id)?;
        requeue_history(&tx, &id, cursor)?;
    }
    tx.commit().map_err(|e| e.to_string())
}
pub fn control(registry: &mut Registry, id: &str, state: &str) -> Result<()> {
    let tx = registry
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    crate::search_control::transition(
        &tx,
        id,
        state,
        crate::search_control::Cause::Operator,
        chrono::Utc::now().timestamp_millis(),
    )?;
    tx.commit().map_err(|e| e.to_string())
}

fn admit(registry: &mut Registry, lease: &Lease, config: &Configuration) -> Result<()> {
    let (_, cursor, _, _, _, jobs) = read(&registry.connection, &lease.id)?;
    if jobs
        .iter()
        .any(|j| j.state == "queued" || j.state == "pending")
        || cursor >= config.maximum_candidates
    {
        return Ok(());
    }
    let end = (cursor + config.queue_width).min(config.maximum_candidates);
    let mut incoming = Vec::new();
    let mut bytes = 0usize;
    for slot in cursor..end {
        let definition = definition(config, slot)?;
        let body = serde_json::to_string(&definition).map_err(|e| e.to_string())?;
        if body.len() > 65536 {
            return Err("corpus search job exceeds metadata cap".into());
        }
        bytes += body.len();
        incoming.push((slot, body, data::hash(&definition)?));
    }
    let tx = registry
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    fence(&tx, lease, chrono::Utc::now().timestamp_millis())?;
    let (count, size): (usize, usize) = tx
        .query_row(
            "SELECT count(*),coalesce(sum(length(CAST(body AS BLOB))),0) FROM corpus_search_jobs",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    if count + incoming.len() > 4096 || size + bytes > 16 * 1024 * 1024 {
        return Err("durable corpus queue quota reached".into());
    }
    for (slot, body, hash) in incoming {
        tx.execute("INSERT INTO corpus_search_jobs(search_id,slot,body,hash,state) VALUES(?1,?2,?3,?4,'queued')",params![lease.id,slot,body,hash]).map_err(|e|e.to_string())?;
    }
    let n = tx
        .execute(
            "UPDATE corpus_searches SET cursor=?2 WHERE id=?1 AND cursor=?3",
            params![lease.id, end, cursor],
        )
        .map_err(|e| e.to_string())?;
    if n != 1 {
        return Err("search admission cursor changed".into());
    }
    tx.commit().map_err(|e| e.to_string())
}
/// Recover an admitted batch before generating another bounded candidate region.
pub fn step(database: &Path, run: SearchRun<'_>, limits: CorpusBatchLimits) -> Result<Step> {
    if !(1..=16).contains(&limits.workers)
        || !(1..=64).contains(&limits.max_new_chunks)
        || !(1..=86_400_000).contains(&limits.lease_ms)
    {
        return Err("invalid corpus search worker/step/lease limits".into());
    }
    let config = configuration(&run)?;
    let mut registry = Registry::open(database)?;
    let id = register(&mut registry, &config)?;
    let Some(lease) = claim(
        &mut registry,
        &id,
        chrono::Utc::now().timestamp_millis(),
        limits.lease_ms,
    )?
    else {
        let status = status(&registry, &id)?;
        let kind = if status["state"] == "running" {
            "busy"
        } else if status["state"] == "exhausted" {
            "exhausted"
        } else {
            "held"
        };
        return Ok(Step {
            kind: kind.into(),
            status,
        });
    };
    let result = (|| -> Result<Step> {
        admit(&mut registry, &lease, &config)?;
        fence(
            &registry.connection,
            &lease,
            chrono::Utc::now().timestamp_millis(),
        )?;
        let (_, _, _, _, _, jobs) = read(&registry.connection, &id)?;
        let pending: Vec<_> = jobs
            .iter()
            .filter(|j| j.state == "queued" || j.state == "pending")
            .collect();
        let mut unique = Vec::<&Job>::new();
        let mut mapping = std::collections::BTreeMap::new();
        for job in &pending {
            if data::hash(&job.definition)?
                != data::hash(&definition(&config, job.definition.slot)?)?
            {
                return Err("search slot no longer matches frozen generator".into());
            }
            let key = job.definition.key.identity()?;
            if let std::collections::btree_map::Entry::Vacant(entry) = mapping.entry(key) {
                entry.insert(unique.len());
                unique.push(job);
            }
        }
        let mut blocked = false;
        let updates = if unique.is_empty() {
            Vec::new()
        } else {
            let runs: Vec<_> = unique
                .iter()
                .map(|j| CorpusRun {
                    base: run.base,
                    spec: run.specification,
                    strategy: &j.definition.strategy,
                    costs: run.costs,
                    budget: run.budget,
                })
                .collect();
            let report = tick_engine::execute_corpus_batch(database, &runs, limits)?;
            blocked = report.jobs.iter().all(|r| match r {
                Ok(CorpusProgress::Busy { .. }) => true,
                Ok(CorpusProgress::Held { state, .. }) => state != "stopped",
                _ => false,
            });
            pending
                .iter()
                .map(|job| {
                    let index = mapping[&job
                        .definition
                        .key
                        .identity()
                        .expect("prevalidated search key")];
                    let (state, error) = match &report.jobs[index] {
                        Ok(CorpusProgress::Completed { .. }) => ("completed", None),
                        Ok(CorpusProgress::Terminal { .. }) => ("terminal", None),
                        Ok(CorpusProgress::Held { state, .. }) if state == "stopped" => {
                            ("terminal", None)
                        }
                        Ok(_) => ("pending", None),
                        Err(error) => ("error", Some(error.chars().take(512).collect::<String>())),
                    };
                    (job.definition.slot, state, error)
                })
                .collect::<Vec<_>>()
        };
        let tx = registry
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        fence(&tx, &lease, chrono::Utc::now().timestamp_millis())?;
        let errored = updates.iter().any(|(_, state, _)| *state == "error");
        for (slot, state, error) in updates {
            tx.execute(
                "UPDATE corpus_search_jobs SET state=?3,error=?4 WHERE search_id=?1 AND slot=?2",
                params![id, slot, state, error],
            )
            .map_err(|e| e.to_string())?;
        }
        let (cursor,remaining):(usize,usize)=tx.query_row("SELECT cursor,(SELECT count(*) FROM corpus_search_jobs WHERE search_id=?1 AND state IN ('queued','pending')) FROM corpus_searches WHERE id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
        if errored {
            crate::search_control::transition(
                &tx,
                &id,
                "paused",
                crate::search_control::Cause::WorkerError,
                chrono::Utc::now().timestamp_millis(),
            )?;
        } else if cursor == config.maximum_candidates && remaining == 0 {
            crate::search_control::transition(
                &tx,
                &id,
                "exhausted",
                crate::search_control::Cause::Exhausted,
                chrono::Utc::now().timestamp_millis(),
            )?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        let status = status(&registry, &id)?;
        Ok(Step {
            kind: if status["state"] == "exhausted" {
                "exhausted"
            } else if blocked {
                "held-or-busy"
            } else {
                "advanced"
            }
            .into(),
            status,
        })
    })();
    let released = release(&registry, &lease);
    match result {
        Err(error) => Err(error),
        Ok(mut value) => {
            released?;
            value.status = status(&registry, &id)?;
            Ok(value)
        }
    }
}

pub fn identity(run: &SearchRun<'_>) -> Result<String> {
    data::hash(&configuration(run)?)
}
pub fn list(registry: &Registry) -> Result<Vec<serde_json::Value>> {
    let ids = {
        let mut query = registry
            .connection
            .prepare("SELECT id FROM corpus_searches ORDER BY rowid DESC LIMIT 65")
            .map_err(|e| e.to_string())?;
        query
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    if ids.len() > 64 {
        return Err("corpus search inventory quota exceeded".into());
    }
    ids.iter().map(|id| status(registry, id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> (std::path::PathBuf, crate::corpus::Specification) {
        let root =
            std::env::temp_dir().join(format!("corpus-search-unit-{name}-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let data = crate::data::synthetic_fixture("EURUSD").unwrap();
        let path = root.join("source.rsf");
        crate::native::write(&path, &data).unwrap();
        let specification = crate::corpus::describe(&[path], 10000).unwrap();
        (root, specification)
    }
    fn run<'a>(
        root: &'a Path,
        spec: &'a crate::corpus::Specification,
        costs: &'a Costs,
    ) -> SearchRun<'a> {
        SearchRun {
            base: root,
            specification: spec,
            costs,
            budget: CorpusBudget {
                max_candles: 10000,
                feature_cells: 1000000,
                max_receipts: 10000,
            },
            seed: 42,
            maximum_candidates: 4,
            queue_width: 2,
        }
    }
    #[test]
    fn pause_intent_epochs_and_backup_preserve_operator_override() {
        let (root, spec) = fixture("intent");
        let costs = Costs::default();
        let mut registry = Registry::open(&root.join("state.sqlite")).unwrap();
        let id = register(
            &mut registry,
            &configuration(&run(&root, &spec, &costs)).unwrap(),
        )
        .unwrap();
        let lease = claim(&mut registry, &id, 100, 1000).unwrap().unwrap();
        {
            let tx = registry
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            crate::search_control::transition(
                &tx,
                &id,
                "paused",
                crate::search_control::Cause::WorkerError,
                110,
            )
            .unwrap();
            tx.commit().unwrap();
        }
        let worker = status(&registry, &id).unwrap();
        assert_eq!(worker["control"]["pause_origin"], "worker_error");
        assert_eq!(worker["control"]["epoch"], "1");
        control(&mut registry, &id, "paused").unwrap();
        let paused = status(&registry, &id).unwrap();
        assert_eq!(paused["control"]["pause_origin"], "operator");
        assert_eq!(paused["control"]["epoch"], "2");
        control(&mut registry, &id, "paused").unwrap();
        assert_eq!(
            status(&registry, &id).unwrap()["control"],
            paused["control"]
        );
        let backup = root.join("backup.sqlite");
        registry.backup(&backup).unwrap();
        let restored = Registry::open(&backup).unwrap();
        assert_eq!(
            status(&restored, &id).unwrap()["control"],
            paused["control"]
        );
        restored.integrity().unwrap();
        drop(restored);
        control(&mut registry, &id, "running").unwrap();
        assert!(fence(&registry.connection, &lease, 120).is_err());
        assert_eq!(status(&registry, &id).unwrap()["control"]["epoch"], "3");
        assert!(status(&registry, &id).unwrap()["control"]["pause_origin"].is_null());
        registry.integrity().unwrap();
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn full_control_history_is_explicit_and_never_blocks_stop() {
        let (root, spec) = fixture("history-cap");
        let costs = Costs::default();
        let mut registry = Registry::open(&root.join("state.sqlite")).unwrap();
        let id = register(
            &mut registry,
            &configuration(&run(&root, &spec, &costs)).unwrap(),
        )
        .unwrap();
        for _ in 0..70 {
            control(&mut registry, &id, "paused").unwrap();
            control(&mut registry, &id, "running").unwrap();
        }
        control(&mut registry, &id, "stopped").unwrap();
        let snapshot = status(&registry, &id).unwrap();
        assert_eq!(snapshot["control"]["epoch"], "141");
        assert_eq!(snapshot["control"]["journaled_transitions"], "128");
        assert_eq!(snapshot["control"]["omitted_transitions"], "13");
        assert_eq!(snapshot["control"]["history_complete"], false);
        assert!(control(&mut registry, &id, "running").is_err());
        assert!(
            registry
                .connection
                .execute("DELETE FROM corpus_search_transitions", [])
                .is_err()
        );
        assert!(
            registry
                .connection
                .execute("UPDATE corpus_search_transitions SET epoch=999", [])
                .is_err()
        );
        registry.integrity().unwrap();
        registry.connection.execute_batch("DROP TRIGGER search_transition_no_delete;DELETE FROM corpus_search_transitions WHERE epoch=128;").unwrap();
        assert!(status(&registry, &id).is_err());
        assert!(registry.integrity().is_err());
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn schema10_pauses_migrate_unknown_and_current_missing_heads_are_not_repaired() {
        let (root, spec) = fixture("legacy-intent");
        let costs = Costs::default();
        let database = root.join("state.sqlite");
        let mut registry = Registry::open(&database).unwrap();
        let id = register(
            &mut registry,
            &configuration(&run(&root, &spec, &costs)).unwrap(),
        )
        .unwrap();
        control(&mut registry, &id, "paused").unwrap();
        registry.connection.execute_batch("DROP TABLE corpus_search_transitions;DROP TABLE corpus_search_runtime;PRAGMA user_version=10;").unwrap();
        drop(registry);
        let mut registry = Registry::open(&database).unwrap();
        let legacy = status(&registry, &id).unwrap();
        assert_eq!(legacy["control"]["pause_origin"], "legacy_unknown");
        assert_eq!(legacy["control"]["epoch"], "0");
        assert!(legacy["control"]["paused_at_ms"].is_null());
        control(&mut registry, &id, "paused").unwrap();
        assert_eq!(
            status(&registry, &id).unwrap()["control"]["pause_origin"],
            "operator"
        );
        registry.integrity().unwrap();
        registry
            .connection
            .execute_batch("DROP TABLE corpus_search_runtime;")
            .unwrap();
        drop(registry);
        let registry = Registry::open(&database).unwrap();
        assert!(status(&registry, &id).is_err());
        assert!(registry.integrity().is_err());
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn migration_revokes_inflight_coordinators_without_changing_search_keys() {
        let (root, spec) = fixture("migration-lease");
        let costs = Costs::default();
        let database = root.join("state.sqlite");
        let mut registry = Registry::open(&database).unwrap();
        let config = configuration(&run(&root, &spec, &costs)).unwrap();
        let id = register(&mut registry, &config).unwrap();
        let old = claim(&mut registry, &id, 100, 1000).unwrap().unwrap();
        registry.connection.execute_batch("DROP TABLE corpus_search_transitions;DROP TABLE corpus_search_runtime;PRAGMA user_version=10;").unwrap();
        drop(registry);
        let registry = Registry::open(&database).unwrap();
        assert!(fence(&registry.connection, &old, 110).is_err());
        let snapshot = status(&registry, &id).unwrap();
        assert_eq!(snapshot["state"], "running");
        assert_eq!(snapshot["control"]["epoch"], "0");
        assert_eq!(data::hash(&config).unwrap(), id);
        assert_eq!(snapshot["attempt"], "1");
        assert_eq!(snapshot["lease_active"], false);
        registry.integrity().unwrap();
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn racing_operator_pause_always_overrides_fenced_worker_error_publication() {
        let (root, spec) = fixture("origin-race");
        let costs = Costs::default();
        let database = root.join("state.sqlite");
        let mut registry = Registry::open(&database).unwrap();
        let id = register(
            &mut registry,
            &configuration(&run(&root, &spec, &costs)).unwrap(),
        )
        .unwrap();
        let lease = claim(&mut registry, &id, 100, 1000).unwrap().unwrap();
        drop(registry);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let worker = {
            let path = database.clone();
            let id = id.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut registry = Registry::open(&path).unwrap();
                barrier.wait();
                let tx = registry
                    .connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .unwrap();
                if fence(&tx, &lease, 110).is_ok() {
                    crate::search_control::transition(
                        &tx,
                        &id,
                        "paused",
                        crate::search_control::Cause::WorkerError,
                        110,
                    )
                    .unwrap();
                    tx.commit().unwrap();
                }
            })
        };
        let mut registry = Registry::open(&database).unwrap();
        barrier.wait();
        control(&mut registry, &id, "paused").unwrap();
        worker.join().unwrap();
        assert_eq!(
            status(&registry, &id).unwrap()["control"]["pause_origin"],
            "operator"
        );
        registry.integrity().unwrap();
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn epoch_fence_rejects_old_nonce_even_if_token_columns_are_restored() {
        let (root, spec) = fixture("epoch-fence");
        let costs = Costs::default();
        let mut registry = Registry::open(&root.join("state.sqlite")).unwrap();
        let id = register(
            &mut registry,
            &configuration(&run(&root, &spec, &costs)).unwrap(),
        )
        .unwrap();
        let lease = claim(&mut registry, &id, 100, 1000).unwrap().unwrap();
        control(&mut registry, &id, "paused").unwrap();
        control(&mut registry, &id, "running").unwrap();
        registry
            .connection
            .execute(
                "UPDATE corpus_searches SET token=?2,attempt=?3,expires_ms=1000 WHERE id=?1",
                params![id, lease.token, lease.attempt],
            )
            .unwrap();
        assert!(fence(&registry.connection, &lease, 120).is_err());
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn replaced_coordinators_and_controls_fence_progress_without_clearing_new_tokens() {
        let (root, spec) = fixture("fencing");
        let costs = Costs::default();
        let config = configuration(&run(&root, &spec, &costs)).unwrap();
        let mut registry = Registry::open(&root.join("state.sqlite")).unwrap();
        let id = register(&mut registry, &config).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let first = claim(&mut registry, &id, now, 100).unwrap().unwrap();
        assert!(claim(&mut registry, &id, now + 50, 100).unwrap().is_none());
        let replacement = claim(&mut registry, &id, now + 101, 100).unwrap().unwrap();
        assert_eq!(replacement.attempt, first.attempt + 1);
        assert!(fence(&registry.connection, &first, now + 102).is_err());
        fence(&registry.connection, &replacement, now + 102).unwrap();
        release(&registry, &first).unwrap();
        fence(&registry.connection, &replacement, now + 102).unwrap();
        control(&mut registry, &id, "paused").unwrap();
        assert!(fence(&registry.connection, &replacement, now + 102).is_err());
        control(&mut registry, &id, "running").unwrap();
        assert!(fence(&registry.connection, &replacement, now + 102).is_err());
        let resumed = claim(&mut registry, &id, now + 102, 100).unwrap().unwrap();
        release(&registry, &replacement).unwrap();
        control(&mut registry, &id, "running").unwrap();
        fence(&registry.connection, &resumed, now + 103).unwrap();
        release(&registry, &resumed).unwrap();
        assert_eq!(status(&registry, &id).unwrap()["lease_active"], false);
        registry.integrity().unwrap();
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn child_publication_before_queue_update_recovers_without_new_experiment_attempts() {
        let (root, spec) = fixture("child-recovery");
        let costs = Costs::default();
        let search = run(&root, &spec, &costs);
        let config = configuration(&search).unwrap();
        let db = root.join("state.sqlite");
        let mut registry = Registry::open(&db).unwrap();
        let id = register(&mut registry, &config).unwrap();
        let lease = claim(
            &mut registry,
            &id,
            chrono::Utc::now().timestamp_millis(),
            60000,
        )
        .unwrap()
        .unwrap();
        admit(&mut registry, &lease, &config).unwrap();
        let (_, _, _, _, _, jobs) = read(&registry.connection, &id).unwrap();
        let runs: Vec<_> = jobs
            .iter()
            .map(|j| CorpusRun {
                base: &root,
                spec: &spec,
                strategy: &j.definition.strategy,
                costs: &costs,
                budget: config.budget,
            })
            .collect();
        let limits = CorpusBatchLimits {
            workers: 2,
            max_parallel_candles: 20000,
            max_parallel_feature_cells: 2000000,
            max_parallel_receipts: 20000,
            max_new_chunks: 64,
            lease_ms: 60000,
        };
        tick_engine::execute_corpus_batch(&db, &runs, limits).unwrap();
        let attempts = registry.counts().unwrap().attempts;
        registry
            .connection
            .execute(
                "UPDATE corpus_searches SET expires_ms=?2 WHERE id=?1",
                params![id, chrono::Utc::now().timestamp_millis() - 1],
            )
            .unwrap();
        drop(registry);
        let recovered = step(&db, search, limits).unwrap();
        assert_eq!(recovered.status["slot_states"]["completed"], 2);
        let registry = Registry::open(&db).unwrap();
        assert_eq!(registry.counts().unwrap().attempts, attempts);
        registry.integrity().unwrap();
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn job_identity_corruption_and_cursor_changes_refuse_status_and_backup_integrity() {
        let (root, spec) = fixture("corruption");
        let costs = Costs::default();
        let config = configuration(&run(&root, &spec, &costs)).unwrap();
        let mut registry = Registry::open(&root.join("state.sqlite")).unwrap();
        let id = register(&mut registry, &config).unwrap();
        let lease = claim(
            &mut registry,
            &id,
            chrono::Utc::now().timestamp_millis(),
            60000,
        )
        .unwrap()
        .unwrap();
        admit(&mut registry, &lease, &config).unwrap();
        assert!(
            registry
                .connection
                .execute(
                    "UPDATE corpus_search_jobs SET hash=?2 WHERE search_id=?1",
                    params![id, "0".repeat(64)]
                )
                .is_err()
        );
        registry.integrity().unwrap();
        registry
            .connection
            .execute(
                "UPDATE corpus_searches SET cursor=cursor+1 WHERE id=?1",
                [&id],
            )
            .unwrap();
        assert!(status(&registry, &id).is_err());
        registry
            .connection
            .execute(
                "UPDATE corpus_searches SET cursor=cursor-1 WHERE id=?1",
                [&id],
            )
            .unwrap();
        registry.integrity().unwrap();
        registry
            .connection
            .execute_batch("DROP TRIGGER corpus_search_job_immutable")
            .unwrap();
        registry
            .connection
            .execute(
                "UPDATE corpus_search_jobs SET hash=?2 WHERE search_id=?1",
                params![id, "0".repeat(64)],
            )
            .unwrap();
        assert!(status(&registry, &id).is_err());
        assert!(registry.integrity().is_err());
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    fn failed_slots(
        name: &str,
        slots: usize,
    ) -> (
        std::path::PathBuf,
        crate::corpus::Specification,
        Costs,
        String,
    ) {
        let (root, spec) = fixture(name);
        let costs = Costs::default();
        let mut search = run(&root, &spec, &costs);
        search.maximum_candidates = slots;
        search.queue_width = slots;
        let config = configuration(&search).unwrap();
        let mut registry = Registry::open(&root.join("state.sqlite")).unwrap();
        let id = register(&mut registry, &config).unwrap();
        let lease = claim(
            &mut registry,
            &id,
            chrono::Utc::now().timestamp_millis(),
            60000,
        )
        .unwrap()
        .unwrap();
        admit(&mut registry, &lease, &config).unwrap();
        release(&registry, &lease).unwrap();
        registry.connection.execute("UPDATE corpus_search_jobs SET state='error',error='fixture worker interruption' WHERE search_id=?1",[&id]).unwrap();
        control(&mut registry, &id, "paused").unwrap();
        registry.integrity().unwrap();
        drop(registry);
        (root, spec, costs, id)
    }
    #[test]
    fn requeue_budget_retains_prior_errors_in_immutable_verified_history_and_backups() {
        let (root, _spec, _costs, id) = failed_slots("requeue-budget", 1);
        let db = root.join("state.sqlite");
        let mut registry = Registry::open(&db).unwrap();
        for number in 1..=3 {
            let result = requeue_errors(&mut registry, &id, 1).unwrap();
            assert_eq!(result["requeued_slots"], serde_json::json!(["0"]));
            let snapshot = status(&registry, &id).unwrap();
            assert_eq!(snapshot["requeue_records"], number.to_string());
            assert_eq!(snapshot["lease_active"], false);
            assert_eq!(snapshot["qualification_granted"], false);
            registry
                .connection
                .execute(
                    "UPDATE corpus_search_jobs SET state='error',error=?2 WHERE search_id=?1",
                    params![id, format!("fixture subsequent interruption {number}")],
                )
                .unwrap();
            control(&mut registry, &id, "paused").unwrap();
        }
        let before = status(&registry, &id).unwrap();
        registry.connection.execute_batch("VACUUM").unwrap();
        assert_eq!(status(&registry, &id).unwrap(), before);
        assert_eq!(before["retry_eligible_slots"], "0");
        assert!(requeue_errors(&mut registry, &id, 1).is_err());
        assert_eq!(status(&registry, &id).unwrap(), before);
        assert!(
            registry
                .connection
                .execute(
                    "DELETE FROM corpus_search_requeues WHERE search_id=?1",
                    [&id]
                )
                .is_err()
        );
        assert!(
            registry
                .connection
                .execute(
                    "UPDATE corpus_search_requeues SET hash=?2 WHERE search_id=?1",
                    params![id, "0".repeat(64)]
                )
                .is_err()
        );
        let backup = root.join("backup.sqlite");
        registry.backup(&backup).unwrap();
        let restored = Registry::open(&backup).unwrap();
        assert_eq!(status(&restored, &id).unwrap(), before);
        restored.integrity().unwrap();
        drop(restored);
        registry
            .connection
            .execute_batch("DROP TRIGGER corpus_search_requeue_no_update")
            .unwrap();
        let body:String=registry.connection.query_row("SELECT body FROM corpus_search_requeues WHERE search_id=?1 AND slot=0 AND sequence=1",[&id],|r|r.get(0)).unwrap();
        let mut record: RequeueRecord = serde_json::from_str(&body).unwrap();
        record.prior_error = "mechanically hash-consistent forgery".into();
        registry.connection.execute("UPDATE corpus_search_requeues SET body=?2,hash=?3 WHERE search_id=?1 AND slot=0 AND sequence=1",params![id,serde_json::to_string(&record).unwrap(),data::hash(&record).unwrap()]).unwrap();
        assert!(status(&registry, &id).is_err());
        assert!(registry.integrity().is_err());
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn concurrent_requeue_requests_have_one_atomic_winner() {
        let (root, _spec, _costs, id) = failed_slots("requeue-race", 1);
        let db = root.join("state.sqlite");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let db = db.clone();
                let id = id.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let mut registry = Registry::open(&db).unwrap();
                    barrier.wait();
                    requeue_errors(&mut registry, &id, 1).is_ok()
                })
            })
            .collect();
        assert_eq!(
            handles
                .into_iter()
                .filter_map(|h| h.join().unwrap().then_some(()))
                .count(),
            1
        );
        let registry = Registry::open(&db).unwrap();
        let snapshot = status(&registry, &id).unwrap();
        assert_eq!(snapshot["requeue_records"], "1");
        assert_eq!(snapshot["slot_states"]["queued"], 1);
        assert_eq!(snapshot["generated_slots"], "1");
        registry.integrity().unwrap();
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn requeue_preserves_active_child_leases_and_yields_busy_without_spinning() {
        let (root, spec, costs, id) = failed_slots("requeue-child", 1);
        let db = root.join("state.sqlite");
        let mut registry = Registry::open(&db).unwrap();
        let (_, _, _, _, _, jobs) = read(&registry.connection, &id).unwrap();
        let experiment = registry.register(&jobs[0].definition.key).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let child = match registry.claim(&experiment, now, 60000).unwrap() {
            crate::storage::Claim::Acquired(lease) => lease,
            _ => panic!("fresh child lease"),
        };
        requeue_errors(&mut registry, &id, 1).unwrap();
        registry
            .validate_lease(&child, chrono::Utc::now().timestamp_millis())
            .unwrap();
        let mut search = run(&root, &spec, &costs);
        search.maximum_candidates = 1;
        search.queue_width = 1;
        let limits = CorpusBatchLimits {
            workers: 1,
            max_parallel_candles: 10000,
            max_parallel_feature_cells: 1000000,
            max_parallel_receipts: 10000,
            max_new_chunks: 64,
            lease_ms: 60000,
        };
        let progress = step(&db, search, limits).unwrap();
        assert_eq!(progress.kind, "held-or-busy");
        assert_eq!(progress.status["slot_states"]["pending"], 1);
        registry
            .validate_lease(&child, chrono::Utc::now().timestamp_millis())
            .unwrap();
        assert_eq!(registry.counts().unwrap().attempts, 1);
        registry.integrity().unwrap();
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn requeue_of_published_child_receipts_recovers_without_new_attempts() {
        let (root, spec) = fixture("requeue-published");
        let costs = Costs::default();
        let mut search = run(&root, &spec, &costs);
        search.maximum_candidates = 2;
        let config = configuration(&search).unwrap();
        let db = root.join("state.sqlite");
        let mut registry = Registry::open(&db).unwrap();
        let id = register(&mut registry, &config).unwrap();
        let lease = claim(
            &mut registry,
            &id,
            chrono::Utc::now().timestamp_millis(),
            60000,
        )
        .unwrap()
        .unwrap();
        admit(&mut registry, &lease, &config).unwrap();
        let (_, _, _, _, _, jobs) = read(&registry.connection, &id).unwrap();
        let runs: Vec<_> = jobs
            .iter()
            .map(|j| CorpusRun {
                base: &root,
                spec: &spec,
                strategy: &j.definition.strategy,
                costs: &costs,
                budget: config.budget,
            })
            .collect();
        let limits = CorpusBatchLimits {
            workers: 2,
            max_parallel_candles: 20000,
            max_parallel_feature_cells: 2000000,
            max_parallel_receipts: 20000,
            max_new_chunks: 64,
            lease_ms: 60000,
        };
        tick_engine::execute_corpus_batch(&db, &runs, limits).unwrap();
        let attempts = registry.counts().unwrap().attempts;
        release(&registry, &lease).unwrap();
        registry.connection.execute("UPDATE corpus_search_jobs SET state='error',error='worker interrupted after child publication' WHERE search_id=?1",[&id]).unwrap();
        control(&mut registry, &id, "paused").unwrap();
        let requeued = requeue_errors(&mut registry, &id, 2).unwrap();
        assert_eq!(requeued["requeued_slots"].as_array().unwrap().len(), 2);
        drop(registry);
        let recovered = step(&db, search, limits).unwrap();
        assert_eq!(recovered.kind, "exhausted");
        assert_eq!(recovered.status["requeue_records"], "2");
        assert_eq!(recovered.status["slot_states"]["completed"], 2);
        let registry = Registry::open(&db).unwrap();
        assert_eq!(registry.counts().unwrap().attempts, attempts);
        registry.integrity().unwrap();
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn requeue_respects_frozen_width_when_other_slots_are_pending() {
        let (root, _spec, _costs, id) = failed_slots("requeue-capacity", 2);
        let mut registry = Registry::open(&root.join("state.sqlite")).unwrap();
        registry.connection.execute("UPDATE corpus_search_jobs SET state='pending',error=NULL WHERE search_id=?1 AND slot=1",[&id]).unwrap();
        let result = requeue_errors(&mut registry, &id, 32).unwrap();
        assert_eq!(result["requeued_slots"], serde_json::json!(["0"]));
        let snapshot = status(&registry, &id).unwrap();
        assert_eq!(snapshot["slot_states"]["queued"], 1);
        assert_eq!(snapshot["slot_states"]["pending"], 1);
        assert_eq!(snapshot["retry_capacity"], "0");
        control(&mut registry, &id, "paused").unwrap();
        assert!(requeue_errors(&mut registry, &id, 32).is_err());
        assert_eq!(status(&registry, &id).unwrap()["requeue_records"], "1");
        registry.integrity().unwrap();
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
}
