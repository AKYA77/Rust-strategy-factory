//! Full-history streaming over the verified native corpus: resumable source reads,
//! resumable import/coverage, and checkpoint-aware incremental decode.
//!
//! This module never acquires data, never copies chunk data, and never infers
//! source completeness, calendar approval, profile approval or research eligibility.
//! Every consumed artifact is re-verified on the same descriptor before its bytes
//! are decoded, and every checkpoint binds the exact source identity it advanced
//! over so a stale or tampered prefix cannot be resumed.
//!
//! Checkpoints are hash-chained, immutable and append-only: each checkpoint records
//! the source spec identity, the consumed chunk range, the event count and the
//! final tick timestamp/sequence of the consumed prefix. Resume refuses unless the
//! current artifacts still match the checkpoint's recorded bindings.

use crate::{
    Result,
    candles::{self, Timeframe},
    contracts::Profile,
    data::{self, Tick},
    native,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const VERSION: &str = "full-history-streaming-v1";
const CHECKPOINT_CAP: usize = 4 * 1024 * 1024;
/// Maximum events decoded per streaming call; keeps each step bounded and resumable.
pub const MAX_STREAM_EVENTS: u64 = 1_000_000;
const STREAM_LIMITATIONS: &str = "Observed ordered native chunks only. Resumable decode binds the exact consumed source identity and advances one verified chunk at a time; it establishes no calendar completeness, profile approval, source licensing, scientific eligibility or research qualification. Resume re-verifies the consumed prefix before advancing and refuses any binding mismatch.";

/// Content-addressed binding of the source artifacts a stream consumes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StreamSpec {
    pub version: String,
    /// Self-identity over every binding below; recomputed and checked on every verify.
    pub identity: String,
    pub instrument: String,
    pub profile_hash: String,
    pub chunks: Vec<StreamChunk>,
    /// Exact ordered-event fingerprint over every chunk in `chunks`, computed in order.
    pub ordered_events_hash: String,
    pub events: u64,
    pub first_ms: i64,
    pub last_ms: i64,
    pub limitations: String,
}

/// One verified chunk within a stream spec.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StreamChunk {
    pub path: String,
    pub dataset_identity: String,
    pub file_hash: String,
    pub events: u64,
    pub first_ms: i64,
    pub last_ms: i64,
}

/// Durable checkpoint bound to one consumed prefix. Immutable once stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StreamCheckpoint {
    pub version: String,
    pub spec_identity: String,
    /// Number of chunks fully consumed (chunks[..consumed_chunks] form the prefix).
    pub consumed_chunks: usize,
    /// Total events consumed in the prefix.
    pub consumed_events: u64,
    /// Timestamp of the last consumed tick (prefix end boundary).
    pub last_timestamp_ms: i64,
    /// Sequence of the last consumed tick.
    pub last_sequence: u64,
    /// Hash of the previous checkpoint in the chain (empty for the first).
    pub previous: String,
    /// Self-hash over this checkpoint's bindings.
    pub self_hash: String,
    pub limitations: String,
}

impl StreamCheckpoint {
    /// Hash over the checkpoint's semantic bindings with `self_hash` blanked — exactly
    /// the value written into `self_hash` when the checkpoint was first created.
    fn hash_binding(&self) -> Result<String> {
        let mut bound = self.clone();
        bound.self_hash = String::new();
        data::hash(&bound)
    }
}

/// Observable progress of a streaming run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamProgress {
    pub version: String,
    pub spec_identity: String,
    pub instrument: String,
    pub consumed_chunks: usize,
    pub total_chunks: usize,
    pub consumed_events: u64,
    pub total_events: u64,
    pub complete: bool,
    pub last_timestamp_ms: i64,
    pub last_sequence: u64,
    pub limitations: String,
}

/// Aggregate decode report for one bounded streaming pass over a spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamReport {
    pub version: String,
    pub spec_identity: String,
    pub instrument: String,
    pub chunks: usize,
    pub events: u64,
    pub candles: u64,
    pub first_ms: i64,
    pub last_ms: i64,
    pub limitations: String,
}

/// Resolve an artifact path relative to `base` when it is not absolute.
fn resolve_path(base: &Path, path: impl AsRef<Path>) -> Result<PathBuf> {
    let canonical = path.as_ref();
    if canonical.is_absolute() {
        Ok(canonical.to_path_buf())
    } else {
        Ok(base.join(canonical))
    }
}

/// Verify current bytes of an artifact match the recorded fingerprint on one descriptor.
fn verify_current_bytes(path: &Path, expected_hash: &str) -> Result<()> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    let cap = MAX_STREAM_EVENTS * 32 + 52 + 1024 * 1024;
    if length == 0 || length > cap {
        return Err("artifact byte budget exceeded".into());
    }
    let mut reader = file.take(length + 1);
    let mut hash = blake3::Hasher::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let actual = hash.finalize().to_hex().to_string();
    if actual != expected_hash {
        return Err("artifact revision/fingerprint checksum mismatch".into());
    }
    Ok(())
}

