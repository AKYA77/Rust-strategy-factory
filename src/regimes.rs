//! Fixed causal descriptive regimes. No fitted thresholds or future candles.
use crate::{Result, data::Dataset};
use std::collections::VecDeque;
pub const VERSION: &str = "prior-21-closed-m1-trend-range-momentum-v2";
#[derive(Debug, Clone, Copy)]
struct Bar {
    start: i64,
    close: i64,
    high: i64,
    low: i64,
}
#[derive(Debug)]
struct Event {
    sequence: u64,
    available_ms: i64,
    label: &'static str,
    momentum: &'static str,
}
/// Descriptive regime at one entry: fixed trend/range bins plus an orthogonal fixed
/// displacement bin, both derived only from strictly prior closed M1 bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Regime {
    pub trend_range: &'static str,
    pub momentum: &'static str,
}
fn label(bars: &VecDeque<Bar>) -> &'static str {
    let last = bars.back().unwrap();
    let (closes, ranges) = bars.iter().take(20).fold((0_i128, 0_i128), |(c, r), b| {
        (c + b.close as i128, r + (b.high - b.low) as i128)
    });
    let trend = (last.close as i128 * 20).cmp(&closes);
    let range = ((last.high - last.low) as i128 * 20).cmp(&ranges);
    use std::cmp::Ordering::{Equal as E, Greater as G, Less as L};
    match (trend, range) {
        (G, G) => "UP_EXPANDED",
        (G, L) => "UP_CONTRACTED",
        (G, E) => "UP_EQUAL_RANGE",
        (L, G) => "DOWN_EXPANDED",
        (L, L) => "DOWN_CONTRACTED",
        (L, E) => "DOWN_EQUAL_RANGE",
        (E, G) => "FLAT_EXPANDED",
        (E, L) => "FLAT_CONTRACTED",
        (E, E) => "FLAT_EQUAL_RANGE",
    }
}
/// Signed net displacement across the 21-bar window relative to the mean completed-bar
/// range. The 2x impulse and 0.5x drift constants are fixed descriptive definitions,
/// never fitted to outcomes; zero-width windows fall back to direction-only bins.
fn momentum(bars: &VecDeque<Bar>) -> &'static str {
    let last = bars.back().unwrap();
    let first = bars.front().unwrap();
    let mean_range = bars
        .iter()
        .take(20)
        .fold(0_i128, |total, b| total + (b.high - b.low) as i128)
        / 20;
    let displacement = (last.close - first.close) as i128;
    let impulse = mean_range * 2;
    let drift = mean_range / 2;
    if displacement > 0 && displacement >= impulse {
        "UP_IMPULSE"
    } else if displacement < 0 && displacement <= -impulse {
        "DOWN_IMPULSE"
    } else if displacement > 0 && displacement >= drift {
        "UP_DRIFT"
    } else if displacement < 0 && displacement <= -drift {
        "DOWN_DRIFT"
    } else {
        "FLAT_CHOP"
    }
}
#[derive(Debug)]
pub struct Classifier {
    events: Vec<Event>,
}
impl Classifier {
    pub fn new(data: &Dataset) -> Result<Self> {
        data.verify()?;
        if data.ticks.len() > 2_000_000 {
            return Err("regime event budget exceeded".into());
        }
        let bucket = |timestamp: i64| {
            timestamp
                .div_euclid(60000)
                .checked_mul(60000)
                .ok_or_else(|| "regime bucket overflow".to_string())
        };
        let first = data.ticks[0];
        let mut current = Bar {
            start: bucket(first.timestamp_ms)?,
            close: first.bid,
            high: first.bid,
            low: first.bid,
        };
        let mut events = Vec::new();
        let mut bars = VecDeque::with_capacity(21);
        let mut after_gap = false;
        for tick in &data.ticks[1..] {
            let start = bucket(tick.timestamp_ms)?;
            if start > current.start {
                if start.checked_sub(current.start) != Some(60000) {
                    bars.clear();
                    after_gap = true;
                } else {
                    if bars.len() == 21 {
                        bars.pop_front();
                    }
                    bars.push_back(current);
                }
                let (name, state) = if bars.len() == 21 {
                    after_gap = false;
                    (label(&bars), momentum(&bars))
                } else if after_gap {
                    ("UNKNOWN_GAP", "UNKNOWN_GAP")
                } else {
                    ("UNKNOWN_WARMUP", "UNKNOWN_WARMUP")
                };
                events.push(Event {
                    sequence: tick.sequence,
                    available_ms: tick.timestamp_ms,
                    label: name,
                    momentum: state,
                });
                current = Bar {
                    start,
                    close: tick.bid,
                    high: tick.bid,
                    low: tick.bid,
                };
            } else {
                current.close = tick.bid;
                current.high = current.high.max(tick.bid);
                current.low = current.low.min(tick.bid);
            }
        }
        Ok(Self { events })
    }
    /// Trend/range bins only. Excludes candles published on the entry event itself,
    /// including timestamp ties.
    pub fn at(&self, entry_sequence: u64, entry_ms: i64) -> &'static str {
        self.profile(entry_sequence, entry_ms).trend_range
    }
    /// Orthogonal displacement bins over the same strictly prior window and freshness gate.
    pub fn state(&self, entry_sequence: u64, entry_ms: i64) -> &'static str {
        self.profile(entry_sequence, entry_ms).momentum
    }
    /// Both descriptive dimensions for one entry; unknown/stale states repeat on both.
    pub fn profile(&self, entry_sequence: u64, entry_ms: i64) -> Regime {
        let n = self.events.partition_point(|e| e.sequence < entry_sequence);
        let Some(index) = n.checked_sub(1) else {
            return Regime {
                trend_range: "UNKNOWN_WARMUP",
                momentum: "UNKNOWN_WARMUP",
            };
        };
        let event = &self.events[index];
        match entry_ms.checked_sub(event.available_ms) {
            Some(age) if (0..=60000).contains(&age) => Regime {
                trend_range: event.label,
                momentum: event.momentum,
            },
            _ => Regime {
                trend_range: "UNKNOWN_STALE",
                momentum: "UNKNOWN_STALE",
            },
        }
    }
}
#[cfg(test)]
mod tests {
    use super::{Bar, momentum};
    use std::collections::VecDeque;
    fn window(closes: &[i64], half_range: i64) -> VecDeque<Bar> {
        closes
            .iter()
            .enumerate()
            .map(|(index, close)| Bar {
                start: index as i64 * 60000,
                close: *close,
                high: close + half_range,
                low: close - half_range,
            })
            .collect()
    }
    #[test]
    fn momentum_bins_use_fixed_descriptive_thresholds() {
        let rising: Vec<i64> = (0..21).map(|i| 1000 + i * 10).collect();
        assert_eq!(momentum(&window(&rising, 5)), "UP_IMPULSE");
        let falling: Vec<i64> = (0..21).map(|i| 1000 - i * 10).collect();
        assert_eq!(momentum(&window(&falling, 5)), "DOWN_IMPULSE");
        // Net +60 against a mean completed-bar range of 100: past the 0.5x drift line
        // but below the 2x impulse line.
        let drifting: Vec<i64> = (0..21).map(|i| 1000 + i * 3).collect();
        assert_eq!(momentum(&window(&drifting, 50)), "UP_DRIFT");
        let flat = vec![1000; 21];
        assert_eq!(momentum(&window(&flat, 0)), "FLAT_CHOP");
        // Zero-width bars leave direction-only bins rather than dividing by zero.
        assert_eq!(momentum(&window(&rising, 0)), "UP_IMPULSE");
    }
}
