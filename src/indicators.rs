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
    pub fn count(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }
    pub fn get(&self, i: usize) -> bool {
        i < self.len && self.words[i / 64] & (1 << (i % 64)) != 0
    }
    pub fn combine(&self, other: &Self, and: bool) -> Result<Self> {
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
