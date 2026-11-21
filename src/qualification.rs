//! Independent strategy qualification: immutable decisions, evidence/approval gates and
//! diverse per-instrument leaderboards (blueprint §8 and §20).
//!
//! Numerical evidence alone never qualifies a strategy. A QUALIFIED board position requires
//! every evidence gate to pass AND an authenticated independent qualification decision under
//! an operator-provisioned authority. Synthetic or diagnostic evidence can never qualify,
//! regardless of numerical results or supplied approvals. Decisions and board snapshots are
//! append-only; rejection and revocation history is retained.
use crate::{
    Result,
    contracts::{INITIAL_EQUITY, SYMBOLS},
    data::{self, DataClass},
    data_approval::Eligibility,
    data_review::Area,
    evidence_gates,
    machine_evidence::hex,
    review_authority::{Authority, Principal},
    storage::Registry,
    strategies::{Rule, Strategy},
    tick_engine::Costs,
    validation::{Interval, PeriodStats},
};
use chrono::{DateTime, Datelike, Months, NaiveDate};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const POLICY_VERSION: &str = "qualification-policy-v1";
pub const GATE_VERSION: &str = "qualification-evidence-gates-v1";
pub const DECISION_VERSION: &str = "independent-qualification-decision-v1";
pub const RANKING_VERSION: &str = "diverse-ranking-v1";
pub const DIVERSITY_VERSION: &str = "structural-diversity-v1";
pub const BOARD_VERSION: &str = "qualification-board-v1";
pub const STORE_SCHEMA_VERSION: u32 = 1;
/// The authoritative engine freezes the 1:3 planned reward/risk target; achieved net results
/// come from the full-period accounting evidence, never from the planned ratio.
pub const FULL_PERIOD_CLASSIFICATION: &str = "registered_full_period_replay";
const LIMIT: &str = "Independent local qualification decisions only. Operator-provisioned credentials and filesystem are the trust root; human scientific judgments are not mathematical proof. Board positions require complete approved evidence and an authenticated independent decision; synthetic/diagnostic data never qualifies. Database/file rollback by the trust-root operator is outside this boundary.";
const MAX_DECISIONS: i64 = 10_000;
const MAX_SCOPE_DECISIONS: usize = 1000;
const MAX_BODY: usize = 262_144;
const MAX_JOURNAL_BYTES: i64 = 128 * 1024 * 1024;
const BOARD_CAPACITY: usize = 5;

// === Frozen policy ===

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub minimum_net_win_rate_bps: u16,
    pub minimum_resolved_per_year: usize,
    pub maximum_consecutive_losses: usize,
    pub minimum_ending_equity_micro_usd: i128,
    pub required_coverage_start_ms: i64,
    pub required_coverage_end_exclusive_ms: i64,
    pub maximum_per_instrument: usize,
    pub maximum_per_diversity_class: usize,
}
impl Policy {
    /// Owner-selected research configuration (2026-11-19): 22 completed months
    /// 2025-01..2026-10, blueprint §8 floors, five positions per instrument.
    pub fn owner_selected() -> Result<Self> {
        let start = NaiveDate::from_ymd_opt(2025, 1, 1)
            .ok_or("invalid research window start")?
            .and_hms_opt(0, 0, 0)
            .ok_or("invalid research window start time")?
            .and_utc()
            .timestamp_millis();
        let end = NaiveDate::from_ymd_opt(2026, 11, 1)
            .ok_or("invalid research window end")?
            .and_hms_opt(0, 0, 0)
            .ok_or("invalid research window end time")?
            .and_utc()
            .timestamp_millis();
        Ok(Self {
            minimum_net_win_rate_bps: 5000,
            minimum_resolved_per_year: 50,
            maximum_consecutive_losses: 5,
            minimum_ending_equity_micro_usd: 500_000_000,
            required_coverage_start_ms: start,
            required_coverage_end_exclusive_ms: end,
            maximum_per_instrument: BOARD_CAPACITY,
            maximum_per_diversity_class: 2,
        })
    }
    /// Floors are blueprint/owner contract minima: a policy may be stricter, never weaker.
    pub fn validate(&self) -> Result<()> {
        let pin = Self::owner_selected()?;
        if self.minimum_net_win_rate_bps < 5000
            || self.minimum_net_win_rate_bps > 10000
            || self.minimum_resolved_per_year < 50
            || self.minimum_resolved_per_year > 1_000_000
            || self.maximum_consecutive_losses > 5
            || self.minimum_ending_equity_micro_usd < 500_000_000
            || self.required_coverage_start_ms != pin.required_coverage_start_ms
            || self.required_coverage_end_exclusive_ms != pin.required_coverage_end_exclusive_ms
            || self.maximum_per_instrument != BOARD_CAPACITY
            || !(1..=BOARD_CAPACITY).contains(&self.maximum_per_diversity_class)
        {
            return Err(
                "qualification policy cannot weaken the blueprint floors, the owner-selected 22-month window or the five-position board contract".into(),
            );
        }
        Ok(())
    }
    /// Exact calendar months the coverage evidence must enumerate, in order.
    pub fn required_months(&self) -> Result<Vec<String>> {
        self.validate()?;
        let mut months = Vec::new();
        let mut cursor = DateTime::from_timestamp_millis(self.required_coverage_start_ms)
            .ok_or("invalid coverage start")?;
        let end = DateTime::from_timestamp_millis(self.required_coverage_end_exclusive_ms)
            .ok_or("invalid coverage end")?;
        while cursor < end {
            months.push(format!("{:04}-{:02}", cursor.year(), cursor.month()));
            cursor = cursor
                .checked_add_months(Months::new(1))
                .ok_or("coverage calendar overflow")?;
            if months.len() > 24 {
                return Err("coverage window exceeds the owner-selected pin".into());
            }
        }
        if months.len() != 22 {
            return Err(
                "owner-selected research window must span exactly 22 completed months".into(),
            );
        }
        Ok(months)
    }
}

