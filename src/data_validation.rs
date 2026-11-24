//! Data validation for historical market data.
//! Validates bid/ask integrity, timestamp ordering, duplicate detection,
//! and gap detection. Structural import success is not sufficient for
//! research approval — semantic quality must also be verified.

use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Severity of a data quality issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum IssueSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// A single data quality issue found during validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityIssue {
    pub severity: IssueSeverity,
    pub category: String,
    pub description: String,
    pub timestamp_ms: Option<i64>,
    pub count: usize,
}

/// Summary of data quality validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityReport {
    pub total_ticks: usize,
    pub valid_ticks: usize,
    pub invalid_ticks: usize,
    pub duplicate_timestamps: usize,
    pub non_chronological_transitions: usize,
    pub gap_count: usize,
    pub max_gap_ms: i64,
    pub issues: Vec<QualityIssue>,
    pub passed: bool,
}

impl QualityReport {
    pub fn new(total_ticks: usize) -> Self {
        Self {
            total_ticks,
            valid_ticks: 0,
            invalid_ticks: 0,
            duplicate_timestamps: 0,
            non_chronological_transitions: 0,
            gap_count: 0,
            max_gap_ms: 0,
            issues: Vec::new(),
            passed: false,
        }
    }

    pub fn add_issue(&mut self, severity: IssueSeverity, category: &str, description: &str, timestamp_ms: Option<i64>, count: usize) {
        self.issues.push(QualityIssue {
            severity,
            category: category.to_string(),
            description: description.to_string(),
            timestamp_ms,
            count,
        });
    }
}

