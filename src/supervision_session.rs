//! Foreground invocation observations and owner-fenced completion, not PID liveness.
use crate::{Result, data, storage::Registry};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
#[derive(Clone)]
pub struct Owner {
    pub id: String,
    token: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    version: String,
    id: String,
    search_id: String,
    pid: u32,
    started_at_ms: i64,
    updated_at_ms: i64,
    finished_at_ms: Option<i64>,
    maximum_rounds: usize,
    maximum_wait_ms: u64,
    round_attempts: usize,
    checks: usize,
    waited_ms: u64,
    exit_reason: Option<String>,
}
type Row = (Body, Option<String>, Option<i64>, Option<String>);
fn valid_time(n: i64) -> bool {
    chrono::DateTime::from_timestamp_millis(n).is_some()
}
pub(crate) fn initialize(c: &mut Connection, migrate: bool) -> Result<()> {
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS corpus_supervision_sessions(id TEXT PRIMARY KEY,search_id TEXT NOT NULL REFERENCES corpus_searches(id),token TEXT,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=4096),hash TEXT NOT NULL CHECK(length(hash)=64),stop_at_ms INTEGER);CREATE TABLE IF NOT EXISTS corpus_supervision_receipts(id TEXT PRIMARY KEY REFERENCES corpus_supervision_sessions(id),body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=4096),hash TEXT NOT NULL CHECK(length(hash)=64));CREATE TRIGGER IF NOT EXISTS supervision_session_no_delete BEFORE DELETE ON corpus_supervision_sessions BEGIN SELECT RAISE(ABORT,'session retained');END;CREATE TRIGGER IF NOT EXISTS supervision_session_identity BEFORE UPDATE OF id,search_id ON corpus_supervision_sessions BEGIN SELECT RAISE(ABORT,'session identity immutable');END;CREATE TRIGGER IF NOT EXISTS supervision_session_terminal BEFORE UPDATE ON corpus_supervision_sessions WHEN json_extract(OLD.body,'$.finished_at_ms') IS NOT NULL BEGIN SELECT RAISE(ABORT,'terminal session immutable');END;CREATE TRIGGER IF NOT EXISTS supervision_receipt_no_update BEFORE UPDATE ON corpus_supervision_receipts BEGIN SELECT RAISE(ABORT,'session receipt immutable');END;CREATE TRIGGER IF NOT EXISTS supervision_receipt_no_delete BEFORE DELETE ON corpus_supervision_receipts BEGIN SELECT RAISE(ABORT,'session receipt immutable');END;").map_err(|e|e.to_string())?;
    if migrate {
        tx.execute_batch(
            "UPDATE corpus_searches SET token=NULL,expires_ms=NULL;PRAGMA user_version=15;",
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}
fn read(c: &Connection, id: &str) -> Result<Row> {
    let (body,hash,source,token,stop):(String,String,String,Option<String>,Option<i64>)=c.query_row("SELECT CASE WHEN length(CAST(body AS BLOB))<=4096 THEN body END,hash,search_id,token,stop_at_ms FROM corpus_supervision_sessions WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
    let b: Body = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    if b.version != "foreground-session-v1"
        || b.id != id
        || b.search_id != source
        || b.pid == 0
        || !valid_time(b.started_at_ms)
        || !valid_time(b.updated_at_ms)
        || b.finished_at_ms.is_some_and(|n| !valid_time(n))
        || stop.is_some_and(|n| !valid_time(n))
        || !(1..=10000).contains(&b.maximum_rounds)
        || b.maximum_wait_ms > 3_600_000
        || b.round_attempts > b.maximum_rounds
        || b.checks > b.maximum_rounds * 2 + 1
        || b.exit_reason
            .as_ref()
            .is_some_and(|s| s.chars().count() > 128)
        || data::hash(&b)? != hash
        || b.finished_at_ms.is_some() != b.exit_reason.is_some()
        || b.finished_at_ms.is_some() == token.is_some()
    {
        return Err("supervision session integrity mismatch".into());
    }
    let receipt:Option<(String,String)>=c.query_row("SELECT CASE WHEN length(CAST(body AS BLOB))<=4096 THEN body END,hash FROM corpus_supervision_receipts WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
    let expected = serde_json::json!({"session":b,"stop_requested_at_ms":stop});
    let receipt_hash = if let Some((body, hash)) = receipt {
        let value: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        if b.finished_at_ms.is_none() || value != expected || data::hash(&value)? != hash {
            return Err("terminal session receipt mismatch".into());
        }
        Some(hash)
    } else {
        if b.finished_at_ms.is_some() {
            return Err("finished session receipt missing".into());
        }
        None
    };
    Ok((b, token, stop, receipt_hash))
}
pub fn start(
    registry: &mut Registry,
    source: &str,
    pid: u32,
    maximum_rounds: usize,
    maximum_wait_ms: u64,
    now: i64,
) -> Result<Owner> {
    if pid == 0
        || !(1..=10000).contains(&maximum_rounds)
        || maximum_wait_ms > 3_600_000
        || !valid_time(now)
    {
        return Err("invalid session admission".into());
    }
    let tx = registry
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    crate::search_control::read(&tx, source)?;
    let count: usize = tx
        .query_row(
            "SELECT count(*) FROM corpus_supervision_sessions",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if count >= 128 {
        return Err("foreground session history full; preserve evidence".into());
    }
    let (id, token): (String, String) = tx
        .query_row(
            "SELECT lower(hex(randomblob(32))),lower(hex(randomblob(32)))",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let b = Body {
        version: "foreground-session-v1".into(),
        id: id.clone(),
        search_id: source.into(),
        pid,
        started_at_ms: now,
        updated_at_ms: now,
        finished_at_ms: None,
        maximum_rounds,
        maximum_wait_ms,
        round_attempts: 0,
        checks: 0,
        waited_ms: 0,
        exit_reason: None,
    };
    tx.execute("INSERT INTO corpus_supervision_sessions(id,search_id,token,body,hash) VALUES(?1,?2,?3,?4,?5)",params![id,source,token,serde_json::to_string(&b).map_err(|e|e.to_string())?,data::hash(&b)?]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(Owner { id, token })
}
fn update(
    registry: &mut Registry,
    owner: &Owner,
    rounds: usize,
    checks: usize,
    waited: u64,
    now: i64,
    reason: Option<&str>,
) -> Result<()> {
    let tx = registry
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let (mut b, token, stop, _) = read(&tx, &owner.id)?;
    if token.as_deref() != Some(&owner.token)
        || !valid_time(now)
        || rounds < b.round_attempts
        || checks < b.checks
        || waited < b.waited_ms
        || rounds > b.maximum_rounds
        || checks > b.maximum_rounds * 2 + 1
        || reason.is_some_and(|s| s.chars().count() > 128)
    {
        return Err("session owner/counter fence refused".into());
    }
    b.round_attempts = rounds;
    b.checks = checks;
    b.waited_ms = waited;
    b.updated_at_ms = now;
    if let Some(reason) = reason {
        b.finished_at_ms = Some(now);
        b.exit_reason = Some(reason.into());
    }
    tx.execute("UPDATE corpus_supervision_sessions SET body=?3,hash=?4,token=CASE WHEN ?5 THEN NULL ELSE token END WHERE id=?1 AND token=?2",params![owner.id,owner.token,serde_json::to_string(&b).map_err(|e|e.to_string())?,data::hash(&b)?,reason.is_some()]).map_err(|e|e.to_string())?;
    if reason.is_some() {
        let value = serde_json::json!({"session":b,"stop_requested_at_ms":stop});
        tx.execute(
            "INSERT INTO corpus_supervision_receipts(id,body,hash) VALUES(?1,?2,?3)",
            params![
                owner.id,
                serde_json::to_string(&value).map_err(|e| e.to_string())?,
                data::hash(&value)?
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}
pub fn heartbeat(
    registry: &mut Registry,
    owner: &Owner,
    rounds: usize,
    checks: usize,
    waited: u64,
    now: i64,
) -> Result<()> {
    update(registry, owner, rounds, checks, waited, now, None)
}
pub fn finish(
    registry: &mut Registry,
    owner: &Owner,
    rounds: usize,
    checks: usize,
    waited: u64,
    now: i64,
    reason: &str,
) -> Result<()> {
    update(registry, owner, rounds, checks, waited, now, Some(reason))
}
pub fn stop_requested(registry: &Registry, owner: &Owner) -> Result<bool> {
    let (_, token, stop, _) = read(&registry.connection, &owner.id)?;
    if token.as_deref() != Some(&owner.token) {
        return Err("session owner fenced".into());
    }
    Ok(stop.is_some())
}
pub fn request_stop(registry: &mut Registry, id: &str, now: i64) -> Result<bool> {
    if !valid_time(now) {
        return Err("invalid stop observation time".into());
    }
    let tx = registry
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let (b, _, stop, _) = read(&tx, id)?;
    if b.finished_at_ms.is_some() || stop.is_some() {
        return Ok(false);
    }
    tx.execute(
        "UPDATE corpus_supervision_sessions SET stop_at_ms=?2 WHERE id=?1",
        params![id, now],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(true)
}
pub fn status(registry: &Registry, id: &str) -> Result<serde_json::Value> {
    let tx = registry
        .connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let (b, _, stop, receipt) = read(&tx, id)?;
    let mut value = serde_json::to_value(&b).map_err(|e| e.to_string())?;
    for field in [
        "pid",
        "started_at_ms",
        "updated_at_ms",
        "maximum_rounds",
        "maximum_wait_ms",
        "round_attempts",
        "checks",
        "waited_ms",
    ] {
        value[field] = value[field].to_string().into();
    }
    value["finished_at_ms"] = b.finished_at_ms.map(|n| n.to_string()).into();
    value["stop_requested_at_ms"] = stop.map(|n| n.to_string()).into();
    value["receipt_hash"] = receipt.into();
    value["observation"] = if b.finished_at_ms.is_some() {
        "finished"
    } else {
        "unclosed; PID liveness not established"
    }
    .into();
    tx.commit().map_err(|e| e.to_string())?;
    Ok(value)
}
pub fn list(registry: &Registry) -> Result<Vec<serde_json::Value>> {
    let ids = {
        let mut q = registry
            .connection
            .prepare("SELECT id FROM corpus_supervision_sessions ORDER BY rowid DESC LIMIT 129")
            .map_err(|e| e.to_string())?;
        q.query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    if ids.len() > 128 {
        return Err("session quota exceeded".into());
    }
    ids.into_iter().map(|id| status(registry, &id)).collect()
}
pub(crate) fn integrity(registry: &Registry) -> Result<()> {
    list(registry)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> (std::path::PathBuf, Registry, String) {
        let root = std::env::temp_dir().join(format!("session-unit-{name}-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let data = crate::data::synthetic_fixture("EURUSD").unwrap();
        let path = root.join("input.rsf");
        crate::native::write(&path, &data).unwrap();
        let spec = crate::corpus::describe(&[path], 10000).unwrap();
        let costs = crate::tick_engine::Costs::default();
        let run = crate::corpus_search::SearchRun {
            base: &root,
            specification: &spec,
            costs: &costs,
            budget: crate::tick_engine::CorpusBudget {
                max_candles: 10000,
                feature_cells: 1000000,
                max_receipts: 10000,
            },
            seed: 42,
            maximum_candidates: 1,
            queue_width: 1,
        };
        let db = root.join("state.sqlite");
        let id = crate::corpus_search::prepare_supervision(
            &db,
            &run,
            &crate::retry_policy::Policy {
                backoff_ms: 1,
                maximum_slots: 1,
            },
        )
        .unwrap();
        let registry = Registry::open(&db).unwrap();
        (root, registry, id)
    }
    #[test]
    fn owner_counter_stop_and_terminal_receipt_fences_survive_backup() {
        let (root, mut registry, id) = fixture("fence");
        let owner = start(&mut registry, &id, 123, 10, 1000, 1000).unwrap();
        let forged = Owner {
            id: owner.id.clone(),
            token: "not-owner".into(),
        };
        assert!(heartbeat(&mut registry, &forged, 0, 0, 0, 1001).is_err());
        heartbeat(&mut registry, &owner, 1, 2, 10, 1002).unwrap();
        assert!(heartbeat(&mut registry, &owner, 0, 2, 10, 1003).is_err());
        assert!(request_stop(&mut registry, &owner.id, 1004).unwrap());
        assert!(stop_requested(&registry, &owner).unwrap());
        finish(
            &mut registry,
            &owner,
            1,
            2,
            10,
            1005,
            "session-stop-request",
        )
        .unwrap();
        let snapshot = status(&registry, &owner.id).unwrap();
        assert_eq!(snapshot["observation"], "finished");
        assert!(!snapshot.to_string().contains("token"));
        assert!(snapshot["receipt_hash"].is_string());
        assert!(heartbeat(&mut registry, &owner, 2, 3, 10, 1006).is_err());
        assert!(!request_stop(&mut registry, &owner.id, 1006).unwrap());
        assert!(
            registry
                .connection
                .execute(
                    "UPDATE corpus_supervision_sessions SET token='x' WHERE id=?1",
                    [&owner.id]
                )
                .is_err()
        );
        registry.backup(&root.join("backup.sqlite")).unwrap();
        let restored = Registry::open(&root.join("backup.sqlite")).unwrap();
        assert_eq!(status(&restored, &owner.id).unwrap(), snapshot);
        restored.integrity().unwrap();
        drop(restored);
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failed_terminal_receipt_insert_rolls_back_and_unclosed_is_not_liveness() {
        let (root, mut registry, id) = fixture("rollback");
        let owner = start(&mut registry, &id, 123, 10, 0, 1000).unwrap();
        assert!(
            status(&registry, &owner.id).unwrap()["observation"]
                .as_str()
                .unwrap()
                .contains("liveness not established")
        );
        registry.connection.execute_batch("CREATE TRIGGER reject_session_receipt BEFORE INSERT ON corpus_supervision_receipts BEGIN SELECT RAISE(ABORT,'injected receipt refusal');END;").unwrap();
        assert!(finish(&mut registry, &owner, 0, 0, 0, 1001, "test").is_err());
        assert!(status(&registry, &owner.id).unwrap()["finished_at_ms"].is_null());
        assert!(!stop_requested(&registry, &owner).unwrap());
        registry
            .connection
            .execute_batch("DROP TRIGGER reject_session_receipt;")
            .unwrap();
        finish(&mut registry, &owner, 0, 0, 0, 1002, "test").unwrap();
        registry.integrity().unwrap();
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn full_session_inventory_blocks_new_start_but_keeps_stop_and_finish() {
        let (root, mut registry, id) = fixture("quota");
        let first = start(&mut registry, &id, 123, 1, 0, 1000).unwrap();
        for _ in 1..128 {
            start(&mut registry, &id, 123, 1, 0, 1000).unwrap();
        }
        assert!(start(&mut registry, &id, 123, 1, 0, 1000).is_err());
        assert!(request_stop(&mut registry, &first.id, 1001).unwrap());
        finish(&mut registry, &first, 0, 0, 0, 1002, "session-stop-request").unwrap();
        assert_eq!(list(&registry).unwrap().len(), 128);
        registry.integrity().unwrap();
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
}