// === Evidence bundle ===

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionIdentity {
    pub engine: String,
    pub indicators: String,
    pub signals: String,
    pub compute_revision: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageEvidence {
    pub receipt_hash: String,
    pub catalog_identity: String,
    pub start_ms: i64,
    pub end_exclusive_ms: i64,
    pub months: Vec<String>,
    pub complete: bool,
    pub events: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountingEvidence {
    pub experiment_id: String,
    pub result_hash: String,
    /// Must be `registered_full_period_replay`; diagnostic simulation is refused.
    pub classification: String,
    pub cutoff_ms: i64,
    pub pooled: PeriodStats,
    pub year_2025: PeriodStats,
    pub year_2026: PeriodStats,
    pub net_micro_usd: i128,
    pub ending_micro_usd: i128,
    pub max_drawdown_micro_usd: i128,
    pub unresolved: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScienceEvidence {
    pub protocol_id: String,
    pub report_hash: String,
    pub report_data_class: DataClass,
    pub gate: evidence_gates::Assessment,
    pub robustness_cases_passed: usize,
    pub robustness_cases_total: usize,
    pub uncertainty_present: bool,
    pub block_uncertainty_present: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataApprovalEvidence {
    pub catalog_identity: String,
    pub machine_id: String,
    pub state: String,
    pub reasons: Vec<String>,
    pub review_head: Option<String>,
    pub live_machine_revalidated: bool,
    pub scope: Interval,
}
impl From<&Eligibility> for DataApprovalEvidence {
    fn from(e: &Eligibility) -> Self {
        Self {
            catalog_identity: e.catalog_identity.clone(),
            machine_id: e.machine_id.clone(),
            state: e.state.into(),
            reasons: e.reasons.clone(),
            review_head: e.review_head.clone(),
            live_machine_revalidated: e.live_machine_revalidated,
            scope: e.scope.clone(),
        }
    }
}
/// Complete qualification evidence presented for one strategy on one instrument.
/// Every identity is exact; the decision path revalidates bindings against live artifacts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBundle {
    pub instrument: String,
    pub concept_id: String,
    pub strategy: Strategy,
    pub declared_class: DataClass,
    pub profile_research_only: bool,
    pub dataset_identity: String,
    pub dataset_hash: String,
    pub source: String,
    pub submitter: String,
    pub profile_hash: String,
    pub costs: Costs,
    pub cost_hash: String,
    pub execution: ExecutionIdentity,
    pub data_approval: DataApprovalEvidence,
    pub coverage: CoverageEvidence,
    pub accounting: AccountingEvidence,
    pub science: ScienceEvidence,
}

// === Evidence gates ===

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum GateState {
    Passed,
    Held,
    Rejected,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GateDecision {
    pub gate: String,
    pub state: GateState,
    pub explanation: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Assessment {
    pub method: String,
    pub policy_hash: String,
    /// REJECTED | HELD | QUALIFICATION_EVIDENCE_COMPLETE
    pub state: String,
    /// Always false: only an authenticated independent decision can qualify.
    pub qualification_granted: bool,
    pub gates: Vec<GateDecision>,
    pub reasons: Vec<String>,
}
fn stats_consistent(s: &PeriodStats) -> bool {
    s.wins
        .checked_add(s.losses)
        .and_then(|n| n.checked_add(s.breakeven))
        == Some(s.resolved)
        && s.max_losing_streak <= s.losses
}
fn win_rate_bps(s: &PeriodStats) -> Option<u16> {
    if s.resolved == 0 || s.resolved > 1_000_000 {
        return None;
    }
    Some(((s.wins as u128 * 10000) / s.resolved as u128) as u16)
}
/// Pure fail-closed gate assessment. Recomputes every numerical threshold from exact
/// counters; missing evidence holds, contract violations reject. Never grants qualification.
pub fn assess(bundle: &EvidenceBundle, policy: &Policy) -> Result<Assessment> {
    policy.validate()?;
    let policy_hash = data::hash(policy)?;
    let mut gates: Vec<GateDecision> = Vec::new();
    let mut add = |gate: &str, state: GateState, explanation: &str| {
        gates.push(GateDecision {
            gate: gate.into(),
            state,
            explanation: explanation.into(),
        });
    };

    // Identity binding.
    let identity_ok = SYMBOLS.contains(&bundle.instrument.as_str())
        && hex(&bundle.concept_id)
        && bundle
            .strategy
            .identity()
            .is_ok_and(|id| id == bundle.concept_id)
        && hex(&bundle.dataset_identity)
        && hex(&bundle.dataset_hash)
        && hex(&bundle.profile_hash)
        && hex(&bundle.cost_hash)
        && !bundle.source.trim().is_empty()
        && bundle.source.len() <= 4096
        && !bundle.submitter.trim().is_empty()
        && bundle.submitter.len() <= 128;
    add(
        "identity-binding",
        if identity_ok {
            GateState::Passed
        } else {
            GateState::Rejected
        },
        "Instrument, canonical strategy identity and exact dataset/profile/cost hashes must bind one registered scope.",
    );

    // Data class: synthetic/quarantined can never qualify.
    let class_state = match bundle.declared_class {
        DataClass::SyntheticTestOnly => GateState::Rejected,
        DataClass::Quarantined => GateState::Rejected,
        _ => GateState::Passed,
    };
    add(
        "data-class",
        class_state,
        match class_state {
            GateState::Rejected => {
                "Synthetic or quarantined data never qualifies regardless of numerical evidence or supplied approvals."
            }
            _ => {
                "Non-synthetic declared class; scientific eligibility is determined by the independent approval chain, not by the declaration."
            }
        },
    );

    // Independent data approvals (five areas, live machine revalidation).
    let approvals_ok = bundle.data_approval.state == "DATA_APPROVED_FOR_SCOPE"
        && bundle.data_approval.reasons.is_empty()
        && bundle.data_approval.live_machine_revalidated
        && bundle
            .data_approval
            .review_head
            .as_ref()
            .is_some_and(|s| hex(s))
        && hex(&bundle.data_approval.catalog_identity)
        && hex(&bundle.data_approval.machine_id)
        && bundle.data_approval.scope.validate().is_ok();
    add(
        "independent-data-approvals",
        if approvals_ok {
            GateState::Passed
        } else {
            GateState::Held
        },
        "Source/profile/calendar/quality/coverage require current independent exact-artifact approvals with live machine revalidation; missing or revoked approvals hold.",
    );

    // Instrument profile.
    let profile_ok = !bundle.profile_research_only && hex(&bundle.profile_hash);
    add(
        "instrument-profile",
        if profile_ok {
            GateState::Passed
        } else {
            GateState::Rejected
        },
        "Research-only instrument profiles cannot be qualified; an approved non-research profile identity is required.",
    );

    // Cost profile: exact binding and blueprint friction floors.
    let costs = &bundle.costs;
    let costs_ok = costs.version == "bid-anchored-floor-v1"
        && costs.spread_floor_pips >= 2
        && costs.spread_floor_pips <= 10_000
        && costs.slippage_pips >= 2
        && costs.slippage_pips <= 10_000
        && costs.commission_micro_usd_per_lot_per_fill >= 0
        && costs.max_conversion_age_ms > 0
        && data::hash(costs).is_ok_and(|h| h == bundle.cost_hash);
    add(
        "cost-profile",
        if costs_ok {
            GateState::Passed
        } else {
            GateState::Rejected
        },
        "Costs must bind their exact hash and keep the minimum 2-pip spread floor and separate 2-pip adverse slippage; weaker friction is refused.",
    );

    // Execution version: evidence must come from the current frozen authoritative engine.
    let execution_ok = bundle.execution.engine == crate::ENGINE_VERSION
        && bundle.execution.indicators == crate::INDICATOR_VERSION
        && bundle.execution.signals == crate::SIGNAL_VERSION
        && hex(&bundle.execution.compute_revision);
    add(
        "execution-version",
        if execution_ok {
            GateState::Passed
        } else {
            GateState::Held
        },
        "Accounting and scientific evidence must bind the current frozen engine/indicator/signal versions and an exact compute revision; stale versions require re-execution.",
    );

    // Complete eligible historical coverage over the owner-selected window.
    let months_ok = policy
        .required_months()
        .is_ok_and(|m| m == bundle.coverage.months);
    let coverage_ok = months_ok
        && bundle.coverage.start_ms == policy.required_coverage_start_ms
        && bundle.coverage.end_exclusive_ms == policy.required_coverage_end_exclusive_ms
        && bundle.coverage.complete
        && bundle.coverage.events > 0
        && hex(&bundle.coverage.receipt_hash)
        && hex(&bundle.coverage.catalog_identity);
    add(
        "complete-coverage",
        if coverage_ok {
            GateState::Passed
        } else {
            GateState::Held
        },
        "Coverage must enumerate exactly the 22 owner-selected completed months with a verified nonempty receipt; partial or shifted windows hold.",
    );

    // Full-period historical accounting (never reset-fold sums, never diagnostic).
    let a = &bundle.accounting;
    let diagnostic = a.classification.contains("diagnostic");
    let counters_ok = stats_consistent(&a.pooled)
        && stats_consistent(&a.year_2025)
        && stats_consistent(&a.year_2026)
        && a.year_2025.resolved.checked_add(a.year_2026.resolved) == Some(a.pooled.resolved)
        && a.year_2025.wins.checked_add(a.year_2026.wins) == Some(a.pooled.wins)
        && a.year_2025.losses.checked_add(a.year_2026.losses) == Some(a.pooled.losses)
        && a.year_2025.breakeven.checked_add(a.year_2026.breakeven) == Some(a.pooled.breakeven)
        && a.pooled.max_losing_streak
            >= a.year_2025
                .max_losing_streak
                .max(a.year_2026.max_losing_streak)
        && INITIAL_EQUITY.checked_add(a.net_micro_usd) == Some(a.ending_micro_usd)
        && a.max_drawdown_micro_usd >= 0;
    let cutoff_ok = DateTime::from_timestamp_millis(a.cutoff_ms).is_some_and(|d| d.year() == 2026);
    let accounting_present =
        hex(&a.experiment_id) && hex(&a.result_hash) && cutoff_ok && counters_ok;
    let numerical_ok = accounting_present
        && a.classification == FULL_PERIOD_CLASSIFICATION
        && a.unresolved == 0
        && a.pooled.resolved > 0
        && win_rate_bps(&a.pooled).is_some_and(|bps| bps >= policy.minimum_net_win_rate_bps)
        && a.year_2025.resolved >= policy.minimum_resolved_per_year
        && a.year_2026.resolved >= policy.minimum_resolved_per_year
        && a.pooled.max_losing_streak <= policy.maximum_consecutive_losses
        && a.ending_micro_usd >= policy.minimum_ending_equity_micro_usd;
    let accounting_state = if diagnostic || !accounting_present {
        GateState::Rejected
    } else if a.classification != FULL_PERIOD_CLASSIFICATION {
        GateState::Held
    } else if numerical_ok {
        GateState::Passed
    } else {
        GateState::Rejected
    };
    add(
        "full-period-accounting",
        accounting_state,
        match accounting_state {
            GateState::Passed => {
                "Single-account full-period replay meets the exact win-rate, annual sample, losing-streak and equity floors with zero unresolved positions."
            }
            GateState::Held => {
                "Full-period registered accounting evidence is missing or incomplete; reset-fold sums and training backtests cannot substitute."
            }
            GateState::Rejected => {
                "Diagnostic simulation, inconsistent counters, unresolved positions or failed numerical floors can never qualify."
            }
        },
    );

    // Independent scientific validation (frozen protocol evidence gates).
    let s = &bundle.science;
    let gate = &s.gate;
    let science_ok = hex(&s.protocol_id)
        && hex(&s.report_hash)
        && s.report_data_class == bundle.declared_class
        && gate.method == evidence_gates::VERSION
        && gate.state == "NUMERICAL_EVIDENCE_ONLY"
        && gate.criteria_passed
        && !gate.qualification_granted
        && s.robustness_cases_total > 0
        && s.robustness_cases_passed <= s.robustness_cases_total
        && s.robustness_cases_passed == s.robustness_cases_total;
    add(
        "independent-science",
        if science_ok {
            GateState::Passed
        } else {
            GateState::Held
        },
        "A frozen protocol report must pass every prespecified OOS evidence criterion including the conditional final-horizon gate and all robustness probes; numerical passes remain evidence only.",
    );

    // Planned reward/risk: the frozen engine hard-codes the 1:3 target; achieved net is
    // assessed from accounting above. A valid stop distance is required.
    let rr_ok = bundle.strategy.stop_ticks > 0 && execution_ok;
    add(
        "planned-reward-risk",
        if rr_ok {
            GateState::Passed
        } else {
            GateState::Held
        },
        "The authoritative engine freezes the 1:3 planned reward/risk; achieved net results are verified from full-period accounting, never from the planned ratio.",
    );

    let rejected = gates.iter().any(|g| g.state == GateState::Rejected);
    let held = gates.iter().any(|g| g.state == GateState::Held);
    let state = if rejected {
        "REJECTED"
    } else if held {
        "HELD"
    } else {
        "QUALIFICATION_EVIDENCE_COMPLETE"
    };
    let reasons = gates
        .iter()
        .filter(|g| g.state != GateState::Passed)
        .map(|g| format!("{}: {}", g.gate, g.explanation))
        .collect();
    Ok(Assessment {
        method: GATE_VERSION.into(),
        policy_hash,
        state: state.into(),
        qualification_granted: false,
        gates,
        reasons,
    })
}

// === Diversity and ranking ===

fn leaf_primitives(rule: &Rule, out: &mut Vec<&'static str>) {
    match rule {
        Rule::AboveMean { .. } => out.push("AboveMean"),
        Rule::BelowMean { .. } => out.push("BelowMean"),
        Rule::Rising { .. } => out.push("Rising"),
        Rule::Falling { .. } => out.push("Falling"),
        Rule::MeanTrend { .. } => out.push("MeanTrend"),
        Rule::CrossMean { .. } => out.push("CrossMean"),
        Rule::RsiThreshold { .. } => out.push("RsiThreshold"),
        Rule::RsiCross { .. } => out.push("RsiCross"),
        Rule::Donchian { .. } => out.push("Donchian"),
        Rule::ChannelEdge { .. } => out.push("ChannelEdge"),
        Rule::RangeExpansion { .. } => out.push("RangeExpansion"),
        Rule::UtcSession { .. } => out.push("UtcSession"),
        Rule::MacdCross { .. } => out.push("MacdCross"),
        Rule::BollingerCross { .. } => out.push("BollingerCross"),
        Rule::All(children) | Rule::Any(children) => {
            for child in children {
                leaf_primitives(child, out);
            }
        }
        Rule::HigherTimeframe { rule, .. } => leaf_primitives(rule, out),
    }
}
/// Structural diversity class: parameter tweaks of one rule shape share a class, so a
/// board cannot fill with near-identical candidates. Versioned heuristic, not a
/// broker/scientific fact; labels bind timeframe, side and the sorted leaf primitive set.
pub fn diversity_class(strategy: &Strategy) -> String {
    let mut leaves = Vec::new();
    leaf_primitives(&strategy.rule, &mut leaves);
    leaves.sort_unstable();
    leaves.dedup();
    format!(
        "{DIVERSITY_VERSION}|{:?}|{:?}|{}",
        strategy.timeframe,
        strategy.side,
        leaves.join("+")
    )
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Score {
    pub method: String,
    pub total: i128,
    pub components: Vec<(String, i128)>,
}
fn clamp_i128(v: i128, lo: i128, hi: i128) -> i128 {
    v.max(lo).min(hi)
}
/// Versioned exact-integer ranking over verified evidence only. Training statistics are
/// structurally absent from the bundle; the methodology never reads them.
pub fn score(bundle: &EvidenceBundle) -> Result<Score> {
    let a = &bundle.accounting;
    if !stats_consistent(&a.pooled) || a.pooled.resolved == 0 {
        return Err("ranking requires consistent nonempty full-period accounting".into());
    }
    let resolved = a.pooled.resolved as i128;
    let win_bps = (a.pooled.wins as i128 * 10000) / resolved;
    let expectancy = a.net_micro_usd / resolved;
    let mut components = Vec::new();
    // Verified net win rate (0..30).
    components.push(("verified_net_win_rate".to_string(), win_bps * 30 / 10000));
    // Complete resolved sample, capped (0..10).
    components.push((
        "complete_trade_count".to_string(),
        clamp_i128(resolved, 0, 10000) * 10 / 10000,
    ));
    // Net expectancy per trade, clamped to ±$100 (0..15).
    components.push((
        "net_expectancy".to_string(),
        (clamp_i128(expectancy, -100_000_000, 100_000_000) + 100_000_000) * 15 / 200_000_000,
    ));
    // Simulated return on the $20 initial equity, capped at 50x (0..10).
    let return_multiple = a
        .ending_micro_usd
        .checked_mul(50)
        .ok_or("return overflow")?
        / INITIAL_EQUITY;
    components.push((
        "simulated_return".to_string(),
        clamp_i128(return_multiple, 0, 50) * 10 / 50,
    ));
    // Maximum drawdown relative to ending equity (0..10, smaller drawdown scores higher).
    let dd_penalty = if a.ending_micro_usd > 0 {
        clamp_i128(
            a.max_drawdown_micro_usd
                .checked_mul(10)
                .ok_or("drawdown overflow")?
                / a.ending_micro_usd,
            0,
            10,
        )
    } else {
        10
    };
    components.push(("max_drawdown".to_string(), 10 - dd_penalty));
    // Losing streak (0..5).
    components.push((
        "loss_streak".to_string(),
        (5 - clamp_i128(a.pooled.max_losing_streak as i128, 0, 5)) * 5 / 5,
    ));
    // Year-by-year stability: the weaker research year bounds the score (0..10).
    let y25 = win_rate_bps(&a.year_2025).map(i128::from).unwrap_or(0);
    let y26 = win_rate_bps(&a.year_2026).map(i128::from).unwrap_or(0);
    components.push(("year_stability".to_string(), y25.min(y26) * 10 / 10000));
    // Parameter/regime/cost robustness: frozen probe pass ratio (0..5).
    let s = &bundle.science;
    let probe_score = if s.robustness_cases_total > 0 {
        (s.robustness_cases_passed as i128 * 5) / s.robustness_cases_total as i128
    } else {
        0
    };
    components.push(("robustness_probes".to_string(), probe_score));
    // Evidence completeness beyond the mandatory gates (0..5).
    let optional = i128::from(s.uncertainty_present) + i128::from(s.block_uncertainty_present);
    components.push(("evidence_completeness".to_string(), optional * 5 / 2));
    let total = components.iter().try_fold(0_i128, |n, (_, v)| {
        n.checked_add(*v).ok_or("score overflow")
    })?;
    Ok(Score {
        method: RANKING_VERSION.into(),
        total,
        components,
    })
}

// === Immutable decision journal ===

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DecisionKind {
    Qualify,
    Reject,
    Revoke,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub decision: DecisionKind,
    pub expected_head: Option<String>,
    pub submitter: String,
    pub explanation: String,
    pub evidence_text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub version: String,
    pub sequence: i64,
    pub instrument: String,
    pub concept_id: String,
    pub policy: Policy,
    pub policy_hash: String,
    pub bundle_hash: String,
    pub assessment_hash: String,
    pub authority_id: String,
    pub reviewer: String,
    pub grant_hash: String,
    pub recorded_ms: i64,
    pub request: Request,
    pub bundle: EvidenceBundle,
    pub assessment: Assessment,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub record: Record,
    pub hash: String,
    pub authenticator: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardEntry {
    pub rank: usize,
    pub concept_id: String,
    pub decision_hash: String,
    pub diversity_class: String,
    pub timeframe: String,
    pub score: Score,
    pub win_rate_bps: u16,
    pub resolved: usize,
    pub net_micro_usd: i128,
    pub ending_micro_usd: i128,
    pub qualified_ms: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcludedCandidate {
    pub concept_id: String,
    pub decision_hash: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardSnapshot {
    pub version: String,
    pub instrument: String,
    pub sequence: i64,
    pub decision_hash: String,
    pub previous: Option<String>,
    pub policy_hash: String,
    pub ranking_method: String,
    pub published_ms: i64,
    pub entries: Vec<BoardEntry>,
    pub excluded: Vec<ExcludedCandidate>,
    pub empty_reasons: Vec<String>,
    pub limitations: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardView {
    pub instrument: String,
    pub ranking_method: String,
    pub policy_hash: Option<String>,
    pub entries: Vec<BoardEntry>,
    pub excluded: Vec<ExcludedCandidate>,
    pub empty_reasons: Vec<String>,
    pub head: Option<String>,
    pub limitations: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeState {
    pub instrument: String,
    pub concept_id: String,
    pub head: Option<String>,
    pub events: Vec<Event>,
    /// QUALIFIED | REJECTED | REVOKED | UNDECIDED
    pub current_state: String,
    pub current_authority_authenticated: bool,
    pub limitations: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct HistoryFilter {
    pub instrument: Option<String>,
    pub concept_id: Option<String>,
    pub decision: Option<DecisionKind>,
    pub diversity_class: Option<String>,
    pub timeframe: Option<String>,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub after_sequence: Option<i64>,
    pub limit: Option<usize>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryPage {
    pub events: Vec<Event>,
    pub next_sequence: Option<i64>,
    pub limitations: String,
}
/// Live artifacts required to revalidate a Qualify decision before publication.
pub struct LiveEvidence<'a> {
    pub registry: &'a Registry,
    pub catalog: &'a crate::catalog::Catalog,
    pub authority: &'a Authority,
    pub archive: &'a Path,
    pub dataset: &'a data::Dataset,
}

pub struct Store {
    connection: Connection,
}
fn db_err(e: rusqlite::Error) -> String {
    e.to_string()
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path).map_err(db_err)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(30))
            .map_err(db_err)?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
            )
            .map_err(db_err)?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(db_err)?;
        if version > i64::from(STORE_SCHEMA_VERSION) {
            return Err("qualification store schema newer than supported".into());
        }
        if version == 0 {
            connection
                .execute_batch(
                    "CREATE TABLE qualification_decisions(
                        sequence INTEGER PRIMARY KEY,
                        id TEXT NOT NULL UNIQUE CHECK(length(id)=64),
                        instrument TEXT NOT NULL,
                        concept_id TEXT NOT NULL CHECK(length(concept_id)=64),
                        body BLOB NOT NULL CHECK(typeof(body)='blob' AND length(body)<=262144),
                        authenticator TEXT NOT NULL CHECK(length(authenticator)=64));
                    CREATE TRIGGER qualification_decisions_no_update BEFORE UPDATE ON qualification_decisions BEGIN SELECT RAISE(ABORT,'qualification decisions immutable');END;
                    CREATE TRIGGER qualification_decisions_no_delete BEFORE DELETE ON qualification_decisions BEGIN SELECT RAISE(ABORT,'qualification decisions immutable');END;
                    CREATE INDEX qualification_scope ON qualification_decisions(instrument,concept_id,sequence);
                    CREATE TABLE qualification_boards(
                        sequence INTEGER PRIMARY KEY,
                        id TEXT NOT NULL UNIQUE CHECK(length(id)=64),
                        instrument TEXT NOT NULL,
                        body BLOB NOT NULL CHECK(typeof(body)='blob' AND length(body)<=262144));
                    CREATE TRIGGER qualification_boards_no_update BEFORE UPDATE ON qualification_boards BEGIN SELECT RAISE(ABORT,'qualification boards immutable');END;
                    CREATE TRIGGER qualification_boards_no_delete BEFORE DELETE ON qualification_boards BEGIN SELECT RAISE(ABORT,'qualification boards immutable');END;
                    CREATE INDEX qualification_boards_instrument ON qualification_boards(instrument,sequence);
                    PRAGMA user_version=1;",
                )
                .map_err(db_err)?;
        }
        Ok(Self { connection })
    }
    pub fn existing(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err("qualification store does not exist".into());
        }
        Self::open(path)
    }
    pub fn integrity(&self) -> Result<()> {
        let verdict: String = self
            .connection
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(db_err)?;
        if verdict != "ok" {
            return Err("qualification store integrity check failed".into());
        }
        Ok(())
    }
    /// Verified no-overwrite snapshot for offline backup/restore flows.
    pub fn backup(&self, destination: &Path) -> Result<()> {
        if destination.exists() {
            return Err("qualification backup destination must be new".into());
        }
        self.integrity()?;
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).map_err(|_| "OS randomness unavailable")?;
        let nonce = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let temp = parent.join(format!(".qualification-backup-{nonce}.sqlite"));
        {
            let mut dest = Connection::open(&temp).map_err(db_err)?;
            let backup =
                rusqlite::backup::Backup::new(&self.connection, &mut dest).map_err(db_err)?;
            backup
                .run_to_completion(100, std::time::Duration::from_millis(0), None)
                .map_err(db_err)?;
        }
        let verified = Connection::open(&temp).map_err(db_err)?;
        let verdict: String = verified
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(db_err)?;
        drop(verified);
        if verdict != "ok" {
            let _ = std::fs::remove_file(&temp);
            return Err("qualification backup integrity check failed".into());
        }
        // No-replace publication: hard link refuses an existing destination.
        std::fs::hard_link(&temp, destination).map_err(|e| e.to_string())?;
        std::fs::remove_file(&temp).map_err(|e| e.to_string())?;
        Ok(())
    }
    fn scope_history(&self, instrument: &str, concept_id: &str) -> Result<Vec<Event>> {
        if !SYMBOLS.contains(&instrument) || !hex(concept_id) {
            return Err("invalid qualification scope".into());
        }
        let mut q = self
            .connection
            .prepare(
                "SELECT sequence,id,instrument,concept_id,body,authenticator FROM qualification_decisions WHERE instrument=?1 AND concept_id=?2 ORDER BY sequence LIMIT 1001",
            )
            .map_err(db_err)?;
        let rows = q
            .query_map(params![instrument, concept_id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .map_err(db_err)?;
        let mut events = Vec::new();
        let mut previous = None;
        let mut last_ms: Option<i64> = None;
        for row in rows {
            let (sequence, id, row_instrument, row_concept, body, tag): (
                i64,
                String,
                String,
                String,
                Vec<u8>,
                String,
            ) = row.map_err(db_err)?;
            let event = decode(sequence, id, body, tag)?;
            if event.record.instrument != row_instrument
                || event.record.concept_id != row_concept
                || event.record.instrument != instrument
                || event.record.concept_id != concept_id
                || event.record.request.expected_head != previous
                || last_ms.is_some_and(|at| event.record.recorded_ms < at)
            {
                return Err("qualification history chain mismatch".into());
            }
            previous = Some(event.hash.clone());
            last_ms = Some(event.record.recorded_ms);
            events.push(event);
            if events.len() > MAX_SCOPE_DECISIONS {
                return Err("qualification scope exceeds 1000 decisions".into());
            }
        }
        Ok(events)
    }
    fn all_events(&self) -> Result<Vec<Event>> {
        let mut q = self
            .connection
            .prepare("SELECT sequence,id,instrument,concept_id,body,authenticator FROM qualification_decisions ORDER BY sequence LIMIT 10001")
            .map_err(db_err)?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .map_err(db_err)?;
        let mut events = Vec::new();
        for row in rows {
            let (sequence, id, instrument, concept, body, tag): (
                i64,
                String,
                String,
                String,
                Vec<u8>,
                String,
            ) = row.map_err(db_err)?;
            let event = decode(sequence, id, body, tag)?;
            if event.record.instrument != instrument || event.record.concept_id != concept {
                return Err("qualification decision scope binding mismatch".into());
            }
            events.push(event);
            if events.len() as i64 > MAX_DECISIONS {
                return Err("qualification journal quota exceeded".into());
            }
        }
        Ok(events)
    }
    fn latest_snapshot(&self, instrument: &str) -> Result<Option<BoardSnapshot>> {
        let row: Option<(Vec<u8>,)> = self
            .connection
            .query_row(
                "SELECT body FROM qualification_boards WHERE instrument=?1 ORDER BY sequence DESC LIMIT 1",
                [instrument],
                |r| Ok((r.get(0)?,)),
            )
            .optional()
            .map_err(db_err)?;
        match row {
            Some((body,)) => {
                let snapshot: BoardSnapshot =
                    serde_json::from_slice(&body).map_err(|e| e.to_string())?;
                if snapshot.instrument != instrument || snapshot.version != BOARD_VERSION {
                    return Err("board snapshot integrity mismatch".into());
                }
                Ok(Some(snapshot))
            }
            None => Ok(None),
        }
    }
    /// Append an immutable decision. Qualify requires complete gates plus live artifact
    /// revalidation; Reject/Revoke only require the independent grant and head fencing.
    pub fn decide(
        &mut self,
        authority: &Authority,
        principal: &Principal,
        request: Request,
        bundle: &EvidenceBundle,
        policy: &Policy,
        live: Option<&LiveEvidence<'_>>,
    ) -> Result<Event> {
        validate_request(&request)?;
        policy.validate()?;
        if request.submitter != bundle.submitter {
            return Err("decision submitter must match the evidence bundle submitter".into());
        }
        let assessment = assess(bundle, policy)?;
        let now = chrono::Utc::now().timestamp_millis();
        let grant = authority.grant(
            principal,
            &bundle.instrument,
            &bundle.source,
            Area::ScientificQualification,
            now,
        )?;
        if grant.submitter != request.submitter {
            return Err(
                "reviewer grant does not cover this submitter's qualification scope".into(),
            );
        }
        if request.decision == DecisionKind::Qualify {
            if assessment.state != "QUALIFICATION_EVIDENCE_COMPLETE" {
                return Err(format!(
                    "qualification refused: evidence gates {}",
                    assessment.state
                ));
            }
            let live = live.ok_or("qualification requires live registry/catalog revalidation")?;
            revalidate_live(bundle, policy, live)?;
        }
        if request.decision == DecisionKind::Revoke {
            let events = self.scope_history(&bundle.instrument, &bundle.concept_id)?;
            let latest = events
                .last()
                .ok_or("nothing to revoke: scope has no decisions")?;
            if latest.record.request.decision != DecisionKind::Qualify {
                return Err("nothing to revoke: latest decision is not a qualification".into());
            }
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let events = scope_history_tx(&tx, &bundle.instrument, &bundle.concept_id)?;
        if events.len() >= MAX_SCOPE_DECISIONS {
            return Err("qualification scope decision quota exceeded".into());
        }
        if events.last().map(|e| e.hash.clone()) != request.expected_head {
            return Err(
                "qualification head changed; inspect latest evidence before deciding".into(),
            );
        }
        if events.last().is_some_and(|e| now < e.record.recorded_ms) {
            return Err("qualification clock moved backwards".into());
        }
        let (n, bytes, last): (i64, i64, i64) = tx
            .query_row(
                "SELECT COUNT(*),COALESCE(SUM(length(body)),0),COALESCE(MAX(sequence),0) FROM qualification_decisions",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(db_err)?;
        let record = Record {
            version: DECISION_VERSION.into(),
            sequence: last.checked_add(1).ok_or("decision sequence overflow")?,
            instrument: bundle.instrument.clone(),
            concept_id: bundle.concept_id.clone(),
            policy: policy.clone(),
            policy_hash: data::hash(policy)?,
            bundle_hash: data::hash(bundle)?,
            assessment_hash: data::hash(&assessment)?,
            authority_id: authority.identity().into(),
            reviewer: principal.name().into(),
            grant_hash: data::hash(grant)?,
            recorded_ms: now,
            request,
            bundle: bundle.clone(),
            assessment,
        };
        let body = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
        if n >= MAX_DECISIONS
            || body.len() > MAX_BODY
            || bytes + body.len() as i64 > MAX_JOURNAL_BYTES
        {
            return Err("qualification decision journal quota exceeded".into());
        }
        let event = Event {
            hash: data::hash(&record)?,
            authenticator: authority.sign(&record)?,
            record,
        };
        tx.execute(
            "INSERT INTO qualification_decisions VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                event.record.sequence,
                event.hash,
                event.record.instrument,
                event.record.concept_id,
                body,
                event.authenticator
            ],
        )
        .map_err(db_err)?;
        // Publish the immutable board snapshot for this instrument atomically.
        let snapshot = build_snapshot(&tx, &event, authority, now)?;
        let snapshot_body = serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?;
        if snapshot_body.len() > MAX_BODY {
            return Err("board snapshot exceeds body budget".into());
        }
        let (sn, sbytes, slast): (i64, i64, i64) = tx
            .query_row(
                "SELECT COUNT(*),COALESCE(SUM(length(body)),0),COALESCE(MAX(sequence),0) FROM qualification_boards",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(db_err)?;
        if sn >= MAX_DECISIONS || sbytes + snapshot_body.len() as i64 > MAX_JOURNAL_BYTES {
            return Err("board snapshot journal quota exceeded".into());
        }
        let snapshot_hash = data::hash(&snapshot)?;
        tx.execute(
            "INSERT INTO qualification_boards VALUES(?1,?2,?3,?4)",
            params![
                slast.checked_add(1).ok_or("snapshot sequence overflow")?,
                snapshot_hash,
                snapshot.instrument,
                snapshot_body
            ],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(event)
    }
    pub fn scope_state(
        &self,
        instrument: &str,
        concept_id: &str,
        authority: &Authority,
    ) -> Result<ScopeState> {
        let events = self.scope_history(instrument, concept_id)?;
        let now = chrono::Utc::now().timestamp_millis();
        let authenticated = match events.last() {
            Some(e) => decision_authentic(authority, e, now)?,
            None => true,
        };
        let current_state = match events.last() {
            Some(e) if e.record.request.decision == DecisionKind::Qualify && authenticated => {
                "QUALIFIED"
            }
            Some(e) if e.record.request.decision == DecisionKind::Qualify => "REVOKED",
            Some(e) if e.record.request.decision == DecisionKind::Revoke => "REVOKED",
            Some(_) => "REJECTED",
            None => "UNDECIDED",
        };
        Ok(ScopeState {
            instrument: instrument.into(),
            concept_id: concept_id.into(),
            head: events.last().map(|e| e.hash.clone()),
            events,
            current_state: current_state.into(),
            current_authority_authenticated: authenticated,
            limitations: LIMIT.into(),
        })
    }
    /// Current board derived from the authenticated journal (authoritative live view).
    pub fn board(&self, instrument: &str, authority: &Authority) -> Result<BoardView> {
        if !SYMBOLS.contains(&instrument) {
            return Err("unknown instrument".into());
        }
        let events = self.all_events()?;
        let now = chrono::Utc::now().timestamp_millis();
        derive_board(&events, instrument, authority, now)
    }
    pub fn boards(&self, authority: &Authority) -> Result<Vec<BoardView>> {
        let events = self.all_events()?;
        let now = chrono::Utc::now().timestamp_millis();
        SYMBOLS
            .iter()
            .map(|instrument| derive_board(&events, instrument, authority, now))
            .collect()
    }
    pub fn latest_board_snapshot(&self, instrument: &str) -> Result<Option<BoardSnapshot>> {
        if !SYMBOLS.contains(&instrument) {
            return Err("unknown instrument".into());
        }
        self.latest_snapshot(instrument)
    }
    /// Immutable leader history: retained board snapshots, keyset paginated.
    pub fn leaders(
        &self,
        instrument: &str,
        after_sequence: Option<i64>,
        limit: usize,
    ) -> Result<(Vec<BoardSnapshot>, Option<i64>)> {
        if !SYMBOLS.contains(&instrument) || !(1..=200).contains(&limit) {
            return Err("invalid leader history scope/limit".into());
        }
        let mut q = self
            .connection
            .prepare(
                "SELECT sequence,id,body FROM qualification_boards WHERE instrument=?1 AND (?2 IS NULL OR sequence>?2) ORDER BY sequence LIMIT ?3",
            )
            .map_err(db_err)?;
        let rows = q
            .query_map(params![instrument, after_sequence, limit as i64 + 1], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(db_err)?;
        let mut snapshots = Vec::new();
        for row in rows {
            let (sequence, id, body) = row.map_err(db_err)?;
            let snapshot: BoardSnapshot =
                serde_json::from_slice(&body).map_err(|e| e.to_string())?;
            if snapshot.sequence != sequence
                || snapshot.instrument != instrument
                || data::hash(&snapshot)? != id
            {
                return Err("leader history integrity mismatch".into());
            }
            snapshots.push(snapshot);
        }
        let next = if snapshots.len() > limit {
            snapshots.pop();
            snapshots.last().map(|s| s.sequence)
        } else {
            None
        };
        Ok((snapshots, next))
    }
    /// Archive of decisions (including rejected candidates) with exact filters.
    pub fn history(&self, filter: &HistoryFilter) -> Result<HistoryPage> {
        let limit = filter.limit.unwrap_or(50);
        if !(1..=200).contains(&limit) {
            return Err("history limit must be 1..200".into());
        }
        if let Some(instrument) = &filter.instrument
            && !SYMBOLS.contains(&instrument.as_str())
        {
            return Err("unknown instrument filter".into());
        }
        if let Some(concept) = &filter.concept_id
            && !hex(concept)
        {
            return Err("invalid concept filter".into());
        }
        let mut q = self
            .connection
            .prepare(
                "SELECT sequence,id,instrument,concept_id,body,authenticator FROM qualification_decisions WHERE (?1 IS NULL OR instrument=?1) AND (?2 IS NULL OR concept_id=?2) AND (?3 IS NULL OR sequence>?3) ORDER BY sequence LIMIT 10001",
            )
            .map_err(db_err)?;
        let rows = q
            .query_map(
                params![filter.instrument, filter.concept_id, filter.after_sequence],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .map_err(db_err)?;
        let mut events = Vec::new();
        let mut scanned = 0_i64;
        let mut next_sequence = None;
        for row in rows {
            let (sequence, id, instrument, concept, body, tag): (
                i64,
                String,
                String,
                String,
                Vec<u8>,
                String,
            ) = row.map_err(db_err)?;
            scanned += 1;
            if scanned > MAX_DECISIONS {
                return Err("history scan exceeds journal quota".into());
            }
            let event = decode(sequence, id, body, tag)?;
            if event.record.instrument != instrument || event.record.concept_id != concept {
                return Err("qualification decision scope binding mismatch".into());
            }
            if let Some(kind) = filter.decision
                && event.record.request.decision != kind
            {
                continue;
            }
            if let Some(from) = filter.from_ms
                && event.record.recorded_ms < from
            {
                continue;
            }
            if let Some(to) = filter.to_ms
                && event.record.recorded_ms > to
            {
                continue;
            }
            if let Some(class) = &filter.diversity_class
                && diversity_class(&event.record.bundle.strategy) != *class
            {
                continue;
            }
            if let Some(tf) = &filter.timeframe
                && format!("{:?}", event.record.bundle.strategy.timeframe) != *tf
            {
                continue;
            }
            if events.len() == limit {
                next_sequence = Some(event.record.sequence);
                break;
            }
            events.push(event);
        }
        Ok(HistoryPage {
            events,
            next_sequence,
            limitations: LIMIT.into(),
        })
    }
}

fn validate_request(r: &Request) -> Result<()> {
    if r.expected_head.as_ref().is_some_and(|h| !hex(h))
        || r.submitter.trim().is_empty()
        || r.submitter.len() > 128
        || r.explanation.trim().is_empty()
        || r.explanation.len() > 4096
        || r.evidence_text.trim().is_empty()
        || r.evidence_text.len() > 65536
    {
        return Err("qualification decision requires exact head context, submitter and bounded retained evidence".into());
    }
    Ok(())
}
fn decode(sequence: i64, id: String, body: Vec<u8>, authenticator: String) -> Result<Event> {
    let record: Record = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    validate_request(&record.request)?;
    if record.version != DECISION_VERSION
        || record.sequence != sequence
        || sequence <= 0
        || !hex(&id)
        || !hex(&authenticator)
        || !hex(&record.authority_id)
        || !hex(&record.grant_hash)
        || !hex(&record.policy_hash)
        || !hex(&record.bundle_hash)
        || !hex(&record.assessment_hash)
        || record.reviewer.trim().is_empty()
        || record.reviewer.len() > 128
        || chrono::DateTime::from_timestamp_millis(record.recorded_ms).is_none()
        || data::hash(&record)? != id
        || data::hash(&record.bundle)? != record.bundle_hash
        || data::hash(&record.assessment)? != record.assessment_hash
    {
        return Err("qualification decision integrity mismatch".into());
    }
    Ok(Event {
        record,
        hash: id,
        authenticator,
    })
}
fn scope_history_tx(tx: &Connection, instrument: &str, concept_id: &str) -> Result<Vec<Event>> {
    let mut q = tx
        .prepare(
            "SELECT sequence,id,body,authenticator FROM qualification_decisions WHERE instrument=?1 AND concept_id=?2 ORDER BY sequence LIMIT 1001",
        )
        .map_err(db_err)?;
    let rows = q
        .query_map(params![instrument, concept_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .map_err(db_err)?;
    let mut events = Vec::new();
    let mut previous = None;
    let mut last_ms: Option<i64> = None;
    for row in rows {
        let (sequence, id, body, tag): (i64, String, Vec<u8>, String) = row.map_err(db_err)?;
        let event = decode(sequence, id, body, tag)?;
        if event.record.request.expected_head != previous
            || last_ms.is_some_and(|at| event.record.recorded_ms < at)
        {
            return Err("qualification history chain mismatch".into());
        }
        previous = Some(event.hash.clone());
        last_ms = Some(event.record.recorded_ms);
        events.push(event);
        if events.len() > MAX_SCOPE_DECISIONS {
            return Err("qualification scope exceeds 1000 decisions".into());
        }
    }
    Ok(events)
}
fn decision_authentic(authority: &Authority, e: &Event, now: i64) -> Result<bool> {
    if now < e.record.recorded_ms
        || e.record.authority_id != authority.identity()
        || !authority.authentic(&e.record, &e.authenticator)?
    {
        return Ok(false);
    }
    let past = authority.permission(
        &e.record.reviewer,
        &e.record.instrument,
        &e.record.bundle.source,
        Area::ScientificQualification,
        e.record.recorded_ms,
    );
    let current = authority.permission(
        &e.record.reviewer,
        &e.record.instrument,
        &e.record.bundle.source,
        Area::ScientificQualification,
        now,
    );
    Ok(
        past.is_ok_and(|g| data::hash(g).is_ok_and(|h| h == e.record.grant_hash))
            && current.is_ok(),
    )
}
/// Live revalidation of every artifact a Qualify decision binds. Supplied hashes are never
/// trusted: the registry experiment, frozen protocol report and catalog eligibility are
/// re-read and recomputed at decision time.
fn revalidate_live(
    bundle: &EvidenceBundle,
    policy: &Policy,
    live: &LiveEvidence<'_>,
) -> Result<()> {
    // Full-period accounting experiment: exact registered identity and completed receipt.
    let key = live
        .registry
        .experiment_key(&bundle.accounting.experiment_id)?
        .ok_or("accounting experiment missing from registry")?;
    if key.instrument != bundle.instrument
        || key.concept_hash != bundle.concept_id
        || key.dataset_hash != bundle.dataset_hash
        || key.cost_hash != bundle.cost_hash
        || key.profile_hash != bundle.profile_hash
        || key.engine_version != bundle.execution.engine
        || key.protocol_hash != bundle.science.protocol_id
        || key.code_revision != bundle.execution.compute_revision
    {
        return Err("accounting experiment binding mismatch".into());
    }
    if key.start_ms > policy.required_coverage_start_ms
        || key.end_exclusive_ms < policy.required_coverage_end_exclusive_ms
    {
        return Err("accounting experiment does not cover the complete research window".into());
    }
    let result = live
        .registry
        .result(&bundle.accounting.experiment_id)?
        .ok_or("accounting experiment result missing")?;
    if result.outcome != "completed" || result.result_hash != bundle.accounting.result_hash {
        return Err("accounting receipt is not a completed exact-hash result".into());
    }
    // Frozen scientific protocol report: integrity and candidate evidence gate.
    let report = crate::science::read_report(live.registry, &bundle.science.protocol_id)?
        .ok_or("scientific protocol report missing")?;
    if data::hash(&report)? != bundle.science.report_hash {
        return Err("scientific report hash mismatch".into());
    }
    if report.data_class != bundle.declared_class
        || report.data_class != bundle.science.report_data_class
    {
        return Err("scientific report data class mismatch".into());
    }
    let candidate = report
        .candidates
        .iter()
        .find(|c| c.concept_id == bundle.concept_id)
        .ok_or("scientific report omits the qualified concept")?;
    let gate = candidate
        .evidence_gate
        .as_ref()
        .ok_or("scientific report lacks frozen evidence gates")?;
    if data::hash(gate)? != data::hash(&bundle.science.gate)?
        || !gate.criteria_passed
        || gate.qualification_granted
    {
        return Err("frozen evidence gate mismatch or incomplete".into());
    }
    let probes = candidate
        .robustness_assessment
        .as_ref()
        .ok_or("scientific report lacks robustness assessment")?;
    let passed = probes
        .cases
        .iter()
        .filter(|c| c.state == "PASS_DECLARED_PROBES")
        .count();
    if passed != bundle.science.robustness_cases_passed
        || probes.cases.len() != bundle.science.robustness_cases_total
        || candidate.uncertainty.is_some() != bundle.science.uncertainty_present
        || candidate.block_uncertainty.is_some() != bundle.science.block_uncertainty_present
    {
        return Err("scientific evidence counters mismatch".into());
    }
    if candidate
        .experiments
        .iter()
        .any(|e| e.outcome != "completed")
    {
        return Err("scientific protocol windows are not all completed".into());
    }
    // Live catalog eligibility recomputation (five independent areas + machine revalidation).
    let eligibility = live.catalog.data_eligibility(
        live.authority,
        &bundle.data_approval.machine_id,
        live.archive,
        live.dataset,
    )?;
    if eligibility.state != "DATA_APPROVED_FOR_SCOPE"
        || eligibility.catalog_identity != bundle.data_approval.catalog_identity
        || eligibility.review_head != bundle.data_approval.review_head
    {
        return Err(
            "live data eligibility changed or does not match the presented approvals".into(),
        );
    }
    if eligibility.scope != bundle.data_approval.scope {
        return Err("approved data scope changed".into());
    }
    Ok(())
}
fn derive_board(
    events: &[Event],
    instrument: &str,
    authority: &Authority,
    now: i64,
) -> Result<BoardView> {
    // Latest decision per scope on this instrument; events arrive in sequence order.
    let mut latest: std::collections::BTreeMap<String, &Event> = std::collections::BTreeMap::new();
    let mut head: Option<String> = None;
    let mut rejected = 0_usize;
    let mut revoked = 0_usize;
    for e in events.iter().filter(|e| e.record.instrument == instrument) {
        latest.insert(e.record.concept_id.clone(), e);
        head = Some(e.hash.clone());
    }
    // The board policy is the frozen policy of the most recent Qualify decision;
    // ties break by decision hash so the choice is deterministic.
    let board_policy_event = latest
        .values()
        .filter(|e| e.record.request.decision == DecisionKind::Qualify)
        .max_by(|a, b| {
            a.record
                .recorded_ms
                .cmp(&b.record.recorded_ms)
                .then_with(|| a.hash.cmp(&b.hash))
        });
    let board_policy = board_policy_event.map(|e| e.record.policy.clone());
    let diversity_cap = board_policy
        .as_ref()
        .map(|p| p.maximum_per_diversity_class)
        .unwrap_or(2);
    let mut qualified: Vec<(&Event, Score)> = Vec::new();
    let mut excluded = Vec::new();
    for (concept, e) in &latest {
        match e.record.request.decision {
            DecisionKind::Reject => {
                rejected += 1;
                continue;
            }
            DecisionKind::Revoke => {
                revoked += 1;
                continue;
            }
            DecisionKind::Qualify => {}
        }
        if !decision_authentic(authority, e, now)? {
            excluded.push(ExcludedCandidate {
                concept_id: concept.clone(),
                decision_hash: e.hash.clone(),
                reason: "qualification decision is not authenticated under the current authority/grant; revalidation required".into(),
            });
            continue;
        }
        if board_policy
            .as_ref()
            .is_some_and(|p| data::hash(p).is_ok_and(|h| h != e.record.policy_hash))
        {
            excluded.push(ExcludedCandidate {
                concept_id: concept.clone(),
                decision_hash: e.hash.clone(),
                reason: "qualified under a different policy version; requalification under the current board policy required".into(),
            });
            continue;
        }
        let s = score(&e.record.bundle)?;
        qualified.push((e, s));
    }
    // Deterministic ranking: score desc, verified win rate desc, sample desc, concept asc.
    qualified.sort_by(|(a, sa), (b, sb)| {
        sb.total
            .cmp(&sa.total)
            .then_with(|| {
                let wa = a.record.bundle.accounting.pooled.wins as u128 * 10000
                    / (a.record.bundle.accounting.pooled.resolved as u128).max(1);
                let wb = b.record.bundle.accounting.pooled.wins as u128 * 10000
                    / (b.record.bundle.accounting.pooled.resolved as u128).max(1);
                wb.cmp(&wa)
            })
            .then_with(|| {
                b.record
                    .bundle
                    .accounting
                    .pooled
                    .resolved
                    .cmp(&a.record.bundle.accounting.pooled.resolved)
            })
            .then_with(|| a.record.concept_id.cmp(&b.record.concept_id))
    });
    let mut entries = Vec::new();
    let mut class_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for (e, s) in qualified {
        let class = diversity_class(&e.record.bundle.strategy);
        let count = class_counts.entry(class.clone()).or_insert(0);
        if *count >= diversity_cap {
            excluded.push(ExcludedCandidate {
                concept_id: e.record.concept_id.clone(),
                decision_hash: e.hash.clone(),
                reason: format!(
                    "diversity cap: at most {diversity_cap} strategies per structural class {class}"
                ),
            });
            continue;
        }
        if entries.len() >= BOARD_CAPACITY {
            excluded.push(ExcludedCandidate {
                concept_id: e.record.concept_id.clone(),
                decision_hash: e.hash.clone(),
                reason: "board full: five independently qualified strategies already ranked".into(),
            });
            continue;
        }
        *count += 1;
        let a = &e.record.bundle.accounting;
        entries.push(BoardEntry {
            rank: entries.len() + 1,
            concept_id: e.record.concept_id.clone(),
            decision_hash: e.hash.clone(),
            diversity_class: class,
            timeframe: format!("{:?}", e.record.bundle.strategy.timeframe),
            score: s,
            win_rate_bps: win_rate_bps(&a.pooled).unwrap_or(0),
            resolved: a.pooled.resolved,
            net_micro_usd: a.net_micro_usd,
            ending_micro_usd: a.ending_micro_usd,
            qualified_ms: e.record.recorded_ms,
        });
    }
    let mut empty_reasons = Vec::new();
    if entries.is_empty() {
        empty_reasons.push(format!(
            "No independently qualified strategy for {instrument}: {rejected} rejected and {revoked} revoked scopes retained in the archive; qualified positions require complete approved evidence and an authenticated independent decision."
        ));
        if !excluded.is_empty() {
            empty_reasons.push("Qualified candidates exist but none is currently board-eligible; see excluded reasons.".into());
        }
    }
    Ok(BoardView {
        instrument: instrument.into(),
        ranking_method: RANKING_VERSION.into(),
        policy_hash: board_policy.map(|p| data::hash(&p)).transpose()?,
        entries,
        excluded,
        empty_reasons,
        head,
        limitations: LIMIT.into(),
    })
}
fn build_snapshot(
    tx: &Connection,
    event: &Event,
    authority: &Authority,
    now: i64,
) -> Result<BoardSnapshot> {
    let mut q = tx
        .prepare("SELECT sequence,id,instrument,concept_id,body,authenticator FROM qualification_decisions ORDER BY sequence LIMIT 10001")
        .map_err(db_err)?;
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .map_err(db_err)?;
    let mut events = Vec::new();
    for row in rows {
        let (sequence, id, instrument, concept, body, tag): (
            i64,
            String,
            String,
            String,
            Vec<u8>,
            String,
        ) = row.map_err(db_err)?;
        let e = decode(sequence, id, body, tag)?;
        if e.record.instrument != instrument || e.record.concept_id != concept {
            return Err("qualification decision scope binding mismatch".into());
        }
        events.push(e);
    }
    drop(q);
    let view = derive_board(&events, &event.record.instrument, authority, now)?;
    let previous: Option<String> = tx
        .query_row(
            "SELECT id FROM qualification_boards WHERE instrument=?1 ORDER BY sequence DESC LIMIT 1",
            [&event.record.instrument],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_err)?;
    let sequence: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(sequence),0) FROM qualification_boards",
            [],
            |r| r.get(0),
        )
        .map_err(db_err)?;
    Ok(BoardSnapshot {
        version: BOARD_VERSION.into(),
        instrument: event.record.instrument.clone(),
        sequence: sequence
            .checked_add(1)
            .ok_or("snapshot sequence overflow")?,
        decision_hash: event.hash.clone(),
        previous,
        policy_hash: view.policy_hash.clone().unwrap_or_default(),
        ranking_method: RANKING_VERSION.into(),
        published_ms: now,
        entries: view.entries,
        excluded: view.excluded,
        empty_reasons: view.empty_reasons,
        limitations: LIMIT.into(),
    })
}

#[cfg(test)]
mod tests;
