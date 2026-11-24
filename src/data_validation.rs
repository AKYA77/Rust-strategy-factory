//! Structural data validation for historical tick datasets.
//!
//! V2 mandate addendum (P0): equal timestamps are **not** inherently duplicates.
//! The canonical source contract orders events by `(timestamp_ms, sequence)`;
//! distinct events at the same timestamp are legitimate and identical prices are
//! legitimate. Gaps are measured in time, never by comparing a price-unit profile
//! tick against milliseconds, and market closure is not automatically a data gap.
//!
//! Validation is streaming and bounded: counters are exact, while issue examples
//! are capped. Structural validity is NOT source/licensing/calendar/scientific
//! approval — `passed` only means the structural invariants held.

use crate::Result;
use serde::{Deserialize, Serialize};

/// Cap on retained example issues; aggregate counters remain exact.
const MAX_EXAMPLES: usize = 64;
/// Gaps above one hour are reported as informational evidence, not as a failure.
const LARGE_GAP_MS: i64 = 3_600_000;

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
    /// Repeated `(timestamp_ms, sequence)` identities — NOT timestamp ties.
    pub duplicate_timestamps: usize,
    /// Legitimate ordered events sharing a timestamp.
    pub timestamp_ties: usize,
    pub non_chronological_transitions: usize,
    pub gap_count: usize,
    pub max_gap_ms: i64,
    /// How many example issues were not retained by the bounded collector.
    pub suppressed_examples: usize,
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
            timestamp_ties: 0,
            non_chronological_transitions: 0,
            gap_count: 0,
            max_gap_ms: 0,
            suppressed_examples: 0,
            issues: Vec::new(),
            passed: false,
        }
    }

    pub fn add_issue(
        &mut self,
        severity: IssueSeverity,
        category: &str,
        description: &str,
        timestamp_ms: Option<i64>,
        count: usize,
    ) {
        if self.issues.len() < MAX_EXAMPLES {
            self.issues.push(QualityIssue {
                severity,
                category: category.to_string(),
                description: description.to_string(),
                timestamp_ms,
                count,
            });
        } else {
            self.suppressed_examples = self.suppressed_examples.saturating_add(1);
        }
    }
}

