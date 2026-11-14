//! Mechanical observations under an explicitly unapproved calendar declaration.
use crate::{
    Result,
    catalog::{Entry, Facts},
    coverage,
    data::{self, Dataset},
    validation::Interval,
};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub reference: String,
    pub scope: Interval,
    pub open: Vec<Interval>,
}
#[derive(Debug, Serialize)]
pub struct Evidence {
    pub version: &'static str,
    pub catalog_hash: String,
    pub calendar_hash: String,
    pub declaration: Declaration,
    pub observed: coverage::Report,
    pub observations_in_open: usize,
    pub observations_outside_open: usize,
    pub outside_examples: Vec<coverage::Event>,
    pub unobserved_open_intervals: usize,
    pub unobserved_examples: Vec<Interval>,
    pub limitations: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub evidence: Evidence,
    pub hash: String,
}
pub fn assess(dataset: &Dataset, entry: &Entry, declaration: Declaration) -> Result<Report> {
    declaration.validate()?;
    if data::hash(&entry.receipt)? != entry.hash || entry.receipt.facts != Facts::observe(dataset)?
    {
        return Err("calendar evidence catalog/dataset context mismatch".into());
    }
    let observed = coverage::report(dataset, declaration.scope.clone(), 3600000, 100)?;
    let a = dataset
        .ticks
        .partition_point(|t| t.timestamp_ms < declaration.scope.start_ms);
    let b = dataset
        .ticks
        .partition_point(|t| t.timestamp_ms < declaration.scope.end_exclusive_ms);
    let ticks = &dataset.ticks[a..b];
    let mut in_open = 0;
    let mut outside = 0;
    let mut outside_examples = Vec::new();
    let mut cursor = 0;
    for t in ticks {
        while cursor < declaration.open.len()
            && declaration.open[cursor].end_exclusive_ms <= t.timestamp_ms
        {
            cursor += 1;
        }
        if cursor < declaration.open.len() && declaration.open[cursor].contains(t.timestamp_ms) {
            in_open += 1;
        } else {
            outside += 1;
            if outside_examples.len() < 100 {
                outside_examples.push(coverage::Event::from(t));
            }
        }
    }
    let mut unobserved = 0;
    let mut unobserved_examples = Vec::new();
    for w in &declaration.open {
        let first = ticks.partition_point(|t| t.timestamp_ms < w.start_ms);
        if first == ticks.len() || ticks[first].timestamp_ms >= w.end_exclusive_ms {
            unobserved += 1;
            if unobserved_examples.len() < 100 {
                unobserved_examples.push(w.clone());
            }
        }
    }
    let evidence = Evidence {
        version: "declared-calendar-observations-v1",
        catalog_hash: entry.hash.clone(),
        calendar_hash: data::hash(&declaration)?,
        declaration,
        observed,
        observations_in_open: in_open,
        observations_outside_open: outside,
        outside_examples,
        unobserved_open_intervals: unobserved,
        unobserved_examples,
        limitations: "Calendar reference and schedule are unapproved supplied assumptions. Counts are mechanical observations, not source authenticity, missing-quote proof, complete history, quality/profile/calendar approval or eligibility. Empty declared sessions may have no quotes legitimately. No quotes repaired, inserted, removed or reordered. Example vectors are capped at100; total counts are exact.",
    };
    let hash = data::hash(&evidence)?;
    Ok(Report { evidence, hash })
}

impl Declaration {
    pub fn validate(&self) -> Result<()> {
        self.scope.validate()?;
        if self.reference.trim().is_empty()
            || self.reference.len() > 4096
            || self.open.len() > 100000
        {
            return Err("calendar declaration budget/reference invalid".into());
        }
        let mut last = self.scope.start_ms;
        for w in &self.open {
            w.validate()?;
            if w.start_ms < last
                || w.start_ms < self.scope.start_ms
                || w.end_exclusive_ms > self.scope.end_exclusive_ms
            {
                return Err("calendar intervals must be ordered disjoint and within scope".into());
            }
            last = w.end_exclusive_ms;
        }
        Ok(())
    }
}
