//! Deterministic L1 entry-order lifecycle shared with authoritative replay.
//! Full fills are a research assumption: quotes do not establish depth or queue position.
use crate::{
    Result,
    contracts::{Profile, Side},
    data::Tick,
    tick_engine::Costs,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Market,
    StopMarket { trigger: i64 },
    Limit { limit: i64 },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fill {
    pub sequence: u64,
    pub timestamp_ms: i64,
    pub price: i64,
    pub volume_micro_lots: i64,
    pub side: Side,
    pub kind: Kind,
    pub model: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    Pending,
    Filled(Fill),
    Cancelled {
        at_ms: i64,
    },
    Expired {
        deadline_ms: i64,
        observed_at_ms: i64,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    side: Side,
    kind: Kind,
    volume_micro_lots: i64,
    submitted_ms: i64,
    submitted_sequence: u64,
    expires_ms: i64,
    state: State,
    last_ms: i64,
    last_sequence: u64,
    last_quote: Tick,
    profile_hash: String,
    cost_hash: String,
}
/// A market buy pays effective ask plus adverse slippage; a sell receives bid minus slippage.
pub fn market_price(p: &Profile, costs: &Costs, tick: Tick, side: Side) -> Result<i64> {
    let (bid, ask) = costs.effective(p, tick)?;
    market_from_effective(p, costs, bid, ask, side)
}
/// Internal callers already validated the profile/costs through Costs::effective for this event.
pub(crate) fn market_from_effective(
    p: &Profile,
    costs: &Costs,
    bid: i64,
    ask: i64,
    side: Side,
) -> Result<i64> {
    let slip = costs
        .slippage_pips
        .checked_mul(p.pip)
        .ok_or("slippage overflow")?;
    let price = match side {
        Side::Long => ask.checked_add(slip),
        Side::Short => bid.checked_sub(slip),
    }
    .ok_or("fill price overflow")?;
    if price <= 0 || price % p.tick != 0 {
        return Err("nonpositive or off-grid fill price".into());
    }
    Ok(price)
}
impl Order {
    pub fn submit(
        p: &Profile,
        costs: &Costs,
        side: Side,
        kind: Kind,
        volume_micro_lots: i64,
        submitted: Tick,
        expires_ms: i64,
    ) -> Result<Self> {
        costs.effective(p, submitted)?;
        if volume_micro_lots < p.min_micro_lots
            || volume_micro_lots > p.max_micro_lots
            || volume_micro_lots % p.step_micro_lots != 0
            || expires_ms <= submitted.timestamp_ms
        {
            return Err("invalid order volume or expiry".into());
        }
        let level = match kind {
            Kind::Market => None,
            Kind::StopMarket { trigger } => Some(trigger),
            Kind::Limit { limit } => Some(limit),
        };
        if level.is_some_and(|l| l <= 0 || l % p.tick != 0) {
            return Err("invalid order price grid".into());
        }
        Ok(Self {
            side,
            kind,
            volume_micro_lots,
            submitted_ms: submitted.timestamp_ms,
            submitted_sequence: submitted.sequence,
            expires_ms,
            state: State::Pending,
            last_ms: submitted.timestamp_ms,
            last_sequence: submitted.sequence,
            last_quote: submitted,
            profile_hash: crate::data::hash(p)?,
            cost_hash: crate::data::hash(costs)?,
        })
    }
    pub fn state(&self) -> &State {
        &self.state
    }
    pub fn cancel(&mut self, now_ms: i64) -> Result<bool> {
        if now_ms < self.last_ms {
            return Err("cancellation predates observed events".into());
        }
        if !matches!(self.state, State::Pending) {
            return Ok(false);
        }
        if now_ms >= self.expires_ms {
            self.advance_clock(now_ms)?;
            return Ok(false);
        }
        self.state = State::Cancelled { at_ms: now_ms };
        self.last_ms = now_ms;
        Ok(true)
    }
    /// Orders may expire without a quote; this never creates an execution price.
    pub fn advance_clock(&mut self, now_ms: i64) -> Result<()> {
        if now_ms < self.last_ms {
            return Err("clock moved backwards".into());
        }
        if now_ms >= self.expires_ms && matches!(self.state, State::Pending) {
            self.state = State::Expired {
                deadline_ms: self.expires_ms,
                observed_at_ms: now_ms,
            };
        }
        self.last_ms = now_ms;
        Ok(())
    }
    /// Returns a receipt only for a new fill. Re-observing an event cannot fill twice.
    pub fn on_quote(&mut self, p: &Profile, costs: &Costs, tick: Tick) -> Result<Option<Fill>> {
        if self.profile_hash != crate::data::hash(p)? || self.cost_hash != crate::data::hash(costs)?
        {
            return Err("order profile/cost changed after submission".into());
        }
        let (bid, ask) = costs.effective(p, tick)?;
        if tick.sequence < self.last_sequence || tick.timestamp_ms < self.last_ms {
            return Err("out-of-order quote for order".into());
        }
        if tick.sequence == self.last_sequence {
            if tick != self.last_quote {
                return Err("conflicting quote for an observed sequence".into());
            }
            return Ok(None);
        }
        self.advance_clock(tick.timestamp_ms)?;
        self.last_sequence = tick.sequence;
        self.last_quote = tick;
        if !matches!(self.state, State::Pending) {
            return Ok(None);
        }
        let eligible = match self.kind {
            Kind::Market => true,
            Kind::StopMarket { trigger } => match self.side {
                Side::Long => ask >= trigger,
                Side::Short => bid <= trigger,
            },
            Kind::Limit { limit } => match self.side {
                Side::Long => ask <= limit,
                Side::Short => bid >= limit,
            },
        };
        if !eligible {
            return Ok(None);
        }
        let price = if matches!(self.kind, Kind::Limit { .. }) {
            if self.side == Side::Long { ask } else { bid }
        } else {
            market_price(p, costs, tick, self.side)?
        };
        let fill=Fill {sequence:tick.sequence,timestamp_ms:tick.timestamp_ms,price,volume_micro_lots:self.volume_micro_lots,side:self.side,kind:self.kind,
            model:"entry-l1-full-fill-v1; price-taking quote assumption; no queue/depth evidence; limit eligible at observed executable quote without market slippage".into()};
        self.state = State::Filled(fill.clone());
        Ok(Some(fill))
    }
}
