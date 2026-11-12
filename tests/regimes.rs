use rust_strategy_factory::{
    contracts::Profile,
    data::{self, Dataset, Tick},
    regimes::Classifier,
};
fn dataset(ticks: Vec<Tick>) -> Dataset {
    data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        ticks,
        "SYNTHETIC_TEST_ONLY regime fixture",
        "generated".into(),
        true,
    )
    .unwrap()
}
fn ticks() -> Vec<Tick> {
    (0..60)
        .map(|i| Tick {
            timestamp_ms: i * 60000,
            sequence: i as u64,
            bid: 100000 + i * 10,
            ask: 100020 + i * 10,
        })
        .collect()
}
#[test]
fn labels_use_strictly_prior_publications_and_remain_prefix_causal() {
    let original = ticks();
    let full = Classifier::new(&dataset(original.clone())).unwrap();
    assert_eq!(full.at(21, 21 * 60000), "UNKNOWN_WARMUP");
    assert_eq!(full.at(22, 22 * 60000), "UP_EQUAL_RANGE");
    for entry in 22..60 {
        let mut prefix = original[..=entry].to_vec();
        // Entry quote and any future event cannot alter its classification.
        prefix[entry].bid += 100000;
        prefix[entry].ask += 100000;
        let truncated = Classifier::new(&dataset(prefix)).unwrap();
        assert_eq!(
            full.at(entry as u64, entry as i64 * 60000),
            truncated.at(entry as u64, entry as i64 * 60000)
        );
    }
    assert_eq!(full.at(22, 21 * 60000), "UP_EQUAL_RANGE"); // tied publication time, later sequence
    assert_eq!(full.at(22, 23 * 60000 + 1), "UNKNOWN_STALE");
}
#[test]
fn gaps_reset_history_and_expanded_ranges_are_closed_prior_evidence() {
    let mut gap = ticks();
    for tick in &mut gap[25..] {
        tick.timestamp_ms += 3_600_000;
    }
    let classifier = Classifier::new(&dataset(gap.clone())).unwrap();
    assert_eq!(classifier.at(26, gap[26].timestamp_ms), "UNKNOWN_GAP");
    assert_eq!(classifier.at(48, gap[48].timestamp_ms), "UP_EQUAL_RANGE");
    let mut ranged = ticks();
    // Extra event inside bar20 enlarges its range; it is published on the next minute.
    ranged.push(Tick {
        timestamp_ms: 20 * 60000 + 10000,
        sequence: 0,
        bid: 101000,
        ask: 101020,
    });
    ranged.sort_by_key(|t| t.timestamp_ms); // Test fixture construction only; importer never repairs source.
    for (i, t) in ranged.iter_mut().enumerate() {
        t.sequence = i as u64;
    }
    let entry = ranged
        .iter()
        .find(|t| t.timestamp_ms == 22 * 60000)
        .unwrap();
    let classifier = Classifier::new(&dataset(ranged.clone())).unwrap();
    assert_eq!(
        classifier.at(entry.sequence, entry.timestamp_ms),
        "UP_EXPANDED"
    );
    let mut low = ticks();
    for t in &mut low {
        t.bid = 200000 - t.bid;
        t.ask = t.bid + 20;
    }
    assert_eq!(
        Classifier::new(&dataset(low)).unwrap().at(22, 22 * 60000),
        "DOWN_EQUAL_RANGE"
    );
}