/// Build a `StreamSpec` by verifying every artifact in `paths` exactly once, in order.
/// Does not decode quotes; binds consumed-byte fingerprints and source identities.
pub fn describe(base: &Path, paths: &[PathBuf]) -> Result<StreamSpec> {
    if paths.is_empty() || paths.len() > 10_000 {
        return Err("stream requires one to ten thousand artifacts".into());
    }
    let mut chunks = Vec::with_capacity(paths.len());
    let mut events = SequenceHash::new();
    let mut profile: Option<Profile> = None;
    let mut first_ms: Option<i64> = None;
    let mut last_ms: Option<i64> = None;
    let mut total_events = 0u64;
    let mut previous: Option<Tick> = None;
    for path in paths {
        let canonical = resolve_path(base, path)?;
        let (dataset, fingerprint) = native::read_bounded(&canonical, MAX_STREAM_EVENTS as usize)?;
        if dataset.ticks.is_empty() || dataset.ticks.len() as u64 > MAX_STREAM_EVENTS {
            return Err("stream artifact event budget exceeded".into());
        }
        if profile.is_none() {
            profile = Some(dataset.profile.clone());
        } else if profile.as_ref() != Some(&dataset.profile) {
            return Err("stream artifacts do not share one profile".into());
        }
        if dataset.ticks[0].timestamp_ms > dataset.ticks[dataset.ticks.len() - 1].timestamp_ms {
            return Err("stream artifact timestamps out of order".into());
        }
        if let Some(prev) = previous {
            let first = dataset.ticks[0];
            if first.timestamp_ms < prev.timestamp_ms || first.sequence <= prev.sequence {
                return Err("stream cross-artifact timestamp/sequence order invalid".into());
            }
        }
        previous = Some(dataset.ticks[dataset.ticks.len() - 1]);
        for tick in &dataset.ticks {
            events.push(tick)?;
        }
        total_events = total_events
            .checked_add(dataset.ticks.len() as u64)
            .ok_or("event overflow")?;
        first_ms = first_ms.or(Some(dataset.ticks[0].timestamp_ms));
        last_ms = Some(dataset.ticks[dataset.ticks.len() - 1].timestamp_ms);
        // Exact consumed-bytes verification on the same descriptor used for decoding.
        verify_current_bytes(&canonical, &fingerprint)?;
        chunks.push(StreamChunk {
            path: canonical
                .to_str()
                .ok_or("non-UTF8 artifact path")?
                .to_string(),
            dataset_identity: data::dataset_identity(&dataset.manifest)?,
            file_hash: fingerprint,
            events: dataset.ticks.len() as u64,
            first_ms: dataset.ticks[0].timestamp_ms,
            last_ms: dataset.ticks[dataset.ticks.len() - 1].timestamp_ms,
        });
    }
    let profile = profile.ok_or("empty stream")?;
    let mut spec = StreamSpec {
        version: VERSION.into(),
        identity: String::new(),
        instrument: profile.symbol.clone(),
        profile_hash: data::hash(&profile)?,
        chunks,
        ordered_events_hash: events.finish(),
        events: total_events,
        first_ms: first_ms.ok_or("empty stream")?,
        last_ms: last_ms.ok_or("empty stream")?,
        limitations: STREAM_LIMITATIONS.into(),
    };
    spec.identity = identity(&spec)?;
    Ok(spec)
}

/// Identity of a spec: hash over every binding, never tolerant of moved or retitled chunks.
pub fn identity(spec: &StreamSpec) -> Result<String> {
    if spec.version != VERSION || spec.chunks.is_empty() || spec.chunks.len() > 10_000 {
        return Err("invalid stream spec".into());
    }
    data::hash(&(
        VERSION,
        &spec.instrument,
        &spec.profile_hash,
        &spec.chunks,
        spec.ordered_events_hash.clone(),
        spec.events,
        spec.first_ms,
        spec.last_ms,
    ))
}

/// Re-verify an entire spec against current artifacts; refuses any mismatch.
pub fn verify(base: &Path, spec: &StreamSpec) -> Result<()> {
    if identity(spec)? != spec.identity {
        return Err("stream spec identity mismatch".into());
    }
    let mut previous: Option<Tick> = None;
    for chunk in &spec.chunks {
        let canonical = resolve_path(base, &chunk.path)?;
        let (dataset, fingerprint) = native::read_bounded(&canonical, MAX_STREAM_EVENTS as usize)?;
        if fingerprint != chunk.file_hash {
            return Err("stream artifact revision/fingerprint mismatch".into());
        }
        if dataset.ticks.len() as u64 != chunk.events {
            return Err("stream artifact event count mismatch".into());
        }
        let first_ms = dataset.ticks[0].timestamp_ms;
        let first_sequence = dataset.ticks[0].sequence;
        if let Some(prev) = previous
            && (first_ms < prev.timestamp_ms || first_sequence <= prev.sequence)
        {
            return Err("stream cross-artifact order mismatch on resume".into());
        }
        previous = Some(dataset.ticks[dataset.ticks.len() - 1]);
    }
    Ok(())
}

