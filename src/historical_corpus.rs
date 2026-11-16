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

pub const VERSION: &str = "historical-monthly-corpus-v1";
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
                    m.year,
                    m.month,
                    &m.dataset_hash,
                    &m.source_sha256,
                    &m.parquet_sha256,
                    m.rows,
                    m.first_ms,
                    m.last_ms,
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
    let mut months = Vec::with_capacity(partitions.len());
    let mut profile: Option<Profile> = None;
    for partition in &partitions {
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
            chunk_files: receipt.chunks.iter().map(|c| c.file.clone()).collect(),
            chunk_hashes: receipt
                .chunks
                .iter()
                .map(|c| c.dataset_hash.clone())
                .collect(),
        });
    }
    let profile = profile.ok_or("no monthly sources")?;
    validate_months(&months, expected)?;
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
    validate_months(&spec.months, spec.months.len())?;
    for month in &spec.months {
        let dir = Path::new(&month.output_dir);
        let receipt = historical_migration::verify_output(dir, None)?;
        if receipt.source.dataset_hash != month.dataset_hash
            || receipt.source.source_sha256 != month.source_sha256
            || receipt.source.rows != month.rows
            || receipt.first_ms != month.first_ms
            || receipt.last_ms != month.last_ms
            || receipt.profile != spec.profile
        {
            return Err("historical corpus month receipt mismatch".into());
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
            if !dataset.manifest.source.contains(&month.dataset_hash) {
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
