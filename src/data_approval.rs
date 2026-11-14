//! Authenticated independent review decisions, distinct from supplied assertions.
use crate::{
    Result,
    catalog::{self, Catalog},
    data::{self, DataClass, Dataset},
    data_review::Area,
    machine_evidence::{self, hex},
    review_authority::{Authority, Principal},
};
use rusqlite::{Connection, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Approve,
    Reject,
    Revoke,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub machine_id: String,
    pub expected_head: Option<String>,
    pub area: Area,
    pub decision: Decision,
    pub explanation: String,
    pub evidence_text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub version: String,
    pub sequence: i64,
    pub catalog_identity: String,
    pub catalog_hash: String,
    pub machine_hash: String,
    pub authority_id: String,
    pub reviewer: String,
    pub grant_hash: String,
    pub recorded_ms: i64,
    pub request: Request,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub record: Record,
    pub hash: String,
    pub authenticator: String,
}
#[derive(Debug, Serialize)]
pub struct ReviewState {
    pub catalog_identity: String,
    pub head: Option<String>,
    pub events: Vec<Event>,
    pub current_authority_authenticated: bool,
    pub limitations: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Eligibility {
    pub machine_id: String,
    pub catalog_identity: String,
    pub scope: crate::validation::Interval,
    pub state: &'static str,
    pub reasons: Vec<String>,
    pub review_head: Option<String>,
    pub live_machine_revalidated: bool,
    pub limitations: &'static str,
}
const LIMIT: &str = "Independent local data-review decisions only. Operator-provisioned credentials and filesystem are the trust root; human source/profile/calendar/quality/coverage judgments are not mathematical proof. No strategy qualification, complete annual history, statistical independence or protected validation is granted. Synthetic data never becomes eligible. Database/file rollback by the trust-root operator is outside this boundary.";
pub(crate) fn exists(c: &Connection) -> Result<bool> {
    c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='catalog_decisions')",[],|r|r.get(0)).map_err(|e|e.to_string())
}
pub(crate) fn initialize(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS catalog_decisions(sequence INTEGER PRIMARY KEY,id TEXT NOT NULL UNIQUE CHECK(length(id)=64),body BLOB NOT NULL CHECK(typeof(body)='blob' AND length(body)<=131072),authenticator TEXT NOT NULL CHECK(length(authenticator)=64));CREATE TRIGGER IF NOT EXISTS decisions_no_update BEFORE UPDATE ON catalog_decisions BEGIN SELECT RAISE(ABORT,'review decisions immutable');END;CREATE TRIGGER IF NOT EXISTS decisions_no_delete BEFORE DELETE ON catalog_decisions BEGIN SELECT RAISE(ABORT,'review decisions immutable');END;CREATE INDEX IF NOT EXISTS decisions_scope ON catalog_decisions(json_extract(CAST(body AS TEXT),'$.catalog_identity'),sequence);").map_err(|e|e.to_string())
}
fn validate_request(r: &Request) -> Result<()> {
    if !hex(&r.machine_id)
        || r.expected_head.as_ref().is_some_and(|h| !hex(h))
        || r.explanation.trim().is_empty()
        || r.explanation.len() > 4096
        || r.evidence_text.trim().is_empty()
        || r.evidence_text.len() > 65536
    {
        return Err(
            "review requires exact machine/head context and bounded retained evidence".into(),
        );
    }
    Ok(())
}
fn decode(
    c: &Connection,
    sequence: i64,
    id: String,
    body: Vec<u8>,
    authenticator: String,
) -> Result<Event> {
    let record: Record = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    validate_request(&record.request)?;
    if record.version != "independent-data-decision-v1"
        || record.sequence != sequence
        || sequence <= 0
        || !hex(&id)
        || !hex(&authenticator)
        || !hex(&record.authority_id)
        || !hex(&record.grant_hash)
        || record.reviewer.trim().is_empty()
        || record.reviewer.len() > 128
        || chrono::DateTime::from_timestamp_millis(record.recorded_ms).is_none()
        || data::hash(&record)? != id
    {
        return Err("review decision integrity mismatch".into());
    }
    let a = machine_evidence::read(c, &record.request.machine_id)?
        .ok_or("decision machine evidence missing")?;
    if a.hash != record.machine_hash
        || a.record.binding.catalog_identity != record.catalog_identity
        || a.record.binding.catalog_hash != record.catalog_hash
    {
        return Err("review decision artifact binding mismatch".into());
    }
    Ok(Event {
        record,
        hash: id,
        authenticator,
    })
}
fn history(c: &Connection, identity: &str) -> Result<Vec<Event>> {
    if !hex(identity) {
        return Err("invalid review scope".into());
    }
    catalog::get(c, identity)?.ok_or("review catalog observation missing")?;
    if !exists(c)? {
        return Ok(Vec::new());
    }
    let mut q=c.prepare("SELECT sequence,id,CASE WHEN typeof(body)='blob' AND length(body)<=131072 THEN body END,authenticator FROM catalog_decisions WHERE json_extract(CAST(body AS TEXT),'$.catalog_identity')=?1 ORDER BY sequence LIMIT 1001").map_err(|e|e.to_string())?;
    let mut events = Vec::new();
    let mut previous = None;
    let mut last_ms = None;
    for row in q
        .query_map([identity], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .map_err(|e| e.to_string())?
    {
        let (sequence, id, body, tag) = row.map_err(|e| e.to_string())?;
        let e = decode(c, sequence, id, body, tag)?;
        if e.record.catalog_identity != identity
            || e.record.request.expected_head != previous
            || last_ms.is_some_and(|at| e.record.recorded_ms < at)
        {
            return Err("review history chain mismatch".into());
        }
        previous = Some(e.hash.clone());
        last_ms = Some(e.record.recorded_ms);
        events.push(e);
        if events.len() > 1000 {
            return Err("review scope exceeds1000 decisions".into());
        }
    }
    Ok(events)
}
fn authentic(a: &Authority, e: &Event, f: &catalog::Facts, now: i64) -> Result<bool> {
    if now < e.record.recorded_ms
        || e.record.authority_id != a.identity()
        || !a.authentic(&e.record, &e.authenticator)?
    {
        return Ok(false);
    }
    let past = a.permission(
        &e.record.reviewer,
        &f.profile.symbol,
        &f.source,
        e.record.request.area,
        e.record.recorded_ms,
    );
    let current = a.permission(
        &e.record.reviewer,
        &f.profile.symbol,
        &f.source,
        e.record.request.area,
        now,
    );
    Ok(
        past.is_ok_and(|g| data::hash(g).is_ok_and(|h| h == e.record.grant_hash))
            && current.is_ok(),
    )
}
pub(crate) fn copy(source: &Connection, dest: &Connection) -> Result<()> {
    if !exists(source)? {
        return Ok(());
    }
    let cols: i64 = source
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('catalog_decisions')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let (n, b): (i64, i64) = source
        .query_row(
            "SELECT COUNT(*),COALESCE(SUM(length(body)),0) FROM catalog_decisions",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    if cols != 4 || n > 10000 || b > 128 * 1024 * 1024 {
        return Err("decision snapshot schema/quota unsupported".into());
    }
    let mut q=source.prepare("SELECT DISTINCT json_extract(CAST(body AS TEXT),'$.catalog_identity') FROM catalog_decisions").map_err(|e|e.to_string())?;
    let mut copied = 0;
    for identity in q
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
    {
        for e in history(source, &identity.map_err(|e| e.to_string())?)? {
            dest.execute(
                "INSERT INTO catalog_decisions VALUES(?1,?2,?3,?4)",
                params![
                    e.record.sequence,
                    e.hash,
                    serde_json::to_vec(&e.record).map_err(|e| e.to_string())?,
                    e.authenticator
                ],
            )
            .map_err(|e| e.to_string())?;
            copied += 1;
        }
    }
    if copied != n {
        return Err("decision snapshot count mismatch".into());
    }
    Ok(())
}
impl Catalog {
    pub fn review_state(&self, identity: &str, authority: &Authority) -> Result<ReviewState> {
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        let entry = catalog::get(&tx, identity)?.ok_or("review catalog observation missing")?;
        let events = history(&tx, identity)?;
        let now = chrono::Utc::now().timestamp_millis();
        let authenticated = events.iter().try_fold(true, |v, e| {
            Ok::<_, String>(v && authentic(authority, e, &entry.receipt.facts, now)?)
        })?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(ReviewState {
            catalog_identity: identity.into(),
            head: events.last().map(|e| e.hash.clone()),
            events,
            current_authority_authenticated: authenticated,
            limitations: LIMIT,
        })
    }
    pub fn decide(
        &mut self,
        authority: &Authority,
        principal: &Principal,
        request: Request,
    ) -> Result<Event> {
        validate_request(&request)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let a =
            machine_evidence::read(&tx, &request.machine_id)?.ok_or("machine admission missing")?;
        let b = &a.record.binding;
        let entry = catalog::get(&tx, &b.catalog_identity)?.ok_or("catalog observation missing")?;
        let f = &entry.receipt.facts;
        let now = chrono::Utc::now().timestamp_millis();
        let grant = authority.grant(principal, &f.profile.symbol, &f.source, request.area, now)?;
        if request.decision == Decision::Approve
            && (f.declared_class == DataClass::SyntheticTestOnly
                || f.declared_class == DataClass::Quarantined
                || f.profile.research_only)
        {
            return Err(
                "synthetic or research-only profiles cannot receive scientific approval".into(),
            );
        }
        let events = history(&tx, &b.catalog_identity)?;
        if events.len() >= 1000 {
            return Err("review scope decision quota exceeded".into());
        }
        if events.last().map(|e| e.hash.clone()) != request.expected_head {
            return Err("review head changed; inspect latest evidence before deciding".into());
        }
        if events.last().is_some_and(|e| now < e.record.recorded_ms) {
            return Err("review clock moved backwards".into());
        }
        let (n,bytes,last):(i64,i64,i64)=tx.query_row("SELECT COUNT(*),COALESCE(SUM(length(body)),0),COALESCE(MAX(sequence),0) FROM catalog_decisions",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
        let record = Record {
            version: "independent-data-decision-v1".into(),
            sequence: last.checked_add(1).ok_or("decision sequence overflow")?,
            catalog_identity: b.catalog_identity.clone(),
            catalog_hash: b.catalog_hash.clone(),
            machine_hash: a.hash,
            authority_id: authority.identity().into(),
            reviewer: principal.name().into(),
            grant_hash: data::hash(grant)?,
            recorded_ms: now,
            request,
        };
        let body = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
        if n >= 10000 || body.len() > 131072 || bytes + body.len() as i64 > 128 * 1024 * 1024 {
            return Err("review decision journal quota exceeded".into());
        }
        let e = Event {
            hash: data::hash(&record)?,
            authenticator: authority.sign(&record)?,
            record,
        };
        tx.execute(
            "INSERT INTO catalog_decisions VALUES(?1,?2,?3,?4)",
            params![e.record.sequence, e.hash, body, e.authenticator],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(e)
    }
    /// Live gate for exactly the admitted calendar scope. Never qualifies a strategy.
    pub fn data_eligibility(
        &self,
        authority: &Authority,
        machine_id: &str,
        archive: &Path,
        dataset: &Dataset,
    ) -> Result<Eligibility> {
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        let a = machine_evidence::read(&tx, machine_id)?.ok_or("machine admission missing")?;
        let b = &a.record.binding;
        let entry = catalog::get(&tx, &b.catalog_identity)?.ok_or("catalog observation missing")?;
        let f = &entry.receipt.facts;
        let events = history(&tx, &b.catalog_identity)?;
        let now = chrono::Utc::now().timestamp_millis();
        let mut reasons = Vec::new();
        if f.declared_class == DataClass::SyntheticTestOnly
            || f.declared_class == DataClass::Quarantined
        {
            reasons.push("Synthetic/quarantined data cannot become scientifically eligible".into());
        }
        if f.profile.research_only {
            reasons.push("Instrument profile is research-only".into());
        }
        let live = self
            .revalidate_machine(machine_id, archive, dataset)
            .is_ok();
        if !live {
            reasons.push("Exact archive/dataset/calendar machine revalidation failed".into());
        }
        let mut q=tx.prepare("SELECT id FROM catalog_entries WHERE json_extract(CAST(body AS TEXT),'$.correction.parent')=?1 LIMIT 1").map_err(|e|e.to_string())?;
        let children = q
            .query_map([&b.catalog_identity], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        for child in children {
            catalog::get(&tx, &child.map_err(|e| e.to_string())?)?.ok_or("correction missing")?;
            reasons.push(
                "Catalog receipt superseded by explicit source correction; review the new revision"
                    .into(),
            );
        }
        for area in [
            Area::Source,
            Area::Profile,
            Area::Calendar,
            Area::Quality,
            Area::Coverage,
        ] {
            let latest = events.iter().rev().find(|e| e.record.request.area == area);
            let approved = if let Some(e) = latest {
                e.record.request.decision == Decision::Approve
                    && e.record.request.machine_id == machine_id
                    && e.record.machine_hash == a.hash
                    && authentic(authority, e, f, now)?
            } else {
                false
            };
            if !approved {
                reasons.push(format!("{area:?}: current independent exact-artifact approval absent, rejected, revoked or expired"));
            }
        }
        if live {
            let calendar = crate::calendar_evidence::assess(dataset, &entry, b.calendar.clone())?;
            if calendar.evidence.observations_outside_open > 0
                || calendar.evidence.unobserved_open_intervals > 0
                || calendar.evidence.observed.events == 0
            {
                reasons.push(
                    "Declared-calendar anomalies or empty scope require revised evidence".into(),
                );
            }
        }
        drop(q);
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Eligibility {
            machine_id: machine_id.into(),
            catalog_identity: b.catalog_identity.clone(),
            scope: b.calendar.scope.clone(),
            state: if reasons.is_empty() {
                "DATA_APPROVED_FOR_SCOPE"
            } else {
                "DATA_BLOCKED"
            },
            reasons,
            review_head: events.last().map(|e| e.hash.clone()),
            live_machine_revalidated: live,
            limitations: LIMIT,
        })
    }
}
