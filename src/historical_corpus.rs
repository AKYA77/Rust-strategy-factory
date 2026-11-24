//! Explicit multi-month historical corpus over independently verified monthly native sources.
//!
//! This module registers and traverses artifacts that already exist; it does not create a
//! second replay engine and does not copy chunk data. Every month keeps its own source
//! identity and provenance; the aggregate identity is a deterministic hash over those source
//! identities, never a source-approval claim.

use crate::{
    Result,
    contracts::{self, Profile},
    data::{self, Tick},
    historical_migration::{self, Partition},
    native,
};
use chrono::Datelike;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const VERSION: &str = "historical-monthly-corpus-v2";
/// The owner-selected active research period: exactly the 22 completed months 2025-01 through
/// 2026-10 for the eight canonical instruments (owner decision 2026-11-19). Derived from
/// [`ResearchWindow::OWNER_SELECTED`] so the month count and the calendar pin cannot drift apart.
pub const EXPECTED_MONTHS: usize = ResearchWindow::OWNER_SELECTED.span() as usize;
const LIMITATIONS: &str = "Aggregate multi-month corpus over independently verified monthly native sources. Aggregate identity is not source approval: no calendar completeness, broker/profile approval, licensing or scientific eligibility is established. Month boundaries are explicit; per-month source identities and provenance remain distinct. Sequence numbers restart at each month and are not globally increasing across months.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonthlySource {
    pub instrument: String,
    pub year: u32,
    pub month: u32,
    pub dataset_hash: String,
    pub source_sha256: String,
    pub parquet_sha256: String,
    pub rows: u64,
    pub first_ms: i64,
    pub last_ms: i64,
    pub output_dir: String,
    pub parquet_path: String,
    pub chunk_files: Vec<String>,
    pub chunk_hashes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoricalCorpus {
    pub version: String,
    pub instrument: String,
    pub profile: Profile,
    pub months: Vec<MonthlySource>,
    pub identity: String,
    pub limitations: String,
}

const fn month_index(year: u32, month: u32) -> u64 {
    year as u64 * 12 + month as u64
}

/// An explicit inclusive window of completed calendar months.
///
/// This is the configuration surface for the historical research period. The owner-selected
/// active window is [`ResearchWindow::OWNER_SELECTED`]; any other exact interval can be supplied
/// as JSON through the `historical-corpus-register-window-config` CLI. A window is a calendar
/// pin, not a sliding "latest N months" selection: registration resolves exactly these months
/// from the canonical corpus and refuses with the exact missing months when any of them is not
/// a completed verified partition. No window is ever padded, silently truncated or backfilled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchWindow {
    pub start_year: u32,
    pub start_month: u32,
    pub end_year: u32,
    pub end_month: u32,
}

impl ResearchWindow {
    /// Owner-selected active research period (owner decision 2026-11-19, final for the current
    /// research configuration): exactly 2025-01-01 through 2026-10-31, the 22 completed months
    /// present in the structurally verified canonical corpus (8 instruments, 176 partitions).
    /// 2026-11 and 2026-12 are not completed/available and are not part of the active window;
    /// no download or backfill is performed to extend it.
    pub const OWNER_SELECTED: Self = Self {
        start_year: 2025,
        start_month: 1,
        end_year: 2026,
        end_month: 10,
    };

    /// Reject invalid calendar fields and reversed windows before any selection arithmetic.
    pub fn validate(&self) -> Result<()> {
        for (year, month) in [
            (self.start_year, self.start_month),
            (self.end_year, self.end_month),
        ] {
            if !(2000..=2200).contains(&year) || !(1..=12).contains(&month) {
                return Err(format!(
                    "invalid research window calendar month {year:04}-{month:02}"
                ));
            }
        }
        if month_index(self.start_year, self.start_month)
            > month_index(self.end_year, self.end_month)
        {
            return Err("research window start must not be after its end".into());
        }
        Ok(())
    }

    /// Inclusive month count as raw arithmetic. Only meaningful for a validated window (start
    /// not after end); the owner-selected constant is valid by construction.
    pub const fn span(&self) -> u64 {
        month_index(self.end_year, self.end_month) - month_index(self.start_year, self.start_month)
            + 1
    }

    /// Validated inclusive month count.
    pub fn month_count(&self) -> Result<usize> {
        self.validate()?;
        Ok(self.span() as usize)
    }

    /// Whether a calendar month lies inside the inclusive window.
    pub fn contains(&self, year: u32, month: u32) -> bool {
        let index = month_index(year, month);
        month_index(self.start_year, self.start_month) <= index
            && index <= month_index(self.end_year, self.end_month)
    }

    /// Every calendar month of the window in chronological order, validated.
    pub fn months(&self) -> Result<Vec<(u32, u32)>> {
        self.validate()?;
        let mut months = Vec::with_capacity(self.span() as usize);
        let (mut year, mut month) = (self.start_year, self.start_month);
        loop {
            months.push((year, month));
            if (year, month) == (self.end_year, self.end_month) {
                break;
            }
            month += 1;
            if month > 12 {
                month = 1;
                year += 1;
            }
        }
        Ok(months)
    }

    /// Human-readable label for receipts and refusal messages.
    pub fn label(&self) -> String {
        format!(
            "{}-{:02}..{}-{:02}",
            self.start_year, self.start_month, self.end_year, self.end_month
        )
    }
}

/// Exact source binding: the chunk's source description must contain the month's dataset
/// hash as a whole field, not as a substring of a longer value.
fn source_binds(description: &str, dataset_hash: &str) -> bool {
    description.split(':').any(|part| part == dataset_hash)
}

/// Refuse missing, overlapping, ambiguous or non-contiguous months.
pub fn validate_months(months: &[MonthlySource], expected: usize) -> Result<()> {
    if months.is_empty() || months.len() != expected {
        return Err(format!(
            "expected {expected} contiguous monthly partitions, found {}",
            months.len()
        ));
    }
    let mut previous: Option<&MonthlySource> = None;
    for month in months {
        if !(2000..=2200).contains(&month.year) || !(1..=12).contains(&month.month) {
            return Err("invalid calendar month".into());
        }
        if month.rows == 0
            || month.first_ms > month.last_ms
            || month.chunk_files.is_empty()
            || month.chunk_files.len() != month.chunk_hashes.len()
        {
            return Err("invalid monthly source boundaries or chunk inventory".into());
        }
        if let Some(previous) = previous {
            if month_index(month.year, month.month)
                != month_index(previous.year, previous.month) + 1
            {
                return Err("monthly partitions are not contiguous".into());
            }
            if month.first_ms < previous.last_ms {
                return Err("monthly partitions overlap or are out of order".into());
            }
        }
        previous = Some(month);
    }
    Ok(())
}

/// Canonical partitions for one instrument, excluding the seven noncanonical records.
pub fn plan_instrument(root: &Path, instrument: &str) -> Result<Vec<Partition>> {
    let mut partitions: Vec<Partition> = historical_migration::plan(root)?
        .into_iter()
        .filter(|p| p.instrument == instrument)
        .collect();
    partitions.sort_by_key(|p| (p.year, p.month));
    Ok(partitions)
}

pub fn identity(instrument: &str, profile: &Profile, months: &[MonthlySource]) -> Result<String> {
    data::hash(&(
        VERSION,
        instrument,
        profile,
        months
            .iter()
            .map(|m| {
                (
                    &m.instrument,
                    m.year,
                    m.month,
                    &m.dataset_hash,
                    &m.source_sha256,
                    &m.parquet_sha256,
                    m.rows,
                    m.first_ms,
                    m.last_ms,
                    &m.parquet_path,
                    &m.output_dir,
                    &m.chunk_files,
                    &m.chunk_hashes,
                )
            })
            .collect::<Vec<_>>(),
    ))
}

/// Register one instrument's verified monthly sources into an aggregate specification.
/// Missing or mismatched monthly output is refused; no chunk data is copied.
pub fn register(
    root: &Path,
    output_root: &Path,
    instrument: &str,
    expected: usize,
) -> Result<HistoricalCorpus> {
    let partitions = plan_instrument(root, instrument)?;
    if partitions.len() != expected {
        return Err(format!(
            "canonical plan has {} partitions for {instrument}, expected {expected}",
            partitions.len()
        ));
    }
    register_partitions(output_root, instrument, &partitions)
}

/// Register the most recent `months` contiguous completed monthly partitions for one instrument.
///
/// This is the configurable completed-month mechanism required by the master blueprint: an
/// exact interval is expressible without padding or fabrication, and a window longer than the
/// available completed months reports the precise shortfall. The owner-selected active research
/// period is the explicit calendar pin [`ResearchWindow::OWNER_SELECTED`] (22 months,
/// 2025-01..2026-10; owner decision 2026-11-19), registered by [`register_research_window`];
/// a 24-month window is refused while 2026-11 and 2026-12 are not completed/available. The
/// selection here is the latest contiguous run, so a window never spans a gap.
pub fn register_window(
    root: &Path,
    output_root: &Path,
    instrument: &str,
    months: usize,
) -> Result<HistoricalCorpus> {
    if months == 0 {
        return Err("historical window must be at least one completed month".into());
    }
    let partitions = plan_instrument(root, instrument)?;
    if partitions.len() < months {
        return Err(format!(
            "only {} completed monthly partitions are available for {instrument}; a {months}-month window cannot be formed",
            partitions.len()
        ));
    }
    let start = partitions.len() - months;
    register_partitions(output_root, instrument, &partitions[start..])
}

/// Register exactly the months of an explicit research window for one instrument.
///
/// Every calendar month of the window must exist as a canonical completed partition; missing
/// months are refused with the exact list instead of padding, silent truncation or fabrication.
/// Canonical partitions outside the window are not selected, so later completed months never
/// slide an explicit pin. The owner-selected active period is [`ResearchWindow::OWNER_SELECTED`]
/// (2025-01..2026-10, 22 completed months); a window extending into 2026-11/2026-12 is refused
/// because those months are not completed/available, and no data is downloaded to satisfy it.
pub fn register_research_window(
    root: &Path,
    output_root: &Path,
    instrument: &str,
    window: &ResearchWindow,
) -> Result<HistoricalCorpus> {
    let required = window.months()?;
    let partitions = plan_instrument(root, instrument)?;
    let selected: Vec<Partition> = partitions
        .into_iter()
        .filter(|partition| window.contains(partition.year, partition.month))
        .collect();
    let missing: Vec<String> = required
        .iter()
        .filter(|(year, month)| {
            !selected
                .iter()
                .any(|partition| partition.year == *year && partition.month == *month)
        })
        .map(|(year, month)| format!("{year:04}-{month:02}"))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "research window {} requires {} completed months for {instrument}; only {} available; missing: {}",
            window.label(),
            required.len(),
            selected.len(),
            missing.join(", ")
        ));
    }
    register_partitions(output_root, instrument, &selected)
}

fn register_partitions(
    output_root: &Path,
    instrument: &str,
    partitions: &[Partition],
) -> Result<HistoricalCorpus> {
    let mut months = Vec::with_capacity(partitions.len());
    let mut profile: Option<Profile> = None;
    for partition in partitions {
        let dir = output_root.join(format!(
            "{}-{year:04}{month:02}",
            partition.instrument,
            year = partition.year,
            month = partition.month
        ));
        let receipt = historical_migration::verify_output(&dir, None)?;
        if receipt.source.dataset_hash != partition.dataset_hash
            || receipt.source.source_sha256 != partition.source_sha256
            || receipt.source.rows != partition.rows
            || receipt.source.instrument != instrument
            || receipt.source.parquet_path != partition.parquet_path
            || receipt.profile.symbol != instrument
        {
            return Err("monthly migration receipt does not match the canonical plan".into());
        }
        let expected_profile = profile.get_or_insert_with(|| receipt.profile.clone());
        if *expected_profile != receipt.profile {
            return Err("monthly sources do not share one profile".into());
        }
        months.push(MonthlySource {
            instrument: instrument.into(),
            year: partition.year,
            month: partition.month,
            dataset_hash: receipt.source.dataset_hash.clone(),
            source_sha256: receipt.source.source_sha256.clone(),
            parquet_sha256: receipt.source.parquet_sha256.clone(),
            rows: receipt.events,
            first_ms: receipt.first_ms,
            last_ms: receipt.last_ms,
            output_dir: dir.to_string_lossy().to_string(),
            parquet_path: partition.parquet_path.clone(),
            chunk_files: receipt.chunks.iter().map(|c| c.file.clone()).collect(),
            chunk_hashes: receipt
                .chunks
                .iter()
                .map(|c| c.dataset_hash.clone())
                .collect(),
        });
    }
    let profile = profile.ok_or("no monthly sources")?;
    validate_months(&months, partitions.len())?;
    let identity = identity(instrument, &profile, &months)?;
    Ok(HistoricalCorpus {
        version: VERSION.into(),
        instrument: instrument.into(),
        profile,
        months,
        identity,
        limitations: LIMITATIONS.into(),
    })
}

