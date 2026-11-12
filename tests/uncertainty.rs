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
