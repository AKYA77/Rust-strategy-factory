//! Bounded deterministic migration of the legacy corrected Parquet corpus into native `.rsf` chunks.
//!
//! This is a format conversion, not a qualification step. The legacy stream was already
//! corrected and quarantined by the retired importer; this module preserves that stream
//! exactly, re-verifies its logical SHA-256, and refuses corrupted or inconsistent inputs.
//! No calendar, quality, profile or research-eligibility approval is granted here.

use crate::{
    Result,
    contracts::Profile,
    data::{self, DataClass, Tick},
    native,
};
use parquet::{
    column::reader::ColumnReader,
    file::{
        metadata::FileMetaData,
        reader::{FileReader, RowGroupReader},
        serialized_reader::SerializedFileReader,
    },
    schema::parser::parse_message_type,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

/// Parser identity recorded in every migrated chunk manifest.
pub const CHUNK_PARSER_VERSION: &str = "historical-parquet-migration-chunk-v1";
/// Receipt identity for a completed migration directory.
pub const RECEIPT_VERSION: &str = "historical-parquet-migration-receipt-v1";
const LEGACY_SCHEMA: &str = "message schema { optional binary instrument_id (String); optional int64 event_ts_utc_ns; optional int64 bid_int; optional int64 ask_int; optional int64 price_scale; optional binary source_id (String); optional binary ingest_batch_id (String); optional int64 original_record_order; optional binary source_precision (String); optional binary quality_flags (String); }";
/// Aligned with the native corpus admission cap (`ordered-native-corpus-v1` = 100,000).
const DEFAULT_CHUNK_EVENTS: usize = 100_000;
const MAX_CHUNK_EVENTS: usize = 100_000;
const MAX_PARQUET_FILE: u64 = 512 * 1024 * 1024;
const MAX_CHUNKS: usize = 8_192;
const NS_PER_MS: i64 = 1_000_000;
const GAP_MS: i64 = 3_600_000;
/// Decoded-row and string-byte preflight for one untrusted Parquet row group. A compressed
/// file-size ceiling does not bound decoded allocation, so these are enforced explicitly.
const MAX_ROW_GROUP_ROWS: usize = 65_536;
const MAX_GROUP_STRING_BYTES: u64 = 256 * 1024 * 1024;
/// Read columns in bounded windows so decoded allocation is capped before the string-byte
/// budget is exceeded, rather than materializing an entire untrusted group first.
const MAX_WINDOW_ROWS: usize = 8_192;

/// Trusted legacy catalog manifest. The 24-field shape is the canonical `data/v2` shape;
/// alternate or incomplete legacy manifests are refused rather than guessed at.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyManifest {
    pub schema_version: String,
    pub dataset_hash: String,
    pub source_sha256: String,
    pub parquet_sha256: String,
    pub quote_stream_sha256: String,
    pub rows: u64,
    pub first_ns: i64,
    pub latest_verified_ns: i64,
    pub instrument: String,
    pub provenance: serde_json::Value,
    pub fidelity: String,
    pub price_scale: i64,
    pub source_precision: String,
    pub source_id: String,
    pub ingest_batch_id: String,
    pub quality_status: String,
    pub batch_rows: u64,
    pub parquet_row_groups: u64,
    pub consecutive_exact_duplicates_retained: u64,
    pub observed_price_quantum: String,
    pub quantum_caveat: String,
    pub gap_count_over_60s: u64,
    pub gap_sample: serde_json::Value,
    pub readback_exact: bool,
}

/// Quarantine and correction evidence extracted from a legacy import receipt.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportEvidence {
    pub corrected_content_sha256: String,
    #[serde(default)]
    pub source_order_correction: serde_json::Value,
    #[serde(default)]
    pub source_quality_quarantine: serde_json::Value,
}

/// Conversion configuration. A source manifest is required for real-data conversion.
#[derive(Debug, Clone)]
pub struct Config {
    pub source_parquet: PathBuf,
    pub output_dir: PathBuf,
    pub profile: Profile,
    pub legacy_manifest: Option<LegacyManifest>,
    pub legacy_manifest_path: Option<PathBuf>,
    pub import_evidence: Option<ImportEvidence>,
    pub chunk_events: usize,
}

