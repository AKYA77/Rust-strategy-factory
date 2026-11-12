//! Numerical gates are separate from independent scientific qualification.
//! This module cannot issue VALIDATED/qualified status without a protected evidence system.
use crate::Result;
use chrono::{DateTime, Datelike, Months, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub exit_ms: i64,
    pub net_micro_usd: i128,
}
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeriodStats {
    pub resolved: usize,
    pub wins: usize,
    pub losses: usize,
    pub breakeven: usize,
    pub max_losing_streak: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assessment {
    pub protocol: String,
    pub pooled: PeriodStats,
    pub year_2025: PeriodStats,
    pub year_2026: PeriodStats,
    pub net_micro_usd: i128,
    pub ending_micro_usd: i128,
    pub numeric_gates_passed: bool,
    pub status: String,
    pub reasons: Vec<String>,
}
fn update(stats: &mut PeriodStats, streak: &mut usize, value: i128) {
    stats.resolved += 1;
    if value > 0 {
        stats.wins += 1;
        *streak = 0;
    } else if value < 0 {
        stats.losses += 1;
        *streak += 1;
    } else {
        stats.breakeven += 1;
    }
    stats.max_losing_streak = stats.max_losing_streak.max(*streak);
}
pub fn assess(trades: &[Observation], cutoff_ms: i64, unresolved: usize) -> Result<Assessment> {
    let cutoff = DateTime::from_timestamp_millis(cutoff_ms).ok_or("invalid cutoff")?;
    if cutoff.year() != 2026 {
        return Err("this qualification contract requires an explicit 2026 YTD cutoff".into());
    }
    let mut pooled = PeriodStats::default();
    let mut y25 = PeriodStats::default();
    let mut y26 = PeriodStats::default();
    let (mut streak, mut s25, mut s26) = (0, 0, 0);
    let mut net = 0_i128;
    for (i, t) in trades.iter().enumerate() {
        if t.exit_ms > cutoff_ms || i > 0 && t.exit_ms < trades[i - 1].exit_ms {
            return Err("nonchronological trades or exit beyond cutoff".into());
        }
        let year = DateTime::from_timestamp_millis(t.exit_ms)
            .ok_or("invalid exit timestamp")?
            .year();
        if ![2025, 2026].contains(&year) {
            return Err("trade outside approved research years".into());
        }
        update(&mut pooled, &mut streak, t.net_micro_usd);
        if year == 2025 {
            update(&mut y25, &mut s25, t.net_micro_usd);
        } else {
            update(&mut y26, &mut s26, t.net_micro_usd);
        }
        net = net
            .checked_add(t.net_micro_usd)
            .ok_or("aggregate P&L overflow")?;
    }
    let ending = crate::contracts::INITIAL_EQUITY
        .checked_add(net)
        .ok_or("equity overflow")?;
    let mut reasons = Vec::new();
    if pooled.resolved == 0 || pooled.wins < pooled.resolved.div_ceil(2) {
        reasons.push("verified net win rate below 50% or no resolved trades".into());
    }
    if y25.resolved < 50 {
        reasons.push("fewer than 50 resolved exits in 2025".into());
    }
    if y26.resolved < 50 {
        reasons.push("fewer than 50 resolved exits in 2026 YTD".into());
    }
    if pooled.max_losing_streak > 5 {
        reasons.push("consecutive net losing trades exceed five".into());
    }
    if unresolved > 0 {
        reasons.push("unresolved trades require evidence review".into());
    }
    if ending < 500_000_000 {
        reasons.push("hypothetical ending equity below $500 objective".into());
    }
    let passed = reasons.is_empty();
    // Explicitly fail closed: numeric results alone are never independent validation.
    reasons.push("independent protected validation, approved provenance/profile, full coverage, accounting and version evidence not established by numerical assessment".into());
    Ok(Assessment {
        protocol: "numeric-2025-2026-v1".into(),
        pooled,
        year_2025: y25,
        year_2026: y26,
        net_micro_usd: net,
        ending_micro_usd: ending,
        numeric_gates_passed: passed,
        status: if passed {
            "INSUFFICIENT_EVIDENCE".into()
        } else {
            "REJECTED".into()
        },
        reasons,
    })
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interval {
    pub start_ms: i64,
    pub end_exclusive_ms: i64,
}
impl Interval {
    pub fn validate(&self) -> Result<()> {
        if self.start_ms >= self.end_exclusive_ms
            || DateTime::from_timestamp_millis(self.start_ms).is_none()
            || DateTime::from_timestamp_millis(self.end_exclusive_ms).is_none()
        {
            return Err("invalid chronological interval".into());
        }
        Ok(())
    }
    pub fn contains(&self, time: i64) -> bool {
        self.start_ms <= time && time < self.end_exclusive_ms
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fold {
    pub train: Interval,
    pub test: Interval,
    pub embargo_ms: i64,
}
impl Fold {
    pub fn validate(&self) -> Result<()> {
        self.train.validate()?;
        self.test.validate()?;
        if self.embargo_ms < crate::contracts::MAX_HOLD_MS
            || self
                .train
                .end_exclusive_ms
                .checked_add(self.embargo_ms)
                .ok_or("embargo overflow")?
                > self.test.start_ms
        {
            return Err(
                "training overlaps test or embargo is shorter than maximum trade lifetime".into(),
            );
        }
        Ok(())
    }
    /// Both decision and final outcome must belong to training. No crossing positions leak labels.
    pub fn training_observation(&self, decision_ms: i64, exit_ms: i64) -> bool {
        decision_ms <= exit_ms && self.train.contains(decision_ms) && self.train.contains(exit_ms)
    }
}
/// Expanding prior-only training and calendar-three-month tests; incomplete final folds omitted.
pub fn walk_forward(
    training_start_ms: i64,
    first_test_ms: i64,
    end_exclusive_ms: i64,
    embargo_ms: i64,
) -> Result<Vec<Fold>> {
    let mut start: DateTime<Utc> =
        DateTime::from_timestamp_millis(first_test_ms).ok_or("invalid first test")?;
    if start.day() != 1 || start.time() != chrono::NaiveTime::MIN {
        return Err("walk-forward tests must begin at UTC calendar-month boundary".into());
    }
    let mut folds = Vec::new();
    loop {
        if folds.len() >= 400 {
            return Err("walk-forward fold budget exceeded".into());
        }
        let next = start
            .checked_add_months(Months::new(3))
            .ok_or("calendar overflow")?;
        if next.timestamp_millis() > end_exclusive_ms {
            break;
        }
        let fold = Fold {
            train: Interval {
                start_ms: training_start_ms,
                end_exclusive_ms: start
                    .timestamp_millis()
                    .checked_sub(embargo_ms)
                    .ok_or("embargo overflow")?,
            },
            test: Interval {
                start_ms: start.timestamp_millis(),
                end_exclusive_ms: next.timestamp_millis(),
            },
            embargo_ms,
        };
        fold.validate()?;
        folds.push(fold);
        start = next;
    }
    if folds.is_empty() {
        return Err("insufficient chronological coverage for one complete three-month fold".into());
    }
    Ok(folds)
}
