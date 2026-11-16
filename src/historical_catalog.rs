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

pub const VERSION: &str = "historical-corpus-catalog-v2";
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
pub struct GateDetail {
    pub state: Gate,
    pub required_evidence: Vec<String>,
    pub evidence: Vec<String>,
    pub dataset_identity: Option<String>,
    pub receipt: Option<String>,
    pub explanation: String,
}

impl GateDetail {
    pub fn pending(required: &[&str], explanation: &str) -> Self {
        Self {
            state: Gate::Pending,
            required_evidence: required.iter().map(|s| (*s).to_string()).collect(),
            evidence: Vec::new(),
            dataset_identity: None,
            receipt: None,
            explanation: explanation.into(),
        }
    }
    pub fn accept(&mut self, dataset_identity: &str, receipt: &str, evidence: Vec<String>) {
        if evidence.iter().all(|e| e.trim().is_empty()) {
            self.state = Gate::Pending;
            self.dataset_identity = None;
            self.receipt = None;
            self.explanation =
                "Pending: acceptance requires retained evidence; missing evidence is not accepted."
                    .into();
            return;
        }
        self.state = Gate::Accepted;
        self.dataset_identity = Some(dataset_identity.into());
        self.receipt = Some(receipt.into());
        self.evidence = evidence;
        self.explanation = format!(
            "Accepted for dataset {dataset_identity} via receipt {receipt}; a reviewer-supplied claim bound to evidence, not independent verification."
        );
    }
    pub fn reject(&mut self, explanation: &str) {
        self.state = Gate::Rejected;
        self.dataset_identity = None;
        self.receipt = None;
        self.explanation = explanation.into();
    }
    pub fn set_pending(&mut self) {
        self.state = Gate::Pending;
        self.dataset_identity = None;
        self.receipt = None;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GateDecisions {
    pub calendar: Gate,
    pub profile: Gate,
    pub legal: Gate,
    pub scientific: Gate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gates {
    pub calendar: GateDetail,
    pub profile: GateDetail,
    pub legal: GateDetail,
    pub scientific: GateDetail,
}

impl Gates {
    pub fn pending() -> Self {
        Self {
            calendar: GateDetail::pending(
                &[
                    "broker/session definition (open/close, holidays, DST)",
                    "per-instrument coverage and timestamp-continuity report",
                ],
                "Pending: no broker-confirmed session calendar or coverage acceptance is recorded.",
            ),
            profile: GateDetail::pending(
                &[
                    "broker instrument specification (contract size, tick/pip, rollover)",
                    "instrument/profile review record",
                ],
                "Pending: no broker instrument specification or profile review is recorded.",
            ),
            legal: GateDetail::pending(
                &[
                    "provider license terms for source_id (e.g. HistData.com)",
                    "redistribution and research-use review record",
                ],
                "Pending: provider licensing and redistribution rights are not established.",
            ),
            scientific: GateDetail::pending(
                &["research protocol approval", "data-quality review record"],
                "Pending: no scientific protocol or data-quality approval is recorded.",
            ),
        }
    }
    pub fn accepted_all(&self) -> bool {
        [
            self.calendar.state,
            self.profile.state,
            self.legal.state,
            self.scientific.state,
        ]
        .iter()
        .all(|g| *g == Gate::Accepted)
    }
    pub fn any_rejected(&self) -> bool {
        [
            self.calendar.state,
            self.profile.state,
            self.legal.state,
            self.scientific.state,
        ]
        .contains(&Gate::Rejected)
    }
    pub fn evidence(&self) -> Vec<String> {
        let mut all = Vec::new();
        for gate in [&self.calendar, &self.profile, &self.legal, &self.scientific] {
            if gate.state == Gate::Accepted {
                all.extend(gate.evidence.iter().cloned());
            }
        }
        all
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
    pub structural_evidence: Vec<String>,
    pub review_state: Option<ReviewState>,
    pub eligible_for_research: bool,
}

impl InstrumentStatus {
    /// Eligibility requires structural validity, all four gates accepted, and no rejected
    /// review. A reviewer-supplied acceptance flag is a claim, not independent verification.
    fn compute_eligible(&self) -> bool {
        self.structural_valid
            && self.gates.accepted_all()
            && self.review_state != Some(ReviewState::Rejected)
    }
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
        .saturating_add(array_len(value, "duplicate_blocks_removed"))
        .saturating_add(array_len(value, "uncorrectable"))
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
    let Some(canonical) = canonical else {
        status.state = PartitionState::Noncanonical;
        return status;
    };
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
            // A foreign or self-consistent receipt must never be reported as a verified
            // canonical partition: bind it to the canonical plan identity.
            if receipt.source.dataset_hash != canonical.dataset_hash
                || receipt.source.source_sha256 != canonical.source_sha256
                || receipt.source.rows != canonical.rows
                || receipt.source.instrument != canonical.instrument
                || receipt.source.parquet_path != canonical.parquet_path
            {
                status.state = PartitionState::LoadFailure;
                status.load_error =
                    Some("monthly receipt does not match the canonical partition".into());
                return status;
            }
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

fn validate_plan_months(plan: &[Partition]) -> Result<()> {
    let mut previous: Option<(u32, u32)> = None;
    for partition in plan {
        if partition.year == 0 || partition.year > 4000 || !(1..=12).contains(&partition.month) {
            return Err("canonical plan contains an invalid calendar month".into());
        }
        if let Some((year, month)) = previous {
            let current = partition.year as u64 * 12 + partition.month as u64;
            let previous_index = year as u64 * 12 + month as u64;
            if current == previous_index {
                return Err("canonical plan contains a duplicate month".into());
            }
            if current != previous_index + 1 {
                return Err("canonical plan months are not contiguous".into());
            }
        }
        previous = Some((partition.year, partition.month));
    }
    Ok(())
}

/// Structural status for one instrument. Never asserts calendar or research eligibility.
pub fn scan_instrument(
    root: &Path,
    output_root: &Path,
    instrument: &str,
    expected: usize,
) -> Result<InstrumentStatus> {
    let plan = historical_corpus::plan_instrument(root, instrument)?;
    validate_plan_months(&plan)?;
    if plan.len() != expected {
        return Err(format!(
            "canonical plan has {} partitions for {instrument}, expected {expected}",
            plan.len()
        ));
    }
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
    let gates = Gates::pending();
    let structural_evidence = vec![format!(
        "{instrument}: {verified}/{expected} monthly partitions structurally verified"
    )];
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
        structural_evidence,
        review_state: None,
        eligible_for_research: false,
    })
}

fn catalog_identity(instruments: &[InstrumentStatus]) -> Result<String> {
    data::hash(&(
        VERSION,
        instruments
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
    ))
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
    let identity = catalog_identity(&statuses)?;
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
        if catalog_identity(&self.instruments)? != self.identity {
            return Err("historical catalog identity mismatch".into());
        }
        for instrument in &self.instruments {
            if instrument.months.len() != instrument.expected_months {
                return Err("historical catalog month count mismatch".into());
            }
            let mut previous: Option<(u32, u32)> = None;
            for month in &instrument.months {
                if month.instrument != instrument.instrument {
                    return Err("historical catalog month instrument mismatch".into());
                }
                if month.state == PartitionState::Verified && month.load_error.is_some() {
                    return Err("historical catalog verified month has a load error".into());
                }
                if let Some((year, previous_month)) = previous {
                    let current = month.year as u64 * 12 + month.month as u64;
                    let previous_index = year as u64 * 12 + previous_month as u64;
                    if current <= previous_index {
                        return Err("historical catalog months are not strictly increasing".into());
                    }
                }
                previous = Some((month.year, month.month));
            }
            let structural_valid = instrument.coverage == "complete"
                && instrument
                    .months
                    .iter()
                    .all(|m| m.state == PartitionState::Verified);
            if structural_valid != instrument.structural_valid {
                return Err("historical catalog structural flag mismatch".into());
            }
            if instrument.compute_eligible() != instrument.eligible_for_research {
                return Err("historical catalog eligibility mismatch".into());
            }
        }
        Ok(())
    }

    /// Record explicit gate decisions with retained evidence. Structural validity is still
    /// required for eligibility; a pending or rejected gate blocks it.
    pub fn bind_assertions(
        &mut self,
        dataset_identity: &str,
        receipt: &str,
        decisions: GateDecisions,
        evidence: Vec<String>,
    ) -> Result<()> {
        if dataset_identity != self.identity {
            return Err("assertion dataset identity does not match this catalog".into());
        }
        for instrument in &mut self.instruments {
            for (gate, state) in [
                (&mut instrument.gates.calendar, decisions.calendar),
                (&mut instrument.gates.profile, decisions.profile),
                (&mut instrument.gates.legal, decisions.legal),
                (&mut instrument.gates.scientific, decisions.scientific),
            ] {
                match state {
                    Gate::Accepted => gate.accept(dataset_identity, receipt, evidence.clone()),
                    Gate::Rejected => gate.reject("Rejected by explicit assertion."),
                    Gate::Pending => gate.set_pending(),
                }
            }
            instrument.eligible_for_research = instrument.compute_eligible();
        }
        Ok(())
    }

    /// Bind a receipt-bound review report to this catalog identity and update the gates it
    /// covers. Legal and scientific gates remain pending unless separately recorded.
    pub fn bind_review(&mut self, report: &data_review::Report) -> Result<()> {
        // Refuse forged or corrupted reports before trusting any decision: the record hash,
        // version and evidence hashes must verify.
        let verified = data_review::Report::verified(report.record.clone(), report.hash.clone())?;
        let report = &verified;
        if report.record.request.catalog_identity != self.identity {
            return Err("review catalog identity does not match this catalog".into());
        }
        if report.record.request.catalog_hash != data::hash(&self.instruments)? {
            return Err("review catalog hash does not match this catalog".into());
        }
        // Refuse to bind into a catalog whose own identity or flags are inconsistent.
        self.verify()?;
        let catalog_identity = self.identity.clone();
        let receipt = report.hash.clone();
        for instrument in &mut self.instruments {
            let mut changed = false;
            for (index, assessment) in report.record.request.assessments.iter().enumerate() {
                let evidence = format!(
                    "{:?} evidence {}",
                    assessment.area,
                    report
                        .record
                        .evidence_hashes
                        .get(index)
                        .map_or("", String::as_str)
                );
                let gate = match assessment.area {
                    Area::Calendar => Some(&mut instrument.gates.calendar),
                    Area::Profile => Some(&mut instrument.gates.profile),
                    _ => None,
                };
                if let Some(gate) = gate {
                    match assessment.decision {
                        Decision::AcceptedAssertion => {
                            gate.accept(&catalog_identity, &receipt, vec![evidence])
                        }
                        Decision::Rejected => gate.reject(&format!(
                            "{:?} rejected by review receipt {receipt}",
                            assessment.area
                        )),
                        Decision::Pending => gate.set_pending(),
                    }
                    changed = true;
                } else {
                    instrument.structural_evidence.push(evidence);
                }
            }
            if changed {
                instrument.review_state = Some(report.state);
                instrument.eligible_for_research = instrument.compute_eligible();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(state: Gate) -> GateDetail {
        let mut gate = GateDetail::pending(&[], "test");
        gate.state = state;
        gate
    }

    fn gates(calendar: Gate, profile: Gate, legal: Gate, scientific: Gate) -> Gates {
        Gates {
            calendar: gate(calendar),
            profile: gate(profile),
            legal: gate(legal),
            scientific: gate(scientific),
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
        let mut catalog = one_instrument_catalog();
        let identity = catalog.identity.clone();
        catalog
            .bind_assertions(
                &identity,
                "receipt-1",
                GateDecisions {
                    calendar: Gate::Accepted,
                    profile: Gate::Accepted,
                    legal: Gate::Accepted,
                    scientific: Gate::Accepted,
                },
                vec!["legal+scientific evidence".into()],
            )
            .unwrap();
        assert!(catalog.instruments[0].eligible_for_research);
        assert_eq!(
            catalog.instruments[0]
                .gates
                .legal
                .dataset_identity
                .as_deref(),
            Some(identity.as_str())
        );
        assert_eq!(
            catalog.instruments[0].gates.legal.receipt.as_deref(),
            Some("receipt-1")
        );
        assert_eq!(catalog.instruments[0].gates.legal.evidence.len(), 1);
        catalog
            .bind_assertions(
                &identity,
                "receipt-1",
                GateDecisions {
                    calendar: Gate::Accepted,
                    profile: Gate::Accepted,
                    legal: Gate::Pending,
                    scientific: Gate::Accepted,
                },
                vec![],
            )
            .unwrap();
        assert!(!catalog.instruments[0].eligible_for_research);
        // A mismatched dataset identity is refused.
        assert!(
            catalog
                .bind_assertions(
                    &"b".repeat(64),
                    "receipt-1",
                    GateDecisions {
                        calendar: Gate::Accepted,
                        profile: Gate::Accepted,
                        legal: Gate::Accepted,
                        scientific: Gate::Accepted,
                    },
                    vec!["evidence".into()],
                )
                .is_err()
        );
    }

    #[test]
    fn bind_review_updates_gates_and_keeps_legal_pending() {
        use crate::data_review::{self, Area, Assessment, Decision, Request};
        let mut catalog = one_instrument_catalog();
        let identity = catalog.identity.clone();
        let catalog_hash = crate::data::hash(&catalog.instruments).unwrap();
        let assessment = |area: Area| Assessment {
            area,
            decision: Decision::AcceptedAssertion,
            explanation: "accepted".into(),
            evidence_text: "evidence".into(),
        };
        let report = data_review::Report::create(Request {
            catalog_identity: identity.clone(),
            catalog_hash,
            reviewer: "reviewer".into(),
            assessments: [
                assessment(Area::Source),
                assessment(Area::Profile),
                assessment(Area::Calendar),
                assessment(Area::Quality),
                assessment(Area::Coverage),
            ],
        })
        .unwrap();
        catalog.bind_review(&report).unwrap();
        assert_eq!(catalog.instruments[0].gates.calendar.state, Gate::Accepted);
        assert_eq!(catalog.instruments[0].gates.profile.state, Gate::Accepted);
        assert_eq!(catalog.instruments[0].gates.legal.state, Gate::Pending);
        assert_eq!(catalog.instruments[0].gates.scientific.state, Gate::Pending);
        assert!(!catalog.instruments[0].eligible_for_research);
        assert_eq!(
            catalog.instruments[0].review_state,
            Some(data_review::State::AssertionsRecorded)
        );
        assert_eq!(
            catalog.instruments[0]
                .gates
                .calendar
                .dataset_identity
                .as_deref(),
            Some(identity.as_str())
        );
        assert_eq!(
            catalog.instruments[0].gates.calendar.receipt.as_deref(),
            Some(report.hash.as_str())
        );
        assert!(catalog.instruments[0].structural_evidence.len() >= 3);
    }

    fn one_instrument_catalog() -> Catalog {
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
                structural_evidence: vec!["structural".into()],
                review_state: None,
                eligible_for_research: false,
            }],
            identity: String::new(),
            limitations: LIMITATIONS.into(),
        };
        catalog.identity = catalog_identity(&catalog.instruments).unwrap();
        catalog
    }

    #[test]
    fn acceptance_requires_retained_evidence() {
        let mut gate = GateDetail::pending(&["e"], "pending");
        gate.accept("dataset-1", "receipt-1", vec![]);
        assert_eq!(gate.state, Gate::Pending);
        gate.accept("dataset-1", "receipt-1", vec!["".into(), "  ".into()]);
        assert_eq!(gate.state, Gate::Pending);
        gate.accept("dataset-1", "receipt-1", vec!["evidence".into()]);
        assert_eq!(gate.state, Gate::Accepted);
    }

    #[test]
    fn bind_review_refuses_forged_or_tampered_report() {
        use crate::data_review::{self, Area, Assessment, Decision, Request};
        let mut catalog = one_instrument_catalog();
        let identity = catalog.identity.clone();
        let catalog_hash = crate::data::hash(&catalog.instruments).unwrap();
        let assessment = |area: Area| Assessment {
            area,
            decision: Decision::AcceptedAssertion,
            explanation: "accepted".into(),
            evidence_text: "evidence".into(),
        };
        let make = || {
            data_review::Report::create(Request {
                catalog_identity: identity.clone(),
                catalog_hash: catalog_hash.clone(),
                reviewer: "reviewer".into(),
                assessments: [
                    assessment(Area::Source),
                    assessment(Area::Profile),
                    assessment(Area::Calendar),
                    assessment(Area::Quality),
                    assessment(Area::Coverage),
                ],
            })
            .unwrap()
        };
        let mut bad_hash = make();
        bad_hash.hash = "0".repeat(64);
        assert!(catalog.bind_review(&bad_hash).is_err());
        let mut bad_evidence = make();
        bad_evidence.record.evidence_hashes[0] = "0".repeat(64);
        assert!(catalog.bind_review(&bad_evidence).is_err());
        let mut wrong_identity = make();
        wrong_identity.record.request.catalog_identity = "1".repeat(64);
        wrong_identity.hash = crate::data::hash(&wrong_identity.record).unwrap();
        assert!(catalog.bind_review(&wrong_identity).is_err());
        let mut wrong_catalog_hash = make();
        wrong_catalog_hash.record.request.catalog_hash = "f".repeat(64);
        wrong_catalog_hash.hash = crate::data::hash(&wrong_catalog_hash.record).unwrap();
        assert!(catalog.bind_review(&wrong_catalog_hash).is_err());
    }

    #[test]
    fn rejected_review_blocks_eligibility() {
        use crate::data_review::{self, Area, Assessment, Decision, Request};
        let mut catalog = one_instrument_catalog();
        let identity = catalog.identity.clone();
        let catalog_hash = crate::data::hash(&catalog.instruments).unwrap();
        let accepted = |area: Area| Assessment {
            area,
            decision: Decision::AcceptedAssertion,
            explanation: "accepted".into(),
            evidence_text: "evidence".into(),
        };
        let rejected = |area: Area| Assessment {
            area,
            decision: Decision::Rejected,
            explanation: "rejected".into(),
            evidence_text: "evidence".into(),
        };
        let report = data_review::Report::create(Request {
            catalog_identity: identity.clone(),
            catalog_hash,
            reviewer: "reviewer".into(),
            assessments: [
                accepted(Area::Source),
                accepted(Area::Profile),
                accepted(Area::Calendar),
                rejected(Area::Quality),
                accepted(Area::Coverage),
            ],
        })
        .unwrap();
        assert_eq!(report.state, data_review::State::Rejected);
        catalog.bind_review(&report).unwrap();
        catalog
            .bind_assertions(
                &identity,
                "receipt",
                GateDecisions {
                    calendar: Gate::Accepted,
                    profile: Gate::Accepted,
                    legal: Gate::Accepted,
                    scientific: Gate::Accepted,
                },
                vec!["evidence".into()],
            )
            .unwrap();
        assert!(catalog.instruments[0].gates.accepted_all());
        assert_eq!(
            catalog.instruments[0].review_state,
            Some(data_review::State::Rejected)
        );
        assert!(!catalog.instruments[0].eligible_for_research);
        catalog.verify().unwrap();
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
