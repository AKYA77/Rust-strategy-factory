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
    fs::File,
    io::{Read, Write},
    path::Path,
};
const VERSION: &str = "ordered-native-corpus-v1";
const BLOCK_VERSION: &str = "ordered-block-corpus-v2";
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
#[derive(Debug, Serialize)]
pub struct CandleSummary {
    pub timeframe: Timeframe,
    pub closed_candles: u64,
    pub content_hash: String,
}
#[derive(Debug, Serialize)]
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
/// Verified immutable candle contexts built alongside inspection in one source pass.
/// This is a bounded in-memory snapshot, not current eligibility or a resume proof.
pub struct Prepared {
    report: Report,
    frames: std::collections::BTreeMap<Timeframe, Vec<candles::Candle>>,
}
impl Prepared {
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
    if !(1..=1000000).contains(&max_candles) {
        return Err("invalid aggregate corpus candle budget".into());
    }
    inspect_prepared(base, spec, Some((primary, max_candles)))
}
pub fn inspect(base: &Path, spec: &Specification) -> Result<Report> {
    Ok(inspect_prepared(base, spec, None)?.report)
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
    visit_blocks(base, spec, |d| {
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
    if max_candles == 0 || max_candles > 1_000_000 {
        return Err("corpus candle context budget must be1..1000000".into());
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
fn signals_internal(
    base: &Path,
    spec: &Specification,
    strategy: &crate::strategies::Strategy,
    max_candles: usize,
    feature_cells: usize,
    window: Option<&crate::validation::Interval>,
) -> Result<crate::indicators::Mask> {
    let strategy = strategy.canonical()?;
    if max_candles == 0 || max_candles > 1_000_000 {
        return Err("invalid aggregate corpus candle budget".into());
    }
    let mut frames = std::collections::BTreeMap::new();
    let mut streams = Vec::new();
    let mut total = 0;
    visit_blocks(base, spec, |dataset| {
        if streams.is_empty() {
            for tf in Timeframe::ALL {
                if tf.millis() >= strategy.timeframe.millis() {
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
    let candles = frames
        .get(&strategy.timeframe)
        .ok_or("primary frame missing")?;
    crate::strategies::Features::with_frames(candles, feature_cells, &frames, strategy.timeframe)?
        .signal(&strategy.rule)
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
    mut visit: impl FnMut(QuoteBlock<'_>) -> Result<()>,
) -> Result<()> {
    spec.identity()?;
    let mut context = None;
    let mut previous = None;
    for (chunk_index, chunk) in spec.chunks.iter().enumerate() {
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