/// Decode a bounded incremental slice of the stream starting at `from_chunk`, re-verifying
/// every artifact in the consumed prefix before advancing. Returns how far the stream
/// progressed and whether it is complete.
pub fn decode(
    base: &Path,
    spec: &StreamSpec,
    from_chunk: usize,
    max_events: u64,
) -> Result<(Vec<Tick>, usize, u64, bool)> {
    if from_chunk > spec.chunks.len() {
        return Err("stream resume chunk outside specification".into());
    }
    if max_events == 0 || max_events > MAX_STREAM_EVENTS {
        return Err("invalid stream event budget".into());
    }
    // Re-verify the consumed prefix before consuming anything new.
    verify_prefix(base, spec, from_chunk)?;
    let mut decoded = Vec::new();
    let mut consumed_chunks = from_chunk;
    let mut consumed_events = 0u64;
    let mut previous: Option<Tick> = None;
    for (index, chunk) in spec.chunks.iter().enumerate().skip(from_chunk) {
        let canonical = resolve_path(base, &chunk.path)?;
        let (dataset, fingerprint) = native::read_bounded(&canonical, MAX_STREAM_EVENTS as usize)?;
        if fingerprint != chunk.file_hash {
            return Err("stream artifact revision/fingerprint mismatch during decode".into());
        }
        if dataset.ticks.len() as u64 != chunk.events {
            return Err("stream artifact event count mismatch during decode".into());
        }
        if let Some(prev) = previous {
            let first = dataset.ticks[0];
            if first.timestamp_ms < prev.timestamp_ms || first.sequence <= prev.sequence {
                return Err("stream cross-artifact order mismatch during decode".into());
            }
        }
        for tick in &dataset.ticks {
            if consumed_events >= max_events {
                return Ok((decoded, consumed_chunks, consumed_events, false));
            }
            decoded.push(*tick);
            previous = Some(*tick);
            consumed_events = consumed_events.checked_add(1).ok_or("event overflow")?;
        }
        consumed_chunks = index + 1;
    }
    let complete = consumed_chunks == spec.chunks.len();
    Ok((decoded, consumed_chunks, consumed_events, complete))
}

/// Re-verify the consumed prefix of `spec` (first `consumed_chunks` chunks).
fn verify_prefix(base: &Path, spec: &StreamSpec, consumed_chunks: usize) -> Result<()> {
    let mut previous: Option<Tick> = None;
    for chunk in spec.chunks.iter().take(consumed_chunks) {
        let canonical = resolve_path(base, &chunk.path)?;
        let (dataset, fingerprint) = native::read_bounded(&canonical, MAX_STREAM_EVENTS as usize)?;
        if fingerprint != chunk.file_hash {
            return Err("stream consumed prefix artifact mismatch: resume refused".into());
        }
        if dataset.ticks.len() as u64 != chunk.events {
            return Err("stream consumed prefix event count mismatch: resume refused".into());
        }
        if let Some(prev) = previous {
            let first = dataset.ticks[0];
            if first.timestamp_ms < prev.timestamp_ms || first.sequence <= prev.sequence {
                return Err("stream consumed prefix order mismatch: resume refused".into());
            }
        }
        previous = Some(dataset.ticks[dataset.ticks.len() - 1]);
    }
    Ok(())
}

/// Advance a checkpoint by consuming one verified chunk.
fn advance_checkpoint(
    previous: &StreamCheckpoint,
    spec: &StreamSpec,
    chunk_index: usize,
    chunk_events: u64,
    last_tick: Tick,
) -> Result<StreamCheckpoint> {
    if chunk_index >= spec.chunks.len() {
        return Err("stream checkpoint advance outside spec".into());
    }
    let checkpoint = StreamCheckpoint {
        version: VERSION.into(),
        spec_identity: previous.spec_identity.clone(),
        consumed_chunks: previous.consumed_chunks + 1,
        consumed_events: previous
            .consumed_events
            .checked_add(chunk_events)
            .ok_or("stream checkpoint event overflow")?,
        last_timestamp_ms: last_tick.timestamp_ms,
        last_sequence: last_tick.sequence,
        previous: previous.self_hash.clone(),
        self_hash: String::new(),
        limitations: STREAM_LIMITATIONS.into(),
    };
    let self_hash = data::hash(&checkpoint)?;
    Ok(StreamCheckpoint {
        self_hash,
        ..checkpoint
    })
}

