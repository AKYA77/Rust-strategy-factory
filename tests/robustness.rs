use rust_strategy_factory::{
    candles::Timeframe,
    contracts::Side,
    robustness,
    strategies::{Rule, Strategy},
    tick_engine::Costs,
};
fn strategy(stop_ticks: i64) -> Strategy {
    Strategy {
        stop_policy: None,
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
    assert_eq!(cases.len(), 5);
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
    assert_eq!(at_min.len(), 4);
    assert_eq!(at_min[1].strategy.stop_ticks, 2);
    let at_max = robustness::cases(&strategy(1_000_000), &Costs::default()).unwrap();
    assert_eq!(at_max.len(), 4);
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

#[test]
fn strategy_probes_preserve_costs_at_stress_ceiling() {
    let costs = Costs {
        slippage_pips: 10_000,
        ..Default::default()
    };
    let cases = robustness::strategy_cases(&strategy(100), &costs).unwrap();
    assert!(!cases.is_empty());
    for case in cases {
        assert_eq!(case.costs.slippage_pips, 10_000);
        assert_ne!(
            case.strategy.identity().unwrap(),
            strategy(100).identity().unwrap()
        );
    }
}

fn passing_probe_evidence() -> Vec<robustness::EvidenceSet> {
    use rust_strategy_factory::{
        data,
        science::{DescriptiveStats, Evidence},
    };
    robustness::cases(&strategy(100), &Costs::default())
        .unwrap()
        .into_iter()
        .map(|case| robustness::EvidenceSet {
            method: robustness::VERSION.into(),
            concept_id: case.strategy.identity().unwrap(),
            cost_hash: data::hash(&case.costs).unwrap(),
            experiments: (0..2)
                .map(|i| Evidence {
                    phase: format!("robustness-{}-fold-{i}-out-of-sample", case.name),
                    experiment_id: format!("{:064x}", i + 1),
                    outcome: "completed".into(),
                    reused: false,
                })
                .collect(),
            case,
            pooled_reset_fold_stats: DescriptiveStats {
                closed: 100,
                wins: 50,
                losses: 50,
                breakeven: 0,
                maximum_losing_streak: 5,
                sum_net_micro_usd: 0,
            },
            unresolved_positions: 0,
            failed_windows: 0,
            status: "DESCRIPTIVE_ONLY".into(),
        })
        .collect()
}
fn probe_policy() -> robustness::ProbePolicy {
    robustness::ProbePolicy {
        minimum_resolved_per_probe: 100,
        minimum_net_win_rate_bps: 5000,
        minimum_sum_net_micro_usd: 0,
        maximum_consecutive_losses: 5,
    }
}
#[test]
fn frozen_probe_policy_has_exact_numeric_boundaries_without_qualification() {
    let evidence = passing_probe_evidence();
    let policy = probe_policy();
    let assess = |e: &[robustness::EvidenceSet]| {
        robustness::assess_probes(&strategy(100), &Costs::default(), 2, e, &policy).unwrap()
    };
    let pass = assess(&evidence);
    assert_eq!(pass.state, "PASS_DECLARED_PROBES");
    assert!(!pass.qualification_granted);
    for failure in ["trades", "wins", "net", "streak"] {
        let mut e = evidence.clone();
        let s = &mut e[0].pooled_reset_fold_stats;
        match failure {
            "trades" => {
                s.closed = 99;
                s.losses = 49;
            }
            "wins" => {
                s.wins = 49;
                s.losses = 51;
            }
            "net" => s.sum_net_micro_usd = -1,
            "streak" => s.maximum_losing_streak = 6,
            _ => unreachable!(),
        };
        let result = assess(&e);
        assert_eq!(result.state, "REJECTED");
        assert!(!result.cases[0].reasons.is_empty());
        assert!(!result.qualification_granted);
    }
    let mut breakevens = evidence.clone();
    breakevens[0].pooled_reset_fold_stats.wins = 49;
    breakevens[0].pooled_reset_fold_stats.breakeven = 1;
    assert_eq!(assess(&breakevens).state, "REJECTED");
}
#[test]
fn incomplete_and_forged_probe_evidence_never_passes() {
    let evidence = passing_probe_evidence();
    let policy = probe_policy();
    let assess = |e: &[robustness::EvidenceSet]| {
        robustness::assess_probes(&strategy(100), &Costs::default(), 2, e, &policy)
    };
    assert_eq!(assess(&[]).unwrap().state, "UNKNOWN");
    assert_eq!(assess(&evidence[1..]).unwrap().state, "UNKNOWN");
    let mut unresolved = evidence.clone();
    unresolved[0].unresolved_positions = 1;
    assert_eq!(assess(&unresolved).unwrap().state, "UNKNOWN");
    let mut failed = evidence.clone();
    failed[0].failed_windows = 1;
    failed[0].experiments[0].outcome = "failed".into();
    assert_eq!(assess(&failed).unwrap().state, "UNKNOWN");
    let mut forged = evidence.clone();
    forged[0].cost_hash = "0".repeat(64);
    assert!(assess(&forged).is_err());
    let mut duplicate = evidence.clone();
    duplicate.push(evidence[0].clone());
    assert!(assess(&duplicate).is_err());
    let mut overflow = evidence.clone();
    overflow[0].pooled_reset_fold_stats.closed = usize::MAX;
    assert!(assess(&overflow).is_err());
    let mut weak = policy.clone();
    weak.minimum_net_win_rate_bps = 4999;
    assert!(weak.validate().is_err());
    weak = policy.clone();
    weak.maximum_consecutive_losses = 6;
    assert!(weak.validate().is_err());
}
