//! Checkpointed local normalized CSV imports. No acquisition or scientific approval.
use crate::{
    Result,
    contracts::Profile,
    data::{self, Tick},
    native,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};

const SOURCE_CAP: u64 = 64 * 1024 * 1024;
const OUTPUT_CAP: u64 = 128 * 1024 * 1024;
const APP_ID: i64 = 0x5253494d;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub parser: String,
    pub archive_hash: String,
    pub archive_bytes: u64,
    pub profile: Profile,
    pub source: String,
    pub synthetic: bool,
    pub chunk_events: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chunk {
    pub file: String,
    pub dataset_hash: String,
    pub events: usize,
    pub next_byte: u64,
    pub bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub configuration: Configuration,
    pub started_at: String,
    pub next_byte: u64,
    pub events: usize,
    pub complete: bool,
    pub chunks: Vec<Chunk>,
    pub limitations: String,
}
fn db(e: rusqlite::Error) -> String {
    e.to_string()
}
fn open(root: &Path) -> Result<Connection> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let c = Connection::open(root.join("normalized-import.sqlite")).map_err(db)?;
    let id: i64 = c
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .map_err(db)?;
    if id == 0 {
        let tables: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .map_err(db)?;
        if tables != 0 {
            return Err("refusing unrelated import ledger".into());
        }
        c.execute_batch(&format!("BEGIN IMMEDIATE;PRAGMA application_id={APP_ID};CREATE TABLE import_state(singleton INTEGER PRIMARY KEY CHECK(singleton=1),body TEXT NOT NULL,hash TEXT NOT NULL);COMMIT;")).map_err(db)?;
    } else if id != APP_ID {
        return Err("refusing unrelated import ledger".into());
    }
    c.execute_batch("PRAGMA journal_mode=WAL;PRAGMA synchronous=FULL;")
        .map_err(db)?;
    Ok(c)
}
fn load(c: &Connection) -> Result<Option<Progress>> {
    let bounds: Option<(i64, i64)> = c
        .query_row(
            "SELECT length(CAST(body AS BLOB)),length(hash) FROM import_state WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db)?;
    if let Some((body, hash)) = bounds
        && (!(1..=2 * 1024 * 1024).contains(&body) || hash != 64)
    {
        return Err("import checkpoint size invalid".into());
    }
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT body,hash FROM import_state WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db)?;
    row.map(|(body, hash)| {
        let value: Progress = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        if data::hash(&value)? != hash {
            return Err("import checkpoint integrity mismatch".into());
        }
        let config = &value.configuration;
        config.profile.validate()?;
        if config.parser != "normalized-csv-chunks-v1"
            || config.archive_bytes > SOURCE_CAP
            || config.chunk_events == 0
            || config.chunk_events > 100_000
            || config.source.trim().is_empty()
            || config.source.len() > 4096
        {
            return Err("import checkpoint configuration invalid".into());
        }
        Ok(value)
    })
    .transpose()
}
fn store(c: &Connection, value: &Progress) -> Result<()> {
    c.execute("INSERT INTO import_state VALUES(1,?1,?2) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body,hash=excluded.hash",params![serde_json::to_string(value).map_err(|e|e.to_string())?,data::hash(value)?]).map_err(db)?;
    Ok(())
}
fn verify_chunks(root: &Path, p: &Progress) -> Result<Option<Tick>> {
    if p.chunks.len() > 4096 || p.events > 2_000_000 || p.next_byte > p.configuration.archive_bytes
    {
        return Err("import checkpoint bounds invalid".into());
    }
    let mut previous: Option<Tick> = None;
    let mut events = 0usize;
    let mut bytes = 0u64;
    let mut offset = 0;
    for (i, chunk) in p.chunks.iter().enumerate() {
        if chunk.file != format!("chunk-{i:06}.rsf")
            || chunk.next_byte <= offset
            || chunk.next_byte > p.configuration.archive_bytes
        {
            return Err("import chunk checkpoint order invalid".into());
        }
        let path = root.join(&chunk.file);
        let d = native::read(&path)?;
        if data::hash(&d)? != chunk.dataset_hash
            || d.ticks.len() != chunk.events
            || fs::metadata(path).map_err(|e| e.to_string())?.len() != chunk.bytes
        {
            return Err("committed import chunk mismatch".into());
        }
        if let Some(prev) = previous {
            let first = d.ticks.first().ok_or("empty committed chunk")?;
            if first.timestamp_ms < prev.timestamp_ms || first.sequence <= prev.sequence {
                return Err("committed import boundary order invalid".into());
            }
        }
        previous = d.ticks.last().copied();
        events += chunk.events;
        bytes = bytes
            .checked_add(chunk.bytes)
            .ok_or("import bytes overflow")?;
        offset = chunk.next_byte;
    }
    if events != p.events || bytes > OUTPUT_CAP || (!p.chunks.is_empty() && offset > p.next_byte) {
        return Err("import totals invalid".into());
    }
    Ok(previous)
}
/// Status verifies every published chunk. Missing/tampered artifacts refuse, never disappear.
pub fn status(root: &Path) -> Result<Progress> {
    if !root.join("normalized-import.sqlite").is_file() {
        return Err("import ledger does not exist".into());
    }
    let mut c = open(root)?;
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    let p = load(&tx)?.ok_or("import not initialized")?;
    verify_chunks(root, &p)?;
    tx.commit().map_err(db)?;
    Ok(p)
}

