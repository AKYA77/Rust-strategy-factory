//! Integer contracts. Prices use profile units; money is USD millionths, volume micro-lots.
use crate::Result;
use serde::{Deserialize, Serialize};

pub const INITIAL_EQUITY: i128 = 20_000_000;
pub const MAX_HOLD_MS: i64 = 86_400_000;
/// Final owner-approved research scope (V2 mandate, 2026-11-24): seven FX pairs.
/// XAUUSD was removed from the active scope; gold submissions are rejected and
/// project-owned gold-only data was deleted. Historical eight-symbol receipts are
/// retained as immutable audit evidence but are superseded, not current.
pub const SYMBOLS: [&str; 7] = [
    "EURUSD", "GBPUSD", "USDJPY", "USDCHF", "USDCAD", "AUDUSD", "NZDUSD",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub symbol: String,
    pub version: String,
    pub base: String,
    pub quote: String,
    pub price_scale: i64,
    pub tick: i64,
    pub pip: i64,
    pub contract_units: i64,
    pub min_micro_lots: i64,
    pub step_micro_lots: i64,
    pub max_micro_lots: i64,
    pub leverage: i64,
    pub min_stop_ticks: i64,
    pub provenance: String,
    pub research_only: bool,
}
impl Profile {
    /// Explicit deterministic SOFTWARE TEST conventions, never broker specifications.
    pub fn synthetic(symbol: &str) -> Result<Self> {
        if !SYMBOLS.contains(&symbol) {
            return Err("unsupported instrument".into());
        }
        let jpy = symbol == "USDJPY";
        Ok(Self {
            symbol: symbol.into(),
            version: "synthetic-v1".into(),
            base: symbol[..3].into(),
            quote: symbol[3..].into(),
            price_scale: if jpy { 1000 } else { 100_000 },
            tick: 1,
            pip: 10,
            contract_units: 100_000,
            min_micro_lots: 10,
            step_micro_lots: 10,
            max_micro_lots: 1_000_000,
            leverage: 100,
            min_stop_ticks: 1,
            provenance: "SYNTHETIC_TEST_ONLY; arbitrary test volume/leverage conventions, never broker specifications or approved research parameters".into(),
            research_only: true,
        })
    }
    pub fn validate(&self) -> Result<()> {
        if !SYMBOLS.contains(&self.symbol.as_str())
            || self.base != self.symbol[..3]
            || self.quote != self.symbol[3..]
            || self.version.is_empty()
            || self.provenance.is_empty()
            || [
                self.price_scale,
                self.tick,
                self.pip,
                self.contract_units,
                self.min_micro_lots,
                self.step_micro_lots,
                self.max_micro_lots,
                self.leverage,
                self.min_stop_ticks,
            ]
            .iter()
            .any(|v| *v <= 0)
            || self.pip % self.tick != 0
            || self.min_micro_lots % self.step_micro_lots != 0
            || self.max_micro_lots < self.min_micro_lots
            || self.price_scale > 1_000_000_000
            || self.contract_units > 1_000_000_000
            || self.leverage > 100_000
            || self.max_micro_lots > 1_000_000_000
        {
            return Err("invalid instrument profile".into());
        }
        Ok(())
    }
    pub fn price(&self, decimal: &str) -> Result<i64> {
        self.validate()?;
        let (whole, frac) = decimal.split_once('.').unwrap_or((decimal, ""));
        if whole.is_empty()
            || !whole.bytes().all(|c| c.is_ascii_digit())
            || !frac.bytes().all(|c| c.is_ascii_digit())
            || frac.len() > 9
        {
            return Err("invalid exact decimal".into());
        }
        let denom = 10_i128.pow(frac.len() as u32);
        let w: i128 = whole.parse().map_err(|_| "price overflow")?;
        let f: i128 = if frac.is_empty() {
            0
        } else {
            frac.parse().map_err(|_| "price overflow")?
        };
        let units = w
            .checked_mul(denom)
            .and_then(|v| v.checked_add(f))
            .and_then(|v| v.checked_mul(self.price_scale.into()))
            .ok_or("price overflow")?;
        if units % denom != 0 {
            return Err("price precision exceeds profile".into());
        }
        let p = i64::try_from(units / denom).map_err(|_| "price overflow")?;
        if p <= 0 || p % self.tick != 0 {
            return Err("price is nonpositive or off grid".into());
        }
        Ok(p)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Long,
    Short,
}
impl Side {
    pub fn sign(self) -> i64 {
        if self == Self::Long { 1 } else { -1 }
    }
}

/// Prior-only USD per quote currency. Non-USD quotes require an explicit timestamped rate.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversion {
    pub usd_num: i64,
    pub quote_den: i64,
    pub as_of_ms: i64,
}
impl Conversion {
    pub fn usd() -> Self {
        Self {
            usd_num: 1,
            quote_den: 1,
            as_of_ms: i64::MIN,
        }
    }
    pub fn validate(self, p: &Profile, event_ms: i64, max_age_ms: i64) -> Result<()> {
        if self.usd_num <= 0
            || self.quote_den <= 0
            || self.usd_num > 1_000_000_000
            || self.quote_den > 1_000_000_000
            || self.as_of_ms > event_ms
            || (p.quote == "USD" && (self.usd_num != self.quote_den))
            // The identity sentinel is valid only for USD-quoted instruments;
            // a non-USD quote must carry a real, event-aligned observation.
            || (p.quote != "USD" && self.as_of_ms == i64::MIN)
            || (p.quote != "USD"
                && self.as_of_ms != i64::MIN
                && i128::from(event_ms) - i128::from(self.as_of_ms) > i128::from(max_age_ms))
        {
            return Err("missing, future, stale or invalid currency conversion".into());
        }
        Ok(())
    }
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    a
}