/// Append-only hash-chained checkpoint store. Each stored checkpoint is immutable and
/// self-authenticating; resume loads the latest and refuses binding mismatches.
#[derive(Debug, Clone)]
pub struct CheckpointStore {
    dir: PathBuf,
}

impl CheckpointStore {
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    /// Store a checkpoint atomically without replacing an existing file of the same name.
    pub fn store(&self, checkpoint: &StreamCheckpoint) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(checkpoint).map_err(|e| e.to_string())?;
        if bytes.len() > CHECKPOINT_CAP {
            return Err("stream checkpoint exceeds budget".into());
        }
        let name = format!("{}.json", checkpoint.self_hash);
        let final_path = self.dir.join(&name);
        // Idempotent: an identical checkpoint is already durable; never replace a
        // content-addressed file. A different body at the same name is a collision.
        if let Ok(existing) = fs::read(&final_path) {
            if existing == bytes {
                return Ok(());
            }
            return Err("stream checkpoint hash collision: existing content differs".into());
        }
        let temp_path = self.dir.join(format!(
            ".{}.{}.tmp",
            checkpoint.self_hash,
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|e| e.to_string())?;
        let written = file.write_all(&bytes).and_then(|_| file.sync_all());
        drop(file);
        if let Err(e) = written {
            let _ = fs::remove_file(&temp_path);
            return Err(e.to_string());
        }
        fs::hard_link(&temp_path, &final_path).map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&temp_path);
        fs::File::open(&self.dir)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Load the latest checkpoint by modification order, or None if none exists.
    pub fn latest(&self) -> Result<Option<StreamCheckpoint>> {
        let mut entries: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
        for entry in fs::read_dir(&self.dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let metadata = entry.metadata().map_err(|e| e.to_string())?;
            if metadata.is_file() {
                entries.push((
                    entry.path(),
                    metadata.modified().map_err(|e| e.to_string())?,
                ));
            }
        }
        entries.sort_by_key(|(_, m)| *m);
        for (path, _) in entries.into_iter().rev() {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.ends_with(".json") || name.starts_with('.') {
                continue;
            }
            let bytes = fs::read(&path).map_err(|e| e.to_string())?;
            let checkpoint: StreamCheckpoint =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if checkpoint.hash_binding()? != checkpoint.self_hash {
                continue; // corrupt entry: skip, never trust
            }
            return Ok(Some(checkpoint));
        }
        Ok(None)
    }

    /// Load a checkpoint by its self-hash name.
    pub fn load(&self, self_hash: &str) -> Result<Option<StreamCheckpoint>> {
        let path = self.dir.join(format!("{self_hash}.json"));
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        let checkpoint: StreamCheckpoint =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if checkpoint.hash_binding()? != checkpoint.self_hash {
            return Err("stream checkpoint self-hash mismatch".into());
        }
        Ok(Some(checkpoint))
    }
}

/// Build the initial checkpoint for a spec at zero consumption.
fn initial_checkpoint(spec: &StreamSpec) -> Result<StreamCheckpoint> {
    let identity = identity(spec)?;
    let checkpoint = StreamCheckpoint {
        version: VERSION.into(),
        spec_identity: identity,
        consumed_chunks: 0,
        consumed_events: 0,
        last_timestamp_ms: 0,
        last_sequence: 0,
        previous: String::new(),
        self_hash: String::new(),
        limitations: STREAM_LIMITATIONS.into(),
    };
    let self_hash = data::hash(&checkpoint)?;
    Ok(StreamCheckpoint {
        self_hash,
        ..checkpoint
    })
}

/// Resumable streaming driver: decodes one bounded step per call, persists a checkpoint
/// after each consumed chunk, and resumes by re-verifying the consumed prefix.
#[derive(Debug)]
pub struct StreamDriver {
    base: PathBuf,
    spec: StreamSpec,
    store: CheckpointStore,
    checkpoint: StreamCheckpoint,
}

impl StreamDriver {
    /// Open or create a driver for `spec`. If a checkpoint exists for this spec identity
    /// it is loaded and its bindings re-verified; otherwise a fresh checkpoint at zero is
    /// created and stored.
    pub fn open(base: &Path, spec: &StreamSpec, store: &CheckpointStore) -> Result<Self> {
        let identity = identity(spec)?;
        let stored = match store.latest()? {
            Some(c) if c.spec_identity == identity => Some(c),
            _ => None,
        };
        let checkpoint = match stored {
            Some(c) => c,
            None => initial_checkpoint(spec)?,
        };
        // Re-verify the consumed prefix against current artifacts before allowing any advance.
        verify_prefix(base, spec, checkpoint.consumed_chunks)?;
        store.store(&checkpoint)?;
        Ok(Self {
            base: base.to_path_buf(),
            spec: spec.clone(),
            store: store.clone(),
            checkpoint,
        })
    }

