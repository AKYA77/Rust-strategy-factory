//! Generation component only: no replay, discovery quality or independent validation claim.
use rust_strategy_factory::{
    candles::Timeframe,
    contracts::Side,
    data, exploration, research,
    strategies::{Rule, Strategy},
    tick_engine::Costs,
};
use std::{collections::BTreeSet, hint::black_box, time::Instant};
fn main() {
    let parent = Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        entry: None,
        stop_ticks: 100,
        stop_policy: None,
        rule: Rule::All(vec![
            Rule::AboveMean { window: 20 },
            Rule::Rising { lookback: 5 },
        ]),
    };
    let costs = Costs::default();
    let warmup = exploration::cases(&parent, &costs, 42).unwrap();
    assert!(!warmup.is_empty());
    let mut measurements = Vec::new();
    let mut corpus = BTreeSet::new();
    for repetition in 0..5 {
        let start = Instant::now();
        let mut generated = 0;
        let mut workload = Vec::new();
        for seed in 0..100 {
            let cases = exploration::cases(black_box(&parent), black_box(&costs), seed).unwrap();
            assert!(cases.len() <= 8);
            let ids = cases
                .iter()
                .map(|c| c.strategy.identity().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), ids.len());
            generated += ids.len();
            corpus.extend(ids.iter().cloned());
            workload.push(ids);
        }
        measurements.push(
            serde_json::json!({"repetition":repetition,"parent_calls":100,
            "generated":generated,"elapsed_ns":start.elapsed().as_nanos(),
            "ordered_workload_hash":data::hash(&workload).unwrap()}),
        );
    }
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "class":"COMPONENT_GENERATION_NOT_RESEARCH_THROUGHPUT",
        "operation":"100 seeded bounded structural rule proposals for one synthetic M1 long parent",
        "cache":"warm template pool; generation/canonicalization/hash/dedup and output-identity checks included",
        "method":exploration::VERSION,"parent":parent,"unique_concepts_across_seeds":corpus.len(),
        "compute_revision":research::compute_revision().unwrap(),"measurements":measurements,
        "limitations":"No market data or tick simulations; uniqueness means canonical concepts only, not independent signals or useful diversity. No A/B speed or discovery-quality claim."
    })).unwrap());
}