#[derive(Deserialize)]
struct Row {
    timestamp_ms: i64,
    sequence: u64,
    bid: String,
    ask: String,
}
/// One durable chunk per call. Source is snapshotted within64MiB, hashed each invocation;
/// resume avoids re-parsing/publishing committed rows but is not full-history streaming.
pub fn step(
    input: &Path,
    root: &Path,
    profile: Profile,
    source: &str,
    synthetic: bool,
    chunk_events: usize,
) -> Result<Progress> {
    profile.validate()?;
    if source.trim().is_empty()
        || source.len() > 4096
        || chunk_events == 0
        || chunk_events > 100_000
        || serde_json::to_vec(&profile)
            .map_err(|e| e.to_string())?
            .len()
            > 65536
    {
        return Err("invalid import source/profile/chunk budget".into());
    }
    let mut bytes = Vec::new();
    File::open(input)
        .map_err(|e| e.to_string())?
        .take(SOURCE_CAP + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > SOURCE_CAP {
        return Err("normalized source exceeds64MiB snapshot budget".into());
    }
    let config = Configuration {
        parser: "normalized-csv-chunks-v1".into(),
        archive_hash: blake3::hash(&bytes).to_hex().to_string(),
        archive_bytes: bytes.len() as u64,
        profile,
        source: source.into(),
        synthetic,
        chunk_events,
    };
    let mut reader = csv::Reader::from_reader(std::io::Cursor::new(bytes.as_slice()));
    if reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .collect::<Vec<_>>()
        != ["timestamp_ms", "sequence", "bid", "ask"]
    {
        return Err("expected CSV header timestamp_ms,sequence,bid,ask".into());
    }
    let header_end = reader.position().byte();
    let mut c = open(root)?;
    {
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        if let Some(p) = load(&tx)? {
            if p.configuration != config {
                return Err(
                    "source revision or import configuration changed; use a new import directory"
                        .into(),
                );
            }
        } else {
            store(&tx,&Progress{configuration:config.clone(),started_at:chrono::Utc::now().to_rfc3339(),next_byte:header_end,events:0,complete:false,chunks:Vec::new(),limitations:"Bounded local normalized chunks only. Complete means this exact source snapshot was parsed; not historical completeness, calendar/quality/profile approval or research eligibility. Failed rows never advance the checkpoint. No acquisition or full-history streaming.".into()})?;
        }
        tx.commit().map_err(db)?;
        File::open(root)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
    }
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    let mut p = load(&tx)?.ok_or("import checkpoint missing")?;
    let previous = verify_chunks(root, &p)?;
    if p.complete {
        tx.commit().map_err(db)?;
        return Ok(p);
    }
    if p.next_byte < header_end {
        return Err("invalid CSV checkpoint offset".into());
    }
    let mut pos = csv::Position::new();
    pos.set_byte(p.next_byte);
    pos.set_record(p.events as u64 + 1);
    reader.seek(pos).map_err(|e| e.to_string())?;
    let mut ticks = Vec::new();
    let mut eof = false;
    for _ in 0..chunk_events {
        let mut record = csv::StringRecord::new();
        if !reader.read_record(&mut record).map_err(|e| e.to_string())? {
            eof = true;
            break;
        }
        let row: Row = record
            .deserialize(None)
            .map_err(|e| format!("source event {}: {e}", p.events + ticks.len()))?;
        ticks.push(Tick {
            timestamp_ms: row.timestamp_ms,
            sequence: row.sequence,
            bid: config.profile.price(&row.bid)?,
            ask: config.profile.price(&row.ask)?,
        });
    }
    if p.events + ticks.len() > 2_000_000 {
        return Err("import event budget exceeded".into());
    }
    if ticks.is_empty() && p.events == 0 {
        return Err("empty normalized source".into());
    }
    if let (Some(prev), Some(first)) = (previous, ticks.first())
        && (first.timestamp_ms < prev.timestamp_ms || first.sequence <= prev.sequence)
    {
        return Err("cross-chunk timestamp or sequence order invalid".into());
    }
    if !ticks.is_empty() {
        if p.chunks.len() == 4096 {
            return Err("import chunk budget exceeded".into());
        }
        let mut d = data::dataset(
            config.profile.clone(),
            ticks,
            &config.source,
            config.archive_hash.clone(),
            synthetic,
        )?;
        d.manifest.imported_at = p.started_at.clone();
        d.manifest.parser_version = config.parser.clone();
        d.verify()?;
        let meta =
            serde_json::to_vec(&serde_json::json!({"profile":d.profile,"manifest":d.manifest}))
                .map_err(|e| e.to_string())?
                .len() as u64;
        let predicted = 52 + meta + 32 * d.ticks.len() as u64;
        if p.chunks.iter().map(|c| c.bytes).sum::<u64>() + predicted > OUTPUT_CAP {
            return Err("import artifact quota exceeded".into());
        }
        let name = format!("chunk-{:06}.rsf", p.chunks.len());
        let path = root.join(&name);
        let hash = data::hash(&d)?;
        if path.exists() {
            if data::hash(&native::read(&path)?)? != hash {
                return Err("uncommitted chunk conflicts; preserve and investigate".into());
            }
        } else {
            native::write(&path, &d)?;
        }
        let size = fs::metadata(&path).map_err(|e| e.to_string())?.len();
        if size != predicted {
            return Err("unexpected native artifact size".into());
        }
        p.events += d.ticks.len();
        p.chunks.push(Chunk {
            file: name,
            dataset_hash: hash,
            events: d.ticks.len(),
            next_byte: reader.position().byte(),
            bytes: size,
        });
    }
    p.next_byte = reader.position().byte();
    p.complete = eof || p.next_byte == config.archive_bytes;
    store(&tx, &p)?;
    tx.commit().map_err(db)?;
    Ok(p)
}