    /// Progress report without consuming anything.
    pub fn progress(&self) -> StreamProgress {
        StreamProgress {
            version: VERSION.into(),
            spec_identity: self.checkpoint.spec_identity.clone(),
            instrument: self.spec.instrument.clone(),
            consumed_chunks: self.checkpoint.consumed_chunks,
            total_chunks: self.spec.chunks.len(),
            consumed_events: self.checkpoint.consumed_events,
            total_events: self.spec.events,
            complete: self.checkpoint.consumed_chunks == self.spec.chunks.len(),
            last_timestamp_ms: self.checkpoint.last_timestamp_ms,
            last_sequence: self.checkpoint.last_sequence,
            limitations: STREAM_LIMITATIONS.into(),
        }
    }

    /// Decode one bounded step: consume up to `max_events` more events, advancing and
    /// persisting a checkpoint after each fully consumed chunk. A partial chunk is not
    /// checkpointed until it is fully consumed, so resume re-decodes it from the last
    /// committed boundary.
    pub fn step(&mut self, max_events: u64) -> Result<(Vec<Tick>, bool)> {
        if self.checkpoint.consumed_chunks == self.spec.chunks.len() {
            return Ok((Vec::new(), true));
        }
        if max_events == 0 || max_events > MAX_STREAM_EVENTS {
            return Err("invalid stream event budget".into());
        }
        let mut decoded = Vec::new();
        let mut cursor = self.checkpoint.consumed_chunks;
        let mut budget = max_events;
        let mut checkpoint = self.checkpoint.clone();
        // Last tick of the committed prefix, for cross-chunk order validation.
        let mut previous: Option<Tick> = if cursor == 0 {
            None
        } else {
            Some(Tick {
                timestamp_ms: self.checkpoint.last_timestamp_ms,
                sequence: self.checkpoint.last_sequence,
                bid: 0,
                ask: 0,
            })
        };
        while cursor < self.spec.chunks.len() && budget > 0 {
            let chunk = &self.spec.chunks[cursor];
            let canonical = resolve_path(&self.base, &chunk.path)?;
            let (dataset, fingerprint) =
                native::read_bounded(&canonical, MAX_STREAM_EVENTS as usize)?;
            if fingerprint != chunk.file_hash {
                return Err("stream artifact revision/fingerprint mismatch during decode".into());
            }
            if dataset.ticks.len() as u64 != chunk.events {
                return Err("stream artifact event count mismatch during decode".into());
            }
            if let Some(prev) = previous {
                let first = dataset.ticks[0];
                if first.timestamp_ms < prev.timestamp_ms || first.sequence <= prev.sequence {
                    return Err("stream cross-artifact order mismatch during decode".into());
                }
            }
            let take = budget.min(dataset.ticks.len() as u64);
            for tick in &dataset.ticks[..take as usize] {
                decoded.push(*tick);
                previous = Some(*tick);
            }
            budget = budget.checked_sub(take).ok_or("budget underflow")?;
            if take == dataset.ticks.len() as u64 {
                // Full chunk consumed: checkpoint the completed boundary.
                let last_tick = dataset.ticks[dataset.ticks.len() - 1];
                checkpoint =
                    advance_checkpoint(&checkpoint, &self.spec, cursor, chunk.events, last_tick)?;
                self.store.store(&checkpoint)?;
                cursor += 1;
            } else {
                // Budget exhausted mid-chunk: stop without checkpointing a partial chunk.
                break;
            }
        }
        self.checkpoint = checkpoint;
        let complete = self.checkpoint.consumed_chunks == self.spec.chunks.len();
        Ok((decoded, complete))
    }
}

/// Stream a full spec through the causal M1 candle builder, one bounded pass, reporting
/// exact observed candle/event counts. Re-verifies every artifact before decoding.
pub fn stream_report(base: &Path, spec: &StreamSpec) -> Result<StreamReport> {
    verify(base, spec)?;
    let mut stream = None;
    let mut candles = 0u64;
    let mut events = 0u64;
    let mut first_ms: Option<i64> = None;
    let mut last_ms: Option<i64> = None;
    for chunk in &spec.chunks {
        let canonical = resolve_path(base, &chunk.path)?;
        let (dataset, fingerprint) = native::read_bounded(&canonical, MAX_STREAM_EVENTS as usize)?;
        if fingerprint != chunk.file_hash {
            return Err("stream report artifact fingerprint mismatch".into());
        }
        if dataset.ticks.len() as u64 != chunk.events {
            return Err("stream report artifact event count mismatch".into());
        }
        if stream.is_none() {
            stream = Some(candles::Stream::new(
                dataset.profile.clone(),
                Timeframe::M1,
            )?);
        }
        let stream = stream.as_mut().ok_or("candle stream unavailable")?;
        for tick in &dataset.ticks {
            if stream.push(*tick)?.is_some() {
                candles = candles.checked_add(1).ok_or("candle overflow")?;
            }
            events = events.checked_add(1).ok_or("event overflow")?;
            first_ms = first_ms.or(Some(tick.timestamp_ms));
            last_ms = Some(tick.timestamp_ms);
        }
    }
    Ok(StreamReport {
        version: VERSION.into(),
        spec_identity: identity(spec)?,
        instrument: spec.instrument.clone(),
        chunks: spec.chunks.len(),
        events,
        candles,
        first_ms: first_ms.ok_or("empty stream")?,
        last_ms: last_ms.ok_or("empty stream")?,
        limitations: STREAM_LIMITATIONS.into(),
    })
}

