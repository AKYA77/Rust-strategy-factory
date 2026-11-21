//! Bounded durable visit fairness across frozen campaigns; never equal CPU shares.
use crate::{Result, corpus_campaign, data, operator_config, storage::Registry};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
const VERSION: &str = "campaign-rotation-v1";
fn db(e: rusqlite::Error) -> String {
    e.to_string()
}
fn hex(s: &str) -> bool {
    crate::machine_evidence::hex(s)
}
pub(crate) fn initialize(connection: &mut Connection, previous: i64) -> Result<()> {
    if previous >= 22 {
        return Ok(());
    }
    let tx = crate::storage::writer_tx_open(connection).map_err(|e| e.to_string())?;
    let current: i64 = tx
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db)?;
    if current >= 22 {
        return tx.commit().map_err(db);
    }
    tx.execute_batch("CREATE TABLE IF NOT EXISTS corpus_campaign_rotations(id TEXT PRIMARY KEY,configuration TEXT NOT NULL CHECK(length(CAST(configuration AS BLOB))<=65536),body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=65536),hash TEXT NOT NULL CHECK(length(hash)=64),state TEXT NOT NULL CHECK(state IN ('running','paused','stopped','exhausted')),origin TEXT,epoch INTEGER NOT NULL DEFAULT 0 CHECK(epoch>=0),token TEXT,expires_ms INTEGER,attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt>=0));
        CREATE TABLE IF NOT EXISTS campaign_rotation_owned(invocation_id TEXT PRIMARY KEY REFERENCES corpus_owned_invocations(id),rotation_id TEXT NOT NULL REFERENCES corpus_campaign_rotations(id),sequence INTEGER NOT NULL CHECK(sequence>0),source_index INTEGER NOT NULL CHECK(source_index BETWEEN 0 AND 7),body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=2048),hash TEXT NOT NULL CHECK(length(hash)=64),UNIQUE(rotation_id,sequence));
        CREATE TRIGGER IF NOT EXISTS campaign_rotation_identity BEFORE UPDATE OF id,configuration ON corpus_campaign_rotations BEGIN SELECT RAISE(ABORT,'rotation identity immutable');END;
        CREATE TRIGGER IF NOT EXISTS campaign_rotation_retained BEFORE DELETE ON corpus_campaign_rotations BEGIN SELECT RAISE(ABORT,'rotation retained');END;
        CREATE TRIGGER IF NOT EXISTS campaign_rotation_link_update BEFORE UPDATE ON campaign_rotation_owned BEGIN SELECT RAISE(ABORT,'rotation link immutable');END;
        CREATE TRIGGER IF NOT EXISTS campaign_rotation_link_delete BEFORE DELETE ON campaign_rotation_owned BEGIN SELECT RAISE(ABORT,'rotation link retained');END;
        PRAGMA user_version=22;").map_err(db)?;
    tx.commit().map_err(db)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Member {
    name: String,
    campaign_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    campaigns: Vec<Member>,
    coordinator_lease_ms: i64,
    #[serde(default)]
    resource_guard: Option<crate::resource::Guard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cpu_guard: Option<crate::resource::CpuGuard>,
    #[serde(default)]
    disk_guard: Option<crate::disk::Guard>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    version: String,
    campaigns: Vec<Member>,
    coordinator_lease_ms: i64,
    resource_guard: Option<crate::resource::Guard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cpu_guard: Option<crate::resource::CpuGuard>,
    disk_guard: Option<crate::disk::Guard>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Counts {
    visits: u64,
    progress: u64,
    children: u64,
    unknown_outcomes: u64,
    last_kind: String,
    last_outcome_hash: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    version: String,
    rotation_id: String,
    visits: u64,
    cursor: usize,
    idle: usize,
    completed: bool,
    members: Vec<Counts>,
    owned_hash: String,
}
struct Snapshot {
    definition: Definition,
    body: Body,
    state: String,
    origin: Option<String>,
    epoch: i64,
    attempt: i64,
    expiry: Option<i64>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Link {
    version: String,
    rotation_id: String,
    campaign_id: String,
    invocation_id: String,
    sequence: u64,
    source_index: usize,
    campaign_owned_hash: String,
    parent_hash: String,
    report_hash: String,
    previous_hash: String,
}
fn root(id: &str) -> Result<String> {
    data::hash(&("campaign-rotation-owned-v1", id))
}
fn validate_definition(d: &Definition) -> Result<()> {
    if d.version != VERSION
        || !(1..=8).contains(&d.campaigns.len())
        || !(1..=86400000).contains(&d.coordinator_lease_ms)
    {
        return Err("invalid campaign rotation bounds".into());
    }
    if let Some(g) = &d.resource_guard {
        g.validate()?;
    }
    if let Some(g) = &d.cpu_guard {
        g.validate()?;
    }
    if let Some(g) = &d.disk_guard {
        g.validate()?;
    }
    let mut names = std::collections::BTreeSet::new();
    let mut ids = std::collections::BTreeSet::new();
    for m in &d.campaigns {
        if m.name.trim().is_empty()
            || m.name.len() > 64
            || !hex(&m.campaign_id)
            || !names.insert(&m.name)
            || !ids.insert(&m.campaign_id)
        {
            return Err("invalid or duplicate campaign rotation member".into());
        }
    }
    Ok(())
}
pub fn register_file(path: &Path, database: &Path) -> Result<String> {
    let file: File = operator_config::read(path)?;
    let definition = Definition {
        version: VERSION.into(),
        campaigns: file.campaigns,
        coordinator_lease_ms: file.coordinator_lease_ms,
        resource_guard: file.resource_guard,
        cpu_guard: file.cpu_guard,
        disk_guard: file.disk_guard,
    };
    validate_definition(&definition)?;
    let id = data::hash(&definition)?;
    let configuration = serde_json::to_string(&definition).map_err(|e| e.to_string())?;
    if configuration.len() > 65536 {
        return Err("rotation definition exceeds cap".into());
    }
    let registry = Registry::open(database)?;
    let tx = crate::storage::writer_tx(&registry.connection)?;
    for member in &definition.campaigns {
        corpus_campaign::status_on(&tx, &member.campaign_id)?;
    }
    let prior: Option<String> = tx
        .query_row(
            "SELECT configuration FROM corpus_campaign_rotations WHERE id=?1",
            [&id],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?;
    if let Some(prior) = prior {
        if prior != configuration {
            return Err("immutable rotation definition mismatch".into());
        }
        read(&tx, &id)?;
    } else {
        let (count,bytes):(usize,usize)=tx.query_row("SELECT count(*),coalesce(sum(length(CAST(configuration AS BLOB))),0) FROM corpus_campaign_rotations",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db)?;
        if count >= 16 || bytes + configuration.len() > 1048576 {
            return Err("campaign rotation metadata quota reached".into());
        }
        let members = definition
            .campaigns
            .iter()
            .map(|_| Counts {
                visits: 0,
                progress: 0,
                children: 0,
                unknown_outcomes: 0,
                last_kind: "unvisited".into(),
                last_outcome_hash: None,
            })
            .collect();
        let body = Body {
            version: VERSION.into(),
            rotation_id: id.clone(),
            visits: 0,
            cursor: 0,
            idle: 0,
            completed: false,
            members,
            owned_hash: root(&id)?,
        };
        tx.execute("INSERT INTO corpus_campaign_rotations(id,configuration,body,hash,state) VALUES(?1,?2,?3,?4,'running')",params![id,configuration,serde_json::to_string(&body).map_err(|e|e.to_string())?,data::hash(&body)?]).map_err(db)?;
    }
    tx.commit().map_err(db)?;
    Ok(id)
}
type Row = (
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
    i64,
    i64,
    Option<String>,
    Option<i64>,
);
fn read(connection: &Connection, id: &str) -> Result<Snapshot> {
    if !hex(id) {
        return Err("invalid rotation identity".into());
    }
    let (definition,body,hash,state,origin,epoch,attempt,token,expiry):Row=connection.query_row("SELECT CASE WHEN length(CAST(configuration AS BLOB))<=65536 THEN configuration ELSE NULL END,CASE WHEN length(CAST(body AS BLOB))<=65536 THEN body ELSE NULL END,hash,state,origin,epoch,attempt,token,expires_ms FROM corpus_campaign_rotations WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).map_err(db)?;
    let definition: Definition =
        serde_json::from_str(&definition.ok_or("rotation definition cap exceeded")?)
            .map_err(|e| e.to_string())?;
    validate_definition(&definition)?;
    let body: Body = serde_json::from_str(&body.ok_or("rotation state cap exceeded")?)
        .map_err(|e| e.to_string())?;
    let valid_origin = match state.as_str() {
        "paused" => matches!(origin.as_deref(), Some("operator" | "scheduler_idle")),
        "running" | "stopped" | "exhausted" => origin.is_none(),
        _ => false,
    };
    if data::hash(&definition)? != id
        || data::hash(&body)? != hash
        || body.version != VERSION
        || body.rotation_id != id
        || body.members.len() != definition.campaigns.len()
        || body.cursor != (body.visits % definition.campaigns.len() as u64) as usize
        || body.idle > definition.campaigns.len()
        || body.completed != (state == "exhausted")
        || state == "running" && body.idle >= definition.campaigns.len()
        || !valid_origin
        || epoch < 0
        || attempt < 0
        || body.visits > attempt as u64
        || token.is_some() != expiry.is_some()
        || token.as_deref().is_some_and(|s| !hex(s))
        || expiry.is_some_and(|n| chrono::DateTime::from_timestamp_millis(n).is_none())
    {
        return Err("rotation head integrity failure".into());
    }
    let mut visits = 0u64;
    for m in &body.members {
        if m.progress > m.visits
            || m.children
                .checked_add(m.unknown_outcomes)
                .is_none_or(|n| n > m.visits)
            || m.last_outcome_hash.is_some() != (m.visits > 0)
            || m.last_outcome_hash.as_deref().is_some_and(|s| !hex(s))
            || !matches!(
                m.last_kind.as_str(),
                "unvisited"
                    | "advanced"
                    | "advanced_region"
                    | "exhausted"
                    | "held"
                    | "backoff"
                    | "pressure"
                    | "busy_or_held"
                    | "cursor_changed"
                    | "signal"
                    | "source_error"
            )
            || (m.last_kind == "unvisited") != (m.visits == 0)
        {
            return Err("rotation member counters invalid".into());
        }
        visits = visits
            .checked_add(m.visits)
            .ok_or("rotation counter overflow")?;
    }
    if visits != body.visits {
        return Err("rotation visit prefix mismatch".into());
    }
    let snapshot = Snapshot {
        definition,
        body,
        state,
        origin,
        epoch,
        attempt,
        expiry,
    };
    costs(connection, &snapshot)?;
    Ok(snapshot)
}
#[derive(Default)]
struct Cost {
    children: u64,
    cpu_observed: u64,
    cpu_unknown: u64,
    cpu_ticks: u128,
    elapsed_observed: u64,
    elapsed_unknown: u64,
    elapsed_ms: u128,
}
fn costs(connection: &Connection, s: &Snapshot) -> Result<Vec<Cost>> {
    let total: usize = connection
        .query_row("SELECT count(*) FROM campaign_rotation_owned", [], |r| {
            r.get(0)
        })
        .map_err(db)?;
    if total > 128 {
        return Err("rotation owned link quota exceeded".into());
    }
    let mut costs: Vec<Cost> = s
        .definition
        .campaigns
        .iter()
        .map(|_| Cost::default())
        .collect();
    let mut previous = root(&s.body.rotation_id)?;
    let mut last = 0u64;
    let mut q=connection.prepare("SELECT invocation_id,sequence,source_index,body,hash FROM campaign_rotation_owned WHERE rotation_id=?1 ORDER BY sequence").map_err(db)?;
    let rows = q
        .query_map([&s.body.rotation_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u64>(1)?,
                r.get::<_, usize>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(db)?;
    for row in rows {
        let (invocation, sequence, index, body, hash) = row.map_err(db)?;
        if body.len() > 2048 {
            return Err("rotation link body cap exceeded".into());
        }
        let item: Link = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        if item.version != "rotation-owned-v1"
            || item.rotation_id != s.body.rotation_id
            || item.invocation_id != invocation
            || item.sequence != sequence
            || sequence <= last
            || sequence > s.body.visits
            || index >= costs.len()
            || item.source_index != index
            || index != ((sequence - 1) % costs.len() as u64) as usize
            || item.campaign_id != s.definition.campaigns[index].campaign_id
            || item.previous_hash != previous
            || data::hash(&item)? != hash
        {
            return Err("rotation owned link chain mismatch".into());
        }
        let (campaign, parent_hash, report_hash, campaign_hash) =
            corpus_campaign::owned_binding(connection, &invocation)?;
        let parent = crate::owned_history::status_on(connection, &invocation)?;
        if campaign != item.campaign_id
            || parent_hash != item.parent_hash
            || report_hash != item.report_hash
            || campaign_hash != item.campaign_owned_hash
            || parent["observation_hash"] != item.parent_hash
            || parent["summary"]["report_hash"] != item.report_hash
            || parent["finished_at_ms"].is_null()
        {
            return Err("rotation owned parent binding mismatch".into());
        }
        let cost = &mut costs[index];
        cost.children += 1;
        if let Some(cpu) = parent["resources"]["last_cpu_ticks"].as_u64() {
            cost.cpu_observed += 1;
            cost.cpu_ticks = cost
                .cpu_ticks
                .checked_add(cpu as u128)
                .ok_or("rotation CPU sum overflow")?;
        } else {
            cost.cpu_unknown += 1;
        }
        if let Some(elapsed) = parent["summary"]["elapsed_ms"]
            .as_str()
            .and_then(|n| n.parse::<u128>().ok())
        {
            cost.elapsed_observed += 1;
            cost.elapsed_ms = cost
                .elapsed_ms
                .checked_add(elapsed)
                .ok_or("rotation elapsed sum overflow")?;
        } else {
            cost.elapsed_unknown += 1;
        }
        previous = hash;
        last = sequence;
    }
    if previous != s.body.owned_hash
        || costs
            .iter()
            .zip(&s.body.members)
            .any(|(c, m)| c.children != m.children)
    {
        return Err("rotation owned append head/count mismatch".into());
    }
    Ok(costs)
}
fn snapshot(connection: &Connection, id: &str, s: &Snapshot) -> Result<serde_json::Value> {
    let costs = costs(connection, s)?;
    let mut members = Vec::new();
    for (index, ((member, count), cost)) in s
        .definition
        .campaigns
        .iter()
        .zip(&s.body.members)
        .zip(costs)
        .enumerate()
    {
        let current = corpus_campaign::status_on(connection, &member.campaign_id).ok();
        members.push(serde_json::json!({"index":index.to_string(),"name":member.name,"campaign_id":member.campaign_id,"visits":count.visits.to_string(),"progress_visits":count.progress.to_string(),"known_children":count.children.to_string(),"unknown_outcome_visits":count.unknown_outcomes.to_string(),"last_kind":count.last_kind,"last_outcome_hash":count.last_outcome_hash,"cpu_observed_children":cost.cpu_observed.to_string(),"cpu_unobserved_children":cost.cpu_unknown.to_string(),"observed_native_cpu_ticks":cost.cpu_ticks.to_string(),"elapsed_observed_children":cost.elapsed_observed.to_string(),"elapsed_unobserved_children":cost.elapsed_unknown.to_string(),"observed_parent_elapsed_ms":cost.elapsed_ms.to_string(),"current_metadata_available":current.is_some(),"current_state":current.as_ref().map(|v|v["execution"]["state"].clone())}));
    }
    Ok(
        serde_json::json!({"rotation_id":id,"state":s.state,"pause_origin":s.origin,"epoch":s.epoch.to_string(),"attempt":s.attempt.to_string(),"visits":s.body.visits.to_string(),"next_member_index":s.body.cursor.to_string(),"idle_visits":s.body.idle.to_string(),"lease_active":s.expiry.is_some_and(|n|n>chrono::Utc::now().timestamp_millis()),"owned_hash":s.body.owned_hash,"members":members,"qualification_granted":false,"limitations":"Bounded round-robin visit fairness, not equal CPU allocation or new simulations. CPU and elapsed costs cover linked observed parents only; unavailable outcomes are unknown, never zero. Metadata availability does not verify current source eligibility. Failed outer publication may retain unlinked work in the campaign/parent histories."}),
    )
}
pub fn status(registry: &Registry, id: &str) -> Result<serde_json::Value> {
    let tx = registry.connection.unchecked_transaction().map_err(db)?;
    let s = read(&tx, id)?;
    let value = snapshot(&tx, id, &s)?;
    tx.commit().map_err(db)?;
    Ok(value)
}
pub fn list(registry: &Registry) -> Result<Vec<serde_json::Value>> {
    let tx = registry.connection.unchecked_transaction().map_err(db)?;
    integrity(&tx)?;
    let ids = {
        let mut q = tx
            .prepare("SELECT id FROM corpus_campaign_rotations ORDER BY id LIMIT 17")
            .map_err(db)?;
        q.query_map([], |r| r.get::<_, String>(0))
            .map_err(db)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)?
    };
    let rows = ids
        .iter()
        .map(|id| snapshot(&tx, id, &read(&tx, id)?))
        .collect::<Result<Vec<_>>>()?;
    tx.commit().map_err(db)?;
    Ok(rows)
}
pub(crate) fn epoch(connection: &Connection, id: &str) -> Result<i64> {
    Ok(read(connection, id)?.epoch)
}
fn write(connection: &Connection, s: &Snapshot) -> Result<()> {
    let body = serde_json::to_string(&s.body).map_err(|e| e.to_string())?;
    if body.len() > 65536 {
        return Err("rotation state exceeds cap".into());
    }
    connection.execute("UPDATE corpus_campaign_rotations SET body=?2,hash=?3,state=?4,origin=?5,epoch=?6 WHERE id=?1",params![s.body.rotation_id,body,data::hash(&s.body)?,s.state,s.origin,s.epoch]).map_err(db)?;
    Ok(())
}
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
        return Err("invalid rotation control".into());
    }
    let tx = crate::storage::writer_tx(&registry.connection)?;
    let mut s = read(&tx, id)?;
    if expected_epoch.is_some_and(|epoch| epoch != s.epoch) {
        return Err("stale rotation control epoch".into());
    }
    if s.state == state && (state != "paused" || s.origin.as_deref() == Some("operator")) {
        return tx.commit().map_err(db);
    }
    if ["stopped", "exhausted"].contains(&s.state.as_str()) {
        return Err("rotation terminal state is immutable".into());
    }
    s.state = state.into();
    s.origin = (state == "paused").then(|| "operator".into());
    s.epoch = s.epoch.checked_add(1).ok_or("rotation epoch overflow")?;
    if state == "running" {
        s.body.idle = 0;
    }
    write(&tx, &s)?;
    tx.execute(
        "UPDATE corpus_campaign_rotations SET token=NULL,expires_ms=NULL WHERE id=?1",
        [id],
    )
    .map_err(db)?;
    tx.commit().map_err(db)
}
fn append_link(
    connection: &Connection,
    s: &mut Snapshot,
    index: usize,
    invocation: &str,
) -> Result<()> {
    let (campaign_id, parent_hash, report_hash, campaign_owned_hash) =
        corpus_campaign::owned_binding(connection, invocation)?;
    if campaign_id != s.definition.campaigns[index].campaign_id {
        return Err("rotation member invocation mismatch".into());
    }
    let item = Link {
        version: "rotation-owned-v1".into(),
        rotation_id: s.body.rotation_id.clone(),
        campaign_id,
        invocation_id: invocation.into(),
        sequence: s.body.visits,
        source_index: index,
        campaign_owned_hash,
        parent_hash,
        report_hash,
        previous_hash: s.body.owned_hash.clone(),
    };
    let hash = data::hash(&item)?;
    let total: usize = connection
        .query_row("SELECT count(*) FROM campaign_rotation_owned", [], |r| {
            r.get(0)
        })
        .map_err(db)?;
    if total >= 128 {
        return Err("rotation owned link quota reached".into());
    }
    connection.execute("INSERT INTO campaign_rotation_owned(invocation_id,rotation_id,sequence,source_index,body,hash) VALUES(?1,?2,?3,?4,?5,?6)",params![invocation,s.body.rotation_id,s.body.visits,index,serde_json::to_string(&item).map_err(|e|e.to_string())?,hash]).map_err(db)?;
    s.body.owned_hash = hash;
    Ok(())
}
pub fn step(
    database: &Path,
    id: &str,
    supervision: &corpus_campaign::Supervision<'_>,
) -> Result<serde_json::Value> {
    supervision.limits.validate()?;
    let policy: crate::retry_policy::Policy = operator_config::read(supervision.policy)?;
    policy.validate()?;
    let mut registry = Registry::open(database)?;
    let s = read(&registry.connection, id)?;
    if s.state != "running" {
        return Ok(
            serde_json::json!({"kind":if s.state=="exhausted"{"exhausted"}else{"held"},"status":status(&registry,id)?}),
        );
    }
    let required = supervision
        .limits
        .maximum_wall_ms
        .checked_add(supervision.limits.shutdown_grace_ms)
        .and_then(|n| n.checked_add(20000))
        .ok_or("rotation lease margin overflow")?;
    if s.definition.coordinator_lease_ms
        <= i64::try_from(required).map_err(|_| "rotation lease margin overflow")?
    {
        return Err("rotation lease must exceed child wall plus grace plus 20000 ms".into());
    }
    if supervision
        .shutdown
        .load(std::sync::atomic::Ordering::Acquire)
    {
        return Ok(serde_json::json!({"kind":"signal","status":status(&registry,id)?}));
    }
    let disk = s
        .definition
        .disk_guard
        .as_ref()
        .map(|g| g.check(database))
        .transpose()?;
    let resource = s
        .definition
        .resource_guard
        .as_ref()
        .map(|g| g.check())
        .transpose()?;
    let cpu = s
        .definition
        .cpu_guard
        .as_ref()
        .map(|g| g.check())
        .transpose()?;
    if disk.as_ref().is_some_and(|d| !d.allowed)
        || resource.as_ref().is_some_and(|d| !d.allowed)
        || cpu.as_ref().is_some_and(|d| !d.allowed)
    {
        return Ok(
            serde_json::json!({"kind":"pressure","disk_admission":disk,"resource_admission":resource,"cpu_admission":cpu,"status":status(&registry,id)?}),
        );
    }
    let Some(lease) = crate::coordinator::claim(
        &mut registry,
        crate::coordinator::Kind::CampaignRotation,
        id,
        chrono::Utc::now().timestamp_millis(),
        s.definition.coordinator_lease_ms,
    )?
    else {
        return Ok(serde_json::json!({"kind":"busy_or_held","status":status(&registry,id)?}));
    };
    let result = (|| -> Result<serde_json::Value> {
        let current = read(&registry.connection, id)?;
        crate::coordinator::fence(
            &registry.connection,
            &lease,
            chrono::Utc::now().timestamp_millis(),
        )?;
        if current.body.cursor != s.body.cursor {
            return Ok(serde_json::json!({"kind":"cursor_changed"}));
        }
        let index = current.body.cursor;
        let campaign = &current.definition.campaigns[index].campaign_id;
        let before = corpus_campaign::status(&registry, campaign).ok();
        let child = corpus_campaign::supervised_step(database, campaign, supervision);
        let tx = crate::storage::writer_tx(&registry.connection)?;
        crate::coordinator::fence(&tx, &lease, chrono::Utc::now().timestamp_millis())?;
        let mut current = read(&tx, id)?;
        let (kind, outcome, unknown, progress) = match &child {
            Ok(value) => {
                let kind = value["kind"].as_str().unwrap_or("source_error");
                let allowed = matches!(
                    kind,
                    "advanced"
                        | "advanced_region"
                        | "exhausted"
                        | "held"
                        | "backoff"
                        | "pressure"
                        | "busy_or_held"
                        | "cursor_changed"
                        | "signal"
                );
                let progress = before.as_ref().is_some_and(|old| {
                    old["execution"]["next_region"] != value["status"]["execution"]["next_region"]
                }) || matches!(
                    value["owned"]["child_report"]["last"]["progress"]["kind"].as_str(),
                    Some("advanced" | "exhausted")
                );
                (
                    if allowed { kind } else { "source_error" }.to_owned(),
                    data::hash(value)?,
                    !allowed && value["owned"]["invocation_id"].as_str().is_none(),
                    progress,
                )
            }
            Err(error) => (
                "source_error".into(),
                data::hash(&("source_error", error))?,
                true,
                false,
            ),
        };
        current.body.visits = current
            .body
            .visits
            .checked_add(1)
            .ok_or("rotation visit overflow")?;
        current.body.cursor = (index + 1) % current.definition.campaigns.len();
        current.body.idle = if progress { 0 } else { current.body.idle + 1 };
        let count = &mut current.body.members[index];
        count.visits = count.visits.checked_add(1).ok_or("member visit overflow")?;
        count.progress += u64::from(progress);
        count.unknown_outcomes += u64::from(unknown);
        count.last_kind = kind;
        count.last_outcome_hash = Some(outcome);
        if let Ok(value) = &child
            && let Some(invocation) = value["owned"]["invocation_id"].as_str()
        {
            count.children += 1;
            append_link(&tx, &mut current, index, invocation)?;
        }
        current.body.completed = current.definition.campaigns.iter().all(|m| {
            corpus_campaign::status_on(&tx, &m.campaign_id)
                .is_ok_and(|v| v["execution"]["state"] == "exhausted")
        });
        if current.body.completed || current.body.idle >= current.definition.campaigns.len() {
            current.state = if current.body.completed {
                "exhausted"
            } else {
                "paused"
            }
            .into();
            current.origin = (!current.body.completed).then(|| "scheduler_idle".into());
            current.epoch = current
                .epoch
                .checked_add(1)
                .ok_or("rotation epoch overflow")?;
            tx.execute(
                "UPDATE corpus_campaign_rotations SET token=NULL,expires_ms=NULL WHERE id=?1",
                [id],
            )
            .map_err(db)?;
        }
        write(&tx, &current)?;
        read(&tx, id)?;
        tx.commit().map_err(db)?;
        Ok(
            serde_json::json!({"kind":"visited","source_index":index.to_string(),"outcome":match child{Ok(value)=>value,Err(error)=>serde_json::json!({"kind":"source_error","message":error})}}),
        )
    })();
    let released = crate::coordinator::release(&registry, &lease);
    let mut value = result?;
    released?;
    value["status"] = status(&registry, id)?;
    Ok(value)
}
pub fn run(
    database: &Path,
    id: &str,
    supervision: &corpus_campaign::Supervision<'_>,
    maximum: usize,
) -> Result<serde_json::Value> {
    if !(1..=10000).contains(&maximum) {
        return Err("rotation visit bound must be 1..10000".into());
    }
    let mut visits = 0usize;
    let mut last = None;
    let mut reason = "visit_limit";
    for _ in 0..maximum {
        if supervision
            .shutdown
            .load(std::sync::atomic::Ordering::Acquire)
        {
            reason = "signal";
            break;
        }
        let value = step(database, id, supervision)?;
        let visited = value["kind"] == "visited";
        visits += usize::from(visited);
        if !visited {
            reason = match value["kind"].as_str() {
                Some("pressure") => "pressure",
                Some("signal") => "signal",
                Some("exhausted") => "exhausted",
                Some("busy_or_held") => "busy_or_held",
                _ => "held",
            };
        } else if value["status"]["state"] != "running" {
            reason = if value["status"]["state"] == "exhausted" {
                "exhausted"
            } else {
                "idle"
            };
        }
        let keep = visited && value["status"]["state"] == "running";
        last = Some(value);
        if !keep {
            break;
        }
    }
    Ok(
        serde_json::json!({"rotation_id":id,"visits":visits.to_string(),"stop_reason":reason,"last":last,"qualification_granted":false}),
    )
}
pub(crate) fn integrity(connection: &Connection) -> Result<()> {
    let (count,bytes):(usize,usize)=connection.query_row("SELECT count(*),coalesce(sum(length(CAST(configuration AS BLOB))+length(CAST(body AS BLOB))),0) FROM corpus_campaign_rotations",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db)?;
    if count > 16 || bytes > 2097152 {
        return Err("rotation persisted metadata quota exceeded".into());
    }
    let mut q = connection
        .prepare("SELECT id FROM corpus_campaign_rotations")
        .map_err(db)?;
    let ids = q.query_map([], |r| r.get::<_, String>(0)).map_err(db)?;
    for id in ids {
        read(connection, &id.map_err(db)?)?;
    }
    Ok(())
}

fn retry_wakeup(
    connection: &Connection,
    s: &Snapshot,
    policy: &crate::retry_policy::Policy,
    now: i64,
) -> Option<i64> {
    s.definition
        .campaigns
        .iter()
        .zip(&s.body.members)
        .filter(|(_, m)| m.last_kind != "source_error")
        .filter_map(|(m, _)| {
            corpus_campaign::retry_wakeup_in(connection, &m.campaign_id, policy, now)
                .ok()
                .flatten()
        })
        .min()
}
fn resume_idle(registry: &mut Registry, id: &str, expected_epoch: i64) -> Result<bool> {
    let tx = crate::storage::writer_tx(&registry.connection)?;
    let mut s = read(&tx, id)?;
    if s.state != "paused"
        || s.origin.as_deref() != Some("scheduler_idle")
        || s.epoch != expected_epoch
    {
        return Ok(false);
    }
    s.state = "running".into();
    s.origin = None;
    s.epoch = s.epoch.checked_add(1).ok_or("rotation epoch overflow")?;
    s.body.idle = 0;
    write(&tx, &s)?;
    tx.execute(
        "UPDATE corpus_campaign_rotations SET token=NULL,expires_ms=NULL WHERE id=?1",
        [id],
    )
    .map_err(db)?;
    tx.commit().map_err(db)?;
    Ok(true)
}
/// Cooperative wait budget, not a hard invocation/filesystem deadline.
pub fn run_wait(
    database: &Path,
    id: &str,
    supervision: &corpus_campaign::Supervision<'_>,
    maximum: usize,
    total_wait_ms: u64,
) -> Result<serde_json::Value> {
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};
    if !(1..=10000).contains(&maximum) || total_wait_ms > 3600000 {
        return Err("rotation loop requires 1..10000 visits and 0..3600000 wait ms".into());
    }
    supervision.limits.validate()?;
    let policy: crate::retry_policy::Policy = operator_config::read(supervision.policy)?;
    policy.validate()?;
    let policy_hash = data::hash(&policy)?;
    let mut visits = 0usize;
    let mut checks = 0usize;
    let mut waited = Duration::ZERO;
    let budget = Duration::from_millis(total_wait_ms);
    let mut last = None;
    let mut retry_at = None;
    let mut reason = "check_limit";
    'outer: while checks < maximum * 2 + 1 && visits < maximum {
        if supervision.shutdown.load(Ordering::Acquire) {
            reason = "signal";
            break;
        }
        let current_policy: crate::retry_policy::Policy =
            operator_config::read(supervision.policy)?;
        if data::hash(&current_policy)? != policy_hash {
            return Err("rotation loop policy changed during invocation".into());
        }
        let mut registry = Registry::open(database)?;
        let state = read(&registry.connection, id)?;
        checks += 1;
        if state.state == "exhausted" {
            reason = "exhausted";
            break;
        }
        if state.state == "paused" && state.origin.as_deref() == Some("scheduler_idle") {
            let now = chrono::Utc::now().timestamp_millis();
            let Some(deadline) = retry_wakeup(&registry.connection, &state, &policy, now) else {
                reason = "idle";
                break;
            };
            retry_at = Some(deadline);
            if now >= deadline {
                if !resume_idle(&mut registry, id, state.epoch)? {
                    reason = "held";
                    break;
                }
                continue;
            }
            if budget.is_zero() {
                reason = "backoff";
                break;
            }
            let remaining = budget.saturating_sub(waited);
            if remaining.is_zero() {
                reason = "wait_budget";
                break;
            }
            let target = Duration::from_millis((deadline - now) as u64)
                .max(Duration::from_millis(100))
                .min(remaining);
            let started = Instant::now();
            while started.elapsed() < target {
                if supervision.shutdown.load(Ordering::Acquire) {
                    waited += started.elapsed();
                    reason = "signal";
                    break 'outer;
                }
                let live_policy: crate::retry_policy::Policy =
                    operator_config::read(supervision.policy)?;
                if data::hash(&live_policy)? != policy_hash {
                    return Err("rotation loop policy changed during wait".into());
                }
                let current = read(&registry.connection, id)?;
                if current.state != "paused"
                    || current.origin.as_deref() != Some("scheduler_idle")
                    || current.epoch != state.epoch
                {
                    waited += started.elapsed();
                    reason = "held";
                    break 'outer;
                }
                std::thread::sleep(
                    Duration::from_millis(50).min(target.saturating_sub(started.elapsed())),
                );
            }
            waited += started.elapsed();
            continue;
        }
        if state.state != "running" {
            reason = "held";
            break;
        }
        drop(registry);
        let value = step(database, id, supervision)?;
        let visited = value["kind"] == "visited";
        visits += usize::from(visited);
        if !visited {
            reason = match value["kind"].as_str() {
                Some("pressure") => "pressure",
                Some("busy_or_held") => "busy_or_held",
                Some("signal") => "signal",
                _ => "held",
            };
            last = Some(value);
            break;
        }
        last = Some(value);
    }
    if supervision.shutdown.load(Ordering::Acquire) {
        reason = "signal";
    } else if visits >= maximum {
        reason = "visit_limit";
    }
    Ok(
        serde_json::json!({"rotation_id":id,"visits":visits.to_string(),"checks":checks.to_string(),"waited_ms":waited.as_millis().to_string(),"retry_at_ms":retry_at.map(|n|n.to_string()),"stop_reason":reason,"last":last,"qualification_granted":false,"limitations":"Foreground cumulative cooperative waiting for typed eligible source retries only. Source errors and operator/terminal holds do not auto-resume; no hard filesystem/kernel deadline or new simulation count is implied."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Registry, String) {
        let registry = Registry::open(Path::new(":memory:")).unwrap();
        let definition = Definition {
            version: VERSION.into(),
            campaigns: vec![
                Member {
                    name: "one".into(),
                    campaign_id: "a".repeat(64),
                },
                Member {
                    name: "two".into(),
                    campaign_id: "b".repeat(64),
                },
            ],
            coordinator_lease_ms: 60000,
            resource_guard: None,
            cpu_guard: None,
            disk_guard: None,
        };
        let id = data::hash(&definition).unwrap();
        let body = Body {
            version: VERSION.into(),
            rotation_id: id.clone(),
            visits: 0,
            cursor: 0,
            idle: 0,
            completed: false,
            members: (0..2)
                .map(|_| Counts {
                    visits: 0,
                    progress: 0,
                    children: 0,
                    unknown_outcomes: 0,
                    last_kind: "unvisited".into(),
                    last_outcome_hash: None,
                })
                .collect(),
            owned_hash: root(&id).unwrap(),
        };
        registry.connection.execute("INSERT INTO corpus_campaign_rotations(id,configuration,body,hash,state) VALUES(?1,?2,?3,?4,'running')",params![id,serde_json::to_string(&definition).unwrap(),serde_json::to_string(&body).unwrap(),data::hash(&body).unwrap()]).unwrap();
        (registry, id)
    }
    #[test]
    fn reviewed_rotation_epoch_rejects_stale_idempotence_and_preserves_current_nonce() {
        use crate::coordinator::{self, Kind};
        let (mut registry, id) = fixture();
        let now = chrono::Utc::now().timestamp_millis();
        let lease = coordinator::claim(&mut registry, Kind::CampaignRotation, &id, now, 60000)
            .unwrap()
            .unwrap();
        control_expected(&mut registry, &id, "running", 0).unwrap();
        coordinator::fence(&registry.connection, &lease, now).unwrap();
        control_expected(&mut registry, &id, "paused", 0).unwrap();
        let held = status(&registry, &id).unwrap();
        for state in ["running", "paused", "stopped"] {
            assert!(control_expected(&mut registry, &id, state, 0).is_err());
            assert_eq!(status(&registry, &id).unwrap(), held);
        }
        assert_eq!(list(&registry).unwrap(), vec![held]);
        control_expected(&mut registry, &id, "running", 1).unwrap();
        assert!(coordinator::fence(&registry.connection, &lease, now).is_err());
    }
    #[test]
    fn automatic_idle_resume_requires_same_epoch_and_cannot_override_operator_intent() {
        let (mut registry, id) = fixture();
        let mut s = read(&registry.connection, &id).unwrap();
        s.state = "paused".into();
        s.origin = Some("scheduler_idle".into());
        s.body.idle = 2;
        write(&registry.connection, &s).unwrap();
        control(&mut registry, &id, "paused").unwrap();
        assert!(!resume_idle(&mut registry, &id, 0).unwrap());
        assert_eq!(
            read(&registry.connection, &id).unwrap().origin.as_deref(),
            Some("operator")
        );
        control(&mut registry, &id, "running").unwrap();
        let mut s = read(&registry.connection, &id).unwrap();
        s.state = "paused".into();
        s.origin = Some("scheduler_idle".into());
        s.body.idle = 2;
        s.epoch += 1;
        write(&registry.connection, &s).unwrap();
        assert!(!resume_idle(&mut registry, &id, s.epoch - 1).unwrap());
        assert!(resume_idle(&mut registry, &id, s.epoch).unwrap());
        assert_eq!(read(&registry.connection, &id).unwrap().state, "running");
    }
    #[test]
    fn rotation_epoch_and_actual_schema_fence_competing_and_stale_actors() {
        use crate::coordinator::{self, Kind};
        let (mut registry, id) = fixture();
        let now = chrono::Utc::now().timestamp_millis();
        let old = coordinator::claim(&mut registry, Kind::CampaignRotation, &id, now, 60000)
            .unwrap()
            .unwrap();
        assert!(
            coordinator::claim(&mut registry, Kind::CampaignRotation, &id, now, 60000)
                .unwrap()
                .is_none()
        );
        control(&mut registry, &id, "paused").unwrap();
        control(&mut registry, &id, "running").unwrap();
        assert!(coordinator::fence(&registry.connection, &old, now).is_err());
        let current = coordinator::claim(&mut registry, Kind::CampaignRotation, &id, now, 60000)
            .unwrap()
            .unwrap();
        coordinator::release(&registry, &old).unwrap();
        coordinator::fence(&registry.connection, &current, now).unwrap();
        initialize(&mut registry.connection, 21).unwrap();
        coordinator::fence(&registry.connection, &current, now).unwrap();
        control(&mut registry, &id, "stopped").unwrap();
        assert!(control(&mut registry, &id, "running").is_err());
        assert!(
            coordinator::claim(&mut registry, Kind::CampaignRotation, &id, now, 60000)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn scheduler_idle_operator_override_and_resigned_fake_process_counts_refuse() {
        let (mut registry, id) = fixture();
        let mut s = read(&registry.connection, &id).unwrap();
        s.state = "paused".into();
        s.origin = Some("scheduler_idle".into());
        s.body.idle = 2;
        write(&registry.connection, &s).unwrap();
        control(&mut registry, &id, "paused").unwrap();
        let held = read(&registry.connection, &id).unwrap();
        assert_eq!(held.origin.as_deref(), Some("operator"));
        assert_eq!(held.epoch, 1);
        control(&mut registry, &id, "running").unwrap();
        assert_eq!(read(&registry.connection, &id).unwrap().body.idle, 0);
        let mut s = read(&registry.connection, &id).unwrap();
        s.body.visits = 1;
        s.body.cursor = 1;
        s.body.members[0] = Counts {
            visits: 1,
            progress: 0,
            children: 1,
            unknown_outcomes: 0,
            last_kind: "held".into(),
            last_outcome_hash: Some("c".repeat(64)),
        };
        write(&registry.connection, &s).unwrap();
        registry
            .connection
            .execute(
                "UPDATE corpus_campaign_rotations SET attempt=1 WHERE id=?1",
                [&id],
            )
            .unwrap();
        assert!(read(&registry.connection, &id).is_err());
    }
}
