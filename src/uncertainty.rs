//! Conditional diagnostics only: market-trade independence and search completeness are unproven.
use crate::{Result, validation::Observation};
use serde::{Deserialize, Serialize};

pub const VERSION: &str = "conditional-hoeffding-bonferroni-v1";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Diagnostic {
    pub method: String,
    pub observations: usize,
    pub net_wins: usize,
    pub declared_trials: u64,
    pub family_error_probability: f64,
    pub empirical_net_win_rate: Option<f64>,
    pub conditional_lower_net_win_rate: Option<f64>,
    pub assumptions_verified: bool,
    pub warnings: Vec<String>,
}
/// One-sided Hoeffding bound at alpha / declared_trials (Bonferroni union bound).
/// If observations are independent Bernoulli draws with a common probability, and the
/// trial count covers the whole prespecified family, simultaneous lower coverage is
/// at least 1-alpha, without requiring independence between candidates. Neither
/// prerequisite is established by these receipts. This is not a qualification gate.
pub fn diagnose(trades: &[Observation], declared_trials: u64) -> Result<Diagnostic> {
    if declared_trials == 0 || trades.len() > 1_000_000 {
        return Err("invalid uncertainty observation/trial budget".into());
    }
    if trades.windows(2).any(|w| w[0].exit_ms > w[1].exit_ms) {
        return Err("uncertainty observations must retain chronological order".into());
    }
    let wins = trades.iter().filter(|t| t.net_micro_usd > 0).count();
    let n = trades.len();
    let alpha = 0.05_f64;
    let rate = (n > 0).then(|| wins as f64 / n as f64);
    // log(m) - log(alpha) avoids an overflowing integer or floating multiplication.
    let lower = rate.map(|p| {
        (p - (((declared_trials as f64).ln() - alpha.ln()) / (2.0 * n as f64)).sqrt()).max(0.0)
    });
    let mut warnings = vec![
        "Conditional diagnostic only; trade dependence/common success probability are unverified"
            .into(),
        "Declared trial budget does not prove complete corpus-wide search/multiplicity accounting"
            .into(),
        "Net wins exclude breakevens; zero-trade windows do not establish historical coverage"
            .into(),
        "Reset-fold win probability is not expectancy, compounded equity, or final qualification"
            .into(),
    ];
    if n < 100 {
        warnings
            .push("Fewer than 100 pooled observations; annual sample gates are separate".into());
    }
    if n == 0 {
        warnings.push("No resolved OOS observations; no interval can be estimated".into());
    }
    Ok(Diagnostic {
        method: VERSION.into(),
        observations: n,
        net_wins: wins,
        declared_trials,
        family_error_probability: alpha,
        empirical_net_win_rate: rate,
        conditional_lower_net_win_rate: lower,
        assumptions_verified: false,
        warnings,
    })
}

pub const BLOCK_VERSION: &str = "conditional-time-block-hoeffding-bonferroni-v1";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BlockDiagnostic {
    pub method: String,
    pub block_ms: i64,
    pub occupied_blocks: usize,
    pub sum_squared_block_sizes: u64,
    pub observations: usize,
    pub net_wins: usize,
    pub declared_trials: u64,
    pub family_error_probability: f64,
    pub effective_blocks: Option<f64>,
    pub empirical_net_win_rate: Option<f64>,
    pub conditional_lower_net_win_rate: Option<f64>,
    pub assumptions_verified: bool,
    pub warnings: Vec<String>,
}
pub fn validate_block_ms(block_ms: i64) -> Result<()> {
    if !(86_400_000..=90 * 86_400_000).contains(&block_ms) {
        return Err("uncertainty blocks must span1..90 days".into());
    }
    Ok(())
}
/// Sensitivity to arbitrary within-block dependence: for independent block scores
/// conditional on their sizes, Hoeffding's denominator uses sum(n_b^2), rather
/// than N. Epoch-aligned blocks and duration must be frozen before OOS exposure.
/// Cross-block independence/conditional-size assumptions remain unverified.
pub fn diagnose_blocks(
    trades: &[Observation],
    declared_trials: u64,
    block_ms: i64,
) -> Result<BlockDiagnostic> {
    validate_block_ms(block_ms)?;
    let scalar = diagnose(trades, declared_trials)?;
    let mut blocks = std::collections::BTreeMap::<i64, u64>::new();
    for trade in trades {
        *blocks
            .entry(trade.exit_ms.div_euclid(block_ms))
            .or_default() += 1;
    }
    let squares: u128 = blocks
        .values()
        .map(|n| u128::from(*n) * u128::from(*n))
        .sum();
    let n = trades.len();
    let effective = (n > 0).then(|| (n as f64).powi(2) / squares as f64);
    let lower = scalar.empirical_net_win_rate.map(|p| {
        let correction = (((declared_trials as f64).ln() - scalar.family_error_probability.ln())
            * squares as f64
            / (2.0 * (n as f64).powi(2)))
        .sqrt();
        (p - correction).max(0.0)
    });
    Ok(BlockDiagnostic {method:BLOCK_VERSION.into(),block_ms,occupied_blocks:blocks.len(),sum_squared_block_sizes:squares as u64,observations:n,net_wins:scalar.net_wins,declared_trials,family_error_probability:scalar.family_error_probability,effective_blocks:effective,empirical_net_win_rate:scalar.empirical_net_win_rate,conditional_lower_net_win_rate:lower,assumptions_verified:false,warnings:vec![
        "Conditional sensitivity diagnostic; arbitrary within-block dependence allowed, but independent block scores conditional on observed sizes remain unverified".into(),
        "Epoch-aligned exit-time blocks can remain dependent across boundaries; duration must be frozen before OOS exposure".into(),
        "Empty blocks contribute no observations or effective sample size; coverage, unresolved positions and annual samples are separate gates".into(),
        "Declared trials do not prove complete multiplicity accounting; neither this bound nor synthetic evidence qualifies a strategy".into(),
    ]})
}
