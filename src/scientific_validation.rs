//! Owner-criteria scientific validation over labelled execution evidence.
//!
//! This module computes the owner-approved numerical criteria — net win rate of at least
//! 50.00%, at most five consecutive losses, the frozen 3:1 planned reward/risk, at least 50
//! resolved exits in each of 2025 and 2026, and the hypothetical equity objective — from
//! execution reports that already carry per-month outcome statistics.
//!
//! It is strictly fail-closed and descriptive:
//!
//! * `qualification_granted` is always `false`; no function here can qualify a strategy.
//! * Diagnostic (`diagnostic_simulation`) or ineligible evidence is `REJECTED`, never
//!   presented as a numerical pass.
//! * A reset-fold diagnostic summary can never establish the continuous single-account
//!   full-period accounting the owner criteria require, so that criterion stays `unknown`.
//! * Synthetic or diagnostic evidence can never qualify.
//!
//! The evaluator changes no financial rule and reads only already-produced reports.

use crate::Result;
use serde::{Deserialize, Serialize};

/// Method identifier recorded in every report this module produces.
pub const METHOD: &str = "scientific-validation-reset-fold-v1";
/// The engine freezes a 1:3 planned reward/risk; 3000 milli == 3.000.
pub const PLANNED_RR_MILLI: u64 = 3_000;
/// Owner floor: at least 50 resolved exits in 2025 and at least 50 in 2026.
pub const MINIMUM_RESOLVED_PER_YEAR: usize = 50;
/// Owner floor: verified net win rate of at least 50.00%.
pub const MINIMUM_NET_WIN_RATE_BPS: u16 = 5_000;
/// Owner ceiling: at most five consecutive net losing trades.
pub const MAXIMUM_CONSECUTIVE_LOSSES: usize = 5;
/// Hypothetical equity objective in micro-USD ($500).
pub const EQUITY_OBJECTIVE_MICRO_USD: i128 = 500_000_000;

const STATUS_REJECTED: &str = "REJECTED";
const STATUS_INSUFFICIENT: &str = "INSUFFICIENT_EVIDENCE";
const STATE_PASS: &str = "pass";
const STATE_FAIL: &str = "fail";
const STATE_UNKNOWN: &str = "unknown";

