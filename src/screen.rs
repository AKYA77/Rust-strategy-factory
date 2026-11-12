//! Conservative fixed-position barrier sets from actual bid/ask candles.
//! These are conditional price-barrier bounds, not net win rates or complete strategy backtests.
use crate::{
    Result,
    candles::Candle,
    contracts::{Profile, Side},
    data::Tick,
    tick_engine::Costs,
};
use serde::{Deserialize, Serialize};
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Possible {
    NoHit,
    StopMarket,
    TargetLimit,
    DeadlineMarket,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bounds {
    pub version: String,
    pub possible: Vec<Possible>,
    pub entry_or_deadline_uncertain: bool,
    pub sl_tp_collision: bool,
}
#[derive(Debug, Copy, Clone)]
pub struct Position {
    pub side: Side,
    pub entry_ms: i64,
    pub stop: i64,
    pub target: i64,
    pub deadline_ms: i64,
}
pub fn barrier_bounds(p: &Profile, costs: &Costs, c: &Candle, pos: Position) -> Result<Bounds> {
    p.validate()?;
    if pos.entry_ms >= pos.deadline_ms
        || pos.stop <= 0
        || pos.target <= 0
        || pos.stop % p.tick != 0
        || pos.target % p.tick != 0
        || c.events == 0
        || c.start_ms >= c.available_ms
        || pos.side == Side::Long && pos.stop >= pos.target
        || pos.side == Side::Short && pos.stop <= pos.target
    {
        return Err("invalid fixed-barrier query".into());
    }
    for o in [&c.bid, &c.ask] {
        if o.low <= 0
            || o.low > o.open
            || o.low > o.close
            || o.high < o.open
            || o.high < o.close
            || [o.open, o.high, o.low, o.close]
                .iter()
                .any(|v| v % p.tick != 0)
        {
            return Err("invalid candle OHLC".into());
        }
    }
    let make = |bid, ask| Tick {
        timestamp_ms: c.start_ms,
        sequence: 0,
        bid,
        ask,
    };
    let (_, open_ask) = costs.effective(p, make(c.bid.open, c.ask.open))?;
    let (_, close_ask) = costs.effective(p, make(c.bid.close, c.ask.close))?;
    if c.ask.low < c.bid.low || c.ask.high < c.bid.high {
        return Err("inconsistent uncrossed quote extrema".into());
    }
    let floor = costs
        .spread_floor_pips
        .checked_mul(p.pip)
        .ok_or("spread overflow")?;
    let low_floor = c.bid.low.checked_add(floor).ok_or("spread overflow")?;
    let high_floor = c.bid.high.checked_add(floor).ok_or("spread overflow")?;
    let ask_min_lower = c.ask.low.max(low_floor);
    let ask_min_upper = c
        .ask
        .high
        .max(low_floor)
        .min(c.ask.low.max(high_floor))
        .min(open_ask)
        .min(close_ask);
    let ask_max = c.ask.high.max(high_floor);
    let (stop_possible, target_possible, target_certain) = match pos.side {
        Side::Long => (
            c.bid.low <= pos.stop,
            c.bid.high >= pos.target,
            c.bid.high >= pos.target,
        ),
        Side::Short => (
            ask_max >= pos.stop,
            ask_min_lower <= pos.target,
            ask_min_upper <= pos.target,
        ),
    };
    let collision = stop_possible && target_possible;
    let uncertain = c.start_ms <= pos.entry_ms || c.available_ms > pos.deadline_ms;
    let mut possible = Vec::new();
    if c.start_ms >= pos.deadline_ms {
        possible.push(Possible::DeadlineMarket);
    } else {
        if stop_possible {
            possible.push(Possible::StopMarket);
        }
        if target_possible {
            possible.push(Possible::TargetLimit);
        }
        if uncertain || !stop_possible && !target_certain {
            possible.push(Possible::NoHit);
        }
        if c.available_ms > pos.deadline_ms {
            possible.push(Possible::DeadlineMarket);
        }
    }
    Ok(Bounds {
        version: "conditional-barrier-bounds-v1".into(),
        possible,
        entry_or_deadline_uncertain: uncertain,
        sl_tp_collision: collision,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Audit {
    pub audited_trades: usize,
    pub uncovered_trades: usize,
    pub collision_bars: usize,
    pub conditional_ambiguities: usize,
}
/// Compare bounds against actual replay receipts. Unclosed final bars stay explicitly uncovered.
pub fn audit_replay(
    data: &crate::data::Dataset,
    costs: &Costs,
    replay: &crate::tick_engine::Replay,
) -> Result<Audit> {
    let bars = crate::candles::build(data, crate::candles::Timeframe::M1)?;
    let mut audit = Audit {
        audited_trades: 0,
        uncovered_trades: 0,
        collision_bars: 0,
        conditional_ambiguities: 0,
    };
    for trade in &replay.trades {
        let n = bars.partition_point(|c| c.start_ms <= trade.exit_ms);
        let Some(c) = n
            .checked_sub(1)
            .map(|i| &bars[i])
            .filter(|c| trade.exit_ms < i64::saturating_add(c.start_ms, 60000))
        else {
            audit.uncovered_trades += 1;
            continue;
        };
        let bounds = barrier_bounds(
            &data.profile,
            costs,
            c,
            Position {
                side: trade.side,
                entry_ms: trade.entry_ms,
                stop: trade.stop,
                target: trade.target,
                deadline_ms: trade
                    .entry_ms
                    .checked_add(crate::contracts::MAX_HOLD_MS)
                    .ok_or("deadline overflow")?,
            },
        )?;
        let actual = match trade.reason {
            crate::tick_engine::ExitReason::StopMarket => Possible::StopMarket,
            crate::tick_engine::ExitReason::TargetLimit => Possible::TargetLimit,
            crate::tick_engine::ExitReason::DeadlineMarket => Possible::DeadlineMarket,
        };
        if !bounds.possible.contains(&actual) {
            return Err("candle barrier bounds excluded the actual sequential fill; optimization must remain disabled".into());
        }
        audit.audited_trades += 1;
        audit.collision_bars += usize::from(bounds.sl_tp_collision);
        audit.conditional_ambiguities += usize::from(bounds.possible.len() > 1);
    }
    Ok(audit)
}
