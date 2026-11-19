//! Explicit multi-month historical corpus over independently verified monthly native sources.
//!
//! This module registers and traverses artifacts that already exist; it does not create a
//! second replay engine and does not copy chunk data. Every month keeps its own source
//! identity and provenance; the aggregate identity is a deterministic hash over those source
//! identities, never a source-approval claim.

use crate::{
    Result,
    contracts::Profile,
    data::{self, Tick},
    historical_migration::{self, Partition},
    native,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const VERSION: &str = "historical-monthly-corpus-v2";
/// 2025-01 through 2026-10 for the eight canonical instruments.
pub const EXPECTED_MONTHS: usize = 22;
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

fn month_index(year: u32, month: u32) -> u64 {
    year as u64 * 12 + month as u64
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
/// The master blueprint asks for a configurable exact 24-completed-month research window. The
/// canonical corpus currently holds fewer completed months, so this reports the precise shortfall
/// instead of padding, truncating silently or fabricating periods. The selection is the latest
/// contiguous run, so a window never spans a gap.
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
    pub initial_micro_usd: i128,
    /// The engine's `realized_micro_usd` field, which is the ending account balance (it starts at
    /// `contracts::INITIAL_EQUITY` and is not a profit figure by itself).
    pub realized_micro_usd: i128,
    pub final_marked_micro_usd: i128,
    pub maximum_drawdown_micro_usd: i128,
    pub trades: usize,
    pub rejections: usize,
    pub receipt_hash: String,
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
const EXECUTION_LIMITATIONS: &str = "Diagnostic simulation only. The authoritative sequential engine and frozen financial rules are applied to real structurally-valid literal-USD history using explicit test-convention profiles, so results are not broker-approved execution. No research eligibility, calendar/profile/licensing approval, scientific validity or qualification is granted. Non-USD-quote instruments and eligibility-bearing data classes are refused. Counts and money figures are observations, not performance evidence.";

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
        initial_micro_usd: replay.initial_micro_usd,
        realized_micro_usd: replay.realized_micro_usd,
        final_marked_micro_usd: replay.final_marked_micro_usd,
        maximum_drawdown_micro_usd: replay.maximum_drawdown_micro_usd,
        trades: replay.trades.len(),
        rejections: replay.rejections.len(),
        receipt_hash: crate::data::hash(&replay)?,
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
    };
    for executed in months {
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

const EXECUTION_CACHE_VERSION: &str = "historical-diagnostic-execution-cache-v2";

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
            initial_micro_usd: initial,
            realized_micro_usd: marked,
            final_marked_micro_usd: marked,
            maximum_drawdown_micro_usd: drawdown,
            trades,
            rejections,
            receipt_hash: "d".repeat(64),
        }
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
