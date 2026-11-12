//! Fixed causal descriptive regimes. No fitted thresholds or future candles.
use crate::{Result, data::Dataset};
use std::collections::VecDeque;
pub const VERSION: &str = "prior-21-closed-m1-trend-range-v1";
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
}
#[derive(Debug)]
pub struct Classifier {
    events: Vec<Event>,
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
                let name = if bars.len() == 21 {
                    after_gap = false;
                    label(&bars)
                } else if after_gap {
                    "UNKNOWN_GAP"
                } else {
                    "UNKNOWN_WARMUP"
                };
                events.push(Event {
                    sequence: tick.sequence,
                    available_ms: tick.timestamp_ms,
                    label: name,
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
    /// Excludes candles published on the entry event itself, including timestamp ties.
    pub fn at(&self, entry_sequence: u64, entry_ms: i64) -> &'static str {
        let n = self.events.partition_point(|e| e.sequence < entry_sequence);
        let Some(index) = n.checked_sub(1) else {
            return "UNKNOWN_WARMUP";
        };
        let event = &self.events[index];
        match entry_ms.checked_sub(event.available_ms) {
            Some(age) if (0..=60000).contains(&age) => event.label,
            _ => "UNKNOWN_STALE",
        }
    }
}
