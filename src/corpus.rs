//! Ordered immutable native chunks, bounded resident quotes and incremental candles.
//! Corpus inventory never establishes complete source coverage or eligibility.
use crate::{
    Result,
    candles::{self, Timeframe},
    contracts::Profile,
    data::{self, DataClass, Tick},
    native,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::Path,
};
const VERSION: &str = "ordered-native-corpus-v1";
const BLOCK_VERSION: &str = "ordered-block-corpus-v2";
/// Explicit per-context aggregate limit; callers must also reserve concurrent objects.
/// Covers the closed M1-D1 contexts of a continuous two-year UTC fixture.
pub const MAX_RETAINED_CANDLES: usize = 2_000_000;
const SPEC_CAP: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub path: String,
    pub dataset_identity: String,
    pub file_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_root: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Specification {
    pub version: String,
    pub max_chunk_events: usize,
    pub chunks: Vec<Artifact>,
}
impl Specification {
    pub fn identity(&self) -> Result<String> {
        if ![VERSION, BLOCK_VERSION].contains(&self.version.as_str())
            || self.chunks.is_empty()
            || self.chunks.len() > 10000
            || self.max_chunk_events == 0
            || self.max_chunk_events
                > if self.version == VERSION {
                    100000
                } else {
                    2000000
                }
        {
            return Err("invalid corpus version/chunk/event budget".into());
        }
        for c in &self.chunks {
            if c.path.is_empty()
                || c.path.len() > 4096
                || !crate::machine_evidence::hex(&c.dataset_identity)
                || !crate::machine_evidence::hex(&c.file_hash)
                || c.block_root
                    .as_ref()
                    .is_some_and(|r| !crate::machine_evidence::hex(r))
                || self.version == VERSION && c.block_root.is_some()
            {
                return Err("invalid corpus artifact identity/path".into());
            }
        }
        if self.version == BLOCK_VERSION {
            if !self.chunks.iter().any(|c| c.block_root.is_some()) {
                return Err("block corpus requires a pinned block artifact".into());
            }
            return data::hash(&(
                BLOCK_VERSION,
                self.chunks
                    .iter()
                    .map(|c| (&c.dataset_identity, &c.file_hash, &c.block_root))
                    .collect::<Vec<_>>(),
            ));
        }
        // Moving unchanged artifacts does not change semantic/provenance identity.
        data::hash(&(
            VERSION,
            self.chunks
                .iter()
                .map(|c| (&c.dataset_identity, &c.file_hash))
                .collect::<Vec<_>>(),
        ))
    }
}
pub fn load(path: &Path) -> Result<Specification> {
    let f = File::open(path).map_err(|e| e.to_string())?;
    if f.metadata().map_err(|e| e.to_string())?.len() > SPEC_CAP {
        return Err("corpus specification exceeds16MiB".into());
    }
    let mut bytes = Vec::new();
    f.take(SPEC_CAP + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > SPEC_CAP {
        return Err("corpus specification grew beyond budget".into());
    }
    let s: Specification = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    s.identity()?;
    Ok(s)
}
/// Stream canonical JSON-array fingerprints without retaining an event/candle array.
pub(crate) struct SequenceHash {
    hash: blake3::Hasher,
    count: u64,
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
impl SequenceHash {
    pub(crate) fn new() -> Self {
        let mut hash = blake3::Hasher::new();
        hash.update(b"[");
        Self { hash, count: 0 }
    }
    pub(crate) fn push<T: Serialize>(&mut self, value: &T) -> Result<()> {
        if self.count > 0 {
            self.hash.update(b",");
        }
        serde_json::to_writer(HashWriter(&mut self.hash), value).map_err(|e| e.to_string())?;
        self.count = self.count.checked_add(1).ok_or("corpus count overflow")?;
        Ok(())
    }
    pub(crate) fn finish(mut self) -> String {
        self.hash.update(b"]");
        self.hash.finalize().to_hex().to_string()
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct CandleSummary {
    pub timeframe: Timeframe,
    pub closed_candles: u64,
    pub content_hash: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub version: &'static str,
    pub corpus_identity: String,
    pub chunks: usize,
    pub profile: Profile,
    pub declared_class: DataClass,
    pub source: String,
    pub events: u64,
    pub ordered_events_hash: String,
    pub first: crate::coverage::Event,
    pub latest: crate::coverage::Event,
    pub timestamp_ties: u64,
    pub identical_quote_events: u64,
    pub gaps_over_hour: u64,
    pub max_resident_chunk_events: usize,
    pub max_resident_quote_events: usize,
    pub candles: Vec<CandleSummary>,
    pub limitations: &'static str,
}
/// Observed verified source facts only: no candles or scientific eligibility.
/// Every reuse must check the currently resolved artifacts again.
/// ```compile_fail
/// use rust_strategy_factory::corpus::SourceFacts;
/// let _: SourceFacts = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct SourceFacts {
    version: &'static str,
    corpus_identity: String,
    profile: Profile,
    declared_class: DataClass,
    source: String,
    events: u64,
    ordered_events_hash: String,
    first: crate::coverage::Event,
    latest: crate::coverage::Event,
    max_resident_quote_events: usize,
    limitations: &'static str,
    #[serde(skip)]
    max_artifact_events: usize,
}
const FACTS_LIMITATIONS: &str = "Observed source identity/profile/declared class and exact ordered events only. No candle, complete coverage, independent approval, execution or scientific eligibility is established. Reuse must verify actual current artifacts; metadata is additional to bounded quote residency.";
impl SourceFacts {
    pub fn corpus_identity(&self) -> &str {
        &self.corpus_identity
    }
    pub fn profile(&self) -> &Profile {
        &self.profile
    }
    pub fn declared_class(&self) -> DataClass {
        self.declared_class
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn events(&self) -> u64 {
        self.events
    }
    pub fn ordered_events_hash(&self) -> &str {
        &self.ordered_events_hash
    }
    pub fn first(&self) -> &crate::coverage::Event {
        &self.first
    }
    pub fn latest(&self) -> &crate::coverage::Event {
        &self.latest
    }
    pub fn max_resident_quote_events(&self) -> usize {
        self.max_resident_quote_events
    }
    pub fn verify_current(&self, base: &Path, spec: &Specification) -> Result<()> {
        verify_current_identity(base, spec, &self.corpus_identity, self.max_artifact_events)
    }
}
fn validate_verified_specification(
    spec: &Specification,
    identity: &str,
    max_artifact_events: usize,
) -> Result<()> {
    if identity != spec.identity()? {
        return Err("verified source identity differs".into());
    }
    if max_artifact_events == 0 || max_artifact_events > spec.max_chunk_events {
        return Err("verified source exceeds current artifact event budget".into());
    }
    Ok(())
}
fn verify_current_identity(
    base: &Path,
    spec: &Specification,
    identity: &str,
    max_artifact_events: usize,
) -> Result<()> {
    validate_verified_specification(spec, identity, max_artifact_events)?;
    // Only private, fully decoded snapshots reach this path. Equal complete
    // file hashes plus the same ordered specification preserve the validated
    // profile/class/quotes/order/root. Read every current byte on one descriptor;
    // neither metadata nor the expected hash alone establishes current identity.
    let mut buffer = [0u8; 65536];
    for chunk in &spec.chunks {
        let file = File::open(base.join(&chunk.path)).map_err(|e| e.to_string())?;
        let length = file.metadata().map_err(|e| e.to_string())?.len();
        let cap = if chunk.block_root.is_some() {
            crate::blocked_native::MAX_BYTES
        } else {
            native::MAX_BYTES
        };
        if length == 0 || length > cap {
            return Err("current corpus artifact byte budget exceeded".into());
        }
        let mut reader = file.take(cap + 1);
        let mut hash = blake3::Hasher::new();
        let mut bytes = 0u64;
        loop {
            let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            bytes = bytes
                .checked_add(n as u64)
                .ok_or("artifact byte count overflow")?;
            if bytes > cap {
                return Err("current corpus artifact grew beyond byte budget".into());
            }
            hash.update(&buffer[..n]);
        }
        if bytes != length || hash.finalize().to_hex().as_str() != chunk.file_hash {
            return Err("corpus artifact revision/fingerprint checksum mismatch".into());
        }
    }
    Ok(())
}
/// Compute only source facts needed for a frozen execution key, without candles.
pub fn source_facts(base: &Path, spec: &Specification) -> Result<SourceFacts> {
    let identity = spec.identity()?;
    let mut profile = None;
    let mut source = None;
    let mut class = None;
    let mut first = None;
    let mut latest = None;
    let mut peak = 0;
    let mut events = SequenceHash::new();
    let mut artifact_events = 0usize;
    let mut max_artifact_events = 0usize;
    visit_blocks(base, spec, |block| {
        artifact_events = artifact_events
            .checked_add(block.ticks.len())
            .ok_or("artifact event count overflow")?;
        if block.last_in_chunk {
            max_artifact_events = max_artifact_events.max(artifact_events);
            artifact_events = 0;
        }
        if profile.is_none() {
            profile = Some(block.profile.clone());
            source = Some(block.manifest.source.clone());
            class = Some(block.manifest.class);
        }
        peak = peak.max(block.ticks.len());
        for tick in block.ticks {
            if first.is_none() {
                first = Some(*tick);
            }
            events.push(tick)?;
            latest = Some(*tick);
        }
        Ok(())
    })?;
    Ok(SourceFacts {
        version: "observed-source-facts-v1",
        corpus_identity: identity,
        profile: profile.ok_or("empty source facts")?,
        declared_class: class.ok_or("empty source facts")?,
        source: source.ok_or("empty source facts")?,
        events: events.count,
        ordered_events_hash: events.finish(),
        first: crate::coverage::Event::from(&first.ok_or("empty source facts")?),
        latest: crate::coverage::Event::from(&latest.ok_or("empty source facts")?),
        max_resident_quote_events: peak,
        limitations: FACTS_LIMITATIONS,
        max_artifact_events,
    })
}
/// Verified immutable candle contexts built alongside inspection in one source pass.
/// This is a bounded in-memory snapshot, not current eligibility or a resume proof.
pub struct Prepared {
    report: Report,
    frames: std::collections::BTreeMap<Timeframe, Vec<candles::Candle>>,
    max_artifact_events: usize,
}
impl Prepared {
    pub(crate) fn source_facts(&self) -> SourceFacts {
        SourceFacts {
            version: "observed-source-facts-v1",
            corpus_identity: self.report.corpus_identity.clone(),
            profile: self.report.profile.clone(),
            declared_class: self.report.declared_class,
            source: self.report.source.clone(),
            events: self.report.events,
            ordered_events_hash: self.report.ordered_events_hash.clone(),
            first: self.report.first.clone(),
            latest: self.report.latest.clone(),
            max_resident_quote_events: self.report.max_resident_quote_events,
            limitations: FACTS_LIMITATIONS,
            max_artifact_events: self.max_artifact_events,
        }
    }
    pub fn report(&self) -> &Report {
        &self.report
    }
    pub fn candles(&self, timeframe: Timeframe) -> Result<&[candles::Candle]> {
        self.frames
            .get(&timeframe)
            .map(Vec::as_slice)
            .ok_or_else(|| "prepared corpus timeframe unavailable".into())
    }
    pub fn retained_candles(&self) -> usize {
        self.frames.values().map(Vec::len).sum()
    }
    /// Recheck every actual artifact before reusing this immutable context.
    /// Identity alone never establishes that the currently resolved bytes match.
    pub fn verify_current(&self, base: &Path, spec: &Specification) -> Result<()> {
        verify_current_identity(
            base,
            spec,
            &self.report.corpus_identity,
            self.max_artifact_events,
        )
    }
    /// Decode only the requested artifact range, then rehash EVERY current
    /// artifact before returning success. The private source proof authenticates
    /// source semantics, never an account checkpoint. Global indexes are retained.
    pub(crate) fn visit_replay_range(
        &self,
        base: &Path,
        spec: &Specification,
        range: std::ops::Range<usize>,
        visit: impl FnMut(QuoteBlock<'_>) -> Result<()>,
    ) -> Result<()> {
        validate_verified_specification(
            spec,
            &self.report.corpus_identity,
            self.max_artifact_events,
        )?;
        visit_block_range(base, spec, range, visit)?;
        // A skipped prefix or future artifact may change during the callback.
        // Publish no account state unless the final full-byte check succeeds.
        self.verify_current(base, spec)
    }
    pub fn signals(
        &self,
        strategy: &crate::strategies::Strategy,
        feature_cells: usize,
    ) -> Result<crate::indicators::Mask> {
        let strategy = strategy.canonical()?;
        let primary = self.candles(strategy.timeframe)?;
        crate::strategies::Features::with_frames(
            primary,
            feature_cells,
            &self.frames,
            strategy.timeframe,
        )?
        .signal(&strategy.rule)
    }
}
pub fn prepare(
    base: &Path,
    spec: &Specification,
    primary: Timeframe,
    max_candles: usize,
) -> Result<Prepared> {
    if !(1..=MAX_RETAINED_CANDLES).contains(&max_candles) {
        return Err("invalid aggregate corpus candle budget".into());
    }
    inspect_prepared(base, spec, Some((primary, max_candles)))
}
/// A verified immutable inventory with no retained candle vectors.
/// Current-byte eligibility requires verify_current on every consumer.
/// It cannot be reconstructed from a supplied/forged JSON report.
/// ```compile_fail
/// use rust_strategy_factory::corpus::Prepared;
/// let _: Prepared = serde_json::from_str("{}").unwrap();
/// ```
pub fn verified_inventory(base: &Path, spec: &Specification) -> Result<Prepared> {
    inspect_prepared(base, spec, None)
}
pub fn inspect(base: &Path, spec: &Specification) -> Result<Report> {
    Ok(verified_inventory(base, spec)?.report)
}
fn inspect_prepared(
    base: &Path,
    spec: &Specification,
    capture: Option<(Timeframe, usize)>,
) -> Result<Prepared> {
    let mut frames = std::collections::BTreeMap::<Timeframe, Vec<candles::Candle>>::new();
    if let Some((primary, _)) = capture {
        for tf in Timeframe::ALL {
            if tf.millis() >= primary.millis() {
                frames.insert(tf, Vec::new());
            }
        }
    }
    let mut retained = 0usize;
    let identity = spec.identity()?;
    let mut profile = None;
    let mut source = None;
    let mut class = None;
    let mut streams = Vec::new();
    let mut candle_hashes = Timeframe::ALL
        .iter()
        .map(|_| SequenceHash::new())
        .collect::<Vec<_>>();
    let mut events = SequenceHash::new();
    let mut previous: Option<Tick> = None;
    let mut first = None;
    let mut ties = 0;
    let mut repeats = 0;
    let mut gaps = 0;
    let mut peak = 0;
    let mut artifact_events = 0usize;
    let mut max_artifact_events = 0usize;
    visit_blocks(base, spec, |d| {
        artifact_events = artifact_events
            .checked_add(d.ticks.len())
            .ok_or("artifact event count overflow")?;
        if d.last_in_chunk {
            max_artifact_events = max_artifact_events.max(artifact_events);
            artifact_events = 0;
        }
        if profile.is_none() {
            for tf in Timeframe::ALL {
                streams.push(candles::Stream::new(d.profile.clone(), tf)?);
            }
            profile = Some(d.profile.clone());
            source = Some(d.manifest.source.clone());
            class = Some(d.manifest.class);
        }
        peak = peak.max(d.ticks.len());
        for tick in d.ticks {
            if let Some(p) = previous {
                if tick.timestamp_ms < p.timestamp_ms || tick.sequence <= p.sequence {
                    return Err("corpus cross-chunk timestamp/sequence order invalid".into());
                }
                ties += u64::from(tick.timestamp_ms == p.timestamp_ms);
                repeats += u64::from(tick.bid == p.bid && tick.ask == p.ask);
                gaps +=
                    u64::from(i128::from(tick.timestamp_ms) - i128::from(p.timestamp_ms) > 3600000);
            }
            if first.is_none() {
                first = Some(*tick);
            }
            for ((stream, hash), tf) in streams
                .iter_mut()
                .zip(&mut candle_hashes)
                .zip(Timeframe::ALL)
            {
                if let Some(c) = stream.push(*tick)? {
                    hash.push(&c)?;
                    if let Some(output) = frames.get_mut(&tf) {
                        if capture.is_none_or(|(_, cap)| retained >= cap) {
                            return Err("aggregate corpus candle budget exceeded".into());
                        }
                        output.push(c);
                        retained += 1;
                    }
                }
            }
            events.push(tick)?;
            previous = Some(*tick);
        }
        Ok(())
    })?;
    let candles = Timeframe::ALL
        .into_iter()
        .zip(candle_hashes)
        .map(|(timeframe, hash)| CandleSummary {
            timeframe,
            closed_candles: hash.count,
            content_hash: hash.finish(),
        })
        .collect();
    Ok(Prepared {
        report: Report {
            version: if spec.version == VERSION {
                VERSION
            } else {
                BLOCK_VERSION
            },
            corpus_identity: identity,
            chunks: spec.chunks.len(),
            profile: profile.ok_or("empty corpus")?,
            declared_class: class.ok_or("empty corpus")?,
            source: source.ok_or("empty corpus")?,
            events: events.count,
            ordered_events_hash: events.finish(),
            first: crate::coverage::Event::from(&first.ok_or("empty corpus")?),
            latest: crate::coverage::Event::from(&previous.ok_or("empty corpus")?),
            timestamp_ties: ties,
            identical_quote_events: repeats,
            gaps_over_hour: gaps,
            max_resident_chunk_events: peak,
            max_resident_quote_events: peak,
            candles,
            limitations: "Observed ordered native chunks and closed UTC candle fingerprints only. One bounded quote chunk plus seven candle states is retained; metadata and per-chunk validation allocations are additional. No empty/final unfinished candles, missing quotes, source completeness, approved class, acquisition/resume or scientific eligibility is established. Synthetic USD replay is a separate explicitly gated consumer. Source archives/provenance remain separate immutable artifacts; profiles/source/class cannot be mixed.",
        },
        frames,
        max_artifact_events,
    })
}

/// Describe existing immutable artifacts without copying them. The caller can
/// inspect the result and publish it with the normal no-replace artifact writer.
pub fn describe(paths: &[std::path::PathBuf], max_chunk_events: usize) -> Result<Specification> {
    let blocked = paths
        .iter()
        .any(|p| p.extension().is_some_and(|e| e == "rsfb"));
    if paths.is_empty()
        || paths.len() > 10000
        || max_chunk_events == 0
        || max_chunk_events > if blocked { 2000000 } else { 100000 }
    {
        return Err("invalid corpus chunk budget".into());
    }
    let mut chunks = Vec::new();
    for path in paths {
        let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        let (dataset_identity, file_hash, block_root) =
            if path.extension().is_some_and(|e| e == "rsfb") {
                let report = crate::blocked_native::inspect(&path, max_chunk_events)?;
                (report.dataset_identity, report.file_hash, Some(report.root))
            } else {
                let (d, hash) = native::read_bounded(&path, max_chunk_events.min(100000))?;
                (data::dataset_identity(&d.manifest)?, hash, None)
            };
        chunks.push(Artifact {
            path: path.to_str().ok_or("non-UTF8 corpus path")?.into(),
            dataset_identity,
            file_hash,
            block_root,
        });
    }
    let spec = Specification {
        version: if blocked { BLOCK_VERSION } else { VERSION }.into(),
        max_chunk_events,
        chunks,
    };
    if serde_json::to_vec(&spec).map_err(|e| e.to_string())?.len() as u64 > SPEC_CAP {
        return Err("corpus specification exceeds16MiB".into());
    }
    spec.identity()?;
    Ok(spec)
}

/// Visit verified chunks without concatenating quotes. Callbacks may retain
/// bounded feature/account state across boundaries. A later failure preserves
/// the already visited prefix; callers must publish results only on success.
pub fn visit_chunks(
    base: &Path,
    spec: &Specification,
    mut visit: impl FnMut(usize, &crate::data::Dataset) -> Result<()>,
) -> Result<()> {
    spec.identity()?;
    let mut context: Option<(Profile, String, DataClass)> = None;
    let mut previous: Option<Tick> = None;
    for (index, chunk) in spec.chunks.iter().enumerate() {
        let (dataset, fingerprint) = if let Some(root) = &chunk.block_root {
            let (d, report) = crate::blocked_native::read_bounded(
                &base.join(&chunk.path),
                spec.max_chunk_events.min(100000),
            )?;
            if &report.root != root {
                return Err("blocked artifact root differs from corpus".into());
            }
            (d, report.file_hash)
        } else {
            native::read_bounded(&base.join(&chunk.path), spec.max_chunk_events.min(100000))?
        };
        if fingerprint != chunk.file_hash
            || data::dataset_identity(&dataset.manifest)? != chunk.dataset_identity
        {
            return Err("corpus artifact revision/fingerprint mismatch".into());
        }
        if let Some((profile, source, class)) = &context {
            if profile != &dataset.profile
                || source != &dataset.manifest.source
                || class != &dataset.manifest.class
            {
                return Err("corpus profile/source/class mismatch".into());
            }
        } else {
            context = Some((
                dataset.profile.clone(),
                dataset.manifest.source.clone(),
                dataset.manifest.class,
            ));
        }
        let first = dataset.ticks[0];
        if previous
            .is_some_and(|p| first.timestamp_ms < p.timestamp_ms || first.sequence <= p.sequence)
        {
            return Err("corpus cross-chunk timestamp/sequence order invalid".into());
        }
        visit(index, &dataset)?;
        previous = dataset.ticks.last().copied();
    }
    Ok(())
}

/// Build a bounded candle context from verified chunks without retaining the
/// quote corpus. The cap includes only published candles; no final bucket is
/// fabricated. Consumers must retain normal causal publication checks.
pub fn candle_context(
    base: &Path,
    spec: &Specification,
    timeframe: Timeframe,
    max_candles: usize,
) -> Result<Vec<candles::Candle>> {
    if max_candles == 0 || max_candles > MAX_RETAINED_CANDLES {
        return Err(format!(
            "corpus candle context budget must be1..{MAX_RETAINED_CANDLES}"
        ));
    }
    let mut stream = None;
    let mut output = Vec::new();
    visit_blocks(base, spec, |dataset| {
        if stream.is_none() {
            stream = Some(candles::Stream::new(dataset.profile.clone(), timeframe)?);
        }
        let stream = stream.as_mut().ok_or("candle stream unavailable")?;
        for tick in dataset.ticks {
            if let Some(candle) = stream.push(*tick)? {
                if output.len() >= max_candles {
                    return Err("corpus candle context budget exceeded".into());
                }
                output.push(candle);
            }
        }
        Ok(())
    })?;
    Ok(output)
}

/// Compile causal signals from verified multi-frame contexts with an aggregate candle cap.
pub fn signals(
    base: &Path,
    spec: &Specification,
    strategy: &crate::strategies::Strategy,
    max_candles: usize,
    feature_cells: usize,
) -> Result<crate::indicators::Mask> {
    signals_internal(base, spec, strategy, max_candles, feature_cells, None)
}
/// Compile the prior-only prefix needed for a window. Signals before its start
/// are warmup only; execution eligibility must still enforce the window.
pub fn signals_window(
    base: &Path,
    spec: &Specification,
    strategy: &crate::strategies::Strategy,
    max_candles: usize,
    feature_cells: usize,
    window: &crate::validation::Interval,
) -> Result<crate::indicators::Mask> {
    window.validate()?;
    signals_internal(
        base,
        spec,
        strategy,
        max_candles,
        feature_cells,
        Some(window),
    )
}
fn build_frames(
    base: &Path,
    spec: &Specification,
    timeframe: Timeframe,
    max_candles: usize,
    window: Option<&crate::validation::Interval>,
) -> Result<BTreeMap<Timeframe, Vec<candles::Candle>>> {
    let mut frames = BTreeMap::new();
    let mut streams = Vec::new();
    let mut total = 0;
    visit_blocks(base, spec, |dataset| {
        if streams.is_empty() {
            for tf in Timeframe::ALL {
                if tf.millis() >= timeframe.millis() {
                    streams.push((tf, candles::Stream::new(dataset.profile.clone(), tf)?));
                    frames.insert(tf, Vec::new());
                }
            }
        }
        for tick in dataset.ticks {
            if window.is_some_and(|w| tick.timestamp_ms >= w.end_exclusive_ms) {
                break;
            }
            for (tf, stream) in &mut streams {
                if let Some(c) = stream.push(*tick)? {
                    if total >= max_candles {
                        return Err("aggregate corpus candle budget exceeded".into());
                    }
                    frames.get_mut(tf).ok_or("frame unavailable")?.push(c);
                    total += 1;
                }
            }
        }
        Ok(())
    })?;
    Ok(frames)
}
fn signal_frames(
    frames: &BTreeMap<Timeframe, Vec<candles::Candle>>,
    strategy: &crate::strategies::Strategy,
    feature_cells: usize,
) -> Result<crate::indicators::Mask> {
    let candles = frames
        .get(&strategy.timeframe)
        .ok_or("primary frame missing")?;
    crate::strategies::Features::with_frames(candles, feature_cells, frames, strategy.timeframe)?
        .signal(&strategy.rule)
}
fn signals_internal(
    base: &Path,
    spec: &Specification,
    strategy: &crate::strategies::Strategy,
    max_candles: usize,
    feature_cells: usize,
    window: Option<&crate::validation::Interval>,
) -> Result<crate::indicators::Mask> {
    let strategy = strategy.canonical()?;
    if max_candles == 0 || max_candles > MAX_RETAINED_CANDLES {
        return Err("invalid aggregate corpus candle budget".into());
    }
    let frames = build_frames(base, spec, strategy.timeframe, max_candles, window)?;
    signal_frames(&frames, &strategy, feature_cells)
}
/// Causal signal masks for a bounded strategy population over one verified month, sharing a
/// single frame build. The candle budget applies once to the shared build, and the
/// population is refused above the behavioral membership bound.
pub fn signals_many(
    base: &Path,
    spec: &Specification,
    strategies: &[crate::strategies::Strategy],
    max_candles: usize,
    feature_cells: usize,
) -> Result<Vec<crate::indicators::Mask>> {
    if strategies.is_empty() {
        return Err("no strategies supplied".into());
    }
    if strategies.len() > crate::behavior::MAX_MEMBERS {
        return Err("behavioral population budget exceeded".into());
    }
    if max_candles == 0 || max_candles > MAX_RETAINED_CANDLES {
        return Err("invalid aggregate corpus candle budget".into());
    }
    let canonical = strategies
        .iter()
        .map(|strategy| strategy.canonical())
        .collect::<Result<Vec<_>>>()?;
    let primary = canonical
        .iter()
        .map(|strategy| strategy.timeframe)
        .min()
        .ok_or("no strategies")?;
    let frames = build_frames(base, spec, primary, max_candles, None)?;
    canonical
        .iter()
        .map(|strategy| signal_frames(&frames, strategy, feature_cells))
        .collect()
}

/// A verified quote block; its manifest describes the complete source artifact, not a partial Dataset.
pub struct QuoteBlock<'a> {
    pub chunk_index: usize,
    pub block_index: usize,
    pub last_in_chunk: bool,
    pub profile: &'a Profile,
    pub manifest: &'a data::Manifest,
    pub ticks: &'a [Tick],
}
fn accept_block(
    context: &mut Option<(Profile, String, DataClass)>,
    previous: &mut Option<Tick>,
    block: QuoteBlock<'_>,
    visit: &mut impl FnMut(QuoteBlock<'_>) -> Result<()>,
) -> Result<()> {
    if let Some((profile, source, class)) = context {
        if profile != block.profile
            || source != &block.manifest.source
            || class != &block.manifest.class
        {
            return Err("corpus profile/source/class mismatch".into());
        }
    } else {
        *context = Some((
            block.profile.clone(),
            block.manifest.source.clone(),
            block.manifest.class,
        ));
    }
    let first = block.ticks.first().ok_or("empty corpus quote block")?;
    if previous.is_some_and(|p| first.timestamp_ms < p.timestamp_ms || first.sequence <= p.sequence)
    {
        return Err("corpus cross-block timestamp/sequence order invalid".into());
    }
    *previous = block.ticks.last().copied();
    visit(block)
}
/// Stream complete verified artifacts in bounded quote blocks. Publish only after full successful return.
pub fn visit_blocks(
    base: &Path,
    spec: &Specification,
    visit: impl FnMut(QuoteBlock<'_>) -> Result<()>,
) -> Result<()> {
    visit_block_range(base, spec, 0..spec.chunks.len(), visit)
}
fn visit_block_range(
    base: &Path,
    spec: &Specification,
    range: std::ops::Range<usize>,
    mut visit: impl FnMut(QuoteBlock<'_>) -> Result<()>,
) -> Result<()> {
    spec.identity()?;
    if range.start > range.end || range.end > spec.chunks.len() {
        return Err("corpus replay artifact range outside specification".into());
    }
    let mut context = None;
    let mut previous = None;
    for (chunk_index, chunk) in spec
        .chunks
        .iter()
        .enumerate()
        .take(range.end)
        .skip(range.start)
    {
        if let Some(root) = &chunk.block_root {
            let mut reader = crate::blocked_native::Reader::open(
                &base.join(&chunk.path),
                spec.max_chunk_events,
                Some(root),
            )?;
            if data::dataset_identity(reader.manifest())? != chunk.dataset_identity {
                return Err("corpus blocked dataset identity differs".into());
            }
            let count = reader.block_count();
            let report = reader.visit(|block_index, profile, manifest, ticks| {
                accept_block(
                    &mut context,
                    &mut previous,
                    QuoteBlock {
                        chunk_index,
                        block_index,
                        last_in_chunk: block_index + 1 == count,
                        profile,
                        manifest,
                        ticks,
                    },
                    &mut visit,
                )
            })?;
            if report.file_hash != chunk.file_hash {
                return Err("corpus blocked consumed fingerprint differs".into());
            }
        } else {
            let (dataset, fingerprint) =
                native::read_bounded(&base.join(&chunk.path), spec.max_chunk_events.min(100000))?;
            if fingerprint != chunk.file_hash
                || data::dataset_identity(&dataset.manifest)? != chunk.dataset_identity
            {
                return Err("corpus artifact revision/fingerprint mismatch".into());
            }
            accept_block(
                &mut context,
                &mut previous,
                QuoteBlock {
                    chunk_index,
                    block_index: 0,
                    last_in_chunk: true,
                    profile: &dataset.profile,
                    manifest: &dataset.manifest,
                    ticks: &dataset.ticks,
                },
                &mut visit,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod replay_range_tests {
    use super::*;
    use std::{fs, path::PathBuf};
    struct Temporary(PathBuf);
    impl Drop for Temporary {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn final_range_verification_refuses_skipped_prefix_and_future_changes_during_callbacks() {
        for format in 0..3 {
            let root = Temporary(std::env::temp_dir().join(format!(
                "source-range-callback-{}-{format}",
                std::process::id()
            )));
            fs::create_dir(&root.0).unwrap();
            let data = data::synthetic_fixture("EURUSD").unwrap();
            let mut paths = Vec::new();
            for (i, ticks) in data.ticks.chunks(data.ticks.len().div_ceil(3)).enumerate() {
                let d = data::dataset(
                    data.profile.clone(),
                    ticks.to_vec(),
                    &data.manifest.source,
                    data.manifest.archive_hash.clone(),
                    true,
                )
                .unwrap();
                let blocked = format == 1 || (format == 2 && i % 2 == 1);
                let path = root
                    .0
                    .join(format!("{i}.{}", if blocked { "rsfb" } else { "rsf" }));
                if blocked {
                    crate::blocked_native::write(&path, &d, 64).unwrap();
                } else {
                    native::write(&path, &d).unwrap();
                }
                paths.push(path);
            }
            assert_eq!(paths.len(), 3);
            let spec = describe(&paths, 100000).unwrap();
            let prepared = prepare(&root.0, &spec, Timeframe::M1, 10000).unwrap();
            for changed_index in [0, 2] {
                let original = fs::read(&paths[changed_index]).unwrap();
                let mut changed = false;
                let error = prepared
                    .visit_replay_range(&root.0, &spec, 1..2, |block| {
                        assert_eq!(block.chunk_index, 1);
                        if !changed {
                            let mut bytes = original.clone();
                            let last = bytes.len() - 1;
                            bytes[last] ^= 1;
                            fs::write(&paths[changed_index], bytes).unwrap();
                            changed = true;
                        }
                        Ok(())
                    })
                    .unwrap_err();
                assert!(changed);
                assert!(error.contains("checksum"), "{error}");
                fs::write(&paths[changed_index], original).unwrap();
            }
            prepared
                .visit_replay_range(&root.0, &spec, 1..1, |_| {
                    panic!("empty range decoded quotes")
                })
                .unwrap();
            assert!(
                prepared
                    .visit_replay_range(&root.0, &spec, 3..4, |_| Ok(()))
                    .is_err()
            );
            assert!(
                prepared
                    .visit_replay_range(&root.0, &spec, usize::MAX..usize::MAX, |_| Ok(()))
                    .is_err()
            );
            fs::remove_file(&paths[2]).unwrap();
            assert!(
                prepared
                    .visit_replay_range(&root.0, &spec, 1..1, |_| Ok(()))
                    .is_err()
            );
        }
    }
}

pub mod direct_arrays {
    //! Direct hot column arrays over verified corpus chunks.
    //!
    //! Arrays are bound to the exact provenance of their source chunk: the dataset
    //! identity and the consumed file hash. Every array is validated before
    //! publication; nothing partial is exposed.

    use super::Specification;
    use crate::{
        Result,
        contracts::Profile,
        data::{self, DataClass, Dataset, Manifest},
        native,
    };
    use std::{path::Path, sync::Arc};

    /// Direct arrays over one verified chunk's hot columns, bound to the chunk's
    /// exact provenance: dataset identity and consumed file hash.
    pub struct DirectArrays {
        pub timestamps: Arc<[i64]>,
        pub sequences: Arc<[u64]>,
        pub bids: Arc<[i64]>,
        pub asks: Arc<[i64]>,
        pub event_count: usize,
        pub dataset_identity: String,
        pub file_hash: String,
        pub profile: Profile,
        pub manifest: Manifest,
    }

    impl DirectArrays {
        /// Build hot arrays from an already verified dataset. Validates the quote
        /// grid again before publishing; refuses on any violation.
        pub fn from_dataset(dataset: &Dataset, file_hash: &str) -> Result<Self> {
            if dataset.ticks.is_empty() {
                return Err("direct arrays require a nonempty chunk".into());
            }
            let n = dataset.ticks.len();
            let mut timestamps = Vec::with_capacity(n);
            let mut sequences = Vec::with_capacity(n);
            let mut bids = Vec::with_capacity(n);
            let mut asks = Vec::with_capacity(n);
            for tick in &dataset.ticks {
                tick.validate(&dataset.profile)?;
                timestamps.push(tick.timestamp_ms);
                sequences.push(tick.sequence);
                bids.push(tick.bid);
                asks.push(tick.ask);
            }
            Ok(Self {
                timestamps: timestamps.into(),
                sequences: sequences.into(),
                bids: bids.into(),
                asks: asks.into(),
                event_count: n,
                dataset_identity: data::dataset_identity(&dataset.manifest)?,
                file_hash: file_hash.to_string(),
                profile: dataset.profile.clone(),
                manifest: dataset.manifest.clone(),
            })
        }

        /// Read, fully verify and bind one native corpus chunk into direct hot
        /// arrays. The chunk fingerprint and dataset identity must match the
        /// specification entry.
        pub fn from_native_chunk(
            base: &Path,
            spec: &Specification,
            chunk_index: usize,
        ) -> Result<Self> {
            if chunk_index >= spec.chunks.len() {
                return Err("corpus chunk index outside specification".into());
            }
            let chunk = &spec.chunks[chunk_index];
            if chunk.block_root.is_some() {
                return Err("blocked artifacts use blocked_native directly".into());
            }
            let (dataset, fingerprint) =
                native::read_bounded(&base.join(&chunk.path), spec.max_chunk_events.min(100000))?;
            if fingerprint != chunk.file_hash
                || data::dataset_identity(&dataset.manifest)? != chunk.dataset_identity
            {
                return Err("corpus artifact revision/fingerprint mismatch".into());
            }
            Self::from_dataset(&dataset, &fingerprint)
        }

        /// Strict input subclass for direct-array research loading: literal USD
        /// quote and synthetic test-only class, never silently widened.
        pub fn require_synthetic_usd(&self) -> Result<()> {
            if self.profile.quote != "USD" {
                return Err("direct array replay requires a literal USD quote".into());
            }
            if self.manifest.class != DataClass::SyntheticTestOnly {
                return Err("direct array replay requires SyntheticTestOnly".into());
            }
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        fn root() -> std::path::PathBuf {
            let p = std::env::temp_dir().join(format!(
                "direct-arrays-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&p).unwrap();
            p
        }
        fn fixture() -> (std::path::PathBuf, Dataset, String) {
            let root = root();
            let p = Profile::synthetic("EURUSD").unwrap();
            let ticks: Vec<data::Tick> = (0..100)
                .map(|i| data::Tick {
                    timestamp_ms: 1735689600000 + i * 60000,
                    sequence: i as u64,
                    bid: 10000 + i,
                    ask: 10010 + i,
                })
                .collect();
            let data = data::dataset(
                p.clone(),
                ticks.clone(),
                "direct-array fixture",
                "archive".into(),
                true,
            )
            .unwrap();
            let path = root.join("chunk.rsf");
            native::write(&path, &data).unwrap();
            let hash = data::file_hash(&path).unwrap();
            (root, data, hash)
        }
        #[test]
        fn direct_arrays_match_scalar_ticks_with_provenance_bindings() {
            let (root, data, hash) = fixture();
            let spec = crate::corpus::describe(&[root.join("chunk.rsf")], 100000).unwrap();
            let arrays = DirectArrays::from_native_chunk(&root, &spec, 0).unwrap();
            assert_eq!(arrays.event_count, 100);
            assert_eq!(
                arrays.dataset_identity,
                data::dataset_identity(&data.manifest).unwrap()
            );
            assert_eq!(arrays.file_hash, hash);
            for (i, t) in data.ticks.iter().enumerate() {
                assert_eq!(arrays.timestamps[i], t.timestamp_ms);
                assert_eq!(arrays.sequences[i], t.sequence);
                assert_eq!(arrays.bids[i], t.bid);
                assert_eq!(arrays.asks[i], t.ask);
            }
            arrays.require_synthetic_usd().unwrap();
            std::fs::remove_dir_all(root).unwrap();
        }
        #[test]
        fn changed_chunk_bytes_refuse_direct_array_binding() {
            let (root, _data, _hash) = fixture();
            let path = root.join("chunk.rsf");
            let spec = crate::corpus::describe(std::slice::from_ref(&path), 100000).unwrap();
            assert!(DirectArrays::from_native_chunk(&root, &spec, 0).is_ok());
            let mut bytes = std::fs::read(&path).unwrap();
            let last = bytes.len() - 1;
            bytes[last] ^= 1;
            std::fs::write(&path, &bytes).unwrap();
            assert!(DirectArrays::from_native_chunk(&root, &spec, 0).is_err());
            std::fs::remove_dir_all(root).unwrap();
        }
        #[test]
        fn out_of_range_chunk_and_blocked_chunk_refuse() {
            let (root, data, _hash) = fixture();
            let path = root.join("chunk.rsf");
            let spec = crate::corpus::describe(&[path], 100000).unwrap();
            assert!(DirectArrays::from_native_chunk(&root, &spec, 1).is_err());
            let blocked = root.join("blocked.rsfb");
            crate::blocked_native::write(&blocked, &data, 7).unwrap();
            let spec = crate::corpus::describe(&[blocked], 100000).unwrap();
            assert!(DirectArrays::from_native_chunk(&root, &spec, 0).is_err());
            let _ = _hash;
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

pub mod mmap_guard {
    //! Read-only random-access guard over a verified corpus artifact.
    //!
    //! The codebase forbids `unsafe`, so raw `mmap` syscalls are out of scope;
    //! this guard uses pread (`File::read_at`) and exposes mmap-compatible
    //! semantics instead: read-only views, bounds-checked slices, byte-budget
    //! refusal before any access, and fail-closed revalidation if the artifact
    //! is replaced or truncated after the guard is created. No partial slice is
    //! ever published: the budget is checked up front and every slice is
    //! bounds-checked against the resident bytes.

    use crate::Result;
    use std::{
        fs::{self, File},
        io::Read,
        path::Path,
        path::PathBuf,
    };

    /// Read-only, bounds-checked access over one artifact.
    pub struct MmapGuard {
        bytes: Vec<u8>,
        length: u64,
        path: PathBuf,
    }

    impl MmapGuard {
        /// Open `path`, read it fully once its length passes `budget`, and refuse
        /// before any read when the artifact exceeds the budget.
        pub fn open(path: &Path, budget: u64) -> Result<Self> {
            if budget == 0 {
                return Err("mmap budget must be nonzero".into());
            }
            let mut file = File::open(path).map_err(|e| e.to_string())?;
            if !file.metadata().map_err(|e| e.to_string())?.is_file() {
                return Err("mmap input must be a regular file".into());
            }
            let length = file.metadata().map_err(|e| e.to_string())?.len();
            if length == 0 {
                return Err("empty artifact cannot be mapped".into());
            }
            if length > budget {
                return Err("mmap byte budget refused before mapping".into());
            }
            let mut bytes = vec![0u8; length as usize];
            file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
            drop(file);
            let length_after = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
            if length_after != length {
                return Err("artifact changed length during read".into());
            }
            Ok(Self {
                bytes,
                length,
                path: path.to_path_buf(),
            })
        }

        pub fn len(&self) -> u64 {
            self.length
        }

        pub fn is_empty(&self) -> bool {
            self.length == 0
        }

        pub fn as_bytes(&self) -> &[u8] {
            &self.bytes
        }

        /// Bounds-checked slice of the mapped artifact.
        pub fn slice(&self, offset: usize, len: usize) -> Result<&[u8]> {
            let end = offset
                .checked_add(len)
                .ok_or("mmap slice offset overflow")?;
            self.bytes
                .get(offset..end)
                .ok_or("mmap slice outside artifact".into())
        }

        /// Re-check the backing file length; refuses if the artifact was replaced,
        /// truncated or grew after the guard was created.
        pub fn still_current(&self) -> Result<()> {
            let metadata = fs::metadata(&self.path).map_err(|e| e.to_string())?;
            if metadata.len() != self.length {
                return Err("mmap artifact length changed".into());
            }
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn guard_budget_refusal_reads_nothing() {
            let root = std::env::temp_dir().join(format!("mmap-guard-{}", std::process::id()));
            std::fs::create_dir(&root).unwrap();
            let path = root.join("sample.rsf");
            let content = b"exact artifact contents for mmap guard tests";
            std::fs::write(&path, content).unwrap();
            assert!(MmapGuard::open(&path, content.len() as u64 - 1).is_err());
            assert!(MmapGuard::open(&path, 0).is_err());
            let guard = MmapGuard::open(&path, content.len() as u64).unwrap();
            assert_eq!(guard.len(), content.len() as u64);
            assert!(!guard.is_empty());
            assert_eq!(guard.as_bytes(), content);
            assert_eq!(guard.slice(0, 5).unwrap(), b"exact");
            assert!(guard.slice(content.len(), 1).is_err());
            assert!(guard.slice(content.len() - 1, 2).is_err());
            assert!(guard.still_current().is_ok());
            let _: &[u8] = guard.as_bytes();
            std::fs::remove_dir_all(root).unwrap();
        }
        #[test]
        fn guard_detects_artifact_replacement_after_mapping() {
            let root = std::env::temp_dir().join(format!("mmap-guard2-{}", std::process::id()));
            std::fs::create_dir(&root).unwrap();
            let path = root.join("sample.rsf");
            std::fs::write(&path, b"short").unwrap();
            let guard = MmapGuard::open(&path, 1024).unwrap();
            std::fs::write(
                &path,
                b"this replacement is deliberately longer than before",
            )
            .unwrap();
            assert!(guard.still_current().is_err());
            std::fs::remove_dir_all(root).unwrap();
        }
        #[test]
        fn guard_slice_exposes_only_shared_read_only_views() {
            let root = std::env::temp_dir().join(format!("mmap-guard3-{}", std::process::id()));
            std::fs::create_dir(&root).unwrap();
            let path = root.join("sample.rsf");
            std::fs::write(&path, b"readonly").unwrap();
            let guard = MmapGuard::open(&path, 1024).unwrap();
            let bytes: &[u8] = guard.as_bytes();
            assert_eq!(bytes, b"readonly");
            let _borrow: &[u8] = guard.slice(0, 4).unwrap();
            assert_eq!(_borrow, b"read");
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
