//! Authoritative ordered L1 replay, simplified full-fill model. No liquidity/queue claims.
use crate::{
    Result,
    candles::Candle,
    contracts::{self, Conversion, MAX_HOLD_MS, Profile, Side},
    data::{Dataset, Tick},
    indicators::Mask,
    strategies::Strategy,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Costs {
    pub version: String,
    pub spread_floor_pips: i64,
    pub slippage_pips: i64,
    pub commission_micro_usd_per_lot_per_fill: i64,
    pub max_conversion_age_ms: i64,
}
impl Default for Costs {
    fn default() -> Self {
        Self {
            version: "bid-anchored-floor-v1".into(),
            spread_floor_pips: 2,
            slippage_pips: 2,
            commission_micro_usd_per_lot_per_fill: 0,
            max_conversion_age_ms: 60_000,
        }
    }
}
impl Costs {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.version != "bid-anchored-floor-v1"
            || self.spread_floor_pips < 2
            || self.slippage_pips < 2
            || self.spread_floor_pips > 10_000
            || self.slippage_pips > 10_000
            || self.commission_micro_usd_per_lot_per_fill < 0
            || self.max_conversion_age_ms < 0
        {
            return Err("invalid or unsupported cost contract".into());
        }
        Ok(())
    }
    /// Keep recorded bid; widen ask upward, with exact outward grid rounding.
    pub fn effective(&self, p: &Profile, tick: Tick) -> Result<(i64, i64)> {
        p.validate()?;
        self.validate()?;
        tick.validate(p)?;
        let floor = self
            .spread_floor_pips
            .checked_mul(p.pip)
            .ok_or("spread overflow")?;
        let ask = tick
            .ask
            .max(tick.bid.checked_add(floor).ok_or("spread overflow")?);
        let ask = ask
            .checked_add((p.tick - ask % p.tick) % p.tick)
            .ok_or("spread rounding overflow")?;
        Ok((tick.bid, ask))
    }
    pub(crate) fn commission(&self, volume: i64) -> i128 {
        (i128::from(self.commission_micro_usd_per_lot_per_fill) * i128::from(volume) + 999_999)
            / 1_000_000
    }
}
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExitReason {
    StopMarket,
    TargetLimit,
    DeadlineMarket,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trade {
    pub entry_event: u64,
    pub exit_event: u64,
    pub entry_ms: i64,
    pub exit_ms: i64,
    pub side: Side,
    pub entry: i64,
    pub exit: i64,
    pub stop: i64,
    pub target: i64,
    pub volume_micro_lots: i64,
    pub net_micro_usd: i128,
    pub commission_micro_usd: i128,
    pub reason: ExitReason,
    pub mfe_price_units: i64,
    pub mae_price_units: i64,
    pub crossed_gap_ms: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenPosition {
    pub entry_event: u64,
    pub entry_ms: i64,
    pub deadline_ms: i64,
    pub side: Side,
    pub entry: i64,
    pub stop: i64,
    pub target: i64,
    pub volume_micro_lots: i64,
    pub entry_commission: i128,
    pub mfe: i64,
    pub mae: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rejection {
    pub event: u64,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingEntry {
    pub order: crate::orders::Order,
    pub signal_event: u64,
    pub planned_entry: i64,
    pub kind: crate::orders::Kind,
    pub reservation: crate::risk::Reservation,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryStatus {
    Placed,
    Filled,
    Expired,
    Cancelled,
    RiskRejected,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryReceipt {
    pub event: u64,
    pub timestamp_ms: i64,
    pub signal_event: u64,
    pub status: EntryStatus,
    pub kind: crate::orders::Kind,
    pub reservation: crate::risk::Reservation,
    pub fill: Option<crate::orders::Fill>,
    pub reason: String,
}
fn receipt(
    t: Tick,
    pending: &PendingEntry,
    status: EntryStatus,
    fill: Option<crate::orders::Fill>,
    reason: &str,
) -> EntryReceipt {
    EntryReceipt {
        event: t.sequence,
        timestamp_ms: t.timestamp_ms,
        signal_event: pending.signal_event,
        status,
        kind: pending.kind,
        reservation: pending.reservation,
        fill,
        reason: reason.into(),
    }
}
fn protective(entry: i64, side: Side, distance: i64) -> Result<(i64, i64)> {
    let stop = entry
        .checked_sub(side.sign().checked_mul(distance).ok_or("stop overflow")?)
        .ok_or("stop overflow")?;
    let target = entry
        .checked_add(
            side.sign()
                .checked_mul(distance.checked_mul(3).ok_or("target overflow")?)
                .ok_or("target overflow")?,
        )
        .ok_or("target overflow")?;
    if stop <= 0 || target <= 0 {
        return Err("nonpositive protective level".into());
    }
    Ok((stop, target))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replay {
    pub version: String,
    pub strategy_id: String,
    pub dataset_id: String,
    pub profile_id: String,
    pub cost_id: String,
    pub conversion_id: String,
    pub fill_assumption: String,
    pub initial_micro_usd: i128,
    pub realized_micro_usd: i128,
    pub final_marked_micro_usd: i128,
    pub maximum_drawdown_micro_usd: i128,
    pub trades: Vec<Trade>,
    pub rejections: Vec<Rejection>,
    pub unresolved: Option<OpenPosition>,
    pub pending_entry: Option<PendingEntry>,
    pub entry_receipts: Vec<EntryReceipt>,
    pub evaluation_window: Option<crate::validation::Interval>,
}
fn signed_delta(side: Side, exit: i64, entry: i64) -> Result<i64> {
    exit.checked_sub(entry)
        .and_then(|d| d.checked_mul(side.sign()))
        .ok_or("price delta overflow".into())
}
pub fn replay(
    data: &Dataset,
    candles: &[Candle],
    mask: &Mask,
    strategy: &Strategy,
    costs: &Costs,
    conversions: &[Conversion],
) -> Result<Replay> {
    replay_internal(data, candles, mask, strategy, costs, conversions, None)
}
/// Warmup is causal and read-only. All positions start flat; no events at/after end are consumed.
pub fn replay_window(
    data: &Dataset,
    candles: &[Candle],
    mask: &Mask,
    strategy: &Strategy,
    costs: &Costs,
    conversions: &[Conversion],
    window: &crate::validation::Interval,
) -> Result<Replay> {
    window.validate()?;
    replay_internal(
        data,
        candles,
        mask,
        strategy,
        costs,
        conversions,
        Some(window),
    )
}
fn replay_internal(
    data: &Dataset,
    candles: &[Candle],
    mask: &Mask,
    strategy: &Strategy,
    costs: &Costs,
    conversions: &[Conversion],
    window: Option<&crate::validation::Interval>,
) -> Result<Replay> {
    data.verify()?;
    costs.validate()?;
    let strategy = strategy.canonical()?;
    let p = &data.profile;
    if conversions.len() != data.ticks.len() {
        return Err("one explicit conversion observation required per event".into());
    }
    // Callers cannot inject future-bearing candles or arbitrary signal vectors.
    if candles != crate::candles::build(data, strategy.timeframe)? {
        return Err("candles inconsistent with dataset/timeframe".into());
    }
    let mut features = crate::strategies::Features::with_context(
        candles,
        candles.len().saturating_mul(128),
        data,
        strategy.timeframe,
    )?;
    if *mask != features.signal(&strategy.rule)? {
        return Err("signals inconsistent with frozen strategy".into());
    }
    let distance = strategy
        .stop_ticks
        .checked_mul(p.tick)
        .ok_or("stop overflow")?;
    let slip = costs
        .slippage_pips
        .checked_mul(p.pip)
        .ok_or("slippage overflow")?;
    if strategy.stop_ticks < p.min_stop_ticks {
        return Err("stop below profile minimum".into());
    }
    let mut balance = contracts::INITIAL_EQUITY;
    let mut marked = balance;
    let mut peak = balance;
    let mut drawdown = 0;
    let mut trades = Vec::new();
    let mut rejections = Vec::new();
    let mut position: Option<OpenPosition> = None;
    let mut candle_i = 0;
    let mut pending: Option<PendingEntry> = None;
    let mut entry_receipts = Vec::new();
    for (event_i, t) in data.ticks.iter().enumerate() {
        if let Some(window) = window {
            if t.timestamp_ms >= window.end_exclusive_ms {
                break;
            }
            if t.timestamp_ms < window.start_ms {
                continue;
            }
        }
        let fx = conversions[event_i];
        fx.validate(p, t.timestamp_ms, costs.max_conversion_age_ms)?;
        let (bid, ask) = costs.effective(p, *t)?;
        let mut closed_now = false;
        if let Some(mut pos) = position.take() {
            let mark = if pos.side == Side::Long { bid } else { ask };
            let movement = signed_delta(pos.side, mark, pos.entry)?;
            pos.mfe = pos.mfe.max(movement);
            pos.mae = pos.mae.min(movement);
            let stop_hit = if pos.side == Side::Long {
                bid <= pos.stop
            } else {
                ask >= pos.stop
            };
            let target_hit = if pos.side == Side::Long {
                bid >= pos.target
            } else {
                ask <= pos.target
            };
            // Expiration first: a resting target is cancelled at the deadline before processing the quote.
            let reason = if t.timestamp_ms >= pos.deadline_ms {
                Some(ExitReason::DeadlineMarket)
            } else if stop_hit {
                Some(ExitReason::StopMarket)
            } else if target_hit {
                Some(ExitReason::TargetLimit)
            } else {
                None
            };
            if let Some(reason) = reason {
                let exit = if reason == ExitReason::TargetLimit {
                    pos.target
                } else {
                    crate::orders::market_from_effective(
                        p,
                        costs,
                        bid,
                        ask,
                        if pos.side == Side::Long {
                            Side::Short
                        } else {
                            Side::Long
                        },
                    )?
                };
                if exit <= 0 {
                    return Err("nonpositive exit under cost model".into());
                }
                let commission = costs.commission(pos.volume_micro_lots);
                let gross = contracts::pnl(
                    p,
                    signed_delta(pos.side, exit, pos.entry)?,
                    pos.volume_micro_lots,
                    fx,
                )?;
                balance = balance
                    .checked_add(gross)
                    .and_then(|v| v.checked_sub(commission))
                    .ok_or("balance overflow")?;
                trades.push(Trade {
                    entry_event: pos.entry_event,
                    exit_event: t.sequence,
                    entry_ms: pos.entry_ms,
                    exit_ms: t.timestamp_ms,
                    side: pos.side,
                    entry: pos.entry,
                    exit,
                    stop: pos.stop,
                    target: pos.target,
                    volume_micro_lots: pos.volume_micro_lots,
                    net_micro_usd: gross - pos.entry_commission - commission,
                    commission_micro_usd: pos.entry_commission + commission,
                    reason,
                    mfe_price_units: pos.mfe,
                    mae_price_units: pos.mae,
                    crossed_gap_ms: if event_i == 0 {
                        0
                    } else {
                        t.timestamp_ms - data.ticks[event_i - 1].timestamp_ms
                    },
                });
                closed_now = true;
            } else {
                position = Some(pos);
            }
        }
        if let Some(plan) = &strategy.entry {
            let (_, _, cancel_on_false) = plan.parameters();
            let mut filled_now = false;
            if let Some(mut intent) = pending.take() {
                let mut preview = intent.order.clone();
                let candidate = preview.on_quote(p, costs, *t)?;
                if let Some(fill) = candidate {
                    let request = crate::risk::Request {
                        equity: balance,
                        entry: fill.price,
                        mark: if strategy.side == Side::Long {
                            bid
                        } else {
                            ask
                        },
                        side: strategy.side,
                        stop_with_exit_slip: distance.checked_add(slip).ok_or("risk overflow")?,
                        conversion: fx,
                    };
                    let accepted = crate::risk::reserve(p, costs, request, fill.volume_micro_lots)
                        .and_then(|r| Ok((r, protective(fill.price, strategy.side, distance)?)));
                    match accepted {
                        Ok((reservation, (stop, target))) => {
                            intent.reservation = reservation;
                            balance -= reservation.entry_fee_micro_usd;
                            position = Some(OpenPosition {
                                entry_event: t.sequence,
                                entry_ms: t.timestamp_ms,
                                deadline_ms: t
                                    .timestamp_ms
                                    .checked_add(MAX_HOLD_MS)
                                    .ok_or("deadline overflow")?,
                                side: strategy.side,
                                entry: fill.price,
                                stop,
                                target,
                                volume_micro_lots: reservation.volume,
                                entry_commission: reservation.entry_fee_micro_usd,
                                mfe: 0,
                                mae: 0,
                            });
                            entry_receipts.push(receipt(
                                *t,
                                &intent,
                                EntryStatus::Filled,
                                Some(fill),
                                "fixed quantity accepted by pre-fill marked-equity/risk gate",
                            ));
                            filled_now = true;
                        }
                        Err(reason) => {
                            intent.order.cancel(t.timestamp_ms)?;
                            entry_receipts.push(receipt(
                                *t,
                                &intent,
                                EntryStatus::RiskRejected,
                                None,
                                &reason,
                            ));
                            rejections.push(Rejection {
                                event: t.sequence,
                                reason,
                            });
                        }
                    }
                } else if matches!(preview.state(), crate::orders::State::Expired { .. }) {
                    entry_receipts.push(receipt(
                        *t,
                        &intent,
                        EntryStatus::Expired,
                        None,
                        "pending entry expired before quote eligibility",
                    ));
                } else {
                    intent.order = preview;
                    pending = Some(intent);
                }
            }
            // Existing orders consume the quote first. Newly observed signals cancel only surviving intents.
            if cancel_on_false
                && candle_i < candles.len()
                && candles[candle_i].publication_sequence == t.sequence
                && !mask.get(candle_i)
                && let Some(mut intent) = pending.take()
            {
                intent.order.cancel(t.timestamp_ms)?;
                let status = if matches!(intent.order.state(), crate::orders::State::Expired { .. })
                {
                    EntryStatus::Expired
                } else {
                    EntryStatus::Cancelled
                };
                entry_receipts.push(receipt(
                    *t,
                    &intent,
                    status,
                    None,
                    "closed signal invalidated pending entry",
                ));
            }
            // New orders are placed after existing order processing; they cannot fill on publication.
            while candle_i < candles.len() && candles[candle_i].publication_sequence <= t.sequence {
                let wants = mask.get(candle_i)
                    && window.is_none_or(|w| w.contains(candles[candle_i].available_ms));
                candle_i += 1;
                if !wants || pending.is_some() || position.is_some() || closed_now || filled_now {
                    continue;
                }
                let placement = || -> Result<PendingEntry> {
                    let (offset_ticks, ttl, _) = plan.parameters();
                    let offset = offset_ticks
                        .checked_mul(p.tick)
                        .ok_or("entry offset overflow")?;
                    let anchor = if strategy.side == Side::Long {
                        ask
                    } else {
                        bid
                    };
                    let sign = if matches!(plan, crate::strategies::EntryPlan::StopMarket { .. }) {
                        strategy.side.sign()
                    } else {
                        -strategy.side.sign()
                    };
                    let level = anchor
                        .checked_add(sign.checked_mul(offset).ok_or("entry offset overflow")?)
                        .ok_or("entry price overflow")?;
                    let is_stop = matches!(plan, crate::strategies::EntryPlan::StopMarket { .. });
                    let kind = if is_stop {
                        crate::orders::Kind::StopMarket { trigger: level }
                    } else {
                        crate::orders::Kind::Limit { limit: level }
                    };
                    let entry = level
                        .checked_add(if is_stop {
                            strategy
                                .side
                                .sign()
                                .checked_mul(slip)
                                .ok_or("entry slippage overflow")?
                        } else {
                            0
                        })
                        .ok_or("entry price overflow")?;
                    protective(entry, strategy.side, distance)?;
                    let friction = ask
                        .checked_sub(bid)
                        .and_then(|s| s.checked_add(if is_stop { slip } else { 0 }))
                        .ok_or("friction overflow")?;
                    let mark = entry
                        .checked_sub(
                            strategy
                                .side
                                .sign()
                                .checked_mul(friction)
                                .ok_or("mark overflow")?,
                        )
                        .ok_or("mark overflow")?;
                    let reservation = crate::risk::size(
                        p,
                        costs,
                        crate::risk::Request {
                            equity: balance,
                            entry,
                            mark,
                            side: strategy.side,
                            stop_with_exit_slip: distance
                                .checked_add(slip)
                                .ok_or("risk overflow")?,
                            conversion: fx,
                        },
                    )?;
                    let order = crate::orders::Order::submit(
                        p,
                        costs,
                        strategy.side,
                        kind,
                        reservation.volume,
                        *t,
                        t.timestamp_ms
                            .checked_add(ttl)
                            .ok_or("entry expiry overflow")?,
                    )?;
                    Ok(PendingEntry {
                        order,
                        signal_event: t.sequence,
                        planned_entry: entry,
                        kind,
                        reservation,
                    })
                };
                match placement() {
                    Ok(intent) => {
                        entry_receipts.push(receipt(*t,&intent,EntryStatus::Placed,None,"quantity fixed at publication; one pending reservation blocks other entries"));
                        pending = Some(intent);
                    }
                    Err(reason) => rejections.push(Rejection {
                        event: t.sequence,
                        reason,
                    }),
                }
            }
        } else {
            // Market-only v1 concepts retain signal timing; sizing now includes marked entry costs.
            while candle_i < candles.len() && candles[candle_i].publication_sequence < t.sequence {
                let wants = mask.get(candle_i)
                    && window.is_none_or(|w| w.contains(candles[candle_i].available_ms));
                candle_i += 1;
                if !wants || position.is_some() || closed_now {
                    continue;
                }
                let entry =
                    crate::orders::market_from_effective(p, costs, bid, ask, strategy.side)?;
                let sizing = crate::risk::size(
                    p,
                    costs,
                    crate::risk::Request {
                        equity: balance,
                        entry,
                        mark: if strategy.side == Side::Long {
                            bid
                        } else {
                            ask
                        },
                        side: strategy.side,
                        stop_with_exit_slip: distance.checked_add(slip).ok_or("risk overflow")?,
                        conversion: fx,
                    },
                );
                match sizing.and_then(|r| Ok((r, protective(entry, strategy.side, distance)?))) {
                    Ok((r, (stop, target))) => {
                        balance -= r.entry_fee_micro_usd;
                        position = Some(OpenPosition {
                            entry_event: t.sequence,
                            entry_ms: t.timestamp_ms,
                            deadline_ms: t
                                .timestamp_ms
                                .checked_add(MAX_HOLD_MS)
                                .ok_or("deadline overflow")?,
                            side: strategy.side,
                            entry,
                            stop,
                            target,
                            volume_micro_lots: r.volume,
                            entry_commission: r.entry_fee_micro_usd,
                            mfe: 0,
                            mae: 0,
                        });
                    }
                    Err(reason) => rejections.push(Rejection {
                        event: t.sequence,
                        reason,
                    }),
                }
            }
        }
        marked = if let Some(pos) = &position {
            balance
                .checked_add(contracts::pnl(
                    p,
                    signed_delta(
                        pos.side,
                        if pos.side == Side::Long { bid } else { ask },
                        pos.entry,
                    )?,
                    pos.volume_micro_lots,
                    fx,
                )?)
                .ok_or("marked equity overflow")?
        } else {
            balance
        };
        peak = peak.max(marked);
        drawdown = drawdown.max(peak - marked);
    }
    Ok(Replay { version: crate::ENGINE_VERSION.into(), strategy_id: strategy.identity()?, dataset_id: data.manifest.content_hash.clone(),
        profile_id: data.manifest.profile_hash.clone(), cost_id: crate::data::hash(costs)?,
        conversion_id: crate::data::hash(&conversions)?,
        fill_assumption: "L1 full-fill research assumption; no historical depth/queue verification; target limit at requested price; deadline cancels protective orders; USD-per-quote marks supplied explicitly".into(),
        initial_micro_usd: contracts::INITIAL_EQUITY, realized_micro_usd: balance, final_marked_micro_usd: marked,
        maximum_drawdown_micro_usd: drawdown, trades, rejections, unresolved: position, pending_entry: pending, entry_receipts, evaluation_window:window.cloned() })
}
