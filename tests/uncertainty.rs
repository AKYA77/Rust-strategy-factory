use rust_strategy_factory::{uncertainty, validation::Observation};
fn observations(n: usize, wins: usize) -> Vec<Observation> {
    (0..n)
        .map(|i| Observation {
            exit_ms: i as i64,
            net_micro_usd: if i < wins { 1 } else { -1 },
        })
        .collect()
}
#[test]
fn independently_calculated_bounds_and_declared_multiplicity() {
    let t = observations(100, 75);
    let one = uncertainty::diagnose(&t, 1).unwrap();
    // sqrt(ln(20)/200) = 0.12238734153404082.
    assert!((one.conditional_lower_net_win_rate.unwrap() - 0.6276126584659592).abs() < 1e-14);
    let many = uncertainty::diagnose(&t, 1000).unwrap();
    assert!(
        (many.conditional_lower_net_win_rate.unwrap() - (0.75 - (20000_f64.ln() / 200.0).sqrt()))
            .abs()
            < 1e-14
    );
    assert!(many.conditional_lower_net_win_rate < one.conditional_lower_net_win_rate);
    assert!(!one.assumptions_verified);
    assert_eq!(one, uncertainty::diagnose(&t, 1).unwrap());
}
#[test]
fn no_data_is_not_zero_uncertainty_and_breakevens_are_not_wins() {
    let empty = uncertainty::diagnose(&[], 100).unwrap();
    assert_eq!(empty.empirical_net_win_rate, None);
    assert_eq!(empty.conditional_lower_net_win_rate, None);
    let mut t = observations(2, 1);
    t[0].net_micro_usd = 0;
    let a = uncertainty::diagnose(&t, 1).unwrap();
    assert_eq!(a.net_wins, 0);
    assert_eq!(a.conditional_lower_net_win_rate, Some(0.0));
}
#[test]
fn uncertainty_changes_monotonically_and_never_certifies_independence() {
    for n in [1, 10, 100, 1000] {
        let mut previous = 0.0;
        for wins in 0..=n {
            let a = uncertainty::diagnose(&observations(n, wins), u64::MAX).unwrap();
            let lower = a.conditional_lower_net_win_rate.unwrap();
            assert!(lower >= previous && lower <= a.empirical_net_win_rate.unwrap());
            assert!(!a.assumptions_verified);
            assert!(a.warnings.iter().any(|w| w.contains("dependence")));
            previous = lower;
        }
    }
    assert!(uncertainty::diagnose(&[], 0).is_err());
    assert!(
        uncertainty::diagnose(
            &[
                Observation {
                    exit_ms: 2,
                    net_micro_usd: 1
                },
                Observation {
                    exit_ms: 1,
                    net_micro_usd: 1
                }
            ],
            1
        )
        .is_err()
    );
}

#[test]
fn time_block_bound_matches_independent_weighted_hoeffding_calculation() {
    let day = 86_400_000;
    let mut trades = Vec::new();
    // Fixed independent oracle: block sizes2,3,5; sum of squares38; N10.
    for (block, size) in [2, 3, 5].into_iter().enumerate() {
        for _ in 0..size {
            trades.push(Observation {
                exit_ms: block as i64 * day,
                net_micro_usd: 1,
            });
        }
    }
    let diagnostic = uncertainty::diagnose_blocks(&trades, 7, day).unwrap();
    assert_eq!(diagnostic.occupied_blocks, 3);
    assert_eq!(diagnostic.effective_blocks, Some(100.0 / 38.0));
    let expected = (1.0 - ((140f64).ln() * 38.0 / 200.0).sqrt()).max(0.0);
    assert!((diagnostic.conditional_lower_net_win_rate.unwrap() - expected).abs() < 1e-14);
    assert!(!diagnostic.assumptions_verified);
    assert!(
        diagnostic.conditional_lower_net_win_rate
            <= uncertainty::diagnose(&trades, 7)
                .unwrap()
                .conditional_lower_net_win_rate
    );
}
#[test]
fn clustered_successes_do_not_gain_certainty_by_repeating_same_block() {
    let day = 86_400_000;
    for n in [1, 100, 10000] {
        let trades = observations(n, n);
        let d = uncertainty::diagnose_blocks(&trades, 1, day).unwrap();
        assert_eq!(d.effective_blocks, Some(1.0));
        assert_eq!(d.conditional_lower_net_win_rate, Some(0.0));
    }
    let mut trades = observations(100, 100);
    for (i, t) in trades.iter_mut().enumerate() {
        t.exit_ms = i as i64 * day;
    }
    let independent = uncertainty::diagnose_blocks(&trades, 1, day).unwrap();
    assert!(
        (independent.conditional_lower_net_win_rate.unwrap()
            - uncertainty::diagnose(&trades, 1)
                .unwrap()
                .conditional_lower_net_win_rate
                .unwrap())
        .abs()
            < 1e-14
    );
    let grouped = uncertainty::diagnose_blocks(&trades, 1, 10 * day).unwrap();
    assert_eq!(grouped.effective_blocks, Some(10.0));
    assert!(grouped.conditional_lower_net_win_rate < independent.conditional_lower_net_win_rate);
    let empty = uncertainty::diagnose_blocks(&[], 1, day).unwrap();
    assert_eq!(empty.effective_blocks, None);
    assert_eq!(empty.conditional_lower_net_win_rate, None);
    assert_eq!(empty.occupied_blocks, 0);
    assert!(uncertainty::diagnose_blocks(&trades, 1, day - 1).is_err());
    assert!(uncertainty::diagnose_blocks(&trades, 1, 91 * day).is_err());
}
