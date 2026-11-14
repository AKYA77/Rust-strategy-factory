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
pub(crate) fn protective(entry: i64, side: Side, distance: i64) -> Result<(i64, i64)> {
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
pub(crate) const FILL_ASSUMPTION: &str = "L1 full-fill research assumption; no historical depth/queue verification; target limit at requested price; deadline cancels protective orders; USD-per-quote marks supplied explicitly";
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
pub(crate) fn signed_delta(side: Side, exit: i64, entry: i64) -> Result<i64> {
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
pub(crate) struct PreparedReplay {
    pub strategy: Strategy,
    pub distances: crate::stops::Prepared,
    pub slip: i64,
    pub visible: usize,
}
pub(crate) fn prepare_replay(
    data: &Dataset,
    candles: &[Candle],
    mask: &Mask,
    strategy: &Strategy,
    costs: &Costs,
    conversions: &[Conversion],
    window: Option<&crate::validation::Interval>,
) -> Result<PreparedReplay> {
    if let Some(window) = window {
        window.validate()?;
    }
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
    mask.validate()?;
    let visible = window.map_or(candles.len(), |w| {
        candles.partition_point(|c| c.available_ms < w.end_exclusive_ms)
    });
    let mut features = crate::strategies::Features::with_context(
        &candles[..visible],
        visible.saturating_mul(128),
        data,
        strategy.timeframe,
    )?;
    let expected = features.signal(&strategy.rule)?;
    if mask.len() != candles.len() || !mask.matches_prefix(&expected, visible)? {
        return Err("signals inconsistent with frozen strategy".into());
    }
    let distances = crate::stops::prepare(&strategy, p, &candles[..visible])?;
    let slip = costs
        .slippage_pips
        .checked_mul(p.pip)
        .ok_or("slippage overflow")?;
    Ok(PreparedReplay {
        strategy,
        distances,
        slip,
        visible,
    })
}
struct EventContext<'a> {
    p: &'a Profile,
    strategy: &'a Strategy,
    distances: &'a crate::stops::Prepared,
    costs: &'a Costs,
    candles: &'a [Candle],
    mask: &'a Mask,
    window: Option<&'a crate::validation::Interval>,
    slip: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccountState {
    balance: i128,
    marked: i128,
    peak: i128,
    drawdown: i128,
    trades: Vec<Trade>,
    rejections: Vec<Rejection>,
    position: Option<OpenPosition>,
    candle_i: usize,
    pending: Option<PendingEntry>,
    entry_receipts: Vec<EntryReceipt>,
}
impl AccountState {
    fn initial() -> Self {
        Self {
            balance: contracts::INITIAL_EQUITY,
            marked: contracts::INITIAL_EQUITY,
            peak: contracts::INITIAL_EQUITY,
            drawdown: 0,
            trades: Vec::new(),
            rejections: Vec::new(),
            position: None,
            candle_i: 0,
            pending: None,
            entry_receipts: Vec::new(),
        }
    }
    fn advance(
        &mut self,
        t: &Tick,
        fx: Conversion,
        previous_ms: Option<i64>,
        context: &EventContext<'_>,
    ) -> Result<()> {
        let EventContext {
            p,
            strategy,
            distances,
            costs,
            candles,
            mask,
            window,
            slip,
        } = *context;
        fx.validate(p, t.timestamp_ms, costs.max_conversion_age_ms)?;
        let (bid, ask) = costs.effective(p, *t)?;
        let mut closed_now = false;
        if let Some(mut pos) = self.position.take() {
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
                self.balance = self
                    .balance
                    .checked_add(gross)
                    .and_then(|v| v.checked_sub(commission))
                    .ok_or("self.balance overflow")?;
                self.trades.push(Trade {
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
                    crossed_gap_ms: previous_ms.map_or(0, |previous| t.timestamp_ms - previous),
                });
                closed_now = true;
            } else {
                self.position = Some(pos);
            }
        }
        if let Some(plan) = &strategy.entry {
            let (_, _, cancel_on_false) = plan.parameters();
            let mut filled_now = false;
            if let Some(mut intent) = self.pending.take() {
                let mut preview = intent.order.clone();
                let candidate = preview.on_quote(p, costs, *t)?;
                if let Some(fill) = candidate {
                    let distance = intent
                        .reservation
                        .stop_distance_price_units
                        .ok_or("legacy self.pending reservation lacks frozen stop distance")?;
                    let request = crate::risk::Request {
                        equity: self.balance,
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
                            self.balance -= reservation.entry_fee_micro_usd;
                            self.position = Some(OpenPosition {
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
                            self.entry_receipts.push(receipt(
                                *t,
                                &intent,
                                EntryStatus::Filled,
                                Some(fill),
                                "fixed quantity accepted by pre-fill self.marked-equity/risk gate",
                            ));
                            filled_now = true;
                        }
                        Err(reason) => {
                            intent.order.cancel(t.timestamp_ms)?;
                            self.entry_receipts.push(receipt(
                                *t,
                                &intent,
                                EntryStatus::RiskRejected,
                                None,
                                &reason,
                            ));
                            self.rejections.push(Rejection {
                                event: t.sequence,
                                reason,
                            });
                        }
                    }
                } else if matches!(preview.state(), crate::orders::State::Expired { .. }) {
                    self.entry_receipts.push(receipt(
                        *t,
                        &intent,
                        EntryStatus::Expired,
                        None,
                        "self.pending entry expired before quote eligibility",
                    ));
                } else {
                    intent.order = preview;
                    self.pending = Some(intent);
                }
            }
            // Existing orders consume the quote first. Newly observed signals cancel only surviving intents.
            if cancel_on_false
                && self.candle_i < candles.len()
                && candles[self.candle_i].publication_sequence == t.sequence
                && !mask.get(self.candle_i)
                && let Some(mut intent) = self.pending.take()
            {
                intent.order.cancel(t.timestamp_ms)?;
                let status = if matches!(intent.order.state(), crate::orders::State::Expired { .. })
                {
                    EntryStatus::Expired
                } else {
                    EntryStatus::Cancelled
                };
                self.entry_receipts.push(receipt(
                    *t,
                    &intent,
                    status,
                    None,
                    "closed signal invalidated self.pending entry",
                ));
            }
            // New orders are placed after existing order processing; they cannot fill on publication.
            while self.candle_i < candles.len()
                && candles[self.candle_i].publication_sequence <= t.sequence
            {
                let distance = distances.at(self.candle_i);
                let wants = mask.get(self.candle_i)
                    && window.is_none_or(|w| w.contains(candles[self.candle_i].available_ms));
                self.candle_i += 1;
                if !wants
                    || self.pending.is_some()
                    || self.position.is_some()
                    || closed_now
                    || filled_now
                {
                    continue;
                }
                let crate::stops::Distance::Ready(distance) = distance else {
                    self.rejections.push(Rejection {
                        event: t.sequence,
                        reason: distance.rejection().into(),
                    });
                    continue;
                };
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
                            equity: self.balance,
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
                        self.entry_receipts.push(receipt(*t,&intent,EntryStatus::Placed,None,"quantity fixed at publication; one self.pending reservation blocks other entries"));
                        self.pending = Some(intent);
                    }
                    Err(reason) => self.rejections.push(Rejection {
                        event: t.sequence,
                        reason,
                    }),
                }
            }
        } else {
            // Market-only v1 concepts retain signal timing; sizing now includes self.marked entry costs.
            while self.candle_i < candles.len()
                && candles[self.candle_i].publication_sequence < t.sequence
            {
                let distance = distances.at(self.candle_i);
                let wants = mask.get(self.candle_i)
                    && window.is_none_or(|w| w.contains(candles[self.candle_i].available_ms));
                self.candle_i += 1;
                if !wants || self.position.is_some() || closed_now {
                    continue;
                }
                let crate::stops::Distance::Ready(distance) = distance else {
                    self.rejections.push(Rejection {
                        event: t.sequence,
                        reason: distance.rejection().into(),
                    });
                    continue;
                };
                let entry =
                    crate::orders::market_from_effective(p, costs, bid, ask, strategy.side)?;
                let sizing = crate::risk::size(
                    p,
                    costs,
                    crate::risk::Request {
                        equity: self.balance,
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
                        self.balance -= r.entry_fee_micro_usd;
                        self.position = Some(OpenPosition {
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
                    Err(reason) => self.rejections.push(Rejection {
                        event: t.sequence,
                        reason,
                    }),
                }
            }
        }
        self.marked = if let Some(pos) = &self.position {
            self.balance
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
                .ok_or("self.marked equity overflow")?
        } else {
            self.balance
        };
        self.peak = self.peak.max(self.marked);
        self.drawdown = self.drawdown.max(self.peak - self.marked);
        Ok(())
    }
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
    let PreparedReplay {
        strategy,
        distances,
        slip,
        ..
    } = prepare_replay(data, candles, mask, strategy, costs, conversions, window)?;
    let mut state = AccountState::initial();
    let context = EventContext {
        p: &data.profile,
        strategy: &strategy,
        distances: &distances,
        costs,
        candles,
        mask,
        window,
        slip,
    };
    for (event_i, t) in data.ticks.iter().enumerate() {
        if let Some(window) = window {
            if t.timestamp_ms >= window.end_exclusive_ms {
                break;
            }
            if t.timestamp_ms < window.start_ms {
                continue;
            }
        }
        state.advance(
            t,
            conversions[event_i],
            event_i.checked_sub(1).map(|i| data.ticks[i].timestamp_ms),
            &context,
        )?;
    }
    let AccountState {
        balance,
        marked,
        drawdown,
        trades,
        rejections,
        position,
        pending,
        entry_receipts,
        ..
    } = state;
    Ok(Replay {
        version: crate::ENGINE_VERSION.into(),
        strategy_id: strategy.identity()?,
        dataset_id: data.manifest.content_hash.clone(),
        profile_id: data.manifest.profile_hash.clone(),
        cost_id: crate::data::hash(costs)?,
        conversion_id: crate::data::hash(&conversions)?,
        fill_assumption: FILL_ASSUMPTION.into(),
        initial_micro_usd: contracts::INITIAL_EQUITY,
        realized_micro_usd: balance,
        final_marked_micro_usd: marked,
        maximum_drawdown_micro_usd: drawdown,
        trades,
        rejections,
        unresolved: position,
        pending_entry: pending,
        entry_receipts,
        evaluation_window: window.cloned(),
    })
}

/// Exact synthetic USD corpus replay using the authoritative event transition.
/// Feature contexts and result receipts have separate explicit caps. Real/protected
/// research requires registry/exposure integration and is deliberately refused.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusBudget {
    pub max_candles: usize,
    pub feature_cells: usize,
    pub max_receipts: usize,
}
/// Freeze all corpus execution inputs before acquiring a registry lease.
pub fn corpus_key(
    base: &std::path::Path,
    spec: &crate::corpus::Specification,
    strategy: &Strategy,
    costs: &Costs,
    budget: CorpusBudget,
) -> Result<crate::storage::ExperimentKey> {
    costs.validate()?;
    if budget.max_candles == 0
        || budget.max_candles > 1_000_000
        || budget.feature_cells == 0
        || budget.max_receipts == 0
        || budget.max_receipts > 1_000_000
    {
        return Err("invalid corpus execution budget".into());
    }
    let observed = crate::corpus::inspect(base, spec)?;
    if observed.declared_class != crate::data::DataClass::SyntheticTestOnly
        || observed.profile.quote != "USD"
    {
        return Err("registered corpus execution requires synthetic literal USD".into());
    }
    let strategy = strategy.canonical()?;
    Ok(crate::storage::ExperimentKey {
        concept_hash: strategy.identity()?,
        instrument: observed.profile.symbol.clone(),
        dataset_hash: observed.ordered_events_hash,
        start_ms: observed.first.timestamp_ms,
        end_exclusive_ms: observed
            .latest
            .timestamp_ms
            .checked_add(1)
            .ok_or("corpus end overflow")?,
        signal_version: crate::SIGNAL_VERSION.into(),
        indicator_version: crate::INDICATOR_VERSION.into(),
        cost_hash: crate::data::hash(costs)?,
        fill_policy: FILL_ASSUMPTION.into(),
        profile_hash: crate::data::hash(&observed.profile)?,
        engine_version: crate::ENGINE_VERSION.into(),
        protocol_hash: "synthetic-corpus-training-v1".into(),
        code_revision: crate::research::compute_revision()?,
        configuration_hash: crate::data::hash(&(
            "corpus-run-v1",
            spec.identity()?,
            strategy,
            costs,
            budget,
            crate::research::compute_revision()?,
        ))?,
        stage: crate::storage::Stage::TickVerification,
    })
}
pub struct CorpusRun<'a> {
    pub base: &'a std::path::Path,
    pub spec: &'a crate::corpus::Specification,
    pub strategy: &'a Strategy,
    pub costs: &'a Costs,
    pub budget: CorpusBudget,
}
struct ResumeControl<'a> {
    registry: &'a mut crate::storage::Registry,
    lease: &'a crate::storage::Lease,
    max_new_chunks: usize,
    clock: &'a mut dyn FnMut() -> i64,
}
/// Resume validation recomputes the saved prefix before accepting stored state.
/// This bounds memory and detects semantic forgery, but is not a fast seek resume.
pub fn advance_corpus(
    registry: &mut crate::storage::Registry,
    lease: &crate::storage::Lease,
    run: CorpusRun<'_>,
    max_new_chunks: usize,
    mut clock: impl FnMut() -> i64,
) -> Result<Option<Replay>> {
    if !(1..=64).contains(&max_new_chunks) {
        return Err("corpus step must process1..64 new chunks".into());
    }
    registry.validate_lease(lease, clock())?;
    let expected = corpus_key(run.base, run.spec, run.strategy, run.costs, run.budget)?;
    if registry.experiment_key(&lease.experiment_id)?.as_ref() != Some(&expected) {
        return Err("registered corpus inputs or build differ".into());
    }
    replay_corpus_internal(
        run,
        Some(ResumeControl {
            registry,
            lease,
            max_new_chunks,
            clock: &mut clock,
        }),
    )
}
pub fn replay_corpus(
    base: &std::path::Path,
    spec: &crate::corpus::Specification,
    strategy: &Strategy,
    costs: &Costs,
    budget: CorpusBudget,
) -> Result<Replay> {
    replay_corpus_internal(
        CorpusRun {
            base,
            spec,
            strategy,
            costs,
            budget,
        },
        None,
    )?
    .ok_or_else(|| "unexpected incomplete corpus replay".into())
}
fn replay_corpus_internal(
    run: CorpusRun<'_>,
    mut control: Option<ResumeControl<'_>>,
) -> Result<Option<Replay>> {
    let CorpusRun {
        base,
        spec,
        strategy,
        costs,
        budget,
    } = run;
    let CorpusBudget {
        max_candles,
        feature_cells,
        max_receipts,
    } = budget;
    costs.validate()?;
    if max_receipts == 0 || max_receipts > 1_000_000 {
        return Err("invalid corpus replay receipt budget".into());
    }
    let observed = crate::corpus::inspect(base, spec)?;
    if observed.declared_class != crate::data::DataClass::SyntheticTestOnly
        || observed.profile.quote != "USD"
    {
        return Err("corpus replay currently requires synthetic literal-USD inputs; real/protected exposure and FX conversion streams are not integrated".into());
    }
    let strategy = strategy.canonical()?;
    let candles = crate::corpus::candle_context(base, spec, strategy.timeframe, max_candles)?;
    let mask = crate::corpus::signals(base, spec, &strategy, max_candles, feature_cells)?;
    let distances = crate::stops::prepare(&strategy, &observed.profile, &candles)?;
    let slip = costs
        .slippage_pips
        .checked_mul(observed.profile.pip)
        .ok_or("slippage overflow")?;
    let context = EventContext {
        p: &observed.profile,
        strategy: &strategy,
        distances: &distances,
        costs,
        candles: &candles,
        mask: &mask,
        window: None,
        slip,
    };
    let saved = if let Some(c) = &control {
        c.registry.replay_checkpoint(&c.lease.experiment_id)?
    } else {
        None
    };
    let old_cursor = saved.as_ref().map_or(0, |(r, _)| r.cursor);
    if old_cursor < 0 || old_cursor as usize > spec.chunks.len() {
        return Err("checkpoint cursor outside corpus".into());
    }
    let stop = control.as_ref().map_or(spec.chunks.len(), |c| {
        (old_cursor as usize)
            .saturating_add(c.max_new_chunks)
            .min(spec.chunks.len())
    });
    let mut events = 0u64;
    let mut boundary = 0usize;
    let mut state = AccountState::initial();
    let mut previous = None;
    let mut conversions = crate::corpus::SequenceHash::new();
    crate::corpus::visit_chunks(base, spec, |index, dataset| {
        if index >= stop {
            return Ok(());
        }
        for tick in &dataset.ticks {
            let fx = Conversion::usd();
            state.advance(tick, fx, previous, &context)?;
            if state
                .trades
                .len()
                .saturating_add(state.rejections.len())
                .saturating_add(state.entry_receipts.len())
                > max_receipts
            {
                return Err("corpus replay receipt budget exceeded".into());
            }
            events += 1;
            conversions.push(&fx)?;
            previous = Some(tick.timestamp_ms);
        }
        boundary = index + 1;
        if boundary == old_cursor as usize {
            let actual = serde_json::json!({"version":"corpus-account-state-v1","corpus":spec.identity()?,"account":state,"previous_ms":previous,"events":events});
            if saved.as_ref().is_none_or(|(r, _)| r.state != actual) {
                return Err("checkpoint account state differs from verified source prefix".into());
            }
        }
        Ok(())
    })?;
    if let Some(c) = &mut control {
        if boundary > old_cursor as usize {
            let payload = serde_json::json!({"version":"corpus-account-state-v1","corpus":spec.identity()?,"account":state,"previous_ms":previous,"events":events});
            c.registry.save_replay_checkpoint(
                c.lease,
                (c.clock)(),
                old_cursor,
                boundary as i64,
                payload,
            )?;
        }
        if boundary < spec.chunks.len() {
            return Ok(None);
        }
    }
    Ok(Some(Replay {
        version: crate::ENGINE_VERSION.into(),
        strategy_id: strategy.identity()?,
        dataset_id: observed.ordered_events_hash,
        profile_id: crate::data::hash(&observed.profile)?,
        cost_id: crate::data::hash(costs)?,
        conversion_id: conversions.finish(),
        fill_assumption: FILL_ASSUMPTION.into(),
        initial_micro_usd: contracts::INITIAL_EQUITY,
        realized_micro_usd: state.balance,
        final_marked_micro_usd: state.marked,
        maximum_drawdown_micro_usd: state.drawdown,
        trades: state.trades,
        rejections: state.rejections,
        unresolved: state.position,
        pending_entry: state.pending,
        entry_receipts: state.entry_receipts,
        evaluation_window: None,
    }))
}