impl Config {
    pub fn new(source_parquet: &Path, output_dir: &Path, profile: Profile) -> Result<Self> {
        let chunk_events = DEFAULT_CHUNK_EVENTS;
        if chunk_events == 0 || chunk_events > MAX_CHUNK_EVENTS {
            return Err("invalid default chunk budget".into());
        }
        Ok(Self {
            source_parquet: source_parquet.to_path_buf(),
            output_dir: output_dir.to_path_buf(),
            profile,
            legacy_manifest: None,
            legacy_manifest_path: None,
            import_evidence: None,
            chunk_events,
        })
    }
    pub fn with_chunk_events(mut self, chunk_events: usize) -> Result<Self> {
        if chunk_events == 0 || chunk_events > MAX_CHUNK_EVENTS {
            return Err("chunk budget must be 1..=100000".into());
        }
        self.chunk_events = chunk_events;
        Ok(self)
    }
    pub fn with_manifest(mut self, path: &Path) -> Result<Self> {
        let file = File::open(path).map_err(|e| e.to_string())?;
        if file.metadata().map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
            return Err("legacy manifest exceeds 16 MiB budget".into());
        }
        let manifest: LegacyManifest =
            serde_json::from_reader(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
        if manifest.schema_version != "data/v2" {
            return Err("unsupported legacy schema version".into());
        }
        self.legacy_manifest = Some(manifest);
        self.legacy_manifest_path = Some(path.to_path_buf());
        Ok(self)
    }
    pub fn with_import_receipt(mut self, path: &Path) -> Result<Self> {
        let value: serde_json::Value =
            serde_json::from_reader(File::open(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let resume = value
            .get("import_resume")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let evidence = ImportEvidence {
            corrected_content_sha256: resume
                .get("corrected_content_sha256")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            source_order_correction: resume
                .get("source_order_correction")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            source_quality_quarantine: resume
                .get("source_quality_quarantine")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        };
        self.import_evidence = Some(evidence);
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkReceipt {
    pub file: String,
    pub events: u64,
    pub bytes: u64,
    pub dataset_hash: String,
    pub first_ms: i64,
    pub last_ms: i64,
    pub first_sequence: u64,
    pub last_sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceReceipt {
    pub parquet_path: String,
    pub parquet_sha256: String,
    pub source_sha256: String,
    pub dataset_hash: String,
    pub instrument: String,
    pub rows: u64,
    pub first_ns: i64,
    pub latest_verified_ns: i64,
    pub price_scale: i64,
    pub source_precision: String,
    pub source_id: String,
    pub ingest_batch_id: String,
    pub quality_status: String,
    pub fidelity: String,
    pub provenance: serde_json::Value,
    pub import_evidence: Option<ImportEvidence>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: String,
    pub profile: Profile,
    pub class: DataClass,
    pub source: SourceReceipt,
    pub chunks: Vec<ChunkReceipt>,
    pub events: u64,
    pub first_ms: i64,
    pub last_ms: i64,
    pub timestamp_ties: u64,
    pub identical_quote_events: u64,
    pub gaps_over_hour: u64,
    pub quote_stream_sha256: String,
    pub output_hash: String,
    pub limitations: String,
}

struct State {
    chunks: Vec<ChunkReceipt>,
    rows_done: u64,
    last_tick: Option<Tick>,
    ties: u64,
    repeats: u64,
    gaps: u64,
    first_ms: Option<i64>,
}

impl State {
    fn empty() -> Self {
        Self {
            chunks: Vec::new(),
            rows_done: 0,
            last_tick: None,
            ties: 0,
            repeats: 0,
            gaps: 0,
            first_ms: None,
        }
    }
}

struct GroupColumns {
    instrument: Vec<String>,
    ts: Vec<i64>,
    bid: Vec<i64>,
    ask: Vec<i64>,
    scale: Vec<i64>,
    source: Vec<String>,
    batch: Vec<String>,
    order: Vec<i64>,
    precision: Vec<String>,
    quality: Vec<String>,
}

#[derive(Clone)]
struct Meta {
    instrument: String,
    scale: i64,
    source: String,
    batch: String,
    precision: String,
}

impl Meta {
    fn from_columns(cols: &GroupColumns, i: usize) -> Result<Self> {
        Ok(Self {
            instrument: cols.instrument[i].clone(),
            scale: cols.scale[i],
            source: cols.source[i].clone(),
            batch: cols.batch[i].clone(),
            precision: cols.precision[i].clone(),
        })
    }
    fn same(&self, other: &Self) -> bool {
        self.instrument == other.instrument
            && self.scale == other.scale
            && self.source == other.source
            && self.batch == other.batch
            && self.precision == other.precision
    }
}

#[derive(Serialize)]
struct StreamMetadata<'a> {
    batch: &'a str,
    instrument: &'a str,
    precision: &'a str,
    scale: i64,
    source: &'a str,
}

struct QuoteScan {
    hash: String,
    rows: u64,
    first_ns: i64,
    last_ns: i64,
    meta: Meta,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(err)?;
    let length = file.metadata().map_err(err)?.len();
    if length > MAX_PARQUET_FILE {
        return Err("legacy Parquet file budget exceeded".into());
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(err)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

fn verify_schema(meta: &FileMetaData) -> Result<()> {
    let expected = parse_message_type(LEGACY_SCHEMA).map_err(err)?;
    if meta.schema_descr().root_schema() != &expected {
        return Err("legacy Parquet exact schema mismatch".into());
    }
    Ok(())
}

fn read_int64_column(group: &dyn RowGroupReader, index: usize, rows: usize) -> Result<Vec<i64>> {
    let mut column = match group.get_column_reader(index).map_err(err)? {
        ColumnReader::Int64ColumnReader(column) => column,
        _ => return Err(format!("legacy column {index} physical type mismatch")),
    };
    let mut values = Vec::with_capacity(rows);
    while values.len() < rows {
        let window = (rows - values.len()).min(MAX_WINDOW_ROWS);
        let mut def = Vec::new();
        let mut chunk = Vec::new();
        let (_, values_read, levels) = column
            .read_records(window, Some(&mut def), None, &mut chunk)
            .map_err(err)?;
        if levels != window
            || values_read != window
            || def.len() != window
            || def.iter().any(|d| *d != 1)
        {
            return Err(format!(
                "legacy column {index} level/value count or null mismatch"
            ));
        }
        values.extend_from_slice(&chunk);
    }
    Ok(values)
}

fn read_string_column(
    group: &dyn RowGroupReader,
    index: usize,
    rows: usize,
    budget: &mut u64,
) -> Result<Vec<String>> {
    let mut column = match group.get_column_reader(index).map_err(err)? {
        ColumnReader::ByteArrayColumnReader(column) => column,
        _ => return Err(format!("legacy column {index} physical type mismatch")),
    };
    let mut values = Vec::with_capacity(rows);
    while values.len() < rows {
        let window = (rows - values.len()).min(MAX_WINDOW_ROWS);
        let mut def = Vec::new();
        let mut chunk = Vec::new();
        let (_, values_read, levels) = column
            .read_records(window, Some(&mut def), None, &mut chunk)
            .map_err(err)?;
        if levels != window
            || values_read != window
            || def.len() != window
            || def.iter().any(|d| *d != 1)
        {
            return Err(format!(
                "legacy column {index} level/value count or null mismatch"
            ));
        }
        let bytes = chunk.iter().map(|value| value.len() as u64).sum::<u64>();
        if bytes > *budget {
            return Err(format!(
                "legacy column {index} exceeds the decoded string byte budget"
            ));
        }
        *budget -= bytes;
        values.extend(
            chunk
                .iter()
                .map(|value| value.as_utf8().map(str::to_string).map_err(err))
                .collect::<Result<Vec<_>>>()?,
        );
    }
    Ok(values)
}

fn read_group(group: &dyn RowGroupReader) -> Result<GroupColumns> {
    let rows = group.metadata().num_rows() as usize;
    if rows == 0 {
        return Err("legacy Parquet row group is empty".into());
    }
    if rows > MAX_ROW_GROUP_ROWS {
        return Err("legacy Parquet row group exceeds the decoded-row budget".into());
    }
    let mut string_budget = MAX_GROUP_STRING_BYTES;
    Ok(GroupColumns {
        instrument: read_string_column(group, 0, rows, &mut string_budget)?,
        ts: read_int64_column(group, 1, rows)?,
        bid: read_int64_column(group, 2, rows)?,
        ask: read_int64_column(group, 3, rows)?,
        scale: read_int64_column(group, 4, rows)?,
        source: read_string_column(group, 5, rows, &mut string_budget)?,
        batch: read_string_column(group, 6, rows, &mut string_budget)?,
        order: read_int64_column(group, 7, rows)?,
        precision: read_string_column(group, 8, rows, &mut string_budget)?,
        quality: read_string_column(group, 9, rows, &mut string_budget)?,
    })
}

fn open_reader(path: &Path) -> Result<SerializedFileReader<File>> {
    let file = File::open(path).map_err(err)?;
    if file.metadata().map_err(err)?.len() > MAX_PARQUET_FILE {
        return Err("legacy Parquet file budget exceeded".into());
    }
    SerializedFileReader::new(file).map_err(err)
}

fn scan_quote_stream(path: &Path) -> Result<QuoteScan> {
    let reader = open_reader(path)?;
    verify_schema(reader.metadata().file_metadata())?;
    let mut hasher = Sha256::new();
    let mut meta: Option<Meta> = None;
    let mut rows = 0u64;
    let mut first_ns = 0i64;
    let mut last_ns = 0i64;
    for index in 0..reader.metadata().num_row_groups() {
        let group = reader.get_row_group(index).map_err(err)?;
        let cols = read_group(group.as_ref())?;
        for i in 0..cols.ts.len() {
            if let Some(current) = &meta {
                let row = Meta::from_columns(&cols, i)?;
                if !current.same(&row) {
                    return Err("legacy Parquet mixes source/instrument units".into());
                }
            } else {
                let row = Meta::from_columns(&cols, i)?;
                let serialized = StreamMetadata {
                    batch: &row.batch,
                    instrument: &row.instrument,
                    precision: &row.precision,
                    scale: row.scale,
                    source: &row.source,
                };
                hasher.update(serde_json::to_vec(&serialized).map_err(err)?);
                meta = Some(row);
                first_ns = cols.ts[i];
            }
            hasher.update(cols.ts[i].to_be_bytes());
            hasher.update(cols.bid[i].to_be_bytes());
            hasher.update(cols.ask[i].to_be_bytes());
            hasher.update(cols.order[i].to_be_bytes());
            last_ns = cols.ts[i];
            rows += 1;
        }
    }
    let meta = meta.ok_or("empty legacy Parquet")?;
    Ok(QuoteScan {
        hash: hex(&hasher.finalize()),
        rows,
        first_ns,
        last_ns,
        meta,
    })
}

fn expected_meta(config: &Config) -> Result<Option<Meta>> {
    let Some(manifest) = &config.legacy_manifest else {
        return Ok(None);
    };
    Ok(Some(Meta {
        instrument: manifest.instrument.clone(),
        scale: manifest.price_scale,
        source: manifest.source_id.clone(),
        batch: manifest.ingest_batch_id.clone(),
        precision: manifest.source_precision.clone(),
    }))
}

fn source_description(config: &Config, parquet_sha256: &str) -> String {
    match &config.legacy_manifest {
        Some(manifest) => format!(
            "historical-parquet-migration-v1:{}:{}",
            manifest.dataset_hash, manifest.source_sha256
        ),
        None => format!("historical-parquet-migration-v1:unverified:{parquet_sha256}"),
    }
}

fn archive_hash(config: &Config, parquet_sha256: &str) -> String {
    config
        .legacy_manifest
        .as_ref()
        .map(|m| m.source_sha256.clone())
        .unwrap_or_else(|| parquet_sha256.to_string())
}

fn deterministic_imported_at(archive_hash: &str) -> String {
    format!("historical-parquet-migration-v1:{archive_hash}")
}

fn source_limitations(config: &Config) -> Vec<String> {
    let mut limitations = Vec::new();
    match &config.legacy_manifest {
        Some(manifest) => {
            if let Some(values) = manifest.provenance.get("limitations").and_then(|v| v.as_array()) {
                for value in values {
                    if let Some(text) = value.as_str() {
                        limitations.push(text.to_string());
                    }
                }
            }
            if !manifest.quantum_caveat.is_empty() {
                limitations.push(manifest.quantum_caveat.clone());
            }
            limitations.push(format!("legacy quality status: {}", manifest.quality_status));
        }
        None => limitations.push(
            "Unverified source: no trusted legacy catalog manifest was supplied; conversion is structural only."
                .into(),
        ),
    }
    limitations.push(
        "Conversion preserves the already-corrected legacy stream; it does not reapply, invent or discard corrections."
            .into(),
    );
    limitations.push(
        "No calendar, quality, profile or research-eligibility approval is granted by conversion."
            .into(),
    );
    limitations
}

fn rebuild_state(config: &Config, source_desc: &str, archive: &str) -> Result<State> {
    let mut state = State::empty();
    let mut entries: Vec<(usize, String)> = Vec::new();
    for entry in fs::read_dir(&config.output_dir).map_err(err)? {
        let entry = entry.map_err(err)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(index) = name
            .strip_prefix("chunk-")
            .and_then(|rest| rest.strip_suffix(".rsf"))
            .and_then(|index| index.parse::<usize>().ok())
        {
            entries.push((index, name));
        }
    }
    entries.sort_by_key(|(index, _)| *index);
    for (expected, (index, name)) in entries.into_iter().enumerate() {
        if index != expected {
            return Err("legacy migration chunk sequence has a gap or unexpected file".into());
        }
        let path = config.output_dir.join(&name);
        let dataset = native::read(&path)?;
        let events = dataset.ticks.len() as u64;
        if events == 0 {
            return Err("empty migrated chunk".into());
        }
        if dataset.profile != config.profile
            || dataset.manifest.parser_version != CHUNK_PARSER_VERSION
            || dataset.manifest.source != source_desc
            || dataset.manifest.archive_hash != archive
            || dataset.manifest.symbol != config.profile.symbol
            || dataset.manifest.class != DataClass::RealStructurallyValid
            || dataset.manifest.events as u64 != events
        {
            return Err("migrated chunk context mismatch".into());
        }
        let first = dataset.ticks[0];
        let last = dataset.ticks[dataset.ticks.len() - 1];
        if first.sequence != state.rows_done || last.sequence != state.rows_done + events - 1 {
            return Err("migrated chunk sequence is not continuous".into());
        }
        state.ties += dataset.manifest.timestamp_ties as u64;
        state.repeats += dataset.manifest.identical_quote_events as u64;
        state.gaps += dataset.manifest.gaps_over_hour as u64;
        if let Some(previous) = state.last_tick {
            if first.timestamp_ms < previous.timestamp_ms {
                return Err("migrated chunk timestamp order is not continuous".into());
            }
            if first.timestamp_ms == previous.timestamp_ms {
                state.ties += 1;
            }
            if first.bid == previous.bid && first.ask == previous.ask {
                state.repeats += 1;
            }
            if first.timestamp_ms - previous.timestamp_ms > GAP_MS {
                state.gaps += 1;
            }
        } else {
            state.first_ms = Some(first.timestamp_ms);
        }
        let receipt = ChunkReceipt {
            file: name,
            events,
            bytes: fs::metadata(&path).map_err(err)?.len(),
            dataset_hash: data::hash(&dataset)?,
            first_ms: first.timestamp_ms,
            last_ms: last.timestamp_ms,
            first_sequence: first.sequence,
            last_sequence: last.sequence,
        };
        state.rows_done += events;
        state.last_tick = Some(last);
        state.chunks.push(receipt);
    }
    if state.chunks.len() > MAX_CHUNKS {
        return Err("migrated chunk budget exceeded".into());
    }
    Ok(state)
}

#[allow(clippy::too_many_arguments)]
fn write_chunk(
    config: &Config,
    index: usize,
    ticks: &mut Vec<Tick>,
    state: &mut State,
    previous: Option<Tick>,
    source_desc: &str,
    archive: &str,
) -> Result<()> {
    if ticks.is_empty() {
        return Err("cannot publish an empty migrated chunk".into());
    }
    let first = ticks[0];
    let last = ticks[ticks.len() - 1];
    let mut dataset = data::dataset(
        config.profile.clone(),
        std::mem::take(ticks),
        source_desc,
        archive.to_string(),
        false,
    )?;
    dataset.manifest.parser_version = CHUNK_PARSER_VERSION.into();
    dataset.manifest.imported_at = deterministic_imported_at(archive);
    dataset.verify()?;
    let hash = data::hash(&dataset)?;
    let name = format!("chunk-{index:06}.rsf");
    let path = config.output_dir.join(&name);
    if path.exists() {
        let existing = native::read(&path)?;
        if data::hash(&existing)? != hash {
            return Err("uncommitted migrated chunk conflicts; preserve and investigate".into());
        }
    } else {
        native::write(&path, &dataset)?;
    }
    let bytes = fs::metadata(&path).map_err(err)?.len();
    state.ties += dataset.manifest.timestamp_ties as u64;
    state.repeats += dataset.manifest.identical_quote_events as u64;
    state.gaps += dataset.manifest.gaps_over_hour as u64;
    if let Some(previous) = previous {
        if first.timestamp_ms < previous.timestamp_ms {
            return Err("migrated chunk timestamp order is not continuous".into());
        }
        if first.timestamp_ms == previous.timestamp_ms {
            state.ties += 1;
        }
        if first.bid == previous.bid && first.ask == previous.ask {
            state.repeats += 1;
        }
        if first.timestamp_ms - previous.timestamp_ms > GAP_MS {
            state.gaps += 1;
        }
    } else {
        state.first_ms = Some(first.timestamp_ms);
    }
    state.rows_done = last.sequence + 1;
    state.last_tick = Some(last);
    state.chunks.push(ChunkReceipt {
        file: name,
        events: dataset.manifest.events as u64,
        bytes,
        dataset_hash: hash,
        first_ms: first.timestamp_ms,
        last_ms: last.timestamp_ms,
        first_sequence: first.sequence,
        last_sequence: last.sequence,
    });
    Ok(())
}

fn scale_factor(profile: &Profile, scale: i64) -> Result<i64> {
    let profile_scale = profile.price_scale;
    if profile_scale <= 0 || scale <= 0 || scale % profile_scale != 0 {
        return Err("legacy price scale is not an exact multiple of the profile scale".into());
    }
    let factor = scale / profile_scale;
    if factor <= 0 || factor > i64::MAX / 2 {
        return Err("legacy price scale conversion factor invalid".into());
    }
    Ok(factor)
}

fn convert_scale(config: &Config, scale: i64) -> Result<i64> {
    scale_factor(&config.profile, scale)
}

/// Bind idempotent reuse to the exact requested source bytes, manifest identity and profile.
/// A self-consistent existing receipt is not evidence for a different requested conversion.
fn verify_reuse_scope(config: &Config, receipt: &Receipt) -> Result<()> {
    if receipt.version != RECEIPT_VERSION {
        return Err("completed migration receipt version mismatch".into());
    }
    if receipt.class != DataClass::RealStructurallyValid {
        return Err("completed migration receipt is not structurally valid".into());
    }
    if receipt.profile != config.profile {
        return Err("requested profile does not match the completed migration".into());
    }
    let source_hash = sha256_file(&config.source_parquet)?;
    if source_hash != receipt.source.parquet_sha256 {
        return Err("requested source bytes do not match the completed migration".into());
    }
    if let Some(manifest) = &config.legacy_manifest
        && (receipt.source.dataset_hash != manifest.dataset_hash
            || receipt.source.source_sha256 != manifest.source_sha256
            || receipt.source.parquet_sha256 != manifest.parquet_sha256
            || receipt.source.rows != manifest.rows
            || receipt.source.instrument != manifest.instrument
            || receipt.source.price_scale != manifest.price_scale
            || receipt.source.source_id != manifest.source_id
            || receipt.source.ingest_batch_id != manifest.ingest_batch_id
            || receipt.source.quality_status != manifest.quality_status
            || receipt.source.fidelity != manifest.fidelity
            || receipt.source.source_precision != manifest.source_precision
            || receipt.source.provenance != manifest.provenance
            || receipt.quote_stream_sha256 != manifest.quote_stream_sha256
            || receipt.source.first_ns != manifest.first_ns
            || receipt.source.latest_verified_ns != manifest.latest_verified_ns)
    {
        return Err(
            "requested legacy manifest identity does not match the completed migration".into(),
        );
    }
    if receipt.source.import_evidence != config.import_evidence {
        return Err("requested import evidence does not match the completed migration".into());
    }
    Ok(())
}

/// Compare published native ticks with the exact legacy source rows they claim to represent.
/// Self-consistent native checksums and a fresh receipt hash do not establish this equality.
fn verify_chunks_against_source(
    output_dir: &Path,
    chunks: &[ChunkReceipt],
    rows: u64,
    profile: &Profile,
    source_parquet: &Path,
) -> Result<()> {
    if rows == 0 {
        return Ok(());
    }
    let reader = open_reader(source_parquet)?;
    verify_schema(reader.metadata().file_metadata())?;
    let first = reader.get_row_group(0).map_err(err)?;
    let first_cols = read_group(first.as_ref())?;
    let factor = scale_factor(profile, first_cols.scale[0])?;
    let expected_scale = factor * profile.price_scale;
    let mut chunk_index = 0usize;
    let mut tick_index = 0usize;
    let mut current: Option<crate::data::Dataset> = None;
    let mut seen = 0u64;
    'groups: for group_index in 0..reader.metadata().num_row_groups() {
        let group = reader.get_row_group(group_index).map_err(err)?;
        let cols = read_group(group.as_ref())?;
        for i in 0..cols.ts.len() {
            if seen >= rows {
                break 'groups;
            }
            while current.as_ref().is_none_or(|d| tick_index >= d.ticks.len()) {
                if chunk_index >= chunks.len() {
                    return Err("native chunks are shorter than the recorded row count".into());
                }
                let path = output_dir.join(&chunks[chunk_index].file);
                current = Some(native::read(&path)?);
                tick_index = 0;
                chunk_index += 1;
            }
            let dataset = current.as_ref().ok_or("adopted native chunk missing")?;
            let adopted = dataset.ticks[tick_index];
            let ts_ns = cols.ts[i];
            if ts_ns < 0 || ts_ns % NS_PER_MS != 0 || cols.scale[i] != expected_scale {
                return Err("legacy source row is inconsistent during equivalence check".into());
            }
            let expected = Tick {
                timestamp_ms: ts_ns / NS_PER_MS,
                sequence: seen,
                bid: cols.bid[i] / factor,
                ask: cols.ask[i] / factor,
            };
            if adopted != expected {
                return Err(
                    "native output differs from the legacy source rows it claims to represent"
                        .into(),
                );
            }
            tick_index += 1;
            seen += 1;
        }
    }
    if seen != rows {
        return Err("legacy source is shorter than the recorded native output".into());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn convert_ticks(
    config: &Config,
    cols: &GroupColumns,
    start: usize,
    expected: &mut Option<Meta>,
    factor: i64,
    chunk: &mut Vec<Tick>,
    rows_done: u64,
    previous: &mut Option<Tick>,
) -> Result<()> {
    for i in start..cols.ts.len() {
        let row = Meta::from_columns(cols, i)?;
        match expected {
            None => *expected = Some(row),
            Some(current) => {
                if !current.same(&row) {
                    return Err("legacy Parquet mixes source/instrument units".into());
                }
            }
        }
        if !cols.quality[i].is_empty() {
            return Err("legacy Parquet row carries quality flags".into());
        }
        let ts_ns = cols.ts[i];
        if ts_ns < 0 || ts_ns % NS_PER_MS != 0 {
            return Err("legacy Parquet timestamp is negative or not exact to milliseconds".into());
        }
        let timestamp_ms = ts_ns / NS_PER_MS;
        let bid_int = cols.bid[i];
        let ask_int = cols.ask[i];
        if bid_int <= 0 || ask_int < bid_int {
            return Err("legacy Parquet row is nonpositive or crossed".into());
        }
        if bid_int % factor != 0 || ask_int % factor != 0 {
            return Err("legacy Parquet price is not exact in profile units".into());
        }
        let bid = bid_int / factor;
        let ask = ask_int / factor;
        let tick = Tick {
            timestamp_ms,
            sequence: rows_done + chunk.len() as u64,
            bid,
            ask,
        };
        tick.validate(&config.profile)?;
        if chrono::DateTime::from_timestamp_millis(timestamp_ms).is_none() {
            return Err("legacy Parquet timestamp outside supported calendar".into());
        }
        if let Some(prev) = *previous
            && timestamp_ms < prev.timestamp_ms
        {
            return Err("legacy Parquet timestamp order is not monotonic".into());
        }
        *previous = Some(tick);
        chunk.push(tick);
    }
    Ok(())
}

#[derive(Serialize)]
struct OutputIdentity<'a> {
    version: &'a str,
    profile: &'a Profile,
    dataset_hash: &'a str,
    source_sha256: &'a str,
    parquet_sha256: &'a str,
    instrument: &'a str,
    rows: u64,
    first_ns: i64,
    latest_verified_ns: i64,
    price_scale: i64,
    source_precision: &'a str,
    source_id: &'a str,
    ingest_batch_id: &'a str,
    quality_status: &'a str,
    fidelity: &'a str,
    provenance: &'a serde_json::Value,
    import_evidence: &'a Option<ImportEvidence>,
    limitations: &'a [String],
    chunks: &'a [ChunkReceipt],
    events: u64,
    first_ms: i64,
    last_ms: i64,
    timestamp_ties: u64,
    identical_quote_events: u64,
    gaps_over_hour: u64,
    quote_stream_sha256: &'a str,
}

fn compute_output_hash(receipt: &Receipt) -> Result<String> {
    let identity = OutputIdentity {
        version: &receipt.version,
        profile: &receipt.profile,
        dataset_hash: &receipt.source.dataset_hash,
        source_sha256: &receipt.source.source_sha256,
        parquet_sha256: &receipt.source.parquet_sha256,
        instrument: &receipt.source.instrument,
        rows: receipt.source.rows,
        first_ns: receipt.source.first_ns,
        latest_verified_ns: receipt.source.latest_verified_ns,
        price_scale: receipt.source.price_scale,
        source_precision: &receipt.source.source_precision,
        source_id: &receipt.source.source_id,
        ingest_batch_id: &receipt.source.ingest_batch_id,
        quality_status: &receipt.source.quality_status,
        fidelity: &receipt.source.fidelity,
        provenance: &receipt.source.provenance,
        import_evidence: &receipt.source.import_evidence,
        limitations: &receipt.source.limitations,
        chunks: &receipt.chunks,
        events: receipt.events,
        first_ms: receipt.first_ms,
        last_ms: receipt.last_ms,
        timestamp_ties: receipt.timestamp_ties,
        identical_quote_events: receipt.identical_quote_events,
        gaps_over_hour: receipt.gaps_over_hour,
        quote_stream_sha256: &receipt.quote_stream_sha256,
    };
    data::hash(&identity)
}

fn publish_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(err)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or("artifact filename missing")?
        .to_string_lossy()
        .to_string();
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(err)?;
    let temporary = parent.join(format!(
        ".{name}.{:032x}.tmp",
        nonce
            .iter()
            .fold(0u128, |acc, b| (acc << 8) | u128::from(*b))
    ));
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::hard_link(&temporary, path)?;
        File::open(parent)?.sync_all()
    })();
    let cleanup = fs::remove_file(&temporary);
    result.and(cleanup).map_err(err)
}

/// Atomically replace a derived status artifact. Unlike [`publish_json`], this permits an
/// existing file to be superseded, which is correct for a recomputable batch report.
fn publish_json_replace(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(err)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or("artifact filename missing")?
        .to_string_lossy()
        .to_string();
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(err)?;
    let temporary = parent.join(format!(
        ".{name}.{:032x}.tmp",
        nonce
            .iter()
            .fold(0u128, |acc, b| (acc << 8) | u128::from(*b))
    ));
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(err)
}

/// Convert one legacy canonical partition into a restart-safe directory of native chunks.
///
/// Re-running a completed conversion verifies and returns the existing receipt. Re-running a
/// partially written directory verifies the committed chunks and continues from their exact
/// boundary; chunks are never replaced or duplicated.
pub fn convert(config: &Config) -> Result<Receipt> {
    config.profile.validate()?;
    if config.chunk_events == 0 || config.chunk_events > MAX_CHUNK_EVENTS {
        return Err("chunk budget must be 1..=100000".into());
    }
    fs::create_dir_all(&config.output_dir).map_err(err)?;
    let receipt_path = config.output_dir.join("manifest.json");
    if receipt_path.is_file() {
        let receipt = verify_output(&config.output_dir, None)?;
        verify_reuse_scope(config, &receipt)?;
        return Ok(receipt);
    }
    let parquet_sha256 = sha256_file(&config.source_parquet)?;
    if let Some(manifest) = &config.legacy_manifest
        && manifest.parquet_sha256 != parquet_sha256
    {
        return Err("legacy Parquet SHA256 mismatch".into());
    }
    let reader = open_reader(&config.source_parquet)?;
    verify_schema(reader.metadata().file_metadata())?;
    let total_rows = reader.metadata().file_metadata().num_rows() as u64;
    if let Some(manifest) = &config.legacy_manifest
        && manifest.rows != total_rows
    {
        return Err("legacy Parquet row count mismatch".into());
    }
    let factor = match &config.legacy_manifest {
        Some(manifest) => convert_scale(config, manifest.price_scale)?,
        None => {
            let first = reader.get_row_group(0).map_err(err)?;
            let cols = read_group(first.as_ref())?;
            convert_scale(config, cols.scale[0])?
        }
    };
    let source_desc = source_description(config, &parquet_sha256);
    let archive = archive_hash(config, &parquet_sha256);
    let mut state = rebuild_state(config, &source_desc, &archive)?;
    if state.rows_done > total_rows {
        return Err("migrated chunks exceed the legacy source row count".into());
    }
    verify_chunks_against_source(
        &config.output_dir,
        &state.chunks,
        state.rows_done,
        &config.profile,
        &config.source_parquet,
    )?;
    let expected = expected_meta(config)?;
    if state.rows_done < total_rows {
        let mut remaining = state.rows_done;
        let mut group_index = 0usize;
        let row_groups = reader.metadata().num_row_groups();
        while group_index < row_groups && remaining > 0 {
            let rows = reader
                .get_row_group(group_index)
                .map_err(err)?
                .metadata()
                .num_rows() as u64;
            if remaining >= rows {
                remaining -= rows;
                group_index += 1;
            } else {
                break;
            }
        }
        if group_index == row_groups && remaining > 0 {
            return Err("resume offset exceeds legacy source rows".into());
        }
        let skip_in_group = remaining as usize;
        let mut chunk: Vec<Tick> = Vec::with_capacity(config.chunk_events);
        let mut previous = state.last_tick;
        let mut expected_row = expected.clone();
        for index in group_index..row_groups {
            let group = reader.get_row_group(index).map_err(err)?;
            let cols = read_group(group.as_ref())?;
            let rows = cols.ts.len();
            let start = if index == group_index {
                skip_in_group
            } else {
                0
            };
            if start > rows {
                return Err("resume offset exceeds legacy row group".into());
            }
            convert_ticks(
                config,
                &cols,
                start,
                &mut expected_row,
                factor,
                &mut chunk,
                state.rows_done,
                &mut previous,
            )?;
            while chunk.len() >= config.chunk_events {
                let index = state.chunks.len();
                if index >= MAX_CHUNKS {
                    return Err("migrated chunk budget exceeded".into());
                }
                let mut block: Vec<Tick> = chunk.drain(..config.chunk_events).collect();
                let before = state.last_tick;
                write_chunk(
                    config,
                    index,
                    &mut block,
                    &mut state,
                    before,
                    &source_desc,
                    &archive,
                )?;
            }
        }
        if !chunk.is_empty() {
            let index = state.chunks.len();
            if index >= MAX_CHUNKS {
                return Err("migrated chunk budget exceeded".into());
            }
            let before = state.last_tick;
            write_chunk(
                config,
                index,
                &mut chunk,
                &mut state,
                before,
                &source_desc,
                &archive,
            )?;
        }
    }
    if state.rows_done != total_rows {
        return Err("migration did not consume the exact legacy source rows".into());
    }
    if state.chunks.is_empty() {
        return Err("legacy source produced no migrated chunks".into());
    }
    let first_ms = state.first_ms.ok_or("migration first timestamp missing")?;
    let last_ms = state
        .last_tick
        .map(|t| t.timestamp_ms)
        .ok_or("migration last timestamp missing")?;
    let scan = scan_quote_stream(&config.source_parquet)?;
    if scan.rows != total_rows
        || scan.first_ns != state.first_ms.map(|ms| ms * NS_PER_MS).unwrap_or(0)
    {
        return Err("legacy quote stream boundaries mismatch".into());
    }
    if scan.last_ns != last_ms * NS_PER_MS {
        return Err("legacy quote stream boundaries mismatch".into());
    }
    if let Some(manifest) = &config.legacy_manifest
        && (scan.hash != manifest.quote_stream_sha256
            || scan.meta.instrument != manifest.instrument
            || scan.meta.scale != manifest.price_scale
            || scan.meta.source != manifest.source_id
            || scan.meta.batch != manifest.ingest_batch_id
            || scan.meta.precision != manifest.source_precision
            || scan.first_ns != manifest.first_ns
            || scan.last_ns != manifest.latest_verified_ns)
    {
        return Err("legacy manifest identity or quote stream mismatch".into());
    }
    let parquet_sha256_again = sha256_file(&config.source_parquet)?;
    if parquet_sha256_again != parquet_sha256 {
        return Err("legacy Parquet changed during migration".into());
    }
    let limitations = source_limitations(config);
    let source = SourceReceipt {
        parquet_path: config.source_parquet.to_string_lossy().to_string(),
        parquet_sha256: parquet_sha256.clone(),
        source_sha256: config
            .legacy_manifest
            .as_ref()
            .map(|m| m.source_sha256.clone())
            .unwrap_or_else(|| archive.clone()),
        dataset_hash: config
            .legacy_manifest
            .as_ref()
            .map(|m| m.dataset_hash.clone())
            .unwrap_or_else(|| parquet_sha256.clone()),
        instrument: scan.meta.instrument.clone(),
        rows: total_rows,
        first_ns: scan.first_ns,
        latest_verified_ns: scan.last_ns,
        price_scale: scan.meta.scale,
        source_precision: scan.meta.precision.clone(),
        source_id: scan.meta.source.clone(),
        ingest_batch_id: scan.meta.batch.clone(),
        quality_status: config
            .legacy_manifest
            .as_ref()
            .map(|m| m.quality_status.clone())
            .unwrap_or_else(|| "UNVERIFIED".into()),
        fidelity: config
            .legacy_manifest
            .as_ref()
            .map(|m| m.fidelity.clone())
            .unwrap_or_else(|| "SOURCE_BID_ASK".into()),
        provenance: config
            .legacy_manifest
            .as_ref()
            .map(|m| m.provenance.clone())
            .unwrap_or(serde_json::Value::Null),
        import_evidence: config.import_evidence.clone(),
        limitations: limitations.clone(),
    };
    let mut receipt = Receipt {
        version: RECEIPT_VERSION.into(),
        profile: config.profile.clone(),
        class: DataClass::RealStructurallyValid,
        source,
        chunks: state.chunks.clone(),
        events: state.rows_done,
        first_ms,
        last_ms,
        timestamp_ties: state.ties,
        identical_quote_events: state.repeats,
        gaps_over_hour: state.gaps,
        quote_stream_sha256: scan.hash.clone(),
        output_hash: String::new(),
        limitations: limitations.join("\n"),
    };
    receipt.output_hash = compute_output_hash(&receipt)?;
    publish_json(&receipt_path, &receipt)?;
    Ok(receipt)
}

/// Verify a published migration directory. Partial output (no receipt) is refused.
/// When `source_parquet` is supplied the logical quote stream is independently recomputed.
pub fn verify_output(dir: &Path, source_parquet: Option<&Path>) -> Result<Receipt> {
    let path = dir.join("manifest.json");
    if !path.is_file() {
        return Err("migration receipt missing; output is partial".into());
    }
    let bytes = fs::read(&path).map_err(err)?;
    let receipt: Receipt = serde_json::from_slice(&bytes).map_err(err)?;
    if receipt.version != RECEIPT_VERSION {
        return Err("unsupported migration receipt version".into());
    }
    receipt.profile.validate()?;
    if receipt.class != DataClass::RealStructurallyValid {
        return Err("migrated receipt is not structurally valid".into());
    }
    if receipt.output_hash != compute_output_hash(&receipt)? {
        return Err("migration receipt output hash mismatch".into());
    }
    if receipt.chunks.is_empty() || receipt.chunks.len() > MAX_CHUNKS {
        return Err("migration receipt chunk budget invalid".into());
    }
    let source_desc = source_description_from_receipt(&receipt);
    let mut rows = 0u64;
    let mut last_tick: Option<Tick> = None;
    let mut ties = 0u64;
    let mut repeats = 0u64;
    let mut gaps = 0u64;
    let mut first_ms: Option<i64> = None;
    for chunk in &receipt.chunks {
        let path = dir.join(&chunk.file);
        let dataset = native::read(&path)?;
        let events = dataset.ticks.len() as u64;
        if dataset.profile != receipt.profile
            || dataset.manifest.parser_version != CHUNK_PARSER_VERSION
            || dataset.manifest.source != source_desc
            || dataset.manifest.class != DataClass::RealStructurallyValid
            || dataset.manifest.events as u64 != events
            || chunk.events != events
            || chunk.bytes != fs::metadata(&path).map_err(err)?.len()
            || chunk.dataset_hash != data::hash(&dataset)?
        {
            return Err("migrated chunk receipt mismatch".into());
        }
        let first = dataset.ticks[0];
        let last = dataset.ticks[dataset.ticks.len() - 1];
        if chunk.first_ms != first.timestamp_ms
            || chunk.last_ms != last.timestamp_ms
            || chunk.first_sequence != first.sequence
            || chunk.last_sequence != last.sequence
            || first.sequence != rows
            || last.sequence != rows + events - 1
        {
            return Err("migrated chunk sequence receipt mismatch".into());
        }
        ties += dataset.manifest.timestamp_ties as u64;
        repeats += dataset.manifest.identical_quote_events as u64;
        gaps += dataset.manifest.gaps_over_hour as u64;
        if let Some(previous) = last_tick {
            if first.timestamp_ms < previous.timestamp_ms {
                return Err("migrated chunk timestamp order mismatch".into());
            }
            if first.timestamp_ms == previous.timestamp_ms {
                ties += 1;
            }
            if first.bid == previous.bid && first.ask == previous.ask {
                repeats += 1;
            }
            if first.timestamp_ms - previous.timestamp_ms > GAP_MS {
                gaps += 1;
            }
        } else {
            first_ms = Some(first.timestamp_ms);
        }
        rows += events;
        last_tick = Some(last);
    }
    if rows != receipt.events
        || first_ms != Some(receipt.first_ms)
        || last_tick.map(|t| t.timestamp_ms) != Some(receipt.last_ms)
        || ties != receipt.timestamp_ties
        || repeats != receipt.identical_quote_events
        || gaps != receipt.gaps_over_hour
    {
        return Err("migration receipt totals mismatch".into());
    }
    if let Some(source) = source_parquet {
        let scan = scan_quote_stream(source)?;
        if scan.hash != receipt.quote_stream_sha256
            || scan.rows != receipt.source.rows
            || scan.first_ns != receipt.source.first_ns
            || scan.last_ns != receipt.source.latest_verified_ns
            || scan.meta.instrument != receipt.source.instrument
            || scan.meta.scale != receipt.source.price_scale
            || scan.meta.source != receipt.source.source_id
            || scan.meta.batch != receipt.source.ingest_batch_id
            || scan.meta.precision != receipt.source.source_precision
        {
            return Err("legacy source quote stream mismatch".into());
        }
        verify_chunks_against_source(
            dir,
            &receipt.chunks,
            receipt.events,
            &receipt.profile,
            source,
        )?;
    }
    Ok(receipt)
}

fn source_description_from_receipt(receipt: &Receipt) -> String {
    format!(
        "historical-parquet-migration-v1:{}:{}",
        receipt.source.dataset_hash, receipt.source.source_sha256
    )
}

/// One canonical instrument-month partition selected for migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Partition {
    pub instrument: String,
    pub year: u32,
    pub month: u32,
    pub dataset_hash: String,
    pub parquet_path: String,
    pub manifest_path: String,
    pub import_receipt_path: Option<String>,
    pub rows: u64,
    pub price_scale: i64,
    pub source_sha256: String,
    pub quote_stream_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartitionReport {
    pub instrument: String,
    pub year: u32,
    pub month: u32,
    pub dataset_hash: String,
    pub status: String,
    pub events: u64,
    pub chunks: u64,
    pub output_hash: String,
    pub output_dir: String,
    pub error: Option<String>,
    pub wall_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchReport {
    pub version: String,
    pub root: String,
    pub output_root: String,
    pub workers: usize,
    pub chunk_events: usize,
    pub planned: usize,
    pub converted: usize,
    pub already_complete: usize,
    pub failed: usize,
    pub events: u64,
    pub partitions: Vec<PartitionReport>,
    pub limitations: String,
}

const BATCH_VERSION: &str = "historical-parquet-migration-batch-v1";
const CANONICAL_QUALITY: &str = "STRUCTURAL_PASS_CALENDAR_REVIEW_REQUIRED";

/// Select the canonical instrument-month partitions from the retired factory's catalog.
///
/// Only exact 24-field `data/v2` manifests with the canonical quality status are accepted.
/// The instrument/year/month tuple comes from joining `source_sha256` to the archive
/// manifest. Duplicate tuples resolve to the lexicographically smallest `dataset_hash`.
pub fn plan(root: &Path) -> Result<Vec<Partition>> {
    let archives = root.join("archives");
    let catalog = root.join("catalog");
    let mut archive_meta: std::collections::HashMap<String, (String, u32, u32)> =
        std::collections::HashMap::new();
    for entry in fs::read_dir(&archives).map_err(err)? {
        let entry = entry.map_err(err)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with("-ticks.manifest.json") {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_reader(File::open(entry.path()).map_err(err)?).map_err(err)?;
        let (Some(sha), Some(instrument), Some(year), Some(month)) = (
            value.get("sha256").and_then(|v| v.as_str()),
            value.get("instrument").and_then(|v| v.as_str()),
            value.get("year").and_then(|v| v.as_u64()),
            value.get("month").and_then(|v| v.as_u64()),
        ) else {
            continue;
        };
        archive_meta.insert(
            sha.to_string(),
            (instrument.to_string(), year as u32, month as u32),
        );
    }
    let mut receipt_by_source: std::collections::HashMap<String, PathBuf> =
        std::collections::HashMap::new();
    let receipts = catalog.join("import-receipts");
    if receipts.is_dir() {
        for entry in fs::read_dir(&receipts).map_err(err)? {
            let entry = entry.map_err(err)?;
            let value: serde_json::Value =
                serde_json::from_reader(File::open(entry.path()).map_err(err)?).map_err(err)?;
            let source = value.get("source");
            if let Some(sha) = source
                .and_then(|s| s.get("sha256"))
                .and_then(|v| v.as_str())
            {
                receipt_by_source.insert(sha.to_string(), entry.path());
            }
        }
    }
    let mut groups: std::collections::HashMap<(String, u32, u32), Partition> =
        std::collections::HashMap::new();
    for entry in fs::read_dir(&catalog).map_err(err)? {
        let entry = entry.map_err(err)?;
        if !entry.path().is_dir() {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        if !manifest_path.is_file() {
            continue;
        }
        let Ok(manifest) =
            serde_json::from_reader::<_, LegacyManifest>(File::open(&manifest_path).map_err(err)?)
        else {
            continue;
        };
        if manifest.quality_status != CANONICAL_QUALITY {
            continue;
        }
        let Some((instrument, year, month)) = archive_meta.get(&manifest.source_sha256) else {
            continue;
        };
        let parquet_path = entry.path().join("quotes.parquet");
        if !parquet_path.is_file() {
            continue;
        }
        let candidate = Partition {
            instrument: instrument.clone(),
            year: *year,
            month: *month,
            dataset_hash: manifest.dataset_hash.clone(),
            parquet_path: parquet_path.to_string_lossy().to_string(),
            manifest_path: manifest_path.to_string_lossy().to_string(),
            import_receipt_path: receipt_by_source
                .get(&manifest.source_sha256)
                .map(|p| p.to_string_lossy().to_string()),
            rows: manifest.rows,
            price_scale: manifest.price_scale,
            source_sha256: manifest.source_sha256.clone(),
            quote_stream_sha256: manifest.quote_stream_sha256.clone(),
        };
        groups
            .entry((instrument.clone(), *year, *month))
            .and_modify(|existing| {
                if candidate.dataset_hash < existing.dataset_hash {
                    *existing = candidate.clone();
                }
            })
            .or_insert(candidate);
    }
    let mut partitions: Vec<Partition> = groups.into_values().collect();
    partitions.sort_by(|a, b| {
        (a.instrument.as_str(), a.year, a.month).cmp(&(b.instrument.as_str(), b.year, b.month))
    });
    Ok(partitions)
}

fn convert_partition(
    partition: &Partition,
    output_root: &Path,
    profiles_dir: &Path,
    chunk_events: usize,
) -> PartitionReport {
    let start = std::time::Instant::now();
    let output_dir = output_root.join(format!(
        "{}-{year:04}{month:02}",
        partition.instrument,
        year = partition.year,
        month = partition.month
    ));
    let mut report = PartitionReport {
        instrument: partition.instrument.clone(),
        year: partition.year,
        month: partition.month,
        dataset_hash: partition.dataset_hash.clone(),
        status: "failed".into(),
        events: 0,
        chunks: 0,
        output_hash: String::new(),
        output_dir: output_dir.to_string_lossy().to_string(),
        error: None,
        wall_ms: 0,
    };
    let already_complete = output_dir.join("manifest.json").is_file();
    let result = (|| -> Result<Receipt> {
        let profile: Profile = serde_json::from_reader(
            File::open(profiles_dir.join(format!("{}.json", partition.instrument))).map_err(err)?,
        )
        .map_err(err)?;
        let parquet = Path::new(&partition.parquet_path);
        let mut config = Config::new(parquet, &output_dir, profile)?
            .with_manifest(Path::new(&partition.manifest_path))?
            .with_chunk_events(chunk_events)?;
        if let Some(receipt) = &partition.import_receipt_path {
            config = config.with_import_receipt(Path::new(receipt))?;
        }
        convert(&config)
    })();
    report.wall_ms = start.elapsed().as_millis() as u64;
    match result {
        Ok(receipt) => {
            report.status = if already_complete {
                "already_complete".to_string()
            } else {
                "converted".to_string()
            };
            report.events = receipt.events;
            report.chunks = receipt.chunks.len() as u64;
            report.output_hash = receipt.output_hash;
        }
        Err(error) => report.error = Some(error),
    }
    report
}

/// Convert a planned batch with a bounded worker pool. Completed partitions are re-verified
/// and skipped, so the batch is safe to restart.
pub fn run_batch(
    root: &Path,
    output_root: &Path,
    profiles_dir: &Path,
    workers: usize,
    chunk_events: usize,
    limit: Option<usize>,
) -> Result<BatchReport> {
    if workers == 0 || workers > 16 {
        return Err("worker count must be 1..=16".into());
    }
    let mut partitions = plan(root)?;
    if let Some(limit) = limit {
        partitions.truncate(limit);
    }
    if partitions.is_empty() {
        return Err("no canonical partitions found".into());
    }
    fs::create_dir_all(output_root).map_err(err)?;
    let next = Arc::new(AtomicUsize::new(0));
    let results: Arc<Mutex<Vec<Option<PartitionReport>>>> =
        Arc::new(Mutex::new((0..partitions.len()).map(|_| None).collect()));
    let shared = Arc::new(partitions.clone());
    let mut handles = Vec::new();
    for _ in 0..workers {
        let next = Arc::clone(&next);
        let results = Arc::clone(&results);
        let shared = Arc::clone(&shared);
        let output_root = output_root.to_path_buf();
        let profiles_dir = profiles_dir.to_path_buf();
        handles.push(std::thread::spawn(move || {
            loop {
                let index = next.fetch_add(1, Ordering::SeqCst);
                if index >= shared.len() {
                    break;
                }
                let report =
                    convert_partition(&shared[index], &output_root, &profiles_dir, chunk_events);
                if let Ok(mut slot) = results.lock() {
                    slot[index] = Some(report);
                }
            }
        }));
    }
    for handle in handles {
        handle.join().map_err(|_| "migration worker panicked")?;
    }
    let partitions_report = results
        .lock()
        .map_err(|_| "migration result lock poisoned")?
        .iter()
        .filter_map(|value| value.clone())
        .collect::<Vec<_>>();
    let mut report = BatchReport {
        version: BATCH_VERSION.into(),
        root: root.to_string_lossy().to_string(),
        output_root: output_root.to_string_lossy().to_string(),
        workers,
        chunk_events,
        planned: partitions.len(),
        converted: 0,
        already_complete: 0,
        failed: 0,
        events: 0,
        partitions: partitions_report,
        limitations: "Format conversion only. Completion means each selected canonical partition was converted and verified; it does not establish calendar completeness, profile approval, source licensing or research eligibility.".into(),
    };
    for partition in &report.partitions {
        match partition.status.as_str() {
            "converted" => report.converted += 1,
            "already_complete" => report.already_complete += 1,
            _ => report.failed += 1,
        }
        report.events += partition.events;
    }
    publish_json_replace(&output_root.join("batch-report.json"), &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parquet::{
        basic::{Compression, Encoding, ZstdLevel},
        data_type::{ByteArray, ByteArrayType, Int64Type},
        file::{
            properties::{WriterProperties, WriterVersion},
            writer::SerializedFileWriter,
        },
        schema::parser::parse_message_type,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct LegacyRow {
        instrument: &'static str,
        ts_ns: i64,
        bid: i64,
        ask: i64,
        scale: i64,
        source: &'static str,
        batch: &'static str,
        order: i64,
        precision: &'static str,
        quality: &'static str,
    }

    fn rows(count: usize) -> Vec<LegacyRow> {
        (0..count)
            .map(|i| LegacyRow {
                instrument: "NZDUSD",
                ts_ns: 1_764_565_200_184_000_000 + i as i64 * 1_000_000,
                bid: 573_080 + (i as i64 % 5) * 10,
                ask: 573_080 + (i as i64 % 5) * 10 + 20,
                scale: 1_000_000,
                source: "HistData.com",
                batch: "test-batch",
                order: i as i64,
                precision: "millisecond",
                quality: "",
            })
            .collect()
    }

    /// Pairs of rows share a timestamp, exercising tie ordering across chunk boundaries.
    fn rows_with_ties(count: usize) -> Vec<LegacyRow> {
        (0..count)
            .map(|i| LegacyRow {
                instrument: "NZDUSD",
                ts_ns: 1_764_565_200_184_000_000 + (i as i64 / 2) * 1_000_000,
                bid: 573_080 + (i as i64 % 5) * 10,
                ask: 573_080 + (i as i64 % 5) * 10 + 20,
                scale: 1_000_000,
                source: "HistData.com",
                batch: "test-batch",
                order: i as i64,
                precision: "millisecond",
                quality: "",
            })
            .collect()
    }

    fn write_legacy(path: &Path, rows: &[LegacyRow], batch_rows: usize) -> Result<()> {
        let schema = Arc::new(parse_message_type(LEGACY_SCHEMA).map_err(err)?);
        let props = Arc::new(
            WriterProperties::builder()
                .set_writer_version(WriterVersion::PARQUET_1_0)
                .set_dictionary_enabled(false)
                .set_encoding(Encoding::PLAIN)
                .set_compression(Compression::ZSTD(ZstdLevel::try_new(1).map_err(err)?))
                .build(),
        );
        let mut writer = SerializedFileWriter::new(File::create(path).map_err(err)?, schema, props)
            .map_err(err)?;
        for block in rows.chunks(batch_rows) {
            let n = block.len();
            let def = vec![1i16; n];
            let strings = |pick: fn(&LegacyRow) -> &'static str| {
                block
                    .iter()
                    .map(|row| ByteArray::from(pick(row).as_bytes().to_vec()))
                    .collect::<Vec<_>>()
            };
            let ints = |pick: fn(&LegacyRow) -> i64| block.iter().map(pick).collect::<Vec<_>>();
            let instrument = strings(|r| r.instrument);
            let ts = ints(|r| r.ts_ns);
            let bid = ints(|r| r.bid);
            let ask = ints(|r| r.ask);
            let scale = ints(|r| r.scale);
            let source = strings(|r| r.source);
            let batch = strings(|r| r.batch);
            let order = ints(|r| r.order);
            let precision = strings(|r| r.precision);
            let quality = strings(|r| r.quality);
            let mut group = writer.next_row_group().map_err(err)?;
            for column in 0..10 {
                let mut writer = group.next_column().map_err(err)?.ok_or("missing column")?;
                let written = match column {
                    0 => writer
                        .typed::<ByteArrayType>()
                        .write_batch(&instrument, Some(&def), None),
                    1 => writer
                        .typed::<Int64Type>()
                        .write_batch(&ts, Some(&def), None),
                    2 => writer
                        .typed::<Int64Type>()
                        .write_batch(&bid, Some(&def), None),
                    3 => writer
                        .typed::<Int64Type>()
                        .write_batch(&ask, Some(&def), None),
                    4 => writer
                        .typed::<Int64Type>()
                        .write_batch(&scale, Some(&def), None),
                    5 => writer
                        .typed::<ByteArrayType>()
                        .write_batch(&source, Some(&def), None),
                    6 => writer
                        .typed::<ByteArrayType>()
                        .write_batch(&batch, Some(&def), None),
                    7 => writer
                        .typed::<Int64Type>()
                        .write_batch(&order, Some(&def), None),
                    8 => writer
                        .typed::<ByteArrayType>()
                        .write_batch(&precision, Some(&def), None),
                    _ => writer
                        .typed::<ByteArrayType>()
                        .write_batch(&quality, Some(&def), None),
                }
                .map_err(err)?;
                if written != n {
                    return Err("short legacy column write".into());
                }
                writer.close().map_err(err)?;
            }
            group.close().map_err(err)?;
        }
        writer.close().map_err(err)?;
        Ok(())
    }

    fn migration_profile(symbol: &str) -> Profile {
        let tick = if symbol == "USDJPY" || symbol == "XAUUSD" {
            1_000
        } else {
            10
        };
        Profile {
            symbol: symbol.into(),
            version: "migration-profile-v1".into(),
            base: symbol[..3].into(),
            quote: symbol[3..].into(),
            price_scale: 1_000_000,
            tick,
            pip: tick * 10,
            contract_units: if symbol == "XAUUSD" { 100 } else { 100_000 },
            min_micro_lots: 10,
            step_micro_lots: 10,
            max_micro_lots: 1_000_000,
            leverage: 100,
            min_stop_ticks: 1,
            provenance: "MIGRATION_PROFILE_V1; source fixed-point scale 1000000; tick equals observed source quantum; pip is not an approved specification; not research eligible".into(),
            research_only: true,
        }
    }

    /// Test-only quote scan that reads columns directly, bypassing the row-group cap so a
    /// correct manifest can be built for an oversized-group refusal test.
    fn scan_quote_stream_unchecked(path: &Path) -> Result<QuoteScan> {
        let reader = open_reader(path)?;
        verify_schema(reader.metadata().file_metadata())?;
        let mut hasher = Sha256::new();
        let mut meta: Option<Meta> = None;
        let mut rows = 0u64;
        let mut first_ns = 0i64;
        let mut last_ns = 0i64;
        for index in 0..reader.metadata().num_row_groups() {
            let group = reader.get_row_group(index).map_err(err)?;
            let n = group.metadata().num_rows() as usize;
            let mut budget = u64::MAX;
            let instrument = read_string_column(group.as_ref(), 0, n, &mut budget)?;
            let ts = read_int64_column(group.as_ref(), 1, n)?;
            let bid = read_int64_column(group.as_ref(), 2, n)?;
            let ask = read_int64_column(group.as_ref(), 3, n)?;
            let scale = read_int64_column(group.as_ref(), 4, n)?;
            let source = read_string_column(group.as_ref(), 5, n, &mut budget)?;
            let batch = read_string_column(group.as_ref(), 6, n, &mut budget)?;
            let order = read_int64_column(group.as_ref(), 7, n)?;
            let precision = read_string_column(group.as_ref(), 8, n, &mut budget)?;
            let _quality = read_string_column(group.as_ref(), 9, n, &mut budget)?;
            for i in 0..n {
                if let Some(current) = &meta {
                    let row = Meta {
                        instrument: instrument[i].clone(),
                        scale: scale[i],
                        source: source[i].clone(),
                        batch: batch[i].clone(),
                        precision: precision[i].clone(),
                    };
                    if !current.same(&row) {
                        return Err("legacy Parquet mixes source/instrument units".into());
                    }
                } else {
                    let row = Meta {
                        instrument: instrument[i].clone(),
                        scale: scale[i],
                        source: source[i].clone(),
                        batch: batch[i].clone(),
                        precision: precision[i].clone(),
                    };
                    hasher.update(
                        serde_json::to_vec(&StreamMetadata {
                            batch: &row.batch,
                            instrument: &row.instrument,
                            precision: &row.precision,
                            scale: row.scale,
                            source: &row.source,
                        })
                        .map_err(err)?,
                    );
                    meta = Some(row);
                    first_ns = ts[i];
                }
                hasher.update(ts[i].to_be_bytes());
                hasher.update(bid[i].to_be_bytes());
                hasher.update(ask[i].to_be_bytes());
                hasher.update(order[i].to_be_bytes());
                last_ns = ts[i];
                rows += 1;
            }
        }
        let meta = meta.ok_or("empty legacy Parquet")?;
        Ok(QuoteScan {
            hash: hex(&hasher.finalize()),
            rows,
            first_ns,
            last_ns,
            meta,
        })
    }

    fn write_manifest(path: &Path, scan: &QuoteScan, parquet_sha256: &str) -> Result<()> {
        write_manifest_identity(
            path,
            scan,
            parquet_sha256,
            "test-dataset-hash",
            "test-source-sha256",
        )
    }

    fn write_manifest_identity(
        path: &Path,
        scan: &QuoteScan,
        parquet_sha256: &str,
        dataset_hash: &str,
        source_sha256: &str,
    ) -> Result<()> {
        let value = serde_json::json!({
            "schema_version": "data/v2",
            "dataset_hash": dataset_hash,
            "source_sha256": source_sha256,
            "parquet_sha256": parquet_sha256,
            "quote_stream_sha256": scan.hash,
            "rows": scan.rows,
            "first_ns": scan.first_ns,
            "latest_verified_ns": scan.last_ns,
            "instrument": scan.meta.instrument,
            "provenance": {"license": "test", "limitations": ["test limitation"]},
            "fidelity": "SOURCE_BID_ASK",
            "price_scale": scan.meta.scale,
            "source_precision": scan.meta.precision,
            "source_id": scan.meta.source,
            "ingest_batch_id": scan.meta.batch,
            "quality_status": "STRUCTURAL_PASS_CALENDAR_REVIEW_REQUIRED",
            "batch_rows": 2,
            "parquet_row_groups": 1,
            "consecutive_exact_duplicates_retained": 0,
            "observed_price_quantum": "0.00001",
            "quantum_caveat": "test quantum caveat",
            "gap_count_over_60s": 0,
            "gap_sample": [],
            "readback_exact": true,
        });
        fs::write(path, serde_json::to_vec(&value).map_err(err)?).map_err(err)
    }

    fn temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("rsf-historical-{}-{label}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixture(profile: Profile, chunk_events: usize) -> (PathBuf, PathBuf, PathBuf, QuoteScan) {
        let dir = temp_dir("fixture");
        let parquet = dir.join("legacy.parquet");
        let manifest = dir.join("manifest.json");
        let output = dir.join("output");
        write_legacy(&parquet, &rows(20), 2).unwrap();
        let scan = scan_quote_stream(&parquet).unwrap();
        write_manifest(&manifest, &scan, &sha256_file(&parquet).unwrap()).unwrap();
        let _config = Config::new(&parquet, &output, profile)
            .unwrap()
            .with_chunk_events(chunk_events)
            .unwrap()
            .with_manifest(&manifest)
            .unwrap();
        (dir, parquet, output, scan)
    }

    #[test]
    fn converts_exact_values_and_chunks_deterministically() {
        let (_dir, parquet, output, scan) = fixture(migration_profile("NZDUSD"), 7);
        let config = Config::new(&parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_chunk_events(7)
            .unwrap()
            .with_manifest(&_dir.join("manifest.json"))
            .unwrap();
        let receipt = convert(&config).unwrap();
        assert_eq!(receipt.events, 20);
        assert_eq!(receipt.chunks.len(), 3);
        assert_eq!(receipt.quote_stream_sha256, scan.hash);
        assert_eq!(receipt.first_ms, 1_764_565_200_184);
        assert_eq!(receipt.last_ms, 1_764_565_200_184 + 19);
        assert_eq!(receipt.class, DataClass::RealStructurallyValid);
        let verified = verify_output(&output, Some(&parquet)).unwrap();
        assert_eq!(verified.output_hash, receipt.output_hash);
        let again = convert(&config).unwrap();
        assert_eq!(again.output_hash, receipt.output_hash);
        let mut total = 0;
        for chunk in &receipt.chunks {
            let dataset = native::read(&output.join(&chunk.file)).unwrap();
            assert_eq!(dataset.ticks[0].bid % 10, 0);
            total += dataset.ticks.len();
        }
        assert_eq!(total, 20);
    }

    #[test]
    fn deterministic_across_output_directories() {
        let (_dir, parquet, output, _scan) = fixture(migration_profile("NZDUSD"), 6);
        let manifest = _dir.join("manifest.json");
        let first = convert(
            &Config::new(&parquet, &output, migration_profile("NZDUSD"))
                .unwrap()
                .with_chunk_events(6)
                .unwrap()
                .with_manifest(&manifest)
                .unwrap(),
        )
        .unwrap();
        let other_output = _dir.join("output-other");
        let second = convert(
            &Config::new(&parquet, &other_output, migration_profile("NZDUSD"))
                .unwrap()
                .with_chunk_events(6)
                .unwrap()
                .with_manifest(&manifest)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(first.output_hash, second.output_hash);
        assert_eq!(first.chunks, second.chunks);
    }

    #[test]
    fn restart_from_committed_chunks_is_safe() {
        let (dir, parquet, output, _) = fixture(migration_profile("NZDUSD"), 5);
        let manifest = dir.join("manifest.json");
        let config = Config::new(&parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_chunk_events(5)
            .unwrap()
            .with_manifest(&manifest)
            .unwrap();
        let first = convert(&config).unwrap();
        fs::remove_file(output.join("manifest.json")).unwrap();
        assert!(verify_output(&output, None).is_err());
        let second = convert(&config).unwrap();
        assert_eq!(first.output_hash, second.output_hash);
        assert_eq!(first.chunks, second.chunks);
    }

    #[test]
    fn refuses_corrupted_chunk_and_changed_source() {
        let (dir, parquet, output, _) = fixture(migration_profile("NZDUSD"), 10);
        let manifest = dir.join("manifest.json");
        let config = Config::new(&parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_chunk_events(10)
            .unwrap()
            .with_manifest(&manifest)
            .unwrap();
        convert(&config).unwrap();
        let chunk_path = output.join(config_and_chunk(&output));
        let mut bytes = fs::read(&chunk_path).unwrap();
        bytes.truncate(bytes.len() / 2);
        fs::write(&chunk_path, bytes).unwrap();
        assert!(verify_output(&output, None).is_err());
        assert!(convert(&config).is_err());
    }

    fn config_and_chunk(output: &Path) -> String {
        let mut names: Vec<String> = fs::read_dir(output)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("chunk-"))
            .collect();
        names.sort();
        names.first().cloned().unwrap()
    }

    #[test]
    fn refuses_crossed_and_nonmillisecond_source_rows() {
        let dir = temp_dir("bad");
        let parquet = dir.join("legacy.parquet");
        let mut bad = rows(4);
        bad[2].ask = bad[2].bid - 10;
        write_legacy(&parquet, &bad, 2).unwrap();
        let scan = scan_quote_stream(&parquet).unwrap();
        let manifest = dir.join("manifest.json");
        write_manifest(&manifest, &scan, &sha256_file(&parquet).unwrap()).unwrap();
        let output = dir.join("output");
        let config = Config::new(&parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_manifest(&manifest)
            .unwrap();
        assert!(convert(&config).is_err());

        let parquet2 = dir.join("legacy2.parquet");
        let mut bad2 = rows(4);
        bad2[1].ts_ns += 1;
        write_legacy(&parquet2, &bad2, 2).unwrap();
        let scan2 = scan_quote_stream(&parquet2).unwrap();
        let manifest2 = dir.join("manifest2.json");
        write_manifest(&manifest2, &scan2, &sha256_file(&parquet2).unwrap()).unwrap();
        let output2 = dir.join("output2");
        let config2 = Config::new(&parquet2, &output2, migration_profile("NZDUSD"))
            .unwrap()
            .with_manifest(&manifest2)
            .unwrap();
        assert!(convert(&config2).is_err());
    }

    #[test]
    fn refuses_wrong_manifest_identity() {
        let (dir, parquet, output, scan) = fixture(migration_profile("NZDUSD"), 10);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        value["quote_stream_sha256"] = serde_json::json!("0".repeat(64));
        let manifest = dir.join("manifest-wrong.json");
        fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        let config = Config::new(&parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_manifest(&manifest)
            .unwrap();
        assert!(convert(&config).is_err());
        let _ = scan;
    }

    #[test]
    fn converts_with_profile_scale_reduction() {
        let (_dir, parquet, output, _) = fixture(Profile::synthetic("NZDUSD").unwrap(), 100);
        let config = Config::new(&parquet, &output, Profile::synthetic("NZDUSD").unwrap())
            .unwrap()
            .with_manifest(&_dir.join("manifest.json"))
            .unwrap();
        let receipt = convert(&config).unwrap();
        assert_eq!(receipt.events, 20);
        let dataset = native::read(&output.join(&receipt.chunks[0].file)).unwrap();
        assert_eq!(dataset.ticks[0].bid, 57_308);
        assert_eq!(dataset.ticks[0].ask, 57_310);
    }

    #[test]
    fn codex_review_completed_conversion_must_bind_requested_profile_and_source() {
        let (dir, parquet, output, _) = fixture(migration_profile("NZDUSD"), 7);
        let config = Config::new(&parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_chunk_events(7)
            .unwrap()
            .with_manifest(&dir.join("manifest.json"))
            .unwrap();
        convert(&config).unwrap();
        let changed = Config::new(
            &dir.join("missing-different-source.parquet"),
            &output,
            migration_profile("EURUSD"),
        )
        .unwrap();
        assert!(
            convert(&changed).is_err(),
            "completed conversion accepted a different requested profile and missing source"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn codex_review_partial_resume_must_reject_self_hashed_different_prices() {
        let (dir, parquet, output, _) = fixture(migration_profile("NZDUSD"), 7);
        let config = Config::new(&parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_chunk_events(7)
            .unwrap()
            .with_manifest(&dir.join("manifest.json"))
            .unwrap();
        let initial = convert(&config).unwrap();
        fs::remove_file(output.join("manifest.json")).unwrap();
        let path = output.join(&initial.chunks[0].file);
        let original = native::read(&path).unwrap();
        let mut ticks = original.ticks.clone();
        for tick in &mut ticks {
            tick.bid += original.profile.tick;
            tick.ask += original.profile.tick;
        }
        let mut changed = data::dataset(
            original.profile.clone(),
            ticks,
            &original.manifest.source,
            original.manifest.archive_hash.clone(),
            false,
        )
        .unwrap();
        changed.manifest.parser_version = original.manifest.parser_version.clone();
        changed.manifest.imported_at = original.manifest.imported_at.clone();
        fs::remove_file(&path).unwrap();
        native::write(&path, &changed).unwrap();
        assert!(
            convert(&config).is_err(),
            "resume accepted different native prices bound only by self-computed hashes"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reuse_rejects_different_source_bytes_at_same_output() {
        let (dir, parquet, output, _) = fixture(migration_profile("NZDUSD"), 7);
        let config = Config::new(&parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_chunk_events(7)
            .unwrap()
            .with_manifest(&dir.join("manifest.json"))
            .unwrap();
        convert(&config).unwrap();
        let other = dir.join("other.parquet");
        let mut changed = rows(20);
        for row in &mut changed {
            row.bid += 10;
            row.ask += 10;
        }
        write_legacy(&other, &changed, 2).unwrap();
        let reuse = Config::new(&other, &output, migration_profile("NZDUSD")).unwrap();
        assert!(
            convert(&reuse).is_err(),
            "completed output was reused for different source bytes"
        );
    }

    #[test]
    fn reuse_rejects_changed_manifest_identity() {
        let (dir, parquet, output, _) = fixture(migration_profile("NZDUSD"), 7);
        let config = Config::new(&parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_chunk_events(7)
            .unwrap()
            .with_manifest(&dir.join("manifest.json"))
            .unwrap();
        convert(&config).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        value["dataset_hash"] = serde_json::json!("different-dataset-hash");
        let manifest = dir.join("manifest-changed.json");
        fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        let reuse = Config::new(&parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_manifest(&manifest)
            .unwrap();
        assert!(
            convert(&reuse).is_err(),
            "completed output was reused for a different manifest identity"
        );
    }

    #[test]
    fn resume_rejects_changed_source_data_with_updated_manifest() {
        let (dir, parquet, output, _) = fixture(migration_profile("NZDUSD"), 7);
        let config = Config::new(&parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_chunk_events(7)
            .unwrap()
            .with_manifest(&dir.join("manifest.json"))
            .unwrap();
        convert(&config).unwrap();
        fs::remove_file(output.join("manifest.json")).unwrap();
        let changed_parquet = dir.join("changed.parquet");
        let mut changed = rows(20);
        for row in &mut changed {
            row.bid += 10;
            row.ask += 10;
        }
        write_legacy(&changed_parquet, &changed, 2).unwrap();
        let scan = scan_quote_stream(&changed_parquet).unwrap();
        let manifest = dir.join("manifest-changed-source.json");
        write_manifest(&manifest, &scan, &sha256_file(&changed_parquet).unwrap()).unwrap();
        let resume = Config::new(&changed_parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_chunk_events(7)
            .unwrap()
            .with_manifest(&manifest)
            .unwrap();
        assert!(
            convert(&resume).is_err(),
            "resume accepted adopted chunks that differ from the changed source"
        );
    }

    #[test]
    fn refuses_oversized_row_group() {
        let dir = temp_dir("biggroup");
        let parquet = dir.join("legacy.parquet");
        let count = MAX_ROW_GROUP_ROWS + 1;
        write_legacy(&parquet, &rows(count), count).unwrap();
        // Build a correct manifest without the row-group cap so the refusal is specifically
        // the decoded-row preflight, not a later quote-stream mismatch.
        let scan = scan_quote_stream_unchecked(&parquet).unwrap();
        let manifest = dir.join("manifest.json");
        write_manifest(&manifest, &scan, &sha256_file(&parquet).unwrap()).unwrap();
        let output = dir.join("output");
        let config = Config::new(&parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_manifest(&manifest)
            .unwrap();
        let error = convert(&config).unwrap_err();
        assert!(
            error.contains("decoded-row budget"),
            "oversized untrusted row group was not refused by the row budget: {error}"
        );
    }

    #[test]
    fn refuses_string_column_beyond_decoded_byte_budget() {
        let dir = temp_dir("bigstrings");
        let parquet = dir.join("legacy.parquet");
        let mut rows = rows(MAX_ROW_GROUP_ROWS);
        let big: &'static str = Box::leak("x".repeat(5_000).into_boxed_str());
        for row in &mut rows {
            row.source = big;
        }
        write_legacy(&parquet, &rows, MAX_ROW_GROUP_ROWS).unwrap();
        let scan = scan_quote_stream_unchecked(&parquet).unwrap();
        let manifest = dir.join("manifest.json");
        write_manifest(&manifest, &scan, &sha256_file(&parquet).unwrap()).unwrap();
        let output = dir.join("output");
        let config = Config::new(&parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_manifest(&manifest)
            .unwrap();
        let error = convert(&config).unwrap_err();
        assert!(
            error.contains("decoded string byte budget"),
            "oversized decoded string column was not refused: {error}"
        );
    }

    #[test]
    fn migrated_chunks_are_admissible_to_native_corpus() {
        let (dir, parquet, output, _) = fixture(migration_profile("NZDUSD"), 7);
        let config = Config::new(&parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_chunk_events(7)
            .unwrap()
            .with_manifest(&dir.join("manifest.json"))
            .unwrap();
        let receipt = convert(&config).unwrap();
        assert!(receipt.chunks.len() > 1);
        let paths: Vec<PathBuf> = receipt
            .chunks
            .iter()
            .map(|c| output.join(&c.file))
            .collect();
        let spec = crate::corpus::describe(&paths, 100_000).unwrap();
        assert_eq!(spec.chunks.len(), receipt.chunks.len());
        let mut visited = 0usize;
        crate::corpus::visit_chunks(&output, &spec, |_index, dataset| {
            visited += dataset.ticks.len();
            Ok(())
        })
        .unwrap();
        assert_eq!(visited, 20);
    }

    #[test]
    fn converted_timestamp_ties_are_preserved_and_admissible() {
        let dir = temp_dir("ties");
        let parquet = dir.join("legacy.parquet");
        let manifest = dir.join("manifest.json");
        write_legacy(&parquet, &rows_with_ties(20), 2).unwrap();
        let scan = scan_quote_stream(&parquet).unwrap();
        write_manifest(&manifest, &scan, &sha256_file(&parquet).unwrap()).unwrap();
        let output = dir.join("output");
        let config = Config::new(&parquet, &output, migration_profile("NZDUSD"))
            .unwrap()
            .with_chunk_events(7)
            .unwrap()
            .with_manifest(&manifest)
            .unwrap();
        let receipt = convert(&config).unwrap();
        assert_eq!(receipt.timestamp_ties, 10);
        let paths: Vec<PathBuf> = receipt
            .chunks
            .iter()
            .map(|c| output.join(&c.file))
            .collect();
        let spec = crate::corpus::describe(&paths, 100_000).unwrap();
        crate::corpus::visit_chunks(&output, &spec, |_, _| Ok(())).unwrap();
    }

    #[test]
    fn distinct_months_remain_distinct_single_source_corpora() {
        let dir = temp_dir("months");
        let mut corpora = Vec::new();
        for name in ["month-a", "month-b"] {
            let parquet = dir.join(format!("{name}.parquet"));
            let manifest = dir.join(format!("{name}-manifest.json"));
            write_legacy(&parquet, &rows(20), 2).unwrap();
            let scan = scan_quote_stream(&parquet).unwrap();
            write_manifest_identity(
                &manifest,
                &scan,
                &sha256_file(&parquet).unwrap(),
                name,
                name,
            )
            .unwrap();
            let output = dir.join(name);
            let config = Config::new(&parquet, &output, migration_profile("NZDUSD"))
                .unwrap()
                .with_chunk_events(7)
                .unwrap()
                .with_manifest(&manifest)
                .unwrap();
            let receipt = convert(&config).unwrap();
            let paths: Vec<PathBuf> = receipt
                .chunks
                .iter()
                .map(|c| output.join(&c.file))
                .collect();
            let spec = crate::corpus::describe(&paths, 100_000).unwrap();
            corpora.push((output, spec));
        }
        for (output, spec) in &corpora {
            crate::corpus::visit_chunks(output, spec, |_, _| Ok(())).unwrap();
        }
        let mut all_paths: Vec<PathBuf> = Vec::new();
        for (output, spec) in &corpora {
            all_paths.extend(spec.chunks.iter().map(|c| output.join(&c.path)));
        }
        let combined = crate::corpus::describe(&all_paths, 100_000).unwrap();
        assert!(
            crate::corpus::visit_chunks(&dir, &combined, |_, _| Ok(())).is_err(),
            "distinct monthly sources must not be merged into one unverified corpus"
        );
    }
}