/// Re-verify every monthly receipt and chunk against the specification.
pub fn verify(spec: &HistoricalCorpus) -> Result<()> {
    if spec.version != VERSION
        || spec.instrument != spec.profile.symbol
        || spec.months.is_empty()
        || spec.identity != identity(&spec.instrument, &spec.profile, &spec.months)?
        || spec.limitations != LIMITATIONS
    {
        return Err("invalid historical corpus specification".into());
    }
    validate_months(&spec.months, EXPECTED_MONTHS)?;
    // A full-length specification must cover exactly the owner-selected research window, not
    // merely any contiguous run of the same length.
    let window = ResearchWindow::OWNER_SELECTED;
    let first = &spec.months[0];
    let last = &spec.months[spec.months.len() - 1];
    if (first.year, first.month) != (window.start_year, window.start_month)
        || (last.year, last.month) != (window.end_year, window.end_month)
    {
        return Err(format!(
            "historical corpus does not cover the owner-selected research window {}",
            window.label()
        ));
    }
    for month in &spec.months {
        if month.instrument != spec.instrument {
            return Err("historical corpus month instrument mismatch".into());
        }
        let dir = Path::new(&month.output_dir);
        let receipt = historical_migration::verify_output(dir, None)?;
        if receipt.source.dataset_hash != month.dataset_hash
            || receipt.source.source_sha256 != month.source_sha256
            || receipt.source.parquet_sha256 != month.parquet_sha256
            || receipt.source.parquet_path != month.parquet_path
            || receipt.source.rows != month.rows
            || receipt.first_ms != month.first_ms
            || receipt.last_ms != month.last_ms
            || receipt.profile != spec.profile
        {
            return Err("historical corpus month receipt mismatch".into());
        }
        if receipt.chunks.len() != month.chunk_files.len()
            || receipt
                .chunks
                .iter()
                .map(|c| &c.file)
                .ne(month.chunk_files.iter())
            || receipt
                .chunks
                .iter()
                .map(|c| &c.dataset_hash)
                .ne(month.chunk_hashes.iter())
        {
            return Err("historical corpus chunk inventory mismatch".into());
        }
        for (file, hash) in month.chunk_files.iter().zip(&month.chunk_hashes) {
            let dataset = native::read(&dir.join(file))?;
            if data::hash(&dataset)? != *hash {
                return Err("historical corpus chunk hash mismatch".into());
            }
        }
    }
    Ok(())
}

/// Source-bound verification: every monthly chunk must equal the canonical legacy Parquet
/// prefix, not just its own receipt. Expensive; use for acceptance, not repeated scans.
pub fn verify_bound(spec: &HistoricalCorpus) -> Result<()> {
    verify(spec)?;
    for month in &spec.months {
        let receipt = historical_migration::verify_output(
            Path::new(&month.output_dir),
            Some(Path::new(&month.parquet_path)),
        )?;
        if receipt.source.dataset_hash != month.dataset_hash || receipt.source.rows != month.rows {
            return Err("historical corpus source-bound receipt mismatch".into());
        }
    }
    Ok(())
}

/// Traverse one month's chunks, preserving its source identity and sequence space.
pub fn visit_month<F: FnMut(&data::Dataset) -> Result<()>>(
    spec: &HistoricalCorpus,
    index: usize,
    mut visit: F,
) -> Result<()> {
    let month = spec
        .months
        .get(index)
        .ok_or("historical corpus month index out of range")?;
    if month.chunk_files.len() != month.chunk_hashes.len() {
        return Err("historical corpus chunk inventory mismatch".into());
    }
    let dir = Path::new(&month.output_dir);
    for (file, hash) in month.chunk_files.iter().zip(&month.chunk_hashes) {
        let dataset = native::read(&dir.join(file))?;
        if data::hash(&dataset)? != *hash || dataset.profile != spec.profile {
            return Err("historical corpus chunk mismatch during traversal".into());
        }
        visit(&dataset)?;
    }
    Ok(())
}

