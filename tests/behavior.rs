use rust_strategy_factory::{
    behavior,
    candles::Timeframe,
    contracts::{Profile, Side},
    data::{self, Dataset, Tick},
    strategies::{Rule, Strategy},
};

fn dataset(ticks: Vec<Tick>) -> Dataset {
    data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        ticks,
        "SYNTHETIC_TEST_ONLY behavior fixture",
        "generated".into(),
        true,
    )
    .unwrap()
}
/// Oscillating quotes so mean/RIS-style rules actually fire on a bounded prefix.
fn ticks() -> Vec<Tick> {
    (0..400)
        .map(|i| {
            let wave = (i * 37 % 2000) - 1000;
            Tick {
                timestamp_ms: i * 60000,
                sequence: i as u64,
                bid: 100000 + wave,
                ask: 100020 + wave,
            }
        })
        .collect()
}
fn strategy(rule: Rule) -> Strategy {
    Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks: 80,
        rule,
        entry: None,
        stop_policy: None,
    }
}
#[test]
fn diversity_is_exact_deterministic_and_reports_complete_distances() {
    let data = dataset(ticks());
    let population = vec![
        strategy(Rule::AboveMean { window: 2 }),
        strategy(Rule::AboveMean { window: 3 }),
        strategy(Rule::BelowMean { window: 2 }),
    ];
    let first = behavior::measure(&data, &population, 200).unwrap();
    assert_eq!(first.members, 3);
    assert_eq!(first.pairs, 3);
    assert_eq!(first.unique_concepts, 3);
    assert_eq!(first.distances_bps.len(), 3);
    assert!(first.distances_bps.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(
        first.min_distance_bps,
        *first.distances_bps.first().unwrap()
    );
    assert_eq!(first.max_distance_bps, *first.distances_bps.last().unwrap());
    assert_eq!(
        first.mean_distance_bps,
        first.distances_bps.iter().sum::<u64>() / 3
    );
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&behavior::measure(&data, &population, 200).unwrap()).unwrap()
    );
    // Input order must not matter: pairing is over concept-sorted members.
    let mut reversed = population.clone();
    reversed.reverse();
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&behavior::measure(&data, &reversed, 200).unwrap()).unwrap()
    );
    // The aggregate equals the pure pairwise computation over the same vectors.
    let vectors = population
        .iter()
        .map(|s| behavior::decisions(&data, s, 200).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&behavior::diversity(vectors).unwrap()).unwrap()
    );
}
#[test]
fn identical_rules_have_zero_distance_and_duplicates_are_reported() {
    let data = dataset(ticks());
    let population = vec![
        strategy(Rule::AboveMean { window: 2 }),
        strategy(Rule::AboveMean { window: 2 }),
    ];
    let evidence = behavior::measure(&data, &population, 200).unwrap();
    assert_eq!(evidence.pairs, 1);
    assert_eq!(evidence.zero_distance_pairs, 1);
    assert_eq!(evidence.mean_distance_bps, 0);
    assert_eq!(evidence.unique_concepts, 1);
    assert_eq!(evidence.population.len(), 2);
    assert_eq!(
        evidence.population[0].signals,
        evidence.population[1].signals
    );
}
#[test]
fn decision_vectors_are_causal_prefixes_within_budget() {
    let data = dataset(ticks());
    let s = strategy(Rule::AboveMean { window: 5 });
    let (concept, timeframe, vector) = behavior::decisions(&data, &s, 50).unwrap();
    assert_eq!(timeframe, Timeframe::M1);
    assert_eq!(vector.len(), 50);
    assert_eq!(concept, s.identity().unwrap());
    // A truncated dataset must not change the common prefix.
    let truncated = dataset(ticks()[..120].to_vec());
    let (_, _, prefix) = behavior::decisions(&truncated, &s, 50).unwrap();
    assert_eq!(vector, prefix);
}
#[test]
fn population_and_bar_budgets_are_refused_not_truncated() {
    let data = dataset(ticks());
    let one = strategy(Rule::AboveMean { window: 2 });
    assert!(behavior::measure(&data, &[], 200).is_err());
    assert!(behavior::measure(&data, std::slice::from_ref(&one), 0).is_err());
    assert!(behavior::measure(&data, std::slice::from_ref(&one), behavior::MAX_BARS + 1).is_err());
    let oversized = (0..=behavior::MAX_MEMBERS)
        .map(|window| strategy(Rule::AboveMean { window: window + 1 }))
        .collect::<Vec<_>>();
    assert!(behavior::measure(&data, &oversized, 200).is_err());
    let empty = behavior::diversity(Vec::new()).unwrap_err().to_string();
    assert!(empty.contains("at least one"));
    let duplicate = behavior::measure(&data, &[one.clone(), one], 200).unwrap();
    assert_eq!(duplicate.unique_concepts, 1);
    assert_eq!(duplicate.population.len(), 2);
}
