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
const SPEC_CAP: u64 = 16 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub path: String,
    pub dataset_identity: String,
    pub file_hash: String,
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
        if self.version != VERSION
            || self.chunks.is_empty()
            || self.chunks.len() > 10000
            || !(1..=100000).contains(&self.max_chunk_events)
        {
            return Err("invalid corpus version/chunk/event budget".into());
        }
        for c in &self.chunks {
            if c.path.is_empty()
                || c.path.len() > 4096
                || !crate::machine_evidence::hex(&c.dataset_identity)
                || !crate::machine_evidence::hex(&c.file_hash)
            {
                return Err("invalid corpus artifact identity/path".into());
            }
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
struct SequenceHash {
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
    fn new() -> Self {
        let mut hash = blake3::Hasher::new();
        hash.update(b"[");
        Self { hash, count: 0 }
    }
    fn push<T: Serialize>(&mut self, value: &T) -> Result<()> {
        if self.count > 0 {
            self.hash.update(b",");
        }
        serde_json::to_writer(HashWriter(&mut self.hash), value).map_err(|e| e.to_string())?;
        self.count = self.count.checked_add(1).ok_or("corpus count overflow")?;
        Ok(())
    }
    fn finish(mut self) -> String {
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
    pub candles: Vec<CandleSummary>,
    pub limitations: &'static str,
}
pub fn inspect(base: &Path, spec: &Specification) -> Result<Report> {
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
    visit_chunks(base, spec, |_, d| {
        if profile.is_none() {
            for tf in Timeframe::ALL {
                streams.push(candles::Stream::new(d.profile.clone(), tf)?);
            }
            profile = Some(d.profile.clone());
            source = Some(d.manifest.source.clone());
            class = Some(d.manifest.class);
        }
        peak = peak.max(d.ticks.len());
        for tick in &d.ticks {
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
            for (stream, hash) in streams.iter_mut().zip(&mut candle_hashes) {
                if let Some(c) = stream.push(*tick)? {
                    hash.push(&c)?;
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
    Ok(Report {
        version: VERSION,
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
        candles,
        limitations: "Observed ordered native chunks and closed UTC candle fingerprints only. One bounded quote chunk plus seven candle states is retained; metadata and per-chunk validation allocations are additional. No empty/final unfinished candles, missing quotes, source completeness, approved class, acquisition/resume, scientific eligibility or strategy/replay streaming is established. Source archives/provenance remain separate immutable artifacts; profiles/source/class cannot be mixed.",
    })
}

/// Describe existing immutable artifacts without copying them. The caller can
/// inspect the result and publish it with the normal no-replace artifact writer.
pub fn describe(paths: &[std::path::PathBuf], max_chunk_events: usize) -> Result<Specification> {
    if paths.is_empty() || paths.len() > 10000 || !(1..=100000).contains(&max_chunk_events) {
        return Err("invalid corpus chunk budget".into());
    }
    let mut chunks = Vec::new();
    for path in paths {
        let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        let (d, file_hash) = native::read_bounded(&path, max_chunk_events)?;
        chunks.push(Artifact {
            path: path.to_str().ok_or("non-UTF8 corpus path")?.into(),
            dataset_identity: data::dataset_identity(&d.manifest)?,
            file_hash,
        });
    }
    let spec = Specification {
        version: VERSION.into(),
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
        let (dataset, fingerprint) =
            native::read_bounded(&base.join(&chunk.path), spec.max_chunk_events)?;
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
    visit_chunks(base, spec, |_, dataset| {
        if stream.is_none() {
            stream = Some(candles::Stream::new(dataset.profile.clone(), timeframe)?);
        }
        let stream = stream.as_mut().ok_or("candle stream unavailable")?;
        for tick in &dataset.ticks {
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

/// Compile bounded causal primary-frame signals using the existing feature
/// engine. Higher-timeframe rules still require verified multi-frame context
/// and are refused by Features::new rather than silently approximated.
pub fn signals(
    base: &Path,
    spec: &Specification,
    strategy: &crate::strategies::Strategy,
    max_candles: usize,
    feature_cells: usize,
) -> Result<crate::indicators::Mask> {
    let strategy = strategy.canonical()?;
    let candles = candle_context(base, spec, strategy.timeframe, max_candles)?;
    crate::strategies::Features::new(&candles, feature_cells)?.signal(&strategy.rule)
}
