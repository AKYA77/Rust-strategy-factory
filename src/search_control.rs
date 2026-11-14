//! Durable search pause intent and state epochs, with bounded immutable transition detail.
use crate::{Result, data};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
const VERSION: &str = "search-control-v1";
const PER_SEARCH: usize = 128;
const TOTAL: usize = 4096;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Origin {
    Operator,
    WorkerError,
    LegacyUnknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Cause {
    Operator,
    WorkerError,
    Exhausted,
    OperatorRequeue,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Head {
    version: String,
    search_id: String,
    initial_state: String,
    pub state: String,
    pub epoch: i64,
    pub origin: Option<Origin>,
    paused_at: Option<i64>,
    journaled: usize,
    omitted: i64,
    last_hash: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    version: String,
    search_id: String,
    epoch: i64,
    at_ms: i64,
    from: String,
    to: String,
    cause: Cause,
    previous_hash: String,
}
fn db(e: rusqlite::Error) -> String {
    e.to_string()
}
fn valid_state(s: &str) -> bool {
    ["running", "paused", "stopped", "exhausted"].contains(&s)
}
fn valid_time(t: i64) -> bool {
    chrono::DateTime::from_timestamp_millis(t).is_some()
}
fn put(c: &Connection, head: &Head) -> Result<()> {
    let body = serde_json::to_string(head).map_err(|e| e.to_string())?;
    if body.len() > 4096 {
        return Err("search control head budget exceeded".into());
    }
    c.execute("INSERT INTO corpus_search_runtime(search_id,body,hash) VALUES(?1,?2,?3) ON CONFLICT(search_id) DO UPDATE SET body=excluded.body,hash=excluded.hash",params![head.search_id,body,data::hash(head)?]).map_err(db)?;
    Ok(())
}
pub(crate) fn register(c: &Connection, id: &str, state: &str) -> Result<()> {
    if !valid_state(state) {
        return Err("invalid search control initial state".into());
    }
    put(
        c,
        &Head {
            version: VERSION.into(),
            search_id: id.into(),
            initial_state: state.into(),
            state: state.into(),
            epoch: 0,
            origin: (state == "paused").then_some(Origin::LegacyUnknown),
            paused_at: None,
            journaled: 0,
            omitted: 0,
            last_hash: String::new(),
        },
    )
}
pub(crate) fn initialize(c: &mut Connection, migrate: bool) -> Result<()> {
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS corpus_search_runtime(search_id TEXT PRIMARY KEY REFERENCES corpus_searches(id),body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=4096),hash TEXT NOT NULL CHECK(length(hash)=64));
    CREATE TABLE IF NOT EXISTS corpus_search_transitions(event_id INTEGER PRIMARY KEY,search_id TEXT NOT NULL REFERENCES corpus_searches(id),epoch INTEGER NOT NULL CHECK(epoch>0),body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=4096),hash TEXT NOT NULL CHECK(length(hash)=64),UNIQUE(search_id,epoch));
    CREATE TRIGGER IF NOT EXISTS search_runtime_key_immutable BEFORE UPDATE OF search_id ON corpus_search_runtime BEGIN SELECT RAISE(ABORT,'search runtime key immutable');END;
    CREATE TRIGGER IF NOT EXISTS search_runtime_no_delete BEFORE DELETE ON corpus_search_runtime BEGIN SELECT RAISE(ABORT,'search runtime retained');END;
    CREATE TRIGGER IF NOT EXISTS search_transition_no_update BEFORE UPDATE ON corpus_search_transitions BEGIN SELECT RAISE(ABORT,'search transition immutable');END;
    CREATE TRIGGER IF NOT EXISTS search_transition_no_delete BEFORE DELETE ON corpus_search_transitions BEGIN SELECT RAISE(ABORT,'search transition immutable');END;").map_err(db)?;
    if migrate {
        let rows = {
            let mut q = tx
                .prepare("SELECT id,state FROM corpus_searches LIMIT 65")
                .map_err(db)?;
            q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(db)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db)?
        };
        if rows.len() > 64 {
            return Err("search migration count budget exceeded".into());
        }
        for (id, state) in rows {
            tx.execute(
                "UPDATE corpus_searches SET token=NULL,expires_ms=NULL WHERE id=?1",
                [&id],
            )
            .map_err(db)?;
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM corpus_search_runtime WHERE search_id=?1)",
                    [&id],
                    |r| r.get(0),
                )
                .map_err(db)?;
            if !exists {
                register(&tx, &id, &state)?;
            }
        }
        tx.execute_batch("PRAGMA user_version=11;").map_err(db)?;
    }
    tx.commit().map_err(db)
}
fn history(c: &Connection, id: &str) -> Result<Vec<(Event, String)>> {
    let mut q=c.prepare("SELECT epoch,CASE WHEN length(CAST(body AS BLOB))<=4096 THEN body END,hash FROM corpus_search_transitions WHERE search_id=?1 ORDER BY epoch LIMIT 129").map_err(db)?;
    let rows = q
        .query_map([id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(db)?;
    let mut events: Vec<(Event, String)> = Vec::new();
    for row in rows {
        let (epoch, body, hash) = row.map_err(db)?;
        let e: Event = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        if events.len() >= PER_SEARCH
            || e.version != VERSION
            || e.search_id != id
            || e.epoch != epoch
            || epoch != events.len() as i64 + 1
            || !valid_time(e.at_ms)
            || data::hash(&e)? != hash
            || e.previous_hash != events.last().map_or("", |(_, hash)| hash)
            || !valid_state(&e.from)
            || !valid_state(&e.to)
            || !allowed(&e.from, &e.to, e.cause)
            || events.last().is_some_and(|(last, _)| last.to != e.from)
        {
            return Err("search transition history integrity mismatch".into());
        }
        events.push((e, hash));
    }
    Ok(events)
}
fn allowed(from: &str, to: &str, cause: Cause) -> bool {
    match cause {
        Cause::Operator => {
            ["running", "paused"].contains(&from) && ["running", "paused", "stopped"].contains(&to)
        }
        Cause::WorkerError => from == "running" && to == "paused",
        Cause::Exhausted => from == "running" && to == "exhausted",
        Cause::OperatorRequeue => from == "paused" && to == "running",
    }
}
pub(crate) fn read(c: &Connection, id: &str) -> Result<Head> {
    let row:Option<(String,String,String)>=c.query_row("SELECT CASE WHEN length(CAST(r.body AS BLOB))<=4096 THEN r.body END,r.hash,s.state FROM corpus_search_runtime r JOIN corpus_searches s ON s.id=r.search_id WHERE r.search_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db)?;
    let (body, hash, state) =
        row.ok_or("search control head missing; migration or state is incomplete")?;
    let h: Head = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    if h.version != VERSION
        || h.search_id != id
        || h.state != state
        || !valid_state(&h.state)
        || !valid_state(&h.initial_state)
        || h.epoch < 0
        || h.omitted < 0
        || h.journaled > PER_SEARCH
        || h.epoch
            != h.omitted
                .checked_add(h.journaled as i64)
                .ok_or("search epoch count overflow")?
        || data::hash(&h)? != hash
    {
        return Err("search control head integrity mismatch".into());
    }
    if h.state == "paused" {
        match h.origin {
            Some(Origin::LegacyUnknown) if h.epoch == 0 && h.paused_at.is_none() => {}
            Some(Origin::Operator | Origin::WorkerError) if h.paused_at.is_some_and(valid_time) => {
            }
            _ => return Err("paused search origin/time mismatch".into()),
        }
    } else if h.origin.is_some() || h.paused_at.is_some() {
        return Err("nonpaused search retains pause intent".into());
    }
    let events = history(c, id)?;
    if events.len() != h.journaled
        || events.last().map_or("", |(_, hash)| hash) != h.last_hash
        || events
            .first()
            .is_some_and(|(e, _)| e.from != h.initial_state)
        || h.omitted == 0 && events.last().is_some_and(|(e, _)| e.to != h.state)
    {
        return Err("search control history/head mismatch".into());
    }
    if h.omitted == 0
        && let Some((e, _)) = events.last()
    {
        let expected = if e.to == "paused" {
            Some(if e.cause == Cause::WorkerError {
                Origin::WorkerError
            } else {
                Origin::Operator
            })
        } else {
            None
        };
        if expected != h.origin || h.paused_at != expected.map(|_| e.at_ms) {
            return Err("search pause intent/history mismatch".into());
        }
    }
    Ok(h)
}
/// Caller must hold an immediate transaction. No history quota can prevent a control.
pub(crate) fn transition(
    c: &Connection,
    id: &str,
    to: &str,
    cause: Cause,
    now: i64,
) -> Result<Head> {
    let mut h = read(c, id)?;
    if !valid_time(now) || !allowed(&h.state, to, cause) {
        return Err("invalid search state transition".into());
    }
    if cause == Cause::Operator
        && h.state == to
        && (to == "running" || h.origin == Some(Origin::Operator))
    {
        return Ok(h);
    }
    let epoch = h
        .epoch
        .checked_add(1)
        .ok_or("search control epoch overflow")?;
    let event = Event {
        version: VERSION.into(),
        search_id: id.into(),
        epoch,
        at_ms: now,
        from: h.state.clone(),
        to: to.into(),
        cause,
        previous_hash: h.last_hash.clone(),
    };
    let body = serde_json::to_string(&event).map_err(|e| e.to_string())?;
    let (count,bytes):(usize,usize)=c.query_row("SELECT count(*),coalesce(sum(length(CAST(body AS BLOB))),0) FROM corpus_search_transitions",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db)?;
    if h.omitted == 0
        && h.journaled < PER_SEARCH
        && count < TOTAL
        && body.len() <= 4096
        && bytes + body.len() <= 8 * 1024 * 1024
    {
        let hash = data::hash(&event)?;
        c.execute(
            "INSERT INTO corpus_search_transitions(search_id,epoch,body,hash) VALUES(?1,?2,?3,?4)",
            params![id, epoch, body, hash],
        )
        .map_err(db)?;
        h.journaled += 1;
        h.last_hash = hash;
    } else {
        h.omitted = h
            .omitted
            .checked_add(1)
            .ok_or("search omitted history count overflow")?;
    }
    h.state = to.into();
    h.epoch = epoch;
    h.origin = if to == "paused" {
        Some(if cause == Cause::WorkerError {
            Origin::WorkerError
        } else {
            Origin::Operator
        })
    } else {
        None
    };
    h.paused_at = h.origin.map(|_| now);
    c.execute(
        "UPDATE corpus_searches SET state=?2,token=NULL,expires_ms=NULL WHERE id=?1",
        params![id, to],
    )
    .map_err(db)?;
    put(c, &h)?;
    Ok(h)
}
pub(crate) fn status(c: &Connection, id: &str) -> Result<serde_json::Value> {
    let h = read(c, id)?;
    let recent:Vec<_>=history(c,id)?.iter().rev().take(32).map(|(e,hash)|serde_json::json!({"epoch":e.epoch.to_string(),"at_ms":e.at_ms.to_string(),"from":e.from,"to":e.to,"cause":e.cause,"hash":hash})).collect();
    Ok(
        serde_json::json!({"epoch":h.epoch.to_string(),"pause_origin":h.origin,"paused_at_ms":h.paused_at.map(|n|n.to_string()),"journaled_transitions":h.journaled.to_string(),"omitted_transitions":h.omitted.to_string(),"history_complete":h.omitted==0,"recent_transitions":recent,"limitations":"Bounded local transition detail; omission is explicit and never prevents stop. Legacy pause intent is unknown. No automatic retry or independent evidence authentication."}),
    )
}

pub(crate) fn integrity(c: &Connection) -> Result<()> {
    let (heads,searches,events,bytes):(usize,usize,usize,usize)=c.query_row("SELECT (SELECT count(*) FROM corpus_search_runtime),(SELECT count(*) FROM corpus_searches),(SELECT count(*) FROM corpus_search_transitions),(SELECT coalesce(sum(length(CAST(body AS BLOB))),0) FROM corpus_search_transitions)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(db)?;
    if heads != searches || heads > 64 || events > TOTAL || bytes > 8 * 1024 * 1024 {
        return Err("search control persisted quotas/head count mismatch".into());
    }
    Ok(())
}
