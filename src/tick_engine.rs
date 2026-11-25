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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
                    // Stop-market orders rest on the executable side, away from the
                    // market: Long stop = ask + offset, Short stop = bid - offset.
                    let level = anchor
                        .checked_add(
                            strategy
                                .side
                                .sign()
                                .checked_mul(offset)
                                .ok_or("entry offset overflow")?,
                        )
                        .ok_or("entry price overflow")?;
                    let kind = crate::orders::Kind::StopMarket { trigger: level };
                    let entry = level
                        .checked_add(
                            strategy
                                .side
                                .sign()
                                .checked_mul(slip)
                                .ok_or("entry slippage overflow")?,
                        )
                        .ok_or("entry price overflow")?;
                    protective(entry, strategy.side, distance)?;
                    let friction = ask
                        .checked_sub(bid)
                        .and_then(|s| s.checked_add(slip))
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
/// Selects the input-class contract for corpus replay.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplayMode {
    /// Registered research replay: synthetic literal-USD inputs only.
    Registered,
    /// Explicitly non-eligible diagnostic simulation over real structurally-valid corpora.
    /// USD-quoted instruments use the identity conversion; USD-base non-USD-quote instruments
    /// use a direction-based reciprocal closing rate derived from their own quote. Runs the
    /// identical engine and accounting as `Registered`; the result must never be presented as
    /// research evidence, eligibility or qualification.
    Diagnostic,
}
pub(crate) fn corpus_configuration_hash(
    source_identity: &str,
    strategy: &Strategy,
    costs: &Costs,
    budget: CorpusBudget,
    revision: &str,
) -> Result<String> {
    crate::data::hash(&(
        "corpus-run-v1",
        source_identity,
        strategy.canonical()?,
        costs,
        budget,
        revision,
    ))
}
/// Freeze all corpus execution inputs before acquiring a registry lease.
pub fn corpus_key(
    base: &std::path::Path,
    spec: &crate::corpus::Specification,
    strategy: &Strategy,
    costs: &Costs,
    budget: CorpusBudget,
) -> Result<crate::storage::ExperimentKey> {
    let observed = crate::corpus::source_facts(base, spec)?;
    corpus_key_from_facts(spec, strategy, costs, budget, &observed)
}
fn corpus_key_from_facts(
    spec: &crate::corpus::Specification,
    strategy: &Strategy,
    costs: &Costs,
    budget: CorpusBudget,
    observed: &crate::corpus::SourceFacts,
) -> Result<crate::storage::ExperimentKey> {
    if observed.corpus_identity() != spec.identity()? {
        return Err("source key inventory differs from specification".into());
    }
    costs.validate()?;
    if budget.max_candles == 0
        || budget.max_candles > crate::corpus::MAX_RETAINED_CANDLES
        || budget.feature_cells == 0
        || budget.max_receipts == 0
        || budget.max_receipts > 1_000_000
    {
        return Err("invalid corpus execution budget".into());
    }
    // Registration identity keys are reserved for the frozen synthetic literal-USD
    // corpus-step contract. Real structurally-valid data reaches research through
    // `ReplayMode::Registered` (see `replay_corpus`), not through this key path.
    if observed.declared_class() != crate::data::DataClass::SyntheticTestOnly
        || observed.profile().quote != "USD"
    {
        return Err("registered corpus execution requires synthetic literal USD".into());
    }
    let strategy = strategy.canonical()?;
    Ok(crate::storage::ExperimentKey {
        concept_hash: strategy.identity()?,
        instrument: observed.profile().symbol.clone(),
        dataset_hash: observed.ordered_events_hash().to_string(),
        start_ms: observed.first().timestamp_ms,
        end_exclusive_ms: observed
            .latest()
            .timestamp_ms
            .checked_add(1)
            .ok_or("corpus end overflow")?,
        signal_version: crate::SIGNAL_VERSION.into(),
        indicator_version: crate::INDICATOR_VERSION.into(),
        cost_hash: crate::data::hash(costs)?,
        fill_policy: FILL_ASSUMPTION.into(),
        profile_hash: crate::data::hash(&observed.profile())?,
        engine_version: crate::ENGINE_VERSION.into(),
        protocol_hash: "synthetic-corpus-training-v1".into(),
        code_revision: crate::research::compute_revision()?,
        configuration_hash: corpus_configuration_hash(
            &spec.identity()?,
            &strategy,
            costs,
            budget,
            &crate::research::compute_revision()?,
        )?,
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
// Private process-local authority: never constructed from stored JSON. One slot per worker.
struct TrustedCheckpoint {
    experiment_id: String,
    source_binding: String,
    cursor: usize,
    checkpoint_hash: String,
    state: AccountState,
    previous: Option<i64>,
    events: u64,
    conversions: crate::corpus::SequenceHash,
}
#[derive(Default)]
struct CheckpointReuse {
    checkpoint: Option<TrustedCheckpoint>,
    advanced_events: u64,
    skipped_events: u64,
    decoded_events: u64,
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
        None,
        None,
        ReplayMode::Registered,
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
        None,
        None,
        ReplayMode::Registered,
    )?
    .ok_or_else(|| "unexpected incomplete corpus replay".into())
}
/// Diagnostic-only corpus replay over real structurally-valid inputs.
///
/// This is the same sequential engine and accounting as [`replay_corpus`]; the differences are
/// the accepted input class and the USD-account conversion semantics. It exists so the converted
/// historical corpus can be exercised end-to-end for diagnosis. The result is **not** research
/// evidence: it must never be registered as an experiment, presented as eligibility, or used for
/// qualification. USD-quoted instruments use the identity conversion; USD-base non-USD-quote
/// instruments use the direction-based reciprocal closing rate derived from their own quote
/// (long -> bid, short -> ask). Cross-currency conversion streams and eligibility-bearing data
/// classes are refused.
pub(crate) fn replay_corpus_diagnostic(
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
        None,
        None,
        ReplayMode::Diagnostic,
    )?
    .ok_or_else(|| "unexpected incomplete corpus replay".into())
}
/// Diagnostic USD-account conversion for one corpus event.
///
/// USD-quoted instruments use the identity conversion. USD-base non-USD-quote instruments use
/// the direction-based reciprocal closing rate derived from the instrument's own quote: the bid
/// for a long and the ask for a short. This is an explicit, unverified research convention, not
/// an approved broker conversion rule.
fn diagnostic_conversion(p: &Profile, tick: &Tick, side: Side) -> Result<Conversion> {
    if p.quote == "USD" {
        return Ok(Conversion::usd());
    }
    let price = match side {
        Side::Long => tick.bid,
        Side::Short => tick.ask,
    };
    Conversion::usd_account_from_usd_base_price(p, price, tick.timestamp_ms)
}
fn replay_corpus_internal(
    run: CorpusRun<'_>,
    mut control: Option<ResumeControl<'_>>,
    prepared: Option<&crate::corpus::Prepared>,
    mut reuse: Option<&mut CheckpointReuse>,
    mode: ReplayMode,
) -> Result<Option<Replay>> {
    let source_binding = if reuse.is_some() {
        Some(corpus_source_binding(&run)?)
    } else {
        None
    };
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
    let strategy = strategy.canonical()?;
    let owned;
    let prepared = match prepared {
        Some(prepared) => prepared,
        None => {
            owned = crate::corpus::prepare(base, spec, strategy.timeframe, max_candles)?;
            &owned
        }
    };
    if prepared.retained_candles() > max_candles
        || prepared.report().corpus_identity != spec.identity()?
    {
        return Err("prepared corpus budget or identity differs".into());
    }
    let observed = prepared.report();
    match mode {
        ReplayMode::Registered => {
            let allowed_classes = [
                crate::data::DataClass::SyntheticTestOnly,
                crate::data::DataClass::RealStructurallyValid,
                crate::data::DataClass::RealQualityAccepted,
                crate::data::DataClass::RealProfileApproved,
            ];
            if !allowed_classes.contains(&observed.declared_class)
                || (observed.profile.quote != "USD" && observed.profile.base != "USD")
            {
                return Err("corpus replay requires synthetic literal-USD, real structurally-valid USD-quote, or USD-base non-USD-quote inputs".into());
            }
        }
        ReplayMode::Diagnostic => {
            if observed.declared_class != crate::data::DataClass::RealStructurallyValid {
                return Err("diagnostic corpus replay supports only real structurally-valid inputs; eligibility-bearing classes, protected exposure and quarantined data are refused".into());
            }
            if observed.profile.quote != "USD" && observed.profile.base != "USD" {
                return Err("diagnostic corpus replay supports USD-quote instruments and USD-base instruments with a USD-account reciprocal conversion; cross-currency conversion is unavailable".into());
            }
        }
    }
    let candles = prepared.candles(strategy.timeframe)?;
    let mask = prepared.signals(&strategy, feature_cells)?;
    let distances = crate::stops::prepare(&strategy, &observed.profile, candles)?;
    let slip = costs
        .slippage_pips
        .checked_mul(observed.profile.pip)
        .ok_or("slippage overflow")?;
    let context = EventContext {
        p: &observed.profile,
        strategy: &strategy,
        distances: &distances,
        costs,
        candles,
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
    let trusted = reuse
        .as_mut()
        .and_then(|r| r.checkpoint.take())
        .filter(|t| {
            saved.as_ref().is_some_and(|(record, hash)| {
                t.cursor == old_cursor as usize
                    && &t.checkpoint_hash == hash
                    && t.experiment_id == record.experiment_id
                    && source_binding.as_ref() == Some(&t.source_binding)
            })
        });
    let resumed = trusted.is_some();
    let (mut events, mut boundary, mut state, mut previous, mut conversions) = match trusted {
        Some(t) => {
            if let Some(r) = &mut reuse {
                r.skipped_events = r
                    .skipped_events
                    .checked_add(t.events)
                    .ok_or("checkpoint reuse counter overflow")?;
            }
            (t.events, t.cursor, t.state, t.previous, t.conversions)
        }
        None => (
            0,
            0,
            AccountState::initial(),
            None,
            crate::corpus::SequenceHash::new(),
        ),
    };
    let mut visit = |dataset: crate::corpus::QuoteBlock<'_>| {
        let index = dataset.chunk_index;
        if let Some(r) = &mut reuse {
            r.decoded_events = r
                .decoded_events
                .checked_add(dataset.ticks.len() as u64)
                .ok_or("decoded event counter overflow")?;
        }
        if index >= stop || (resumed && index < old_cursor as usize) {
            return Ok(());
        }
        for tick in dataset.ticks {
            let fx = match mode {
                ReplayMode::Registered => {
                    if observed.profile.quote == "USD" {
                        Conversion::usd()
                    } else {
                        diagnostic_conversion(&observed.profile, tick, strategy.side)?
                    }
                }
                ReplayMode::Diagnostic => {
                    diagnostic_conversion(&observed.profile, tick, strategy.side)?
                }
            };
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
            events = events.checked_add(1).ok_or("corpus event count overflow")?;
            if let Some(r) = &mut reuse {
                r.advanced_events = r
                    .advanced_events
                    .checked_add(1)
                    .ok_or("checkpoint replay counter overflow")?;
            }
            conversions.push(&fx)?;
            previous = Some(tick.timestamp_ms);
        }
        if !dataset.last_in_chunk {
            return Ok(());
        }
        boundary = index + 1;
        if boundary == old_cursor as usize {
            let actual = serde_json::json!({"version":"corpus-account-state-v1","corpus":spec.identity()?,"account":state,"previous_ms":previous,"events":events});
            if saved.as_ref().is_none_or(|(r, _)| r.state != actual) {
                return Err("checkpoint account state differs from verified source prefix".into());
            }
        }
        Ok(())
    };
    // Only an exact private checkpoint match can omit account-prefix replay.
    // Cold/unmatched stored state still recomputes from artifact zero.
    let start = if resumed { old_cursor as usize } else { 0 };
    if control.is_some() && (start > 0 || stop < spec.chunks.len()) {
        prepared.visit_replay_range(base, spec, start..stop, &mut visit)?;
    } else {
        // Full-span and independent scalar traversal already decode/verify every
        // artifact. With nothing omitted, retain that existing sequential path.
        crate::corpus::visit_blocks(base, spec, &mut visit)?;
    }
    if let Some(c) = &mut control {
        let mut committed_hash = saved.as_ref().map(|(_, hash)| hash.clone());
        if boundary > old_cursor as usize {
            let payload = serde_json::json!({"version":"corpus-account-state-v1","corpus":spec.identity()?,"account":state,"previous_ms":previous,"events":events});
            committed_hash = Some(c.registry.save_replay_checkpoint(
                c.lease,
                (c.clock)(),
                old_cursor,
                boundary as i64,
                payload,
            )?);
        }
        if boundary < spec.chunks.len() {
            if let Some(r) = &mut reuse {
                r.checkpoint = Some(TrustedCheckpoint {
                    experiment_id: c.lease.experiment_id.clone(),
                    source_binding: source_binding.ok_or("missing checkpoint source binding")?,
                    cursor: boundary,
                    checkpoint_hash: committed_hash.ok_or("missing committed checkpoint hash")?,
                    state,
                    previous,
                    events,
                    conversions,
                });
            }
            return Ok(None);
        }
    }
    Ok(Some(Replay {
        version: crate::ENGINE_VERSION.into(),
        strategy_id: strategy.identity()?,
        dataset_id: observed.ordered_events_hash.clone(),
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
    Terminal {
        experiment_id: String,
        outcome: String,
        result: crate::storage::StoredResult,
    },
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
fn corpus_source_binding(run: &CorpusRun<'_>) -> Result<String> {
    let base = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join(run.base);
    crate::data::hash(&(base, run.spec))
}
#[derive(Default, Debug, Clone, Copy, Serialize)]
pub struct CorpusPreparationStats {
    /// Distinct source inventories built by full batch preflight traversal.
    pub preflight_inventory_builds: usize,
    /// Existing private inventories reused after fresh verification of every artifact byte.
    pub preflight_inventory_reuses: usize,
    /// Logical quote events decoded by full preflight inventory construction.
    /// Excludes candle preparation, replay, raw byte scans and validation iterations.
    pub preflight_source_events_decoded: u64,
    pub builds: usize,
    pub reuses: usize,
    /// Verification attempts for warm retained candle contexts.
    pub current_byte_checks: usize,
    /// Verification attempts using a cold no-frame inventory.
    pub inventory_byte_checks: usize,
    /// Account transitions actually evaluated, including cold semantic prefix checks.
    pub account_events_replayed: u64,
    /// Prefix account transitions omitted using private worker-produced committed state.
    pub trusted_prefix_events_skipped: u64,
    /// Quotes decoded by replay traversal; excludes inventory/preparation and raw-byte scans.
    pub source_events_decoded: u64,
    /// Candle objects, not bytes/RSS or OS-enforced memory.
    pub peak_retained_candles_per_worker: usize,
}
#[derive(Default)]
struct WorkerPreparation {
    key_inventory: Option<(String, crate::corpus::SourceFacts)>,
    slot: Option<(String, crate::corpus::Prepared)>,
    stats: CorpusPreparationStats,
    resume: CheckpointReuse,
}
impl WorkerPreparation {
    fn binding(run: &CorpusRun<'_>) -> Result<String> {
        crate::data::hash(&(
            corpus_source_binding(run)?,
            run.strategy.canonical()?.timeframe,
            run.budget.max_candles,
        ))
    }
    fn key(&mut self, run: &CorpusRun<'_>) -> Result<crate::storage::ExperimentKey> {
        let binding = Self::binding(run)?;
        if let Some((stored, prepared)) = &self.slot
            && stored == &binding
        {
            self.stats.current_byte_checks += 1;
            if let Err(error) = prepared.verify_current(run.base, run.spec) {
                self.slot = None;
                self.key_inventory = None;
                return Err(error);
            }
            return corpus_key_from_facts(
                run.spec,
                run.strategy,
                run.costs,
                run.budget,
                &prepared.source_facts(),
            );
        }
        if let Some((stored, inventory)) = &self.key_inventory
            && stored == &corpus_source_binding(run)?
        {
            self.stats.inventory_byte_checks += 1;
            if let Err(error) = inventory.verify_current(run.base, run.spec) {
                self.key_inventory = None;
                return Err(error);
            }
            return corpus_key_from_facts(run.spec, run.strategy, run.costs, run.budget, inventory);
        }
        corpus_key(run.base, run.spec, run.strategy, run.costs, run.budget)
    }
    fn prepare(&mut self, run: &CorpusRun<'_>) -> Result<&crate::corpus::Prepared> {
        let binding = Self::binding(run)?;
        if self
            .slot
            .as_ref()
            .is_some_and(|(stored, _)| stored == &binding)
        {
            self.stats.reuses += 1;
        } else {
            // Release the previous buffer BEFORE constructing its replacement.
            self.slot = None;
            let prepared = crate::corpus::prepare(
                run.base,
                run.spec,
                run.strategy.canonical()?.timeframe,
                run.budget.max_candles,
            )?;
            self.stats.builds += 1;
            self.stats.peak_retained_candles_per_worker = self
                .stats
                .peak_retained_candles_per_worker
                .max(prepared.retained_candles());
            self.slot = Some((binding, prepared));
        }
        Ok(&self.slot.as_ref().ok_or("missing worker preparation")?.1)
    }
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
    execute_corpus_step_prepared(
        registry,
        run,
        max_new_chunks,
        lease_ms,
        &mut clock,
        &mut WorkerPreparation::default(),
    )
}
fn execute_corpus_step_prepared(
    registry: &mut crate::storage::Registry,
    run: CorpusRun<'_>,
    max_new_chunks: usize,
    lease_ms: i64,
    clock: &mut dyn FnMut() -> i64,
    preparation: &mut WorkerPreparation,
) -> Result<CorpusProgress> {
    if !(1..=64).contains(&max_new_chunks) || !(1..=86_400_000).contains(&lease_ms) {
        return Err("invalid corpus step/lease budget".into());
    }
    let mut resume = std::mem::take(&mut preparation.resume);
    let key = preparation.key(&run)?;
    let id = registry.register(&key)?;
    if resume
        .checkpoint
        .as_ref()
        .is_some_and(|c| c.experiment_id != id)
    {
        resume.checkpoint = None;
    }
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
            if result.outcome != "completed" {
                return Ok(CorpusProgress::Terminal {
                    experiment_id: id,
                    outcome: result.outcome.clone(),
                    result,
                });
            }
            let receipt = &result.result["tick_replay"];
            if result.result["kind"] != "synthetic-corpus-replay-v1"
                || result.result["corpus_identity"] != run.spec.identity()?
                || receipt["version"] != key.engine_version
                || receipt["dataset_id"] != key.dataset_hash
                || receipt["strategy_id"] != key.concept_hash
                || receipt["profile_id"] != key.profile_hash
                || receipt["cost_id"] != key.cost_hash
            {
                return Err("cached corpus receipt does not match frozen run".into());
            }
            return Ok(CorpusProgress::Completed {
                experiment_id: id,
                cached: true,
                result,
            });
        }
        crate::storage::Claim::Acquired(lease) => lease,
    };
    let result = (|| {
        let corpus_id = run.spec.identity()?;
        registry.validate_lease(&lease, clock())?;
        if registry.experiment_key(&id)?.as_ref() != Some(&key) {
            return Err("registered corpus inputs or build differ".into());
        }
        let prepared = preparation.prepare(&run)?;
        match replay_corpus_internal(
            run,
            Some(ResumeControl {
                registry,
                lease: &lease,
                max_new_chunks,
                clock,
            }),
            Some(prepared),
            Some(&mut resume),
            ReplayMode::Registered,
        )? {
            None => {
                let cursor = registry
                    .replay_checkpoint(&id)?
                    .ok_or("missing committed corpus checkpoint")?
                    .0
                    .cursor;
                registry.yield_lease(&lease, clock())?;
                preparation.stats.account_events_replayed += resume.advanced_events;
                preparation.stats.trusted_prefix_events_skipped += resume.skipped_events;
                preparation.stats.source_events_decoded += resume.decoded_events;
                resume.advanced_events = 0;
                resume.skipped_events = 0;
                resume.decoded_events = 0;
                preparation.resume = resume;
                Ok(CorpusProgress::Pending {
                    experiment_id: id,
                    cursor,
                })
            }
            Some(replay) => {
                preparation.stats.account_events_replayed += resume.advanced_events;
                preparation.stats.trusted_prefix_events_skipped += resume.skipped_events;
                preparation.stats.source_events_decoded += resume.decoded_events;
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
    })();
    if result.is_err() {
        // One best-effort cleanup attempt. Its transactional nonce/attempt/expiry
        // fence cannot yield a replacement actor or demote a terminal result.
        // Preserve the original execution error if cleanup itself is refused.
        let _ = registry.yield_lease(&lease, clock());
    }
    result
}

#[cfg(test)]
mod account_checkpoint_tests {
    use super::*;
    #[test]
    fn serialized_account_boundaries_preserve_complete_state() {
        use crate::strategies::{EntryPlan, Features, Rule};
        let mut resolved = 0;
        for symbol in ["EURUSD", "GBPUSD"] {
            let data = crate::data::synthetic_fixture(symbol).unwrap();
            for side in [Side::Long, Side::Short] {
                for entry in [
                    None,
                    Some(EntryPlan::StopMarket {
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

/// Declared concurrent object budgets; these are not measured RSS or OS limits.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusBatchLimits {
    pub workers: usize,
    pub max_parallel_candles: usize,
    pub max_parallel_feature_cells: usize,
    pub max_parallel_receipts: usize,
    pub max_new_chunks: usize,
    pub lease_ms: i64,
}
#[derive(Debug, Serialize)]
pub struct CorpusBatchReport {
    pub workers_started: usize,
    pub worker_failures: usize,
    pub preparation: CorpusPreparationStats,
    /// Input order is preserved, including errors and held/busy/terminal jobs.
    pub jobs: Vec<Result<CorpusProgress>>,
    /// Aligned with jobs; None for success, typed conservative provenance for errors.
    pub failure_kinds: Vec<Option<crate::corpus_failure::Kind>>,
}
/// Invocation-local ownership of at most one immutable context per worker.
/// Retains at most one private committed account per worker, within receipt reservations.
/// No leases, threads, connections or eligibility survive a round through this object.
#[derive(Default)]
pub struct CorpusBatchSession {
    binding: Option<String>,
    workers: Vec<WorkerPreparation>,
}
impl CorpusBatchSession {
    fn constrain_retention(&mut self, workers: usize, largest: usize, cap: usize) {
        // Across rounds a prior large context can move to another worker while
        // its old copy is still retained. Reserve that worst case for EVERY
        // worker, or discard old slots before dynamic assignment begins.
        if largest.checked_mul(workers).is_none_or(|total| total > cap) {
            self.release();
        }
    }
    pub fn release(&mut self) {
        self.workers.clear();
        self.binding = None;
    }
    /// Retained candle objects only, not total allocation/RSS or hard enforcement.
    pub fn retained_candles(&self) -> usize {
        self.workers
            .iter()
            .filter_map(|w| w.slot.as_ref())
            .map(|(_, p)| p.retained_candles())
            .sum()
    }
    pub fn round(
        &mut self,
        database: &std::path::Path,
        runs: &[CorpusRun<'_>],
        limits: CorpusBatchLimits,
    ) -> Result<CorpusBatchReport> {
        let result = execute_corpus_batch_session(database, runs, limits, self);
        if match &result {
            Err(_) => true,
            Ok(report) => report.worker_failures > 0 || report.jobs.iter().any(Result::is_err),
        } {
            self.release();
        }
        result
    }
}
/// One bounded checkpoint round. Preflight all jobs before opening the registry;
/// each worker owns a connection and shares only a bounded atomic work cursor.
pub fn execute_corpus_batch(
    database: &std::path::Path,
    runs: &[CorpusRun<'_>],
    limits: CorpusBatchLimits,
) -> Result<CorpusBatchReport> {
    CorpusBatchSession::default().round(database, runs, limits)
}
// Prefer one still-owned checkpoint per worker. Duplicate stale slots are
// possible after a jobless round; only the furthest private cursor gets priority.
fn preferred_checkpoint_jobs(
    workers: &[WorkerPreparation],
    inputs: &[(String, String)],
) -> Vec<Option<usize>> {
    let mut owners = std::collections::BTreeMap::<usize, (usize, usize)>::new();
    for (worker, slot) in workers.iter().enumerate() {
        if let Some(checkpoint) = &slot.resume.checkpoint
            && let Some(job) = inputs
                .iter()
                .position(|(_, id)| id == &checkpoint.experiment_id)
            && owners
                .get(&job)
                .is_none_or(|(_, cursor)| checkpoint.cursor > *cursor)
        {
            owners.insert(job, (worker, checkpoint.cursor));
        }
    }
    let mut preferred = vec![None; workers.len()];
    for (job, (worker, _)) in owners {
        preferred[worker] = Some(job);
    }
    preferred
}
fn next_batch_job(
    next: &std::sync::atomic::AtomicUsize,
    claimed: &[std::sync::atomic::AtomicBool],
) -> Option<usize> {
    use std::sync::atomic::Ordering;
    loop {
        let index = next.fetch_add(1, Ordering::Relaxed);
        let claim = claimed.get(index)?;
        if claim
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            return Some(index);
        }
    }
}
fn execute_corpus_batch_session(
    database: &std::path::Path,
    runs: &[CorpusRun<'_>],
    limits: CorpusBatchLimits,
    session: &mut CorpusBatchSession,
) -> Result<CorpusBatchReport> {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    if runs.is_empty()
        || runs.len() > 256
        || !(1..=16).contains(&limits.workers)
        || !(1..=64).contains(&limits.max_new_chunks)
        || !(1..=86_400_000).contains(&limits.lease_ms)
    {
        return Err("invalid corpus batch job/worker/step/lease limits".into());
    }
    let workers = limits.workers.min(runs.len());
    for (mut values, cap) in [
        (
            runs.iter()
                .map(|r| r.budget.max_candles)
                .collect::<Vec<_>>(),
            limits.max_parallel_candles,
        ),
        (
            runs.iter().map(|r| r.budget.feature_cells).collect(),
            limits.max_parallel_feature_cells,
        ),
        (
            runs.iter().map(|r| r.budget.max_receipts).collect(),
            limits.max_parallel_receipts,
        ),
    ] {
        values.sort_unstable_by(|a, b| b.cmp(a));
        let total = values
            .iter()
            .take(workers)
            .try_fold(0usize, |a, b| a.checked_add(*b))
            .ok_or("corpus aggregate budget overflow")?;
        if total > cap {
            return Err("corpus concurrent budget exceeds declared cap".into());
        }
    }
    session.constrain_retention(
        workers,
        runs.iter()
            .map(|run| run.budget.max_candles)
            .max()
            .ok_or("empty corpus batch")?,
        limits.max_parallel_candles,
    );
    // Retained account receipt vectors require the same worst-case reservation
    // under cross-round reassignment as retained candle contexts.
    session.constrain_retention(
        workers,
        runs.iter()
            .map(|r| r.budget.max_receipts)
            .max()
            .ok_or("empty corpus batch")?,
        limits.max_parallel_receipts,
    );
    let mut identities = std::collections::BTreeSet::new();
    let mut sources = std::collections::BTreeMap::new();
    let mut invocation_inputs = Vec::with_capacity(runs.len());
    let mut preparation = CorpusPreparationStats::default();
    for run in runs {
        let binding = corpus_source_binding(run)?;
        if !sources.contains_key(&binding) {
            // Reuse only already-retained private source proof, independently of
            // account/session authority. Every current byte and operational cap
            // is rechecked before the frozen key or registry can be touched.
            let retained = session.workers.iter().find_map(|worker| {
                worker
                    .key_inventory
                    .as_ref()
                    .filter(|(stored, _)| stored == &binding)
            });
            let inventory = match retained {
                Some((_, inventory)) => {
                    inventory.verify_current(run.base, run.spec)?;
                    preparation.preflight_inventory_reuses += 1;
                    inventory.clone()
                }
                None => {
                    let inventory = crate::corpus::source_facts(run.base, run.spec)?;
                    preparation.preflight_inventory_builds += 1;
                    preparation.preflight_source_events_decoded = preparation
                        .preflight_source_events_decoded
                        .checked_add(inventory.events())
                        .ok_or("preflight decode counter overflow")?;
                    inventory
                }
            };
            sources.insert(binding.clone(), inventory);
        }
        let observed = sources
            .get(&binding)
            .ok_or("missing preflight source report")?;
        let key = corpus_key_from_facts(run.spec, run.strategy, run.costs, run.budget, observed)?;
        let identity = key.identity()?;
        invocation_inputs.push((binding, identity.clone()));
        if !identities.insert(identity) {
            return Err("duplicate frozen corpus job in batch".into());
        }
    }
    let binding = crate::data::hash(&(
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(database),
        limits,
        &invocation_inputs,
    ))?;
    if session.binding.as_ref() != Some(&binding) {
        session.release();
    }
    // Initialize sequentially; do not race schema migrations across workers.
    let mut connections = Vec::with_capacity(workers);
    for _ in 0..workers {
        match crate::storage::Registry::open_classified(database) {
            Ok(connection) => connections.push(connection),
            Err(error) => {
                return Ok(CorpusBatchReport {
                    workers_started: 0,
                    worker_failures: 1,
                    preparation,
                    jobs: runs.iter().map(|_| Err(error.message.clone())).collect(),
                    failure_kinds: vec![Some(error.kind); runs.len()],
                });
            }
        }
    }
    let mut slots = std::mem::take(&mut session.workers);
    slots.resize_with(workers, WorkerPreparation::default);
    slots.truncate(workers);
    let mut restored: Vec<_> = (0..workers).map(|_| WorkerPreparation::default()).collect();
    let preferred = preferred_checkpoint_jobs(&slots, &invocation_inputs);
    let claimed: Vec<_> = (0..runs.len())
        .map(|job| AtomicBool::new(preferred.contains(&Some(job))))
        .collect();
    // Reserve preferred jobs before any worker can steal. On spawn failure the
    // reservation is cancelled BEFORE opening this gate, so no job is lost.
    let gate = (std::sync::Mutex::new(false), std::sync::Condvar::new());
    let next = AtomicUsize::new(0);
    let mut results: Vec<Option<(Result<CorpusProgress>, Option<crate::corpus_failure::Kind>)>> =
        (0..runs.len()).map(|_| None).collect();
    let mut worker_failures = 0;
    let mut workers_started = 0;
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for ((i, mut registry), mut worker_preparation) in
            connections.into_iter().enumerate().zip(slots)
        {
            let next = &next;
            let claimed = &claimed;
            let gate = &gate;
            let preferred_job = preferred[i];
            let inventories = &sources;
            worker_preparation.stats = CorpusPreparationStats {
                peak_retained_candles_per_worker: worker_preparation
                    .slot
                    .as_ref()
                    .map_or(0, |(_, p)| p.retained_candles()),
                ..CorpusPreparationStats::default()
            };
            match std::thread::Builder::new()
                .name(format!("corpus-{i}"))
                .spawn_scoped(scope, move || {
                    drop(
                        gate.1
                            .wait_while(gate.0.lock().unwrap_or_else(|p| p.into_inner()), |ready| {
                                !*ready
                            })
                            .unwrap_or_else(|p| p.into_inner()),
                    );
                    let mut completed = Vec::new();
                    let mut preparation = worker_preparation;
                    let mut preferred_job = preferred_job;
                    while let Some(index) = preferred_job
                        .take()
                        .or_else(|| next_batch_job(next, claimed))
                    {
                        let run = &runs[index];
                        let binding = match corpus_source_binding(run) {
                            Ok(binding) => binding,
                            Err(error) => {
                                completed.push((index, Err(error), None));
                                continue;
                            }
                        };
                        preparation.key_inventory = inventories
                            .get(&binding)
                            .map(|inventory| (binding, inventory.clone()));
                        let result = execute_corpus_step_prepared(
                            &mut registry,
                            CorpusRun {
                                base: run.base,
                                spec: run.spec,
                                strategy: run.strategy,
                                costs: run.costs,
                                budget: run.budget,
                            },
                            limits.max_new_chunks,
                            limits.lease_ms,
                            &mut || chrono::Utc::now().timestamp_millis(),
                            &mut preparation,
                        );
                        // Classify on the worker thread: the typed admission
                        // provenance cell is thread-local, so a writer-gate
                        // refusal must be captured here, before join, to stay
                        // a retry candidate instead of an unclassified error.
                        let kind = match &result {
                            Err(error) => Some(crate::corpus_failure::classify_error(error)),
                            Ok(_) => None,
                        };
                        completed.push((index, result, kind));
                    }
                    (completed, preparation)
                }) {
                Ok(handle) => {
                    workers_started += 1;
                    handles.push((i, handle));
                }
                Err(_) => {
                    if let Some(job) = preferred[i] {
                        claimed[job].store(false, Ordering::Relaxed);
                    }
                    worker_failures += 1;
                }
            }
        }
        *gate.0.lock().unwrap_or_else(|p| p.into_inner()) = true;
        gate.1.notify_all();
        for (index, handle) in handles {
            match handle.join() {
                Ok((completed, worker_preparation)) => {
                    let stats = worker_preparation.stats;
                    restored[index] = worker_preparation;
                    preparation.builds += stats.builds;
                    preparation.reuses += stats.reuses;
                    preparation.current_byte_checks += stats.current_byte_checks;
                    preparation.inventory_byte_checks += stats.inventory_byte_checks;
                    preparation.account_events_replayed += stats.account_events_replayed;
                    preparation.trusted_prefix_events_skipped +=
                        stats.trusted_prefix_events_skipped;
                    preparation.source_events_decoded += stats.source_events_decoded;
                    preparation.peak_retained_candles_per_worker = preparation
                        .peak_retained_candles_per_worker
                        .max(stats.peak_retained_candles_per_worker);
                    for (index, result, kind) in completed {
                        results[index] = Some((result, kind));
                    }
                }
                Err(_) => worker_failures += 1,
            }
        }
    });
    session.workers = restored;
    session.binding = Some(binding);
    let failure_kinds = results
        .iter()
        .map(|r| match r {
            Some((Ok(_), _)) => None,
            Some((Err(_), kind)) => Some(kind.unwrap_or(crate::corpus_failure::Kind::Unclassified)),
            None => Some(crate::corpus_failure::Kind::WorkerInterrupted),
        })
        .collect();
    Ok(CorpusBatchReport {
        workers_started,
        worker_failures,
        preparation,
        failure_kinds,
        jobs: results
            .into_iter()
            .map(|r| {
                r.map(|(result, _)| result).unwrap_or_else(|| {
                    Err("worker interrupted; inspect registry and retry after lease expiry".into())
                })
            })
            .collect(),
    })
}

#[cfg(test)]
mod worker_preparation_tests {
    use super::*;
    use crate::{candles::Timeframe, corpus, data, native, strategies::Rule};
    use std::{fs, path::PathBuf};
    struct Source {
        root: PathBuf,
        spec: corpus::Specification,
    }
    impl Source {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("prepared-worker-{name}-{}", std::process::id()));
            fs::create_dir(&root).unwrap();
            let path = root.join("source.rsf");
            native::write(&path, &data::synthetic_fixture("EURUSD").unwrap()).unwrap();
            let spec = corpus::describe(&[path], 100000).unwrap();
            Self { root, spec }
        }
    }
    impl Drop for Source {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
    /// Real-class corpus fixture for the diagnostic input-class gate. The tick content is the
    /// deterministic synthetic fixture; only the declared class is promoted so the diagnostic path
    /// can be exercised without the multi-gigabyte historical corpus.
    struct RealSource {
        root: PathBuf,
        spec: corpus::Specification,
        data: data::Dataset,
    }
    impl RealSource {
        fn new(name: &str, symbol: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("prepared-real-{name}-{}", std::process::id()));
            fs::create_dir(&root).unwrap();
            let mut data = data::synthetic_fixture(symbol).unwrap();
            data.manifest.class = data::DataClass::RealStructurallyValid;
            let path = root.join("source.rsf");
            native::write(&path, &data).unwrap();
            let spec = corpus::describe(&[path], 100000).unwrap();
            Self { root, spec, data }
        }
    }
    impl Drop for RealSource {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
    fn strategy() -> Strategy {
        Strategy {
            timeframe: Timeframe::M1,
            side: Side::Long,
            stop_ticks: 100,
            rule: Rule::AboveMean { window: 2 },
            entry: None,
            stop_policy: None,
        }
    }
    fn budget() -> CorpusBudget {
        CorpusBudget {
            max_candles: 10000,
            feature_cells: 100000,
            max_receipts: 10000,
        }
    }
    #[test]
    fn diagnostic_replay_refuses_synthetic_inputs() {
        let source = Source::new("mode-gate");
        let error = replay_corpus_diagnostic(
            &source.root,
            &source.spec,
            &strategy(),
            &Costs::default(),
            budget(),
        )
        .expect_err("diagnostic replay must not accept synthetic test-only inputs");
        assert!(error.contains("real structurally-valid"), "{error}");
    }
    #[test]
    fn diagnostic_conversion_is_direction_based_reciprocal() {
        let jpy = Profile::synthetic("USDJPY").unwrap();
        let tick = Tick {
            timestamp_ms: 5_000,
            sequence: 1,
            bid: 150_000,
            ask: 150_100,
        };
        let long = diagnostic_conversion(&jpy, &tick, Side::Long).unwrap();
        assert_eq!(
            (long.usd_num, long.quote_den, long.as_of_ms),
            (1, 150, 5_000)
        );
        let short = diagnostic_conversion(&jpy, &tick, Side::Short).unwrap();
        assert_eq!((short.usd_num, short.quote_den), (10, 1_501));
        // The rate is exactly USD per quote currency: 1 / 150.1.
        assert_eq!(
            i128::from(short.usd_num) * 150_100,
            i128::from(short.quote_den) * 1_000
        );
        let eur = Profile::synthetic("EURUSD").unwrap();
        assert_eq!(
            diagnostic_conversion(&eur, &tick, Side::Long).unwrap(),
            Conversion::usd()
        );
        assert!(Conversion::usd_account_from_usd_base_price(&eur, 100_000, 0).is_err());
    }
    #[test]
    fn diagnostic_replay_executes_usd_base_non_usd_quote_with_exact_reciprocal_conversions() {
        for side in [Side::Long, Side::Short] {
            let source = RealSource::new("usdjpy-conversion", "USDJPY");
            let mut direction = strategy();
            direction.side = side;
            let costs = Costs::default();
            let budget = budget();
            let replay_report =
                replay_corpus_diagnostic(&source.root, &source.spec, &direction, &costs, budget)
                    .expect("USD-base non-USD-quote corpus must execute in diagnostic mode");
            let mut expected = crate::corpus::SequenceHash::new();
            for tick in &source.data.ticks {
                expected
                    .push(&diagnostic_conversion(&source.data.profile, tick, side).unwrap())
                    .unwrap();
            }
            assert_eq!(
                replay_report.conversion_id,
                expected.finish(),
                "diagnostic replay must use the exact direction-based conversion sequence"
            );
            // The corpus path must match the materialized explicit-conversion engine exactly.
            let candles = crate::candles::build(&source.data, direction.timeframe).unwrap();
            let mut features = crate::strategies::Features::with_context(
                &candles,
                candles.len().saturating_mul(128),
                &source.data,
                direction.timeframe,
            )
            .unwrap();
            let mask = features.signal(&direction.rule).unwrap();
            let conversions: Vec<Conversion> = source
                .data
                .ticks
                .iter()
                .map(|tick| diagnostic_conversion(&source.data.profile, tick, side).unwrap())
                .collect();
            let materialized = replay(
                &source.data,
                &candles,
                &mask,
                &direction,
                &costs,
                &conversions,
            )
            .expect("materialized explicit-conversion replay must succeed");
            assert_eq!(
                replay_report.realized_micro_usd,
                materialized.realized_micro_usd
            );
            assert_eq!(
                replay_report.final_marked_micro_usd,
                materialized.final_marked_micro_usd
            );
            assert_eq!(
                replay_report.maximum_drawdown_micro_usd,
                materialized.maximum_drawdown_micro_usd
            );
            assert_eq!(replay_report.trades, materialized.trades);
            assert_eq!(replay_report.rejections, materialized.rejections);
            assert!(
                !replay_report.trades.is_empty(),
                "{side:?}: fixture must produce engine activity"
            );
        }
    }
    #[test]
    fn diagnostic_replay_usd_quote_uses_identity_conversion() {
        // Every canonical profile is either USD-quoted or USD-base, so the cross-currency
        // refusal is a defensive gate. A USD-quoted profile must take the identity path.
        let eur = RealSource::new("eurusd-gate", "EURUSD");
        let direction = strategy();
        let replay = replay_corpus_diagnostic(
            &eur.root,
            &eur.spec,
            &direction,
            &Costs::default(),
            budget(),
        )
        .expect("USD-quoted real-class corpus must execute");
        let mut expected = crate::corpus::SequenceHash::new();
        for tick in &eur.data.ticks {
            expected
                .push(&diagnostic_conversion(&eur.data.profile, tick, direction.side).unwrap())
                .unwrap();
        }
        assert_eq!(replay.conversion_id, expected.finish());
        assert!(eur.data.ticks.iter().all(|tick| {
            diagnostic_conversion(&eur.data.profile, tick, direction.side).unwrap()
                == Conversion::usd()
        }));
    }
    #[test]
    fn saturated_writer_gate_refuses_batch_typed_and_recovers_after_pressure() {
        let source = Source::new("gate-saturation");
        let db = source.root.join("saturated.sqlite");
        let costs = Costs::default();
        let budget = budget();
        let s = strategy();
        let runs = [CorpusRun {
            base: &source.root,
            spec: &source.spec,
            strategy: &s,
            costs: &costs,
            budget,
        }];
        let limits = CorpusBatchLimits {
            workers: 1,
            max_parallel_candles: 20000,
            max_parallel_feature_cells: 200000,
            max_parallel_receipts: 20000,
            max_new_chunks: 1,
            lease_ms: 60000,
        };
        // Occupy both in-process admission slots for this database file for
        // the whole batch call: startup must refuse with the typed busy kind
        // (a retry candidate), not panic or abort unclassified.
        // Open both handles before the write lock is held: schema bootstrap
        // writes would otherwise queue behind the open transaction.
        let holder_a = crate::storage::Registry::open(&db).unwrap();
        let holder_b = crate::storage::Registry::open(&db).unwrap();
        holder_b
            .connection
            .busy_timeout(std::time::Duration::from_secs(60))
            .unwrap();
        let tx_a = crate::storage::writer_tx(&holder_a.connection).unwrap();
        // WAL allows only one open writer: the second admitted slot parks in
        // BEGIN IMMEDIATE's busy wait on a separate thread (long busy timeout
        // so it cannot expire inside the test window).
        let parked = std::thread::spawn(move || {
            let tx = crate::storage::writer_tx(&holder_b.connection).unwrap();
            tx.commit().unwrap();
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while holder_a.writer_admission()["in_flight"] != "2" {
            assert!(
                std::time::Instant::now() < deadline,
                "gate never saturated: {}",
                holder_a.writer_admission()
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut session = CorpusBatchSession::default();
        let refused = session.round(&db, &runs, limits).unwrap();
        assert_eq!(refused.workers_started, 0);
        assert_eq!(refused.worker_failures, 1);
        assert_eq!(
            refused.failure_kinds,
            vec![Some(crate::corpus_failure::Kind::RegistryAdmissionBusy)]
        );
        assert!(refused.jobs[0].is_err());
        assert!(refused.failure_kinds[0].unwrap().retry_candidate());
        drop(tx_a);
        parked.join().unwrap();
        drop(holder_a);
        // Once pressure clears, the same session admits and completes the job.
        let recovered = session.round(&db, &runs, limits).unwrap();
        assert_eq!(recovered.worker_failures, 0);
        assert!(recovered.jobs[0].is_ok(), "{:?}", recovered.jobs[0]);
    }
    fn affinity_source(name: &str) -> Source {
        let mut source = Source::new(name);
        let data = data::synthetic_fixture("EURUSD").unwrap();
        let mut paths = Vec::new();
        for (i, ticks) in data.ticks.chunks(250).enumerate() {
            let part = data::dataset(
                data.profile.clone(),
                ticks.to_vec(),
                &data.manifest.source,
                data.manifest.archive_hash.clone(),
                true,
            )
            .unwrap();
            let path = source.root.join(format!("affinity-{i}.rsf"));
            native::write(&path, &part).unwrap();
            paths.push(path);
        }
        source.spec = corpus::describe(&paths, 100000).unwrap();
        assert!(source.spec.chunks.len() > 2);
        source
    }
    #[test]
    fn affinity_reuses_every_available_worker_checkpoint_with_parallel_receipt_parity() {
        let source = affinity_source("affinity-parity");
        let costs = Costs::default();
        let budget = budget();
        let mut strategies = [strategy(), strategy(), strategy()];
        strategies[1].entry = Some(crate::strategies::EntryPlan::StopMarket {
            offset_ticks: 3,
            ttl_ms: 120000,
            cancel_on_signal_false: true,
        });
        strategies[2].entry = Some(crate::strategies::EntryPlan::StopMarket {
            offset_ticks: 2,
            ttl_ms: 120000,
            cancel_on_signal_false: true,
        });
        let runs: Vec<_> = strategies
            .iter()
            .map(|s| CorpusRun {
                base: &source.root,
                spec: &source.spec,
                strategy: s,
                costs: &costs,
                budget,
            })
            .collect();
        let limits = CorpusBatchLimits {
            workers: 2,
            max_parallel_candles: 20000,
            max_parallel_feature_cells: 200000,
            max_parallel_receipts: 20000,
            max_new_chunks: 1,
            lease_ms: 60000,
        };
        let db = source.root.join("affinity.sqlite");
        let mut session = CorpusBatchSession::default();
        let first = session.round(&db, &runs, limits).unwrap();
        assert_eq!(first.worker_failures, 0);
        assert!(first.jobs.iter().all(Result::is_ok));
        let mut available = std::collections::BTreeMap::new();
        for w in &session.workers {
            if let Some(c) = &w.resume.checkpoint {
                available.insert(c.experiment_id.clone(), c.events);
            }
        }
        let expected: u64 = available.values().sum();
        assert!(expected > 0);
        let second = session.round(&db, &runs, limits).unwrap();
        assert_eq!(second.worker_failures, 0);
        assert_eq!(second.preparation.trusted_prefix_events_skipped, expected);
        assert_eq!(second.preparation.source_events_decoded, 1500 - expected);
        assert!(
            second
                .jobs
                .iter()
                .all(|r| matches!(r, Ok(CorpusProgress::Pending { cursor: 2, .. })))
        );
        let mut completed = vec![None; 3];
        for _ in 2..source.spec.chunks.len() {
            let r = session.round(&db, &runs, limits).unwrap();
            assert_eq!(r.worker_failures, 0);
            for (i, result) in r.jobs.into_iter().enumerate() {
                match result.unwrap() {
                    CorpusProgress::Pending { .. } => {}
                    CorpusProgress::Completed { result, .. } => completed[i] = Some(result),
                    other => panic!("{other:?}"),
                }
            }
        }
        for (i, strategy) in strategies.iter().enumerate() {
            let reference =
                replay_corpus(&source.root, &source.spec, strategy, &costs, budget).unwrap();
            assert!(!reference.trades.is_empty());
            assert_eq!(
                completed[i].as_ref().unwrap().result["tick_replay"],
                serde_json::to_value(reference).unwrap()
            );
        }
        crate::storage::Registry::open(&db)
            .unwrap()
            .integrity()
            .unwrap();
    }
    #[test]
    fn affinity_prefers_latest_private_cursor_when_idle_slots_contain_stale_duplicates() {
        let source = affinity_source("affinity-duplicates");
        let costs = Costs::default();
        let budget = budget();
        let strategy = strategy();
        let run = || CorpusRun {
            base: &source.root,
            spec: &source.spec,
            strategy: &strategy,
            costs: &costs,
            budget,
        };
        let mut registry =
            crate::storage::Registry::open(&source.root.join("duplicates.sqlite")).unwrap();
        let mut old = WorkerPreparation::default();
        let mut latest = WorkerPreparation::default();
        let first =
            execute_corpus_step_prepared(&mut registry, run(), 1, 60000, &mut || 1000, &mut old)
                .unwrap();
        let CorpusProgress::Pending { experiment_id, .. } = first else {
            panic!()
        };
        execute_corpus_step_prepared(&mut registry, run(), 1, 60000, &mut || 1001, &mut latest)
            .unwrap();
        assert_eq!(old.resume.checkpoint.as_ref().unwrap().cursor, 1);
        assert_eq!(latest.resume.checkpoint.as_ref().unwrap().cursor, 2);
        assert_eq!(
            preferred_checkpoint_jobs(&[old, latest], &[("source".into(), experiment_id)]),
            vec![None, Some(0)]
        );
    }
    #[test]
    fn cancelled_startup_reservations_and_concurrent_stealing_execute_each_job_exactly_once() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        for reserved in [0, 3, 17, 99] {
            let claimed: Vec<_> = (0..100)
                .map(|j| AtomicBool::new(j == reserved || j == 7))
                .collect();
            // Failed worker's reservation is cancelled before the start gate opens.
            if reserved != 7 {
                claimed[7].store(false, Ordering::Relaxed);
            }
            let next = AtomicUsize::new(0);
            let mut jobs = std::thread::scope(|scope| {
                let a = scope.spawn(|| {
                    let mut v = vec![reserved];
                    while let Some(j) = next_batch_job(&next, &claimed) {
                        v.push(j);
                    }
                    v
                });
                let b = scope.spawn(|| {
                    let mut v = Vec::new();
                    while let Some(j) = next_batch_job(&next, &claimed) {
                        v.push(j);
                    }
                    v
                });
                let mut all = a.join().unwrap();
                all.extend(b.join().unwrap());
                all
            });
            jobs.sort_unstable();
            assert_eq!(jobs, (0..100).collect::<Vec<_>>());
        }
    }

    #[test]
    fn combined_warm_checkpoint_still_refuses_consumed_source_mutation_after_key_admission() {
        std::thread::scope(|scope| {
            for blocked in [false, true] {
                scope.spawn(move || {
                    let mut source = Source::new(if blocked {
                        "combined-admission-block"
                    } else {
                        "combined-admission-native"
                    });
                    let data = data::synthetic_fixture("EURUSD").unwrap();
                    let mut paths = Vec::new();
                    for (i, ticks) in data.ticks.chunks(data.ticks.len().div_ceil(2)).enumerate() {
                        let part = data::dataset(
                            data.profile.clone(),
                            ticks.to_vec(),
                            &data.manifest.source,
                            data.manifest.archive_hash.clone(),
                            true,
                        )
                        .unwrap();
                        let path = source
                            .root
                            .join(format!("part-{i}.{}", if blocked { "rsfb" } else { "rsf" }));
                        if blocked {
                            crate::blocked_native::write(&path, &part, 64).unwrap();
                        } else {
                            native::write(&path, &part).unwrap();
                        }
                        paths.push(path);
                    }
                    source.spec = corpus::describe(&paths, 100000).unwrap();
                    let strategy = strategy();
                    let costs = Costs::default();
                    let budget = budget();
                    let run = || CorpusRun {
                        base: &source.root,
                        spec: &source.spec,
                        strategy: &strategy,
                        costs: &costs,
                        budget,
                    };
                    let mut registry =
                        crate::storage::Registry::open(&source.root.join("state.sqlite")).unwrap();
                    let mut cache = WorkerPreparation::default();
                    let first = execute_corpus_step_prepared(
                        &mut registry,
                        run(),
                        1,
                        60000,
                        &mut || 1000,
                        &mut cache,
                    )
                    .unwrap();
                    let CorpusProgress::Pending { experiment_id, .. } = first else {
                        panic!()
                    };
                    assert!(cache.resume.checkpoint.is_some());
                    let prior = registry
                        .replay_checkpoint(&experiment_id)
                        .unwrap()
                        .unwrap()
                        .1;
                    let original = fs::read(&paths[0]).unwrap();
                    let mut calls = 0;
                    let error = execute_corpus_step_prepared(
                        &mut registry,
                        run(),
                        1,
                        60000,
                        &mut || {
                            calls += 1;
                            if calls == 2 {
                                let mut bytes = fs::read(&paths[0]).unwrap();
                                let last = bytes.len() - 1;
                                bytes[last] ^= 1;
                                fs::write(&paths[0], bytes).unwrap();
                            }
                            1001
                        },
                        &mut cache,
                    )
                    .unwrap_err();
                    assert!(error.contains("checksum"), "{error}");
                    assert_eq!(
                        registry
                            .replay_checkpoint(&experiment_id)
                            .unwrap()
                            .unwrap()
                            .1,
                        prior
                    );
                    assert!(registry.result(&experiment_id).unwrap().is_none());
                    assert!(cache.resume.checkpoint.is_none());
                    let sql = rusqlite::Connection::open(source.root.join("state.sqlite")).unwrap();
                    let (state, token): (String, Option<String>) = sql
                        .query_row(
                            "SELECT state,token FROM experiments WHERE id=?1",
                            [&experiment_id],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .unwrap();
                    assert_eq!(state, "queued");
                    assert!(token.is_none());
                    assert_eq!(registry.counts().unwrap().attempts, 2);
                    fs::write(&paths[0], original).unwrap();
                    let before = cache.stats.account_events_replayed;
                    let progress = execute_corpus_step_prepared(
                        &mut registry,
                        run(),
                        1,
                        60000,
                        &mut || 1002,
                        &mut cache,
                    )
                    .unwrap();
                    let CorpusProgress::Completed {
                        result,
                        cached: false,
                        ..
                    } = progress
                    else {
                        panic!("{progress:?}")
                    };
                    // Error consumes private authority. Immediate retry must reconstruct
                    // the exact complete account prefix rather than reuse failed state.
                    assert_eq!(
                        cache.stats.account_events_replayed - before,
                        data.ticks.len() as u64
                    );
                    let reference =
                        replay_corpus(&source.root, &source.spec, &strategy, &costs, budget)
                            .unwrap();
                    assert!(!reference.trades.is_empty());
                    assert_eq!(
                        result.result["tick_replay"],
                        serde_json::to_value(reference).unwrap()
                    );
                    assert_eq!(registry.counts().unwrap().attempts, 3);
                    registry.integrity().unwrap();
                });
            }
        });
    }

    #[test]
    fn cold_inventory_never_grants_real_or_non_usd_execution_authority() {
        for (symbol, synthetic) in [("USDJPY", true), ("EURUSD", false)] {
            let mut source = Source::new(&format!("inventory-gates-{symbol}"));
            let generated = data::synthetic_fixture(symbol).unwrap();
            let dataset = data::dataset(
                generated.profile,
                generated.ticks,
                "SYNTHETIC gate fixture; unreviewed declaration is not genuine evidence",
                "generated-no-archive".into(),
                synthetic,
            )
            .unwrap();
            let path = source.root.join("source.rsf");
            fs::remove_file(&path).unwrap();
            native::write(&path, &dataset).unwrap();
            source.spec = corpus::describe(&[path], 100000).unwrap();
            let inventory = corpus::verified_inventory(&source.root, &source.spec).unwrap();
            let strategy = strategy();
            let costs = Costs::default();
            let run = CorpusRun {
                base: &source.root,
                spec: &source.spec,
                strategy: &strategy,
                costs: &costs,
                budget: budget(),
            };
            let mut cache = WorkerPreparation {
                key_inventory: Some((
                    corpus_source_binding(&run).unwrap(),
                    inventory.source_facts(),
                )),
                ..WorkerPreparation::default()
            };
            let mut registry =
                crate::storage::Registry::open(&source.root.join("state.sqlite")).unwrap();
            assert!(
                execute_corpus_step_prepared(
                    &mut registry,
                    run,
                    64,
                    60000,
                    &mut || 1000,
                    &mut cache
                )
                .is_err()
            );
            assert_eq!(registry.counts().unwrap().registered, 0);
            assert_eq!(cache.stats.inventory_byte_checks, 1);
            assert_eq!(cache.stats.builds, 0);
        }
    }
    #[test]
    fn cold_inventory_key_rechecks_current_bytes_before_registration_and_cached_reuse() {
        let source = Source::new("cold-inventory");
        let strategy = strategy();
        let costs = Costs::default();
        let run = || CorpusRun {
            base: &source.root,
            spec: &source.spec,
            strategy: &strategy,
            costs: &costs,
            budget: budget(),
        };
        let inventory = crate::corpus::verified_inventory(&source.root, &source.spec).unwrap();
        assert_eq!(inventory.retained_candles(), 0);
        let expected = corpus_key(&source.root, &source.spec, &strategy, &costs, budget()).unwrap();
        let binding = corpus_source_binding(&run()).unwrap();
        let mut cache = WorkerPreparation {
            key_inventory: Some((binding.clone(), inventory.source_facts())),
            ..WorkerPreparation::default()
        };
        assert_eq!(cache.key(&run()).unwrap(), expected);
        assert_eq!(cache.stats.inventory_byte_checks, 1);
        assert_eq!(cache.stats.builds, 0);
        let path = &source.spec.chunks[0].path;
        let bytes = fs::read(path).unwrap();
        let mut bad = bytes.clone();
        let index = bad.len() / 2;
        bad[index] ^= 1;
        fs::write(path, bad).unwrap();
        let mut registry =
            crate::storage::Registry::open(&source.root.join("state.sqlite")).unwrap();
        assert!(
            execute_corpus_step_prepared(&mut registry, run(), 64, 60000, &mut || 1000, &mut cache)
                .is_err()
        );
        assert!(cache.key_inventory.is_none());
        assert!(
            registry
                .experiment_key(&expected.identity().unwrap())
                .unwrap()
                .is_none()
        );
        fs::write(path, bytes).unwrap();
        cache.key_inventory = Some((binding, inventory.source_facts()));
        let completed =
            execute_corpus_step_prepared(&mut registry, run(), 64, 60000, &mut || 1001, &mut cache)
                .unwrap();
        assert!(matches!(
            completed,
            CorpusProgress::Completed { cached: false, .. }
        ));
        cache.slot = None;
        let builds = cache.stats.builds;
        let cached =
            execute_corpus_step_prepared(&mut registry, run(), 64, 60000, &mut || 1002, &mut cache)
                .unwrap();
        assert!(matches!(
            cached,
            CorpusProgress::Completed { cached: true, .. }
        ));
        assert_eq!(cache.stats.builds, builds);
        assert_eq!(cache.stats.inventory_byte_checks, 4);
        registry.integrity().unwrap();
    }
    #[test]
    fn retained_worker_reservations_cover_dynamic_reassignment_or_release_before_work() {
        let source = Source::new("retention-reservations");
        let costs = Costs::default();
        let strategy = strategy();
        let mut budget = budget();
        budget.max_candles = 500;
        let run = CorpusRun {
            base: &source.root,
            spec: &source.spec,
            strategy: &strategy,
            costs: &costs,
            budget,
        };
        let mut worker = WorkerPreparation::default();
        worker.prepare(&run).unwrap();
        let retained = worker.slot.as_ref().unwrap().1.retained_candles();
        assert!(retained * 2 > 520);
        let mut session = CorpusBatchSession {
            binding: Some("fixture".into()),
            workers: vec![worker],
        };
        session.constrain_retention(2, 500, 1000);
        assert_eq!(session.retained_candles(), retained);
        assert!(session.binding.is_some());
        // Top-two jobs could be500 and20. That520 admission does NOT reserve
        // two500 slots if the large job moves workers on the following round.
        session.constrain_retention(2, 500, 520);
        assert_eq!(session.retained_candles(), 0);
        assert!(session.binding.is_none());
        let mut worker = WorkerPreparation::default();
        worker.prepare(&run).unwrap();
        session.workers.push(worker);
        session.binding = Some("fixture".into());
        session.constrain_retention(2, usize::MAX, usize::MAX);
        assert_eq!(session.retained_candles(), 0);
        assert!(session.binding.is_none());
    }
    #[test]
    fn worker_preparation_refuses_mutation_before_registration_and_recovers_after_restore() {
        let source = Source::new("mutation");
        let costs = Costs::default();
        let first = strategy();
        let mut second = first.clone();
        second.stop_ticks += 10;
        let mut cache = WorkerPreparation::default();
        let mut registry =
            crate::storage::Registry::open(&source.root.join("state.sqlite")).unwrap();
        let run = |strategy| CorpusRun {
            base: source.root.as_path(),
            spec: &source.spec,
            strategy,
            costs: &costs,
            budget: budget(),
        };
        let completed = execute_corpus_step_prepared(
            &mut registry,
            run(&first),
            64,
            60000,
            &mut || 1000,
            &mut cache,
        )
        .unwrap();
        assert!(matches!(
            completed,
            CorpusProgress::Completed { cached: false, .. }
        ));
        let next_key = corpus_key(
            source.root.as_path(),
            &source.spec,
            &second,
            &costs,
            budget(),
        )
        .unwrap();
        let next_id = next_key.identity().unwrap();
        assert!(registry.experiment_key(&next_id).unwrap().is_none());
        let path = &source.spec.chunks[0].path;
        let bytes = fs::read(path).unwrap();
        let mut bad = bytes.clone();
        let index = bad.len() / 2;
        bad[index] ^= 1;
        fs::write(path, bad).unwrap();
        assert!(
            execute_corpus_step_prepared(
                &mut registry,
                run(&second),
                64,
                60000,
                &mut || 1001,
                &mut cache
            )
            .is_err()
        );
        assert!(cache.slot.is_none());
        assert!(registry.experiment_key(&next_id).unwrap().is_none());
        fs::write(path, &bytes).unwrap();
        let recovered = execute_corpus_step_prepared(
            &mut registry,
            run(&second),
            64,
            60000,
            &mut || 1002,
            &mut cache,
        )
        .unwrap();
        let CorpusProgress::Completed {
            result,
            cached: false,
            ..
        } = recovered
        else {
            panic!("expected restored completion")
        };
        assert_eq!(
            result.result["tick_replay"],
            serde_json::to_value(
                replay_corpus(
                    source.root.as_path(),
                    &source.spec,
                    &second,
                    &costs,
                    budget()
                )
                .unwrap()
            )
            .unwrap()
        );
        assert_eq!(cache.stats.builds, 2);
        assert_eq!(cache.stats.current_byte_checks, 1);
        assert_eq!(cache.stats.reuses, 0);
        let cached = execute_corpus_step_prepared(
            &mut registry,
            run(&second),
            64,
            60000,
            &mut || 1003,
            &mut cache,
        )
        .unwrap();
        assert!(matches!(
            cached,
            CorpusProgress::Completed { cached: true, .. }
        ));
        assert_eq!(cache.stats.current_byte_checks, 2);
        assert_eq!(cache.stats.reuses, 0); // Cached results start no account replay.
        registry.integrity().unwrap();
    }
    #[test]
    fn worker_preparation_mutation_after_admission_cannot_publish_checkpoint_or_receipt() {
        let source = Source::new("after-admission");
        let costs = Costs::default();
        let first = strategy();
        let mut second = first.clone();
        second.stop_ticks += 10;
        let mut cache = WorkerPreparation::default();
        let mut registry =
            crate::storage::Registry::open(&source.root.join("state.sqlite")).unwrap();
        let run = |strategy| CorpusRun {
            base: source.root.as_path(),
            spec: &source.spec,
            strategy,
            costs: &costs,
            budget: budget(),
        };
        execute_corpus_step_prepared(
            &mut registry,
            run(&first),
            64,
            60000,
            &mut || 1000,
            &mut cache,
        )
        .unwrap();
        let id = corpus_key(
            source.root.as_path(),
            &source.spec,
            &second,
            &costs,
            budget(),
        )
        .unwrap()
        .identity()
        .unwrap();
        let path = &source.spec.chunks[0].path;
        let bytes = fs::read(path).unwrap();
        let mut bad = bytes.clone();
        let index = bad.len() / 2;
        bad[index] ^= 1;
        let mut changed = false;
        let mut clock = || {
            if !changed {
                fs::write(path, &bad).unwrap();
                changed = true;
            }
            1001
        };
        assert!(
            execute_corpus_step_prepared(
                &mut registry,
                run(&second),
                64,
                60000,
                &mut clock,
                &mut cache
            )
            .is_err()
        );
        assert!(changed);
        assert!(registry.experiment_key(&id).unwrap().is_some());
        assert!(registry.replay_checkpoint(&id).unwrap().is_none());
        assert!(registry.result(&id).unwrap().is_none());
        fs::write(path, &bytes).unwrap();
        let completed = execute_corpus_step_prepared(
            &mut registry,
            run(&second),
            64,
            60000,
            &mut || 100000,
            &mut cache,
        )
        .unwrap();
        let CorpusProgress::Completed {
            result,
            cached: false,
            ..
        } = completed
        else {
            panic!("expected restored completion")
        };
        assert_eq!(
            result.result["tick_replay"],
            serde_json::to_value(
                replay_corpus(
                    source.root.as_path(),
                    &source.spec,
                    &second,
                    &costs,
                    budget()
                )
                .unwrap()
            )
            .unwrap()
        );
        assert_eq!(cache.stats.builds, 1);
        assert_eq!(cache.stats.reuses, 2);
        assert_eq!(cache.stats.current_byte_checks, 2);
        registry.integrity().unwrap();
    }
    #[test]
    fn worker_preparation_binds_paths_timeframe_and_cap_and_releases_failed_replacement() {
        let source = Source::new("binding");
        let costs = Costs::default();
        let primary = strategy();
        let mut budget = budget();
        let mut cache = WorkerPreparation::default();
        let run = CorpusRun {
            base: &source.root,
            spec: &source.spec,
            strategy: &primary,
            costs: &costs,
            budget,
        };
        let binding = WorkerPreparation::binding(&run).unwrap();
        cache.key(&run).unwrap();
        cache.prepare(&run).unwrap();
        let expected = cache.slot.as_ref().unwrap().1.retained_candles();
        cache.key(&run).unwrap();
        cache.prepare(&run).unwrap();
        assert_eq!(cache.stats.builds, 1);
        assert_eq!(cache.stats.reuses, 1);
        let moved = source.root.join("moved.rsf");
        fs::copy(&source.spec.chunks[0].path, &moved).unwrap();
        let mut relocated = source.spec.clone();
        relocated.chunks[0].path = moved.to_str().unwrap().into();
        assert_eq!(
            source.spec.identity().unwrap(),
            relocated.identity().unwrap()
        );
        let run = CorpusRun {
            base: &source.root,
            spec: &relocated,
            strategy: &primary,
            costs: &costs,
            budget,
        };
        assert_ne!(binding, WorkerPreparation::binding(&run).unwrap());
        cache.key(&run).unwrap();
        cache.prepare(&run).unwrap();
        assert_eq!(cache.stats.builds, 2);
        let mut higher = primary.clone();
        higher.timeframe = Timeframe::M5;
        let run = CorpusRun {
            base: &source.root,
            spec: &relocated,
            strategy: &higher,
            costs: &costs,
            budget,
        };
        assert_ne!(binding, WorkerPreparation::binding(&run).unwrap());
        cache.key(&run).unwrap();
        cache.prepare(&run).unwrap();
        assert_eq!(cache.stats.builds, 3);
        budget.max_candles = 1;
        let run = CorpusRun {
            base: &source.root,
            spec: &source.spec,
            strategy: &primary,
            costs: &costs,
            budget,
        };
        assert_ne!(binding, WorkerPreparation::binding(&run).unwrap());
        cache.key(&run).unwrap();
        assert!(cache.prepare(&run).is_err());
        assert!(cache.slot.is_none());
        assert_eq!(cache.stats.peak_retained_candles_per_worker, expected);
    }
}
