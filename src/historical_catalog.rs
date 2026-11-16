//! Historical corpus catalog: structural status and review gates per instrument and month.
//!
//! This exposes *status* only. It never grants calendar, profile, legal or scientific
//! approval, and never converts a structural pass into research eligibility.

use crate::{
    Result, data,
    data_review::{self, Area, Decision, State as ReviewState},
    historical_corpus,
    historical_migration::{self, Partition},
};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const VERSION: &str = "historical-corpus-catalog-v1";
const LIMITATIONS: &str = "Catalog status only. Structural validity is not calendar completeness, broker/profile approval, licensing, scientific eligibility or research readiness. A rejected or pending gate blocks research eligibility; an accepted assertion is a reviewer-supplied claim bound to evidence, not an independent qualification.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartitionState {
    Verified,
    Missing,
    Noncanonical,
    LoadFailure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartitionStatus {
    pub instrument: String,
    pub year: u32,
    pub month: u32,
    pub state: PartitionState,
    pub dataset_hash: Option<String>,
    pub source_sha256: Option<String>,
    pub parquet_sha256: Option<String>,
    pub rows: u64,
    pub first_ms: Option<i64>,
    pub last_ms: Option<i64>,
    pub timestamp_ties: u64,
    pub gaps_over_hour: u64,
    pub correction_records: u64,
    pub quarantine_records: u64,
    pub load_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gate {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gates {
    pub calendar: Gate,
    pub profile: Gate,
    pub legal: Gate,
    pub scientific: Gate,
    pub evidence: Vec<String>,
}

impl Gates {
    pub fn pending() -> Self {
        Self {
            calendar: Gate::Pending,
            profile: Gate::Pending,
            legal: Gate::Pending,
            scientific: Gate::Pending,
            evidence: Vec::new(),
        }
    }
    pub fn accepted_all(&self) -> bool {
        [self.calendar, self.profile, self.legal, self.scientific]
            .iter()
            .all(|g| *g == Gate::Accepted)
    }
    pub fn any_rejected(&self) -> bool {
        [self.calendar, self.profile, self.legal, self.scientific].contains(&Gate::Rejected)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentStatus {
    pub instrument: String,
    pub months: Vec<PartitionStatus>,
    pub expected_months: usize,
    pub coverage: String,
    pub structural_valid: bool,
    pub gates: Gates,
    pub review_state: Option<ReviewState>,
    pub eligible_for_research: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: String,
    pub instruments: Vec<InstrumentStatus>,
    pub identity: String,
    pub limitations: String,
}

fn array_len(value: &serde_json::Value, key: &str) -> u64 {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map(|items| items.len() as u64)
        .unwrap_or(0)
}

fn field(value: &serde_json::Value, key: &str) -> u64 {
    value.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
}

/// Rows reordered or dropped by the retired importer's source-order correction.
fn correction_records(value: &serde_json::Value) -> u64 {
    field(value, "rows_reordered")
        + array_len(value, "duplicate_blocks_removed")
        + array_len(value, "uncorrectable")
}

/// Quotes removed by the retired importer's quality quarantine.
fn quarantine_records(value: &serde_json::Value) -> u64 {
    let removed = field(value, "crossed_quotes_removed");
    if removed > 0 {
        removed
    } else {
        array_len(value, "first_removed_positions")
    }
}

fn month_dir(output_root: &Path, instrument: &str, year: u32, month: u32) -> std::path::PathBuf {
    output_root.join(format!("{instrument}-{year:04}{month:02}"))
}

fn scan_partition(
    instrument: &str,
    year: u32,
    month: u32,
    canonical: Option<&Partition>,
    output_root: &Path,
) -> PartitionStatus {
    let mut status = PartitionStatus {
        instrument: instrument.into(),
        year,
        month,
        state: PartitionState::Missing,
        dataset_hash: None,
        source_sha256: None,
        parquet_sha256: None,
        rows: 0,
        first_ms: None,
        last_ms: None,
        timestamp_ties: 0,
        gaps_over_hour: 0,
        correction_records: 0,
        quarantine_records: 0,
        load_error: None,
    };
    if canonical.is_none() {
        status.state = PartitionState::Noncanonical;
        return status;
    }
    match historical_migration::verify_receipt_only(&month_dir(
        output_root,
        instrument,
        year,
        month,
    )) {
        Err(error) => {
            status.state = PartitionState::LoadFailure;
            status.load_error = Some(error);
        }
        Ok(receipt) => {
            status.state = PartitionState::Verified;
            status.dataset_hash = Some(receipt.source.dataset_hash);
            status.source_sha256 = Some(receipt.source.source_sha256);
            status.parquet_sha256 = Some(receipt.source.parquet_sha256);
            status.rows = receipt.events;
            status.first_ms = Some(receipt.first_ms);
            status.last_ms = Some(receipt.last_ms);
            status.timestamp_ties = receipt.timestamp_ties;
            status.gaps_over_hour = receipt.gaps_over_hour;
            if let Some(evidence) = &receipt.source.import_evidence {
                status.correction_records = correction_records(&evidence.source_order_correction);
                status.quarantine_records = quarantine_records(&evidence.source_quality_quarantine);
            }
        }
    }
    status
}

fn extra_partitions(output_root: &Path, instrument: &str, plan: &[Partition]) -> Vec<(u32, u32)> {
    let prefix = format!("{instrument}-");
    let mut extras = Vec::new();
    let Ok(entries) = std::fs::read_dir(output_root) else {
        return extras;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(rest) = name.strip_prefix(&prefix) else {
            continue;
        };
        if rest.len() != 6 || !rest.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let year = rest[0..4].parse::<u32>().ok();
        let month = rest[4..6].parse::<u32>().ok();
        if let (Some(year), Some(month)) = (year, month)
            && !plan.iter().any(|p| p.year == year && p.month == month)
        {
            extras.push((year, month));
        }
    }
    extras.sort();
    extras
}

/// Structural status for one instrument. Never asserts calendar or research eligibility.
pub fn scan_instrument(
    root: &Path,
    output_root: &Path,
    instrument: &str,
    expected: usize,
) -> Result<InstrumentStatus> {
    let plan = historical_corpus::plan_instrument(root, instrument)?;
    let mut months: Vec<PartitionStatus> = plan
        .iter()
        .map(|p| scan_partition(instrument, p.year, p.month, Some(p), output_root))
        .collect();
    for (year, month) in extra_partitions(output_root, instrument, &plan) {
        months.push(scan_partition(instrument, year, month, None, output_root));
    }
    months.sort_by_key(|m| (m.year, m.month));
    let verified = months
        .iter()
        .filter(|m| m.state == PartitionState::Verified)
        .count();
    let complete =
        verified == expected && months.iter().all(|m| m.state == PartitionState::Verified);
    let structural_valid = complete && months.iter().all(|m| m.load_error.is_none());
    let mut gates = Gates::pending();
    gates.evidence.push(format!(
        "{instrument}: {verified}/{expected} monthly partitions structurally verified"
    ));
    Ok(InstrumentStatus {
        instrument: instrument.into(),
        months,
        expected_months: expected,
        coverage: if complete {
            "complete".into()
        } else {
            "gaps".into()
        },
        structural_valid,
        gates,
        review_state: None,
        eligible_for_research: false,
    })
}

pub fn scan_catalog(
    root: &Path,
    output_root: &Path,
    instruments: &[&str],
    expected: usize,
) -> Result<Catalog> {
    let mut statuses = Vec::new();
    for instrument in instruments {
        statuses.push(scan_instrument(root, output_root, instrument, expected)?);
    }
    let identity = data::hash(&(
        VERSION,
        statuses
            .iter()
            .map(|s| {
                (
                    &s.instrument,
                    s.expected_months,
                    &s.coverage,
                    s.structural_valid,
                    s.months
                        .iter()
                        .map(|m| {
                            (
                                m.year,
                                m.month,
                                m.state,
                                &m.dataset_hash,
                                &m.source_sha256,
                                m.rows,
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>(),
    ))?;
    Ok(Catalog {
        version: VERSION.into(),
        instruments: statuses,
        identity,
        limitations: LIMITATIONS.into(),
    })
}

impl Catalog {
    pub fn verify(&self) -> Result<()> {
        if self.version != VERSION || self.limitations != LIMITATIONS {
            return Err("invalid historical catalog".into());
        }
        let expected = data::hash(&(
            VERSION,
            self.instruments
                .iter()
                .map(|s| {
                    (
                        &s.instrument,
                        s.expected_months,
                        &s.coverage,
                        s.structural_valid,
                        s.months
                            .iter()
                            .map(|m| {
                                (
                                    m.year,
                                    m.month,
                                    m.state,
                                    &m.dataset_hash,
                                    &m.source_sha256,
                                    m.rows,
                                )
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>(),
        ))?;
        if expected != self.identity {
            return Err("historical catalog identity mismatch".into());
        }
        for instrument in &self.instruments {
            if instrument.months.len() != instrument.expected_months {
                return Err("historical catalog month count mismatch".into());
            }
            let structural_valid = instrument.coverage == "complete"
                && instrument
                    .months
                    .iter()
                    .all(|m| m.state == PartitionState::Verified);
            if structural_valid != instrument.structural_valid {
                return Err("historical catalog structural flag mismatch".into());
            }
            let eligible = structural_valid && instrument.gates.accepted_all();
            if eligible != instrument.eligible_for_research {
                return Err("historical catalog eligibility mismatch".into());
            }
        }
        Ok(())
    }

    /// Record explicit gate decisions with retained evidence. Structural validity is still
    /// required for eligibility; a pending or rejected gate blocks it.
    pub fn bind_assertions(
        &mut self,
        calendar: Gate,
        profile: Gate,
        legal: Gate,
        scientific: Gate,
        evidence: Vec<String>,
    ) {
        for instrument in &mut self.instruments {
            instrument.gates.calendar = calendar;
            instrument.gates.profile = profile;
            instrument.gates.legal = legal;
            instrument.gates.scientific = scientific;
            instrument.gates.evidence.extend(evidence.iter().cloned());
            instrument.eligible_for_research =
                instrument.structural_valid && instrument.gates.accepted_all();
        }
    }

    /// Bind a receipt-bound review report to this catalog identity and update the gates it
    /// covers. Legal and scientific gates remain pending unless separately recorded.
    pub fn bind_review(&mut self, report: &data_review::Report) -> Result<()> {
        if report.record.request.catalog_identity != self.identity {
            return Err("review catalog identity does not match this catalog".into());
        }
        for instrument in &mut self.instruments {
            let mut changed = false;
            for assessment in &report.record.request.assessments {
                let gate = match assessment.area {
                    Area::Calendar => Some(&mut instrument.gates.calendar),
                    Area::Profile => Some(&mut instrument.gates.profile),
                    _ => None,
                };
                if let Some(gate) = gate {
                    *gate = match assessment.decision {
                        Decision::AcceptedAssertion => Gate::Accepted,
                        Decision::Rejected => Gate::Rejected,
                        Decision::Pending => Gate::Pending,
                    };
                    changed = true;
                } else {
                    instrument.gates.evidence.push(format!(
                        "{:?}: {:?} evidence {}",
                        assessment.area,
                        assessment.decision,
                        report.record.evidence_hashes
                            [assessment_index(&report.record.request.assessments, assessment.area)]
                    ));
                }
            }
            if changed {
                instrument.review_state = Some(report.state);
                instrument.eligible_for_research =
                    instrument.structural_valid && instrument.gates.accepted_all();
            }
        }
        Ok(())
    }
}

fn assessment_index(assessments: &[data_review::Assessment], area: Area) -> usize {
    assessments
        .iter()
        .position(|a| a.area == area)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gates(calendar: Gate, profile: Gate, legal: Gate, scientific: Gate) -> Gates {
        Gates {
            calendar,
            profile,
            legal,
            scientific,
            evidence: vec!["e".into()],
        }
    }

    #[test]
    fn pending_gates_never_grant_eligibility() {
        assert!(!Gates::pending().accepted_all());
        assert!(!Gates::pending().any_rejected());
        assert!(
            !gates(
                Gate::Accepted,
                Gate::Accepted,
                Gate::Pending,
                Gate::Accepted
            )
            .accepted_all()
        );
        assert!(
            gates(
                Gate::Accepted,
                Gate::Accepted,
                Gate::Accepted,
                Gate::Accepted
            )
            .accepted_all()
        );
        assert!(
            gates(
                Gate::Accepted,
                Gate::Rejected,
                Gate::Accepted,
                Gate::Accepted
            )
            .any_rejected()
        );
    }

    #[test]
    fn scan_partition_marks_missing_and_noncanonical() {
        let _root = Path::new("/nonexistent-root");
        let output = Path::new("/nonexistent-output");
        let missing = scan_partition("EURUSD", 2025, 1, None, output);
        assert_eq!(missing.state, PartitionState::Noncanonical);
        let canonical = Partition {
            instrument: "EURUSD".into(),
            year: 2025,
            month: 1,
            dataset_hash: "d".into(),
            parquet_path: "/nonexistent-root/x.parquet".into(),
            manifest_path: "/nonexistent-root/x.json".into(),
            import_receipt_path: None,
            rows: 10,
            price_scale: 1_000_000,
            source_sha256: "a".repeat(64),
            quote_stream_sha256: "b".repeat(64),
        };
        let missing = scan_partition("EURUSD", 2025, 1, Some(&canonical), output);
        assert_eq!(missing.state, PartitionState::LoadFailure);
        assert!(missing.load_error.is_some());
    }

    #[test]
    fn explicit_gates_control_eligibility() {
        let mut catalog = Catalog {
            version: VERSION.into(),
            instruments: vec![InstrumentStatus {
                instrument: "EURUSD".into(),
                months: vec![PartitionStatus {
                    instrument: "EURUSD".into(),
                    year: 2025,
                    month: 1,
                    state: PartitionState::Verified,
                    dataset_hash: Some("d".into()),
                    source_sha256: Some("a".repeat(64)),
                    parquet_sha256: Some("b".repeat(64)),
                    rows: 1,
                    first_ms: Some(0),
                    last_ms: Some(1),
                    timestamp_ties: 0,
                    gaps_over_hour: 0,
                    correction_records: 0,
                    quarantine_records: 0,
                    load_error: None,
                }],
                expected_months: 1,
                coverage: "complete".into(),
                structural_valid: true,
                gates: Gates::pending(),
                review_state: None,
                eligible_for_research: false,
            }],
            identity: "0".repeat(64),
            limitations: LIMITATIONS.into(),
        };
        catalog.bind_assertions(
            Gate::Accepted,
            Gate::Accepted,
            Gate::Accepted,
            Gate::Accepted,
            vec!["legal+scientific evidence".into()],
        );
        assert!(catalog.instruments[0].eligible_for_research);
        assert_eq!(catalog.instruments[0].gates.evidence.len(), 1);
        catalog.bind_assertions(
            Gate::Accepted,
            Gate::Accepted,
            Gate::Pending,
            Gate::Accepted,
            vec![],
        );
        assert!(!catalog.instruments[0].eligible_for_research);
    }

    #[test]
    fn evidence_counts_match_legacy_shape() {
        let correction = serde_json::json!({
            "rows_reordered": 126,
            "duplicate_blocks_removed": [],
            "uncorrectable": []
        });
        assert_eq!(correction_records(&correction), 126);
        let quarantine = serde_json::json!({"crossed_quotes_removed": 353});
        assert_eq!(quarantine_records(&quarantine), 353);
        let positional = serde_json::json!({"first_removed_positions": [1, 2, 3]});
        assert_eq!(quarantine_records(&positional), 3);
        assert_eq!(
            correction_records(&serde_json::json!({"duplicate_blocks_removed": [1]})),
            1
        );
    }
}
