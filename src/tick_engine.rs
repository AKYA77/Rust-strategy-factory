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
    fn validate(&self) -> Result<()> {
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
    fn commission(&self, volume: i64) -> i128 {
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
pub struct Replay {
    pub version: String,
    pub strategy_id: String,
    pub dataset_id: String,
    pub profile_id: String,
    pub cost_id: String,
    pub fill_assumption: String,
    pub initial_micro_usd: i128,
    pub realized_micro_usd: i128,
    pub final_marked_micro_usd: i128,
    pub maximum_drawdown_micro_usd: i128,
    pub trades: Vec<Trade>,
    pub rejections: Vec<Rejection>,
    pub unresolved: Option<OpenPosition>,
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
    let mut features = crate::strategies::Features::new(candles, candles.len().saturating_mul(34))?;
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
    for (event_i, t) in data.ticks.iter().enumerate() {
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
                    mark.checked_sub(
                        pos.side
                            .sign()
                            .checked_mul(slip)
                            .ok_or("slippage overflow")?,
                    )
                    .ok_or("fill overflow")?
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
        // Signals observed at publication execute only at a strictly subsequent event.
        while candle_i < candles.len() && candles[candle_i].publication_sequence < t.sequence {
            let wants = mask.get(candle_i);
            candle_i += 1;
            if !wants || position.is_some() || closed_now {
                continue;
            }
            let entry = if strategy.side == Side::Long {
                ask.checked_add(slip)
            } else {
                bid.checked_sub(slip)
            }
            .ok_or("entry overflow")?;
            let stop = entry
                .checked_sub(
                    strategy
                        .side
                        .sign()
                        .checked_mul(distance)
                        .ok_or("stop overflow")?,
                )
                .ok_or("stop overflow")?;
            let target = entry
                .checked_add(
                    strategy
                        .side
                        .sign()
                        .checked_mul(distance.checked_mul(3).ok_or("target overflow")?)
                        .ok_or("target overflow")?,
                )
                .ok_or("target overflow")?;
            if entry <= 0 || stop <= 0 || target <= 0 {
                rejections.push(Rejection {
                    event: t.sequence,
                    reason: "nonpositive protective level".into(),
                });
                continue;
            }
            let risk_distance = distance.checked_add(slip).ok_or("risk overflow")?;
            match contracts::size(p, balance, entry, risk_distance, fx) {
                Ok(volume) => {
                    let fee = costs.commission(volume);
                    let planned_loss = -contracts::pnl(p, -risk_distance, volume, fx)? + fee * 2;
                    if planned_loss > balance / 10 {
                        rejections.push(Rejection {
                            event: t.sequence,
                            reason: "commissions exceed risk budget".into(),
                        });
                        continue;
                    }
                    balance -= fee;
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
                        volume_micro_lots: volume,
                        entry_commission: fee,
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
    Ok(Replay { version: "ordered-l1-v1".into(), strategy_id: strategy.identity()?, dataset_id: data.manifest.content_hash.clone(),
        profile_id: data.manifest.profile_hash.clone(), cost_id: crate::data::hash(costs)?,
        fill_assumption: "L1 full-fill research assumption; no historical depth/queue verification; target limit at requested price; deadline cancels protective orders; USD-per-quote marks supplied explicitly".into(),
        initial_micro_usd: contracts::INITIAL_EQUITY, realized_micro_usd: balance, final_marked_micro_usd: marked,
        maximum_drawdown_micro_usd: drawdown, trades, rejections, unresolved: position })
}