/// Resumable coverage over a stream spec: per-chunk observed first/latest event and byte
/// coverage, bounded and read-only. Re-verifies every artifact before reporting.
pub fn stream_coverage(base: &Path, spec: &StreamSpec) -> Result<StreamCoverageReport> {
    verify(base, spec)?;
    let mut per_chunk = Vec::with_capacity(spec.chunks.len());
    let mut events = 0u64;
    let mut first_ms: Option<i64> = None;
    let mut last_ms: Option<i64> = None;
    let mut previous_last: Option<i64> = None;
    for chunk in &spec.chunks {
        let canonical = resolve_path(base, &chunk.path)?;
        let (dataset, fingerprint) = native::read_bounded(&canonical, MAX_STREAM_EVENTS as usize)?;
        if fingerprint != chunk.file_hash {
            return Err("stream coverage artifact fingerprint mismatch".into());
        }
        let bytes = fs::metadata(&canonical).map_err(|e| e.to_string())?.len();
        let chunk_first = dataset.ticks[0].timestamp_ms;
        let chunk_last = dataset.ticks[dataset.ticks.len() - 1].timestamp_ms;
        if let Some(prev) = previous_last
            && chunk_first < prev
        {
            return Err("stream coverage cross-chunk order mismatch".into());
        }
        previous_last = Some(chunk_last);
        events = events.checked_add(chunk.events).ok_or("event overflow")?;
        first_ms = first_ms.or(Some(chunk_first));
        last_ms = Some(chunk_last);
        per_chunk.push(ChunkCoverage {
            path: chunk.path.clone(),
            events: chunk.events,
            first_ms: chunk_first,
            last_ms: chunk_last,
            bytes,
        });
    }
    Ok(StreamCoverageReport {
        version: VERSION.into(),
        spec_identity: identity(spec)?,
        instrument: spec.instrument.clone(),
        chunks: spec.chunks.len(),
        events,
        first_ms: first_ms.ok_or("empty stream")?,
        last_ms: last_ms.ok_or("empty stream")?,
        per_chunk,
        limitations: STREAM_LIMITATIONS.into(),
    })
}

/// Per-chunk coverage observation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkCoverage {
    pub path: String,
    pub events: u64,
    pub first_ms: i64,
    pub last_ms: i64,
    pub bytes: u64,
}

/// Resumable coverage report over a stream spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamCoverageReport {
    pub version: String,
    pub spec_identity: String,
    pub instrument: String,
    pub chunks: usize,
    pub events: u64,
    pub first_ms: i64,
    pub last_ms: i64,
    pub per_chunk: Vec<ChunkCoverage>,
    pub limitations: String,
}

/// Streaming hash of an ordered tick sequence without retaining the ticks.
struct SequenceHash {
    hash: blake3::Hasher,
    count: u64,
}

impl SequenceHash {
    fn new() -> Self {
        let mut hash = blake3::Hasher::new();
        hash.update(b"[");
        Self { hash, count: 0 }
    }
    fn push<T: serde::Serialize>(&mut self, value: &T) -> Result<()> {
        if self.count > 0 {
            self.hash.update(b",");
        }
        serde_json::to_writer(HashWriter(&mut self.hash), value).map_err(|e| e.to_string())?;
        self.count = self.count.checked_add(1).ok_or("stream count overflow")?;
        Ok(())
    }
    fn finish(mut self) -> String {
        self.hash.update(b"]");
        self.hash.finalize().to_hex().to_string()
    }
}

