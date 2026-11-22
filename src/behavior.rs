//! Descriptive behavioral diversity over causal signal vectors.
//!
//! A strategy's behavior here is the boolean decision vector its rule produces over a
//! bounded causal candle prefix. Diversity is the exact pairwise Hamming distance between
//! those vectors, reported in basis points. This is structural description only: it is not
//! performance, robustness, novelty-versus-a-register, or eligibility evidence, and it never
//! adapts or qualifies anything.
use crate::{
    Result,
    candles::{self, Timeframe},
    data::Dataset,
    strategies::{Features, Strategy},
};
use serde::{Deserialize, Serialize};

pub const VERSION: &str = "signal-vector-population-diversity-v1";
/// Hard bounds keep the evidence cheap and deterministic. Members beyond these bounds are
/// refused rather than silently truncated.
pub const MAX_MEMBERS: usize = 64;
pub const MAX_BARS: usize = 200_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub concept_id: String,
    pub timeframe: Timeframe,
    pub bars: usize,
    pub signals: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub method: String,
    pub version: String,
    /// Common prefix length actually compared; shorter than a member's own bars when
    /// timeframes differ.
    pub bars: usize,
    pub members: usize,
    pub unique_concepts: usize,
    pub pairs: usize,
    /// Exact integer basis points, truncated downward; no floating-point accounting.
    pub mean_distance_bps: u64,
    pub min_distance_bps: u64,
    pub max_distance_bps: u64,
    pub zero_distance_pairs: usize,
    pub signals_total: usize,
    /// Ascending pairwise distances; complete for the reported population.
    pub distances_bps: Vec<u64>,
    pub population: Vec<Member>,
    pub warnings: Vec<String>,
    pub scope: String,
}

/// Dense causal decision vector for one strategy over a dataset's bounded candle prefix.
pub fn decisions(
    dataset: &Dataset,
    strategy: &Strategy,
    max_bars: usize,
) -> Result<(String, Timeframe, Vec<bool>)> {
    if max_bars == 0 || max_bars > MAX_BARS {
        return Err("invalid behavioral decision budget".into());
    }
    let strategy = strategy.canonical()?;
    let candles = candles::build(dataset, strategy.timeframe)?;
    let visible = candles.len().min(max_bars);
    if visible == 0 {
        return Err("no candles available for behavioral decisions".into());
    }
    let mut features = Features::with_context(
        &candles[..visible],
        visible.saturating_mul(128),
        dataset,
        strategy.timeframe,
    )?;
    let mask = features.signal(&strategy.rule)?;
    mask.validate()?;
    if mask.len() != visible {
        return Err("behavioral decision vector length mismatch".into());
    }
    let mut vector = vec![false; visible];
    for index in mask.set_indices()? {
        vector[index] = true;
    }
    Ok((strategy.identity()?, strategy.timeframe, vector))
}

/// Measure a bounded population on one dataset. Deterministic: population order follows the
/// supplied order, and pairing is over concept-sorted members.
pub fn measure(dataset: &Dataset, strategies: &[Strategy], max_bars: usize) -> Result<Evidence> {
    if strategies.is_empty() {
        return Err("behavioral diversity requires at least one strategy".into());
    }
    if strategies.len() > MAX_MEMBERS {
        return Err("behavioral population budget exceeded".into());
    }
    let mut vectors = Vec::with_capacity(strategies.len());
    for strategy in strategies {
        vectors.push(decisions(dataset, strategy, max_bars)?);
    }
    diversity(vectors)
}

/// Exact pairwise diversity over decision vectors. Members are sorted by concept id, so the
/// output is independent of input order; distances use the shortest member length.
pub fn diversity(mut members: Vec<(String, Timeframe, Vec<bool>)>) -> Result<Evidence> {
    if members.is_empty() {
        return Err("behavioral diversity requires at least one member".into());
    }
    if members.len() > MAX_MEMBERS {
        return Err("behavioral population budget exceeded".into());
    }
    members.sort_by(|a, b| a.0.cmp(&b.0));
    let bars = members
        .iter()
        .map(|(_, _, vector)| vector.len())
        .min()
        .ok_or("behavioral population empty")?;
    if bars == 0 {
        return Err("behavioral decision vectors are empty".into());
    }
    if members.iter().any(|(_, _, vector)| vector.len() < bars) {
        return Err("behavioral decision vector length mismatch".into());
    }
    let population = members
        .iter()
        .map(|(concept, timeframe, vector)| Member {
            concept_id: concept.clone(),
            timeframe: *timeframe,
            bars: vector.len(),
            signals: vector.iter().filter(|bit| **bit).count(),
        })
        .collect::<Vec<_>>();
    let unique_concepts = population
        .iter()
        .map(|member| member.concept_id.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let mut distances_bps = Vec::with_capacity(members.len() * (members.len() - 1) / 2);
    for i in 0..members.len() {
        for j in (i + 1)..members.len() {
            let diff = members[i]
                .2
                .iter()
                .zip(&members[j].2)
                .take(bars)
                .filter(|(a, b)| a != b)
                .count() as u128;
            distances_bps.push((diff * 10_000 / bars as u128) as u64);
        }
    }
    distances_bps.sort_unstable();
    let pairs = distances_bps.len();
    let zero_distance_pairs = distances_bps.iter().filter(|bps| **bps == 0).count();
    let mean_distance_bps = if pairs == 0 {
        0
    } else {
        distances_bps.iter().sum::<u64>() / pairs as u64
    };
    Ok(Evidence {
        method: "signal-vector-population-diversity".into(),
        version: VERSION.into(),
        bars,
        members: members.len(),
        unique_concepts,
        pairs,
        mean_distance_bps,
        min_distance_bps: distances_bps.first().copied().unwrap_or(0),
        max_distance_bps: distances_bps.last().copied().unwrap_or(0),
        zero_distance_pairs,
        signals_total: population.iter().map(|member| member.signals).sum(),
        distances_bps,
        population,
        warnings: vec![
            "Behavioral distance is a descriptive signal-vector Hamming distance over a bounded causal prefix; it is not performance, robustness, novelty or eligibility evidence."
                .into(),
            "Members with different candle counts are compared on their common prefix only; later bars are excluded."
                .into(),
            "Duplicate concept identities are reported in unique_concepts, not deduplicated or ranked."
                .into(),
        ],
        scope: "Descriptive training-only behavioral diversity; no eligibility, qualification or performance claim.".into(),
    })
}