#[derive(Debug, Serialize)]
pub enum CorpusProgress {
    Held {
        experiment_id: String,
        state: String,
    },
    Pending {
        experiment_id: String,
        cursor: i64,
    },
    Busy {
        experiment_id: String,
    },
    Completed {
        experiment_id: String,
        cached: bool,
        result: crate::storage::StoredResult,
    },
}
/// A standalone safe checkpoint: reclaim, validate, advance, then yield or
/// publish through the registry's token-fenced completion transaction.
pub fn execute_corpus_step(
    registry: &mut crate::storage::Registry,
    run: CorpusRun<'_>,
    max_new_chunks: usize,
    lease_ms: i64,
    mut clock: impl FnMut() -> i64,
) -> Result<CorpusProgress> {
    let key = corpus_key(run.base, run.spec, run.strategy, run.costs, run.budget)?;
    let id = registry.register(&key)?;
    let control = registry.corpus_control(&id)?;
    if control != "running" {
        return Ok(CorpusProgress::Held {
            experiment_id: id,
            state: control,
        });
    }
    let lease = match registry.claim(&id, clock(), lease_ms)? {
        crate::storage::Claim::Busy => return Ok(CorpusProgress::Busy { experiment_id: id }),
        crate::storage::Claim::Reused(result) => {
            return Ok(CorpusProgress::Completed {
                experiment_id: id,
                cached: true,
                result,
            });
        }
        crate::storage::Claim::Acquired(lease) => lease,
    };
    let corpus_id = run.spec.identity()?;
    match advance_corpus(registry, &lease, run, max_new_chunks, &mut clock)? {
        None => {
            let cursor = registry
                .replay_checkpoint(&id)?
                .ok_or("missing committed corpus checkpoint")?
                .0
                .cursor;
            registry.yield_lease(&lease, clock())?;
            Ok(CorpusProgress::Pending {
                experiment_id: id,
                cursor,
            })
        }
        Some(replay) => {
            let control = registry.corpus_control(&id)?;
            if control != "running" {
                registry.yield_lease(&lease, clock())?;
                return Ok(CorpusProgress::Held {
                    experiment_id: id,
                    state: control,
                });
            }
            let result = serde_json::json!({"kind":"synthetic-corpus-replay-v1","corpus_identity":corpus_id,"tick_replay":replay});
            registry.finish(&lease, clock(), crate::storage::Outcome::Completed, &result)?;
            let result = registry
                .result(&id)?
                .ok_or("completed corpus receipt missing")?;
            Ok(CorpusProgress::Completed {
                experiment_id: id,
                cached: false,
                result,
            })
        }
    }
}