/// Traverse months in order, preserving each month's chunks and source identity.
/// Sequence numbers restart at each month; a consumer needing one global sequence must rebase
/// explicitly and record that transformation.
pub fn visit_months<F: FnMut(usize, &data::Dataset) -> Result<()>>(
    spec: &HistoricalCorpus,
    mut visit: F,
) -> Result<()> {
    if spec.identity != identity(&spec.instrument, &spec.profile, &spec.months)? {
        return Err("historical corpus identity mismatch".into());
    }
    let mut previous: Option<Tick> = None;
    for (index, month) in spec.months.iter().enumerate() {
        let dir = Path::new(&month.output_dir);
        for (file, hash) in month.chunk_files.iter().zip(&month.chunk_hashes) {
            let dataset = native::read(&dir.join(file))?;
            if data::hash(&dataset)? != *hash || dataset.profile != spec.profile {
                return Err("historical corpus chunk mismatch during traversal".into());
            }
            let first = dataset.ticks[0];
            if let Some(previous) = previous
                && first.timestamp_ms < previous.timestamp_ms
            {
                return Err("historical corpus cross-month timestamp order invalid".into());
            }
            previous = Some(dataset.ticks[dataset.ticks.len() - 1]);
            visit(index, &dataset)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonthAudit {
    pub year: u32,
    pub month: u32,
    pub chunks: usize,
    pub rows: u64,
    pub candles: u64,
    pub first_ms: i64,
    pub last_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditReport {
    pub instrument: String,
    pub identity: String,
    pub months: usize,
    pub chunks: usize,
    pub rows: u64,
    pub candles: u64,
    pub first_ms: i64,
    pub last_ms: i64,
    pub per_month: Vec<MonthAudit>,
    pub limitations: String,
}

/// Stream the whole corpus with bounded memory, verifying source attribution, per-month
/// sequence continuity, cross-month ordering and causal candle generation.
pub fn audit(spec: &HistoricalCorpus) -> Result<AuditReport> {
    verify(spec)?;
    let mut per_month = Vec::with_capacity(spec.months.len());
    let mut rows = 0u64;
    let mut candles = 0u64;
    let mut chunks = 0usize;
    let mut previous_last: Option<i64> = None;
    for month in &spec.months {
        let mut expected_sequence = 0u64;
        let mut month_rows = 0u64;
        let mut month_candles = 0u64;
        let mut month_chunks = 0usize;
        let mut month_first: Option<i64> = None;
        let mut month_last: Option<i64> = None;
        let dir = Path::new(&month.output_dir);
        for (file, hash) in month.chunk_files.iter().zip(&month.chunk_hashes) {
            let dataset = native::read(&dir.join(file))?;
            if data::hash(&dataset)? != *hash || dataset.profile != spec.profile {
                return Err("historical corpus chunk mismatch during audit".into());
            }
            if !source_binds(&dataset.manifest.source, &month.dataset_hash) {
                return Err("historical corpus source attribution mismatch during audit".into());
            }
            if dataset.ticks[0].sequence != expected_sequence {
                return Err("historical corpus sequence continuity mismatch during audit".into());
            }
            expected_sequence += dataset.ticks.len() as u64;
            if let Some(previous) = previous_last
                && dataset.ticks[0].timestamp_ms < previous
            {
                return Err("historical corpus cross-month timestamp order invalid".into());
            }
            let first_ms = dataset.ticks[0].timestamp_ms;
            let last_ms = dataset.ticks[dataset.ticks.len() - 1].timestamp_ms;
            month_first = Some(month_first.map_or(first_ms, |v| v.min(first_ms)));
            month_last = Some(month_last.map_or(last_ms, |v| v.max(last_ms)));
            previous_last = Some(last_ms);
            let built = crate::candles::build(&dataset, crate::candles::Timeframe::M1)?;
            month_candles += built.len() as u64;
            month_rows += dataset.ticks.len() as u64;
            month_chunks += 1;
        }
        if month_rows != month.rows {
            return Err("historical corpus month row count mismatch during audit".into());
        }
        rows += month_rows;
        candles += month_candles;
        chunks += month_chunks;
        per_month.push(MonthAudit {
            year: month.year,
            month: month.month,
            chunks: month_chunks,
            rows: month_rows,
            candles: month_candles,
            first_ms: month_first.unwrap_or(0),
            last_ms: month_last.unwrap_or(0),
        });
    }
    let expected_rows: u64 = spec.months.iter().map(|m| m.rows).sum();
    if rows != expected_rows {
        return Err("historical corpus total row count mismatch during audit".into());
    }
    Ok(AuditReport {
        instrument: spec.instrument.clone(),
        identity: spec.identity.clone(),
        months: spec.months.len(),
        chunks,
        rows,
        candles,
        first_ms: per_month.first().map(|m| m.first_ms).unwrap_or(0),
        last_ms: per_month.last().map(|m| m.last_ms).unwrap_or(0),
        per_month,
        limitations: LIMITATIONS.into(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonthStudy {
    pub year: u32,
    pub month: u32,
    pub corpus_identity: String,
    pub events: u64,
    pub candles: usize,
    pub signals: usize,
    pub first_ms: i64,
    pub last_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudyReport {
    pub instrument: String,
    pub identity: String,
    pub strategy_identity: String,
    pub months: Vec<MonthStudy>,
    pub events: u64,
    pub candles: usize,
    pub signals: usize,
    pub limitations: String,
}

/// Bounded structural study of one instrument's real historical corpus: per month, build the
/// native corpus specification, verify it, and compute observed facts, closed candles and
/// causal signals. No account replay and no eligibility grant.
pub fn study(
    spec: &HistoricalCorpus,
    strategy: &crate::strategies::Strategy,
    max_candles: usize,
    feature_cells: usize,
) -> Result<StudyReport> {
    let mut months = Vec::with_capacity(spec.months.len());
    let mut events = 0u64;
    let mut candles = 0usize;
    let mut signals = 0usize;
    for month in &spec.months {
        let dir = Path::new(&month.output_dir);
        let chunks: Vec<std::path::PathBuf> = month
            .chunk_files
            .iter()
            .map(|file| dir.join(file))
            .collect();
        let corpus_spec =
            crate::corpus::describe(&chunks, crate::historical_migration::MAX_CHUNK_EVENTS)?;
        crate::corpus::inspect(dir, &corpus_spec)?;
        let facts = crate::corpus::source_facts(dir, &corpus_spec)?;
        let candles_vec =
            crate::corpus::candle_context(dir, &corpus_spec, strategy.timeframe, max_candles)?;
        let mask = crate::corpus::signals(dir, &corpus_spec, strategy, max_candles, feature_cells)?;
        events += facts.events();
        candles += candles_vec.len();
        signals += mask.count();
        months.push(MonthStudy {
            year: month.year,
            month: month.month,
            corpus_identity: corpus_spec.identity()?,
            events: facts.events(),
            candles: candles_vec.len(),
            signals: mask.count(),
            first_ms: facts.first().timestamp_ms,
            last_ms: facts.latest().timestamp_ms,
        });
    }
    Ok(StudyReport {
        instrument: spec.instrument.clone(),
        identity: spec.identity.clone(),
        strategy_identity: strategy.identity()?,
        months,
        events,
        candles,
        signals,
        limitations: LIMITATIONS.into(),
    })
}

/// Derived, diagnostic-only trade outcome statistics for one executed month.
///
/// These are descriptive observations computed from the same closed-trade list the engine
/// already produced. They change no financial rule, grant no eligibility and are never
/// research evidence. They exist so the owner-approved numerical criteria (net win rate,
/// consecutive-loss streak, achieved reward/risk) can be reported alongside the execution
/// totals instead of being uncomputable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TradeOutcomeStats {
    /// Closed trades; equals the engine's `trades` length for the month.
    pub resolved: usize,
    pub wins: usize,
    pub losses: usize,
    pub breakevens: usize,
    /// Longest run of consecutive net-losing closed trades inside this month. A net winner
    /// resets the streak and a breakeven does not, matching `validation::assess`. A diagnostic
    /// month starts a fresh account, so this streak resets at every month boundary and is a
    /// reset-fold streak, not a continuous full-period streak.
    pub max_losing_streak: usize,
    /// Consecutive net-losing closed trades at the end of the month. Zero when the last closed
    /// trade was a win or breakeven, or when there are no closed trades. Used to chain losing
    /// streaks across month boundaries in continuous full-period accounting.
    pub ending_streak: usize,
    /// Sum of positive closed-trade net P&L. Additive.
    pub gross_win_micro_usd: i128,
    /// Sum of the magnitudes of negative closed-trade net P&L. Additive; never negative.
    pub gross_loss_micro_usd: i128,
    /// Sum of closed-trade net P&L. Excludes any unresolved end-of-month position.
    pub net_micro_usd: i128,
    /// `1` when the month ended holding an unresolved position, else `0`. Unresolved positions
    /// are not closed trades and are never counted as wins or losses.
    pub unresolved_positions: usize,
}

/// Compute [`TradeOutcomeStats`] from a month's closed trades. Checked arithmetic: an
/// overflowing aggregate is an error, never a wrapped or silent value.
pub fn trade_outcome_stats(
    trades: &[crate::tick_engine::Trade],
    unresolved: bool,
) -> Result<TradeOutcomeStats> {
    let mut stats = TradeOutcomeStats {
        resolved: trades.len(),
        unresolved_positions: usize::from(unresolved),
        ..TradeOutcomeStats::default()
    };
    let mut streak = 0usize;
    for trade in trades {
        if trade.net_micro_usd > 0 {
            stats.wins += 1;
            streak = 0;
            stats.gross_win_micro_usd = stats
                .gross_win_micro_usd
                .checked_add(trade.net_micro_usd)
                .ok_or("trade outcome gross win overflow")?;
        } else if trade.net_micro_usd < 0 {
            stats.losses += 1;
            streak += 1;
            stats.max_losing_streak = stats.max_losing_streak.max(streak);
            stats.gross_loss_micro_usd = stats
                .gross_loss_micro_usd
                .checked_add(
                    trade
                        .net_micro_usd
                        .checked_neg()
                        .ok_or("trade outcome loss magnitude overflow")?,
                )
                .ok_or("trade outcome gross loss overflow")?;
        } else {
            stats.breakevens += 1;
        }
        stats.net_micro_usd = stats
            .net_micro_usd
            .checked_add(trade.net_micro_usd)
            .ok_or("trade outcome net overflow")?;
    }
    stats.ending_streak = streak;
    Ok(stats)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusGroup {
    pub timeframe: crate::candles::Timeframe,
    pub evidence: crate::behavior::Evidence,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusDiversity {
    pub instrument: String,
    pub corpus_identity: String,
    pub months: usize,
    pub events: u64,
    /// One diversity measurement per primary timeframe; mixed-timeframe populations are
    /// never compared across unequal candle grids.
    pub groups: Vec<CorpusGroup>,
    pub limitations: String,
}

/// Bounded read-only behavioral diversity over a registered multi-month corpus: per month,
/// verify the month's inventory and compute causal decision vectors for the whole population
/// in one shared frame build, then concatenate chronologically and measure exact pairwise
/// distances per timeframe. No replay, no account state, no eligibility or qualification input.
pub fn diversity(
    spec: &HistoricalCorpus,
    strategies: &[crate::strategies::Strategy],
    max_candles: usize,
    feature_cells: usize,
) -> Result<CorpusDiversity> {
    if strategies.is_empty() {
        return Err("behavioral diversity requires at least one strategy".into());
    }
    let mut vectors: BTreeMap<crate::candles::Timeframe, Vec<(String, Vec<bool>)>> =
        BTreeMap::new();
    for strategy in strategies {
        let canonical = strategy.canonical()?;
        vectors
            .entry(canonical.timeframe)
            .or_default()
            .push((canonical.identity()?, Vec::new()));
    }
    let mut events = 0u64;
    let mut counters: BTreeMap<crate::candles::Timeframe, usize> = BTreeMap::new();
    for month in &spec.months {
        let dir = Path::new(&month.output_dir);
        let chunks: Vec<std::path::PathBuf> = month
            .chunk_files
            .iter()
            .map(|file| dir.join(file))
            .collect();
        let corpus_spec =
            crate::corpus::describe(&chunks, crate::historical_migration::MAX_CHUNK_EVENTS)?;
        crate::corpus::inspect(dir, &corpus_spec)?;
        let facts = crate::corpus::source_facts(dir, &corpus_spec)?;
        let masks =
            crate::corpus::signals_many(dir, &corpus_spec, strategies, max_candles, feature_cells)?;
        events += facts.events();
        for (strategy, mask) in strategies.iter().zip(masks.iter()) {
            let canonical = strategy.canonical()?;
            let len = mask.len();
            let mut dense = vec![false; len];
            for index in mask.set_indices()? {
                dense[index] = true;
            }
            let timeframe = canonical.timeframe;
            let group = vectors
                .get_mut(&timeframe)
                .ok_or("behavioral group missing")?;
            let index = counters.entry(timeframe).or_default();
            let member = group.get_mut(*index).ok_or("behavioral member missing")?;
            member.1.extend(dense);
            *index += 1;
        }
        for counter in counters.values_mut() {
            *counter = 0;
        }
    }
    let mut groups = Vec::with_capacity(vectors.len());
    for (timeframe, members) in vectors {
        let evidence = crate::behavior::diversity(
            members
                .into_iter()
                .map(|(concept, vector)| (concept, timeframe, vector))
                .collect(),
        )?;
        groups.push(CorpusGroup {
            timeframe,
            evidence,
        });
    }
    Ok(CorpusDiversity {
        instrument: spec.instrument.clone(),
        corpus_identity: spec.identity.clone(),
        months: spec.months.len(),
        events,
        limitations: LIMITATIONS.into(),
        groups,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonthExecution {
    pub year: u32,
    pub month: u32,
    /// Always `diagnostic_simulation`. Every month record is self-labelling so a diagnostic result
    /// cannot be republished as research evidence by a future caller.
    pub classification: String,
    /// Always `none`. No eligibility is granted or implied.
    pub eligibility: String,
    pub corpus_identity: String,
    /// The migration source dataset hash for this month; a provenance binding, not the replay id.
    pub source_dataset_hash: String,
    /// The replay's own `dataset_id` (`observed.ordered_events_hash`); this is what `receipt_hash`
    /// actually binds.
    pub replay_dataset_id: String,
    pub declared_class: String,
    pub profile_id: String,
    /// Explicit USD-account conversion convention used for this month (`identity_usd_v1` or
    /// `diagnostic_reciprocal_closing_rate_v1_long_bid_short_ask`).
    pub conversion_policy: String,
    pub initial_micro_usd: i128,
    /// The engine's `realized_micro_usd` field, which is the ending account balance (it starts at
    /// `contracts::INITIAL_EQUITY` and is not a profit figure by itself).
    pub realized_micro_usd: i128,
    pub final_marked_micro_usd: i128,
    pub maximum_drawdown_micro_usd: i128,
    pub trades: usize,
    pub rejections: usize,
    pub receipt_hash: String,
    /// Diagnostic-only derived outcome statistics for this month's closed trades. Never used
    /// as a financial result and never sufficient for eligibility or qualification.
    pub outcomes: TradeOutcomeStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonthRefusal {
    pub year: u32,
    pub month: u32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionTotals {
    pub months_executed: usize,
    pub months_refused: usize,
    pub trades: usize,
    pub rejections: usize,
    /// Sum of each executed month's starting equity. Reported for transparency, not as a result.
    pub initial_micro_usd: i128,
    /// Sum over executed months of `final_marked_micro_usd - initial_micro_usd`. Additive.
    pub pnl_micro_usd: i128,
    /// Maximum single-month drawdown, not a sum of per-month drawdowns.
    pub maximum_drawdown_micro_usd: i128,
    /// Closed-trade outcome sums, aggregated over executed months. Diagnostic only.
    pub wins: usize,
    pub losses: usize,
    pub breakevens: usize,
    /// Worst per-month losing streak. Months reset their account, so this is a reset-fold
    /// maximum and not a continuous full-period streak.
    pub max_losing_streak: usize,
    pub gross_win_micro_usd: i128,
    pub gross_loss_micro_usd: i128,
    /// Sum over months of closed-trade net P&L. Additive; excludes unresolved positions.
    pub net_micro_usd: i128,
    /// Months that ended holding an unresolved position.
    pub unresolved_positions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReport {
    pub instrument: String,
    pub identity: String,
    pub strategy_identity: String,
    /// Always `diagnostic_simulation`. Never research evidence.
    pub classification: String,
    /// Always `none`. No eligibility is granted or implied.
    pub eligibility: String,
    /// Explicit USD-account conversion convention applied to this instrument.
    pub conversion_policy: String,
    pub months: Vec<MonthExecution>,
    pub refusals: Vec<MonthRefusal>,
    pub totals: ExecutionTotals,
    /// Months served from the caller's content-addressed cache instead of being replayed.
    pub cache_hits: usize,
    /// Months actually replayed (cache absent, empty or not usable).
    pub cache_misses: usize,
    pub limitations: String,
}

pub const EXECUTION_CLASSIFICATION: &str = "diagnostic_simulation";
pub const EXECUTION_ELIGIBILITY: &str = "none";
/// Continuous full-period single-account diagnostic execution over all months of an instrument.
///
/// Months are chained so each month's ending marked equity becomes the next month's starting
/// equity, producing compounded full-period equity, a continuous losing streak across month
/// boundaries, and a full-period maximum drawdown. Trade outcomes are computed per-month by the
/// same authoritative sequential engine as reset-fold diagnostic execution; only the accounting
/// is continuous. Classification is `continuous_diagnostic`, eligibility `none`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuousExecution {
    pub instrument: String,
    pub identity: String,
    pub strategy_identity: String,
    pub classification: String,
    pub eligibility: String,
    pub conversion_policy: String,
    pub months: Vec<ContinuousMonthExecution>,
    pub totals: ContinuousTotals,
    pub limitations: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuousMonthExecution {
    pub year: u32,
    pub month: u32,
    pub starting_micro_usd: i128,
    pub ending_micro_usd: i128,
    pub monthly_pnl_micro_usd: i128,
    pub trades: usize,
    pub rejections: usize,
    pub wins: usize,
    pub losses: usize,
    pub breakevens: usize,
    pub month_ending_streak: usize,
    pub gross_win_micro_usd: i128,
    pub gross_loss_micro_usd: i128,
    pub net_micro_usd: i128,
    pub peak_equity_micro_usd: i128,
    pub drawdown_from_peak_micro_usd: i128,
    pub unresolved_positions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuousTotals {
    pub months_executed: usize,
    pub months_refused: usize,
    pub trades: usize,
    pub rejections: usize,
    pub initial_micro_usd: i128,
    pub ending_micro_usd: i128,
    pub net_pnl_micro_usd: i128,
    pub full_period_max_drawdown_micro_usd: i128,
    pub wins: usize,
    pub losses: usize,
    pub breakevens: usize,
    pub continuous_max_losing_streak: usize,
    pub gross_win_micro_usd: i128,
    pub gross_loss_micro_usd: i128,
    pub net_win_rate_bps: u16,
    pub unresolved_positions: usize,
}

const CONTINUOUS_CLASSIFICATION: &str = "continuous_diagnostic";
const CONTINUOUS_ELIGIBILITY: &str = "none";
const CONTINUOUS_LIMITATIONS: &str = "Continuous full-period single-account diagnostic execution over the registered multi-month corpus. Trade outcomes are computed per-month by the authoritative sequential engine starting from INITIAL_EQUITY each month; only the accounting is continuous (compounded equity, cross-month losing streak, full-period drawdown). Classification is continuous_diagnostic, eligibility none. No research eligibility, qualification, or scientific approval is granted.";

pub fn execute_continuous(
    spec: &HistoricalCorpus,
    strategy: &crate::strategies::Strategy,
    costs: &crate::tick_engine::Costs,
    budget: crate::tick_engine::CorpusBudget,
) -> Result<ContinuousExecution> {
    verify(spec)?;
    let mut executed_months = Vec::with_capacity(spec.months.len());
    let mut refusals = Vec::new();
    for index in 0..spec.months.len() {
        let (year, month) = (spec.months[index].year, spec.months[index].month);
        match execute_month(spec, index, strategy, costs, budget) {
            Ok(executed) => executed_months.push(executed),
            Err(reason) => refusals.push(MonthRefusal {
                year,
                month,
                reason,
            }),
        }
    }
    let (months, totals) = aggregate_continuous(&executed_months, &refusals)?;
    Ok(ContinuousExecution {
        instrument: spec.instrument.clone(),
        identity: spec.identity.clone(),
        strategy_identity: strategy.identity()?,
        classification: CONTINUOUS_CLASSIFICATION.into(),
        eligibility: CONTINUOUS_ELIGIBILITY.into(),
        conversion_policy: conversion_policy(&spec.profile).into(),
        months,
        totals,
        limitations: CONTINUOUS_LIMITATIONS.into(),
    })
}

/// Chain one month's closed-trade outcomes into the continuous full-period losing streak.
///
/// A month containing a net-winning closed trade breaks any run carried from the previous
/// month: `ending_streak` is measured from that month's last win, so it replaces the carried
/// run instead of extending it. A month with no wins — breakeven-only, loss-only, or with no
/// closed trades — extends the carried run by its trailing losses. This matches
/// `validation::assess`, where a breakeven neither resets nor extends a streak.
fn chain_losing_streak(continuous: usize, outcomes: &TradeOutcomeStats) -> usize {
    if outcomes.wins > 0 {
        outcomes.ending_streak
    } else {
        continuous.saturating_add(outcomes.ending_streak)
    }
}

/// Continuous full-period accounting over executed months: compounded equity, a cross-month
/// losing streak and a full-period drawdown. Pure (no engine, no I/O) so the accounting is
/// unit-testable directly from `MonthExecution` values, mirroring [`aggregate_totals`].
///
/// Every engine month starts at `contracts::INITIAL_EQUITY`, so a month's contribution to the
/// continuous equity is exactly its closed-trade net P&L; there is no starting-equity offset.
fn aggregate_continuous(
    executed_months: &[MonthExecution],
    refusals: &[MonthRefusal],
) -> Result<(Vec<ContinuousMonthExecution>, ContinuousTotals)> {
    let mut months = Vec::with_capacity(executed_months.len());
    let mut running_equity = contracts::INITIAL_EQUITY;
    let mut peak_equity = contracts::INITIAL_EQUITY;
    let mut cumulative_wins = 0usize;
    let mut cumulative_losses = 0usize;
    let mut cumulative_breakevens = 0usize;
    let mut continuous_streak = 0usize;
    let mut max_continuous_streak = 0usize;
    let mut cumulative_gross_win = 0i128;
    let mut cumulative_gross_loss = 0i128;
    let mut cumulative_net = 0i128;
    let mut cumulative_trades = 0usize;
    let mut cumulative_rejections = 0usize;
    let mut cumulative_unresolved = 0usize;

    for executed in executed_months {
        let month_starting = running_equity;
        let monthly_pnl = executed.outcomes.net_micro_usd;
        running_equity = running_equity
            .checked_add(monthly_pnl)
            .ok_or("continuous running equity overflow")?;
        if running_equity > peak_equity {
            peak_equity = running_equity;
        }
        let drawdown_from_peak = peak_equity
            .checked_sub(running_equity)
            .ok_or("continuous drawdown overflow")?;

        // Chain losing streak across month boundary.
        continuous_streak = chain_losing_streak(continuous_streak, &executed.outcomes);
        max_continuous_streak = max_continuous_streak.max(continuous_streak);

        cumulative_wins = cumulative_wins
            .checked_add(executed.outcomes.wins)
            .ok_or("continuous wins overflow")?;
        cumulative_losses = cumulative_losses
            .checked_add(executed.outcomes.losses)
            .ok_or("continuous losses overflow")?;
        cumulative_breakevens = cumulative_breakevens
            .checked_add(executed.outcomes.breakevens)
            .ok_or("continuous breakevens overflow")?;
        cumulative_gross_win = cumulative_gross_win
            .checked_add(executed.outcomes.gross_win_micro_usd)
            .ok_or("continuous gross win overflow")?;
        cumulative_gross_loss = cumulative_gross_loss
            .checked_add(executed.outcomes.gross_loss_micro_usd)
            .ok_or("continuous gross loss overflow")?;
        cumulative_net = cumulative_net
            .checked_add(executed.outcomes.net_micro_usd)
            .ok_or("continuous net overflow")?;
        cumulative_trades = cumulative_trades
            .checked_add(executed.trades)
            .ok_or("continuous trades overflow")?;
        cumulative_rejections = cumulative_rejections
            .checked_add(executed.rejections)
            .ok_or("continuous rejections overflow")?;
        cumulative_unresolved = cumulative_unresolved
            .checked_add(executed.outcomes.unresolved_positions)
            .ok_or("continuous unresolved overflow")?;

        months.push(ContinuousMonthExecution {
            year: executed.year,
            month: executed.month,
            starting_micro_usd: month_starting,
            ending_micro_usd: running_equity,
            monthly_pnl_micro_usd: monthly_pnl,
            trades: executed.trades,
            rejections: executed.rejections,
            wins: executed.outcomes.wins,
            losses: executed.outcomes.losses,
            breakevens: executed.outcomes.breakevens,
            month_ending_streak: executed.outcomes.ending_streak,
            gross_win_micro_usd: executed.outcomes.gross_win_micro_usd,
            gross_loss_micro_usd: executed.outcomes.gross_loss_micro_usd,
            net_micro_usd: executed.outcomes.net_micro_usd,
            peak_equity_micro_usd: peak_equity,
            drawdown_from_peak_micro_usd: drawdown_from_peak,
            unresolved_positions: executed.outcomes.unresolved_positions,
        });
    }

    let resolved = cumulative_wins + cumulative_losses + cumulative_breakevens;
    let net_win_rate_bps = if resolved > 0 {
        (cumulative_wins as u128 * 10_000 / resolved as u128) as u16
    } else {
        0
    };

    let totals = ContinuousTotals {
        months_executed: months.len(),
        months_refused: refusals.len(),
        trades: cumulative_trades,
        rejections: cumulative_rejections,
        initial_micro_usd: contracts::INITIAL_EQUITY,
        ending_micro_usd: running_equity,
        net_pnl_micro_usd: running_equity
            .checked_sub(contracts::INITIAL_EQUITY)
            .ok_or("continuous net P&L overflow")?,
        full_period_max_drawdown_micro_usd: peak_equity
            .checked_sub(running_equity.min(peak_equity))
            .unwrap_or(0),
        wins: cumulative_wins,
        losses: cumulative_losses,
        breakevens: cumulative_breakevens,
        continuous_max_losing_streak: max_continuous_streak,
        gross_win_micro_usd: cumulative_gross_win,
        gross_loss_micro_usd: cumulative_gross_loss,
        net_win_rate_bps,
        unresolved_positions: cumulative_unresolved,
    };

    Ok((months, totals))
}

/// USD-quoted instruments need no cross-currency conversion.
pub const CONVERSION_POLICY_IDENTITY: &str = "identity_usd_v1";
/// USD-base non-USD-quote instruments use the direction-based reciprocal closing rate derived
/// from the instrument's own verified quote: long -> bid, short -> ask; USD per quote currency is
/// `price_scale / price`. Explicit unverified research convention, not an approved broker rule.
pub const CONVERSION_POLICY_RECIPROCAL: &str =
    "diagnostic_reciprocal_closing_rate_v1_long_bid_short_ask";
const EXECUTION_LIMITATIONS: &str = "Diagnostic simulation only. The authoritative sequential engine and frozen financial rules are applied to real structurally-valid history using explicit test-convention profiles, so results are not broker-approved execution. USD-quoted instruments use the identity conversion; USD-base non-USD-quote instruments (USDJPY/USDCHF/USDCAD) use a diagnostic direction-based reciprocal closing rate derived from the instrument's own verified quote (long -> bid, short -> ask; USD per quote currency = price_scale / price). That conversion convention is an explicit unverified research assumption, not an approved broker conversion rule; the documented MT5 profitability-based variant is not implemented. Cross-currency pairs and eligibility-bearing data classes are refused. No research eligibility, calendar/profile/licensing approval, scientific validity or qualification is granted. Counts and money figures are observations, not performance evidence.";

/// The explicit USD-account conversion convention applied to an instrument's diagnostic replay.
pub fn conversion_policy(profile: &Profile) -> &'static str {
    if profile.quote == "USD" {
        CONVERSION_POLICY_IDENTITY
    } else {
        CONVERSION_POLICY_RECIPROCAL
    }
}

/// Execute one month through the authoritative sequential engine.
///
/// The month's provenance is re-verified here rather than trusted: the aggregate identity must
/// recompute, and every chunk must still hash to the inventory recorded in the specification, so a
/// forged or stale `HistoricalCorpus` cannot produce a report bound to values it did not read.
/// Callers wanting the whole corpus should use [`execute`], which additionally re-verifies every
/// monthly migration receipt.
pub fn execute_month(
    spec: &HistoricalCorpus,
    index: usize,
    strategy: &crate::strategies::Strategy,
    costs: &crate::tick_engine::Costs,
    budget: crate::tick_engine::CorpusBudget,
) -> Result<MonthExecution> {
    if spec.identity != identity(&spec.instrument, &spec.profile, &spec.months)? {
        return Err("historical corpus identity mismatch".into());
    }
    let month = spec
        .months
        .get(index)
        .ok_or("historical corpus month index out of range")?;
    let dir = Path::new(&month.output_dir);
    for (file, hash) in month.chunk_files.iter().zip(&month.chunk_hashes) {
        let dataset = native::read(&dir.join(file))?;
        if data::hash(&dataset)? != *hash || dataset.profile != spec.profile {
            return Err("historical corpus chunk mismatch during diagnostic execution".into());
        }
    }
    let chunks: Vec<std::path::PathBuf> = month
        .chunk_files
        .iter()
        .map(|file| dir.join(file))
        .collect();
    let corpus_spec =
        crate::corpus::describe(&chunks, crate::historical_migration::MAX_CHUNK_EVENTS)?;
    crate::corpus::inspect(dir, &corpus_spec)?;
    let facts = crate::corpus::source_facts(dir, &corpus_spec)?;
    let replay =
        crate::tick_engine::replay_corpus_diagnostic(dir, &corpus_spec, strategy, costs, budget)?;
    let outcomes = trade_outcome_stats(&replay.trades, replay.unresolved.is_some())?;
    Ok(MonthExecution {
        year: month.year,
        month: month.month,
        classification: EXECUTION_CLASSIFICATION.into(),
        eligibility: EXECUTION_ELIGIBILITY.into(),
        corpus_identity: corpus_spec.identity()?,
        source_dataset_hash: month.dataset_hash.clone(),
        replay_dataset_id: replay.dataset_id.clone(),
        declared_class: format!("{:?}", facts.declared_class()),
        profile_id: crate::data::hash(&spec.profile)?,
        conversion_policy: conversion_policy(&spec.profile).into(),
        initial_micro_usd: replay.initial_micro_usd,
        realized_micro_usd: replay.realized_micro_usd,
        final_marked_micro_usd: replay.final_marked_micro_usd,
        maximum_drawdown_micro_usd: replay.maximum_drawdown_micro_usd,
        trades: replay.trades.len(),
        rejections: replay.rejections.len(),
        receipt_hash: crate::data::hash(&replay)?,
        outcomes,
    })
}

/// Aggregate executed months and refusals. Only additive quantities are summed; the drawdown is
/// the worst single month, and the P&L is the sum of per-month marked-equity deltas.
fn aggregate_totals(
    months: &[MonthExecution],
    refusals: &[MonthRefusal],
) -> Result<ExecutionTotals> {
    let mut totals = ExecutionTotals {
        months_executed: months.len(),
        months_refused: refusals.len(),
        trades: 0,
        rejections: 0,
        initial_micro_usd: 0,
        pnl_micro_usd: 0,
        maximum_drawdown_micro_usd: 0,
        wins: 0,
        losses: 0,
        breakevens: 0,
        max_losing_streak: 0,
        gross_win_micro_usd: 0,
        gross_loss_micro_usd: 0,
        net_micro_usd: 0,
        unresolved_positions: 0,
    };
    for executed in months {
        let outcomes = &executed.outcomes;
        totals.wins = totals
            .wins
            .checked_add(outcomes.wins)
            .ok_or("diagnostic execution win total overflow")?;
        totals.losses = totals
            .losses
            .checked_add(outcomes.losses)
            .ok_or("diagnostic execution loss total overflow")?;
        totals.breakevens = totals
            .breakevens
            .checked_add(outcomes.breakevens)
            .ok_or("diagnostic execution breakeven total overflow")?;
        totals.max_losing_streak = totals.max_losing_streak.max(outcomes.max_losing_streak);
        totals.gross_win_micro_usd = totals
            .gross_win_micro_usd
            .checked_add(outcomes.gross_win_micro_usd)
            .ok_or("diagnostic execution gross win overflow")?;
        totals.gross_loss_micro_usd = totals
            .gross_loss_micro_usd
            .checked_add(outcomes.gross_loss_micro_usd)
            .ok_or("diagnostic execution gross loss overflow")?;
        totals.net_micro_usd = totals
            .net_micro_usd
            .checked_add(outcomes.net_micro_usd)
            .ok_or("diagnostic execution trade net overflow")?;
        totals.unresolved_positions = totals
            .unresolved_positions
            .checked_add(outcomes.unresolved_positions)
            .ok_or("diagnostic execution unresolved total overflow")?;
        totals.trades = totals
            .trades
            .checked_add(executed.trades)
            .ok_or("diagnostic execution trade total overflow")?;
        totals.rejections = totals
            .rejections
            .checked_add(executed.rejections)
            .ok_or("diagnostic execution rejection total overflow")?;
        totals.initial_micro_usd = totals
            .initial_micro_usd
            .checked_add(executed.initial_micro_usd)
            .ok_or("diagnostic execution initial equity total overflow")?;
        let month_pnl = executed
            .final_marked_micro_usd
            .checked_sub(executed.initial_micro_usd)
            .ok_or("diagnostic execution monthly P&L overflow")?;
        totals.pnl_micro_usd = totals
            .pnl_micro_usd
            .checked_add(month_pnl)
            .ok_or("diagnostic execution P&L total overflow")?;
        totals.maximum_drawdown_micro_usd = totals
            .maximum_drawdown_micro_usd
            .max(executed.maximum_drawdown_micro_usd);
    }
    Ok(totals)
}

/// Content-addressed, caller-supplied cache for diagnostic month executions.
///
/// This is a local performance and resumption aid, not a research artifact: entries are keyed by a
/// hash of every input that can change the result (corpus identity, month, chunk inventory,
/// strategy, costs, budgets and engine version), so a hit is only possible when the inputs are
/// identical. Entries carry a self-hash and an unreadable or mismatched entry is refused rather
/// than trusted. Nothing here grants eligibility or feeds the registered research pipeline.
#[derive(Debug, Clone)]
pub struct ExecutionCache {
    dir: std::path::PathBuf,
}

const EXECUTION_CACHE_VERSION: &str = "historical-diagnostic-execution-cache-v4";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheEntry {
    entry_hash: String,
    execution: MonthExecution,
}

impl ExecutionCache {
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    pub fn key(
        spec: &HistoricalCorpus,
        index: usize,
        strategy: &crate::strategies::Strategy,
        costs: &crate::tick_engine::Costs,
        budget: crate::tick_engine::CorpusBudget,
    ) -> Result<String> {
        let month = spec
            .months
            .get(index)
            .ok_or("historical corpus month index out of range")?;
        crate::data::hash(&(
            EXECUTION_CACHE_VERSION,
            &spec.identity,
            index,
            &month.dataset_hash,
            &month.chunk_hashes,
            strategy.identity()?,
            crate::data::hash(costs)?,
            budget.max_candles,
            budget.feature_cells,
            budget.max_receipts,
            crate::ENGINE_VERSION,
        ))
    }

    pub fn load(&self, key: &str) -> Result<Option<MonthExecution>> {
        let Ok(bytes) = std::fs::read(self.dir.join(format!("{key}.json"))) else {
            return Ok(None);
        };
        let entry: CacheEntry = serde_json::from_slice(&bytes)
            .map_err(|e| format!("diagnostic execution cache entry unreadable: {e}"))?;
        if entry.entry_hash != crate::data::hash(&entry.execution)? {
            return Err("diagnostic execution cache entry hash mismatch".into());
        }
        Ok(Some(entry.execution))
    }

    pub fn store(&self, key: &str, execution: &MonthExecution) -> Result<()> {
        let entry = CacheEntry {
            entry_hash: crate::data::hash(execution)?,
            execution: execution.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&entry).map_err(|e| e.to_string())?;
        let final_path = self.dir.join(format!("{key}.json"));
        let temp_path = self.dir.join(format!(".{key}.{}.tmp", std::process::id()));
        std::fs::write(&temp_path, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&temp_path, &final_path).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Diagnostic execution of one instrument's verified historical corpus through the authoritative
/// sequential engine, month by month. Each month keeps its own corpus identity, declared class and
/// profile; months that the engine refuses (for example non-USD quote currencies) are recorded as
/// refusals rather than silently dropped.
///
/// Pass `None` for `cache` to always replay. With a cache, identical months are resumed from the
/// content-addressed entry instead of being replayed; the report records how many months were hits
/// and misses.
pub fn execute_with_cache(
    spec: &HistoricalCorpus,
    strategy: &crate::strategies::Strategy,
    costs: &crate::tick_engine::Costs,
    budget: crate::tick_engine::CorpusBudget,
    cache: Option<&ExecutionCache>,
) -> Result<ExecutionReport> {
    verify(spec)?;
    let mut months = Vec::with_capacity(spec.months.len());
    let mut refusals = Vec::new();
    let mut cache_hits = 0usize;
    let mut cache_misses = 0usize;
    for index in 0..spec.months.len() {
        let (year, month) = (spec.months[index].year, spec.months[index].month);
        let key = match cache {
            Some(_) => Some(ExecutionCache::key(spec, index, strategy, costs, budget)?),
            None => None,
        };
        let cached = match (&cache, &key) {
            (Some(cache), Some(key)) => cache.load(key)?,
            _ => None,
        };
        let outcome = match cached {
            Some(executed) => {
                cache_hits += 1;
                Ok(executed)
            }
            None => {
                cache_misses += 1;
                let executed = execute_month(spec, index, strategy, costs, budget);
                if let (Some(cache), Some(key)) = (cache, &key)
                    && let Ok(executed) = &executed
                {
                    cache.store(key, executed)?;
                }
                executed
            }
        };
        match outcome {
            Ok(executed) => months.push(executed),
            Err(reason) => refusals.push(MonthRefusal {
                year,
                month,
                reason,
            }),
        }
    }
    Ok(ExecutionReport {
        instrument: spec.instrument.clone(),
        identity: spec.identity.clone(),
        strategy_identity: strategy.identity()?,
        classification: EXECUTION_CLASSIFICATION.into(),
        eligibility: EXECUTION_ELIGIBILITY.into(),
        conversion_policy: conversion_policy(&spec.profile).into(),
        months: months.clone(),
        refusals: refusals.clone(),
        totals: aggregate_totals(&months, &refusals)?,
        cache_hits,
        cache_misses,
        limitations: EXECUTION_LIMITATIONS.into(),
    })
}

/// Diagnostic execution with no cache: every month is replayed from source.
pub fn execute(
    spec: &HistoricalCorpus,
    strategy: &crate::strategies::Strategy,
    costs: &crate::tick_engine::Costs,
    budget: crate::tick_engine::CorpusBudget,
) -> Result<ExecutionReport> {
    execute_with_cache(spec, strategy, costs, budget, None)
}

// === Registered full-period real-data execution ===

/// The only accounting classification the qualification full-period gate can accept. It is
/// emitted solely by [`execute_registered_full_period`] through `ReplayMode::Registered`;
/// diagnostic simulation and continuous diagnostic output can never be relabelled into it.
pub const REGISTERED_FULL_PERIOD_CLASSIFICATION: &str = "registered_full_period_replay";
/// Execution never grants eligibility. Only the independent approval chain can.
pub const REGISTERED_FULL_PERIOD_ELIGIBILITY: &str = "none";
const REGISTERED_FULL_PERIOD_LIMITATIONS: &str = "Registered full-period single-account replay over the owner-selected 22-month corpus through the authoritative sequential engine in ReplayMode::Registered (literal-USD identity conversion). The explicit training window is recorded so seen training evidence is never presented as unseen. Eligibility is none: no calendar/profile/legal/scientific approval, research eligibility or qualification is granted. A month that cannot be executed aborts the run, so a partial replay is never labelled registered_full_period_replay. Each monthly engine receipt starts at INITIAL_EQUITY; continuous equity chains the exact closed-trade net P&L, and the qualification gate separately requires zero unresolved end-of-period positions.";

/// Frozen compute identity bound into every registered full-period report.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComputeIdentity {
    pub engine: String,
    pub indicators: String,
    pub signals: String,
    pub compute_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredMonthExecution {
    pub year: u32,
    pub month: u32,
    /// Always [`REGISTERED_FULL_PERIOD_CLASSIFICATION`].
    pub classification: String,
    /// Always [`REGISTERED_FULL_PERIOD_ELIGIBILITY`].
    pub eligibility: String,
    pub corpus_identity: String,
    pub source_dataset_hash: String,
    pub replay_dataset_id: String,
    pub declared_class: String,
    pub profile_id: String,
    pub conversion_policy: String,
    pub initial_micro_usd: i128,
    pub realized_micro_usd: i128,
    pub final_marked_micro_usd: i128,
    pub maximum_drawdown_micro_usd: i128,
    pub trades: usize,
    pub rejections: usize,
    /// Exact hash of the month's authoritative `Replay` receipt.
    pub receipt_hash: String,
    pub outcomes: TradeOutcomeStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredTotals {
    pub months_executed: usize,
    pub trades: usize,
    pub rejections: usize,
    pub initial_micro_usd: i128,
    pub ending_micro_usd: i128,
    pub net_pnl_micro_usd: i128,
    pub full_period_max_drawdown_micro_usd: i128,
    pub wins: usize,
    pub losses: usize,
    pub breakevens: usize,
    pub continuous_max_losing_streak: usize,
    pub gross_win_micro_usd: i128,
    pub gross_loss_micro_usd: i128,
    pub net_win_rate_bps: u16,
    pub unresolved_positions: usize,
}

/// Registered full-period single-account replay over the owner-selected research window.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredFullPeriodExecution {
    pub instrument: String,
    pub identity: String,
    pub strategy_identity: String,
    /// Always [`REGISTERED_FULL_PERIOD_CLASSIFICATION`].
    pub classification: String,
    /// Always [`REGISTERED_FULL_PERIOD_ELIGIBILITY`].
    pub eligibility: String,
    pub conversion_policy: String,
    /// Explicit training window. Real-data runs cannot omit it.
    pub training_window: crate::validation::Interval,
    pub compute: ComputeIdentity,
    pub months: Vec<RegisteredMonthExecution>,
    pub totals: RegisteredTotals,
    /// Exact hash binding the instrument, corpus, strategy, training window, compute identity,
    /// every monthly receipt and the totals.
    pub report_hash: String,
    pub limitations: String,
}

/// Exact accounting counters derived from a registered full-period execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredAccounting {
    pub classification: String,
    pub cutoff_ms: i64,
    pub pooled: crate::validation::PeriodStats,
    pub year_2025: crate::validation::PeriodStats,
    pub year_2026: crate::validation::PeriodStats,
    pub net_micro_usd: i128,
    pub ending_micro_usd: i128,
    pub max_drawdown_micro_usd: i128,
    pub unresolved: usize,
}

impl RegisteredFullPeriodExecution {
    /// Bucket the registered months into the exact pooled/year counters the qualification gate
    /// consumes. Months outside the owner-selected 2025/2026 research years are refused.
    pub fn accounting(&self, cutoff_ms: i64) -> Result<RegisteredAccounting> {
        let cutoff = chrono::DateTime::from_timestamp_millis(cutoff_ms)
            .ok_or("invalid registered accounting cutoff")?;
        if cutoff.year() != 2026 {
            return Err("registered accounting cutoff must lie in 2026".into());
        }
        let mut pooled = crate::validation::PeriodStats::default();
        let mut year_2025 = crate::validation::PeriodStats::default();
        let mut year_2026 = crate::validation::PeriodStats::default();
        for month in &self.months {
            let bucket = match month.year {
                2025 => &mut year_2025,
                2026 => &mut year_2026,
                other => {
                    return Err(format!(
                        "registered month {other}-{:02} is outside the approved 2025/2026 research years",
                        month.month
                    ));
                }
            };
            bucket.resolved = bucket
                .resolved
                .checked_add(month.outcomes.resolved)
                .ok_or("registered resolved overflow")?;
            bucket.wins = bucket
                .wins
                .checked_add(month.outcomes.wins)
                .ok_or("registered wins overflow")?;
            bucket.losses = bucket
                .losses
                .checked_add(month.outcomes.losses)
                .ok_or("registered losses overflow")?;
            bucket.breakeven = bucket
                .breakeven
                .checked_add(month.outcomes.breakevens)
                .ok_or("registered breakeven overflow")?;
            bucket.max_losing_streak = bucket
                .max_losing_streak
                .max(month.outcomes.max_losing_streak);
        }
        pooled.resolved = year_2025
            .resolved
            .checked_add(year_2026.resolved)
            .ok_or("registered pooled resolved overflow")?;
        pooled.wins = year_2025
            .wins
            .checked_add(year_2026.wins)
            .ok_or("registered pooled wins overflow")?;
        pooled.losses = year_2025
            .losses
            .checked_add(year_2026.losses)
            .ok_or("registered pooled losses overflow")?;
        pooled.breakeven = year_2025
            .breakeven
            .checked_add(year_2026.breakeven)
            .ok_or("registered pooled breakeven overflow")?;
        // The continuous streak is the honest full-period streak and bounds both yearly streaks.
        pooled.max_losing_streak = self.totals.continuous_max_losing_streak;
        Ok(RegisteredAccounting {
            classification: self.classification.clone(),
            cutoff_ms,
            pooled,
            year_2025,
            year_2026,
            net_micro_usd: self.totals.net_pnl_micro_usd,
            ending_micro_usd: self.totals.ending_micro_usd,
            max_drawdown_micro_usd: self.totals.full_period_max_drawdown_micro_usd,
            unresolved: self.totals.unresolved_positions,
        })
    }
}

/// Execute the owner-selected research window as a registered full-period single-account replay
/// through `ReplayMode::Registered` (the four-class literal-USD input contract; identity
/// conversion). This is the only pipeline that emits [`REGISTERED_FULL_PERIOD_CLASSIFICATION`].
///
/// `training_window` is required for real data so seen training evidence is never presented as
/// unseen; it must lie inside the evaluated corpus. Synthetic input may omit it (the whole
/// synthetic corpus is recorded as the declared window). A month that cannot be executed aborts
/// the run: a partial replay is never labelled `registered_full_period_replay`. No eligibility,
/// approval or qualification is granted.
pub fn execute_registered_full_period(
    spec: &HistoricalCorpus,
    strategy: &crate::strategies::Strategy,
    costs: &crate::tick_engine::Costs,
    budget: crate::tick_engine::CorpusBudget,
    training_window: Option<crate::validation::Interval>,
) -> Result<RegisteredFullPeriodExecution> {
    if spec.identity != identity(&spec.instrument, &spec.profile, &spec.months)? {
        return Err("historical corpus identity mismatch".into());
    }
    validate_months(&spec.months, spec.months.len())?;
    let window = ResearchWindow::OWNER_SELECTED;
    let first = spec
        .months
        .first()
        .ok_or("historical corpus has no months")?;
    let last = spec
        .months
        .last()
        .ok_or("historical corpus has no months")?;
    if (first.year, first.month) != (window.start_year, window.start_month)
        || (last.year, last.month) != (window.end_year, window.end_month)
    {
        return Err(format!(
            "registered full-period replay requires the owner-selected research window {}; corpus covers {}-{:02}..{}-{:02}",
            window.label(),
            first.year,
            first.month,
            last.year,
            last.month
        ));
    }
    // Read the declared class from verified first-month chunks; the four-class contract is
    // enforced here with an exact message before the engine re-checks it.
    let first_dir = Path::new(&first.output_dir);
    let first_file = first
        .chunk_files
        .first()
        .ok_or("historical corpus month has no chunks")?;
    let first_dataset = native::read(&first_dir.join(first_file))?;
    let first_hash = first
        .chunk_hashes
        .first()
        .ok_or("historical corpus month has no chunk hashes")?;
    if data::hash(&first_dataset)? != *first_hash || first_dataset.profile != spec.profile {
        return Err("historical corpus chunk mismatch during registered execution".into());
    }
    let declared_class = first_dataset.manifest.class;
    let allowed_classes = [
        crate::data::DataClass::SyntheticTestOnly,
        crate::data::DataClass::RealStructurallyValid,
        crate::data::DataClass::RealQualityAccepted,
        crate::data::DataClass::RealProfileApproved,
    ];
    if !allowed_classes.contains(&declared_class) {
        return Err(format!(
            "registered full-period replay refuses data class {declared_class:?}; allowed classes are SyntheticTestOnly, RealStructurallyValid, RealQualityAccepted and RealProfileApproved"
        ));
    }
    let corpus_end_exclusive_ms = last
        .last_ms
        .checked_add(1)
        .ok_or("historical corpus end overflow")?;
    let training_window = match training_window {
        Some(window) => {
            window.validate()?;
            if window.start_ms < first.first_ms || window.end_exclusive_ms > corpus_end_exclusive_ms {
                return Err(
                    "registered training window must lie inside the evaluated corpus".into(),
                );
            }
            window
        }
        None if declared_class == crate::data::DataClass::SyntheticTestOnly => {
            crate::validation::Interval {
                start_ms: first.first_ms,
                end_exclusive_ms: corpus_end_exclusive_ms,
            }
        }
        None => {
            return Err(
                "registered real-data full-period replay requires an explicit training_window (unseen-evidence preservation)".into(),
            )
        }
    };
    let compute = ComputeIdentity {
        engine: crate::ENGINE_VERSION.into(),
        indicators: crate::INDICATOR_VERSION.into(),
        signals: crate::SIGNAL_VERSION.into(),
        compute_revision: crate::research::compute_revision()?,
    };
    let strategy_identity = strategy.identity()?;
    let mut months = Vec::with_capacity(spec.months.len());
    let mut running_equity = contracts::INITIAL_EQUITY;
    let mut peak_equity = contracts::INITIAL_EQUITY;
    let mut max_drawdown = 0i128;
    let mut continuous_streak = 0usize;
    let mut max_continuous_streak = 0usize;
    let mut cumulative_wins = 0usize;
    let mut cumulative_losses = 0usize;
    let mut cumulative_breakevens = 0usize;
    let mut cumulative_gross_win = 0i128;
    let mut cumulative_gross_loss = 0i128;
    let mut cumulative_trades = 0usize;
    let mut cumulative_rejections = 0usize;
    let mut cumulative_unresolved = 0usize;
    for month in &spec.months {
        let dir = Path::new(&month.output_dir);
        for (file, hash) in month.chunk_files.iter().zip(&month.chunk_hashes) {
            let dataset = native::read(&dir.join(file))?;
            if data::hash(&dataset)? != *hash
                || dataset.profile != spec.profile
                || dataset.manifest.class != declared_class
            {
                return Err("historical corpus chunk mismatch during registered execution".into());
            }
        }
        let chunks: Vec<std::path::PathBuf> = month
            .chunk_files
            .iter()
            .map(|file| dir.join(file))
            .collect();
        let corpus_spec =
            crate::corpus::describe(&chunks, crate::historical_migration::MAX_CHUNK_EVENTS)?;
        crate::corpus::inspect(dir, &corpus_spec)?;
        let facts = crate::corpus::source_facts(dir, &corpus_spec)?;
        if facts.declared_class() != declared_class || facts.profile() != &spec.profile {
            return Err(
                "historical corpus source facts mismatch during registered execution".into(),
            );
        }
        // Registered mode: literal-USD identity conversion and the four-class input contract.
        let replay = crate::tick_engine::replay_corpus(dir, &corpus_spec, strategy, costs, budget)?;
        if replay.initial_micro_usd != contracts::INITIAL_EQUITY {
            return Err("registered corpus replay must start from INITIAL_EQUITY".into());
        }
        let outcomes = trade_outcome_stats(&replay.trades, replay.unresolved.is_some())?;
        running_equity = running_equity
            .checked_add(outcomes.net_micro_usd)
            .ok_or("registered running equity overflow")?;
        if running_equity > peak_equity {
            peak_equity = running_equity;
        }
        max_drawdown = max_drawdown.max(
            peak_equity
                .checked_sub(running_equity)
                .ok_or("registered drawdown overflow")?,
        );
        if outcomes.ending_streak == 0 {
            continuous_streak = 0;
        } else {
            continuous_streak = continuous_streak.saturating_add(outcomes.ending_streak);
        }
        max_continuous_streak = max_continuous_streak.max(continuous_streak);
        cumulative_wins = cumulative_wins
            .checked_add(outcomes.wins)
            .ok_or("registered wins overflow")?;
        cumulative_losses = cumulative_losses
            .checked_add(outcomes.losses)
            .ok_or("registered losses overflow")?;
        cumulative_breakevens = cumulative_breakevens
            .checked_add(outcomes.breakevens)
            .ok_or("registered breakevens overflow")?;
        cumulative_gross_win = cumulative_gross_win
            .checked_add(outcomes.gross_win_micro_usd)
            .ok_or("registered gross win overflow")?;
        cumulative_gross_loss = cumulative_gross_loss
            .checked_add(outcomes.gross_loss_micro_usd)
            .ok_or("registered gross loss overflow")?;
        cumulative_trades = cumulative_trades
            .checked_add(replay.trades.len())
            .ok_or("registered trades overflow")?;
        cumulative_rejections = cumulative_rejections
            .checked_add(replay.rejections.len())
            .ok_or("registered rejections overflow")?;
        cumulative_unresolved = cumulative_unresolved
            .checked_add(outcomes.unresolved_positions)
            .ok_or("registered unresolved overflow")?;
        months.push(RegisteredMonthExecution {
            year: month.year,
            month: month.month,
            classification: REGISTERED_FULL_PERIOD_CLASSIFICATION.into(),
            eligibility: REGISTERED_FULL_PERIOD_ELIGIBILITY.into(),
            corpus_identity: corpus_spec.identity()?,
            source_dataset_hash: month.dataset_hash.clone(),
            replay_dataset_id: replay.dataset_id.clone(),
            declared_class: format!("{declared_class:?}"),
            profile_id: crate::data::hash(&spec.profile)?,
            conversion_policy: conversion_policy(&spec.profile).into(),
            initial_micro_usd: replay.initial_micro_usd,
            realized_micro_usd: replay.realized_micro_usd,
            final_marked_micro_usd: replay.final_marked_micro_usd,
            maximum_drawdown_micro_usd: replay.maximum_drawdown_micro_usd,
            trades: replay.trades.len(),
            rejections: replay.rejections.len(),
            receipt_hash: crate::data::hash(&replay)?,
            outcomes,
        });
    }
    let resolved = cumulative_wins
        .checked_add(cumulative_losses)
        .and_then(|n| n.checked_add(cumulative_breakevens))
        .ok_or("registered resolved overflow")?;
    let net_win_rate_bps = if resolved > 0 {
        (cumulative_wins as u128 * 10_000 / resolved as u128) as u16
    } else {
        0
    };
    let totals = RegisteredTotals {
        months_executed: months.len(),
        trades: cumulative_trades,
        rejections: cumulative_rejections,
        initial_micro_usd: contracts::INITIAL_EQUITY,
        ending_micro_usd: running_equity,
        net_pnl_micro_usd: running_equity
            .checked_sub(contracts::INITIAL_EQUITY)
            .ok_or("registered net P&L overflow")?,
        full_period_max_drawdown_micro_usd: max_drawdown,
        wins: cumulative_wins,
        losses: cumulative_losses,
        breakevens: cumulative_breakevens,
        continuous_max_losing_streak: max_continuous_streak,
        gross_win_micro_usd: cumulative_gross_win,
        gross_loss_micro_usd: cumulative_gross_loss,
        net_win_rate_bps,
        unresolved_positions: cumulative_unresolved,
    };
    let report_hash = data::hash(&(
        REGISTERED_FULL_PERIOD_CLASSIFICATION,
        &spec.instrument,
        &spec.identity,
        &strategy_identity,
        &training_window,
        &compute,
        &months,
        &totals,
    ))?;
    Ok(RegisteredFullPeriodExecution {
        instrument: spec.instrument.clone(),
        identity: spec.identity.clone(),
        strategy_identity,
        classification: REGISTERED_FULL_PERIOD_CLASSIFICATION.into(),
        eligibility: REGISTERED_FULL_PERIOD_ELIGIBILITY.into(),
        conversion_policy: conversion_policy(&spec.profile).into(),
        training_window,
        compute,
        months,
        totals,
        report_hash,
        limitations: REGISTERED_FULL_PERIOD_LIMITATIONS.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn month(year: u32, month: u32, first: i64, last: i64) -> MonthlySource {
        MonthlySource {
            instrument: "EURUSD".into(),
            year,
            month,
            dataset_hash: format!("{year:04}{month:02}"),
            source_sha256: "a".repeat(64),
            parquet_sha256: "b".repeat(64),
            rows: 1_000,
            first_ms: first,
            last_ms: last,
            output_dir: format!("/tmp/EURUSD-{year:04}{month:02}"),
            parquet_path: format!("/tmp/EURUSD-{year:04}{month:02}.parquet"),
            chunk_files: vec!["chunk-000000.rsf".into()],
            chunk_hashes: vec!["c".repeat(64)],
        }
    }

    fn contiguous() -> Vec<MonthlySource> {
        (1..=22)
            .map(|m| {
                month(
                    2025 + (m - 1) / 12,
                    (m - 1) % 12 + 1,
                    i64::from(m) * 1_000,
                    i64::from(m) * 1_000 + 500,
                )
            })
            .collect()
    }

    fn execution(
        year: u32,
        month: u32,
        initial: i128,
        marked: i128,
        drawdown: i128,
        trades: usize,
        rejections: usize,
    ) -> MonthExecution {
        MonthExecution {
            year,
            month,
            classification: EXECUTION_CLASSIFICATION.into(),
            eligibility: EXECUTION_ELIGIBILITY.into(),
            corpus_identity: "a".repeat(64),
            source_dataset_hash: "b".repeat(64),
            replay_dataset_id: "e".repeat(64),
            declared_class: "RealStructurallyValid".into(),
            profile_id: "c".repeat(64),
            conversion_policy: CONVERSION_POLICY_IDENTITY.into(),
            initial_micro_usd: initial,
            realized_micro_usd: marked,
            final_marked_micro_usd: marked,
            maximum_drawdown_micro_usd: drawdown,
            trades,
            rejections,
            receipt_hash: "d".repeat(64),
            outcomes: TradeOutcomeStats::default(),
        }
    }

    fn execution_with_outcomes(
        year: u32,
        month: u32,
        outcomes: TradeOutcomeStats,
    ) -> MonthExecution {
        MonthExecution {
            year,
            month,
            classification: EXECUTION_CLASSIFICATION.into(),
            eligibility: EXECUTION_ELIGIBILITY.into(),
            corpus_identity: "a".repeat(64),
            source_dataset_hash: "b".repeat(64),
            replay_dataset_id: "e".repeat(64),
            declared_class: "RealStructurallyValid".into(),
            profile_id: "c".repeat(64),
            conversion_policy: CONVERSION_POLICY_IDENTITY.into(),
            initial_micro_usd: 20_000_000,
            realized_micro_usd: 20_000_000,
            final_marked_micro_usd: 20_000_000,
            maximum_drawdown_micro_usd: 0,
            trades: outcomes.resolved,
            rejections: 0,
            receipt_hash: "d".repeat(64),
            outcomes,
        }
    }

    fn continuous_month(year: u32, month: u32, outcomes: TradeOutcomeStats) -> MonthExecution {
        let net = outcomes.net_micro_usd;
        MonthExecution {
            year,
            month,
            classification: EXECUTION_CLASSIFICATION.into(),
            eligibility: EXECUTION_ELIGIBILITY.into(),
            corpus_identity: "a".repeat(64),
            source_dataset_hash: "b".repeat(64),
            replay_dataset_id: "e".repeat(64),
            declared_class: "RealStructurallyValid".into(),
            profile_id: "c".repeat(64),
            conversion_policy: CONVERSION_POLICY_IDENTITY.into(),
            initial_micro_usd: contracts::INITIAL_EQUITY,
            realized_micro_usd: contracts::INITIAL_EQUITY + net,
            final_marked_micro_usd: contracts::INITIAL_EQUITY + net,
            maximum_drawdown_micro_usd: 0,
            trades: outcomes.resolved,
            rejections: 0,
            receipt_hash: "d".repeat(64),
            outcomes,
        }
    }

    /// Same as [`continuous_month`] but tagged with the reciprocal conversion policy that
    /// applies to non-USD quotes. The continuous accounting never consults a conversion file:
    /// the policy is derived only from the profile quote, so a USD-base instrument such as
    /// USDJPY is accounted across month boundaries with no conversion file and no refusal.
    fn continuous_month_non_usd(
        year: u32,
        month: u32,
        outcomes: TradeOutcomeStats,
    ) -> MonthExecution {
        let net = outcomes.net_micro_usd;
        MonthExecution {
            year,
            month,
            classification: EXECUTION_CLASSIFICATION.into(),
            eligibility: EXECUTION_ELIGIBILITY.into(),
            corpus_identity: "a".repeat(64),
            source_dataset_hash: "b".repeat(64),
            replay_dataset_id: "e".repeat(64),
            declared_class: "RealStructurallyValid".into(),
            profile_id: "c".repeat(64),
            conversion_policy: CONVERSION_POLICY_RECIPROCAL.into(),
            initial_micro_usd: contracts::INITIAL_EQUITY,
            realized_micro_usd: contracts::INITIAL_EQUITY + net,
            final_marked_micro_usd: contracts::INITIAL_EQUITY + net,
            maximum_drawdown_micro_usd: 0,
            trades: outcomes.resolved,
            rejections: 0,
            receipt_hash: "d".repeat(64),
            outcomes,
        }
    }

    fn loss_month(resolved: usize, ending_streak: usize, net_micro_usd: i128) -> TradeOutcomeStats {
        TradeOutcomeStats {
            resolved,
            wins: 0,
            losses: resolved,
            breakevens: 0,
            max_losing_streak: ending_streak,
            ending_streak,
            gross_win_micro_usd: 0,
            gross_loss_micro_usd: -net_micro_usd,
            net_micro_usd,
            unresolved_positions: 0,
        }
    }

    fn breakeven_only_month(resolved: usize) -> TradeOutcomeStats {
        TradeOutcomeStats {
            resolved,
            wins: 0,
            losses: 0,
            breakevens: resolved,
            max_losing_streak: 0,
            ending_streak: 0,
            gross_win_micro_usd: 0,
            gross_loss_micro_usd: 0,
            net_micro_usd: 0,
            unresolved_positions: 0,
        }
    }

    #[test]
    fn chain_losing_streak_keeps_run_across_breakeven_only_month() {
        let loss_two = loss_month(2, 2, -2_000_000);
        let breakevens = breakeven_only_month(3);
        // A breakeven-only month neither resets nor extends the carried run.
        assert_eq!(chain_losing_streak(2, &breakevens), 2);
        // A loss-only month extends it.
        assert_eq!(chain_losing_streak(2, &loss_two), 4);
        // A month with no closed trades keeps the carried run.
        assert_eq!(chain_losing_streak(4, &TradeOutcomeStats::default()), 4);
        // A month containing a win breaks the carried run; its ending streak is measured from
        // that win, so it replaces rather than extends the carried run.
        let win_then_losses = TradeOutcomeStats {
            resolved: 3,
            wins: 1,
            losses: 2,
            breakevens: 0,
            max_losing_streak: 2,
            ending_streak: 2,
            gross_win_micro_usd: 1_000_000,
            gross_loss_micro_usd: 2_000_000,
            net_micro_usd: -1_000_000,
            unresolved_positions: 0,
        };
        assert_eq!(chain_losing_streak(4, &win_then_losses), 2);
    }

    #[test]
    fn aggregate_continuous_chains_streak_over_breakeven_only_month() {
        let months = vec![
            continuous_month(2025, 1, loss_month(2, 2, -2_000_000)),
            continuous_month(2025, 2, breakeven_only_month(3)),
            continuous_month(2025, 3, loss_month(3, 3, -3_000_000)),
        ];
        let refusals = vec![MonthRefusal {
            year: 2025,
            month: 4,
            reason: "unapproved FX conversion".into(),
        }];
        let (continuous, totals) = aggregate_continuous(&months, &refusals).unwrap();
        // The breakeven-only month must not reset the run: 2 + 3 = 5.
        assert_eq!(totals.continuous_max_losing_streak, 5);
        assert_eq!(totals.months_executed, 3);
        assert_eq!(totals.months_refused, 1);
        // Financial totals are unchanged by the streak correction.
        assert_eq!(totals.initial_micro_usd, contracts::INITIAL_EQUITY);
        assert_eq!(
            totals.ending_micro_usd,
            contracts::INITIAL_EQUITY - 5_000_000
        );
        assert_eq!(totals.net_pnl_micro_usd, -5_000_000);
        assert_eq!(totals.full_period_max_drawdown_micro_usd, 5_000_000);
        assert_eq!((totals.wins, totals.losses, totals.breakevens), (0, 5, 3));
        // Month P&L is exactly the closed-trade net; equity and drawdown chain.
        let expected = [
            (
                contracts::INITIAL_EQUITY,
                contracts::INITIAL_EQUITY - 2_000_000,
            ),
            (
                contracts::INITIAL_EQUITY - 2_000_000,
                contracts::INITIAL_EQUITY - 2_000_000,
            ),
            (
                contracts::INITIAL_EQUITY - 2_000_000,
                contracts::INITIAL_EQUITY - 5_000_000,
            ),
        ];
        for (month, (starting, ending)) in continuous.iter().zip(expected) {
            assert_eq!(month.starting_micro_usd, starting);
            assert_eq!(month.ending_micro_usd, ending);
        }
        assert_eq!(
            continuous
                .iter()
                .map(|m| m.monthly_pnl_micro_usd)
                .collect::<Vec<_>>(),
            vec![-2_000_000, 0, -3_000_000]
        );
        // The breakeven-only month's own ending streak is zero, but it did not break the run.
        assert_eq!(continuous[1].month_ending_streak, 0);
        assert_eq!(continuous[1].breakevens, 3);
        assert_eq!(continuous[1].drawdown_from_peak_micro_usd, 2_000_000);
    }

    #[test]
    fn aggregate_continuous_win_breaks_carried_streak() {
        let win_then_losses = TradeOutcomeStats {
            resolved: 3,
            wins: 1,
            losses: 2,
            breakevens: 0,
            max_losing_streak: 2,
            ending_streak: 2,
            gross_win_micro_usd: 1_000_000,
            gross_loss_micro_usd: 2_000_000,
            net_micro_usd: -1_000_000,
            unresolved_positions: 0,
        };
        let months = vec![
            continuous_month(2025, 1, loss_month(2, 2, -2_000_000)),
            continuous_month(2025, 2, win_then_losses),
        ];
        let (_, totals) = aggregate_continuous(&months, &[]).unwrap();
        assert_eq!(totals.continuous_max_losing_streak, 2);
    }

    #[test]
    fn aggregate_continuous_month_pnl_is_closed_trade_net_without_equity_offset() {
        // The removed `initial_micro_usd - INITIAL_EQUITY` term was identically zero for real
        // engine months. Guard that a month whose engine starting equity differs from
        // INITIAL_EQUITY still contributes exactly its closed-trade net to the continuous
        // equity, with no starting-equity offset leaking into the P&L.
        let mut month = continuous_month(
            2025,
            1,
            TradeOutcomeStats {
                resolved: 1,
                wins: 1,
                losses: 0,
                breakevens: 0,
                max_losing_streak: 0,
                ending_streak: 0,
                gross_win_micro_usd: 1_000_000,
                gross_loss_micro_usd: 0,
                net_micro_usd: 1_000_000,
                unresolved_positions: 0,
            },
        );
        month.initial_micro_usd += 7_000_000;
        let (continuous, totals) = aggregate_continuous(&[month], &[]).unwrap();
        assert_eq!(continuous[0].monthly_pnl_micro_usd, 1_000_000);
        assert_eq!(
            totals.ending_micro_usd,
            contracts::INITIAL_EQUITY + 1_000_000
        );
        assert_eq!(totals.net_pnl_micro_usd, 1_000_000);
    }

    #[test]
    fn continuous_accounting_chains_non_usd_instrument_without_conversion_file() {
        // Conversion policy is derived only from the profile quote: USD quotes use the
        // identity conversion and every other quote requires the reciprocal convention.
        // No conversion file exists anywhere in this path, and none is consulted.
        let usdjpy = Profile::synthetic("USDJPY").unwrap();
        assert_eq!(usdjpy.quote, "JPY");
        assert_eq!(conversion_policy(&usdjpy), CONVERSION_POLICY_RECIPROCAL);
        assert_eq!(
            conversion_policy(&Profile::synthetic("EURUSD").unwrap()),
            CONVERSION_POLICY_IDENTITY
        );

        // Two loss-only months of a non-USD instrument, back to back across a month
        // boundary, with no conversion file involved.
        let january = continuous_month_non_usd(2025, 1, loss_month(2, 2, -2_000_000));
        let february = continuous_month_non_usd(2025, 2, loss_month(3, 3, -3_000_000));

        let (continuous, totals) = aggregate_continuous(&[january, february], &[]).unwrap();
        // No refusal: a non-USD profile without a conversion file is accounted, not rejected.
        assert_eq!(totals.months_executed, 2);
        assert_eq!(totals.months_refused, 0);
        // Equity compounds exactly across the boundary: February starts where January ended.
        assert_eq!(continuous[0].starting_micro_usd, contracts::INITIAL_EQUITY);
        assert_eq!(
            continuous[0].ending_micro_usd,
            contracts::INITIAL_EQUITY - 2_000_000
        );
        assert_eq!(
            continuous[1].starting_micro_usd,
            contracts::INITIAL_EQUITY - 2_000_000
        );
        assert_eq!(
            continuous[1].ending_micro_usd,
            contracts::INITIAL_EQUITY - 5_000_000
        );
        assert_eq!(
            totals.ending_micro_usd,
            contracts::INITIAL_EQUITY - 5_000_000
        );
        assert_eq!(totals.net_pnl_micro_usd, -5_000_000);
        // The losing streak survives the month boundary: 2 + 3 = 5.
        assert_eq!(totals.continuous_max_losing_streak, 5);
        assert_eq!(continuous[0].month_ending_streak, 2);
        assert_eq!(continuous[1].month_ending_streak, 3);
        assert_eq!(continuous[1].drawdown_from_peak_micro_usd, 5_000_000);
        assert_eq!(totals.full_period_max_drawdown_micro_usd, 5_000_000);
        assert_eq!((totals.wins, totals.losses, totals.breakevens), (0, 5, 0));
    }

    #[test]
    fn aggregate_totals_sums_only_additive_quantities() {
        let months = vec![
            execution(2025, 1, 20_000_000, 20_500_000, 1_000_000, 10, 100),
            execution(2025, 2, 20_000_000, 19_000_000, 3_000_000, 5, 50),
        ];
        let refusals = vec![MonthRefusal {
            year: 2025,
            month: 3,
            reason: "unapproved FX conversion".into(),
        }];
        let totals = aggregate_totals(&months, &refusals).unwrap();
        assert_eq!(totals.months_executed, 2);
        assert_eq!(totals.months_refused, 1);
        assert_eq!(totals.trades, 15);
        assert_eq!(totals.rejections, 150);
        assert_eq!(totals.initial_micro_usd, 40_000_000);
        assert_eq!(totals.pnl_micro_usd, -500_000);
        assert_eq!(totals.maximum_drawdown_micro_usd, 3_000_000);
    }

    fn trade(net_micro_usd: i128) -> crate::tick_engine::Trade {
        crate::tick_engine::Trade {
            entry_event: 0,
            exit_event: 1,
            entry_ms: 0,
            exit_ms: 1,
            side: crate::contracts::Side::Long,
            entry: 1_000_000,
            exit: 1_000_000,
            stop: 900_000,
            target: 1_300_000,
            volume_micro_lots: 1,
            net_micro_usd,
            commission_micro_usd: 0,
            reason: crate::tick_engine::ExitReason::StopMarket,
            mfe_price_units: 0,
            mae_price_units: 0,
            crossed_gap_ms: 0,
        }
    }

    #[test]
    fn trade_outcome_stats_counts_wins_losses_breakevens_and_streak() {
        let stats = trade_outcome_stats(
            &[
                trade(3_000_000),
                trade(-1_000_000),
                trade(-1_000_000),
                trade(0),
                trade(-1_000_000),
                trade(3_000_000),
            ],
            true,
        )
        .unwrap();
        assert_eq!(
            (stats.resolved, stats.wins, stats.losses, stats.breakevens),
            (6, 2, 3, 1)
        );
        // The breakeven between the two-loss runs does not reset the streak, matching
        // `validation::assess`.
        assert_eq!(stats.max_losing_streak, 3);
        assert_eq!(stats.gross_win_micro_usd, 6_000_000);
        assert_eq!(stats.gross_loss_micro_usd, 3_000_000);
        assert_eq!(stats.net_micro_usd, 3_000_000);
        assert_eq!(stats.unresolved_positions, 1);
    }

    #[test]
    fn trade_outcome_stats_streak_counts_consecutive_net_losses_only() {
        let stats = trade_outcome_stats(
            &[
                trade(-1),
                trade(-1),
                trade(0),
                trade(-1),
                trade(1),
                trade(-1),
            ],
            false,
        )
        .unwrap();
        assert_eq!(stats.max_losing_streak, 3);
        assert_eq!(stats.unresolved_positions, 0);
        assert_eq!(stats.net_micro_usd, -3);
    }

    #[test]
    fn trade_outcome_stats_refuses_overflow() {
        let error = trade_outcome_stats(&[trade(i128::MAX), trade(1)], false)
            .expect_err("an overflowing gross win total must be refused");
        assert!(error.contains("overflow"), "{error}");
        let error = trade_outcome_stats(&[trade(i128::MIN)], false)
            .expect_err("a minimum-value loss magnitude must be refused");
        assert!(error.contains("overflow"), "{error}");
    }

    #[test]
    fn aggregate_totals_sums_outcome_statistics() {
        let months = vec![
            execution_with_outcomes(
                2025,
                1,
                TradeOutcomeStats {
                    resolved: 4,
                    wins: 3,
                    losses: 1,
                    breakevens: 0,
                    max_losing_streak: 1,
                    gross_win_micro_usd: 9_000_000,
                    gross_loss_micro_usd: 1_000_000,
                    net_micro_usd: 8_000_000,
                    unresolved_positions: 1,
                    ending_streak: 0,
                },
            ),
            execution_with_outcomes(
                2026,
                2,
                TradeOutcomeStats {
                    resolved: 2,
                    wins: 1,
                    losses: 1,
                    breakevens: 0,
                    max_losing_streak: 2,
                    gross_win_micro_usd: 3_000_000,
                    gross_loss_micro_usd: 2_000_000,
                    net_micro_usd: 1_000_000,
                    unresolved_positions: 0,
                    ending_streak: 0,
                },
            ),
        ];
        let totals = aggregate_totals(&months, &[]).unwrap();
        assert_eq!(
            (totals.trades, totals.wins, totals.losses, totals.breakevens),
            (6, 4, 2, 0)
        );
        assert_eq!(totals.max_losing_streak, 2);
        assert_eq!(totals.gross_win_micro_usd, 12_000_000);
        assert_eq!(totals.gross_loss_micro_usd, 3_000_000);
        assert_eq!(totals.net_micro_usd, 9_000_000);
        assert_eq!(totals.unresolved_positions, 1);
    }

    #[test]
    fn aggregate_totals_refuses_overflow() {
        let months = vec![
            execution(2025, 1, i128::MAX, 0, 0, 0, 0),
            execution(2025, 2, 1, 0, 0, 0, 0),
        ];
        assert!(aggregate_totals(&months, &[]).is_err());
    }

    fn cache_spec() -> HistoricalCorpus {
        let profile = Profile::synthetic("EURUSD").unwrap();
        let months = contiguous();
        let identity = identity("EURUSD", &profile, &months).unwrap();
        HistoricalCorpus {
            version: VERSION.into(),
            instrument: "EURUSD".into(),
            profile,
            months,
            identity,
            limitations: LIMITATIONS.into(),
        }
    }

    fn cache_strategy() -> crate::strategies::Strategy {
        crate::strategies::Strategy {
            timeframe: crate::candles::Timeframe::M1,
            side: crate::contracts::Side::Long,
            stop_ticks: 100,
            rule: crate::strategies::Rule::AboveMean { window: 2 },
            entry: None,
            stop_policy: None,
        }
    }

    #[test]
    fn execution_cache_key_binds_every_result_input() {
        let spec = cache_spec();
        let strategy = cache_strategy();
        let costs = crate::tick_engine::Costs::default();
        let budget = crate::tick_engine::CorpusBudget {
            max_candles: 1000,
            feature_cells: 1000,
            max_receipts: 1000,
        };
        let other_budget = crate::tick_engine::CorpusBudget {
            max_receipts: 2000,
            ..budget
        };
        let base = ExecutionCache::key(&spec, 0, &strategy, &costs, budget).unwrap();
        assert_eq!(
            base,
            ExecutionCache::key(&spec, 0, &strategy, &costs, budget).unwrap(),
            "cache key must be deterministic"
        );
        assert_ne!(
            base,
            ExecutionCache::key(&spec, 1, &strategy, &costs, budget).unwrap(),
            "different month must not share a key"
        );
        assert_ne!(
            base,
            ExecutionCache::key(&spec, 0, &strategy, &costs, other_budget).unwrap(),
            "different budget must not share a key"
        );
    }

    #[test]
    fn execution_cache_roundtrips_and_refuses_corruption() {
        let dir = std::env::temp_dir().join(format!("exec-cache-roundtrip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = ExecutionCache::open(&dir).unwrap();
        let key = "k".repeat(64);
        assert!(
            cache.load(&key).unwrap().is_none(),
            "absent entry is a miss"
        );
        let value = execution(2025, 1, 20_000_000, 20_500_000, 1_000_000, 10, 100);
        cache.store(&key, &value).unwrap();
        let loaded = cache.load(&key).unwrap().unwrap();
        assert_eq!(
            serde_json::to_string(&loaded).unwrap(),
            serde_json::to_string(&value).unwrap()
        );
        // A truncated entry is refused rather than trusted.
        std::fs::write(dir.join(format!("{key}.json")), b"{ not json").unwrap();
        assert!(cache.load(&key).is_err());
        // A well-formed entry whose self-hash does not match is refused.
        let tampered = CacheEntry {
            entry_hash: "0".repeat(64),
            execution: value.clone(),
        };
        std::fs::write(
            dir.join(format!("{key}.json")),
            serde_json::to_vec(&tampered).unwrap(),
        )
        .unwrap();
        assert!(cache.load(&key).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn accepts_contiguous_ordered_months() {
        validate_months(&contiguous(), 22).unwrap();
    }

    #[test]
    fn refuses_gap_overlap_duplicate_and_wrong_count() {
        let mut gap = contiguous();
        gap.remove(5);
        assert!(validate_months(&gap, 22).is_err());
        let mut overlap = contiguous();
        overlap[6].first_ms = overlap[5].first_ms;
        assert!(validate_months(&overlap, 22).is_err());
        let mut duplicate = contiguous();
        duplicate[7].year = duplicate[6].year;
        duplicate[7].month = duplicate[6].month;
        assert!(validate_months(&duplicate, 22).is_err());
        assert!(validate_months(&contiguous(), 21).is_err());
    }

    #[test]
    fn validate_months_rejects_out_of_range_calendar() {
        let mut alias = contiguous();
        alias.truncate(2);
        alias[0].year = 2025;
        alias[0].month = 12;
        alias[1].year = 2025;
        alias[1].month = 13; // month_index aliases as 2026-01
        assert!(validate_months(&alias, 2).is_err());
    }

    #[test]
    fn owner_selected_window_is_exactly_22_months_2025_01_to_2026_10() {
        let window = ResearchWindow::OWNER_SELECTED;
        window.validate().unwrap();
        assert_eq!(window.span(), 22);
        assert_eq!(window.month_count().unwrap(), 22);
        assert_eq!(EXPECTED_MONTHS, 22);
        assert_eq!(EXPECTED_MONTHS, window.month_count().unwrap());
        let months = window.months().unwrap();
        assert_eq!(months.len(), 22);
        assert_eq!(months[0], (2025, 1));
        assert_eq!(months[11], (2025, 12));
        assert_eq!(months[12], (2026, 1));
        assert_eq!(months[21], (2026, 10));
        assert_eq!(window.label(), "2025-01..2026-10");
        assert!(window.contains(2025, 1));
        assert!(window.contains(2026, 10));
        assert!(!window.contains(2024, 12));
        assert!(!window.contains(2026, 11));
        assert!(!window.contains(2026, 12));
    }

    #[test]
    fn research_window_refuses_invalid_calendar_and_reversed_order() {
        for window in [
            ResearchWindow {
                start_year: 2025,
                start_month: 0,
                end_year: 2026,
                end_month: 10,
            },
            ResearchWindow {
                start_year: 2025,
                start_month: 1,
                end_year: 2026,
                end_month: 13,
            },
            ResearchWindow {
                start_year: 1999,
                start_month: 1,
                end_year: 2026,
                end_month: 10,
            },
            ResearchWindow {
                start_year: 2025,
                start_month: 1,
                end_year: 2201,
                end_month: 10,
            },
            ResearchWindow {
                start_year: 2026,
                start_month: 10,
                end_year: 2025,
                end_month: 1,
            },
        ] {
            assert!(window.validate().is_err(), "{window:?}");
            assert!(window.month_count().is_err(), "{window:?}");
            assert!(window.months().is_err(), "{window:?}");
        }
    }

    #[test]
    fn research_window_enumerates_subwindows_and_roundtrips_json() {
        let window = ResearchWindow {
            start_year: 2025,
            start_month: 11,
            end_year: 2026,
            end_month: 2,
        };
        assert_eq!(
            window.months().unwrap(),
            vec![(2025, 11), (2025, 12), (2026, 1), (2026, 2)]
        );
        assert_eq!(window.month_count().unwrap(), 4);
        let single = ResearchWindow {
            start_year: 2026,
            start_month: 10,
            end_year: 2026,
            end_month: 10,
        };
        assert_eq!(single.months().unwrap(), vec![(2026, 10)]);
        assert_eq!(single.month_count().unwrap(), 1);
        let json = serde_json::to_string(&ResearchWindow::OWNER_SELECTED).unwrap();
        let back: ResearchWindow = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ResearchWindow::OWNER_SELECTED);
        assert!(
            serde_json::from_str::<ResearchWindow>(
                r#"{"start_year":2025,"start_month":1,"end_year":2026,"end_month":10,"extra":1}"#
            )
            .is_err(),
            "unknown window config fields must be refused"
        );
    }

    #[test]
    fn verify_refuses_full_length_specs_outside_the_owner_window() {
        // A contiguous 22-month run shifted one month earlier (2024-12..2026-09) is not the
        // owner-selected window even though its length matches.
        let profile = Profile::synthetic("EURUSD").unwrap();
        let mut shifted = contiguous();
        for source in &mut shifted {
            if source.month == 1 {
                source.year -= 1;
                source.month = 12;
            } else {
                source.month -= 1;
            }
        }
        let identity = identity("EURUSD", &profile, &shifted).unwrap();
        let spec = HistoricalCorpus {
            version: VERSION.into(),
            instrument: "EURUSD".into(),
            profile,
            months: shifted,
            identity,
            limitations: LIMITATIONS.into(),
        };
        let error = verify(&spec)
            .expect_err("a shifted 22-month run must not verify as the owner-selected window");
        assert!(error.contains("owner-selected research window"), "{error}");
        assert!(error.contains("2025-01..2026-10"), "{error}");
        // The canonical-boundary spec passes the window gate and only fails later on the absent
        // synthetic receipts, proving the gate accepts exactly the owner window.
        let canonical = cache_spec();
        let error = verify(&canonical).expect_err("synthetic receipts are absent on disk");
        assert!(!error.contains("owner-selected research window"), "{error}");
    }

    #[test]
    fn identity_is_deterministic_and_content_bound() {
        let profile = Profile::synthetic("EURUSD").unwrap();
        let months = contiguous();
        let first = identity("EURUSD", &profile, &months).unwrap();
        let second = identity("EURUSD", &profile, &months).unwrap();
        assert_eq!(first, second);
        let mut changed = months.clone();
        changed[3].rows += 1;
        assert_ne!(first, identity("EURUSD", &profile, &changed).unwrap());
        assert_ne!(first, identity("GBPUSD", &profile, &months).unwrap());
    }
}
