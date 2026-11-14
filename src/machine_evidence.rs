//! Recompute mechanical evidence rather than trusting submitted report hashes.
use crate::{
    Result, archive_evidence,
    calendar_evidence::{self, Declaration},
    catalog::Entry,
    data::Dataset,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub catalog_hash: String,
    pub archive_report_hash: String,
    pub calendar_report_hash: String,
}
#[derive(Debug, Serialize)]
pub struct Verified {
    pub catalog_hash: String,
    pub archive_report_hash: String,
    pub calendar_report_hash: String,
    pub authority: &'static str,
}
pub fn revalidate(
    archive: &Path,
    dataset: &Dataset,
    entry: &Entry,
    calendar: Declaration,
    claim: &Claim,
) -> Result<Verified> {
    if claim.catalog_hash != entry.hash {
        return Err("machine evidence receipt mismatch".into());
    }
    let raw = archive_evidence::assess(archive, dataset, entry)?;
    let schedule = calendar_evidence::assess(dataset, entry, calendar)?;
    if raw.hash != claim.archive_report_hash || schedule.hash != claim.calendar_report_hash {
        return Err("machine evidence differs from recomputed source/calendar facts".into());
    }
    Ok(Verified {
        catalog_hash: entry.hash.clone(),
        archive_report_hash: raw.hash,
        calendar_report_hash: schedule.hash,
        authority: "Mechanical consistency revalidated only; calendar assumptions and human review authority remain unapproved. No research eligibility grant.",
    })
}

// Stored admission is a past observation. Reading it never asserts that a file
// is still available or that a provider/calendar has independent approval.
use crate::{
    catalog::{self, Catalog},
    data,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{fs::File, io::Read};
const VERSION: &str = "persisted-machine-consistency-v1";
const MAX_BODY: usize = 16 * 1024 * 1024;
const LIMIT: &str = "Past mechanical consistency only; no authenticity, license, calendar/profile/quality/coverage approval or scientific eligibility. Revalidate exact artifacts before use. Revisions require separate admission and reviews. Synthetic data remains synthetic.";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub version: String,
    pub catalog_identity: String,
    pub catalog_hash: String,
    pub archive_hash: String,
    pub archive_bytes: u64,
    pub content_hash: String,
    pub profile_hash: String,
    pub archive_report_hash: String,
    pub calendar_report_hash: String,
    pub calendar: Declaration,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub binding: Binding,
    pub verified_at: String,
    pub limitations: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub id: String,
    pub record: Record,
    pub hash: String,
}
pub(crate) fn hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(crate) fn exists(c: &Connection) -> Result<bool> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='catalog_machine')",
        [],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}
