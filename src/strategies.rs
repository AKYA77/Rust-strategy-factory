use crate::{
    Result,
    candles::{Candle, Timeframe},
    contracts::Side,
    data::hash,
    indicators::{Mask, sma},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Rule {
    AboveMean {
        window: usize,
    },
    BelowMean {
        window: usize,
    },
    Rising {
        lookback: usize,
    },
    Falling {
        lookback: usize,
    },
    MeanTrend {
        fast: usize,
        slow: usize,
        above: bool,
    },
    CrossMean {
        window: usize,
        above: bool,
    },
    RsiThreshold {
        window: usize,
        basis_points: u16,
        above: bool,
    },
    RsiCross {
        window: usize,
        basis_points: u16,
        above: bool,
    },
    Donchian {
        window: usize,
        above: bool,
    },
    ChannelEdge {
        window: usize,
        basis_points: u16,
        upper: bool,
    },
    RangeExpansion {
        window: usize,
        multiple_bps: u32,
    },
    UtcSession {
        start_hour: u8,
        end_hour: u8,
        weekdays: u8,
    },
    All(Vec<Rule>),
    Any(Vec<Rule>),
    HigherTimeframe {
        timeframe: Timeframe,
        rule: Box<Rule>,
    },
    MacdCross {
        fast: usize,
        slow: usize,
        signal_window: usize,
        above: bool,
    },
    BollingerCross {
        window: usize,
        multiple_bps: u32,
        upper: bool,
    },
}
impl Rule {
    fn validate_timeframes(&self, base: Timeframe) -> Result<()> {
        match self {
            Rule::HigherTimeframe { timeframe, rule } => {
                if timeframe.millis() <= base.millis() {
                    return Err("confirmation timeframe must strictly increase".into());
                }
                rule.validate_timeframes(*timeframe)
            }
            Rule::All(rules) | Rule::Any(rules) => {
                for rule in rules {
                    rule.validate_timeframes(base)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    pub fn canonical(&self) -> Result<Self> {
        fn normalize(rule: &Rule, depth: usize, nodes: &mut usize) -> Result<Rule> {
            *nodes += 1;
            if depth > 4 || *nodes > 32 {
                return Err("rule complexity exceeds budget".into());
            }
            match rule {
                Rule::HigherTimeframe { timeframe, rule } => Ok(Rule::HigherTimeframe {
                    timeframe: *timeframe,
                    rule: Box::new(normalize(rule, depth + 1, nodes)?),
                }),
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
                    if is_all {
                        for item in &v {
                            let opposite = match item {
                                Rule::AboveMean { window } => {
                                    Some(Rule::BelowMean { window: *window })
                                }
                                Rule::Rising { lookback } => Some(Rule::Falling {
                                    lookback: *lookback,
                                }),
                                _ => None,
                            };
                            if opposite.is_some_and(|x| v.binary_search(&x).is_ok()) {
                                return Err("contradictory conjunctive rule".into());
                            }
                        }
                    }
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
                Rule::MacdCross {
                    fast,
                    slow,
                    signal_window,
                    ..
                } => {
                    if *fast < 2
                        || slow <= fast
                        || *slow > 100_000
                        || !(2..=100_000).contains(signal_window)
                    {
                        return Err("invalid MACD windows".into());
                    }
                    Ok(rule.clone())
                }
                Rule::BollingerCross {
                    window,
                    multiple_bps,
                    ..
                } => {
                    if !(2..=100_000).contains(window) || !(1..=100_000).contains(multiple_bps) {
                        return Err("invalid Bollinger parameters".into());
                    }
                    Ok(rule.clone())
                }
                Rule::MeanTrend { fast, slow, .. } => {
                    if *fast < 2 || slow <= fast || *slow > 100_000 {
                        return Err("invalid mean trend windows".into());
                    }
                    Ok(rule.clone())
                }
                Rule::CrossMean { window, .. } | Rule::Donchian { window, .. } => {
                    if !(2..=100_000).contains(window) {
                        return Err("invalid prior lookback".into());
                    }
                    Ok(rule.clone())
                }
                Rule::RsiThreshold {
                    window,
                    basis_points,
                    ..
                }
                | Rule::RsiCross {
                    window,
                    basis_points,
                    ..
                }
                | Rule::ChannelEdge {
                    window,
                    basis_points,
                    ..
                } => {
                    if !(2..=100_000).contains(window) || *basis_points > 10000 {
                        return Err("invalid indicator window/threshold".into());
                    }
                    Ok(rule.clone())
                }
                Rule::RangeExpansion {
                    window,
                    multiple_bps,
                } => {
                    if !(2..=100_000).contains(window) || !(1..=100000).contains(multiple_bps) {
                        return Err("invalid volatility rule".into());
                    }
                    Ok(rule.clone())
                }
                Rule::UtcSession {
                    start_hour,
                    end_hour,
                    weekdays,
                } => {
                    if *start_hour >= 24
                        || *end_hour > 24
                        || start_hour == end_hour
                        || *weekdays == 0
                        || *weekdays > 127
                    {
                        return Err("invalid UTC session mask".into());
                    }
                    Ok(rule.clone())
                }
            }
        }
        normalize(self, 0, &mut 0)
    }
}
/// Relative to the publication event's effective executable entry side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryPlan {
    StopMarket {
        offset_ticks: i64,
        ttl_ms: i64,
        cancel_on_signal_false: bool,
    },
    Limit {
        offset_ticks: i64,
        ttl_ms: i64,
        cancel_on_signal_false: bool,
    },
}
impl EntryPlan {
    pub fn parameters(&self) -> (i64, i64, bool) {
        match *self {
            Self::StopMarket {
                offset_ticks,
                ttl_ms,
                cancel_on_signal_false,
            }
            | Self::Limit {
                offset_ticks,
                ttl_ms,
                cancel_on_signal_false,
            } => (offset_ticks, ttl_ms, cancel_on_signal_false),
        }
    }
    fn validate(&self) -> Result<()> {
        let (offset, ttl, _) = self.parameters();
        if !(1..=1_000_000).contains(&offset) || !(1..=crate::contracts::MAX_HOLD_MS).contains(&ttl)
        {
            return Err("entry offset/expiry outside grammar budget".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Strategy {
    pub timeframe: Timeframe,
    pub side: Side,
    pub stop_ticks: i64,
    pub rule: Rule,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<EntryPlan>,
}
impl Strategy {
    pub fn canonical(&self) -> Result<Self> {
        if let Some(plan) = &self.entry {
            plan.validate()?;
        }
        if self.stop_ticks <= 0 || self.stop_ticks > 1_000_000 {
            return Err("stop distance out of bounds".into());
        }
        let rule = self.rule.canonical()?;
        rule.validate_timeframes(self.timeframe)?;
        Ok(Self {
            rule,
            ..self.clone()
        })
    }
    pub fn identity(&self) -> Result<String> {
        hash(&("strategy-v1", self.canonical()?))
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Feature {
    Mean(usize),
    Rsi(usize),
    PriorHigh(usize),
    PriorLow(usize),
    TrueRange,
    PriorRangeMean(usize),
    MacdSign(usize, usize, usize),
    Band(usize, u32, bool),
}
/// Caches share indicator arrays across an entire candidate batch. Limits count retained cells.
pub struct Features<'a> {
    candles: &'a [Candle],
    context: Option<(&'a crate::data::Dataset, Timeframe)>,
    close: Vec<i64>,
    cache: BTreeMap<Feature, Vec<Option<i64>>>,
    signals: BTreeMap<Rule, Mask>,
    max_cells: usize,
    pub cache_hits: usize,
    pub feature_builds: usize,
}
impl<'a> Features<'a> {
    pub fn new(candles: &'a [Candle], max_cells: usize) -> Result<Self> {
        if candles.iter().any(|c| {
            c.bid.low <= 0
                || c.bid.low > c.bid.open
                || c.bid.low > c.bid.close
                || c.bid.high < c.bid.open
                || c.bid.high < c.bid.close
        }) {
            return Err("invalid signal candle OHLC".into());
        }
        if candles.len() > max_cells {
            return Err("feature budget exceeded".into());
        }
        Ok(Self {
            candles,
            context: None,
            close: candles.iter().map(|c| c.bid.close).collect(),
            cache: BTreeMap::new(),
            signals: BTreeMap::new(),
            max_cells,
            cache_hits: 0,
            feature_builds: 0,
        })
    }
    pub fn with_context(
        candles: &'a [Candle],
        max_cells: usize,
        data: &'a crate::data::Dataset,
        timeframe: Timeframe,
    ) -> Result<Self> {
        let mut features = Self::new(candles, max_cells)?;
        features.context = Some((data, timeframe));
        Ok(features)
    }
    fn can_retain(&self, additional: usize) -> bool {
        (1 + self.cache.len() + self.signals.len() + additional).saturating_mul(self.close.len())
            <= self.max_cells
    }
    fn ensure(&mut self, key: Feature) -> Result<()> {
        if self.cache.contains_key(&key) {
            self.cache_hits += 1;
            return Ok(());
        }
        if !self.can_retain(1) {
            return Err("shared feature budget exceeded".into());
        }
        let values = match key {
            Feature::MacdSign(fast, slow, signal) => {
                crate::indicators::macd(&self.close, fast, slow, signal)?
                    .histogram
                    .into_iter()
                    .map(|v| {
                        v.map(|v| {
                            let rounded = v.round();
                            if rounded > 0.0 {
                                1
                            } else if rounded < 0.0 {
                                -1
                            } else {
                                0
                            }
                        })
                    })
                    .collect()
            }
            Feature::Band(window, multiple, upper) => {
                crate::indicators::bollinger(&self.close, window, multiple)?
                    .into_iter()
                    .map(|v| v.map(|v| if upper { v.upper } else { v.lower }))
                    .collect()
            }
            Feature::Mean(w) => sma(&self.close, w)?,
            Feature::Rsi(w) => crate::indicators::rsi(&self.close, w)?
                .into_iter()
                .map(|v| v.map(|v| (v * 100.0).round() as i64))
                .collect(),
            Feature::PriorHigh(w) | Feature::PriorLow(w) => {
                let high: Vec<_> = self.candles.iter().map(|c| c.bid.high).collect();
                let low: Vec<_> = self.candles.iter().map(|c| c.bid.low).collect();
                let (upper, lower) = crate::indicators::prior_extrema(&high, &low, w)?;
                if matches!(key, Feature::PriorHigh(_)) {
                    upper
                } else {
                    lower
                }
            }
            Feature::TrueRange => self
                .candles
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let mut tr = c.bid.high - c.bid.low;
                    if i > 0 {
                        tr = tr
                            .max((c.bid.high - self.close[i - 1]).abs())
                            .max((c.bid.low - self.close[i - 1]).abs());
                    }
                    Some(tr)
                })
                .collect(),
            Feature::PriorRangeMean(w) => {
                self.ensure(Feature::TrueRange)?;
                if !self.can_retain(1) {
                    return Err("shared feature budget exceeded".into());
                }
                let tr: Vec<_> = self.cache[&Feature::TrueRange]
                    .iter()
                    .map(|v| v.unwrap())
                    .collect();
                let current = sma(&tr, w)?;
                (0..current.len())
                    .map(|i| if i == 0 { None } else { current[i - 1] })
                    .collect()
            }
        };
        self.cache.insert(key, values);
        self.feature_builds += 1;
        Ok(())
    }
    pub fn signal(&mut self, rule: &Rule) -> Result<Mask> {
        let canonical = rule.canonical()?;
        if !self.signals.contains_key(&canonical) {
            fn collect(rule: &Rule, out: &mut BTreeSet<Feature>) {
                match rule {
                    Rule::AboveMean { window }
                    | Rule::BelowMean { window }
                    | Rule::CrossMean { window, .. } => {
                        out.insert(Feature::Mean(*window));
                    }
                    Rule::MacdCross {
                        fast,
                        slow,
                        signal_window,
                        ..
                    } => {
                        out.insert(Feature::MacdSign(*fast, *slow, *signal_window));
                    }
                    Rule::BollingerCross {
                        window,
                        multiple_bps,
                        upper,
                    } => {
                        out.insert(Feature::Band(*window, *multiple_bps, *upper));
                    }
                    Rule::MeanTrend { fast, slow, .. } => {
                        out.insert(Feature::Mean(*fast));
                        out.insert(Feature::Mean(*slow));
                    }
                    Rule::RsiThreshold { window, .. } | Rule::RsiCross { window, .. } => {
                        out.insert(Feature::Rsi(*window));
                    }
                    Rule::Donchian { window, above } => {
                        out.insert(if *above {
                            Feature::PriorHigh(*window)
                        } else {
                            Feature::PriorLow(*window)
                        });
                    }
                    Rule::ChannelEdge { window, .. } => {
                        out.insert(Feature::PriorHigh(*window));
                        out.insert(Feature::PriorLow(*window));
                    }
                    Rule::RangeExpansion { window, .. } => {
                        out.insert(Feature::TrueRange);
                        out.insert(Feature::PriorRangeMean(*window));
                    }
                    Rule::All(v) | Rule::Any(v) => {
                        for r in v {
                            collect(r, out);
                        }
                    }
                    _ => {}
                }
            }
            let mut required = BTreeSet::new();
            collect(&canonical, &mut required);
            if (1 + required.len()).saturating_mul(self.close.len()) > self.max_cells {
                return Err("rule requires more feature cells than budget".into());
            }
            let missing = required
                .iter()
                .filter(|k| !self.cache.contains_key(*k))
                .count();
            if !self.can_retain(missing) {
                self.signals.clear();
            }
            while !self.can_retain(missing) {
                let victim = self
                    .cache
                    .keys()
                    .find(|k| !required.contains(*k))
                    .cloned()
                    .ok_or("feature budget exhausted")?;
                self.cache.remove(&victim);
            }
            for key in required {
                self.ensure(key)?;
            }
        }
        self.evaluate(&canonical)
    }
    fn evaluate(&mut self, rule: &Rule) -> Result<Mask> {
        if let Some(mask) = self.signals.get(rule) {
            self.cache_hits += 1;
            return Ok(mask.clone());
        }
        let result = match rule {
            Rule::MacdCross {
                fast,
                slow,
                signal_window,
                above,
            } => {
                let key = Feature::MacdSign(*fast, *slow, *signal_window);
                self.ensure(key.clone())?;
                let values = &self.cache[&key];
                Mask::from_bools((0..values.len()).map(|i|i>0 && matches!((values[i],values[i-1]),(Some(a),Some(b)) if if *above {a>0 && b<=0}else{a<0 && b>=0})))
            }
            Rule::BollingerCross {
                window,
                multiple_bps,
                upper,
            } => {
                let key = Feature::Band(*window, *multiple_bps, *upper);
                self.ensure(key.clone())?;
                let bands = &self.cache[&key];
                Mask::from_bools((0..bands.len()).map(|i|i>0 && matches!((bands[i],bands[i-1]),(Some(a),Some(b)) if if *upper {self.close[i]>a && self.close[i-1]<=b}else{self.close[i]<a && self.close[i-1]>=b})))
            }
            Rule::HigherTimeframe { timeframe, rule } => {
                let (data, base) = self
                    .context
                    .ok_or("higher-timeframe signals require dataset context")?;
                if timeframe.millis() <= base.millis() {
                    return Err("confirmation timeframe must strictly increase".into());
                }
                let used =
                    (1 + self.cache.len() + self.signals.len()).saturating_mul(self.close.len());
                let remaining = self.max_cells.saturating_sub(used);
                let higher = crate::candles::build(data, *timeframe)?;
                let mut features = Features::with_context(&higher, remaining, data, *timeframe)?;
                let mask = features.signal(rule)?;
                Mask::from_bools(self.candles.iter().map(|c| {
                    let n = higher.partition_point(|h| {
                        (h.available_ms, h.publication_sequence)
                            <= (c.available_ms, c.publication_sequence)
                    });
                    n.checked_sub(1).is_some_and(|i| mask.get(i))
                }))
            }
            Rule::AboveMean { window } | Rule::BelowMean { window } => {
                self.ensure(Feature::Mean(*window))?;
                let above = matches!(rule, Rule::AboveMean { .. });
                Mask::from_bools(
                    self.close
                        .iter()
                        .zip(&self.cache[&Feature::Mean(*window)])
                        .map(|(v, a)| a.is_some_and(|a| if above { *v > a } else { *v < a })),
                )
            }
            Rule::Rising { lookback } | Rule::Falling { lookback } => {
                let rising = matches!(rule, Rule::Rising { .. });
                Mask::from_bools(self.close.iter().enumerate().map(|(i, v)| {
                    i >= *lookback
                        && if rising {
                            *v > self.close[i - lookback]
                        } else {
                            *v < self.close[i - lookback]
                        }
                }))
            }
            Rule::MeanTrend { fast, slow, above } => {
                self.ensure(Feature::Mean(*fast))?;
                self.ensure(Feature::Mean(*slow))?;
                Mask::from_bools(
                    self.cache[&Feature::Mean(*fast)]
                        .iter()
                        .zip(&self.cache[&Feature::Mean(*slow)])
                        .map(
                            |(a, b)| matches!((a,b),(Some(a),Some(b)) if if *above {a>b}else{a<b}),
                        ),
                )
            }
            Rule::CrossMean { window, above } => {
                self.ensure(Feature::Mean(*window))?;
                let avg = &self.cache[&Feature::Mean(*window)];
                Mask::from_bools((0..self.close.len()).map(|i| i>0 && matches!((avg[i-1],avg[i]),(Some(prev),Some(now)) if if *above {self.close[i-1]<=prev && self.close[i]>now}else{self.close[i-1]>=prev && self.close[i]<now})))
            }
            Rule::RsiThreshold {
                window,
                basis_points,
                above,
            }
            | Rule::RsiCross {
                window,
                basis_points,
                above,
            } => {
                self.ensure(Feature::Rsi(*window))?;
                let values = &self.cache[&Feature::Rsi(*window)];
                let threshold = i64::from(*basis_points);
                let cross = matches!(rule, Rule::RsiCross { .. });
                Mask::from_bools((0..values.len()).map(|i| {
                    values[i].is_some_and(|v| if *above { v > threshold } else { v < threshold })
                        && (!cross
                            || i > 0
                                && values[i - 1].is_some_and(|v| {
                                    if *above {
                                        v <= threshold
                                    } else {
                                        v >= threshold
                                    }
                                }))
                }))
            }
            Rule::Donchian { window, above } => {
                let key = if *above {
                    Feature::PriorHigh(*window)
                } else {
                    Feature::PriorLow(*window)
                };
                self.ensure(key.clone())?;
                Mask::from_bools(
                    self.close
                        .iter()
                        .zip(&self.cache[&key])
                        .map(|(v, a)| a.is_some_and(|a| if *above { *v > a } else { *v < a })),
                )
            }
            Rule::ChannelEdge {
                window,
                basis_points,
                upper,
            } => {
                self.ensure(Feature::PriorHigh(*window))?;
                self.ensure(Feature::PriorLow(*window))?;
                let high = &self.cache[&Feature::PriorHigh(*window)];
                let low = &self.cache[&Feature::PriorLow(*window)];
                Mask::from_bools((0..self.close.len()).map(|i|matches!((high[i],low[i]),(Some(h),Some(l)) if h>l && self.close[i]>=l && self.close[i]<=h && if *upper {i128::from(h-self.close[i])*10000<=i128::from(h-l)*i128::from(*basis_points)}else{i128::from(self.close[i]-l)*10000<=i128::from(h-l)*i128::from(*basis_points)})))
            }
            Rule::RangeExpansion {
                window,
                multiple_bps,
            } => {
                self.ensure(Feature::TrueRange)?;
                self.ensure(Feature::PriorRangeMean(*window))?;
                Mask::from_bools(self.cache[&Feature::TrueRange].iter().zip(&self.cache[&Feature::PriorRangeMean(*window)]).map(|(now,prev)|matches!((now,prev),(Some(a),Some(b)) if *b>0 && i128::from(*a)*10000>=i128::from(*b)*i128::from(*multiple_bps))))
            }
            Rule::UtcSession {
                start_hour,
                end_hour,
                weekdays,
            } => {
                use chrono::{Datelike, Timelike};
                Mask::from_bools(self.candles.iter().map(|c| {
                    chrono::DateTime::from_timestamp_millis(c.available_ms).is_some_and(|t| {
                        let hour = t.hour() as u8;
                        let session = if start_hour < end_hour {
                            hour >= *start_hour && hour < *end_hour
                        } else {
                            hour >= *start_hour || hour < *end_hour
                        };
                        session && weekdays & (1 << t.weekday().num_days_from_monday()) != 0
                    })
                }))
            }
            Rule::All(rules) | Rule::Any(rules) => {
                let mut acc = self.evaluate(&rules[0])?;
                for r in &rules[1..] {
                    acc = acc.combine(&self.evaluate(r)?, matches!(rule, Rule::All(_)))?;
                }
                acc
            }
        };
        if self.can_retain(1) {
            self.signals.insert(rule.clone(), result.clone());
        }
        Ok(result)
    }
}
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Family {
    MeanReversion,
    Momentum,
    Breakout,
    TrendFollowing,
    Reversal,
    RangeTrading,
    Volatility,
    Session,
    Confluence,
}
impl Family {
    pub const ALL: [Self; 9] = [
        Self::MeanReversion,
        Self::Momentum,
        Self::Breakout,
        Self::TrendFollowing,
        Self::Reversal,
        Self::RangeTrading,
        Self::Volatility,
        Self::Session,
        Self::Confluence,
    ];
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub family: Family,
    pub strategy: Strategy,
}
/// Family metadata is not part of effective concept identity. Templates are bounded typed rules.
pub fn family_templates() -> Vec<Candidate> {
    let mut out = Vec::new();
    for timeframe in Timeframe::ALL {
        for side in [Side::Long, Side::Short] {
            let long = side == Side::Long;
            for window in [5, 10, 20] {
                for stop_ticks in [30, 100] {
                    for family in Family::ALL {
                        let rule = match family {
                            Family::MeanReversion => Rule::All(vec![
                                if long {
                                    Rule::BelowMean { window }
                                } else {
                                    Rule::AboveMean { window }
                                },
                                Rule::RsiThreshold {
                                    window,
                                    basis_points: if long { 3500 } else { 6500 },
                                    above: !long,
                                },
                            ]),
                            Family::Momentum => Rule::All(vec![
                                if long {
                                    Rule::Rising { lookback: window }
                                } else {
                                    Rule::Falling { lookback: window }
                                },
                                Rule::RsiThreshold {
                                    window,
                                    basis_points: if long { 5500 } else { 4500 },
                                    above: long,
                                },
                            ]),
                            Family::Breakout => Rule::Donchian {
                                window,
                                above: long,
                            },
                            Family::TrendFollowing => Rule::All(vec![
                                Rule::MeanTrend {
                                    fast: window,
                                    slow: window * 3,
                                    above: long,
                                },
                                if long {
                                    Rule::AboveMean { window }
                                } else {
                                    Rule::BelowMean { window }
                                },
                            ]),
                            Family::Reversal => Rule::RsiCross {
                                window,
                                basis_points: if long { 3000 } else { 7000 },
                                above: long,
                            },
                            Family::RangeTrading => Rule::ChannelEdge {
                                window,
                                basis_points: 2000,
                                upper: !long,
                            },
                            Family::Volatility => Rule::All(vec![
                                Rule::RangeExpansion {
                                    window,
                                    multiple_bps: 15000,
                                },
                                if long {
                                    Rule::Rising { lookback: 1 }
                                } else {
                                    Rule::Falling { lookback: 1 }
                                },
                            ]),
                            Family::Session => Rule::All(vec![
                                Rule::UtcSession {
                                    start_hour: 7,
                                    end_hour: 17,
                                    weekdays: 31,
                                },
                                Rule::CrossMean {
                                    window,
                                    above: long,
                                },
                            ]),
                            Family::Confluence => Rule::All(vec![
                                Rule::MeanTrend {
                                    fast: window,
                                    slow: window * 2,
                                    above: long,
                                },
                                Rule::Donchian {
                                    window,
                                    above: long,
                                },
                                Rule::RsiThreshold {
                                    window,
                                    basis_points: 5000,
                                    above: long,
                                },
                            ]),
                        };
                        out.push(Candidate {
                            family,
                            strategy: Strategy {
                                timeframe,
                                side,
                                stop_ticks,
                                rule,
                                entry: None,
                            },
                        });
                    }
                }
            }
        }
    }
    out
}
pub fn generate() -> Vec<Strategy> {
    family_templates().into_iter().map(|c| c.strategy).collect()
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
