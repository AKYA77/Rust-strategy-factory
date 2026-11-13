//! Bounded training-only structural hypotheses. No performance or independent evidence input.
use crate::{
    Result,
    candles::Timeframe,
    contracts::Side,
    data,
    robustness::Case,
    strategies::{self, Rule, Strategy},
    tick_engine::Costs,
};
use std::collections::BTreeSet;
pub const VERSION: &str = "bounded-seeded-rule-structure-v1";

/// Cross only rules from compatible directional/timeframe scopes. All execution
/// fields come from the primary parent; donor entry/stop fields are not inherited.
pub fn crossover(parent: &Strategy, donor: &Strategy, conjunction: bool) -> Result<Strategy> {
    let parent = parent.canonical()?;
    let donor = donor.canonical()?;
    if parent.side != donor.side || parent.timeframe != donor.timeframe {
        return Err("crossover requires matching direction and primary timeframe".into());
    }
    let mut child = parent.clone();
    child.rule = if conjunction {
        Rule::All(vec![parent.rule, donor.rule])
    } else {
        Rule::Any(vec![parent.rule, donor.rule])
    };
    // Revalidate the normalized tree: Boolean flattening can expose arity limits.
    child.canonical()?.canonical()
}

pub fn cases(parent: &Strategy, costs: &Costs, seed: u64) -> Result<Vec<Case>> {
    let parent = parent.canonical()?;
    costs.validate()?;
    let mut raw = Vec::new();
    // Relax one direct Boolean term, bounded by the existing grammar's arity.
    if let Rule::All(terms) | Rule::Any(terms) = &parent.rule {
        for index in 0..terms.len() {
            let mut terms = terms.clone();
            terms.remove(index);
            let mut child = parent.clone();
            child.rule = if matches!(parent.rule, Rule::All(_)) {
                Rule::All(terms)
            } else {
                Rule::Any(terms)
            };
            raw.push((format!("structural-drop-term-{index}"), child));
        }
    }
    if let Some(timeframe) = Timeframe::ALL
        .iter()
        .position(|t| *t == parent.timeframe)
        .and_then(|i| Timeframe::ALL.get(i + 1))
        .copied()
    {
        let mut child = parent.clone();
        child.rule = Rule::All(vec![
            parent.rule.clone(),
            Rule::HigherTimeframe {
                timeframe,
                rule: Box::new(if parent.side == Side::Long {
                    Rule::AboveMean { window: 20 }
                } else {
                    Rule::BelowMean { window: 20 }
                }),
            },
        ]);
        raw.push(("structural-add-prior-higher-mean20".into(), child));
    }
    static TEMPLATES: std::sync::OnceLock<Vec<strategies::Candidate>> = std::sync::OnceLock::new();
    let templates = TEMPLATES.get_or_init(strategies::family_templates);
    let mut seen_rules = BTreeSet::from([data::hash(&parent.rule)?]);
    let mut donors = Vec::new();
    for donor in templates
        .iter()
        .filter(|c| c.strategy.side == parent.side && c.strategy.timeframe == parent.timeframe)
    {
        let rule_id = data::hash(&donor.strategy.rule.canonical()?)?;
        if seen_rules.insert(rule_id.clone()) {
            donors.push((
                data::hash(&(VERSION, seed, &rule_id))?,
                rule_id,
                &donor.strategy,
            ));
        }
    }
    donors.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, rule_id, donor) in donors.into_iter().take(2) {
        for conjunction in [true, false] {
            if let Ok(child) = crossover(&parent, donor, conjunction) {
                let mode = if conjunction { "all" } else { "any" };
                raw.push((format!("structural-template-{mode}-{rule_id}"), child));
            }
        }
    }
    let mut seen = BTreeSet::from([parent.identity()?]);
    let mut ranked = Vec::new();
    for (name, child) in raw {
        // Complexity, contradiction and time-hierarchy refusals are omissions,
        // never permission to relax the grammar or substitute an invalid rule.
        if let Ok(child) = child.canonical() {
            let Ok(concept) = child.identity() else {
                continue;
            };
            if seen.insert(concept.clone()) {
                ranked.push((
                    data::hash(&(VERSION, seed, &concept, &name))?,
                    Case {
                        name,
                        strategy: child,
                        costs: costs.clone(),
                    },
                ));
            }
        }
    }
    ranked.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(ranked.into_iter().take(8).map(|(_, case)| case).collect())
}