/// Validate a dataset in one bounded streaming pass.
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
    let mut prev_sequence: u64 = 0;
    let mut gap_anchor: Option<i64> = None;

    for tick in ticks.iter() {
        if tick.validate(profile).is_ok() {
            report.valid_ticks = report.valid_ticks.saturating_add(1);
        } else {
            report.invalid_ticks = report.invalid_ticks.saturating_add(1);
            report.add_issue(
                IssueSeverity::Error,
                "invalid_quote",
                "Nonpositive, crossed, off-grid or otherwise invalid quote",
                Some(tick.timestamp_ms),
                1,
            );
        }

        match prev_timestamp {
            None => {}
            Some(prev) if tick.timestamp_ms < prev => {
                report.non_chronological_transitions =
                    report.non_chronological_transitions.saturating_add(1);
                report.add_issue(
                    IssueSeverity::Error,
                    "non_chronological",
                    "Timestamp goes backward (not chronological)",
                    Some(tick.timestamp_ms),
                    1,
                );
            }
            Some(prev) => {
                if tick.timestamp_ms == prev {
                    report.timestamp_ties = report.timestamp_ties.saturating_add(1);
                }
                // Canonical identity is (timestamp, sequence). A non-increasing
                // sequence is a repeated identity or an out-of-order event.
                if tick.sequence <= prev_sequence {
                    report.duplicate_timestamps = report.duplicate_timestamps.saturating_add(1);
                    report.add_issue(
                        IssueSeverity::Error,
                        "repeated_event_identity",
                        "Event (timestamp, sequence) identity repeats or moves backward",
                        Some(tick.timestamp_ms),
                        1,
                    );
                }
            }
        }

        // Time-based gap evidence between distinct timestamps. No price-unit
        // threshold, no calendar assumption: a closed market is not a data gap.
        if let Some(anchor) = gap_anchor {
            if tick.timestamp_ms > anchor {
                let gap = tick.timestamp_ms - anchor; // positive, cannot overflow
                if gap > LARGE_GAP_MS {
                    report.gap_count = report.gap_count.saturating_add(1);
                    if gap > report.max_gap_ms {
                        report.max_gap_ms = gap;
                    }
                    report.add_issue(
                        IssueSeverity::Info,
                        "large_time_gap",
                        &format!("Time gap of {gap} ms between successive distinct events"),
                        Some(tick.timestamp_ms),
                        1,
                    );
                }
                gap_anchor = Some(tick.timestamp_ms);
            }
        } else {
            gap_anchor = Some(tick.timestamp_ms);
        }

        prev_timestamp = Some(tick.timestamp_ms);
        prev_sequence = tick.sequence;
    }

    // Structural success only. Manifest issues are carried as warnings; source,
    // licensing, calendar and scientific approvals remain separate authorities.
    for issue in &dataset.manifest.issues {
        report.add_issue(IssueSeverity::Warning, "manifest", issue, None, 1);
    }
    if report.suppressed_examples > 0 {
        report.add_issue(
            IssueSeverity::Info,
            "examples_suppressed",
            "Additional issue examples suppressed by the bounded collector",
            None,
            report.suppressed_examples,
        );
    }

    report.passed = report.invalid_ticks == 0
        && report.non_chronological_transitions == 0
        && report.duplicate_timestamps == 0
        && !report
            .issues
            .iter()
            .any(|issue| issue.severity == IssueSeverity::Critical);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contracts::Profile, data::DataClass, data::Manifest};

    fn make_tick(timestamp_ms: i64, sequence: u64, bid: i64, ask: i64) -> crate::data::Tick {
        crate::data::Tick {
            timestamp_ms,
            sequence,
            bid,
            ask,
        }
    }

    fn make_profile() -> Profile {
        Profile {
            symbol: "EURUSD".to_string(),
            version: "test-v1".into(),
            base: "EUR".to_string(),
            quote: "USD".to_string(),
            price_scale: 100_000,
            tick: 1,
            pip: 10,
            contract_units: 100_000,
            min_micro_lots: 10,
            step_micro_lots: 10,
            max_micro_lots: 1_000_000,
            leverage: 100,
            min_stop_ticks: 1,
            provenance: "test".into(),
            research_only: false,
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
            make_tick(1000, 1, 100000, 100010),
            make_tick(2000, 2, 100010, 100020),
            make_tick(3000, 3, 100020, 100030),
        ];
        let report = validate_dataset(&make_dataset(ticks)).unwrap();
        assert!(report.passed);
        assert_eq!(report.valid_ticks, 3);
        assert_eq!(report.invalid_ticks, 0);
        assert_eq!(report.duplicate_timestamps, 0);
    }

    #[test]
    fn test_ordered_timestamp_tie_is_legitimate() {
        let ticks = vec![
            make_tick(1000, 1, 100000, 100010),
            make_tick(1000, 2, 100000, 100010), // identical price, distinct event
            make_tick(2000, 3, 100010, 100020),
        ];
        let report = validate_dataset(&make_dataset(ticks)).unwrap();
        assert!(report.passed, "a timestamp tie is not a duplicate");
        assert_eq!(report.timestamp_ties, 1);
        assert_eq!(report.duplicate_timestamps, 0);
    }

    #[test]
    fn test_repeated_event_identity_is_rejected() {
        let ticks = vec![
            make_tick(1000, 1, 100000, 100010),
            make_tick(1000, 1, 100000, 100010),
            make_tick(2000, 3, 100010, 100020),
        ];
        let report = validate_dataset(&make_dataset(ticks)).unwrap();
        assert!(!report.passed);
        assert_eq!(report.timestamp_ties, 1);
        assert_eq!(report.duplicate_timestamps, 1);
    }

    #[test]
    fn test_out_of_order_sequence_is_rejected() {
        let ticks = vec![
            make_tick(1000, 5, 100000, 100010),
            make_tick(2000, 4, 100010, 100020),
        ];
        let report = validate_dataset(&make_dataset(ticks)).unwrap();
        assert!(!report.passed);
        assert_eq!(report.non_chronological_transitions, 0);
        assert_eq!(report.duplicate_timestamps, 1);
    }

    #[test]
    fn test_non_chronological_detected() {
        let ticks = vec![
            make_tick(2000, 1, 100010, 100020),
            make_tick(1000, 2, 100000, 100010),
        ];
        let report = validate_dataset(&make_dataset(ticks)).unwrap();
        assert!(!report.passed);
        assert_eq!(report.non_chronological_transitions, 1);
    }

    #[test]
    fn test_invalid_quote_detected() {
        let ticks = vec![
            make_tick(1000, 1, 100000, 100010),
            make_tick(2000, 2, 0, 100020),
        ];
        let report = validate_dataset(&make_dataset(ticks)).unwrap();
        assert!(!report.passed);
        assert_eq!(report.invalid_ticks, 1);
    }

    #[test]
    fn test_time_gap_is_evidence_not_a_failure() {
        let ticks = vec![
            make_tick(1000, 1, 100000, 100010),
            make_tick(1000 + 7_200_000, 2, 100000, 100010),
        ];
        let report = validate_dataset(&make_dataset(ticks)).unwrap();
        assert!(
            report.passed,
            "market closure must not fail structural validation"
        );
        assert_eq!(report.gap_count, 1);
        assert_eq!(report.max_gap_ms, 7_200_000);
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.severity == IssueSeverity::Info && i.category == "large_time_gap")
        );
    }

    #[test]
    fn test_examples_are_bounded_and_counters_exact() {
        let ticks: Vec<_> = (1..=200)
            .map(|i| make_tick(i * 1000, i as u64, 0, 100020))
            .collect();
        let report = validate_dataset(&make_dataset(ticks)).unwrap();
        assert_eq!(report.invalid_ticks, 200);
        assert_eq!(report.total_ticks, 200);
        assert!(report.issues.len() <= MAX_EXAMPLES);
        assert!(report.suppressed_examples > 0);
    }

    #[test]
    fn test_empty_dataset_fails() {
        let report = validate_dataset(&make_dataset(Vec::new())).unwrap();
        assert!(!report.passed);
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.severity == IssueSeverity::Critical)
        );
    }

    #[test]
    fn test_manifest_issue_is_not_approval() {
        let mut dataset = make_dataset(vec![
            make_tick(1000, 1, 100000, 100010),
            make_tick(2000, 2, 100010, 100020),
        ]);
        dataset
            .manifest
            .issues
            .push("source provenance pending".into());
        let report = validate_dataset(&dataset).unwrap();
        assert!(report.passed, "structural pass is not source approval");
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.category == "manifest" && i.severity == IssueSeverity::Warning)
        );
    }
}