#[cfg(test)]
mod account_checkpoint_tests {
    use super::*;
    #[test]
    fn serialized_account_boundaries_preserve_complete_state() {
        use crate::strategies::{EntryPlan, Features, Rule};
        let mut resolved = 0;
        for symbol in ["EURUSD", "XAUUSD"] {
            let data = crate::data::synthetic_fixture(symbol).unwrap();
            for side in [Side::Long, Side::Short] {
                for entry in [
                    None,
                    Some(EntryPlan::StopMarket {
                        offset_ticks: 2,
                        ttl_ms: 120000,
                        cancel_on_signal_false: true,
                    }),
                    Some(EntryPlan::Limit {
                        offset_ticks: 2,
                        ttl_ms: 120000,
                        cancel_on_signal_false: true,
                    }),
                ] {
                    let strategy = Strategy {
                        timeframe: crate::candles::Timeframe::M1,
                        side,
                        stop_ticks: 100,
                        rule: Rule::AboveMean { window: 2 },
                        entry,
                        stop_policy: None,
                    };
                    let costs = Costs::default();
                    let candles = crate::candles::build(&data, strategy.timeframe).unwrap();
                    let mask = Features::new(&candles, 100000)
                        .unwrap()
                        .signal(&strategy.rule)
                        .unwrap();
                    let conversions = vec![Conversion::usd(); data.ticks.len()];
                    let prepared = prepare_replay(
                        &data,
                        &candles,
                        &mask,
                        &strategy,
                        &costs,
                        &conversions,
                        None,
                    )
                    .unwrap();
                    let context = EventContext {
                        p: &data.profile,
                        strategy: &prepared.strategy,
                        distances: &prepared.distances,
                        costs: &costs,
                        candles: &candles,
                        mask: &mask,
                        window: None,
                        slip: prepared.slip,
                    };
                    let mut reference = AccountState::initial();
                    for (i, t) in data.ticks.iter().enumerate() {
                        reference
                            .advance(
                                t,
                                conversions[i],
                                i.checked_sub(1).map(|i| data.ticks[i].timestamp_ms),
                                &context,
                            )
                            .unwrap();
                    }
                    resolved += reference.trades.len();
                    for chunk in [17, 127] {
                        let mut restored = AccountState::initial();
                        for (i, t) in data.ticks.iter().enumerate() {
                            restored
                                .advance(
                                    t,
                                    conversions[i],
                                    i.checked_sub(1).map(|i| data.ticks[i].timestamp_ms),
                                    &context,
                                )
                                .unwrap();
                            if (i + 1) % chunk == 0 {
                                restored =
                                    serde_json::from_slice(&serde_json::to_vec(&restored).unwrap())
                                        .unwrap();
                            }
                        }
                        assert_eq!(
                            crate::data::hash(&restored).unwrap(),
                            crate::data::hash(&reference).unwrap()
                        );
                    }
                }
            }
        }
        assert!(resolved > 0);
    }
}
