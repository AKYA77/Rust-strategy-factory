//! Prespecified synthetic campaign scopes and bounded foreground region execution.
use crate::{Result, corpus_search, data, operator_config, storage::Registry};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

const VERSION: &str = "synthetic-corpus-campaign-v1";
const MAX_BODY: usize = 65536;

pub(crate) fn initialize(connection: &mut Connection, previous: i64) -> Result<()> {
    if previous >= 20 {
        return Ok(());
    }
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    if previous < 19 {
        tx.execute_batch("CREATE TABLE IF NOT EXISTS corpus_campaigns(id TEXT PRIMARY KEY,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=65536));
            CREATE TRIGGER IF NOT EXISTS corpus_campaign_no_update BEFORE UPDATE ON corpus_campaigns BEGIN SELECT RAISE(ABORT,'campaign immutable');END;
            CREATE TRIGGER IF NOT EXISTS corpus_campaign_no_delete BEFORE DELETE ON corpus_campaigns BEGIN SELECT RAISE(ABORT,'campaign immutable');END;").map_err(db)?;
    }
    tx.execute_batch("CREATE TABLE IF NOT EXISTS corpus_campaign_runs(id TEXT PRIMARY KEY REFERENCES corpus_campaigns(id),cursor INTEGER NOT NULL DEFAULT 0 CHECK(cursor>=0),state TEXT NOT NULL CHECK(state IN ('running','paused','stopped','exhausted')),epoch INTEGER NOT NULL DEFAULT 0 CHECK(epoch>=0),token TEXT,expires_ms INTEGER,attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt>=0),last_hash TEXT NOT NULL CHECK(length(last_hash)=64));
        CREATE TABLE IF NOT EXISTS corpus_campaign_advances(campaign_id TEXT NOT NULL REFERENCES corpus_campaign_runs(id),region INTEGER NOT NULL CHECK(region BETWEEN 0 AND 15),body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=4096),hash TEXT NOT NULL CHECK(length(hash)=64),PRIMARY KEY(campaign_id,region));
        CREATE TRIGGER IF NOT EXISTS campaign_run_key_immutable BEFORE UPDATE OF id ON corpus_campaign_runs BEGIN SELECT RAISE(ABORT,'campaign run key immutable');END;
        CREATE TRIGGER IF NOT EXISTS campaign_run_no_delete BEFORE DELETE ON corpus_campaign_runs BEGIN SELECT RAISE(ABORT,'campaign run immutable');END;
        CREATE TRIGGER IF NOT EXISTS campaign_advance_no_update BEFORE UPDATE ON corpus_campaign_advances BEGIN SELECT RAISE(ABORT,'campaign progress immutable');END;
        CREATE TRIGGER IF NOT EXISTS campaign_advance_no_delete BEFORE DELETE ON corpus_campaign_advances BEGIN SELECT RAISE(ABORT,'campaign progress immutable');END;
        INSERT OR IGNORE INTO corpus_campaign_runs(id,state,last_hash) SELECT id,'paused',id FROM corpus_campaigns;
        PRAGMA user_version=20;").map_err(db)?;
    tx.commit().map_err(db)
}
fn db(e: rusqlite::Error) -> String {
    e.to_string()
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Region {
    start_index: u64,
    search_id: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    version: String,
    config_path: String,
    source_identity: String,
    seed: u64,
    start_index: u64,
    region_width: usize,
    regions: Vec<Region>,
    distinct_experiments: usize,
    trial_scope_hash: String,
}
fn validate(plan: &Plan) -> Result<()> {
    let total = plan
        .region_width
        .checked_mul(plan.regions.len())
        .ok_or("campaign slot overflow")?;
    if plan.version != VERSION
        || !(1..=16).contains(&plan.regions.len())
        || !(1..=4096).contains(&plan.region_width)
        || total > 4096
        || !(1..=total).contains(&plan.distinct_experiments)
        || !crate::machine_evidence::hex(&plan.source_identity)
        || !crate::machine_evidence::hex(&plan.trial_scope_hash)
        || plan.config_path.len() > 4096
        || !Path::new(&plan.config_path).is_absolute()
    {
        return Err("invalid frozen campaign bounds or identity".into());
    }
    plan.start_index
        .checked_add(total as u64 - 1)
        .ok_or("campaign range overflow")?;
    let mut ids = BTreeSet::new();
    for (index, region) in plan.regions.iter().enumerate() {
        if region.start_index
            != plan
                .start_index
                .checked_add((index * plan.region_width) as u64)
                .ok_or("campaign region overflow")?
            || !crate::machine_evidence::hex(&region.search_id)
            || !ids.insert(&region.search_id)
        {
            return Err("campaign region order/identity mismatch".into());
        }
    }
    Ok(())
}
fn plan_file(path: &Path, count: usize) -> Result<Plan> {
    if !(1..=16).contains(&count) {
        return Err("campaign requires 1..16 regions".into());
    }
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let loaded = operator_config::load_search(&path)?;
    let total = loaded
        .file
        .maximum_candidates
        .checked_mul(count)
        .ok_or("campaign slots overflow")?;
    if !(1..=4096).contains(&total) {
        return Err("campaign declared slots exceed 4096".into());
    }
    loaded
        .file
        .start_index
        .checked_add(total as u64 - 1)
        .ok_or("campaign range overflow")?;
    let mut regions = Vec::new();
    let mut trials = Vec::with_capacity(total);
    for index in 0..count {
        let mut run = loaded.run();
        run.start_index = loaded.file.start_index + (index * loaded.file.maximum_candidates) as u64;
        let (search_id, region_trials) = corpus_search::frozen_region(&run)?;
        regions.push(Region {
            start_index: run.start_index,
            search_id,
        });
        trials.extend(region_trials);
    }
    let plan = Plan {
        version: VERSION.into(),
        config_path: path.to_str().ok_or("non-UTF8 campaign path")?.into(),
        source_identity: loaded.specification.identity()?,
        seed: loaded.file.seed,
        start_index: loaded.file.start_index,
        region_width: loaded.file.maximum_candidates,
        regions,
        distinct_experiments: trials.iter().collect::<BTreeSet<_>>().len(),
        trial_scope_hash: data::hash(&trials)?,
    };
    validate(&plan)?;
    Ok(plan)
}
fn read(connection: &Connection, id: &str) -> Result<Plan> {
    if !crate::machine_evidence::hex(id) {
        return Err("invalid campaign id".into());
    }
    let body: Option<String> = connection.query_row("SELECT CASE WHEN length(CAST(body AS BLOB))<=65536 THEN body ELSE NULL END FROM corpus_campaigns WHERE id=?1", [id], |r|r.get(0)).map_err(db)?;
    let plan: Plan = serde_json::from_str(&body.ok_or("campaign metadata cap exceeded")?)
        .map_err(|e| e.to_string())?;
    validate(&plan)?;
    if data::hash(&plan)? != id {
        return Err("campaign immutable identity mismatch".into());
    }
    Ok(plan)
}
/// Freeze all region/search/trial identities without registering or starting child work.
pub fn register_file(path: &Path, database: &Path, regions: usize) -> Result<String> {
    let plan = plan_file(path, regions)?;
    let body = serde_json::to_string(&plan).map_err(|e| e.to_string())?;
    if body.len() > MAX_BODY {
        return Err("campaign metadata cap exceeded".into());
    }
    let id = data::hash(&plan)?;
    let mut registry = Registry::open(database)?;
    let tx = registry
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    let prior: Option<String> = tx
        .query_row(
            "SELECT body FROM corpus_campaigns WHERE id=?1",
            [&id],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?;
    if let Some(prior) = prior {
        if prior != body {
            return Err("immutable campaign definition differs".into());
        }
        execution(&tx, &id, &plan)?;
    } else {
        let (count, bytes): (usize, usize) = tx
            .query_row(
                "SELECT count(*),coalesce(sum(length(CAST(body AS BLOB))),0) FROM corpus_campaigns",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db)?;
        if count >= 16
            || bytes
                .checked_add(body.len())
                .is_none_or(|v| v > 1024 * 1024)
        {
            return Err("campaign metadata quota reached".into());
        }
        tx.execute(
            "INSERT INTO corpus_campaigns(id,body) VALUES(?1,?2)",
            params![id, body],
        )
        .map_err(db)?;
        tx.execute(
            "INSERT INTO corpus_campaign_runs(id,state,last_hash) VALUES(?1,'running',?1)",
            [&id],
        )
        .map_err(db)?;
    }
    tx.commit().map_err(db)?;
    Ok(id)
}
fn snapshot(plan: &Plan, id: &str, fresh: bool) -> serde_json::Value {
    let regions: Vec<_> = plan.regions.iter().enumerate().map(|(index,r)|serde_json::json!({"region":index.to_string(),"start_index":r.start_index.to_string(),"search_id":r.search_id})).collect();
    serde_json::json!({"campaign_id":id,"source_identity":plan.source_identity,"seed":plan.seed.to_string(),"start_index":plan.start_index.to_string(),"region_width":plan.region_width.to_string(),"regions":regions,"declared_slots":(plan.region_width * plan.regions.len()).to_string(),"distinct_experiments":plan.distinct_experiments.to_string(),"trial_scope_hash":plan.trial_scope_hash,"inputs_freshly_verified":fresh,"execution_supported":true,"qualification_granted":false,"limitations":"Frozen synthetic training trial scope only. Metadata inspection does not verify current inputs. Freezing starts no child. Explicit bounded foreground execution only; no scientific family approval or hard OS deadline."})
}
fn full_snapshot(
    connection: &Connection,
    plan: &Plan,
    id: &str,
    fresh: bool,
) -> Result<serde_json::Value> {
    let mut value = snapshot(plan, id, fresh);
    let head = execution(connection, id, plan)?;
    value["execution"] = serde_json::json!({"state":head.state,"next_region":head.cursor.to_string(),"epoch":head.epoch.to_string(),"attempt":head.attempt.to_string(),"lease_active":head.expiry.is_some_and(|t|t>chrono::Utc::now().timestamp_millis()),"advance_hash":head.last_hash,"completed_regions":head.advances});
    Ok(value)
}
pub fn status(registry: &Registry, id: &str) -> Result<serde_json::Value> {
    let tx = registry.connection.unchecked_transaction().map_err(db)?;
    let value = full_snapshot(&tx, &read(&tx, id)?, id, false)?;
    tx.commit().map_err(db)?;
    Ok(value)
}
pub fn verify_inputs(registry: &Registry, id: &str) -> Result<serde_json::Value> {
    let stored = read(&registry.connection, id)?;
    let current = plan_file(Path::new(&stored.config_path), stored.regions.len())?;
    if data::hash(&current)? != id {
        return Err("campaign frozen input, build or trial scope changed".into());
    }
    let tx = registry.connection.unchecked_transaction().map_err(db)?;
    let value = full_snapshot(&tx, &stored, id, true)?;
    tx.commit().map_err(db)?;
    Ok(value)
}
pub(crate) fn integrity(connection: &Connection) -> Result<()> {
    let (count, bytes): (usize, usize) = connection
        .query_row(
            "SELECT count(*),coalesce(sum(length(CAST(body AS BLOB))),0) FROM corpus_campaigns",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(db)?;
    if count > 16 || bytes > 1024 * 1024 {
        return Err("persisted campaign quota exceeded".into());
    }
    let mut query = connection
        .prepare("SELECT id FROM corpus_campaigns")
        .map_err(db)?;
    let ids = query.query_map([], |r| r.get::<_, String>(0)).map_err(db)?;
    for id in ids {
        let id = id.map_err(db)?;
        let plan = read(connection, &id)?;
        execution(connection, &id, &plan)?;
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Advance {
    region: usize,
    search_id: String,
    source_status_hash: String,
    previous_hash: String,
}
struct Execution {
    cursor: usize,
    state: String,
    epoch: i64,
    attempt: i64,
    expiry: Option<i64>,
    last_hash: String,
    advances: Vec<serde_json::Value>,
}
type ExecutionRow = (usize, String, i64, i64, Option<String>, Option<i64>, String);
fn execution(connection: &Connection, id: &str, plan: &Plan) -> Result<Execution> {
    let (cursor,state,epoch,attempt,token,expiry,last_hash): ExecutionRow = connection.query_row("SELECT cursor,state,epoch,attempt,token,expires_ms,last_hash FROM corpus_campaign_runs WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(db)?;
    if cursor > plan.regions.len()
        || (state == "exhausted") != (cursor == plan.regions.len())
        || !["running", "paused", "stopped", "exhausted"].contains(&state.as_str())
        || epoch < 0
        || attempt < 0
        || token.is_some() != expiry.is_some()
        || token
            .as_ref()
            .is_some_and(|t| !crate::machine_evidence::hex(t))
        || expiry.is_some_and(|t| chrono::DateTime::from_timestamp_millis(t).is_none())
    {
        return Err("campaign execution head integrity failure".into());
    }
    let (count,bytes): (usize,usize)=connection.query_row("SELECT count(*),coalesce(sum(length(CAST(body AS BLOB))),0) FROM corpus_campaign_advances WHERE campaign_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db)?;
    if count != cursor || count > 16 || bytes > 16 * 4096 {
        return Err("campaign progress cursor/budget mismatch".into());
    }
    let mut query=connection.prepare("SELECT region,body,hash FROM corpus_campaign_advances WHERE campaign_id=?1 ORDER BY region").map_err(db)?;
    let rows = query
        .query_map([id], |r| {
            Ok((
                r.get::<_, usize>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(db)?;
    let mut previous = id.to_owned();
    let mut advances = Vec::new();
    for row in rows {
        let (region, body, hash) = row.map_err(db)?;
        let item: Advance = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        if region != advances.len()
            || item.region != region
            || item.search_id != plan.regions[region].search_id
            || item.previous_hash != previous
            || !crate::machine_evidence::hex(&item.source_status_hash)
            || data::hash(&item)? != hash
        {
            return Err("campaign progress chain integrity failure".into());
        }
        advances.push(serde_json::json!({"region":region.to_string(),"search_id":item.search_id,"source_status_hash":item.source_status_hash,"previous_hash":item.previous_hash,"hash":hash}));
        previous = hash;
    }
    if previous != last_hash {
        return Err("campaign progress head hash mismatch".into());
    }
    Ok(Execution {
        cursor,
        state,
        epoch,
        attempt,
        expiry,
        last_hash,
        advances,
    })
}
pub fn list(registry: &Registry) -> Result<Vec<serde_json::Value>> {
    let tx = registry.connection.unchecked_transaction().map_err(db)?;
    integrity(&tx)?;
    let ids = {
        let mut q = tx
            .prepare("SELECT id FROM corpus_campaigns ORDER BY id LIMIT 17")
            .map_err(db)?;
        q.query_map([], |r| r.get::<_, String>(0))
            .map_err(db)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)?
    };
    let rows = ids
        .iter()
        .map(|id| full_snapshot(&tx, &read(&tx, id)?, id, false))
        .collect::<Result<Vec<_>>>()?;
    tx.commit().map_err(db)?;
    Ok(rows)
}
pub(crate) fn epoch(connection: &Connection, id: &str) -> Result<i64> {
    Ok(execution(connection, id, &read(connection, id)?)?.epoch)
}
/// Controls revoke the current parent nonce; an in-flight child may finish its bounded round.
pub fn control(registry: &mut Registry, id: &str, state: &str) -> Result<()> {
    control_at(registry, id, state, None)
}
pub fn control_expected(
    registry: &mut Registry,
    id: &str,
    state: &str,
    expected_epoch: i64,
) -> Result<()> {
    control_at(registry, id, state, Some(expected_epoch))
}
fn control_at(
    registry: &mut Registry,
    id: &str,
    state: &str,
    expected_epoch: Option<i64>,
) -> Result<()> {
    if !["running", "paused", "stopped"].contains(&state) {
        return Err("invalid campaign control".into());
    }
    let tx = registry
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    let head = execution(&tx, id, &read(&tx, id)?)?;
    if expected_epoch.is_some_and(|epoch| epoch != head.epoch) {
        return Err("stale campaign control epoch".into());
    }
    if head.state == state {
        return tx.commit().map_err(db);
    }
    if ["stopped", "exhausted"].contains(&head.state.as_str()) {
        return Err("campaign terminal state is immutable".into());
    }
    let epoch = head.epoch.checked_add(1).ok_or("campaign epoch overflow")?;
    tx.execute(
        "UPDATE corpus_campaign_runs SET state=?2,epoch=?3,token=NULL,expires_ms=NULL WHERE id=?1",
        params![id, state, epoch],
    )
    .map_err(db)?;
    tx.commit().map_err(db)
}
fn publish(
    registry: &mut Registry,
    lease: &crate::coordinator::Lease,
    plan: &Plan,
) -> Result<bool> {
    let tx = registry
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    crate::coordinator::fence(&tx, lease, chrono::Utc::now().timestamp_millis())?;
    let head = execution(&tx, &lease.id, plan)?;
    let source =
        crate::corpus_search::status_connection(&tx, &plan.regions[head.cursor].search_id)?;
    if source["state"] != "exhausted" {
        return Err("campaign region advancement requires verified exhaustion".into());
    }
    let item = Advance {
        region: head.cursor,
        search_id: plan.regions[head.cursor].search_id.clone(),
        source_status_hash: data::hash(&source)?,
        previous_hash: head.last_hash,
    };
    let hash = data::hash(&item)?;
    tx.execute(
        "INSERT INTO corpus_campaign_advances(campaign_id,region,body,hash) VALUES(?1,?2,?3,?4)",
        params![
            lease.id,
            head.cursor,
            serde_json::to_string(&item).map_err(|e| e.to_string())?,
            hash
        ],
    )
    .map_err(db)?;
    let cursor = head.cursor + 1;
    let terminal = cursor == plan.regions.len();
    tx.execute(
        "UPDATE corpus_campaign_runs SET cursor=?2,state=?3,last_hash=?4 WHERE id=?1",
        params![
            lease.id,
            cursor,
            if terminal { "exhausted" } else { "running" },
            hash
        ],
    )
    .map_err(db)?;
    tx.commit().map_err(db)?;
    Ok(terminal)
}
/// At most one ordinary search round. Parent and child coordinator leases are independent.
pub fn step(database: &Path, id: &str) -> Result<serde_json::Value> {
    let mut registry = Registry::open(database)?;
    let plan = read(&registry.connection, id)?;
    let head = execution(&registry.connection, id, &plan)?;
    if head.state != "running" {
        return Ok(
            serde_json::json!({"kind":if head.state=="exhausted" {"exhausted"} else {"held"},"status":status(&registry,id)?}),
        );
    }
    verify_inputs(&registry, id)?;
    let loaded = operator_config::load_search(Path::new(&plan.config_path))?;
    let mut run = loaded.run();
    run.start_index = plan.regions[head.cursor].start_index;
    if corpus_search::identity(&run)? != plan.regions[head.cursor].search_id {
        return Err("campaign source changed before claim".into());
    }
    let disk = loaded
        .file
        .disk_guard
        .as_ref()
        .map(|guard| guard.check(database))
        .transpose()?;
    let resource = loaded
        .file
        .resource_guard
        .as_ref()
        .map(|guard| guard.check())
        .transpose()?;
    if disk.as_ref().is_some_and(|d| !d.allowed) || resource.as_ref().is_some_and(|d| !d.allowed) {
        return Ok(
            serde_json::json!({"kind":"pressure","disk_admission":disk,"resource_admission":resource,"status":status(&registry,id)?}),
        );
    }
    let Some(lease) = crate::coordinator::claim(
        &mut registry,
        crate::coordinator::Kind::Campaign,
        id,
        chrono::Utc::now().timestamp_millis(),
        loaded.file.limits.lease_ms,
    )?
    else {
        return Ok(serde_json::json!({"kind":"busy_or_held","status":status(&registry,id)?}));
    };
    // The cursor can advance between the initial read and claim. Recheck under our own lease.
    let result: Result<serde_json::Value> = (|| {
        let current = execution(&registry.connection, id, &plan)?;
        crate::coordinator::fence(
            &registry.connection,
            &lease,
            chrono::Utc::now().timestamp_millis(),
        )?;
        if current.cursor != head.cursor {
            return Ok(serde_json::json!({"kind":"cursor_changed","status":status(&registry,id)?}));
        }
        let child = corpus_search::step(database, run, loaded.file.limits)?;
        crate::coordinator::fence(
            &registry.connection,
            &lease,
            chrono::Utc::now().timestamp_millis(),
        )?;
        let kind = if child.status["state"] == "exhausted" {
            if publish(&mut registry, &lease, &plan)? {
                "exhausted"
            } else {
                "advanced_region"
            }
        } else if child.kind == "advanced" {
            "advanced"
        } else {
            "held"
        };
        Ok(
            serde_json::json!({"kind":kind,"region":head.cursor.to_string(),"child":child,"disk_admission":disk,"resource_admission":resource,"status":status(&registry,id)?}),
        )
    })();
    let released = crate::coordinator::release(&registry, &lease);
    let mut value = result?;
    released?;
    // Return a fresh snapshot after releasing the parent, rather than a stale active lease flag.
    value["status"] = status(&registry, id)?;
    Ok(value)
}
pub fn run(
    database: &Path,
    id: &str,
    maximum: usize,
    shutdown: &std::sync::atomic::AtomicBool,
) -> Result<serde_json::Value> {
    if !(1..=10000).contains(&maximum) {
        return Err("campaign round limit must be 1..10000".into());
    }
    let mut rounds = 0;
    let mut last = None;
    let mut reason = "round_limit";
    for _ in 0..maximum {
        if shutdown.load(std::sync::atomic::Ordering::Acquire) {
            reason = "signal";
            break;
        }
        let value = step(database, id)?;
        rounds += 1;
        let advance = matches!(
            value["kind"].as_str(),
            Some("advanced" | "advanced_region" | "cursor_changed")
        );
        if !advance {
            reason = match value["kind"].as_str() {
                Some("exhausted") => "exhausted",
                Some("pressure") => "pressure",
                Some("busy_or_held") => "busy_or_held",
                _ => "held",
            };
        }
        last = Some(value);
        if !advance {
            break;
        }
    }
    Ok(
        serde_json::json!({"campaign_id":id,"rounds":rounds.to_string(),"stop_reason":reason,"last":last,"qualification_granted":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> Plan {
        Plan {
            version: VERSION.into(),
            config_path: "/tmp/owned-campaign-config.json".into(),
            source_identity: "a".repeat(64),
            seed: u64::MAX,
            start_index: 12,
            region_width: 2,
            regions: vec![
                Region {
                    start_index: 12,
                    search_id: "b".repeat(64),
                },
                Region {
                    start_index: 14,
                    search_id: "c".repeat(64),
                },
            ],
            distinct_experiments: 4,
            trial_scope_hash: "d".repeat(64),
        }
    }
    fn insert_plan(registry: &Registry) -> (Plan, String) {
        let plan = plan();
        let id = data::hash(&plan).unwrap();
        registry
            .connection
            .execute(
                "INSERT INTO corpus_campaigns(id,body) VALUES(?1,?2)",
                params![id, serde_json::to_string(&plan).unwrap()],
            )
            .unwrap();
        registry
            .connection
            .execute(
                "INSERT INTO corpus_campaign_runs(id,state,last_hash) VALUES(?1,'running',?1)",
                [&id],
            )
            .unwrap();
        (plan, id)
    }
    fn disk_fixture(name: &str) -> (std::path::PathBuf, std::path::PathBuf, String) {
        let root = std::env::temp_dir().join(format!("campaign-{name}-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let source = root.join("source.rsf");
        crate::native::write(&source, &crate::data::synthetic_fixture("EURUSD").unwrap()).unwrap();
        let spec = crate::corpus::describe(&[source], 10000).unwrap();
        std::fs::write(root.join("corpus.json"), serde_json::to_vec(&spec).unwrap()).unwrap();
        let config = serde_json::json!({"specification":"corpus.json","seed":42,"start_index":12,"maximum_candidates":1,"queue_width":1,"budget":{"max_candles":10000,"feature_cells":1000000,"max_receipts":10000},"limits":{"workers":1,"max_parallel_candles":10000,"max_parallel_feature_cells":1000000,"max_parallel_receipts":10000,"max_new_chunks":1,"lease_ms":60000}});
        let config_path = root.join("search.json");
        std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let db = root.join("state.sqlite");
        let id = register_file(&config_path, &db, 2).unwrap();
        (root, db, id)
    }
    #[test]
    fn expected_epoch_controls_and_bounded_inventory_preserve_holds_and_active_nonce() {
        use crate::coordinator::{self, Kind};
        let mut registry = Registry::open(Path::new(":memory:")).unwrap();
        let (_, id) = insert_plan(&registry);
        let now = chrono::Utc::now().timestamp_millis();
        let lease = coordinator::claim(&mut registry, Kind::Campaign, &id, now, 60000)
            .unwrap()
            .unwrap();
        control_expected(&mut registry, &id, "running", 0).unwrap();
        coordinator::fence(&registry.connection, &lease, now).unwrap();
        control_expected(&mut registry, &id, "paused", 0).unwrap();
        let held = status(&registry, &id).unwrap();
        assert!(control_expected(&mut registry, &id, "running", 0).is_err());
        assert!(control_expected(&mut registry, &id, "paused", 0).is_err());
        assert_eq!(status(&registry, &id).unwrap(), held);
        assert_eq!(list(&registry).unwrap(), vec![held]);
        control_expected(&mut registry, &id, "running", 1).unwrap();
        assert!(coordinator::fence(&registry.connection, &lease, now).is_err());
        assert_eq!(status(&registry, &id).unwrap()["execution"]["epoch"], "2");
    }
    #[test]
    fn publication_failure_rolls_back_advance_and_reuses_completed_source_on_restart() {
        let (root, db, id) = disk_fixture("publication");
        let registry = Registry::open(&db).unwrap();
        registry.connection.execute_batch("CREATE TRIGGER injected_campaign_publish BEFORE UPDATE OF cursor ON corpus_campaign_runs BEGIN SELECT RAISE(ABORT,'injected campaign publication failure');END;").unwrap();
        let mut failure = None;
        for _ in 0..10 {
            match step(&db, &id) {
                Ok(_) => {}
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        assert!(
            failure
                .unwrap()
                .contains("injected campaign publication failure")
        );
        let saved = status(&registry, &id).unwrap();
        assert_eq!(saved["execution"]["next_region"], "0");
        assert_eq!(saved["execution"]["lease_active"], false);
        assert!(
            saved["execution"]["completed_regions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let source_id = saved["regions"][0]["search_id"].as_str().unwrap();
        assert_eq!(
            corpus_search::status(&registry, source_id).unwrap()["state"],
            "exhausted"
        );
        let attempts = registry.counts().unwrap().attempts;
        registry
            .connection
            .execute_batch("DROP TRIGGER injected_campaign_publish;")
            .unwrap();
        assert_eq!(step(&db, &id).unwrap()["kind"], "advanced_region");
        assert_eq!(registry.counts().unwrap().attempts, attempts);
        registry.integrity().unwrap();
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn managed_restore_revokes_only_copied_campaign_parent_lease() {
        use crate::coordinator::{self, Kind};
        let (root, db, id) = disk_fixture("restore");
        let mut original = Registry::open(&db).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let old = coordinator::claim(&mut original, Kind::Campaign, &id, now, 60000)
            .unwrap()
            .unwrap();
        crate::recovery::backup(&db, "campaign-source").unwrap();
        let receipt = crate::recovery::restore(&db, "campaign-source", "campaign-copy").unwrap();
        assert_eq!(receipt["revoked_leases"], "1");
        let copied_path = crate::recovery::path(&db, "campaign-copy").unwrap();
        let mut copied = Registry::open(&copied_path).unwrap();
        assert!(coordinator::fence(&copied.connection, &old, now).is_err());
        coordinator::fence(&original.connection, &old, now).unwrap();
        let new = coordinator::claim(&mut copied, Kind::Campaign, &id, now, 60000)
            .unwrap()
            .unwrap();
        assert_ne!(old.token, new.token);
        coordinator::fence(&copied.connection, &new, now).unwrap();
        copied.integrity().unwrap();
        original.integrity().unwrap();
        drop(copied);
        drop(original);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn campaign_epoch_controls_competing_leases_and_publication_rollback_are_fenced() {
        use crate::coordinator::{self, Kind};
        let mut registry = Registry::open(Path::new(":memory:")).unwrap();
        let (plan, id) = insert_plan(&registry);
        let now = chrono::Utc::now().timestamp_millis();
        let old = coordinator::claim(&mut registry, Kind::Campaign, &id, now, 60000)
            .unwrap()
            .unwrap();
        assert!(
            coordinator::claim(&mut registry, Kind::Campaign, &id, now, 60000)
                .unwrap()
                .is_none()
        );
        control(&mut registry, &id, "paused").unwrap();
        control(&mut registry, &id, "running").unwrap();
        assert!(coordinator::fence(&registry.connection, &old, now).is_err());
        let new = coordinator::claim(&mut registry, Kind::Campaign, &id, now, 60000)
            .unwrap()
            .unwrap();
        coordinator::release(&registry, &old).unwrap();
        coordinator::fence(&registry.connection, &new, now).unwrap();
        assert!(publish(&mut registry, &new, &plan).is_err());
        assert_eq!(
            execution(&registry.connection, &id, &plan).unwrap().cursor,
            0
        );
        coordinator::release(&registry, &new).unwrap();
        control(&mut registry, &id, "stopped").unwrap();
        assert!(control(&mut registry, &id, "running").is_err());
        assert!(
            coordinator::claim(&mut registry, Kind::Campaign, &id, now, 60000)
                .unwrap()
                .is_none()
        );
        assert_eq!(status(&registry, &id).unwrap()["execution"]["epoch"], "3");
    }
    #[test]
    fn schema19_campaigns_migrate_held_without_rewriting_frozen_scope() {
        let mut registry = Registry::open(Path::new(":memory:")).unwrap();
        let (plan, id) = insert_plan(&registry);
        registry.connection.execute_batch("DROP TABLE corpus_campaign_advances; DROP TABLE corpus_campaign_runs; PRAGMA user_version=19;").unwrap();
        initialize(&mut registry.connection, 19).unwrap();
        assert_eq!(
            data::hash(&read(&registry.connection, &id).unwrap()).unwrap(),
            id
        );
        let head = execution(&registry.connection, &id, &plan).unwrap();
        assert_eq!(head.state, "paused");
        assert_eq!(head.epoch, 0);
        assert_eq!(head.cursor, 0);
        assert!(head.expiry.is_none());
        integrity(&registry.connection).unwrap();
    }
    #[test]
    fn malformed_campaign_order_duplicates_scope_and_overflow_refuse() {
        let base = plan();
        validate(&base).unwrap();
        let mut cases = Vec::new();
        let mut bad = base.clone();
        bad.start_index = u64::MAX;
        cases.push(bad);
        let mut bad = base.clone();
        bad.regions[1].start_index = 13;
        cases.push(bad);
        let mut bad = base.clone();
        bad.regions[1].search_id = bad.regions[0].search_id.clone();
        cases.push(bad);
        let mut bad = base.clone();
        bad.distinct_experiments = 5;
        cases.push(bad);
        let mut bad = base.clone();
        bad.region_width = 4096;
        cases.push(bad);
        let mut bad = base.clone();
        bad.config_path = "relative.json".into();
        cases.push(bad);
        let mut bad = base.clone();
        bad.trial_scope_hash = "not-a-hash".into();
        cases.push(bad);
        let mut bad = base;
        bad.version = "future-version".into();
        cases.push(bad);
        for bad in cases {
            assert!(validate(&bad).is_err());
        }
    }
    #[test]
    fn campaign_immutability_corruption_missing_schema_and_quota_are_verified() {
        let registry = Registry::open(Path::new(":memory:")).unwrap();
        let plan = plan();
        let id = data::hash(&plan).unwrap();
        let body = serde_json::to_string(&plan).unwrap();
        registry
            .connection
            .execute(
                "INSERT INTO corpus_campaigns(id,body) VALUES(?1,?2)",
                params![id, body],
            )
            .unwrap();
        registry
            .connection
            .execute(
                "INSERT INTO corpus_campaign_runs(id,state,last_hash) VALUES(?1,'running',?1)",
                [&id],
            )
            .unwrap();
        assert!(
            registry
                .connection
                .execute("UPDATE corpus_campaigns SET body=body WHERE id=?1", [&id])
                .is_err()
        );
        assert!(
            registry
                .connection
                .execute("DELETE FROM corpus_campaigns WHERE id=?1", [&id])
                .is_err()
        );
        assert_eq!(
            status(&registry, &id).unwrap()["seed"],
            u64::MAX.to_string()
        );
        integrity(&registry.connection).unwrap();
        for seed in 0..16 {
            let mut extra = plan.clone();
            extra.seed = seed;
            registry
                .connection
                .execute(
                    "INSERT INTO corpus_campaigns(id,body) VALUES(?1,?2)",
                    params![
                        data::hash(&extra).unwrap(),
                        serde_json::to_string(&extra).unwrap()
                    ],
                )
                .unwrap();
        }
        assert!(integrity(&registry.connection).is_err());
        registry
            .connection
            .execute_batch(
                "DROP TRIGGER corpus_campaign_no_update; UPDATE corpus_campaigns SET body='{}';",
            )
            .unwrap();
        assert!(status(&registry, &id).is_err());
        registry
            .connection
            .execute_batch("PRAGMA foreign_keys=OFF; DROP TABLE corpus_campaigns;")
            .unwrap();
        assert!(integrity(&registry.connection).is_err());
    }
}
