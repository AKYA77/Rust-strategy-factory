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
