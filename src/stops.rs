//! Initial protective distance preparation. ATR never reprices an existing order/position.
use crate::{Result, candles::Candle, contracts::Profile, strategies::Strategy};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Policy {
    Atr { window: usize, multiple_bps: u32 },
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Atr {
                window,
                multiple_bps,
            } if (2..=100_000).contains(window) && (1..=100_000).contains(multiple_bps) => Ok(()),
            _ => Err("invalid ATR stop policy".into()),
        }
    }
}
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Distance {
    Warmup,
    Ready(i64),
    ExceedsBudget,
}
impl Distance {
    pub fn rejection(self) -> &'static str {
        match self {
            Self::Warmup => "ATR stop warmup unavailable",
            Self::ExceedsBudget => "ATR stop exceeds distance representation/grammar budget",
            Self::Ready(_) => "stop distance ready",
        }
    }
}
#[derive(Debug)]
pub enum Prepared {
    Fixed(i64),
    Adaptive(Vec<Distance>),
}
impl Prepared {
    pub fn at(&self, index: usize) -> Distance {
        match self {
            Self::Fixed(distance) => Distance::Ready(*distance),
            Self::Adaptive(values) => values.get(index).copied().unwrap_or(Distance::Warmup),
        }
    }
}
fn ceil(n: i128, d: i128) -> i128 {
    n / d + i128::from(n % d != 0)
}
pub fn prepare(strategy: &Strategy, profile: &Profile, candles: &[Candle]) -> Result<Prepared> {
    profile.validate()?;
    if !(1..=1_000_000).contains(&strategy.stop_ticks) {
        return Err("stop floor outside grammar budget".into());
    }
    let Some(policy) = &strategy.stop_policy else {
        if strategy.stop_ticks < profile.min_stop_ticks {
            return Err("stop below profile minimum".into());
        }
        return Ok(Prepared::Fixed(
            strategy
                .stop_ticks
                .checked_mul(profile.tick)
                .ok_or("stop overflow")?,
        ));
    };
    policy.validate()?;
    let floor = strategy.stop_ticks.max(profile.min_stop_ticks).max(1) as i128;
    if floor > 1_000_000 {
        return Err("profile minimum exceeds stop grammar budget".into());
    }
    match policy {
        Policy::Atr {
            window,
            multiple_bps,
        } => {
            let values = crate::indicators::atr_outward(candles, *window)?;
            Ok(Prepared::Adaptive(
                values
                    .into_iter()
                    .map(|atr| {
                        let Some(atr) = atr else {
                            return Distance::Warmup;
                        };
                        let units = ceil(atr as i128 * *multiple_bps as i128, 10000);
                        let ticks = ceil(units, profile.tick as i128).max(floor);
                        if ticks > 1_000_000 {
                            return Distance::ExceedsBudget;
                        }
                        match i64::try_from(ticks * profile.tick as i128) {
                            Ok(distance) => Distance::Ready(distance),
                            Err(_) => Distance::ExceedsBudget,
                        }
                    })
                    .collect(),
            ))
        }
    }
}
