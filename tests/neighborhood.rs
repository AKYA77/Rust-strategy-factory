use rust_strategy_factory::{neighborhood, strategies::Rule};
use std::collections::BTreeSet;
#[test]
fn scalar_probes_are_exact_bounded_deterministic_and_canonical() {
    assert_eq!(
        neighborhood::rules(&Rule::Rising { lookback: 2 }).unwrap(),
        vec![Rule::Rising { lookback: 1 }, Rule::Rising { lookback: 3 }]
    );
    assert_eq!(
        neighborhood::rules(&Rule::Rising { lookback: 1 }).unwrap(),
        vec![Rule::Rising { lookback: 2 }]
    );
    let rule = Rule::All(vec![
        Rule::RsiThreshold {
            window: 14,
            basis_points: 7000,
            above: true,
        },
        Rule::MeanTrend {
            fast: 10,
            slow: 30,
            above: true,
        },
        Rule::RangeExpansion {
            window: 20,
            multiple_bps: 15000,
        },
    ]);
    let results = neighborhood::rules(&rule).unwrap();
    assert_eq!(results.len(), 6);
    let mut equivalent = if let Rule::All(v) = rule.clone() {
        v
    } else {
        unreachable!()
    };
    equivalent.reverse();
    assert_eq!(
        results,
        neighborhood::rules(&Rule::All(equivalent)).unwrap()
    );
    let identities = results
        .iter()
        .map(|r| serde_json::to_string(&r.canonical().unwrap()).unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(identities.len(), results.len());
    assert!(!results.contains(&rule.canonical().unwrap()));
    assert_eq!(
        rule,
        Rule::All(vec![
            Rule::RsiThreshold {
                window: 14,
                basis_points: 7000,
                above: true
            },
            Rule::MeanTrend {
                fast: 10,
                slow: 30,
                above: true
            },
            Rule::RangeExpansion {
                window: 20,
                multiple_bps: 15000
            }
        ])
    );
}
#[test]
fn invalid_windows_thresholds_and_fixed_session_fields_are_not_invented() {
    for rule in [
        Rule::MeanTrend {
            fast: 2,
            slow: 3,
            above: true,
        },
        Rule::MacdCross {
            fast: 2,
            slow: 3,
            signal_window: 2,
            above: true,
        },
        Rule::BollingerCross {
            window: 2,
            multiple_bps: 100000,
            upper: false,
        },
        Rule::RsiCross {
            window: 2,
            basis_points: 10000,
            above: false,
        },
        Rule::ChannelEdge {
            window: 100000,
            basis_points: 0,
            upper: true,
        },
    ] {
        for neighbor in neighborhood::rules(&rule).unwrap() {
            neighbor.canonical().unwrap();
        }
    }
    let macd = neighborhood::rules(&Rule::MacdCross {
        fast: 2,
        slow: 3,
        signal_window: 2,
        above: true,
    })
    .unwrap();
    assert!(macd.iter().any(|r| matches!(
        r,
        Rule::MacdCross {
            signal_window: 3,
            ..
        }
    )));
    assert!(
        neighborhood::rules(&Rule::UtcSession {
            start_hour: 1,
            end_hour: 12,
            weekdays: 31
        })
        .unwrap()
        .is_empty()
    );
    assert!(neighborhood::rules(&Rule::AboveMean { window: 0 }).is_err());
}

#[test]
fn flattened_boolean_rules_must_remain_within_the_canonical_arity_budget() {
    let leaves = |start, end| {
        (start..=end)
            .map(|lookback| Rule::Rising { lookback })
            .collect::<Vec<_>>()
    };
    for conjunction in [true, false] {
        let combine = |v| {
            if conjunction {
                Rule::All(v)
            } else {
                Rule::Any(v)
            }
        };
        let over = combine(vec![combine(leaves(1, 8)), combine(leaves(9, 10))]);
        assert!(
            over.canonical().is_err(),
            "canonicalization must refuse an oversized flattened node"
        );
        let overlapping = combine(vec![combine(leaves(1, 8)), combine(leaves(1, 8))]);
        let canonical = overlapping.canonical().unwrap();
        assert_eq!(canonical, combine(leaves(1, 8)));
        assert_eq!(canonical.canonical().unwrap(), canonical);
    }
}

#[test]
fn seeded_nested_boolean_normalization_is_idempotent_when_accepted() {
    fn next(seed: &mut u64) -> u64 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *seed
    }
    fn generate(seed: &mut u64, depth: usize) -> Rule {
        let n = next(seed);
        if depth == 0 || n.is_multiple_of(3) {
            return Rule::Rising {
                lookback: (n % 20 + 1) as usize,
            };
        }
        let items = (0..(n % 3 + 1))
            .map(|_| generate(seed, depth - 1))
            .collect();
        if n & 1 == 0 {
            Rule::All(items)
        } else {
            Rule::Any(items)
        }
    }
    let mut seed = 42;
    let mut accepted = 0;
    for _ in 0..1000 {
        let rule = generate(&mut seed, 4);
        if let Ok(canonical) = rule.canonical() {
            assert_eq!(canonical.canonical().unwrap(), canonical);
            let restored: Rule =
                serde_json::from_str(&serde_json::to_string(&canonical).unwrap()).unwrap();
            assert_eq!(restored.canonical().unwrap(), canonical);
            accepted += 1;
        }
    }
    assert!(accepted > 100);
}
