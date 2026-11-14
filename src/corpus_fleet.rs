//! Durable round-robin visits across bounded, frozen synthetic corpus searches.
use crate::{
    Result,
    coordinator::{self, Kind, Lease},
    corpus_search, data, operator_config, resource,
    storage::Registry,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
const VERSION: &str = "synthetic-corpus-fleet-v1";
pub(crate) fn initialize(connection: &Connection) -> Result<()> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS corpus_fleets(id TEXT PRIMARY KEY,configuration TEXT NOT NULL,cursor INTEGER NOT NULL DEFAULT 0 CHECK(cursor>=0),state TEXT NOT NULL DEFAULT 'running' CHECK(state IN ('running','paused','stopped','exhausted')),token TEXT,expires_ms INTEGER,attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt>=0),visits INTEGER NOT NULL DEFAULT 0 CHECK(visits>=0),idle_visits INTEGER NOT NULL DEFAULT 0 CHECK(idle_visits>=0));
    CREATE TABLE IF NOT EXISTS corpus_fleet_sources(fleet_id TEXT NOT NULL REFERENCES corpus_fleets(id),source_index INTEGER NOT NULL CHECK(source_index>=0),visits INTEGER NOT NULL DEFAULT 0 CHECK(visits>=0),progress_visits INTEGER NOT NULL DEFAULT 0 CHECK(progress_visits>=0),last_kind TEXT NOT NULL DEFAULT 'unvisited',last_obstacle TEXT,obstacle_at_visit INTEGER,PRIMARY KEY(fleet_id,source_index));
    CREATE TRIGGER IF NOT EXISTS corpus_fleet_definition_immutable BEFORE UPDATE OF id,configuration ON corpus_fleets BEGIN SELECT RAISE(ABORT,'fleet definition immutable');END;
    CREATE TRIGGER IF NOT EXISTS corpus_fleet_no_delete BEFORE DELETE ON corpus_fleets BEGIN SELECT RAISE(ABORT,'fleet immutable');END;
    CREATE TRIGGER IF NOT EXISTS corpus_fleet_source_key_immutable BEFORE UPDATE OF fleet_id,source_index ON corpus_fleet_sources BEGIN SELECT RAISE(ABORT,'fleet source reference immutable');END;
    CREATE TRIGGER IF NOT EXISTS corpus_fleet_source_no_delete BEFORE DELETE ON corpus_fleet_sources BEGIN SELECT RAISE(ABORT,'fleet source immutable');END;").map_err(|e|e.to_string())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceFile {
    name: String,
    config: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FleetFile {
    sources: Vec<SourceFile>,
    coordinator_lease_ms: i64,
    #[serde(default)]
    resource_guard: Option<resource::Guard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    disk_guard: Option<crate::disk::Guard>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    name: String,
    config_path: String,
    search_id: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    version: String,
    sources: Vec<Reference>,
    coordinator_lease_ms: i64,
    resource_guard: Option<resource::Guard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    disk_guard: Option<crate::disk::Guard>,
}
#[derive(Serialize)]
pub struct Step {
    pub kind: String,
    pub visit: Option<serde_json::Value>,
    pub admission_observation: Option<resource::Decision>,
    pub disk_admission: Option<crate::disk::Decision>,
    pub status: serde_json::Value,
}
struct Source {
    index: usize,
    visits: i64,
    progress: i64,
    kind: String,
    obstacle: Option<String>,
    obstacle_at: Option<i64>,
}
struct Snapshot {
    definition: Definition,
    cursor: usize,
    state: String,
    attempt: i64,
    expiry: Option<i64>,
    visits: i64,
    idle: usize,
    sources: Vec<Source>,
}
type HeadRow = (Option<String>, usize, String, i64, Option<i64>, i64, usize);
fn validate(definition: &Definition) -> Result<()> {
    if definition.version != VERSION
        || !(1..=8).contains(&definition.sources.len())
        || !(1..=86_400_000).contains(&definition.coordinator_lease_ms)
    {
        return Err("invalid fleet definition bounds".into());
    }
    if let Some(guard) = &definition.resource_guard {
        guard.validate()?;
    }
    if let Some(guard) = &definition.disk_guard {
        guard.validate()?;
    }
    let mut names = std::collections::BTreeSet::new();
    let mut ids = std::collections::BTreeSet::new();
    for source in &definition.sources {
        if source.name.trim().is_empty()
            || source.name.len() > 64
            || source.config_path.len() > 4096
            || !Path::new(&source.config_path).is_absolute()
            || !crate::machine_evidence::hex(&source.search_id)
            || !names.insert(&source.name)
            || !ids.insert(&source.search_id)
        {
            return Err("invalid/duplicate fleet source reference".into());
        }
    }
    Ok(())
}
/// Freeze currently verifiable source/search identities, without starting workers.
pub fn register_file(config_path: &Path, database: &Path) -> Result<String> {
    let file: FleetFile = operator_config::read(config_path)?;
    if !(1..=8).contains(&file.sources.len()) {
        return Err("fleet requires1..8 sources".into());
    }
    let mut sources = Vec::new();
    for source in file.sources {
        let path = config_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(source.config)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let config_path = path
            .to_str()
            .ok_or("non-UTF8 fleet source path")?
            .to_owned();
        let loaded = operator_config::load_search(&path)?;
        sources.push(Reference {
            name: source.name,
            config_path,
            search_id: corpus_search::identity(&loaded.run())?,
        });
    }
    let definition = Definition {
        version: VERSION.into(),
        sources,
        coordinator_lease_ms: file.coordinator_lease_ms,
        resource_guard: file.resource_guard,
        disk_guard: file.disk_guard,
    };
    validate(&definition)?;
    let id = data::hash(&definition)?;
    let body = serde_json::to_string(&definition).map_err(|e| e.to_string())?;
    if body.len() > 65536 {
        return Err("fleet definition exceeds64 KiB".into());
    }
    let mut registry = Registry::open(database)?;
    let tx = registry
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let previous: Option<String> = tx
        .query_row(
            "SELECT configuration FROM corpus_fleets WHERE id=?1",
            [&id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(previous) = previous {
        if previous != body {
            return Err("immutable fleet definition mismatch".into());
        }
    } else {
        let count: usize = tx
            .query_row("SELECT count(*) FROM corpus_fleets", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if count >= 16 {
            return Err("fleet definition quota reached".into());
        }
        tx.execute(
            "INSERT INTO corpus_fleets(id,configuration) VALUES(?1,?2)",
            params![id, body],
        )
        .map_err(|e| e.to_string())?;
        for index in 0..definition.sources.len() {
            tx.execute(
                "INSERT INTO corpus_fleet_sources(fleet_id,source_index) VALUES(?1,?2)",
                params![id, index],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(id)
}
fn read(connection: &Connection, id: &str) -> Result<Snapshot> {
    let (body,cursor,state,attempt,expiry,visits,idle):HeadRow=connection.query_row("SELECT CASE WHEN length(CAST(configuration AS BLOB))<=65536 THEN configuration ELSE NULL END,cursor,state,attempt,expires_ms,visits,idle_visits FROM corpus_fleets WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(|e|e.to_string())?;
    let body = body.ok_or("fleet definition exceeds metadata budget")?;
    let definition: Definition = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    validate(&definition)?;
    if data::hash(&definition)? != id
        || cursor >= definition.sources.len()
        || visits < 0
        || cursor as i64 != visits % definition.sources.len() as i64
        || idle > definition.sources.len()
        || state == "running" && idle >= definition.sources.len()
    {
        return Err("fleet identity/rotation integrity failure".into());
    }
    let mut query=connection.prepare("SELECT source_index,visits,progress_visits,last_kind,last_obstacle,obstacle_at_visit FROM corpus_fleet_sources WHERE fleet_id=?1 ORDER BY source_index LIMIT 9").map_err(|e|e.to_string())?;
    let rows = query
        .query_map([id], |r| {
            Ok(Source {
                index: r.get(0)?,
                visits: r.get(1)?,
                progress: r.get(2)?,
                kind: r.get(3)?,
                obstacle: r.get(4)?,
                obstacle_at: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut sources = Vec::new();
    let mut sum = 0i64;
    for row in rows {
        let source = row.map_err(|e| e.to_string())?;
        if source.index != sources.len()
            || source.visits < 0
            || source.progress < 0
            || source.progress > source.visits
            || source
                .obstacle
                .as_ref()
                .is_some_and(|s| s.chars().count() > 512)
            || source.obstacle.is_some() != source.obstacle_at.is_some()
            || source
                .obstacle_at
                .is_some_and(|n| n < 1 || n > source.visits)
            || ![
                "unvisited",
                "advanced",
                "exhausted",
                "held",
                "busy",
                "held-or-busy",
                "blocked",
                "resource-pressure",
                "disk-pressure",
                "paused",
                "stopped",
            ]
            .contains(&source.kind.as_str())
        {
            return Err("fleet source-counter integrity failure".into());
        }
        sum = sum
            .checked_add(source.visits)
            .ok_or("fleet visit sum overflow")?;
        sources.push(source);
    }
    if sources.len() != definition.sources.len() || sum != visits {
        return Err("fleet source/visit count mismatch".into());
    }
    Ok(Snapshot {
        definition,
        cursor,
        state,
        attempt,
        expiry,
        visits,
        idle,
        sources,
    })
}
pub fn status(registry: &Registry, id: &str) -> Result<serde_json::Value> {
    let tx = registry
        .connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let snapshot = read(&tx, id)?;
    let sources:Vec<_>=snapshot.sources.iter().map(|source|{let reference=&snapshot.definition.sources[source.index];serde_json::json!({"index":source.index.to_string(),"name":reference.name,"search_id":reference.search_id,"visits":source.visits.to_string(),"progress_visits":source.progress.to_string(),"last_kind":source.kind,"last_obstacle":source.obstacle,"obstacle_at_visit":source.obstacle_at.map(|n|n.to_string())})}).collect();
    tx.commit().map_err(|e| e.to_string())?;
    Ok(
        serde_json::json!({"fleet_id":id,"state":snapshot.state,"next_source_index":snapshot.cursor.to_string(),"visits":snapshot.visits.to_string(),"idle_visits":snapshot.idle.to_string(),"attempt":snapshot.attempt.to_string(),"lease_active":snapshot.expiry.is_some_and(|t|t>chrono::Utc::now().timestamp_millis()),"sources":sources,"disk_reserve_bytes":snapshot.definition.disk_guard.as_ref().map(|g|g.minimum_available_bytes.to_string()),"qualification_granted":false,"limitations":"Round-robin bounded source visits, not equal CPU allocation, new simulation counts, complete error history or qualification. Returned source errors are isolated; filesystem-stall deadlines and hard OS enforcement remain separate."}),
    )
}
pub fn control(registry: &mut Registry, id: &str, state: &str) -> Result<()> {
    if !["running", "paused", "stopped"].contains(&state) {
        return Err("invalid fleet control".into());
    }
    let n=registry.connection.execute("UPDATE corpus_fleets SET state=?2,idle_visits=CASE WHEN ?2='running' THEN 0 ELSE idle_visits END WHERE id=?1 AND state IN ('running','paused')",params![id,state]).map_err(|e|e.to_string())?;
    if n != 1 {
        return Err("unknown or terminal fleet".into());
    }
    Ok(())
}
fn visit(
    registry: &Registry,
    database: &Path,
    reference: &Reference,
) -> Result<(String, bool, Option<String>, Option<serde_json::Value>)> {
    let exists: bool = registry
        .connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM corpus_searches WHERE id=?1)",
            [&reference.search_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists {
        let status = corpus_search::status(registry, &reference.search_id)?;
        if let Some(state) = status["state"].as_str()
            && ["paused", "stopped", "exhausted"].contains(&state)
        {
            return Ok((state.into(), false, None, Some(status)));
        }
    }
    let loaded = operator_config::load_search(Path::new(&reference.config_path))?;
    if corpus_search::identity(&loaded.run())? != reference.search_id {
        return Err("operator source no longer matches frozen search identity/build".into());
    }
    if let Some(guard) = &loaded.file.resource_guard {
        let decision = guard.check()?;
        if !decision.allowed {
            return Ok((
                "resource-pressure".into(),
                false,
                Some(decision.reasons.join("; ")),
                None,
            ));
        }
    }
    if let Some(guard) = &loaded.file.disk_guard {
        let decision = guard.check(database)?;
        if !decision.allowed {
            return Ok((
                "disk-pressure".into(),
                false,
                Some(decision.reasons.join("; ")),
                Some(serde_json::json!({"disk_admission":decision})),
            ));
        }
    }
    let child = corpus_search::step(database, loaded.run(), loaded.file.limits)?;
    let progressed = child.kind == "advanced" || child.kind == "exhausted";
    let kind = if child.kind == "advanced" && child.status["state"] == "paused" {
        "paused".into()
    } else {
        child.kind
    };
    Ok((kind, progressed, None, Some(child.status)))
}
fn publish(
    registry: &mut Registry,
    lease: &Lease,
    index: usize,
    kind: &str,
    progressed: bool,
    obstacle: Option<&str>,
) -> Result<()> {
    let tx = registry
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    coordinator::fence(&tx, lease, chrono::Utc::now().timestamp_millis())?;
    let snapshot = read(&tx, &lease.id)?;
    if snapshot.cursor != index {
        return Err("fleet cursor changed before publication".into());
    }
    let visits = snapshot
        .visits
        .checked_add(1)
        .ok_or("fleet visit counter overflow")?;
    let source = &snapshot.sources[index];
    let count = source
        .visits
        .checked_add(1)
        .ok_or("source visit counter overflow")?;
    let progress = source
        .progress
        .checked_add(i64::from(progressed))
        .ok_or("source progress counter overflow")?;
    tx.execute("UPDATE corpus_fleet_sources SET visits=?3,progress_visits=?4,last_kind=?5,last_obstacle=coalesce(?6,last_obstacle),obstacle_at_visit=CASE WHEN ?6 IS NOT NULL THEN ?3 ELSE obstacle_at_visit END WHERE fleet_id=?1 AND source_index=?2",params![lease.id,index,count,progress,kind,obstacle]).map_err(|e|e.to_string())?;
    let exhausted: usize = tx
        .query_row(
            "SELECT count(*) FROM corpus_fleet_sources WHERE fleet_id=?1 AND last_kind='exhausted'",
            [&lease.id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let idle = if progressed { 0 } else { snapshot.idle + 1 };
    let state = if exhausted == snapshot.definition.sources.len() {
        "exhausted"
    } else if idle >= snapshot.definition.sources.len() {
        "paused"
    } else {
        "running"
    };
    tx.execute(
        "UPDATE corpus_fleets SET cursor=?2,visits=?3,idle_visits=?4,state=?5 WHERE id=?1",
        params![
            lease.id,
            (index + 1) % snapshot.definition.sources.len(),
            visits,
            idle,
            state
        ],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}
pub fn step(database: &Path, id: &str) -> Result<Step> {
    let mut registry = Registry::open(database)?;
    let snapshot = read(&registry.connection, id)?;
    let disk_admission = snapshot
        .definition
        .disk_guard
        .as_ref()
        .map(|guard| guard.check(database))
        .transpose()?;
    if disk_admission.as_ref().is_some_and(|d| !d.allowed) {
        return Ok(Step {
            kind: "disk-pressure".into(),
            visit: None,
            admission_observation: None,
            disk_admission,
            status: status(&registry, id)?,
        });
    }
    let observation = snapshot
        .definition
        .resource_guard
        .as_ref()
        .map(resource::Guard::check)
        .transpose()?;
    if observation.as_ref().is_some_and(|d| !d.allowed) {
        return Ok(Step {
            kind: "resource-pressure".into(),
            visit: None,
            admission_observation: observation,
            disk_admission,
            status: status(&registry, id)?,
        });
    }
    let Some(lease) = coordinator::claim(
        &mut registry,
        Kind::Fleet,
        id,
        chrono::Utc::now().timestamp_millis(),
        snapshot.definition.coordinator_lease_ms,
    )?
    else {
        let status = status(&registry, id)?;
        return Ok(Step {
            kind: if status["state"] == "running" {
                "busy"
            } else if status["state"] == "exhausted" {
                "exhausted"
            } else {
                "held"
            }
            .into(),
            visit: None,
            admission_observation: observation,
            disk_admission,
            status,
        });
    };
    let result = (|| -> Result<Step> {
        let snapshot = read(&registry.connection, id)?;
        coordinator::fence(
            &registry.connection,
            &lease,
            chrono::Utc::now().timestamp_millis(),
        )?;
        let index = snapshot.cursor;
        let reference = &snapshot.definition.sources[index];
        let (kind, progressed, obstacle, child) = match visit(&registry, database, reference) {
            Ok(value) => value,
            Err(error) => (
                "blocked".into(),
                false,
                Some(error.chars().take(512).collect::<String>()),
                None,
            ),
        };
        publish(
            &mut registry,
            &lease,
            index,
            &kind,
            progressed,
            obstacle.as_deref(),
        )?;
        let status = status(&registry, id)?;
        let visit = serde_json::json!({"source_index":index.to_string(),"name":reference.name,"search_id":reference.search_id,"kind":kind,"progressed":progressed,"obstacle":obstacle,"child":child});
        Ok(Step {
            kind: if status["state"] == "exhausted" {
                "exhausted"
            } else if status["state"] == "paused" {
                "no-ready-sources"
            } else {
                "visited"
            }
            .into(),
            visit: Some(visit),
            admission_observation: observation.clone(),
            disk_admission: disk_admission.clone(),
            status,
        })
    })();
    let released = coordinator::release(&registry, &lease);
    match result {
        Err(error) => Err(error),
        Ok(mut step) => {
            released?;
            step.status = status(&registry, id)?;
            Ok(step)
        }
    }
}
pub(crate) fn integrity(registry: &Registry) -> Result<()> {
    let tx = registry
        .connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    let ids = {
        let mut query = tx
            .prepare("SELECT id FROM corpus_fleets LIMIT 17")
            .map_err(|e| e.to_string())?;
        query
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    if ids.len() > 16 {
        return Err("fleet inventory quota exceeded".into());
    }
    for id in ids {
        read(&tx, &id)?;
    }
    tx.commit().map_err(|e| e.to_string())
}

pub fn list(registry: &Registry) -> Result<Vec<serde_json::Value>> {
    let ids = {
        let mut query = registry
            .connection
            .prepare("SELECT id FROM corpus_fleets ORDER BY rowid DESC LIMIT 17")
            .map_err(|e| e.to_string())?;
        query
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    if ids.len() > 16 {
        return Err("fleet inventory quota exceeded".into());
    }
    ids.iter().map(|id| status(registry, id)).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> (std::path::PathBuf, std::path::PathBuf, String) {
        let root =
            std::env::temp_dir().join(format!("corpus-fleet-unit-{name}-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let data = crate::data::synthetic_fixture("EURUSD").unwrap();
        let native = root.join("source.rsf");
        crate::native::write(&native, &data).unwrap();
        let spec = crate::corpus::describe(&[native], 10000).unwrap();
        std::fs::write(root.join("corpus.json"), serde_json::to_vec(&spec).unwrap()).unwrap();
        std::fs::write(root.join("search.json"),serde_json::to_vec(&serde_json::json!({"specification":"corpus.json","seed":42,"maximum_candidates":2,"queue_width":2,"budget":{"max_candles":10000,"feature_cells":1000000,"max_receipts":10000},"limits":{"workers":2,"max_parallel_candles":20000,"max_parallel_feature_cells":2000000,"max_parallel_receipts":20000,"max_new_chunks":64,"lease_ms":60000}})).unwrap()).unwrap();
        let file = root.join("fleet.json");
        std::fs::write(&file,serde_json::to_vec(&serde_json::json!({"sources":[{"name":"one","config":"search.json"}],"coordinator_lease_ms":60000})).unwrap()).unwrap();
        let db = root.join("state.sqlite");
        let id = register_file(&file, &db).unwrap();
        (root, db, id)
    }
    #[test]
    fn omitted_disk_guard_preserves_legacy_fleet_definition_bytes() {
        let legacy = format!(
            r#"{{"version":"synthetic-corpus-fleet-v1","sources":[{{"name":"fixture","config_path":"/operator.json","search_id":"{}"}}],"coordinator_lease_ms":60000,"resource_guard":null}}"#,
            "a".repeat(64)
        );
        let definition: Definition = serde_json::from_str(&legacy).unwrap();
        validate(&definition).unwrap();
        assert_eq!(serde_json::to_string(&definition).unwrap(), legacy);
        assert_eq!(
            data::hash(&definition).unwrap(),
            blake3::hash(legacy.as_bytes()).to_hex().to_string()
        );
    }
    #[test]
    fn replaced_fleet_coordinators_cannot_publish_or_clear_new_leases() {
        let (root, db, id) = fixture("fence");
        let mut registry = Registry::open(&db).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let first = coordinator::claim(&mut registry, Kind::Fleet, &id, now, 100)
            .unwrap()
            .unwrap();
        let second = coordinator::claim(&mut registry, Kind::Fleet, &id, now + 101, 100)
            .unwrap()
            .unwrap();
        assert!(
            publish(
                &mut registry,
                &first,
                0,
                "blocked",
                false,
                Some("stale actor")
            )
            .is_err()
        );
        coordinator::release(&registry, &first).unwrap();
        coordinator::fence(&registry.connection, &second, now + 102).unwrap();
        assert_eq!(status(&registry, &id).unwrap()["visits"], "0");
        control(&mut registry, &id, "paused").unwrap();
        assert!(publish(&mut registry, &second, 0, "blocked", false, None).is_err());
        coordinator::release(&registry, &second).unwrap();
        registry.integrity().unwrap();
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn child_progress_before_rotation_publication_is_recovered_without_new_attempts() {
        let (root, db, id) = fixture("recovery");
        let mut registry = Registry::open(&db).unwrap();
        let lease = coordinator::claim(
            &mut registry,
            Kind::Fleet,
            &id,
            chrono::Utc::now().timestamp_millis(),
            60000,
        )
        .unwrap()
        .unwrap();
        let snapshot = read(&registry.connection, &id).unwrap();
        let (_, progressed, _, _) = visit(&registry, &db, &snapshot.definition.sources[0]).unwrap();
        assert!(progressed);
        let attempts = registry.counts().unwrap().attempts;
        registry
            .connection
            .execute(
                "UPDATE corpus_fleets SET expires_ms=?2 WHERE id=?1",
                params![id, chrono::Utc::now().timestamp_millis() - 1],
            )
            .unwrap();
        drop(registry);
        let recovered = step(&db, &id).unwrap();
        assert_eq!(recovered.kind, "exhausted");
        let registry = Registry::open(&db).unwrap();
        assert_eq!(registry.counts().unwrap().attempts, attempts);
        coordinator::release(&registry, &lease).unwrap();
        registry.integrity().unwrap();
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn corrupt_rotation_or_source_keys_refuse_verified_status() {
        let (root, db, id) = fixture("corrupt");
        let registry = Registry::open(&db).unwrap();
        assert!(
            registry
                .connection
                .execute(
                    "UPDATE corpus_fleet_sources SET source_index=1 WHERE fleet_id=?1",
                    [&id]
                )
                .is_err()
        );
        registry
            .connection
            .execute("UPDATE corpus_fleets SET visits=1 WHERE id=?1", [&id])
            .unwrap();
        assert!(status(&registry, &id).is_err());
        assert!(registry.integrity().is_err());
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
}
