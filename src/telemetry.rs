//! Append-only telemetry and lineage for research control.
//!
//! Every stage transition is accompanied by a bounded telemetry event that binds
//! the experiment, scope, evidence hash and wall-clock time. Events are immutable
//! (update/delete triggers) and paged by a bounded cursor so dashboards can replay
//! an exact ordered feed without unbounded memory growth.
use crate::{Result, data::hash};
use rusqlite::OptionalExtension;

/// Maximum telemetry rows returned by one page.
pub const PAGE_LIMIT: i64 = 200;

/// Schema initializer, invoked from the Registry bootstrap.
pub fn initialize(connection: &rusqlite::Connection) -> Result<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS telemetry_events(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                experiment_id TEXT NOT NULL REFERENCES experiments(id),
                scope TEXT NOT NULL CHECK(length(scope)>0 AND length(scope)<=64),
                event TEXT NOT NULL CHECK(length(event)>0 AND length(event)<=64),
                at_ms INTEGER NOT NULL,
                payload TEXT NOT NULL CHECK(length(payload)<=8192),
                decision_hash TEXT NOT NULL CHECK(length(decision_hash)=64)
            );
            CREATE INDEX IF NOT EXISTS telemetry_experiment ON telemetry_events(experiment_id,id);
            CREATE TRIGGER IF NOT EXISTS telemetry_no_update BEFORE UPDATE ON telemetry_events BEGIN SELECT RAISE(ABORT,'telemetry immutable');END;
            CREATE TRIGGER IF NOT EXISTS telemetry_no_delete BEFORE DELETE ON telemetry_events BEGIN SELECT RAISE(ABORT,'telemetry immutable');END;",
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Event {
    pub id: i64,
    pub experiment_id: String,
    pub scope: String,
    pub event: String,
    pub at_ms: i64,
    pub payload: serde_json::Value,
    pub decision_hash: String,
}

fn digest(payload: &serde_json::Value) -> Result<String> {
    let bytes = serde_json::to_vec(payload).map_err(|e| e.to_string())?;
    hash(&("telemetry-v1", bytes))
}

/// Append one immutable telemetry event for an experiment.
pub fn record(
    registry: &crate::storage::Registry,
    experiment_id: &str,
    scope: &str,
    event: &str,
    at_ms: i64,
    payload: &serde_json::Value,
) -> Result<i64> {
    let decision_hash = digest(payload)?;
    let tx = crate::storage::writer_tx(&registry.connection).map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO telemetry_events(experiment_id,scope,event,at_ms,payload,decision_hash)
         VALUES (?1,?2,?3,?4,?5,?6)",
        rusqlite::params![
            experiment_id,
            scope,
            event,
            at_ms,
            serde_json::to_string(payload).map_err(|e| e.to_string())?,
            decision_hash
        ],
    )
    .map_err(|e| e.to_string())?;
    let id = tx.last_insert_rowid();
    tx.commit().map_err(|e| e.to_string())?;
    Ok(id)
}

/// Bounded ordered page of events for one experiment after `after_id`.
pub fn page(
    registry: &crate::storage::Registry,
    experiment_id: &str,
    after_id: i64,
) -> Result<Vec<Event>> {
    if !(0..=1_000_000_000).contains(&after_id) {
        return Err("invalid telemetry cursor".into());
    }
    let mut stmt = registry
        .connection
        .prepare(
            "SELECT id,experiment_id,scope,event,at_ms,payload,decision_hash
             FROM telemetry_events WHERE experiment_id=?1 AND id>?2
             ORDER BY id LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            rusqlite::params![experiment_id, after_id, PAGE_LIMIT],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (id, experiment_id, scope, event, at_ms, payload, decision_hash) in rows {
        let payload: serde_json::Value =
            serde_json::from_str(&payload).map_err(|e| e.to_string())?;
        out.push(Event {
            id,
            experiment_id,
            scope,
            event,
            at_ms,
            payload,
            decision_hash,
        });
    }
    Ok(out)
}

/// Total number of events recorded for one experiment.
pub fn count(registry: &crate::storage::Registry, experiment_id: &str) -> Result<i64> {
    registry
        .connection
        .query_row(
            "SELECT COUNT(*) FROM telemetry_events WHERE experiment_id=?1",
            [experiment_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
}

/// Latest event id for an experiment (0 when none).
pub fn latest_id(registry: &crate::storage::Registry, experiment_id: &str) -> Result<i64> {
    let id: Option<i64> = registry
        .connection
        .query_row(
            "SELECT MAX(id) FROM telemetry_events WHERE experiment_id=?1",
            [experiment_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    Ok(id.unwrap_or(0))
}
