//! Prespecified, immutable synthetic corpus campaign trial scopes; no execution yet.
use crate::{Result, corpus_search, data, operator_config, storage::Registry};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

const VERSION: &str = "synthetic-corpus-campaign-v1";
const MAX_BODY: usize = 65536;

pub(crate) fn initialize(connection: &mut Connection, migrate: bool) -> Result<()> {
    if !migrate {
        return Ok(());
    }
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS corpus_campaigns(id TEXT PRIMARY KEY,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=65536));
        CREATE TRIGGER IF NOT EXISTS corpus_campaign_no_update BEFORE UPDATE ON corpus_campaigns BEGIN SELECT RAISE(ABORT,'campaign immutable');END;
        CREATE TRIGGER IF NOT EXISTS corpus_campaign_no_delete BEFORE DELETE ON corpus_campaigns BEGIN SELECT RAISE(ABORT,'campaign immutable');END;").map_err(db)?;
    if migrate {
        tx.execute_batch("PRAGMA user_version=19;").map_err(db)?;
    }
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
    }
    tx.commit().map_err(db)?;
    Ok(id)
}
fn snapshot(plan: &Plan, id: &str, fresh: bool) -> serde_json::Value {
    let regions: Vec<_> = plan.regions.iter().enumerate().map(|(index,r)|serde_json::json!({"region":index.to_string(),"start_index":r.start_index.to_string(),"search_id":r.search_id})).collect();
    serde_json::json!({"campaign_id":id,"source_identity":plan.source_identity,"seed":plan.seed.to_string(),"start_index":plan.start_index.to_string(),"region_width":plan.region_width.to_string(),"regions":regions,"declared_slots":(plan.region_width * plan.regions.len()).to_string(),"distinct_experiments":plan.distinct_experiments.to_string(),"trial_scope_hash":plan.trial_scope_hash,"inputs_freshly_verified":fresh,"execution_supported":false,"qualification_granted":false,"limitations":"Frozen synthetic training trial scope only. Metadata inspection does not verify current inputs. No child registration, process start, execution or scientific family approval."})
}
pub fn status(registry: &Registry, id: &str) -> Result<serde_json::Value> {
    Ok(snapshot(&read(&registry.connection, id)?, id, false))
}
pub fn verify_inputs(registry: &Registry, id: &str) -> Result<serde_json::Value> {
    let stored = read(&registry.connection, id)?;
    let current = plan_file(Path::new(&stored.config_path), stored.regions.len())?;
    if data::hash(&current)? != id {
        return Err("campaign frozen input, build or trial scope changed".into());
    }
    Ok(snapshot(&stored, id, true))
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
        read(connection, &id.map_err(db)?)?;
    }
    Ok(())
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
            .execute_batch("DROP TABLE corpus_campaigns;")
            .unwrap();
        assert!(integrity(&registry.connection).is_err());
    }
}
