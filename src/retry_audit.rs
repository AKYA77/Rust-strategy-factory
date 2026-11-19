//! Immutable bounded admission observations with a verified append head.
use crate::{Result, data};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
const PER_SEARCH: usize = 128;
const GLOBAL: usize = 4096;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Head {
    search_id: String,
    since_epoch: i64,
    count: usize,
    last_hash: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    version: String,
    search_id: String,
    sequence: usize,
    at_ms: i64,
    result_epoch: i64,
    previous_hash: String,
    decision: serde_json::Value,
}
fn put(c: &Connection, h: &Head) -> Result<()> {
    c.execute("INSERT INTO corpus_retry_audit_heads(search_id,body,hash) VALUES(?1,?2,?3) ON CONFLICT(search_id) DO UPDATE SET body=excluded.body,hash=excluded.hash",params![h.search_id,serde_json::to_string(h).map_err(|e|e.to_string())?,data::hash(h)?]).map_err(|e|e.to_string())?;
    Ok(())
}
pub(crate) fn register(c: &Connection, id: &str) -> Result<()> {
    let epoch = crate::search_control::read(c, id)?.epoch;
    put(
        c,
        &Head {
            search_id: id.into(),
            since_epoch: epoch,
            count: 0,
            last_hash: String::new(),
        },
    )
}
pub(crate) fn initialize(c: &mut Connection, migrate: bool) -> Result<()> {
    let tx = crate::storage::writer_tx_open(c).map_err(|e| e.to_string())?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS corpus_retry_audit_heads(search_id TEXT PRIMARY KEY REFERENCES corpus_searches(id),body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=4096),hash TEXT NOT NULL CHECK(length(hash)=64));CREATE TABLE IF NOT EXISTS corpus_retry_decisions(search_id TEXT NOT NULL REFERENCES corpus_searches(id),sequence INTEGER NOT NULL CHECK(sequence>0),body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=4096),hash TEXT NOT NULL CHECK(length(hash)=64),PRIMARY KEY(search_id,sequence));CREATE TRIGGER IF NOT EXISTS retry_decision_no_update BEFORE UPDATE ON corpus_retry_decisions BEGIN SELECT RAISE(ABORT,'retry decision immutable');END;CREATE TRIGGER IF NOT EXISTS retry_decision_no_delete BEFORE DELETE ON corpus_retry_decisions BEGIN SELECT RAISE(ABORT,'retry decision immutable');END;CREATE TRIGGER IF NOT EXISTS retry_audit_head_no_delete BEFORE DELETE ON corpus_retry_audit_heads BEGIN SELECT RAISE(ABORT,'retry audit head retained');END;CREATE TRIGGER IF NOT EXISTS retry_audit_head_key BEFORE UPDATE OF search_id ON corpus_retry_audit_heads BEGIN SELECT RAISE(ABORT,'retry audit head key immutable');END;").map_err(|e|e.to_string())?;
    if migrate {
        let ids = {
            let mut q = tx
                .prepare("SELECT id FROM corpus_searches LIMIT 65")
                .map_err(|e| e.to_string())?;
            q.query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        if ids.len() > 64 {
            return Err("retry audit migration count exceeded".into());
        }
        for id in ids {
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM corpus_retry_audit_heads WHERE search_id=?1)",
                    [&id],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !exists {
                register(&tx, &id)?;
            }
        }
        tx.execute_batch(
            "UPDATE corpus_searches SET token=NULL,expires_ms=NULL;PRAGMA user_version=14;",
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}
fn read(c: &Connection, id: &str) -> Result<(Head, Vec<(Event, String)>)> {
    let row:Option<(String,String)>=c.query_row("SELECT CASE WHEN length(CAST(body AS BLOB))<=4096 THEN body END,hash FROM corpus_retry_audit_heads WHERE search_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
    let (body, hash) = row.ok_or("retry audit head missing")?;
    let h: Head = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let current = crate::search_control::read(c, id)?.epoch;
    if h.search_id != id
        || h.since_epoch < 0
        || h.since_epoch > current
        || h.count > PER_SEARCH
        || data::hash(&h)? != hash
    {
        return Err("retry audit head mismatch".into());
    }
    let mut q=c.prepare("SELECT sequence,CASE WHEN length(CAST(body AS BLOB))<=4096 THEN body END,hash FROM corpus_retry_decisions WHERE search_id=?1 ORDER BY sequence LIMIT 129").map_err(|e|e.to_string())?;
    let rows = q
        .query_map([id], |r| {
            Ok((
                r.get::<_, usize>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut events: Vec<(Event, String)> = Vec::new();
    for row in rows {
        let (sequence, body, hash) = row.map_err(|e| e.to_string())?;
        let e: Event = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let kind = e.decision["kind"]
            .as_str()
            .ok_or("retry decision kind missing")?;
        if events.len() >= PER_SEARCH
            || sequence != events.len() + 1
            || e.sequence != sequence
            || e.version != "retry-decision-v1"
            || e.search_id != id
            || e.result_epoch < h.since_epoch
            || e.result_epoch > current
            || chrono::DateTime::from_timestamp_millis(e.at_ms).is_none()
            || e.previous_hash != events.last().map_or("", |(_, hash)| hash)
            || data::hash(&e)? != hash
            || e.decision["search_id"] != id
            || e.decision["workers_started"] != 0
            || !["held", "backoff", "requeued"].contains(&kind)
        {
            return Err("retry decision history mismatch".into());
        }
        let expected = e.decision["expected_epoch"]
            .as_str()
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or("retry expected epoch missing")?;
        if expected < 0
            || e.decision["epoch"]
                .as_str()
                .and_then(|s| s.parse::<i64>().ok())
                != Some(e.result_epoch)
            || kind == "requeued" && expected.checked_add(1) != Some(e.result_epoch)
        {
            return Err("retry decision epoch mismatch".into());
        }
        if let Some(hash) = e.decision["policy_hash"].as_str() {
            let (_, actual) =
                crate::retry_policy::read(c, id)?.ok_or("retry decision policy missing")?;
            if hash != actual {
                return Err("retry decision policy mismatch".into());
            }
        } else if matches!(kind, "requeued" | "backoff") {
            return Err("requeue decision lacks policy".into());
        }
        events.push((e, hash));
    }
    if h.count != events.len() || h.last_hash != events.last().map_or("", |(_, hash)| hash) {
        return Err("retry audit chain/head mismatch".into());
    }
    Ok((h, events))
}
pub(crate) fn capacity(c: &Connection, id: &str) -> Result<bool> {
    let (h, _) = read(c, id)?;
    let (count,bytes):(usize,usize)=c.query_row("SELECT count(*),coalesce(sum(length(CAST(body AS BLOB))),0) FROM corpus_retry_decisions",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
    Ok(h.count < PER_SEARCH && count < GLOBAL && bytes + 4096 <= 8 * 1024 * 1024)
}
pub(crate) fn append(
    c: &Connection,
    id: &str,
    now: i64,
    decision: &serde_json::Value,
) -> Result<serde_json::Value> {
    let (mut h, _) = read(c, id)?;
    let e = Event {
        version: "retry-decision-v1".into(),
        search_id: id.into(),
        sequence: h.count + 1,
        at_ms: now,
        result_epoch: crate::search_control::read(c, id)?.epoch,
        previous_hash: h.last_hash.clone(),
        decision: decision.clone(),
    };
    let body = serde_json::to_string(&e).map_err(|e| e.to_string())?;
    if body.len() > 4096 {
        return Err("retry decision body budget exceeded".into());
    }
    let hash = data::hash(&e)?;
    c.execute(
        "INSERT INTO corpus_retry_decisions(search_id,sequence,body,hash) VALUES(?1,?2,?3,?4)",
        params![id, e.sequence, body, hash],
    )
    .map_err(|e| e.to_string())?;
    h.count += 1;
    h.last_hash = hash.clone();
    put(c, &h)?;
    Ok(serde_json::json!({"sequence":h.count.to_string(),"hash":hash}))
}
pub(crate) fn status(c: &Connection, id: &str) -> Result<serde_json::Value> {
    let (h, events) = read(c, id)?;
    let rows:Vec<_>=events.iter().rev().take(16).map(|(e,hash)|serde_json::json!({"sequence":e.sequence.to_string(),"at_ms":e.at_ms.to_string(),"result_epoch":e.result_epoch.to_string(),"decision":e.decision,"hash":hash})).collect();
    Ok(
        serde_json::json!({"since_epoch":h.since_epoch.to_string(),"count":h.count.to_string(),"maximum_per_search":PER_SEARCH.to_string(),"recent":rows,"scope":"Admission observations since tracking began; counts are not worker starts or simulations. Full log holds new admission; operator controls remain independent."}),
    )
}
pub(crate) fn integrity(c: &Connection) -> Result<()> {
    let (heads,sources,count,bytes):(usize,usize,usize,usize)=c.query_row("SELECT (SELECT count(*) FROM corpus_retry_audit_heads),(SELECT count(*) FROM corpus_searches),(SELECT count(*) FROM corpus_retry_decisions),(SELECT coalesce(sum(length(CAST(body AS BLOB))),0) FROM corpus_retry_decisions)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())?;
    if heads != sources || heads > 64 || count > GLOBAL || bytes > 8 * 1024 * 1024 {
        return Err("retry audit quotas/head count mismatch".into());
    }
    Ok(())
}

pub(crate) fn page(
    c: &Connection,
    id: &str,
    after: Option<&str>,
    limit: usize,
) -> Result<serde_json::Value> {
    if !(1..=32).contains(&limit) {
        return Err("retry decision limit must be1..32".into());
    }
    let (head, events) = read(c, id)?;
    let start = if let Some(cursor) = after {
        let parts: Vec<_> = cursor.split(':').collect();
        if parts.len() != 3 || parts[0] != id {
            return Err("retry cursor scope mismatch".into());
        }
        let seq: usize = parts[1]
            .parse()
            .map_err(|_| "invalid retry sequence cursor")?;
        if seq == 0 || events.get(seq - 1).is_none_or(|(_, hash)| hash != parts[2]) {
            return Err("retry cursor record mismatch".into());
        }
        seq
    } else {
        0
    };
    let end = (start + limit).min(events.len());
    let rows:Vec<_>=events[start..end].iter().map(|(e,hash)|serde_json::json!({"sequence":e.sequence.to_string(),"at_ms":e.at_ms.to_string(),"result_epoch":e.result_epoch.to_string(),"decision":e.decision,"hash":hash})).collect();
    let next_cursor = (end < events.len()).then(|| format!("{id}:{end}:{}", events[end - 1].1));
    Ok(
        serde_json::json!({"search_id":id,"since_epoch":head.since_epoch.to_string(),"total":head.count.to_string(),"rows":rows,"next_cursor":next_cursor,"scope":"Verified immutable admission observations, not simulations or worker starts."}),
    )
}
