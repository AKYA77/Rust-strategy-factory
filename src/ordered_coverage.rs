//! Ordered full-history coverage validation for real-data research inputs.
//!
//! A research config may point at a single chunk inside a migrated instrument-month
//! directory. That is a pointer, not complete instrument history. This module expands
//! every configured month through the local migration receipt and verifies every chunk
//! in receipt order (including cross-chunk sequence continuity), then records an
//! ordered coverage receipt. It never downloads data, never changes the corpus and
//! never asserts provider completeness or qualification.

use crate::{Result, contracts::SYMBOLS, historical_migration};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MonthSpec {
    pub instrument: String,
    pub month: String,
    pub directory: PathBuf,
    pub configured_chunk: String,
}

impl MonthSpec {
    /// Derive an instrument-month directory from a configured first-chunk path.
    pub fn from_dataset(path: &str) -> Result<Self> {
        let dataset = Path::new(path);
        let file = dataset
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("research input filename missing")?;
        if !file.starts_with("chunk-") || !file.ends_with(".rsf") {
            return Err("research input must reference a migrated .rsf chunk".into());
        }
        let directory = dataset
            .parent()
            .ok_or("research input has no instrument-month parent")?;
        let name = directory
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("instrument-month directory name missing")?;
        let (instrument, month) = name
            .split_once('-')
            .ok_or("instrument-month directory must be INSTRUMENT-YYYYMM")?;
        if instrument == "XAUUSD" {
            return Err("XAUUSD is outside the seven-instrument V2 scope".into());
        }
        if !SYMBOLS.contains(&instrument) {
            return Err(format!("unsupported research instrument {instrument}"));
        }
        if month.len() != 6 || !month.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("instrument-month directory requires a YYYYMM month".into());
        }
        let year = month[0..4]
            .parse::<i32>()
            .map_err(|_| "invalid coverage year")?;
        let number = month[4..6]
            .parse::<u32>()
            .map_err(|_| "invalid coverage month")?;
        if NaiveDate::from_ymd_opt(year, number, 1).is_none() {
            return Err("invalid coverage month".into());
        }
        Ok(Self {
            instrument: instrument.to_owned(),
            month: month.to_owned(),
            directory: directory.to_path_buf(),
            configured_chunk: file.to_owned(),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MonthCoverage {
    pub instrument: String,
    pub month: String,
    pub chunks: usize,
    pub events: u64,
    pub first_ms: i64,
    pub last_ms: i64,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub chunk_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub version: &'static str,
    pub stage: String,
    pub config: String,
    pub months: usize,
    pub chunks: usize,
    pub events: u64,
    pub first_ms: Option<i64>,
    pub last_ms: Option<i64>,
    pub ordered: Vec<MonthCoverage>,
    pub limitations: &'static str,
}

// These fields are never consumed by the coverage audit, but must be declared so
// `deny_unknown_fields` accepts the operator's existing research config shape.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputFile {
    dataset: String,
    conversions: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResearchFile {
    #[serde(default)]
    limits: serde_json::Value,
    inputs: Vec<InputFile>,
}

/// Validate every configured instrument-month from the local migration manifest.
///
/// `max_months` bounds a partial audit; `None` validates the complete configured
/// corpus. A partial audit is labelled in the report and never presented as full.
pub fn from_config(config: &Path, stage: &str, max_months: Option<usize>) -> Result<Report> {
    if stage.is_empty() || stage.len() > 64 {
        return Err("coverage stage must be 1..64 characters".into());
    }
    let file = fs::File::open(config).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
        return Err("research config exceeds 16 MiB budget".into());
    }
    let parsed: ResearchFile =
        serde_json::from_reader(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    if parsed.inputs.is_empty() {
        return Err("research config has no inputs".into());
    }
    let base = config.parent().unwrap_or_else(|| Path::new("."));
    let mut specs = Vec::new();
    let mut seen = HashSet::new();
    for input in &parsed.inputs {
        let spec = MonthSpec::from_dataset(&input.dataset)?;
        let absolute = base.join(&spec.directory);
        let spec = MonthSpec {
            directory: absolute,
            ..spec
        };
        let key = (spec.instrument.clone(), spec.month.clone());
        if !seen.insert(key) {
            return Err("duplicate instrument-month in research config".into());
        }
        specs.push(spec);
    }
    let selected = max_months.unwrap_or(specs.len());
    if selected == 0 || selected > specs.len() {
        return Err("coverage month limit is outside the configured corpus".into());
    }
    specs.truncate(selected);

    let mut ordered = Vec::new();
    let mut total_events = 0_u64;
    let mut total_chunks = 0_usize;
    let mut first_ms = None;
    let mut last_ms = None;
    for spec in &specs {
        // verify_output reads and verifies every receipt chunk in order, including
        // the cross-chunk sequence and timestamp continuity of the instrument history.
        let receipt = historical_migration::verify_output(&spec.directory, None)?;
        if !receipt
            .chunks
            .iter()
            .any(|chunk| chunk.file == spec.configured_chunk)
        {
            return Err("configured chunk is absent from the verified month manifest".into());
        }
        let chunks = receipt.chunks.len();
        let events = receipt.events;
        total_events = total_events
            .checked_add(events)
            .ok_or("coverage event total overflow")?;
        total_chunks = total_chunks
            .checked_add(chunks)
            .ok_or("coverage chunk total overflow")?;
        first_ms = Some(first_ms.map_or(receipt.first_ms, |current: i64| {
            current.min(receipt.first_ms)
        }));
        last_ms =
            Some(last_ms.map_or(receipt.last_ms, |current: i64| current.max(receipt.last_ms)));
        ordered.push(MonthCoverage {
            instrument: spec.instrument.clone(),
            month: spec.month.clone(),
            chunks,
            events,
            first_ms: receipt.first_ms,
            last_ms: receipt.last_ms,
            first_sequence: receipt.chunks[0].first_sequence,
            last_sequence: receipt.chunks[chunks - 1].last_sequence,
            chunk_files: receipt
                .chunks
                .iter()
                .map(|chunk| chunk.file.clone())
                .collect(),
        });
    }
    Ok(Report {
        version: "ordered-full-history-coverage-v1",
        stage: stage.to_owned(),
        config: config.display().to_string(),
        months: ordered.len(),
        chunks: total_chunks,
        events: total_events,
        first_ms,
        last_ms,
        ordered,
        limitations: if max_months.is_some() {
            "PARTIAL AUDIT: configured month limit truncates the corpus. Verified chunks are complete and ordered within each included instrument-month only. Observed migrated events do not establish provider calendar completeness, research eligibility or scientific qualification."
        } else {
            "Every configured instrument-month was verified against its local migration receipt and every receipt chunk was read in order. Observed migrated events do not establish provider calendar completeness, research eligibility or scientific qualification."
        },
    })
}