pub const SCOPE: &str = "Reset-fold diagnostic summary only. Each diagnostic month starts a fresh account, so these are descriptive observations rather than a continuous single-account full-period result. No independent calendar/profile/legal/scientific approval is established, and no eligibility or qualification is granted or implied. Synthetic or diagnostic evidence can never qualify.";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YearStats {
    pub resolved: usize,
    pub wins: usize,
    pub losses: usize,
    pub breakevens: usize,
    pub net_micro_usd: i128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    pub name: String,
    /// `pass`, `fail` or `unknown`. `unknown` never counts as a pass.
    pub state: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CriteriaVerdict {
    pub status: String,
    /// True only when every numerical criterion passes. Independent of eligibility.
    pub numeric_gates_passed: bool,
    pub criteria: Vec<Criterion>,
    pub reasons: Vec<String>,
    pub qualification_granted: bool,
    pub scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentValidation {
    pub instrument: String,
    pub identity: String,
    pub strategy_identity: String,
    pub classification: String,
    pub eligibility: String,
    pub conversion_policy: String,
    pub months_executed: usize,
    pub months_refused: usize,
    pub resolved: usize,
    pub wins: usize,
    pub losses: usize,
    pub breakevens: usize,
    /// `wins / resolved` in basis points, floored. Zero when nothing resolved.
    pub net_win_rate_bps: u16,
    /// Worst per-month losing streak. Reset-fold: months reset their account.
    pub max_losing_streak: usize,
    pub gross_win_micro_usd: i128,
    pub gross_loss_micro_usd: i128,
    /// `gross_win / gross_loss` in milli, or `None` when there are no losses.
    pub achieved_rr_milli: Option<u64>,
    pub planned_rr_milli: u64,
    pub net_micro_usd: i128,
    /// `INITIAL_EQUITY + Σ monthly P&L`. Additive descriptive figure, not a compounded account.
    pub additive_ending_micro_usd: i128,
    pub year_2025: YearStats,
    pub year_2026: YearStats,
    pub unresolved_positions: usize,
    pub criteria: CriteriaVerdict,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationTotals {
    pub instruments: usize,
    pub instruments_executed: usize,
    pub instruments_refused: usize,
    pub resolved: usize,
    pub wins: usize,
    pub losses: usize,
    pub breakevens: usize,
    pub net_win_rate_bps: u16,
    pub max_losing_streak: usize,
    pub gross_win_micro_usd: i128,
    pub gross_loss_micro_usd: i128,
    pub net_micro_usd: i128,
    pub additive_ending_micro_usd: i128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationReport {
    pub method: String,
    pub classification: String,
    pub eligibility: String,
    pub instruments: Vec<InstrumentValidation>,
    pub totals: ValidationTotals,
    pub qualification_granted: bool,
    pub limitations: String,
}

fn checked_add_usize(value: &mut usize, add: usize, what: &str) -> Result<()> {
    *value = value
        .checked_add(add)
        .ok_or_else(|| format!("{what} overflow"))?;
    Ok(())
}

fn checked_add_i128(value: &mut i128, add: i128, what: &str) -> Result<()> {
    *value = value
        .checked_add(add)
        .ok_or_else(|| format!("{what} overflow"))?;
    Ok(())
}

fn bucket_year(
    month: &crate::historical_corpus::MonthExecution,
    year_2025: &mut YearStats,
    year_2026: &mut YearStats,
) -> Result<()> {
    let stats = &month.outcomes;
    let bucket = if month.year == 2025 {
        year_2025
    } else if month.year == 2026 {
        year_2026
    } else {
        return Err(format!(
            "diagnostic month {}-{} is outside the approved 2025/2026 research years",
            month.year, month.month
        ));
    };
    checked_add_usize(&mut bucket.resolved, stats.resolved, "resolved")?;
    checked_add_usize(&mut bucket.wins, stats.wins, "wins")?;
    checked_add_usize(&mut bucket.losses, stats.losses, "losses")?;
    checked_add_usize(&mut bucket.breakevens, stats.breakevens, "breakevens")?;
    checked_add_i128(&mut bucket.net_micro_usd, stats.net_micro_usd, "net")?;
    Ok(())
}

fn win_rate_bps(wins: usize, resolved: usize) -> u16 {
    if resolved == 0 {
        return 0;
    }
    (wins as u128)
        .saturating_mul(10_000)
        .checked_div(resolved as u128)
        .unwrap_or(u16::MAX as u128)
        .min(u16::MAX as u128) as u16
}

fn achieved_rr_milli(gross_win: i128, gross_loss: i128) -> Option<u64> {
    if gross_loss <= 0 {
        return None;
    }
    (gross_win.max(0) as u128)
        .saturating_mul(1_000)
        .checked_div(gross_loss as u128)
        .map(|value| value.min(u64::MAX as u128) as u64)
}

fn criterion(name: &str, state: &str, detail: String) -> Criterion {
    Criterion {
        name: name.into(),
        state: state.into(),
        detail,
    }
}

/// Numerical inputs to the fail-closed criteria evaluation.
struct CriteriaInputs<'a> {
    classification: &'a str,
    eligibility: &'a str,
    resolved: usize,
    year_2025: &'a YearStats,
    year_2026: &'a YearStats,
    net_win_rate_bps: u16,
    max_losing_streak: usize,
    achieved_rr_milli: Option<u64>,
    additive_ending_micro_usd: i128,
}

fn evaluate(inputs: &CriteriaInputs<'_>) -> CriteriaVerdict {
    let classification = inputs.classification;
    let eligibility = inputs.eligibility;
    let resolved = inputs.resolved;
    let year_2025 = inputs.year_2025;
    let year_2026 = inputs.year_2026;
    let net_win_rate_bps = inputs.net_win_rate_bps;
    let max_losing_streak = inputs.max_losing_streak;
    let achieved_rr_milli = inputs.achieved_rr_milli;
    let additive_ending_micro_usd = inputs.additive_ending_micro_usd;
    let mut criteria = Vec::new();
    let mut reasons = Vec::new();

    let eligible_class = classification != crate::historical_corpus::EXECUTION_CLASSIFICATION
        && eligibility != crate::historical_corpus::EXECUTION_ELIGIBILITY;
    criteria.push(criterion(
        "declared_data_class",
        if eligible_class { STATE_PASS } else { STATE_FAIL },
        format!(
            "classification={classification} eligibility={eligibility}; diagnostic simulation and ineligible classes are not evidence"
        ),
    ));
    if !eligible_class {
        reasons
            .push("declared evidence class is diagnostic/ineligible and can never qualify".into());
    }

    let resolved_2025 = year_2025.resolved >= MINIMUM_RESOLVED_PER_YEAR;
    criteria.push(criterion(
        "resolved_2025",
        if resolved_2025 {
            STATE_PASS
        } else {
            STATE_FAIL
        },
        format!(
            "resolved exits in 2025 = {} (floor {})",
            year_2025.resolved, MINIMUM_RESOLVED_PER_YEAR
        ),
    ));
    let resolved_2026 = year_2026.resolved >= MINIMUM_RESOLVED_PER_YEAR;
    criteria.push(criterion(
        "resolved_2026",
        if resolved_2026 {
            STATE_PASS
        } else {
            STATE_FAIL
        },
        format!(
            "resolved exits in 2026 = {} (floor {})",
            year_2026.resolved, MINIMUM_RESOLVED_PER_YEAR
        ),
    ));

    let win_rate_ok = resolved > 0 && net_win_rate_bps >= MINIMUM_NET_WIN_RATE_BPS;
    criteria.push(criterion(
        "net_win_rate",
        if win_rate_ok { STATE_PASS } else { STATE_FAIL },
        format!(
            "net win rate = {net_win_rate_bps} bps (floor {MINIMUM_NET_WIN_RATE_BPS} bps) over {resolved} resolved exits"
        ),
    ));

    let streak_ok = max_losing_streak <= MAXIMUM_CONSECUTIVE_LOSSES;
    criteria.push(criterion(
        "max_consecutive_losses",
        if streak_ok { STATE_PASS } else { STATE_FAIL },
        format!(
            "worst per-month losing streak = {max_losing_streak} (ceiling {MAXIMUM_CONSECUTIVE_LOSSES}); reset-fold, not continuous"
        ),
    ));

    let rr_state = match achieved_rr_milli {
        Some(value) if value >= PLANNED_RR_MILLI => STATE_PASS,
        Some(_) => STATE_FAIL,
        None => STATE_UNKNOWN,
    };
    criteria.push(criterion(
        "achieved_reward_risk",
        rr_state,
        format!(
            "achieved gross win/loss = {:?} milli versus the frozen planned {} milli; descriptive only",
            achieved_rr_milli, PLANNED_RR_MILLI
        ),
    ));

    let equity_ok = additive_ending_micro_usd >= EQUITY_OBJECTIVE_MICRO_USD;
    criteria.push(criterion(
        "equity_objective",
        if equity_ok { STATE_PASS } else { STATE_FAIL },
        format!(
            "additive ending equity = {additive_ending_micro_usd} micro-USD (objective {EQUITY_OBJECTIVE_MICRO_USD}); additive, not compounded"
        ),
    ));

    // The reset-fold diagnostic path cannot establish this; it stays unknown and is never
    // counted as a pass.
    criteria.push(criterion(
        "continuous_full_period_accounting",
        STATE_UNKNOWN,
        "diagnostic months reset to the initial account; a continuous single-account full-period replay is not established".into(),
    ));
    criteria.push(criterion(
        "independent_evidence",
        STATE_UNKNOWN,
        "calendar/profile/legal/scientific approvals and corpus-wide coverage remain pending"
            .into(),
    ));

    let numeric_failures: Vec<&Criterion> = criteria
        .iter()
        .filter(|item| {
            item.state == STATE_FAIL
                && matches!(
                    item.name.as_str(),
                    "resolved_2025"
                        | "resolved_2026"
                        | "net_win_rate"
                        | "max_consecutive_losses"
                        | "equity_objective"
                )
        })
        .collect();
    let numeric_gates_passed = numeric_failures.is_empty();
    for item in numeric_failures {
        reasons.push(format!("{} failed: {}", item.name, item.detail));
    }

    let status = if !eligible_class {
        STATUS_REJECTED
    } else if numeric_gates_passed {
        STATUS_INSUFFICIENT
    } else {
        STATUS_REJECTED
    };
    if status == STATUS_INSUFFICIENT {
        reasons.push(
            "numerical criteria alone are never independent qualification; continuous full-period accounting and independent evidence remain unknown".into(),
        );
    }
    reasons.push(
        "independent protected validation, approved provenance/profile, full coverage, accounting and version evidence not established by this assessment".into(),
    );

    CriteriaVerdict {
        status: status.into(),
        numeric_gates_passed,
        criteria,
        reasons,
        qualification_granted: false,
        scope: SCOPE.into(),
    }
}

/// Summarize one instrument's labelled execution report against the owner criteria.
pub fn summarize(
    report: &crate::historical_corpus::ExecutionReport,
) -> Result<InstrumentValidation> {
    let totals = &report.totals;
    let mut year_2025 = YearStats::default();
    let mut year_2026 = YearStats::default();
    for month in &report.months {
        bucket_year(month, &mut year_2025, &mut year_2026)?;
    }
    if totals.wins != year_2025.wins + year_2026.wins
        || totals.losses != year_2025.losses + year_2026.losses
        || totals.breakevens != year_2025.breakevens + year_2026.breakevens
        || totals.trades != year_2025.resolved + year_2026.resolved
    {
        return Err("diagnostic execution totals do not reconcile with per-month outcomes".into());
    }
    let additive_ending_micro_usd = crate::contracts::INITIAL_EQUITY
        .checked_add(totals.pnl_micro_usd)
        .ok_or("additive ending equity overflow")?;
    let criteria = evaluate(&CriteriaInputs {
        classification: &report.classification,
        eligibility: &report.eligibility,
        resolved: totals.trades,
        year_2025: &year_2025,
        year_2026: &year_2026,
        net_win_rate_bps: win_rate_bps(totals.wins, totals.trades),
        max_losing_streak: totals.max_losing_streak,
        achieved_rr_milli: achieved_rr_milli(
            totals.gross_win_micro_usd,
            totals.gross_loss_micro_usd,
        ),
        additive_ending_micro_usd,
    });
    Ok(InstrumentValidation {
        instrument: report.instrument.clone(),
        identity: report.identity.clone(),
        strategy_identity: report.strategy_identity.clone(),
        classification: report.classification.clone(),
        eligibility: report.eligibility.clone(),
        conversion_policy: report.conversion_policy.clone(),
        months_executed: totals.months_executed,
        months_refused: totals.months_refused,
        resolved: totals.trades,
        wins: totals.wins,
        losses: totals.losses,
        breakevens: totals.breakevens,
        net_win_rate_bps: win_rate_bps(totals.wins, totals.trades),
        max_losing_streak: totals.max_losing_streak,
        gross_win_micro_usd: totals.gross_win_micro_usd,
        gross_loss_micro_usd: totals.gross_loss_micro_usd,
        achieved_rr_milli: achieved_rr_milli(
            totals.gross_win_micro_usd,
            totals.gross_loss_micro_usd,
        ),
        planned_rr_milli: PLANNED_RR_MILLI,
        net_micro_usd: totals.net_micro_usd,
        additive_ending_micro_usd,
        year_2025,
        year_2026,
        unresolved_positions: totals.unresolved_positions,
        criteria,
    })
}

/// Summarize every instrument report into one fail-closed validation report.
pub fn summarize_all(
    reports: &[crate::historical_corpus::ExecutionReport],
) -> Result<ValidationReport> {
    let mut instruments = Vec::with_capacity(reports.len());
    for report in reports {
        instruments.push(summarize(report)?);
    }
    let mut totals = ValidationTotals {
        instruments: instruments.len(),
        instruments_executed: instruments
            .iter()
            .filter(|item| item.months_executed > 0)
            .count(),
        instruments_refused: instruments
            .iter()
            .filter(|item| item.months_executed == 0)
            .count(),
        ..ValidationTotals::default()
    };
    for item in &instruments {
        checked_add_usize(&mut totals.resolved, item.resolved, "resolved")?;
        checked_add_usize(&mut totals.wins, item.wins, "wins")?;
        checked_add_usize(&mut totals.losses, item.losses, "losses")?;
        checked_add_usize(&mut totals.breakevens, item.breakevens, "breakevens")?;
        totals.max_losing_streak = totals.max_losing_streak.max(item.max_losing_streak);
        checked_add_i128(
            &mut totals.gross_win_micro_usd,
            item.gross_win_micro_usd,
            "gross win",
        )?;
        checked_add_i128(
            &mut totals.gross_loss_micro_usd,
            item.gross_loss_micro_usd,
            "gross loss",
        )?;
        checked_add_i128(&mut totals.net_micro_usd, item.net_micro_usd, "net")?;
        checked_add_i128(
            &mut totals.additive_ending_micro_usd,
            item.additive_ending_micro_usd,
            "additive ending",
        )?;
    }
    totals.net_win_rate_bps = win_rate_bps(totals.wins, totals.resolved);
    let classification = if instruments
        .iter()
        .all(|item| item.classification == crate::historical_corpus::EXECUTION_CLASSIFICATION)
    {
        crate::historical_corpus::EXECUTION_CLASSIFICATION.to_string()
    } else {
        "mixed".to_string()
    };
    let eligibility = if instruments
        .iter()
        .all(|item| item.eligibility == crate::historical_corpus::EXECUTION_ELIGIBILITY)
    {
        crate::historical_corpus::EXECUTION_ELIGIBILITY.to_string()
    } else {
        "mixed".to_string()
    };
    Ok(ValidationReport {
        method: METHOD.into(),
        classification,
        eligibility,
        instruments,
        totals,
        qualification_granted: false,
        limitations: SCOPE.into(),
    })
}
