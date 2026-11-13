use rust_strategy_factory::{
    candles::{self, Timeframe},
    contracts::{Conversion, Profile, Side},
    data::{self, Dataset, Tick},
    strategies::{Features, Rule, Strategy},
    tick_engine::{self, Costs},
};
fn dataset(ticks: Vec<Tick>) -> Dataset {
    data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        ticks,
        "SYNTHETIC_TEST_ONLY multi-timeframe fixture",
        "generated".into(),
        true,
    )
    .unwrap()
}
fn ticks() -> Vec<Tick> {
    (0..180)
        .flat_map(|i| {
            [
                Tick {
                    timestamp_ms: i * 60000,
                    sequence: (i * 2) as u64,
                    bid: 100000 + i * 50,
                    ask: 100020 + i * 50,
                },
                Tick {
                    timestamp_ms: i * 60000 + 1000,
                    sequence: (i * 2 + 1) as u64,
                    bid: 100010 + i * 50,
                    ask: 100030 + i * 50,
                },
            ]
        })
        .collect()
}
fn strategy() -> Strategy {
    Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks: 100,
        rule: Rule::All(vec![
            Rule::Rising { lookback: 1 },
            Rule::HigherTimeframe {
                timeframe: Timeframe::M5,
                rule: Box::new(Rule::Rising { lookback: 1 }),
            },
        ]),
        entry: None,
    }
}
#[test]
fn higher_confirmation_matches_closed_candle_join_and_actual_tick_replay() {
    let input = dataset(ticks());
    let s = strategy();
    let candles = candles::build(&input, Timeframe::M1).unwrap();
    let mut features = Features::with_context(&candles, 20000, &input, Timeframe::M1).unwrap();
    let signal = features.signal(&s.rule).unwrap();
    let higher = candles::build(&input, Timeframe::M5).unwrap();
    let hmask = Features::new(&higher, 20000)
        .unwrap()
        .signal(&Rule::Rising { lookback: 1 })
        .unwrap();
    let base = Features::new(&candles, 20000)
        .unwrap()
        .signal(&Rule::Rising { lookback: 1 })
        .unwrap();
    for (i, candle) in candles.iter().enumerate() {
        let last = higher.iter().enumerate().rfind(|(_, h)| {
            (h.available_ms, h.publication_sequence)
                <= (candle.available_ms, candle.publication_sequence)
        });
        assert_eq!(
            signal.get(i),
            base.get(i) && last.is_some_and(|(i, _)| hmask.get(i))
        );
        if candle.available_ms < 10 * 60000 {
            assert!(!signal.get(i));
        }
    }
    let before = features.cache_hits;
    assert_eq!(signal, features.signal(&s.rule).unwrap());
    assert!(features.cache_hits > before);
    let replay = tick_engine::replay(
        &input,
        &candles,
        &signal,
        &s,
        &Costs::default(),
        &vec![Conversion::usd(); input.ticks.len()],
    )
    .unwrap();
    assert!(!replay.trades.is_empty());
    assert_eq!(replay.trades[0].entry_event, 21);
    let false_mask =
        rust_strategy_factory::indicators::Mask::from_bools(vec![false; candles.len()]);
    assert!(
        tick_engine::replay(
            &input,
            &candles,
            &false_mask,
            &s,
            &Costs::default(),
            &vec![Conversion::usd(); input.ticks.len()]
        )
        .is_err()
    );
}
#[test]
fn future_quotes_do_not_change_higher_masks_and_warmup_does_not_guess() {
    let all = ticks();
    let full = dataset(all.clone());
    let s = strategy();
    let full_c = candles::build(&full, Timeframe::M1).unwrap();
    let full_m = Features::with_context(&full_c, 20000, &full, Timeframe::M1)
        .unwrap()
        .signal(&s.rule)
        .unwrap();
    for length in [7, 31, 64, 121, 250] {
        let prefix = dataset(all[..length].to_vec());
        let c = candles::build(&prefix, Timeframe::M1).unwrap();
        let m = Features::with_context(&c, 20000, &prefix, Timeframe::M1)
            .unwrap()
            .signal(&s.rule)
            .unwrap();
        for i in 0..c.len() {
            assert_eq!(m.get(i), full_m.get(i));
        }
    }
    assert!(
        Features::new(&full_c, 20000)
            .unwrap()
            .signal(&s.rule)
            .is_err()
    );
    assert!(
        Features::with_context(&full_c, full_c.len(), &full, Timeframe::M1)
            .unwrap()
            .signal(&s.rule)
            .is_err()
    );
}
#[test]
fn timeframe_hierarchy_and_nested_complexity_are_validated() {
    for (base, frame) in [
        (Timeframe::M1, Timeframe::M1),
        (Timeframe::M5, Timeframe::M1),
        (Timeframe::M5, Timeframe::M5),
    ] {
        let mut s = strategy();
        s.timeframe = base;
        s.rule = Rule::HigherTimeframe {
            timeframe: frame,
            rule: Box::new(Rule::Rising { lookback: 1 }),
        };
        assert!(s.canonical().is_err());
    }
    let mut s = strategy();
    s.timeframe = Timeframe::H1;
    assert!(s.canonical().is_err());
    let mut s = strategy();
    s.rule = Rule::HigherTimeframe {
        timeframe: Timeframe::M5,
        rule: Box::new(Rule::HigherTimeframe {
            timeframe: Timeframe::H1,
            rule: Box::new(Rule::Rising { lookback: 1 }),
        }),
    };
    s.canonical().unwrap();
    let input = dataset(ticks());
    let c = candles::build(&input, Timeframe::M1).unwrap();
    let mask = Features::with_context(&c, 20000, &input, Timeframe::M1)
        .unwrap()
        .signal(&s.rule)
        .unwrap();
    assert!(mask.count() > 0);
    for (i, candle) in c.iter().enumerate() {
        if candle.available_ms < 120 * 60000 {
            assert!(!mask.get(i));
        }
    }
    let mut invalid = s.clone();
    if let Rule::HigherTimeframe { rule, .. } = &mut invalid.rule {
        **rule = Rule::HigherTimeframe {
            timeframe: Timeframe::M1,
            rule: Box::new(Rule::Rising { lookback: 1 }),
        };
    }
    assert!(invalid.canonical().is_err());
    let mut rule = Rule::Rising { lookback: 1 };
    for frame in [
        Timeframe::D1,
        Timeframe::H4,
        Timeframe::H1,
        Timeframe::M30,
        Timeframe::M15,
        Timeframe::M5,
    ] {
        rule = Rule::HigherTimeframe {
            timeframe: frame,
            rule: Box::new(rule),
        };
    }
    assert!(rule.canonical().is_err());
}

