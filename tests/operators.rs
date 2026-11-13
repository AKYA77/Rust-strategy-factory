use rust_strategy_factory::{
    candles::{self, Timeframe},
    contracts::{Conversion, Profile, Side},
    data::{self, Tick},
    indicators,
    strategies::{Features, Rule, Strategy},
    tick_engine::{self, Costs},
};
#[test]
fn macd_matches_independent_rational_fixture_and_bands_match_population_variance() {
    let values = [10, 12, 14, 12, 10, 8];
    let m = indicators::macd(&values, 2, 3, 2).unwrap();
    assert_eq!(m.histogram[..3], [None, None, None]);
    for (i, line, signal, histogram) in [
        (3, 1.0 / 3.0, 2.0 / 3.0, -1.0 / 3.0),
        (4, -2.0 / 9.0, 2.0 / 27.0, -8.0 / 27.0),
        (5, -31.0 / 54.0, -29.0 / 81.0, -35.0 / 162.0),
    ] {
        assert!((m.line[i].unwrap() - line).abs() < 1e-12);
        assert!((m.signal[i].unwrap() - signal).abs() < 1e-12);
        assert!((m.histogram[i].unwrap() - histogram).abs() < 1e-12);
    }
    let b = indicators::bollinger(&[10, 12, 14, 16], 3, 20000).unwrap();
    assert_eq!(b[..2], [None, None]);
    assert_eq!(
        b[2],
        Some(indicators::Bands {
            lower: 9,
            middle: 12,
            upper: 15
        })
    );
    assert_eq!(
        b[3],
        Some(indicators::Bands {
            lower: 11,
            middle: 14,
            upper: 17
        })
    );
}
#[test]
fn operators_are_prefix_and_large_integer_translation_invariant_and_fail_closed() {
    let values = (0..150)
        .map(|i| 10000 + (i % 19 - 9) * 23)
        .collect::<Vec<_>>();
    let m = indicators::macd(&values, 3, 8, 3).unwrap();
    let b = indicators::bollinger(&values, 10, 20000).unwrap();
    for length in 1..values.len() {
        let prefix = indicators::macd(&values[..length], 3, 8, 3).unwrap();
        assert_eq!(prefix.histogram, m.histogram[..length]);
        assert_eq!(
            indicators::bollinger(&values[..length], 10, 20000).unwrap(),
            b[..length]
        );
    }
    let offset = 4_000_000_000_000_000_000_i64;
    let shifted = values.iter().map(|v| v + offset).collect::<Vec<_>>();
    assert_eq!(m, indicators::macd(&shifted, 3, 8, 3).unwrap());
    let shifted_b = indicators::bollinger(&shifted, 10, 20000).unwrap();
    for (a, b) in b.iter().zip(shifted_b) {
        match (a, b) {
            (Some(a), Some(b)) => assert_eq!(
                (a.lower + offset, a.middle + offset, a.upper + offset),
                (b.lower, b.middle, b.upper)
            ),
            (None, None) => {}
            _ => panic!("warmup changed"),
        }
    }
    assert!(indicators::macd(&values, 8, 3, 2).is_err());
    assert!(indicators::bollinger(&values, 0, 20000).is_err());
    assert!(indicators::bollinger(&[i64::MIN, i64::MAX], 2, 20000).is_err());
}
#[test]
fn new_crosses_produce_actual_subsequent_quote_trades_with_independent_accounting() {
    let ticks = (0..180)
        .flat_map(|i| {
            let phase = i % 40;
            let level = if phase < 20 {
                0
            } else if phase < 30 {
                (phase - 19) * 200
            } else {
                (40 - phase) * 200
            };
            [
                Tick {
                    timestamp_ms: i * 60000,
                    sequence: (i * 2) as u64,
                    bid: 100000 + level,
                    ask: 100020 + level,
                },
                Tick {
                    timestamp_ms: i * 60000 + 1000,
                    sequence: (i * 2 + 1) as u64,
                    bid: 100010 + level,
                    ask: 100030 + level,
                },
            ]
        })
        .collect();
    let input = data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        ticks,
        "SYNTHETIC_TEST_ONLY new operators",
        "generated".into(),
        true,
    )
    .unwrap();
    let c = candles::build(&input, Timeframe::M1).unwrap();
    let mut f = Features::with_context(&c, 20000, &input, Timeframe::M1).unwrap();
    for rule in [
        Rule::MacdCross {
            fast: 2,
            slow: 5,
            signal_window: 2,
            above: true,
        },
        Rule::BollingerCross {
            window: 20,
            multiple_bps: 20000,
            upper: true,
        },
    ] {
        let s = Strategy {
            stop_policy: None,
            timeframe: Timeframe::M1,
            side: Side::Long,
            stop_ticks: 100,
            rule,
            entry: None,
        };
        let mask = f.signal(&s.rule).unwrap();
        assert!(mask.count() > 0);
        let replay = tick_engine::replay(
            &input,
            &c,
            &mask,
            &s,
            &Costs::default(),
            &vec![Conversion::usd(); input.ticks.len()],
        )
        .unwrap();
        let first = &replay.trades[0];
        assert_eq!((first.entry_event, first.exit_event), (43, 46));
        assert_eq!((first.entry, first.exit), (100450, 100750));
        assert_eq!(first.volume_micro_lots, 16660);
        assert_eq!(first.net_micro_usd, 4_998_000);
    }
}

#[test]
fn expanded_regions_include_typed_macd_and_band_variants() {
    use rust_strategy_factory::{research, strategies};
    fn kinds(rule: &Rule) -> (bool, bool) {
        match rule {
            Rule::MacdCross { .. } => (true, false),
            Rule::BollingerCross { .. } => (false, true),
            Rule::All(v) | Rule::Any(v) => v
                .iter()
                .map(kinds)
                .fold((false, false), |(a, b), (c, d)| (a || c, b || d)),
            Rule::HigherTimeframe { rule, .. } => kinds(rule),
            _ => (false, false),
        }
    }
    let count = strategies::family_templates().len() as u64;
    let mut found = (false, false);
    for slot in 0..count {
        let (_, s) = research::candidate_at(6 * count + slot, 42).unwrap();
        let k = kinds(&s.rule);
        found = (found.0 || k.0, found.1 || k.1);
    }
    assert_eq!(found, (true, true));
}
