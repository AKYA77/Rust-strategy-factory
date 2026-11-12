use crate::{Result, contracts::Profile};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tick {
    pub timestamp_ms: i64,
    pub sequence: u64,
    pub bid: i64,
    pub ask: i64,
}
impl Tick {
    pub fn validate(&self, p: &Profile) -> Result<()> {
        if self.bid <= 0 || self.ask < self.bid || self.bid % p.tick != 0 || self.ask % p.tick != 0
        {
            Err("nonpositive, crossed or off-grid quote".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DataClass {
    SyntheticTestOnly,
    RealUnreviewed,
    RealStructurallyValid,
    RealQualityAccepted,
    RealProfileApproved,
    ResearchEligible,
    Quarantined,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub parser_version: String,
    pub source: String,
    pub imported_at: String,
    pub archive_hash: String,
    pub content_hash: String,
    pub profile_hash: String,
    pub symbol: String,
    pub class: DataClass,
    pub events: usize,
    pub first_ms: i64,
    pub last_ms: i64,
    pub timestamp_ties: usize,
    pub identical_quote_events: usize,
    pub gaps_over_hour: usize,
    pub issues: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset {
    pub manifest: Manifest,
    pub profile: Profile,
    pub ticks: Vec<Tick>,
}
#[derive(Deserialize)]
struct Row {
    timestamp_ms: i64,
    sequence: u64,
    bid: String,
    ask: String,
}

pub fn hash<T: Serialize>(value: &T) -> Result<String> {
    Ok(
        blake3::hash(&serde_json::to_vec(value).map_err(|e| e.to_string())?)
            .to_hex()
            .to_string(),
    )
}
pub fn file_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut h = blake3::Hasher::new();
    let mut buf = [0; 65536];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex().to_string())
}
// Hash the exact byte stream consumed by the parser, never reopen a mutable input path.
struct HashedRead<R> {
    inner: R,
    hasher: blake3::Hasher,
    bytes: u64,
}
impl<R: Read> Read for HashedRead<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..n]);
        self.bytes += n as u64;
        Ok(n)
    }
}
/// Strict normalized CSV importer. Never sorts, deletes duplicates, repairs or infers missing quotes.
/// REAL_STRUCTURALLY_VALID is the maximum automatically assigned real-data status.
pub fn import_csv(
    path: &Path,
    p: Profile,
    source: &str,
    synthetic: bool,
    max_bytes: u64,
    max_events: usize,
) -> Result<Dataset> {
    p.validate()?;
    if source.trim().is_empty() || max_events == 0 {
        return Err("source and event budget required".into());
    }
    let file = File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > max_bytes {
        return Err("source exceeds byte budget".into());
    }
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::None)
        .from_reader(HashedRead {
            inner: BufReader::new(file.take(max_bytes.saturating_add(1))),
            hasher: blake3::Hasher::new(),
            bytes: 0,
        });
    if reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .collect::<Vec<_>>()
        != ["timestamp_ms", "sequence", "bid", "ask"]
    {
        return Err("expected CSV header timestamp_ms,sequence,bid,ask".into());
    }
    let mut ticks = Vec::new();
    for row in reader.deserialize::<Row>() {
        if ticks.len() >= max_events {
            return Err("source exceeds event budget".into());
        }
        let r = row.map_err(|e| format!("row {}: {e}", ticks.len() + 2))?;
        ticks.push(Tick {
            timestamp_ms: r.timestamp_ms,
            sequence: r.sequence,
            bid: p.price(&r.bid)?,
            ask: p.price(&r.ask)?,
        });
    }
    let stream = reader.into_inner();
    if stream.bytes > max_bytes {
        return Err("source grew beyond byte budget during import".into());
    }
    dataset(
        p,
        ticks,
        source,
        stream.hasher.finalize().to_hex().to_string(),
        synthetic,
    )
}
pub fn dataset(
    p: Profile,
    ticks: Vec<Tick>,
    source: &str,
    archive_hash: String,
    synthetic: bool,
) -> Result<Dataset> {
    p.validate()?;
    if ticks.is_empty() {
        return Err("empty dataset".into());
    }
    let mut issues = Vec::new();
    let mut ties = 0;
    let mut repeats = 0;
    let mut gaps = 0;
    for (i, tick) in ticks.iter().enumerate() {
        if let Err(e) = tick.validate(&p)
            && issues.len() < 100
        {
            issues.push(format!("event {i}: {e}"));
        }
        if chrono::DateTime::from_timestamp_millis(tick.timestamp_ms).is_none()
            && issues.len() < 100
        {
            issues.push(format!("event {i}: timestamp outside supported calendar"));
        }
        if i > 0 {
            let prev = ticks[i - 1];
            if (tick.timestamp_ms < prev.timestamp_ms || tick.sequence <= prev.sequence)
                && issues.len() < 100
            {
                issues.push(format!(
                    "event {i}: nonmonotonic timestamp or nonunique sequence"
                ));
            }
            ties += usize::from(tick.timestamp_ms == prev.timestamp_ms);
            repeats += usize::from(tick.bid == prev.bid && tick.ask == prev.ask);
            gaps += usize::from(
                i128::from(tick.timestamp_ms) - i128::from(prev.timestamp_ms) > 3_600_000,
            );
        }
    }
    let class = if !issues.is_empty() {
        DataClass::Quarantined
    } else if synthetic {
        DataClass::SyntheticTestOnly
    } else {
        DataClass::RealStructurallyValid
    };
    let manifest = Manifest {
        parser_version: "normalized-csv-v1".into(),
        source: source.into(),
        imported_at: chrono::Utc::now().to_rfc3339(),
        archive_hash,
        content_hash: hash(&ticks)?,
        profile_hash: hash(&p)?,
        symbol: p.symbol.clone(),
        class,
        events: ticks.len(),
        first_ms: ticks[0].timestamp_ms,
        last_ms: ticks[ticks.len() - 1].timestamp_ms,
        timestamp_ties: ties,
        identical_quote_events: repeats,
        gaps_over_hour: gaps,
        issues,
    };
    Ok(Dataset {
        manifest,
        profile: p,
        ticks,
    })
}
impl Dataset {
    pub fn verify(&self) -> Result<()> {
        self.profile.validate()?;
        if self.manifest.class == DataClass::Quarantined || self.ticks.is_empty() {
            return Err("quarantined or empty dataset".into());
        }
        if self.manifest.content_hash != hash(&self.ticks)?
            || self.manifest.profile_hash != hash(&self.profile)?
            || self.manifest.symbol != self.profile.symbol
            || self.manifest.events != self.ticks.len()
            || self.manifest.first_ms != self.ticks[0].timestamp_ms
            || self.manifest.last_ms != self.ticks[self.ticks.len() - 1].timestamp_ms
        {
            return Err("manifest integrity mismatch".into());
        }
        for (i, t) in self.ticks.iter().enumerate() {
            t.validate(&self.profile)?;
            if chrono::DateTime::from_timestamp_millis(t.timestamp_ms).is_none() {
                return Err("unsupported calendar timestamp".into());
            }
            if i > 0
                && (t.timestamp_ms < self.ticks[i - 1].timestamp_ms
                    || t.sequence <= self.ticks[i - 1].sequence)
            {
                return Err("event order integrity mismatch".into());
            }
        }
        Ok(())
    }
}