struct HashWriter<'a>(&'a mut blake3::Hasher);
impl Write for HashWriter<'_> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.update(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    fn temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rsf-stream-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            n
        ))
    }

    fn write_dataset(
        dir: &Path,
        name: &str,
        ticks: &[Tick],
        profile: &Profile,
        source: &str,
    ) -> Result<PathBuf> {
        let archive_hash = data::hash(&ticks.to_vec())?;
        let dataset = data::dataset(profile.clone(), ticks.to_vec(), source, archive_hash, false)?;
        let path = dir.join(name);
        native::write(&path, &dataset)?;
        Ok(path)
    }

    fn profile() -> Profile {
        Profile::synthetic("EURUSD").unwrap()
    }

    fn chunk_ticks(
        start_ms: i64,
        start_seq: u64,
        count: usize,
        bid_base: i64,
        p: &Profile,
    ) -> Vec<Tick> {
        (0..count)
            .map(|i| Tick {
                timestamp_ms: start_ms + (i as i64) * 60_000,
                sequence: start_seq + i as u64,
                bid: bid_base + (i as i64) * p.pip,
                ask: bid_base + (i as i64) * p.pip + p.pip,
            })
            .collect()
    }

    #[test]
    fn describe_binds_fingerprints_and_identity_is_deterministic() {
        let dir = temp_dir();
        fs::create_dir(&dir).unwrap();
        let p = profile();
        let ticks1 = chunk_ticks(1_735_689_600_000, 0, 3, 1_000_000, &p);
        let ticks2 = chunk_ticks(1_735_691_400_000, 3, 3, 1_000_010, &p);
        let path1 = write_dataset(&dir, "chunk-000000.rsf", &ticks1, &p, "source-v1").unwrap();
        let path2 = write_dataset(&dir, "chunk-000001.rsf", &ticks2, &p, "source-v1").unwrap();
        let spec = describe(&dir, &[path1, path2]).unwrap();
        assert_eq!(spec.instrument, "EURUSD");
        assert_eq!(spec.events, 6);
        assert_eq!(spec.chunks.len(), 2);
        let first = identity(&spec).unwrap();
        let second = identity(&spec).unwrap();
        assert_eq!(first, second);
        // A changed artifact changes the identity.
        let mut bad = spec.clone();
        bad.chunks[0].file_hash = "0".repeat(64);
        assert_ne!(first, identity(&bad).unwrap());
    }

    #[test]
    fn verify_refuses_tampered_artifact() {
        let dir = temp_dir();
        fs::create_dir(&dir).unwrap();
        let p = profile();
        let ticks = chunk_ticks(1_735_689_600_000, 0, 3, 1_000_000, &p);
        let path = write_dataset(&dir, "chunk-000000.rsf", &ticks, &p, "source-v1").unwrap();
        let spec = describe(&dir, std::slice::from_ref(&path)).unwrap();
        assert!(verify(&dir, &spec).is_ok());
        // Flip a byte in the artifact.
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&path, bytes).unwrap();
        assert!(verify(&dir, &spec).is_err());
    }

    #[test]
    fn verify_refuses_out_of_order_chunks() {
        let dir = temp_dir();
        fs::create_dir(&dir).unwrap();
        let p = profile();
        let ticks1 = chunk_ticks(1_735_691_400_000, 3, 2, 1_000_010, &p);
        let ticks2 = chunk_ticks(1_735_689_600_000, 0, 2, 1_000_000, &p);
        let path1 = write_dataset(&dir, "chunk-000000.rsf", &ticks1, &p, "source-v1").unwrap();
        let path2 = write_dataset(&dir, "chunk-000001.rsf", &ticks2, &p, "source-v1").unwrap();
        let error = describe(&dir, &[path1, path2]).unwrap_err();
        assert!(error.contains("order"), "{error}");
    }

    #[test]
    fn decode_resume_advances_and_persists_checkpoint() {
        let dir = temp_dir();
        fs::create_dir(&dir).unwrap();
        let store_dir = temp_dir();
        fs::create_dir(&store_dir).unwrap();
        let p = profile();
        let mut paths = Vec::new();
        for i in 0..3 {
            let ticks = chunk_ticks(
                1_735_689_600_000 + (i as i64) * 180_000,
                i * 3,
                3,
                1_000_000 + (i as i64) * 10,
                &p,
            );
            paths.push(
                write_dataset(&dir, &format!("chunk-{i:06}.rsf"), &ticks, &p, "source-v1").unwrap(),
            );
        }
        let spec = describe(&dir, &paths).unwrap();
        let store = CheckpointStore::open(&store_dir).unwrap();
        let mut driver = StreamDriver::open(&dir, &spec, &store).unwrap();
        assert_eq!(driver.progress().consumed_chunks, 0);
        // Step 1: consume one chunk (3 events).
        let (decoded, complete) = driver.step(3).unwrap();
        assert_eq!(decoded.len(), 3);
        assert!(!complete);
        assert_eq!(driver.progress().consumed_chunks, 1);
        // Step 2: consume two more chunks.
        let (decoded, complete) = driver.step(6).unwrap();
        assert_eq!(decoded.len(), 6);
        assert!(complete);
        assert_eq!(driver.progress().consumed_chunks, 3);
        // Full report matches.
        let report = stream_report(&dir, &spec).unwrap();
        assert_eq!(report.events, 9);
        assert_eq!(report.chunks, 3);
        // Checkpoint persisted and self-authenticating.
        let latest = store.latest().unwrap().unwrap();
        assert_eq!(latest.consumed_chunks, 3);
        assert_eq!(latest.consumed_events, 9);
        assert_eq!(latest.spec_identity, identity(&spec).unwrap());
    }

    #[test]
    fn resume_refuses_tampered_consumed_prefix() {
        let dir = temp_dir();
        fs::create_dir(&dir).unwrap();
        let store_dir = temp_dir();
        fs::create_dir(&store_dir).unwrap();
        let p = profile();
        let ticks1 = chunk_ticks(1_735_689_600_000, 0, 3, 1_000_000, &p);
        let ticks2 = chunk_ticks(1_735_691_400_000, 3, 3, 1_000_010, &p);
        let path1 = write_dataset(&dir, "chunk-000000.rsf", &ticks1, &p, "source-v1").unwrap();
        let path2 = write_dataset(&dir, "chunk-000001.rsf", &ticks2, &p, "source-v1").unwrap();
        let spec = describe(&dir, &[path1.clone(), path2]).unwrap();
        let store = CheckpointStore::open(&store_dir).unwrap();
        let mut driver = StreamDriver::open(&dir, &spec, &store).unwrap();
        driver.step(3).unwrap(); // consume chunk 0
        // Tamper with the consumed chunk 0.
        let mut bytes = fs::read(&path1).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&path1, bytes).unwrap();
        // Reopen must refuse: consumed prefix no longer binds.
        let error = StreamDriver::open(&dir, &spec, &store).unwrap_err();
        assert!(error.contains("mismatch"), "{error}");
    }

    #[test]
    fn checkpoint_chain_is_hash_bound() {
        let dir = temp_dir();
        fs::create_dir(&dir).unwrap();
        let store_dir = temp_dir();
        fs::create_dir(&store_dir).unwrap();
        let p = profile();
        let ticks = chunk_ticks(1_735_689_600_000, 0, 3, 1_000_000, &p);
        let path = write_dataset(&dir, "chunk-000000.rsf", &ticks, &p, "source-v1").unwrap();
        let spec = describe(&dir, &[path]).unwrap();
        let store = CheckpointStore::open(&store_dir).unwrap();
        let mut driver = StreamDriver::open(&dir, &spec, &store).unwrap();
        driver.step(3).unwrap();
        let latest = store.latest().unwrap().unwrap();
        assert!(!latest.previous.is_empty()); // initial checkpoint linked
        // Tamper with stored checkpoint self_hash must be refused on load.
        let mut tampered = latest.clone();
        tampered.self_hash = "0".repeat(64);
        store.store(&tampered).unwrap();
        assert!(store.load(&tampered.self_hash).is_err());
    }

    #[test]
    fn stream_coverage_reports_per_chunk() {
        let dir = temp_dir();
        fs::create_dir(&dir).unwrap();
        let p = profile();
        let ticks1 = chunk_ticks(1_735_689_600_000, 0, 3, 1_000_000, &p);
        let ticks2 = chunk_ticks(1_735_691_400_000, 3, 2, 1_000_010, &p);
        let path1 = write_dataset(&dir, "chunk-000000.rsf", &ticks1, &p, "source-v1").unwrap();
        let path2 = write_dataset(&dir, "chunk-000001.rsf", &ticks2, &p, "source-v1").unwrap();
        let spec = describe(&dir, &[path1, path2]).unwrap();
        let coverage = stream_coverage(&dir, &spec).unwrap();
        assert_eq!(coverage.chunks, 2);
        assert_eq!(coverage.events, 5);
        assert_eq!(coverage.per_chunk.len(), 2);
        assert_eq!(coverage.per_chunk[0].events, 3);
        assert_eq!(coverage.per_chunk[1].events, 2);
    }

    #[test]
    fn empty_and_budget_inputs_are_refused() {
        let dir = temp_dir();
        fs::create_dir(&dir).unwrap();
        let error = describe(&dir, &[]).unwrap_err();
        assert!(error.contains("one to ten thousand"), "{error}");
        let p = profile();
        let ticks = chunk_ticks(1_735_689_600_000, 0, 3, 1_000_000, &p);
        let path = write_dataset(&dir, "chunk-000000.rsf", &ticks, &p, "source-v1").unwrap();
        let spec = describe(&dir, &[path]).unwrap();
        let store = CheckpointStore::open(&temp_dir()).unwrap();
        let mut driver = StreamDriver::open(&dir, &spec, &store).unwrap();
        assert!(driver.step(0).is_err());
        assert!(driver.step(MAX_STREAM_EVENTS + 1).is_err());
    }
}
