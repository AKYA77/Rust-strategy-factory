//! Initial margin and risk reservation v2. USD millionths; exact outward rounding.
use crate::{
    Result,
    contracts::{self, Conversion, Profile, Side},
    tick_engine::Costs,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reservation {
    pub volume: i64,
    pub intended_loss_micro_usd: i128,
    pub initial_margin_micro_usd: i128,
    pub entry_fee_micro_usd: i128,
    pub equity_after_entry_mark_micro_usd: i128,
}
pub fn margin(p: &Profile, entry: i64, volume: i64, fx: Conversion) -> Result<i128> {
    p.validate()?;
    if entry <= 0
        || volume <= 0
        || volume > p.max_micro_lots
        || fx.usd_num <= 0
        || fx.quote_den <= 0
        || fx.usd_num > 1_000_000_000
        || fx.quote_den > 1_000_000_000
    {
        return Err("invalid margin inputs".into());
    }
    let n = i128::from(entry)
        .checked_mul(p.contract_units.into())
        .and_then(|v| v.checked_mul(volume.into()))
        .and_then(|v| v.checked_mul(fx.usd_num.into()))
        .ok_or("margin overflow")?;
    let d = i128::from(p.price_scale) * i128::from(fx.quote_den) * i128::from(p.leverage);
    Ok(n / d + i128::from(n % d != 0))
}
/// Entry mark is the opposite executable side, with entry costs already in entry price.
/// The stop distance includes adverse exit slippage; costs include both commissions.
#[derive(Debug, Clone, Copy)]
pub struct Request {
    pub equity: i128,
    pub entry: i64,
    pub mark: i64,
    pub side: Side,
    pub stop_with_exit_slip: i64,
    pub conversion: Conversion,
}
pub fn reserve(p: &Profile, costs: &Costs, r: Request, volume: i64) -> Result<Reservation> {
    p.validate()?;
    costs.validate()?;
    if r.equity <= 0
        || r.entry <= 0
        || r.mark <= 0
        || r.stop_with_exit_slip <= 0
        || volume < p.min_micro_lots
        || volume > p.max_micro_lots
        || volume % p.step_micro_lots != 0
        || costs.commission_micro_usd_per_lot_per_fill < 0
    {
        return Err("invalid risk request or volume".into());
    }
    let delta = r
        .mark
        .checked_sub(r.entry)
        .and_then(|d| d.checked_mul(r.side.sign()))
        .ok_or("mark overflow")?;
    if delta > 0 {
        return Err("initial executable mark cannot create a favorable entry profit".into());
    }
    let raw_stop = r
        .stop_with_exit_slip
        .checked_sub(
            costs
                .slippage_pips
                .checked_mul(p.pip)
                .ok_or("slippage overflow")?,
        )
        .ok_or("stop distance overflow")?;
    let minimum = p
        .min_stop_ticks
        .max(1)
        .checked_mul(p.tick)
        .ok_or("minimum stop distance overflow")?;
    let from_executable = raw_stop
        .checked_add(delta)
        .ok_or("protective distance overflow")?;
    if from_executable < minimum {
        return Err(
            "initial protective stop is crossed or violates executable-side minimum distance"
                .into(),
        );
    }
    let fee = costs.commission(volume);
    let loss = contracts::pnl(p, -r.stop_with_exit_slip, volume, r.conversion)?
        .checked_neg()
        .and_then(|v| v.checked_add(fee * 2))
        .ok_or("risk overflow")?;
    let marked = r
        .equity
        .checked_add(contracts::pnl(p, delta, volume, r.conversion)?)
        .and_then(|v| v.checked_sub(fee))
        .ok_or("marked equity overflow")?;
    let initial_margin = margin(p, r.entry, volume, r.conversion)?;
    if loss > r.equity / 10 || initial_margin > marked {
        return Err("fixed quantity exceeds current risk budget or marked initial margin".into());
    }
    Ok(Reservation {
        volume,
        intended_loss_micro_usd: loss,
        initial_margin_micro_usd: initial_margin,
        entry_fee_micro_usd: fee,
        equity_after_entry_mark_micro_usd: marked,
    })
}
/// Monotone integer search accounts for fee rounding, post-fill equity and volume steps.
pub fn size(p: &Profile, costs: &Costs, r: Request) -> Result<Reservation> {
    costs.validate()?;
    reserve(p, costs, r, p.min_micro_lots)?;
    let upper = contracts::size(p, r.equity, r.entry, r.stop_with_exit_slip, r.conversion)?;
    let (mut low, mut high) = (
        p.min_micro_lots / p.step_micro_lots - 1,
        upper / p.step_micro_lots,
    );
    while low < high {
        let mid = low + (high - low + 1) / 2;
        if reserve(p, costs, r, mid * p.step_micro_lots).is_ok() {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    if low * p.step_micro_lots < p.min_micro_lots {
        return Err("minimum volume infeasible after costs and marked margin".into());
    }
    reserve(p, costs, r, low * p.step_micro_lots)
}
