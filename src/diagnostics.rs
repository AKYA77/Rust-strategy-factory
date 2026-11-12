//! Descriptive OOS receipt breakdowns; realized cash drawdown is not marked-account drawdown.
use crate::{Result, contracts::INITIAL_EQUITY, tick_engine::Trade};
use chrono::{Datelike, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const VERSION: &str = "oos-exit-year-entry-utc-session-realized-cash-v1";
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Bucket {
    pub resolved: usize,
    pub wins: usize,
    pub losses: usize,
    pub breakevens: usize,
    /// Exact expectancy is this numerator divided by resolved; no float accounting.
    pub sum_net_micro_usd: i128,
}
impl Bucket {
    fn add(&mut self, net: i128) -> Result<()> {
        self.resolved += 1;
        self.wins += usize::from(net > 0);
        self.losses += usize::from(net < 0);
        self.breakevens += usize::from(net == 0);
        self.sum_net_micro_usd = self
            .sum_net_micro_usd
            .checked_add(net)
            .ok_or("breakdown P&L overflow")?;
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoldRisk {
    pub phase: String,
    pub replay_present: bool,
    pub unresolved: bool,
    pub closed: usize,
    pub ending_realized_cash_micro_usd: i128,
    pub peak_realized_cash_micro_usd: i128,
    pub maximum_realized_cash_drawdown_micro_usd: i128,
    pub negative_realized_cash: bool,
    #[serde(skip)]
    last_exit_ms: Option<i64>,
}
impl FoldRisk {
    pub fn new(phase: String) -> Self {
        Self {
            phase,
            replay_present: false,
            unresolved: false,
            closed: 0,
            ending_realized_cash_micro_usd: INITIAL_EQUITY,
            peak_realized_cash_micro_usd: INITIAL_EQUITY,
            maximum_realized_cash_drawdown_micro_usd: 0,
            negative_realized_cash: false,
            last_exit_ms: None,
        }
    }
    fn add(&mut self, net: i128) -> Result<()> {
        let ending = self
            .ending_realized_cash_micro_usd
            .checked_add(net)
            .ok_or("realized cash overflow")?;
        let peak = self.peak_realized_cash_micro_usd.max(ending);
        let drawdown = peak.checked_sub(ending).ok_or("drawdown overflow")?;
        self.closed += 1;
        self.ending_realized_cash_micro_usd = ending;
        self.peak_realized_cash_micro_usd = peak;
        self.maximum_realized_cash_drawdown_micro_usd =
            self.maximum_realized_cash_drawdown_micro_usd.max(drawdown);
        self.negative_realized_cash |= ending < 0;
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breakdown {
    pub method: String,
    pub exit_years: BTreeMap<i32, Bucket>,
    /// UTC entry hour / 6; fixed bins, not broker sessions or DST-adjusted market sessions.
    pub entry_utc_six_hour_bins: BTreeMap<u32, Bucket>,
    pub folds: Vec<FoldRisk>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub entry_regimes: BTreeMap<String, Bucket>,
    pub warnings: Vec<String>,
}
impl Default for Breakdown {
    fn default() -> Self {
        Self{method:VERSION.into(),exit_years:BTreeMap::new(),entry_utc_six_hour_bins:BTreeMap::new(),folds:Vec::new(),entry_regimes:BTreeMap::new(),warnings:vec!["OOS-only reset-fold receipt breakdown; no full-period coverage or qualification".into(),"UTC entry bins are not approved broker/DST sessions".into(),"Realized cash drawdown excludes intratrade marks, MFE/MAE and unresolved positions".into(),"Exact net expectancy per bucket is sum_net_micro_usd / resolved; empty buckets have no estimate".into()]}
    }
}
impl Breakdown {
    pub fn record_regime(&mut self, label: &str, net: i128) -> Result<()> {
        let mut next = self.entry_regimes.get(label).cloned().unwrap_or_default();
        next.add(net)?;
        self.entry_regimes.insert(label.into(), next);
        Ok(())
    }
    pub fn record(&mut self, trade: &Trade, fold: &mut FoldRisk) -> Result<()> {
        if fold.last_exit_ms.is_some_and(|last| trade.entry_ms < last) {
            return Err("nonchronological or overlapping fold receipts".into());
        }
        if trade.entry_ms > trade.exit_ms {
            return Err("entry after exit in receipt breakdown".into());
        }
        let entry =
            chrono::DateTime::from_timestamp_millis(trade.entry_ms).ok_or("invalid entry time")?;
        let exit =
            chrono::DateTime::from_timestamp_millis(trade.exit_ms).ok_or("invalid exit time")?;
        let mut annual = self
            .exit_years
            .get(&exit.year())
            .cloned()
            .unwrap_or_default();
        let mut session = self
            .entry_utc_six_hour_bins
            .get(&(entry.hour() / 6))
            .cloned()
            .unwrap_or_default();
        annual.add(trade.net_micro_usd)?;
        session.add(trade.net_micro_usd)?;
        fold.add(trade.net_micro_usd)?;
        self.exit_years.insert(exit.year(), annual);
        self.entry_utc_six_hour_bins
            .insert(entry.hour() / 6, session);
        fold.last_exit_ms = Some(trade.exit_ms);
        Ok(())
    }
}
