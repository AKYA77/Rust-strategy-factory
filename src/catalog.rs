//! Append-only local observations. Catalog registration never grants data approval.
use crate::{
    Result,
    contracts::Profile,
    data::{self, DataClass, Dataset, Manifest},
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};
const APP_ID: i64 = 0x52534341;
const VERSION: &str = "local-dataset-catalog-v1";
const MAX_BODY: usize = 128 * 1024;
const LIMITATIONS: &str = "Local structural/content observation only. The data class is declared metadata, not catalog approval. No provider authenticity, source availability, archive retention, calendar/quality/profile approval, historical completeness or research eligibility is granted.";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    pub dataset_identity: String,
    pub source: String,
    pub archive_hash: String,
    pub content_hash: String,
    pub parser_version: String,
    pub profile: Profile,
    pub declared_class: DataClass,
    pub events: usize,
    pub first_ms: i64,
    pub last_ms: i64,
    pub first_sequence: String,
    pub last_sequence: String,
    pub timestamp_ties: usize,
    pub identical_quote_events: usize,
    pub gaps_over_hour: usize,
}
fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn sequence(value: &str) -> Result<u64> {
    let n = value
        .parse::<u64>()
        .map_err(|_| "invalid catalog sequence")?;
    if n.to_string() != value {
        return Err("noncanonical catalog sequence".into());
    }
    Ok(n)
}
impl Facts {
    fn validate(&self) -> Result<()> {
        self.profile.validate()?;
        if !hex(&self.dataset_identity)
            || !hex(&self.content_hash)
            || self.source.trim().is_empty()
            || self.source.len() > 4096
            || self.archive_hash.len() > 1024
            || self.parser_version.is_empty()
            || self.parser_version.len() > 128
            || serde_json::to_vec(&self.profile)
                .map_err(|e| e.to_string())?
                .len()
                > 65536
            || self.events == 0
            || self.events > 2_000_000
            || self.declared_class == DataClass::Quarantined
        {
            return Err("invalid catalog facts or metadata budget".into());
        }
        let first = sequence(&self.first_sequence)?;
        let last = sequence(&self.last_sequence)?;
        if self.first_ms > self.last_ms
            || chrono::DateTime::from_timestamp_millis(self.first_ms).is_none()
            || chrono::DateTime::from_timestamp_millis(self.last_ms).is_none()
            || first > last
            || (self.events == 1 && (first != last || self.first_ms != self.last_ms))
            || (self.events > 1 && first == last)
            || self.timestamp_ties > self.events - 1
            || self.identical_quote_events > self.events - 1
            || self.gaps_over_hour > self.events - 1
            || self.timestamp_ties + self.gaps_over_hour > self.events - 1
        {
            return Err("invalid catalog observation bounds".into());
        }
        let manifest = Manifest {
            parser_version: self.parser_version.clone(),
            source: self.source.clone(),
            archive_hash: self.archive_hash.clone(),
            content_hash: self.content_hash.clone(),
            profile_hash: data::hash(&self.profile)?,
            symbol: self.profile.symbol.clone(),
            class: self.declared_class,
            events: self.events,
            first_ms: self.first_ms,
            last_ms: self.last_ms,
            timestamp_ties: self.timestamp_ties,
            identical_quote_events: self.identical_quote_events,
            gaps_over_hour: self.gaps_over_hour,
            issues: Vec::new(),
            imported_at: String::new(),
        };
        if data::dataset_identity(&manifest)? != self.dataset_identity {
            return Err("catalog provenance identity mismatch".into());
        }
        Ok(())
    }
    fn observe(d: &Dataset) -> Result<Self> {
        if d.ticks.len() > 2_000_000 {
            return Err("catalog event budget exceeded".into());
        }
        d.verify()?;
        let mut ties = 0;
        let mut repeats = 0;
        let mut gaps = 0;
        for p in d.ticks.windows(2) {
            ties += usize::from(p[0].timestamp_ms == p[1].timestamp_ms);
            repeats += usize::from(p[0].bid == p[1].bid && p[0].ask == p[1].ask);
            gaps += usize::from(
                i128::from(p[1].timestamp_ms) - i128::from(p[0].timestamp_ms) > 3_600_000,
            );
        }
        let m = &d.manifest;
        let value = Self {
            dataset_identity: data::dataset_identity(m)?,
            source: m.source.clone(),
            archive_hash: m.archive_hash.clone(),
            content_hash: m.content_hash.clone(),
            parser_version: m.parser_version.clone(),
            profile: d.profile.clone(),
            declared_class: m.class,
            events: d.ticks.len(),
            first_ms: d.ticks[0].timestamp_ms,
            last_ms: d.ticks[d.ticks.len() - 1].timestamp_ms,
            first_sequence: d.ticks[0].sequence.to_string(),
            last_sequence: d.ticks[d.ticks.len() - 1].sequence.to_string(),
            timestamp_ties: ties,
            identical_quote_events: repeats,
            gaps_over_hour: gaps,
        };
        value.validate()?;
        Ok(value)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Correction {
    pub parent: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: String,
    pub facts: Facts,
    pub recorded_at: String,
    pub limitations: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction: Option<Correction>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub receipt: Receipt,
    pub hash: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Registration {
    pub entry: Entry,
    pub reused: bool,
}
#[derive(Debug, Serialize)]
pub struct Page {
    pub entries: Vec<Entry>,
    pub next_cursor: Option<String>,
    pub limitations: &'static str,
}
pub struct Catalog {
    connection: Connection,
}
fn db(e: rusqlite::Error) -> String {
    e.to_string()
}
fn decode(id: String, body: Vec<u8>, hash: String) -> Result<Entry> {
    if !hex(&id) || !hex(&hash) || body.len() > MAX_BODY {
        return Err("catalog row bounds invalid".into());
    }
    let receipt: Receipt = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    if receipt.limitations != LIMITATIONS
        || receipt.version
            != if receipt.correction.is_some() {
                "local-dataset-catalog-v2"
            } else {
                VERSION
            }
        || receipt.recorded_at.len() > 100
        || chrono::DateTime::parse_from_rfc3339(&receipt.recorded_at).is_err()
        || receipt.facts.dataset_identity != id
        || data::hash(&receipt)? != hash
    {
        return Err("catalog receipt integrity mismatch".into());
    }
    if let Some(link) = &receipt.correction
        && (!hex(&link.parent)
            || link.parent == id
            || link.reason.trim().is_empty()
            || link.reason.len() > 4096)
    {
        return Err("invalid catalog correction".into());
    }
    receipt.facts.validate()?;
    Ok(Entry { receipt, hash })
}
// Managed JSON is stored as BLOB so SQLite can inspect byte length from record
// metadata before returning a bounded payload to Rust.
const PROJECTION: &str = "CASE WHEN length(id)=64 THEN id END,CASE WHEN typeof(body)='blob' AND length(body)<=131072 THEN body END,CASE WHEN length(hash)=64 THEN hash END";
fn get_raw(c: &Connection, id: &str) -> Result<Option<Entry>> {
    c.query_row(
        &format!("SELECT {PROJECTION} FROM catalog_entries WHERE id=?1"),
        [id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, String>(2)?,
            ))
        },
    )
    .optional()
    .map_err(db)?
    .map(|(id, body, hash)| decode(id, body, hash))
    .transpose()
}
fn correction_context(c: &Connection, entry: &Entry) -> Result<()> {
    let mut current = entry.receipt.clone();
    let mut seen = std::collections::HashSet::new();
    seen.insert(current.facts.dataset_identity.clone());
    for _ in 0..64 {
        let Some(link) = &current.correction else {
            return Ok(());
        };
        if !seen.insert(link.parent.clone()) {
            return Err("catalog correction cycle".into());
        }
        let parent = get_raw(c, &link.parent)?.ok_or("correction parent missing")?;
        let a = &parent.receipt.facts;
        let b = &current.facts;
        if a.source != b.source
            || a.profile.symbol != b.profile.symbol
            || a.declared_class != b.declared_class
            || a.first_ms != b.first_ms
            || a.last_ms != b.last_ms
        {
            return Err("correction parent source/instrument/class/window mismatch".into());
        }
        current = parent.receipt;
    }
    Err("catalog correction depth exceeds64".into())
}

fn get(c: &Connection, id: &str) -> Result<Option<Entry>> {
    let entry = get_raw(c, id)?;
    if let Some(entry) = &entry {
        correction_context(c, entry)?;
    }
    Ok(entry)
}
impl Catalog {
    /// Snapshot all managed receipts, including committed WAL state, into a new
    /// standalone catalog. Never publish a partially copied destination.
    pub fn backup(&self, destination: &Path) -> Result<()> {
        if destination.exists() {
            return Err("catalog backup destination must be new".into());
        }
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
        let nonce = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let temp = parent.join(format!(".catalog-backup-{nonce}.sqlite"));
        drop(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|e| e.to_string())?,
        );
        let publication = (|| -> Result<()> {
            let source = self.connection.unchecked_transaction().map_err(db)?;
            let pages: i64 = source
                .query_row("PRAGMA page_count", [], |r| r.get(0))
                .map_err(db)?;
            let page_size: i64 = source
                .query_row("PRAGMA page_size", [], |r| r.get(0))
                .map_err(db)?;
            if pages
                .checked_mul(page_size)
                .ok_or("catalog snapshot size overflow")?
                > 512 * 1024 * 1024
            {
                return Err("catalog snapshot physical budget exceeded".into());
            }
            let integrity: String = source
                .query_row("PRAGMA integrity_check", [], |r| r.get(0))
                .map_err(db)?;
            if integrity != "ok" {
                return Err("source catalog SQLite integrity mismatch".into());
            }
            let tables: i64 = source
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                    [],
                    |r| r.get(0),
                )
                .map_err(db)?;
            let columns:i64=source.query_row("SELECT COUNT(*) FROM pragma_table_info('catalog_entries') WHERE name IN ('id','body','hash')",[],|r|r.get(0)).map_err(db)?;
            let all_columns: i64 = source
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('catalog_entries')",
                    [],
                    |r| r.get(0),
                )
                .map_err(db)?;
            let (count, bytes): (i64, i64) = source
                .query_row(
                    "SELECT COUNT(*),COALESCE(SUM(length(body)),0) FROM catalog_entries",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(db)?;
            if tables != 1
                || columns != 3
                || all_columns != 3
                || count > 10000
                || bytes > 128 * 1024 * 1024
            {
                return Err("catalog snapshot schema or quota unsupported".into());
            }
            let mut dest = Self::open(&temp)?;
            {
                let write = dest
                    .connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(db)?;
                let mut copied = 0i64;
                {
                    let mut q = source
                        .prepare(&format!(
                            "SELECT {PROJECTION} FROM catalog_entries ORDER BY id"
                        ))
                        .map_err(db)?;
                    let rows = q
                        .query_map([], |r| {
                            Ok((
                                r.get::<_, String>(0)?,
                                r.get::<_, Vec<u8>>(1)?,
                                r.get::<_, String>(2)?,
                            ))
                        })
                        .map_err(db)?;
                    for row in rows {
                        let (id, body, hash) = row.map_err(db)?;
                        let entry = decode(id.clone(), body.clone(), hash.clone())?;
                        correction_context(&source, &entry)?;
                        write
                            .execute(
                                "INSERT INTO catalog_entries VALUES(?1,?2,?3)",
                                params![id, body, hash],
                            )
                            .map_err(db)?;
                        copied += 1;
                    }
                }
                if copied != count {
                    return Err("catalog snapshot row count mismatch".into());
                }
                write.commit().map_err(db)?;
            }
            source.commit().map_err(db)?;
            let integrity: String = dest
                .connection
                .query_row("PRAGMA quick_check", [], |r| r.get(0))
                .map_err(db)?;
            if integrity != "ok" {
                return Err("catalog snapshot integrity mismatch".into());
            }
            let (busy, _, _): (i64, i64, i64) = dest
                .connection
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .map_err(db)?;
            if busy != 0 {
                return Err("catalog snapshot checkpoint busy".into());
            }
            let mode: String = dest
                .connection
                .query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))
                .map_err(db)?;
            if mode != "delete" {
                return Err("catalog snapshot is not standalone".into());
            }
            drop(dest);
            File::open(&temp)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())?;
            fs::hard_link(&temp, destination).map_err(|e| e.to_string())?;
            File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())
        })();
        let cleanup = fs::remove_file(&temp).map_err(|e| e.to_string());
        // Only remove this operation's private SQLite sidecars, never other files.
        for suffix in ["-wal", "-shm"] {
            let sidecar = parent.join(format!(".catalog-backup-{nonce}.sqlite{suffix}"));
            if sidecar.exists() {
                fs::remove_file(sidecar).map_err(|e| e.to_string())?;
            }
        }
        publication.and(cleanup)
    }
    pub fn open(path: &Path) -> Result<Self> {
        Self::connect(path, true)
    }
    pub fn existing(path: &Path) -> Result<Self> {
        Self::connect(path, false)
    }
    fn connect(path: &Path, create: bool) -> Result<Self> {
        let flags = if create {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        };
        let c = Connection::open_with_flags(path, flags).map_err(db)?;
        let id: i64 = c
            .query_row("PRAGMA application_id", [], |r| r.get(0))
            .map_err(db)?;
        if id == 0 && create {
            let tables: i64 = c
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                    [],
                    |r| r.get(0),
                )
                .map_err(db)?;
            if tables != 0 {
                return Err("refusing unrelated catalog database".into());
            }
            c.execute_batch(&format!("BEGIN IMMEDIATE;PRAGMA application_id={APP_ID};CREATE TABLE catalog_entries(id TEXT PRIMARY KEY CHECK(length(id)=64),body BLOB NOT NULL CHECK(typeof(body)='blob' AND length(body)<=131072),hash TEXT NOT NULL CHECK(length(hash)=64));CREATE TRIGGER catalog_no_update BEFORE UPDATE ON catalog_entries BEGIN SELECT RAISE(ABORT,'catalog entries immutable');END;CREATE TRIGGER catalog_no_delete BEFORE DELETE ON catalog_entries BEGIN SELECT RAISE(ABORT,'catalog entries immutable');END;COMMIT;")).map_err(db)?;
        } else if id != APP_ID {
            return Err("refusing unrelated catalog database".into());
        }
        if create {
            c.execute_batch("PRAGMA journal_mode=WAL;PRAGMA synchronous=FULL;")
                .map_err(db)?;
        }
        Ok(Self { connection: c })
    }
    pub fn register(&mut self, d: &Dataset) -> Result<Registration> {
        self.register_linked(d, None)
    }
    pub fn register_correction(
        &mut self,
        d: &Dataset,
        parent: &str,
        reason: &str,
    ) -> Result<Registration> {
        if !hex(parent) || reason.trim().is_empty() || reason.len() > 4096 {
            return Err("correction parent and bounded reason required".into());
        }
        self.register_linked(
            d,
            Some(Correction {
                parent: parent.into(),
                reason: reason.into(),
            }),
        )
    }
    fn register_linked(
        &mut self,
        d: &Dataset,
        correction: Option<Correction>,
    ) -> Result<Registration> {
        let facts = Facts::observe(d)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        if let Some(entry) = get(&tx, &facts.dataset_identity)? {
            if entry.receipt.facts != facts || entry.receipt.correction != correction {
                return Err("catalog identity already has conflicting facts".into());
            }
            tx.commit().map_err(db)?;
            return Ok(Registration {
                entry,
                reused: true,
            });
        }
        let receipt = Receipt {
            version: if correction.is_some() {
                "local-dataset-catalog-v2"
            } else {
                VERSION
            }
            .into(),
            facts,
            recorded_at: chrono::Utc::now().to_rfc3339(),
            limitations: LIMITATIONS.into(),
            correction,
        };
        let provisional = Entry {
            hash: data::hash(&receipt)?,
            receipt: receipt.clone(),
        };
        if provisional
            .receipt
            .correction
            .as_ref()
            .is_some_and(|x| x.parent == provisional.receipt.facts.dataset_identity)
        {
            return Err("self correction forbidden".into());
        }
        correction_context(&tx, &provisional)?;
        let body = serde_json::to_vec(&receipt).map_err(|e| e.to_string())?;
        if body.len() > MAX_BODY {
            return Err("catalog metadata budget exceeded".into());
        }
        let (count, bytes): (i64, i64) = tx
            .query_row(
                "SELECT COUNT(*),COALESCE(SUM(length(body)),0) FROM catalog_entries",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db)?;
        if count >= 10000
            || bytes
                .checked_add(body.len() as i64)
                .ok_or("catalog bytes overflow")?
                > 128 * 1024 * 1024
        {
            return Err("catalog logical quota exceeded".into());
        }
        let hash = data::hash(&receipt)?;
        tx.execute(
            "INSERT INTO catalog_entries VALUES(?1,?2,?3)",
            params![receipt.facts.dataset_identity, body, hash],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(Registration {
            entry: Entry { receipt, hash },
            reused: false,
        })
    }
    pub fn entry(&self, id: &str) -> Result<Option<Entry>> {
        if !hex(id) {
            return Err("invalid catalog identity".into());
        }
        get(&self.connection, id)
    }
    pub fn page(&self, after: Option<&str>, limit: usize) -> Result<Page> {
        if limit == 0 || limit > 100 || after.is_some_and(|s| !hex(s)) {
            return Err("invalid catalog cursor/limit".into());
        }
        let mut q = self
            .connection
            .prepare(&format!(
                "SELECT {PROJECTION} FROM catalog_entries WHERE id>?1 ORDER BY id LIMIT ?2"
            ))
            .map_err(db)?;
        let rows = q
            .query_map(params![after.unwrap_or(""), (limit + 1) as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(db)?;
        let mut entries = Vec::new();
        for row in rows {
            let (id, body, hash) = row.map_err(db)?;
            let entry = decode(id, body, hash)?;
            correction_context(&self.connection, &entry)?;
            entries.push(entry);
        }
        let more = entries.len() > limit;
        entries.truncate(limit);
        let next_cursor = if more {
            entries
                .last()
                .map(|e| e.receipt.facts.dataset_identity.clone())
        } else {
            None
        };
        Ok(Page {
            entries,
            next_cursor,
            limitations: "Immutable local observations, not provider authenticity, acquisition availability, archive retention, calendar/quality/profile approval, complete history or research eligibility. Distinct provenance identities remain separate; cursor orders identities, not event time. Logical quota excludes database/WAL/filesystem overhead.",
        })
    }
}