#[test]
fn autonomous_regions_include_bounded_higher_confirmations() {
    use rust_strategy_factory::research;
    fn has(rule: &Rule) -> bool {
        match rule {
            Rule::HigherTimeframe { .. } => true,
            Rule::All(v) | Rule::Any(v) => v.iter().any(has),
            _ => false,
        }
    }
    let count = rust_strategy_factory::strategies::family_templates().len() as u64;
    let mut confirmations = 0;
    for slot in 0..count {
        let (_, candidate) = research::candidate_at(4 * count + slot, 42).unwrap();
        if candidate.timeframe != Timeframe::D1 {
            assert!(has(&candidate.rule));
            confirmations += 1;
        } else {
            assert!(!has(&candidate.rule));
        }
    }
    assert!(confirmations > 0);
}

#[test]
fn authoritative_mask_verification_accepts_valid_complex_bounded_graphs() {
    let input = dataset(ticks());
    let candles = candles::build(&input, Timeframe::M1).unwrap();
    let leaf = |i: usize| Rule::MeanTrend {
        fast: 2 + i * 2,
        slow: 3 + i * 2,
        above: true,
    };
    let mut groups = (0..7)
        .map(|g| Rule::All((0..3).map(|i| leaf(g * 3 + i)).collect()))
        .collect::<Vec<_>>();
    groups.push(leaf(21));
    let mut s = strategy();
    s.rule = Rule::Any(groups);
    s.canonical().unwrap();
    let mask = Features::with_context(&candles, 20000, &input, Timeframe::M1)
        .unwrap()
        .signal(&s.rule)
        .unwrap();
    assert!(mask.count() > 0);
    let result = tick_engine::replay(
        &input,
        &candles,
        &mask,
        &s,
        &Costs::default(),
        &vec![Conversion::usd(); input.ticks.len()],
    )
    .unwrap();
    assert!(!result.trades.is_empty());
}