impl Conversion {
    /// Exact USD-account conversion for a USD-base instrument from its own quote.
    ///
    /// `price` is the direction-consistent closing price in profile units: the bid for a long
    /// (buy) position and the ask for a short (sell) position. The stored rate is USD per quote
    /// currency, `1 / decimal_price`, represented exactly as `price_scale / price` reduced to
    /// lowest terms. The rate is prior-only: `as_of_ms` is the quote's own event time.
    ///
    /// This is an explicit diagnostic research convention derived from the instrument's own
    /// verified quote, not an approved broker conversion rule. USD-quoted instruments must use
    /// [`Conversion::usd`] instead.
    pub fn usd_account_from_usd_base_price(p: &Profile, price: i64, event_ms: i64) -> Result<Self> {
        p.validate()?;
        if p.base != "USD" {
            return Err("USD-account base-quote conversion requires a USD-base instrument".into());
        }
        if p.quote == "USD" {
            return Err("USD-quoted instruments use the identity conversion".into());
        }
        if price <= 0 || price % p.tick != 0 {
            return Err("invalid conversion price".into());
        }
        let divisor = gcd(p.price_scale, price);
        let conversion = Self {
            usd_num: p.price_scale / divisor,
            quote_den: price / divisor,
            as_of_ms: event_ms,
        };
        conversion.validate(p, event_ms, 0)?;
        Ok(conversion)
    }
}

/// Adverse rounding: fractional losses round away from zero; profits toward zero.
pub fn pnl(p: &Profile, signed_price_delta: i64, volume: i64, fx: Conversion) -> Result<i128> {
    p.validate()?;
    if volume <= 0 || volume > p.max_micro_lots || fx.usd_num <= 0 || fx.quote_den <= 0 {
        return Err("invalid P&L inputs".into());
    }
    let n = i128::from(signed_price_delta)
        .checked_mul(p.contract_units.into())
        .and_then(|v| v.checked_mul(volume.into()))
        .and_then(|v| v.checked_mul(fx.usd_num.into()))
        .ok_or("P&L overflow")?;
    let d = i128::from(p.price_scale) * i128::from(fx.quote_den);
    Ok(n.div_euclid(d))
}

pub fn size(
    p: &Profile,
    equity: i128,
    entry: i64,
    stop_distance: i64,
    fx: Conversion,
) -> Result<i64> {
    p.validate()?;
    if equity <= 0
        || entry <= 0
        || stop_distance <= 0
        || fx.usd_num <= 0
        || fx.quote_den <= 0
        || fx.usd_num > 1_000_000_000
        || fx.quote_den > 1_000_000_000
    {
        return Err("invalid sizing inputs".into());
    }
    let budget = equity / 10;
    let denominator =
        i128::from(stop_distance) * i128::from(p.contract_units) * i128::from(fx.usd_num);
    let numerator = budget
        .checked_mul(p.price_scale.into())
        .and_then(|v| v.checked_mul(fx.quote_den.into()))
        .ok_or("sizing overflow")?;
    let by_risk = numerator / denominator;
    let by_margin = equity
        .checked_mul(p.leverage.into())
        .and_then(|v| v.checked_mul(p.price_scale.into()))
        .and_then(|v| v.checked_mul(fx.quote_den.into()))
        .ok_or("margin overflow")?
        / (i128::from(entry) * i128::from(p.contract_units) * i128::from(fx.usd_num));
    let lots = by_risk.min(by_margin).min(p.max_micro_lots.into()) / i128::from(p.step_micro_lots)
        * i128::from(p.step_micro_lots);
    if lots < p.min_micro_lots.into() {
        return Err("minimum volume infeasible under risk or margin".into());
    }
    Ok(lots as i64)
}
