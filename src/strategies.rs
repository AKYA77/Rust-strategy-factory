use crate::{
    Result,
    candles::{Candle, Timeframe},
    contracts::Side,
    data::hash,
    indicators::{Mask, sma},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Rule {
    AboveMean { window: usize },
    BelowMean { window: usize },
    Rising { lookback: usize },
    Falling { lookback: usize },
    All(Vec<Rule>),
    Any(Vec<Rule>),
}
impl Rule {
    pub fn canonical(&self) -> Result<Self> {
        fn normalize(rule: &Rule, depth: usize, nodes: &mut usize) -> Result<Rule> {
            *nodes += 1;
            if depth > 4 || *nodes > 32 {
                return Err("rule complexity exceeds budget".into());
            }
            match rule {
                Rule::All(items) | Rule::Any(items) => {
                    if items.is_empty() || items.len() > 8 {
                        return Err("Boolean arity out of bounds".into());
                    }
                    let is_all = matches!(rule, Rule::All(_));
                    let mut v = Vec::new();
                    for item in items {
                        let child = normalize(item, depth + 1, nodes)?;
                        match child {
                            Rule::All(children) if is_all => v.extend(children),
                            Rule::Any(children) if !is_all => v.extend(children),
                            x => v.push(x),
                        }
                    }
                    v.sort();
                    v.dedup();
                    if v.len() == 1 {
                        return Ok(v.remove(0));
                    }
                    Ok(if is_all { Rule::All(v) } else { Rule::Any(v) })
                }
                Rule::AboveMean { window } | Rule::BelowMean { window } => {
                    if !(2..=100_000).contains(window) {
                        return Err("invalid mean lookback".into());
                    }
                    Ok(rule.clone())
                }
                Rule::Rising { lookback } | Rule::Falling { lookback } => {
                    if !(1..=100_000).contains(lookback) {
                        return Err("invalid momentum lookback".into());
                    }
                    Ok(rule.clone())
                }
            }
        }
        normalize(self, 0, &mut 0)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Strategy {
    pub timeframe: Timeframe,
    pub side: Side,
    pub stop_ticks: i64,
    pub rule: Rule,
}
impl Strategy {
    pub fn canonical(&self) -> Result<Self> {
        if self.stop_ticks <= 0 || self.stop_ticks > 1_000_000 {
            return Err("stop distance out of bounds".into());
        }
        Ok(Self {
            rule: self.rule.canonical()?,
            ..self.clone()
        })
    }
    pub fn identity(&self) -> Result<String> {
        hash(&("strategy-v1", self.canonical()?))
    }
}
/// A batch shares all SMA computations. Budget counts retained feature cells.
pub struct Features {
    close: Vec<i64>,
    means: BTreeMap<usize, Vec<Option<i64>>>,
    max_cells: usize,
}
impl Features {
    pub fn new(candles: &[Candle], max_cells: usize) -> Result<Self> {
        if candles.len() > max_cells {
            return Err("feature budget exceeded".into());
        }
        Ok(Self {
            close: candles.iter().map(|c| c.bid.close).collect(),
            means: BTreeMap::new(),
            max_cells,
        })
    }
    pub fn signal(&mut self, rule: &Rule) -> Result<Mask> {
        self.evaluate(&rule.canonical()?)
    }
    fn evaluate(&mut self, rule: &Rule) -> Result<Mask> {
        match rule {
            Rule::AboveMean { window } | Rule::BelowMean { window } => {
                if !self.means.contains_key(window) {
                    if (self.means.len() + 2).saturating_mul(self.close.len()) > self.max_cells {
                        return Err("shared feature budget exceeded".into());
                    }
                    self.means.insert(*window, sma(&self.close, *window)?);
                }
                let above = matches!(rule, Rule::AboveMean { .. });
                Ok(Mask::from_bools(
                    self.close
                        .iter()
                        .zip(&self.means[window])
                        .map(|(v, avg)| avg.is_some_and(|a| if above { *v > a } else { *v < a })),
                ))
            }
            Rule::Rising { lookback } | Rule::Falling { lookback } => {
                let rising = matches!(rule, Rule::Rising { .. });
                Ok(Mask::from_bools(self.close.iter().enumerate().map(
                    |(i, v)| {
                        i >= *lookback
                            && if rising {
                                *v > self.close[i - lookback]
                            } else {
                                *v < self.close[i - lookback]
                            }
                    },
                )))
            }
            Rule::All(v) | Rule::Any(v) => {
                let mut acc = self.evaluate(&v[0])?;
                for r in &v[1..] {
                    acc = acc.combine(&self.evaluate(r)?, matches!(rule, Rule::All(_)))?;
                }
                Ok(acc)
            }
        }
    }
}
/// Deterministic initial trend/momentum/mean-reversion grid. Other mandatory families pending.
pub fn generate() -> Vec<Strategy> {
    let mut out = Vec::new();
    for tf in Timeframe::ALL {
        for side in [Side::Long, Side::Short] {
            for window in [5, 10, 20, 50] {
                for stop in [10, 30, 100] {
                    for rule in [
                        Rule::AboveMean { window },
                        Rule::BelowMean { window },
                        Rule::Rising { lookback: window },
                        Rule::Falling { lookback: window },
                    ] {
                        out.push(Strategy {
                            timeframe: tf,
                            side,
                            stop_ticks: stop,
                            rule,
                        });
                    }
                }
            }
        }
    }
    out
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenStatus {
    ProvenNoSignals,
    ScreenUncertain,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Screen {
    pub version: String,
    pub status: ScreenStatus,
    pub signals: usize,
    pub requires_tick_verification: bool,
}
/// Initial conservative policy: never reject any candidate with signals.
/// Candle outcome estimation and audited failure bounds are pending.
pub fn screen(features: &mut Features, strategy: &Strategy) -> Result<Screen> {
    let signals = features.signal(&strategy.rule)?.count();
    Ok(Screen {
        version: "conservative-signals-v1".into(),
        status: if signals == 0 {
            ScreenStatus::ProvenNoSignals
        } else {
            ScreenStatus::ScreenUncertain
        },
        signals,
        requires_tick_verification: signals > 0,
    })
}