pub(crate) fn initialize(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS catalog_machine(id TEXT PRIMARY KEY CHECK(length(id)=64),body BLOB NOT NULL CHECK(typeof(body)='blob' AND length(body)<=16777216),hash TEXT NOT NULL CHECK(length(hash)=64));CREATE TRIGGER IF NOT EXISTS machine_no_update BEFORE UPDATE ON catalog_machine BEGIN SELECT RAISE(ABORT,'machine evidence immutable');END;CREATE TRIGGER IF NOT EXISTS machine_no_delete BEFORE DELETE ON catalog_machine BEGIN SELECT RAISE(ABORT,'machine evidence immutable');END;").map_err(|e| e.to_string())
}
fn validate(c: &Connection, admission: &Admission) -> Result<()> {
    let r = &admission.record;
    let b = &r.binding;
    if b.version != VERSION
        || r.limitations != LIMIT
        || !hex(&admission.id)
        || !hex(&admission.hash)
        || [
            &b.catalog_identity,
            &b.catalog_hash,
            &b.archive_hash,
            &b.content_hash,
            &b.profile_hash,
            &b.archive_report_hash,
            &b.calendar_report_hash,
        ]
        .iter()
        .any(|s| !hex(s))
        || b.archive_bytes == 0
        || b.archive_bytes > 64 * 1024 * 1024
        || chrono::DateTime::parse_from_rfc3339(&r.verified_at).is_err()
        || r.verified_at.len() > 100
        || data::hash(b)? != admission.id
        || data::hash(r)? != admission.hash
    {
        return Err("machine admission integrity mismatch".into());
    }
    // Validate calendar shape on read as well as on recomputation.
    b.calendar.validate()?;
    let entry = catalog::get(c, &b.catalog_identity)?.ok_or("machine catalog receipt missing")?;
    let f = &entry.receipt.facts;
    if entry.hash != b.catalog_hash
        || f.archive_hash != b.archive_hash
        || f.content_hash != b.content_hash
        || data::hash(&f.profile)? != b.profile_hash
    {
        return Err("machine admission artifact context mismatch".into());
    }
    Ok(())
}
pub(crate) fn read(c: &Connection, id: &str) -> Result<Option<Admission>> {
    if !hex(id) {
        return Err("invalid machine evidence identity".into());
    }
    if !exists(c)? {
        return Ok(None);
    }
    let row = c.query_row("SELECT CASE WHEN typeof(body)='blob' AND length(body)<=16777216 THEN body END,hash FROM catalog_machine WHERE id=?1", [id], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))).optional().map_err(|e| e.to_string())?;
    row.map(|(body, hash)| {
        let record = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        let a = Admission {
            id: id.into(),
            record,
            hash,
        };
        validate(c, &a)?;
        Ok(a)
    })
    .transpose()
}
pub(crate) fn copy(source: &Connection, dest: &Connection) -> Result<()> {
    if !exists(source)? {
        return Ok(());
    }
    let columns: i64 = source
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('catalog_machine')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let (n, b): (i64, i64) = source
        .query_row(
            "SELECT COUNT(*),COALESCE(SUM(length(body)),0) FROM catalog_machine",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    if columns != 3 || n > 10000 || b > 128 * 1024 * 1024 {
        return Err("machine snapshot schema/quota unsupported".into());
    }
    let mut q = source
        .prepare("SELECT id FROM catalog_machine ORDER BY id")
        .map_err(|e| e.to_string())?;
    for id in q
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
    {
        let a =
            read(source, &id.map_err(|e| e.to_string())?)?.ok_or("machine admission missing")?;
        dest.execute(
            "INSERT INTO catalog_machine VALUES(?1,?2,?3)",
            params![
                a.id,
                serde_json::to_vec(&a.record).map_err(|e| e.to_string())?,
                a.hash
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn archive_size(path: &Path, expected: &str) -> Result<u64> {
    let mut f = File::open(path)
        .map_err(|e| e.to_string())?
        .take(64 * 1024 * 1024 + 1);
    let mut hash = blake3::Hasher::new();
    let mut bytes = 0;
    let mut buffer = [0; 65536];
    loop {
        let n = f.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        hash.update(&buffer[..n]);
    }
    if bytes > 64 * 1024 * 1024 || hash.finalize().to_hex().as_str() != expected {
        return Err("machine archive changed or exceeds budget".into());
    }
    Ok(bytes)
}
impl Catalog {
    pub fn admit_machine(
        &mut self,
        archive: &Path,
        dataset: &Dataset,
        identity: &str,
        calendar: Declaration,
        claim: &Claim,
    ) -> Result<Admission> {
        let entry = self.entry(identity)?.ok_or("catalog observation missing")?;
        revalidate(archive, dataset, &entry, calendar.clone(), claim)?;
        let b = Binding {
            version: VERSION.into(),
            catalog_identity: identity.into(),
            catalog_hash: entry.hash,
            archive_hash: dataset.manifest.archive_hash.clone(),
            archive_bytes: archive_size(archive, &dataset.manifest.archive_hash)?,
            content_hash: dataset.manifest.content_hash.clone(),
            profile_hash: dataset.manifest.profile_hash.clone(),
            archive_report_hash: claim.archive_report_hash.clone(),
            calendar_report_hash: claim.calendar_report_hash.clone(),
            calendar,
        };
        let id = data::hash(&b)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if let Some(old) = read(&tx, &id)? {
            tx.commit().map_err(|e| e.to_string())?;
            return Ok(old);
        }
        let record = Record {
            binding: b,
            verified_at: chrono::Utc::now().to_rfc3339(),
            limitations: LIMIT.into(),
        };
        let admission = Admission {
            id,
            hash: data::hash(&record)?,
            record,
        };
        validate(&tx, &admission)?;
        let body = serde_json::to_vec(&admission.record).map_err(|e| e.to_string())?;
        let (n, b): (i64, i64) = tx
            .query_row(
                "SELECT COUNT(*),COALESCE(SUM(length(body)),0) FROM catalog_machine",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        if n >= 10000 || body.len() > MAX_BODY || b + body.len() as i64 > 128 * 1024 * 1024 {
            return Err("machine evidence quota exceeded".into());
        }
        tx.execute(
            "INSERT INTO catalog_machine VALUES(?1,?2,?3)",
            params![admission.id, body, admission.hash],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(admission)
    }
    pub fn machine(&self, id: &str) -> Result<Option<Admission>> {
        read(&self.connection, id)
    }
    pub fn revalidate_machine(
        &self,
        id: &str,
        archive: &Path,
        dataset: &Dataset,
    ) -> Result<Verified> {
        let a = self.machine(id)?.ok_or("machine admission missing")?;
        let b = a.record.binding;
        archive_size(archive, &b.archive_hash)?;
        revalidate(
            archive,
            dataset,
            &self
                .entry(&b.catalog_identity)?
                .ok_or("catalog observation missing")?,
            b.calendar,
            &Claim {
                catalog_hash: b.catalog_hash,
                archive_report_hash: b.archive_report_hash,
                calendar_report_hash: b.calendar_report_hash,
            },
        )
    }
}
