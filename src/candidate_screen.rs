//! Bounded, explicitly hypothetical M1 cash model. Never an authoritative fill receipt.
//! Uncertain paths use adverse bar extrema and reserve the whole exit bar. No candidate
//! rejection or scientific eligibility is inferred from this approximate trajectory.
use crate::{
    Result,
    candles::{self, Candle, Timeframe},
    contracts::{self, Conversion, Side},
    data::Dataset,
    indicators::Mask,
    strategies::Strategy,
    tick_engine::{Costs, Replay},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const VERSION: &str = "m1-hypothetical-cash-v1";
pub const MAX_BAR_VISITS: usize = 25_000;

/// Configurable library-level shadow policy. This version never skips replay.
/// A later execution policy must bind configuration into experiment identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShadowPolicy {
    pub version: String,
    pub threshold_bps: usize,
    pub minimum_closed_trades: usize,
    pub promote_uncertain: bool,
    pub audit_sample_bps: u16,
    pub seed: u64,
}
impl Default for ShadowPolicy {
    fn default() -> Self {
        Self {
            version: "m1-shadow-promotion-v1".into(),
            threshold_bps: 5000,
            minimum_closed_trades: 20,
            promote_uncertain: true,
            audit_sample_bps: 1000,
            seed: 42,
        }
    }
}
impl ShadowPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.version != "m1-shadow-promotion-v1"
            || self.threshold_bps > 10000
            || !(1..=10000).contains(&self.minimum_closed_trades)
            || !(1..=10000).contains(&self.audit_sample_bps)
        {
            return Err("invalid shadow promotion/audit policy".into());
        }
        Ok(())
    }
    pub fn decision(&self, e: &Estimate, experiment_id: &str) -> Result<serde_json::Value> {
        self.validate()?;
        if e.version != VERSION || e.estimated_net_wins > e.estimated_closed_trades {
            return Err("invalid/version-mismatched candle estimate".into());
        }
        let would_reject = e.status == "estimated"
            && e.estimated_closed_trades >= self.minimum_closed_trades
            && (!self.promote_uncertain || e.uncertainties.is_empty())
            && e.estimated_net_wins as u128 * 10000
                < self.threshold_bps as u128 * e.estimated_closed_trades as u128;
        let identity = serde_json::to_vec(&(self, experiment_id)).map_err(|e| e.to_string())?;
        let digest = blake3::hash(&identity);
        let bucket = u64::from_le_bytes(
            digest.as_bytes()[..8]
                .try_into()
                .map_err(|_| "audit digest width")?,
        ) % 10000;
        Ok(
            serde_json::json!({"version":self.version,"policy":self,"would_reject":would_reject,
            "selected_for_sampled_audit":would_reject && bucket < u64::from(self.audit_sample_bps),
            "execution":"shadow-only-all-candidates-still-replayed", "proven_failure":false}),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Estimate {
    pub version: String,
    pub interpretation: String,
    pub status: String,
    pub estimated_closed_trades: usize,
    pub estimated_net_wins: usize,
    pub estimated_net_micro_usd: i128,
    pub estimated_net_win_rate_bps: Option<usize>,
    pub estimated_sizing_rejections: usize,
    pub unavailable_stop_signals: usize,
    pub overlapping_signals_skipped: usize,
    pub bar_visits: usize,
    pub uncertainties: BTreeSet<String>,
    pub promotes_to_tick: bool,
}
impl Estimate {
    fn new() -> Self {
        Self {
            version: VERSION.into(),
            interpretation: "Hypothetical sequential USD cash model; actual next-quote market entry, adverse OHLC stop proxy, exact requested target, first real deadline quote; whole exit bar reserved. Totals may be partial. Neither candidate bounds nor tick fills; all signaled candidates escalate.".into(),
            status: "estimated".into(), estimated_closed_trades: 0, estimated_net_wins: 0,
            estimated_net_micro_usd: 0, estimated_net_win_rate_bps: None,
            estimated_sizing_rejections: 0, unavailable_stop_signals: 0,
            overlapping_signals_skipped: 0, bar_visits: 0, uncertainties: BTreeSet::new(),
            promotes_to_tick: true,
        }
    }
    fn flag(&mut self, reason: &str) {
        self.uncertainties.insert(reason.into());
    }
}

/// Inputs and masks must come from the same frozen compilation as replay. M1 bars
/// are built from actual quotes only; no final unfinished bucket is manufactured.
pub fn estimate(
    data: &Dataset,
    primary: &[Candle],
    mask: &Mask,
    strategy: &Strategy,
    costs: &Costs,
    window: Option<&crate::validation::Interval>,
) -> Result<Estimate> {
    data.verify()?;
    costs.validate()?;
    mask.validate()?;
    let strategy = strategy.canonical()?;
    if mask.len() != primary.len() {
        return Err("estimate mask length mismatch".into());
    }
    if let Some(w) = window {
        w.validate()?;
    }
    let mut out = Estimate::new();
    if data.profile.quote != "USD" || strategy.entry.is_some() {
        out.status = "unsupported".into();
        out.flag("pending-entry-or-non-USD-conversion-requires-tick-replay");
        return Ok(out);
    }
    let p = &data.profile;
    let bars = candles::build(data, Timeframe::M1)?;
    let visible = window.map_or(primary.len(), |w| {
        primary.partition_point(|c| c.available_ms < w.end_exclusive_ms)
    });
    let distances = crate::stops::prepare(&strategy, p, &primary[..visible])?;
    let slip = costs
        .slippage_pips
        .checked_mul(p.pip)
        .ok_or("estimate slippage overflow")?;
    let mut equity = contracts::INITIAL_EQUITY;
    let mut busy_through: Option<u64> = None;
    for i in mask.set_indices_prefix(visible)? {
        let signal = &primary[i];
        if window.is_some_and(|w| !w.contains(signal.available_ms)) {
            continue;
        }
        let entry_i = data
            .ticks
            .partition_point(|t| t.sequence <= signal.publication_sequence);
        let Some(t) = data
            .ticks
            .get(entry_i)
            .filter(|t| window.is_none_or(|w| w.contains(t.timestamp_ms)))
        else {
            out.flag("no-eligible-entry-quote-in-scope");
            continue;
        };
        if busy_through.is_some_and(|s| t.sequence <= s) {
            out.overlapping_signals_skipped += 1;
            continue;
        }
        let crate::stops::Distance::Ready(distance) = distances.at(i) else {
            out.unavailable_stop_signals += 1;
            continue;
        };
        let (bid, ask) = costs.effective(p, *t)?;
        let entry = crate::orders::market_from_effective(p, costs, bid, ask, strategy.side)?;
        let request = crate::risk::Request {
            equity,
            entry,
            mark: if strategy.side == Side::Long {
                bid
            } else {
                ask
            },
            side: strategy.side,
            stop_with_exit_slip: distance.checked_add(slip).ok_or("estimate risk overflow")?,
            conversion: Conversion::usd(),
        };
        let Ok(reservation) = crate::risk::size(p, costs, request) else {
            out.estimated_sizing_rejections += 1;
            continue;
        };
        let (stop, target) = crate::tick_engine::protective(entry, strategy.side, distance)?;
        let deadline = t
            .timestamp_ms
            .checked_add(contracts::MAX_HOLD_MS)
            .ok_or("estimate deadline overflow")?;
        let deadline_i = data.ticks.partition_point(|q| q.timestamp_ms < deadline);
        let deadline_quote = data
            .ticks
            .get(deadline_i)
            .filter(|q| window.is_none_or(|w| w.contains(q.timestamp_ms)));
        let first = bars.partition_point(|c| c.start_ms.saturating_add(60000) <= t.timestamp_ms);
        let mut exit = None;
        for c in &bars[first..] {
            if window.is_some_and(|w| c.available_ms >= w.end_exclusive_ms) {
                break;
            }
            if c.start_ms >= deadline {
                break;
            }
            if out.bar_visits == MAX_BAR_VISITS {
                out.status = "budget-exhausted-partial".into();
                out.flag("bar-visit-budget");
                return Ok(finish(out));
            }
            out.bar_visits += 1;
            let bounds = crate::screen::barrier_bounds(
                p,
                costs,
                c,
                crate::screen::Position {
                    side: strategy.side,
                    entry_ms: t.timestamp_ms,
                    stop,
                    target,
                    deadline_ms: deadline,
                },
            )?;
            if c.available_ms > deadline {
                out.flag("deadline-partial-bar-prior-barrier-order-unknown");
                if let Some(q) = deadline_quote {
                    let (b, a) = costs.effective(p, *q)?;
                    exit = Some((
                        crate::orders::market_from_effective(
                            p,
                            costs,
                            b,
                            a,
                            if strategy.side == Side::Long {
                                Side::Short
                            } else {
                                Side::Long
                            },
                        )?,
                        q.sequence,
                    ));
                }
                break;
            }
            if bounds.sl_tp_collision {
                out.flag("SL-TP-order-unknown-stop-proxy-selected");
            }
            if bounds.entry_or_deadline_uncertain {
                out.flag("entry-or-deadline-partial-bar");
            }
            let stop_possible = bounds
                .possible
                .contains(&crate::screen::Possible::StopMarket);
            let target_possible = bounds
                .possible
                .contains(&crate::screen::Possible::TargetLimit);
            if !stop_possible && !target_possible {
                continue;
            }
            let price = if stop_possible {
                out.flag("stop-first-quote-unknown-adverse-extremum-proxy");
                if strategy.side == Side::Long {
                    c.bid
                        .low
                        .checked_sub(slip)
                        .ok_or("estimate stop overflow")?
                } else {
                    c.ask
                        .high
                        .max(
                            c.bid
                                .high
                                .checked_add(
                                    costs
                                        .spread_floor_pips
                                        .checked_mul(p.pip)
                                        .ok_or("estimate spread overflow")?,
                                )
                                .ok_or("estimate spread overflow")?,
                        )
                        .checked_add(slip)
                        .ok_or("estimate stop overflow")?
                }
            } else {
                target
            };
            if strategy.side == Side::Short {
                out.flag("ask-extrema-correlation-unknown");
            }
            out.flag("whole-exit-bar-reserved-not-exact-overlap");
            exit = Some((
                price,
                c.publication_sequence
                    .checked_sub(1)
                    .ok_or("estimate sequence underflow")?,
            ));
            break;
        }
        if exit.is_none() {
            // Use a deadline quote only when every pre-deadline bucket was closed in scope.
            // An unfinished bucket could contain a prior barrier hit and cannot be skipped.
            if let Some(q) = deadline_quote.filter(|q| {
                bars.binary_search_by_key(&q.sequence, |c| c.publication_sequence)
                    .is_ok()
            }) {
                let (b, a) = costs.effective(p, *q)?;
                let price = crate::orders::market_from_effective(
                    p,
                    costs,
                    b,
                    a,
                    if strategy.side == Side::Long {
                        Side::Short
                    } else {
                        Side::Long
                    },
                )?;
                exit = Some((price, q.sequence));
            }
        }
        let Some((price, through)) = exit else {
            out.flag("unresolved-position-or-unclosed-coverage");
            out.status = "unresolved-partial".into();
            break;
        };
        if price <= 0 {
            return Err("estimate nonpositive exit under costs".into());
        }
        let movement = price
            .checked_sub(entry)
            .and_then(|v| v.checked_mul(strategy.side.sign()))
            .ok_or("estimate movement overflow")?;
        let net = contracts::pnl(p, movement, reservation.volume, Conversion::usd())?
            .checked_sub(
                costs
                    .commission(reservation.volume)
                    .checked_mul(2)
                    .ok_or("estimate fee overflow")?,
            )
            .ok_or("estimate net overflow")?;
        out.estimated_closed_trades += 1;
        out.estimated_net_wins += usize::from(net > 0);
        out.estimated_net_micro_usd = out
            .estimated_net_micro_usd
            .checked_add(net)
            .ok_or("estimate sum overflow")?;
        equity = equity.checked_add(net).ok_or("estimate equity overflow")?;
        busy_through = Some(through);
    }
    Ok(finish(out))
}
fn finish(mut out: Estimate) -> Estimate {
    out.estimated_net_win_rate_bps =
        (out.estimated_net_wins * 10000).checked_div(out.estimated_closed_trades);
    out
}

pub fn compare(
    estimate: &Estimate,
    replay: &Replay,
    experiment_id: &str,
    policy: &ShadowPolicy,
) -> Result<serde_json::Value> {
    let decision = policy.decision(estimate, experiment_id)?;
    let wins = replay.trades.iter().filter(|t| t.net_micro_usd > 0).count();
    let actual_unresolved = replay.unresolved.is_some() || replay.pending_entry.is_some();
    let false_negative = if decision["would_reject"] == true && !actual_unresolved {
        Some(
            replay.trades.len() >= policy.minimum_closed_trades
                && wins * 10000 >= policy.threshold_bps * replay.trades.len(),
        )
    } else {
        None
    };
    Ok(serde_json::json!({
        "version":"m1-estimate-vs-tick-v1", "estimated":estimate,
        "actual_closed_trades":replay.trades.len(), "actual_net_wins":wins,
        "actual_net_win_rate_bps":if replay.trades.is_empty(){None}else{Some(wins * 10000 / replay.trades.len())},
        "actual_unresolved":actual_unresolved, "shadow_decision":decision,
        "shadow_false_negative":false_negative,
        "candidate_rejected":false, "false_negative_audit":"all-candidates-replayed-sample-selection-diagnostic-only",
        "qualification_evidence":false
    }))
}
