//! Bounded factual training receipt summaries, without inference or acceptance gates.
use crate::{Result, diagnostics::Bucket, tick_engine::Replay, validation::Interval};
use chrono::{Datelike, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const VERSION: &str = "training-receipt-breakdown-v1";
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub method: String,
    pub closed: Bucket,
    /// Strict consecutive negative exits; a zero or positive exit ends the streak.
    pub maximum_consecutive_losses: usize,
    pub closed_commission_micro_usd: i128,
    pub exit_years: BTreeMap<i32, Bucket>,
    pub entry_utc_six_hour_bins: BTreeMap<u32, Bucket>,
    pub exit_reasons: BTreeMap<String, usize>,
    pub entry_statuses: BTreeMap<String, usize>,
    pub rejection_reasons: BTreeMap<String, usize>,
    pub recorded_initial_micro_usd: i128,
    pub recorded_realized_cash_micro_usd: i128,
    pub recorded_final_marked_micro_usd: i128,
    pub recorded_maximum_drawdown_micro_usd: i128,
    pub unresolved_position: bool,
    pub unresolved_pending_entry: bool,
    pub evidence_limits: Vec<String>,
    pub status: String,
}
fn count(map: &mut BTreeMap<String, usize>, key: String) -> Result<()> {
    if key.len() > 1024 || (!map.contains_key(&key) && map.len() >= 64) {
        return Err("training reason budget exceeded".into());
    }
    let value = map.entry(key).or_default();
    *value = value.checked_add(1).ok_or("training count overflow")?;
    Ok(())
}
pub fn summarize(replay: &Replay, window: &Interval) -> Result<Evidence> {
    window.validate()?;
    if replay.trades.len() > 1_000_000
        || replay.rejections.len() > 1_000_000
        || replay.entry_receipts.len() > 1_000_000
    {
        return Err("training receipt budget exceeded".into());
    }
    let mut evidence=Evidence {
        method:VERSION.into(),closed:Bucket::default(),maximum_consecutive_losses:0,closed_commission_micro_usd:0,
        exit_years:BTreeMap::new(),entry_utc_six_hour_bins:BTreeMap::new(),exit_reasons:BTreeMap::new(),entry_statuses:BTreeMap::new(),rejection_reasons:BTreeMap::new(),
        recorded_initial_micro_usd:replay.initial_micro_usd,recorded_realized_cash_micro_usd:replay.realized_micro_usd,recorded_final_marked_micro_usd:replay.final_marked_micro_usd,recorded_maximum_drawdown_micro_usd:replay.maximum_drawdown_micro_usd,
        unresolved_position:replay.unresolved.is_some(),unresolved_pending_entry:replay.pending_entry.is_some(),
        evidence_limits:vec!["Training-only observed receipts; no independent performance estimate".into(),"Independent sample size and full-history coverage unestablished".into(),"UTC bins are not broker/DST sessions; profile approval is not established here".into(),"Account fields are recorded replay values, not independently recomputed conversion/equity evidence".into(),"Closed commissions exclude unresolved entry commissions".into()],status:"INSUFFICIENT_EVIDENCE".into(),
    };
    let mut last_exit = None;
    let mut streak = 0;
    for trade in &replay.trades {
        if !window.contains(trade.entry_ms)
            || !window.contains(trade.exit_ms)
            || trade.entry_ms > trade.exit_ms
            || trade.entry_event >= trade.exit_event
            || last_exit.is_some_and(|(ms, seq)| trade.entry_ms < ms || trade.entry_event <= seq)
        {
            return Err("training receipt chronology/window mismatch".into());
        }
        let entry = chrono::DateTime::from_timestamp_millis(trade.entry_ms)
            .ok_or("invalid training entry time")?;
        let exit = chrono::DateTime::from_timestamp_millis(trade.exit_ms)
            .ok_or("invalid training exit time")?;
        if !evidence.exit_years.contains_key(&exit.year()) && evidence.exit_years.len() >= 128 {
            return Err("training year budget exceeded".into());
        }
        evidence.closed.add(trade.net_micro_usd)?;
        evidence
            .exit_years
            .entry(exit.year())
            .or_default()
            .add(trade.net_micro_usd)?;
        evidence
            .entry_utc_six_hour_bins
            .entry(entry.hour() / 6)
            .or_default()
            .add(trade.net_micro_usd)?;
        evidence.closed_commission_micro_usd = evidence
            .closed_commission_micro_usd
            .checked_add(trade.commission_micro_usd)
            .ok_or("training commission overflow")?;
        streak = if trade.net_micro_usd < 0 {
            streak + 1
        } else {
            0
        };
        evidence.maximum_consecutive_losses = evidence.maximum_consecutive_losses.max(streak);
        count(&mut evidence.exit_reasons, format!("{:?}", trade.reason))?;
        last_exit = Some((trade.exit_ms, trade.exit_event));
    }
    for receipt in &replay.entry_receipts {
        count(
            &mut evidence.entry_statuses,
            format!("{:?}", receipt.status),
        )?;
    }
    for rejection in &replay.rejections {
        count(&mut evidence.rejection_reasons, rejection.reason.clone())?;
    }
    if evidence.closed.resolved == 0 {
        evidence
            .evidence_limits
            .push("No closed trades; win rate and net expectancy are undefined".into());
    }
    if evidence.unresolved_position || evidence.unresolved_pending_entry {
        evidence
            .evidence_limits
            .push("Unresolved outcomes prevent a complete net comparison".into());
    }
    Ok(evidence)
}
