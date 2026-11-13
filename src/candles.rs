use crate::{
    Result,
    data::{Dataset, Tick},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Timeframe {
    M1,
    M5,
    M15,
    M30,
    H1,
    H4,
    D1,
}
impl Timeframe {
    pub const ALL: [Self; 7] = [
        Self::M1,
        Self::M5,
        Self::M15,
        Self::M30,
        Self::H1,
        Self::H4,
        Self::D1,
    ];
    pub fn millis(self) -> i64 {
        match self {
            Self::M1 => 60_000,
            Self::M5 => 300_000,
            Self::M15 => 900_000,
            Self::M30 => 1_800_000,
            Self::H1 => 3_600_000,
            Self::H4 => 14_400_000,
            Self::D1 => 86_400_000,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ohlc {
    pub open: i64,
    pub high: i64,
    pub low: i64,
    pub close: i64,
}
impl Ohlc {
    fn new(price: i64) -> Self {
        Self {
            open: price,
            high: price,
            low: price,
            close: price,
        }
    }
    fn push(&mut self, price: i64) {
        self.high = self.high.max(price);
        self.low = self.low.min(price);
        self.close = price;
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candle {
    pub start_ms: i64,
    pub available_ms: i64,
    pub publication_sequence: u64,
    pub bid: Ohlc,
    pub ask: Ohlc,
    pub events: usize,
}
fn new(t: Tick, tf: Timeframe) -> Result<Candle> {
    let start = t
        .timestamp_ms
        .div_euclid(tf.millis())
        .checked_mul(tf.millis())
        .ok_or("bucket overflow")?;
    Ok(Candle {
        start_ms: start,
        available_ms: start.checked_add(tf.millis()).ok_or("bucket overflow")?,
        publication_sequence: t.sequence,
        bid: Ohlc::new(t.bid),
        ask: Ohlc::new(t.ask),
        events: 1,
    })
}
/// UTC continuous research calendar v1. No empty buckets, no unfinished final bucket.
/// A closed candle is published when a subsequent event proves its boundary was crossed.
/// This does not implement broker-session or DST calendars: profiles requiring those are blocked.
pub fn build(data: &Dataset, tf: Timeframe) -> Result<Vec<Candle>> {
    data.verify()?;
    let mut out = Vec::new();
    let mut current = new(data.ticks[0], tf)?;
    for t in &data.ticks[1..] {
        if t.timestamp_ms >= current.available_ms {
            current.available_ms = t.timestamp_ms;
            current.publication_sequence = t.sequence;
            out.push(current);
            current = new(*t, tf)?;
        } else {
            current.bid.push(t.bid);
            current.ask.push(t.ask);
            current.events += 1;
        }
    }
    Ok(out)
}
/// Last completed higher timeframe candle actually available at this event.
pub fn available(candles: &[Candle], timestamp_ms: i64, sequence: u64) -> Option<&Candle> {
    let n = candles
        .partition_point(|c| (c.available_ms, c.publication_sequence) <= (timestamp_ms, sequence));
    n.checked_sub(1).map(|i| &candles[i])
}
