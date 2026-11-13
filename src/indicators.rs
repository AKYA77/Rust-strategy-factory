//! Causal shared primitives. None means insufficient prior observations.
use crate::Result;
use serde::{Deserialize, Serialize};

pub fn sma(values: &[i64], window: usize) -> Result<Vec<Option<i64>>> {
    if window == 0 || window > 100_000 {
        return Err("lookback out of bounds".into());
    }
    let mut sum = 0_i128;
    let mut out = Vec::with_capacity(values.len());
    for (i, value) in values.iter().enumerate() {
        sum += i128::from(*value);
        if i >= window {
            sum -= i128::from(values[i - window]);
        }
        out.push(if i + 1 >= window {
            Some((sum / window as i128) as i64)
        } else {
            None
        });
    }
    Ok(out)
}
pub fn ema(values: &[i64], window: usize) -> Result<Vec<Option<f64>>> {
    let seed = sma(values, window)?;
    let alpha = 2.0 / (window + 1) as f64;
    let mut state = None;
    Ok(values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            state = match state {
                Some(prev) => Some(alpha * *v as f64 + (1.0 - alpha) * prev),
                None => seed[i].map(|x| x as f64),
            };
            state
        })
        .collect())
}
pub fn rsi(values: &[i64], window: usize) -> Result<Vec<Option<f64>>> {
    if window == 0 || window > 100_000 {
        return Err("lookback out of bounds".into());
    }
    let mut gain = 0.0;
    let mut loss = 0.0;
    let mut out = vec![None; values.len()];
    for i in 1..values.len() {
        let diff = values[i] as f64 - values[i - 1] as f64;
        let up = diff.max(0.0);
        let down = (-diff).max(0.0);
        if i <= window {
            gain += up;
            loss += down;
        } else {
            gain = gain * (window - 1) as f64 / window as f64 + up;
            loss = loss * (window - 1) as f64 / window as f64 + down;
        }
        if i >= window {
            out[i] = Some(if loss == 0.0 {
                if gain == 0.0 { 50.0 } else { 100.0 }
            } else {
                100.0 - 100.0 / (1.0 + gain / loss)
            });
        }
    }
    Ok(out)
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mask {
    words: Vec<u64>,
    len: usize,
}
impl Mask {
    pub fn from_bools(values: impl IntoIterator<Item = bool>) -> Self {
        let mut words = Vec::new();
        let mut len = 0;
        for b in values {
            if len % 64 == 0 {
                words.push(0);
            }
            if b {
                words[len / 64] |= 1 << (len % 64);
            }
            len += 1;
        }
        Self { words, len }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn validate(&self) -> Result<()> {
        if self.words.len() != self.len.div_ceil(64) {
            return Err("invalid signal mask backing length".into());
        }
        let bits = self.len % 64;
        if bits > 0
            && self
                .words
                .last()
                .is_some_and(|word| word & (!0_u64 << bits) != 0)
        {
            return Err("nonzero signal mask padding".into());
        }
        Ok(())
    }
    pub fn count(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }
    pub fn get(&self, i: usize) -> bool {
        i < self.len
            && self
                .words
                .get(i / 64)
                .is_some_and(|word| word & (1 << (i % 64)) != 0)
    }
    pub fn combine(&self, other: &Self, and: bool) -> Result<Self> {
        self.validate()?;
        other.validate()?;
        if self.len != other.len {
            return Err("incompatible signal masks".into());
        }
        Ok(Self {
            words: self
                .words
                .iter()
                .zip(&other.words)
                .map(|(a, b)| if and { a & b } else { a | b })
                .collect(),
            len: self.len,
        })
    }
}

/// Prior-window extrema. Current values never enter their own breakout reference.
pub type OptionalSeries = Vec<Option<i64>>;
pub fn prior_extrema(
    high: &[i64],
    low: &[i64],
    window: usize,
) -> Result<(OptionalSeries, OptionalSeries)> {
    if high.len() != low.len() || !(1..=100_000).contains(&window) {
        return Err("invalid envelope inputs".into());
    }
    let mut maxima = std::collections::VecDeque::<usize>::new();
    let mut minima = std::collections::VecDeque::<usize>::new();
    let mut upper = vec![None; high.len()];
    let mut lower = vec![None; low.len()];
    for i in 0..high.len() {
        while maxima.front().is_some_and(|j| *j + window < i) {
            maxima.pop_front();
        }
        while minima.front().is_some_and(|j| *j + window < i) {
            minima.pop_front();
        }
        if i >= window {
            upper[i] = maxima.front().map(|j| high[*j]);
            lower[i] = minima.front().map(|j| low[*j]);
        }
        while maxima.back().is_some_and(|j| high[*j] <= high[i]) {
            maxima.pop_back();
        }
        while minima.back().is_some_and(|j| low[*j] >= low[i]) {
            minima.pop_back();
        }
        maxima.push_back(i);
        minima.push_back(i);
    }
    Ok((upper, lower))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Macd {
    pub line: Vec<Option<f64>>,
    pub signal: Vec<Option<f64>>,
    pub histogram: Vec<Option<f64>>,
}
fn floating_ema(values: &[Option<f64>], window: usize) -> Vec<Option<f64>> {
    let (mut count, mut sum, mut state) = (0, 0.0, None);
    let alpha = 2.0 / (window + 1) as f64;
    values
        .iter()
        .map(|value| {
            match value {
                None => {
                    count = 0;
                    sum = 0.0;
                    state = None;
                }
                Some(value) => {
                    if let Some(previous) = state {
                        state = Some(alpha * value + (1.0 - alpha) * previous);
                    } else {
                        count += 1;
                        sum += value;
                        if count == window {
                            state = Some(sum / window as f64);
                        }
                    }
                }
            }
            state
        })
        .collect()
}
/// SMA-seeded floating MACD, centered by exact integer subtraction for translation stability.
/// Existing integer-mean EMA conventions remain unchanged; this method is explicitly separate.
pub fn macd(values: &[i64], fast: usize, slow: usize, signal_window: usize) -> Result<Macd> {
    if fast < 2 || slow <= fast || slow > 100_000 || !(2..=100_000).contains(&signal_window) {
        return Err("invalid MACD windows".into());
    }
    let origin = values.first().copied().unwrap_or(0) as i128;
    let centered = values
        .iter()
        .map(|value| Some((*value as i128 - origin) as f64))
        .collect::<Vec<_>>();
    let fast = floating_ema(&centered, fast);
    let slow = floating_ema(&centered, slow);
    let line = fast
        .iter()
        .zip(&slow)
        .map(|(a, b)| match (a, b) {
            (Some(a), Some(b)) => Some(a - b),
            _ => None,
        })
        .collect::<Vec<_>>();
    let signal = floating_ema(&line, signal_window);
    let histogram = line
        .iter()
        .zip(&signal)
        .map(|(a, b)| match (a, b) {
            (Some(a), Some(b)) => Some(a - b),
            _ => None,
        })
        .collect();
    Ok(Macd {
        line,
        signal,
        histogram,
    })
}
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bands {
    pub lower: i64,
    pub middle: i64,
    pub upper: i64,
}
/// Population variance uses checked exact centered integer sums. Final bands use
/// sqrt and round half-up to whole price units; arithmetic overflow fails closed.
pub fn bollinger(values: &[i64], window: usize, multiple_bps: u32) -> Result<Vec<Option<Bands>>> {
    if !(2..=100_000).contains(&window) || !(1..=100_000).contains(&multiple_bps) {
        return Err("invalid Bollinger parameters".into());
    }
    let origin = values.first().copied().unwrap_or(0) as i128;
    let (mut sum, mut squares) = (0_i128, 0_i128);
    let mut out = Vec::with_capacity(values.len());
    let round = |delta: f64| -> Result<i64> {
        if !delta.is_finite() {
            return Err("nonfinite Bollinger band".into());
        }
        let level = origin
            .checked_add((delta + 0.5).floor() as i128)
            .ok_or("Bollinger band overflow")?;
        i64::try_from(level).map_err(|_| "Bollinger band exceeds price representation".into())
    };
    for (i, value) in values.iter().enumerate() {
        if i >= window {
            let old = values[i - window] as i128 - origin;
            sum = sum.checked_sub(old).ok_or("Bollinger sum overflow")?;
            squares = squares
                .checked_sub(old.checked_mul(old).ok_or("Bollinger square overflow")?)
                .ok_or("Bollinger square sum overflow")?;
        }
        let delta = *value as i128 - origin;
        sum = sum.checked_add(delta).ok_or("Bollinger sum overflow")?;
        squares = squares
            .checked_add(
                delta
                    .checked_mul(delta)
                    .ok_or("Bollinger square overflow")?,
            )
            .ok_or("Bollinger square sum overflow")?;
        if i + 1 < window {
            out.push(None);
            continue;
        }
        let numerator = (window as i128)
            .checked_mul(squares)
            .and_then(|n| sum.checked_mul(sum).and_then(|s| n.checked_sub(s)))
            .ok_or("Bollinger variance overflow")?;
        if numerator < 0 {
            return Err("negative exact Bollinger variance".into());
        }
        let mean = sum as f64 / window as f64;
        let deviation = (numerator as f64).sqrt() / window as f64;
        let width = deviation * multiple_bps as f64 / 10000.0;
        out.push(Some(Bands {
            lower: round(mean - width)?,
            middle: round(mean)?,
            upper: round(mean + width)?,
        }));
    }
    Ok(out)
}
