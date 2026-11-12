use rust_strategy_factory::{
    candles::Timeframe,
    contracts::Side,
    robustness,
    strategies::{Rule, Strategy},
    tick_engine::Costs,
};
fn strategy(stop_ticks: i64) -> Strategy {
    Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks,
        rule: Rule::Rising { lookback: 2 },
        entry: None,
    }
}
#[test]
fn frozen_probes_change_exactly_prespecified_cost_or_stop_fields() {
    let base = strategy(100);
    let costs = Costs::default();
    let cases = robustness::cases(&base, &costs).unwrap();
    assert_eq!(cases.len(), 3);
    assert_eq!(
        cases[0].strategy.identity().unwrap(),
        base.identity().unwrap()
    );
    assert_eq!(
        (
            cases[0].costs.spread_floor_pips,
            cases[0].costs.slippage_pips
        ),
        (3, 3)
    );
    assert_eq!(
        (cases[1].strategy.stop_ticks, cases[2].strategy.stop_ticks),
        (90, 110)
    );
    assert_eq!(cases[1].costs.commission_micro_usd_per_lot_per_fill, 0);
    for case in &cases[1..] {
        assert_eq!(
            serde_json::to_value(&case.costs).unwrap(),
            serde_json::to_value(&costs).unwrap()
        );
    }
    assert_eq!(
        serde_json::to_value(&cases).unwrap(),
        serde_json::to_value(robustness::cases(&base, &costs).unwrap()).unwrap()
    );
    assert_eq!(base.stop_ticks, 100);
}
#[test]
fn probes_obey_stop_domain_and_reject_unrepresentable_cost_stress() {
    let at_min = robustness::cases(&strategy(1), &Costs::default()).unwrap();
    assert_eq!(at_min.len(), 2);
    assert_eq!(at_min[1].strategy.stop_ticks, 2);
    let at_max = robustness::cases(&strategy(1_000_000), &Costs::default()).unwrap();
    assert_eq!(at_max.len(), 2);
    assert_eq!(at_max[1].strategy.stop_ticks, 900_000);
    assert!(
        robustness::cases(
            &strategy(100),
            &Costs {
                slippage_pips: 10_000,
                ..Default::default()
            }
        )
        .is_err()
    );
}
