//! Frozen opt-in retry admission policy; no worker process is launched here.
use crate::{Result, data, storage::Registry};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub backoff_ms: i64,
    pub maximum_slots: usize,
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        if !(1..=86_400_000).contains(&self.backoff_ms) || !(1..=32).contains(&self.maximum_slots) {
            return Err("retry policy requires1..86400000 backoff ms and1..32 slots".into());
        }
        Ok(())
    }
}
pub(crate) fn initialize(c: &mut Connection, migrate: bool) -> Result<()> {
    let tx = c
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS corpus_retry_policies(search_id TEXT PRIMARY KEY REFERENCES corpus_searches(id),body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=4096),hash TEXT NOT NULL CHECK(length(hash)=64));CREATE TRIGGER IF NOT EXISTS retry_policy_no_update BEFORE UPDATE ON corpus_retry_policies BEGIN SELECT RAISE(ABORT,'retry policy immutable');END;CREATE TRIGGER IF NOT EXISTS retry_policy_no_delete BEFORE DELETE ON corpus_retry_policies BEGIN SELECT RAISE(ABORT,'retry policy immutable');END;").map_err(|e|e.to_string())?;
    if migrate {
        tx.execute_batch(
            "UPDATE corpus_searches SET token=NULL,expires_ms=NULL;PRAGMA user_version=13;",
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}
pub fn configure(registry: &mut Registry, id: &str, policy: &Policy) -> Result<String> {
    policy.validate()?;
    let tx = registry
        .connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let head = crate::search_control::read(&tx, id)?;
    if !matches!(head.state.as_str(), "running" | "paused") {
        return Err("terminal search cannot install retry policy".into());
    }
    let hash = data::hash(&("retry-policy-v1", id, policy))?;
    if let Some((previous, old_hash)) = read(&tx, id)? {
        if previous != *policy || old_hash != hash {
            return Err("immutable retry policy differs".into());
        }
    } else {
        tx.execute(
            "INSERT INTO corpus_retry_policies(search_id,body,hash) VALUES(?1,?2,?3)",
            params![
                id,
                serde_json::to_string(policy).map_err(|e| e.to_string())?,
                hash
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(hash)
}
pub(crate) fn read(c: &Connection, id: &str) -> Result<Option<(Policy, String)>> {
    let row:Option<(String,String)>=c.query_row("SELECT CASE WHEN length(CAST(body AS BLOB))<=4096 THEN body END,hash FROM corpus_retry_policies WHERE search_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
    row.map(|(body, hash)| {
        let policy: Policy = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        policy.validate()?;
        if data::hash(&("retry-policy-v1", id, &policy))? != hash {
            return Err("retry policy hash mismatch".into());
        }
        Ok((policy, hash))
    })
    .transpose()
}
pub(crate) fn integrity(c: &Connection) -> Result<()> {
    let ids = {
        let mut q = c
            .prepare("SELECT search_id FROM corpus_retry_policies LIMIT 65")
            .map_err(|e| e.to_string())?;
        q.query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    if ids.len() > 64 {
        return Err("retry policy quota exceeded".into());
    }
    for id in ids {
        read(c, &id)?;
    }
    Ok(())
}
/// One atomic decision. Callers supply a clock for deterministic verification;
/// CLI/HTTP must use their own clock, never a browser-supplied timestamp.
pub fn admit(
    registry: &mut Registry,
    id: &str,
    expected_epoch: i64,
    now: i64,
) -> Result<serde_json::Value> {
    crate::corpus_search::retry_admit(registry, id, expected_epoch, now)
}