/// Validate a dataset's tick data for quality issues.
pub fn validate_dataset(dataset: &crate::data::Dataset) -> Result<QualityReport> {
    let ticks = &dataset.ticks;
    let profile = &dataset.profile;
    let mut report = QualityReport::new(ticks.len());

    if ticks.is_empty() {
        report.add_issue(
            IssueSeverity::Critical,
            "empty_dataset",
            "Dataset contains no ticks",
            None,
            0,
        );
        return Ok(report);
    }

    let mut prev_timestamp: Option<i64> = None;
    let mut seen_timestamps: HashSet<i64> = HashSet::new();
    let mut prev_time_for_gap: Option<i64> = None;
    let profile_tick = profile.tick;

    for tick in ticks.iter() {
        // Validate bid/ask integrity
        let tick_valid = tick.validate(profile).is_ok();
        if !tick_valid {
            report.invalid_ticks += 1;
            report.add_issue(
                IssueSeverity::Error,
                "invalid_quote",
                "Nonpositive, crossed, or off-grid quote",
                Some(tick.timestamp_ms),
                1,
            );
        } else {
            report.valid_ticks += 1;
        }

        // Check for duplicate timestamps
        if !seen_timestamps.insert(tick.timestamp_ms) {
            report.duplicate_timestamps += 1;
            report.add_issue(
                IssueSeverity::Warning,
                "duplicate_timestamp",
                "Duplicate timestamp detected",
                Some(tick.timestamp_ms),
                1,
            );
        }

        // Check chronological ordering
        if let Some(prev) = prev_timestamp {
            if tick.timestamp_ms < prev {
                report.non_chronological_transitions += 1;
                report.add_issue(
                    IssueSeverity::Error,
                    "non_chronological",
                    "Timestamp goes backward (not chronological)",
                    Some(tick.timestamp_ms),
                    1,
                );
            } else if tick.timestamp_ms == prev {
                // Duplicate already counted above
            } else {
                // Check for gaps
                if let Some(prev_time) = prev_time_for_gap {
                    let gap = tick.timestamp_ms - prev_time;
                    if gap > profile_tick as i64 * 2 {
                        // Gap larger than 2 ticks
                        report.gap_count += 1;
                        if gap > report.max_gap_ms {
                            report.max_gap_ms = gap;
                        }
                        if gap > 3600_000 {
                            // Gap larger than 1 hour
                            report.add_issue(
                                IssueSeverity::Warning,
                                "large_gap",
                                &format!("Gap of {} ms (>1 hour) between ticks", gap),
                                Some(tick.timestamp_ms),
                                1,
                            );
                        }
                    }
                }
                prev_time_for_gap = Some(tick.timestamp_ms);
            }
        } else {
            prev_time_for_gap = Some(tick.timestamp_ms);
        }
        prev_timestamp = Some(tick.timestamp_ms);
    }

    // Determine if validation passed
    let has_critical_errors = report.invalid_ticks > 0
        || report.non_chronological_transitions > 0
        || report.issues.iter().any(|i| i.severity == IssueSeverity::Critical);

    report.passed = !has_critical_errors && report.duplicate_timestamps == 0;

    // Add manifest-level issues
    for issue in &dataset.manifest.issues {
        report.add_issue(
            IssueSeverity::Warning,
            "manifest",
            issue,
            None,
            1,
        );
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contracts::Profile, data::Manifest, data::DataClass};

    fn make_tick(timestamp_ms: i64, bid: i64, ask: i64) -> crate::data::Tick {
        crate::data::Tick {
            timestamp_ms,
            sequence: 0,
            bid,
            ask,
        }
    }

    fn make_profile() -> Profile {
        Profile {
            symbol: "EURUSD".to_string(),
            base: "EUR".to_string(),
            quote: "USD".to_string(),
            tick: 100,
            lot_size: 100000,
            min_lot: 1000,
            lot_step: 1000,
            margin_currency: "USD".to_string(),
            point: 100,
            digits: 5,
        }
    }

    fn make_dataset(ticks: Vec<crate::data::Tick>) -> crate::data::Dataset {
        crate::data::Dataset {
            manifest: Manifest {
                parser_version: "test-v1".to_string(),
                source: "test".to_string(),
                imported_at: "2025-01-01T00:00:00Z".to_string(),
                archive_hash: "test".to_string(),
                content_hash: "test".to_string(),
                profile_hash: "test".to_string(),
                symbol: "EURUSD".to_string(),
                class: DataClass::RealStructurallyValid,
                events: ticks.len(),
                first_ms: ticks.first().map(|t| t.timestamp_ms).unwrap_or(0),
                last_ms: ticks.last().map(|t| t.timestamp_ms).unwrap_or(0),
                timestamp_ties: 0,
                identical_quote_events: 0,
                gaps_over_hour: 0,
                issues: Vec::new(),
            },
            profile: make_profile(),
            ticks,
        }
    }

    #[test]
    fn test_valid_dataset_passes() {
        let ticks = vec![
            make_tick(1000, 100000, 100010),
            make_tick(2000, 100010, 100020),
            make_tick(3000, 100020, 100030),
        ];
        let dataset = make_dataset(ticks);
        let report = validate_dataset(&dataset).unwrap();
        assert!(report.passed);
        assert_eq!(report.valid_ticks, 3);
        assert_eq!(report.invalid_ticks, 0);
        assert_eq!(report.duplicate_timestamps, 0);
    }

    #[test]
    fn test_duplicate_timestamps_detected() {
        let ticks = vec![
            make_tick(1000, 100000, 100010),
            make_tick(1000, 100005, 100015), // duplicate timestamp
            make_tick(2000, 100010, 100020),
        ];
        let dataset = make_dataset(ticks);
        let report = validate_dataset(&dataset).unwrap();
        assert!(!report.passed);
        assert_eq!(report.duplicate_timestamps, 1);
    }

    #[test]
    fn test_non_chronological_detected() {
        let ticks = vec![
            make_tick(2000, 100010, 100020),
            make_tick(1000, 100000, 100010), // goes backward
        ];
        let dataset = make_dataset(ticks);
        let report = validate_dataset(&dataset).unwrap();
        assert!(!report.passed);
        assert_eq!(report.non_chronological_transitions, 1);
    }

    #[test]
    fn test_invalid_quote_detected() {
        let ticks = vec![
            make_tick(1000, 100000, 100010),
            make_tick(2000, 0, 100020), // bid = 0, invalid
        ];
        let dataset = make_dataset(ticks);
        let report = validate_dataset(&dataset).unwrap();
        assert!(!report.passed);
        assert_eq!(report.invalid_ticks, 1);
    }

    #[test]
    fn test_empty_dataset_fails() {
        let dataset = make_dataset(Vec::new());
        let report = validate_dataset(&dataset).unwrap();
        assert!(!report.passed);
    }
}
