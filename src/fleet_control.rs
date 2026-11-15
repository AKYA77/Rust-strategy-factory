//! Durable fleet pause intent and state epochs, with bounded immutable transition detail.
use crate::{Result, data};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
const VERSION: &str = "fleet-control-v1";
const PER_FLEET: usize = 128;
const TOTAL: usize = 2048;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Origin {
    Operator,
    SchedulerIdle,
    LegacyUnknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Cause {
    Operator,
    SchedulerIdle,
    Exhausted,
    PolicyResume,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Head {
    version: String,
    fleet_id: String,
    initial_state: String,
    pub state: String,
    pub epoch: i64,
    pub origin: Option<Origin>,
    pub paused_at: Option<i64>,
    journaled: usize,
    omitted: i64,
    last_hash: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    version: String,
    fleet_id: String,
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
        return Err("fleet control head budget exceeded".into());
    }
    c.execute("INSERT INTO corpus_fleet_runtime(fleet_id,body,hash) VALUES(?1,?2,?3) ON CONFLICT(fleet_id) DO UPDATE SET body=excluded.body,hash=excluded.hash",params![head.fleet_id,body,data::hash(head)?]).map_err(db)?;
    Ok(())
}
pub(crate) fn register(c: &Connection, id: &str, state: &str) -> Result<()> {
    if !valid_state(state) {
        return Err("invalid fleet control initial state".into());
    }
    put(
        c,
        &Head {
            version: VERSION.into(),
            fleet_id: id.into(),
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
pub(crate) fn initialize(
    c: &mut Connection,
    migrate: bool,
) -> std::result::Result<(), crate::corpus_failure::AdmissionFailure> {
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(crate::corpus_failure::AdmissionFailure::from)?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS corpus_fleet_runtime(fleet_id TEXT PRIMARY KEY REFERENCES corpus_fleets(id),body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=4096),hash TEXT NOT NULL CHECK(length(hash)=64));
    CREATE TABLE IF NOT EXISTS corpus_fleet_transitions(event_id INTEGER PRIMARY KEY,fleet_id TEXT NOT NULL REFERENCES corpus_fleets(id),epoch INTEGER NOT NULL CHECK(epoch>0),body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=4096),hash TEXT NOT NULL CHECK(length(hash)=64),UNIQUE(fleet_id,epoch));
    CREATE TRIGGER IF NOT EXISTS fleet_runtime_key_immutable BEFORE UPDATE OF fleet_id ON corpus_fleet_runtime BEGIN SELECT RAISE(ABORT,'fleet runtime key immutable');END;
    CREATE TRIGGER IF NOT EXISTS fleet_runtime_no_delete BEFORE DELETE ON corpus_fleet_runtime BEGIN SELECT RAISE(ABORT,'fleet runtime retained');END;
    CREATE TRIGGER IF NOT EXISTS fleet_transition_no_update BEFORE UPDATE ON corpus_fleet_transitions BEGIN SELECT RAISE(ABORT,'fleet transition immutable');END;
    CREATE TRIGGER IF NOT EXISTS fleet_transition_no_delete BEFORE DELETE ON corpus_fleet_transitions BEGIN SELECT RAISE(ABORT,'fleet transition immutable');END;").map_err(crate::corpus_failure::AdmissionFailure::from)?;
    if migrate {
        let rows = {
            let mut q = tx
                .prepare("SELECT id,state FROM corpus_fleets LIMIT 17")
                .map_err(crate::corpus_failure::AdmissionFailure::from)?;
            q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(crate::corpus_failure::AdmissionFailure::from)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(crate::corpus_failure::AdmissionFailure::from)?
        };
        if rows.len() > 16 {
            return Err("fleet migration count budget exceeded".into());
        }
        for (id, state) in rows {
            tx.execute(
                "UPDATE corpus_fleets SET token=NULL,expires_ms=NULL WHERE id=?1",
                [&id],
            )
            .map_err(crate::corpus_failure::AdmissionFailure::from)?;
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM corpus_fleet_runtime WHERE fleet_id=?1)",
                    [&id],
                    |r| r.get(0),
                )
                .map_err(crate::corpus_failure::AdmissionFailure::from)?;
            if !exists {
                register(&tx, &id, &state)?;
            }
        }
        tx.execute_batch("PRAGMA user_version=18;")
            .map_err(crate::corpus_failure::AdmissionFailure::from)?;
    }
    tx.commit()
        .map_err(crate::corpus_failure::AdmissionFailure::from)
}
fn history(c: &Connection, id: &str) -> Result<Vec<(Event, String)>> {
    let mut q=c.prepare("SELECT epoch,CASE WHEN length(CAST(body AS BLOB))<=4096 THEN body END,hash FROM corpus_fleet_transitions WHERE fleet_id=?1 ORDER BY epoch LIMIT 129").map_err(db)?;
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
        if events.len() >= PER_FLEET
            || e.version != VERSION
            || e.fleet_id != id
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
            return Err("fleet transition history integrity mismatch".into());
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
        Cause::SchedulerIdle => from == "running" && to == "paused",
        Cause::Exhausted => from == "running" && to == "exhausted",
        Cause::PolicyResume => from == "paused" && to == "running",
    }
}
pub(crate) fn read(c: &Connection, id: &str) -> Result<Head> {
    let row:Option<(String,String,String)>=c.query_row("SELECT CASE WHEN length(CAST(r.body AS BLOB))<=4096 THEN r.body END,r.hash,s.state FROM corpus_fleet_runtime r JOIN corpus_fleets s ON s.id=r.fleet_id WHERE r.fleet_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db)?;
    let (body, hash, state) =
        row.ok_or("fleet control head missing; migration or state is incomplete")?;
    let h: Head = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    if h.version != VERSION
        || h.fleet_id != id
        || h.state != state
        || h.epoch == 0 && h.state != h.initial_state
        || !valid_state(&h.state)
        || !valid_state(&h.initial_state)
        || h.epoch < 0
        || h.omitted < 0
        || h.journaled > PER_FLEET
        || h.epoch
            != h.omitted
                .checked_add(h.journaled as i64)
                .ok_or("fleet epoch count overflow")?
        || data::hash(&h)? != hash
    {
        return Err("fleet control head integrity mismatch".into());
    }
    if h.state == "paused" {
        match h.origin {
            Some(Origin::LegacyUnknown) if h.epoch == 0 && h.paused_at.is_none() => {}
            Some(Origin::Operator | Origin::SchedulerIdle)
                if h.paused_at.is_some_and(valid_time) => {}
            _ => return Err("paused fleet origin/time mismatch".into()),
        }
    } else if h.origin.is_some() || h.paused_at.is_some() {
        return Err("nonpaused fleet retains pause intent".into());
    }
    let events = history(c, id)?;
    if events.len() != h.journaled
        || events.last().map_or("", |(_, hash)| hash) != h.last_hash
        || events
            .first()
            .is_some_and(|(e, _)| e.from != h.initial_state)
        || h.omitted == 0 && events.last().is_some_and(|(e, _)| e.to != h.state)
    {
        return Err("fleet control history/head mismatch".into());
    }
    if h.omitted == 0
        && let Some((e, _)) = events.last()
    {
        let expected = if e.to == "paused" {
            Some(if e.cause == Cause::SchedulerIdle {
                Origin::SchedulerIdle
            } else {
                Origin::Operator
            })
        } else {
            None
        };
        if expected != h.origin || h.paused_at != expected.map(|_| e.at_ms) {
            return Err("fleet pause intent/history mismatch".into());
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
    if !valid_time(now)
        || !allowed(&h.state, to, cause)
        || cause == Cause::PolicyResume
            && (h.origin != Some(Origin::SchedulerIdle) || h.omitted != 0)
    {
        return Err("invalid fleet state transition".into());
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
        .ok_or("fleet control epoch overflow")?;
    let event = Event {
        version: VERSION.into(),
        fleet_id: id.into(),
        epoch,
        at_ms: now,
        from: h.state.clone(),
        to: to.into(),
        cause,
        previous_hash: h.last_hash.clone(),
    };
    let body = serde_json::to_string(&event).map_err(|e| e.to_string())?;
    let (count,bytes):(usize,usize)=c.query_row("SELECT count(*),coalesce(sum(length(CAST(body AS BLOB))),0) FROM corpus_fleet_transitions",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db)?;
    if h.omitted == 0
        && h.journaled < PER_FLEET
        && count < TOTAL
        && body.len() <= 4096
        && bytes + body.len() <= 8 * 1024 * 1024
    {
        let hash = data::hash(&event)?;
        c.execute(
            "INSERT INTO corpus_fleet_transitions(fleet_id,epoch,body,hash) VALUES(?1,?2,?3,?4)",
            params![id, epoch, body, hash],
        )
        .map_err(db)?;
        h.journaled += 1;
        h.last_hash = hash;
    } else {
        h.omitted = h
            .omitted
            .checked_add(1)
            .ok_or("fleet omitted history count overflow")?;
    }
    h.state = to.into();
    h.epoch = epoch;
    h.origin = if to == "paused" {
        Some(if cause == Cause::SchedulerIdle {
            Origin::SchedulerIdle
        } else {
            Origin::Operator
        })
    } else {
        None
    };
    h.paused_at = h.origin.map(|_| now);
    c.execute(
        "UPDATE corpus_fleets SET state=?2,token=NULL,expires_ms=NULL WHERE id=?1",
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
    let (heads,fleets,events,bytes):(usize,usize,usize,usize)=c.query_row("SELECT (SELECT count(*) FROM corpus_fleet_runtime),(SELECT count(*) FROM corpus_fleets),(SELECT count(*) FROM corpus_fleet_transitions),(SELECT coalesce(sum(length(CAST(body AS BLOB))),0) FROM corpus_fleet_transitions)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(db)?;
    if heads != fleets || heads > 16 || events > TOTAL || bytes > 8 * 1024 * 1024 {
        return Err("fleet control persisted quotas/head count mismatch".into());
    }
    Ok(())
}

/// Resume only a verified scheduler-idle epoch; never operator/legacy/terminal intent.
pub fn resume_idle(
    registry: &mut crate::storage::Registry,
    id: &str,
    expected_epoch: i64,
    now: i64,
) -> Result<()> {
    let tx = registry
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    let head = read(&tx, id)?;
    if head.epoch != expected_epoch
        || head.state != "paused"
        || head.origin != Some(Origin::SchedulerIdle)
        || head.omitted != 0
    {
        return Err("fleet idle resume epoch/origin/history fenced".into());
    }
    transition(&tx, id, "running", Cause::PolicyResume, now)?;
    tx.execute("UPDATE corpus_fleets SET idle_visits=0 WHERE id=?1", [id])
        .map_err(db)?;
    tx.commit().map_err(db)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{coordinator, corpus_fleet, storage::Registry};
    fn fixture(name: &str) -> (std::path::PathBuf, std::path::PathBuf, String) {
        corpus_fleet::tests::fixture(name)
    }
    fn idle(reg: &mut Registry, id: &str) -> i64 {
        let tx = reg
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let h = transition(&tx, id, "paused", Cause::SchedulerIdle, 1000).unwrap();
        tx.commit().unwrap();
        h.epoch
    }
    #[test]
    fn actual_scheduler_idle_epoch_preserves_operator_override_and_terminal_backup() {
        let (root, db, id) = fixture("control-idle");
        let native = root.join("source.rsf");
        let saved = root.join("unavailable.rsf");
        std::fs::rename(&native, &saved).unwrap();
        let step = corpus_fleet::step(&db, &id).unwrap();
        assert_eq!(step.kind, "no-ready-sources");
        assert_eq!(step.status["control"]["pause_origin"], "scheduler_idle");
        assert_eq!(step.status["control"]["epoch"], "1");
        let mut reg = Registry::open(&db).unwrap();
        assert!(resume_idle(&mut reg, &id, 0, 2000).is_err());
        corpus_fleet::control(&mut reg, &id, "paused").unwrap();
        assert_eq!(
            status(&reg.connection, &id).unwrap()["pause_origin"],
            "operator"
        );
        assert!(resume_idle(&mut reg, &id, 1, 2000).is_err());
        assert!(resume_idle(&mut reg, &id, 2, 2000).is_err());
        std::fs::rename(&saved, &native).unwrap();
        corpus_fleet::control(&mut reg, &id, "running").unwrap();
        let epoch = read(&reg.connection, &id).unwrap().epoch;
        let lease = coordinator::claim(
            &mut reg,
            coordinator::Kind::Fleet,
            &id,
            chrono::Utc::now().timestamp_millis(),
            60000,
        )
        .unwrap()
        .unwrap();
        corpus_fleet::control(&mut reg, &id, "running").unwrap();
        assert_eq!(read(&reg.connection, &id).unwrap().epoch, epoch);
        coordinator::fence(
            &reg.connection,
            &lease,
            chrono::Utc::now().timestamp_millis(),
        )
        .unwrap();
        coordinator::release(&reg, &lease).unwrap();
        let done = corpus_fleet::step(&db, &id).unwrap();
        assert_eq!(done.kind, "exhausted");
        assert!(done.status["control"]["pause_origin"].is_null());
        assert!(resume_idle(&mut reg, &id, epoch, 3000).is_err());
        let backup = root.join("control-backup.sqlite");
        reg.backup(&backup).unwrap();
        let restored = Registry::open(&backup).unwrap();
        assert_eq!(
            corpus_fleet::status(&restored, &id).unwrap(),
            corpus_fleet::status(&reg, &id).unwrap()
        );
        restored.integrity().unwrap();
        drop(restored);
        drop(reg);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn competing_idle_resumes_have_one_winner_and_operator_pause_wins_either_order() {
        let (root, db, id) = fixture("control-race");
        let mut reg = Registry::open(&db).unwrap();
        let epoch = idle(&mut reg, &id);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles = (0..2)
            .map(|_| {
                let db = db.clone();
                let id = id.clone();
                let b = barrier.clone();
                std::thread::spawn(move || {
                    let mut r = Registry::open(&db).unwrap();
                    b.wait();
                    resume_idle(&mut r, &id, epoch, 2000).is_ok()
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            handles
                .into_iter()
                .map(|h| usize::from(h.join().unwrap()))
                .sum::<usize>(),
            1
        );
        let epoch = idle(&mut reg, &id);
        let b = std::sync::Arc::new(std::sync::Barrier::new(2));
        let path = db.clone();
        let key = id.clone();
        let barrier = b.clone();
        let automatic = std::thread::spawn(move || {
            let mut r = Registry::open(&path).unwrap();
            barrier.wait();
            resume_idle(&mut r, &key, epoch, 3000)
        });
        let path = db.clone();
        let key = id.clone();
        let operator = std::thread::spawn(move || {
            let mut r = Registry::open(&path).unwrap();
            b.wait();
            corpus_fleet::control(&mut r, &key, "paused")
        });
        automatic.join().unwrap().ok();
        operator.join().unwrap().unwrap();
        let h = read(&reg.connection, &id).unwrap();
        assert_eq!(h.state, "paused");
        assert_eq!(h.origin, Some(Origin::Operator));
        reg.integrity().unwrap();
        drop(reg);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn migration_retains_unknown_pause_and_keys_and_current_missing_heads_are_not_repaired() {
        let (root, db, id) = fixture("control-migration");
        let reg = Registry::open(&db).unwrap();
        let definition: String = reg
            .connection
            .query_row(
                "SELECT configuration FROM corpus_fleets WHERE id=?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        reg.connection.execute_batch("DROP TABLE corpus_fleet_transitions;DROP TABLE corpus_fleet_runtime;UPDATE corpus_fleets SET state='paused',token='legacy',expires_ms=9999999999999;PRAGMA user_version=17;").unwrap();
        drop(reg);
        let mut reg = Registry::open(&db).unwrap();
        let h = read(&reg.connection, &id).unwrap();
        assert_eq!(h.epoch, 0);
        assert_eq!(h.origin, Some(Origin::LegacyUnknown));
        assert!(h.paused_at.is_none());
        assert!(resume_idle(&mut reg, &id, 0, 2000).is_err());
        assert_eq!(
            reg.connection
                .query_row(
                    "SELECT configuration FROM corpus_fleets WHERE id=?1",
                    [&id],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            definition
        );
        assert_eq!(
            corpus_fleet::status(&reg, &id).unwrap()["lease_active"],
            false
        );
        reg.integrity().unwrap();
        reg.connection
            .execute_batch("DROP TABLE corpus_fleet_runtime;")
            .unwrap();
        drop(reg);
        let reg = Registry::open(&db).unwrap();
        assert!(reg.integrity().is_err());
        assert!(corpus_fleet::status(&reg, &id).is_err());
        drop(reg);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn bounded_transition_omission_holds_automatic_resume_but_never_operator_stop() {
        let (root, db, id) = fixture("control-cap");
        let mut reg = Registry::open(&db).unwrap();
        for _ in 0..65 {
            corpus_fleet::control(&mut reg, &id, "paused").unwrap();
            corpus_fleet::control(&mut reg, &id, "running").unwrap();
        }
        let epoch = idle(&mut reg, &id);
        let s = status(&reg.connection, &id).unwrap();
        assert_eq!(s["journaled_transitions"], "128");
        assert_eq!(s["history_complete"], false);
        assert!(resume_idle(&mut reg, &id, epoch, 2000).is_err());
        corpus_fleet::control(&mut reg, &id, "stopped").unwrap();
        assert_eq!(read(&reg.connection, &id).unwrap().state, "stopped");
        assert!(corpus_fleet::control(&mut reg, &id, "running").is_err());
        assert!(
            reg.connection
                .execute(
                    "UPDATE corpus_fleet_transitions SET hash=?1",
                    ["0".repeat(64)]
                )
                .is_err()
        );
        reg.integrity().unwrap();
        drop(reg);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failed_transition_write_rolls_back_state_epoch_and_coordinator_nonce() {
        let (root, db, id) = fixture("control-rollback");
        let mut reg = Registry::open(&db).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let lease = coordinator::claim(&mut reg, coordinator::Kind::Fleet, &id, now, 60000)
            .unwrap()
            .unwrap();
        reg.connection.execute_batch("CREATE TRIGGER fail_control BEFORE INSERT ON corpus_fleet_transitions BEGIN SELECT RAISE(ABORT,'injected control journal failure');END;").unwrap();
        assert!(corpus_fleet::control(&mut reg, &id, "paused").is_err());
        let h = read(&reg.connection, &id).unwrap();
        assert_eq!(h.state, "running");
        assert_eq!(h.epoch, 0);
        coordinator::fence(&reg.connection, &lease, now + 1).unwrap();
        reg.connection
            .execute_batch("DROP TRIGGER fail_control;")
            .unwrap();
        idle(&mut reg, &id);
        assert!(coordinator::fence(&reg.connection, &lease, now + 1).is_err());
        reg.integrity().unwrap();
        drop(reg);
        std::fs::remove_dir_all(root).unwrap();
    }
}
