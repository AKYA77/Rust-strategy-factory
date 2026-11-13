use rust_strategy_factory::{
    candles::Timeframe,
    contracts::Side,
    exploration,
    stops::Policy,
    strategies::{EntryPlan, Rule, Strategy},
    tick_engine::Costs,
};
use std::collections::BTreeSet;
fn parent() -> Strategy {
    Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        entry: Some(EntryPlan::Limit {
            offset_ticks: 3,
            ttl_ms: 1000,
            cancel_on_signal_false: true,
        }),
        stop_ticks: 80,
        stop_policy: Some(Policy::Atr {
            window: 14,
            multiple_bps: 20000,
        }),
        rule: Rule::All(vec![
            Rule::AboveMean { window: 2 },
            Rule::Rising { lookback: 2 },
        ]),
    }
}
#[test]
fn seeded_structure_is_bounded_canonical_distinct_and_preserves_execution() {
    let s = parent();
    let costs = Costs {
        commission_micro_usd_per_lot_per_fill: 12345,
        ..Costs::default()
    };
    let a = exploration::cases(&s, &costs, 42).unwrap();
    assert!(!a.is_empty());
    assert!(a.len() <= 8);
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&exploration::cases(&s, &costs, 42).unwrap()).unwrap()
    );
    let mut equivalent = s.clone();
    if let Rule::All(items) = &mut equivalent.rule {
        items.reverse();
    }
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&exploration::cases(&equivalent, &costs, 42).unwrap()).unwrap()
    );
    assert_ne!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&exploration::cases(&s, &costs, 43).unwrap()).unwrap()
    );
    let mut seen = BTreeSet::from([s.identity().unwrap()]);
    assert!(a.iter().any(|c| c.name.starts_with("structural-template-")));
    assert!(
        a.iter()
            .any(|c| c.name.starts_with("structural-drop-term-"))
    );
    assert!(
        a.iter()
            .any(|c| c.name == "structural-add-prior-higher-mean20")
    );
    for case in a {
        assert!(seen.insert(case.strategy.identity().unwrap()));
        assert_eq!(case.strategy, case.strategy.canonical().unwrap());
        let mut expected = s.clone();
        expected.rule = case.strategy.rule.clone();
        assert_eq!(case.strategy, expected.canonical().unwrap());
        assert_eq!(
            serde_json::to_string(&case.costs).unwrap(),
            serde_json::to_string(&costs).unwrap()
        );
    }
}
#[test]
fn crossover_refuses_mismatched_scope_contradiction_and_excess_complexity() {
    let a = parent();
    let mut b = a.clone();
    b.timeframe = Timeframe::M5;
    assert!(exploration::crossover(&a, &b, true).is_err());
    b = a.clone();
    b.side = Side::Short;
    assert!(exploration::crossover(&a, &b, false).is_err());
    b = a.clone();
    b.rule = Rule::BelowMean { window: 2 };
    assert!(exploration::crossover(&a, &b, true).is_err());
    let child = exploration::crossover(&a, &b, false).unwrap();
    assert!(matches!(child.rule, Rule::Any(_)));
    let mut large = a.clone();
    large.rule = Rule::Any((1..=8).map(|lookback| Rule::Rising { lookback }).collect());
    let mut donor = a.clone();
    donor.rule = Rule::Any((9..=16).map(|lookback| Rule::Rising { lookback }).collect());
    assert!(exploration::crossover(&large, &donor, false).is_err());
    assert!(
        exploration::cases(&large, &Costs::default(), 42)
            .unwrap()
            .len()
            <= 8
    );
    let mut invalid = a.clone();
    invalid.rule = Rule::All(vec![]);
    assert!(exploration::cases(&invalid, &Costs::default(), 0).is_err());
    let mut daily = a.clone();
    daily.timeframe = Timeframe::D1;
    assert!(
        exploration::cases(&daily, &Costs::default(), 0)
            .unwrap()
            .iter()
            .all(|c| c.name != "structural-add-prior-higher-mean20")
    );
}
