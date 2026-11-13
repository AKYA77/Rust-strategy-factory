use rust_strategy_factory::{
    candles::{self, Candle, Ohlc, Timeframe},
    contracts::{Conversion, Profile, Side},
    data::{self, Dataset, Tick},
    indicators,
    stops::{self, Distance, Policy},
    strategies::{EntryPlan, Features, Rule, Strategy},
    tick_engine::{self, Costs, EntryStatus},
};
fn strategy() -> Strategy {
    Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks: 30,
        rule: Rule::AboveMean { window: 2 },
        entry: None,
        stop_policy: Some(Policy::Atr {
            window: 2,
            multiple_bps: 50000,
        }),
    }
}
fn dataset(ticks: Vec<Tick>) -> Dataset {
    data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        ticks,
        "SYNTHETIC_TEST_ONLY ATR fixture",
        "generated".into(),
        true,
    )
    .unwrap()
}
fn market() -> Dataset {
    dataset(vec![
        Tick {
            timestamp_ms: 0,
            sequence: 0,
            bid: 100000,
            ask: 100010,
        },
        Tick {
            timestamp_ms: 60000,
            sequence: 1,
            bid: 100020,
            ask: 100030,
        },
        Tick {
            timestamp_ms: 120000,
            sequence: 2,
            bid: 100040,
            ask: 100050,
        },
        Tick {
            timestamp_ms: 120000,
            sequence: 3,
            bid: 100050,
            ask: 100060,
        },
        Tick {
            timestamp_ms: 121000,
            sequence: 4,
            bid: 100300,
            ask: 100310,
        },
    ])
}
fn run(input: &Dataset, s: &Strategy) -> tick_engine::Replay {
    let c = candles::build(input, s.timeframe).unwrap();
    let mask = Features::with_context(&c, 100000, input, s.timeframe)
        .unwrap()
        .signal(&s.rule)
        .unwrap();
    tick_engine::replay(
        input,
        &c,
        &mask,
        s,
        &Costs::default(),
        &vec![Conversion::usd(); input.ticks.len()],
    )
    .unwrap()
}
fn bars() -> Vec<Candle> {
    [
        (100, 105, 100, 105),
        (105, 115, 105, 110),
        (110, 115, 110, 115),
        (115, 120, 115, 120),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (open, high, low, close))| Candle {
        start_ms: i as i64 * 60000,
        available_ms: (i + 1) as i64 * 60000,
        publication_sequence: i as u64,
        bid: Ohlc {
            open,
            high,
            low,
            close,
        },
        ask: Ohlc {
            open: open + 20,
            high: high + 20,
            low: low + 20,
            close: close + 20,
        },
        events: 2,
    })
    .collect()
}
#[test]
fn outward_wilder_atr_and_grid_distance_match_independent_integer_fixture() {
    let c = bars();
    assert_eq!(
        indicators::atr_outward(&c, 2).unwrap(),
        vec![None, Some(8), Some(7), Some(6)]
    );
    let mut profile = Profile::synthetic("EURUSD").unwrap();
    profile.tick = 5;
    let mut s = strategy();
    s.stop_ticks = 2;
    s.stop_policy = Some(Policy::Atr {
        window: 2,
        multiple_bps: 15000,
    });
    let prepared = stops::prepare(&s, &profile, &c).unwrap();
    assert_eq!(
        (0..4).map(|i| prepared.at(i)).collect::<Vec<_>>(),
        vec![
            Distance::Warmup,
            Distance::Ready(15),
            Distance::Ready(15),
            Distance::Ready(10)
        ]
    );
    for length in 1..=c.len() {
        assert_eq!(
            indicators::atr_outward(&c[..length], 2).unwrap(),
            indicators::atr_outward(&c, 2).unwrap()[..length]
        );
    }
}
#[test]
fn atr_market_stop_has_independently_calculated_risk_volume_and_three_r_target() {
    let input = market();
    let s = strategy();
    let replay = run(&input, &s);
    assert_eq!(replay.trades.len(), 1);
    let trade = &replay.trades[0];
    assert_eq!(
        (trade.entry, trade.stop, trade.exit),
        (100090, 100040, 100240)
    );
    assert_eq!(
        (trade.volume_micro_lots, trade.net_micro_usd),
        (19210, 2_881_500)
    );
    let mut fixed = s;
    fixed.stop_policy = None;
    fixed.stop_ticks = 50;
    assert_eq!(run(&input, &fixed).trades, replay.trades);
    let mut future = input.ticks.clone();
    future.extend([
        Tick {
            timestamp_ms: 180000,
            sequence: 5,
            bid: 8_000_000_000_000_000_000,
            ask: 8_000_000_000_000_000_020,
        },
        Tick {
            timestamp_ms: 240000,
            sequence: 6,
            bid: 8_000_000_000_000_000_000,
            ask: 8_000_000_000_000_000_020,
        },
    ]);
    let future = dataset(future);
    let s = strategy();
    let c = candles::build(&future, Timeframe::M1).unwrap();
    let mask = Features::with_context(&c, 100000, &future, Timeframe::M1)
        .unwrap()
        .signal(&s.rule)
        .unwrap();
    let scoped = tick_engine::replay_window(
        &future,
        &c,
        &mask,
        &s,
        &Costs::default(),
        &vec![Conversion::usd(); future.ticks.len()],
        &rust_strategy_factory::validation::Interval {
            start_ms: 0,
            end_exclusive_ms: 122000,
        },
    )
    .unwrap();
    assert_eq!(scoped.trades, replay.trades);
}
#[test]
fn pending_order_freezes_atr_distance_and_quantity_before_later_volatility_and_fill() {
    let input = dataset(vec![
        Tick {
            timestamp_ms: 0,
            sequence: 0,
            bid: 100000,
            ask: 100010,
        },
        Tick {
            timestamp_ms: 60000,
            sequence: 1,
            bid: 100020,
            ask: 100030,
        },
        Tick {
            timestamp_ms: 120000,
            sequence: 2,
            bid: 100040,
            ask: 100050,
        },
        Tick {
            timestamp_ms: 120001,
            sequence: 3,
            bid: 99000,
            ask: 99010,
        },
        Tick {
            timestamp_ms: 180000,
            sequence: 4,
            bid: 100040,
            ask: 100050,
        },
        Tick {
            timestamp_ms: 180001,
            sequence: 5,
            bid: 100070,
            ask: 100080,
        },
        Tick {
            timestamp_ms: 180002,
            sequence: 6,
            bid: 100350,
            ask: 100360,
        },
    ]);
    let mut s = strategy();
    s.entry = Some(EntryPlan::StopMarket {
        offset_ticks: 30,
        ttl_ms: 180000,
        cancel_on_signal_false: false,
    });
    let c = candles::build(&input, Timeframe::M1).unwrap();
    let prepared = stops::prepare(&s, &input.profile, &c).unwrap();
    assert_eq!(prepared.at(1), Distance::Ready(50));
    assert_eq!(prepared.at(2), Distance::Ready(2625));
    let replay = run(&input, &s);
    assert_eq!(replay.trades.len(), 1);
    let trade = &replay.trades[0];
    assert_eq!(
        (trade.entry, trade.stop, trade.exit),
        (100110, 100060, 100260)
    );
    assert_eq!(
        (trade.volume_micro_lots, trade.net_micro_usd),
        (19210, 2_881_500)
    );
    assert_eq!(replay.entry_receipts.len(), 2);
    assert_eq!(replay.entry_receipts[0].status, EntryStatus::Placed);
    assert_eq!(replay.entry_receipts[1].status, EntryStatus::Filled);
    for receipt in &replay.entry_receipts {
        assert_eq!(receipt.reservation.stop_distance_price_units, Some(50));
        assert_eq!(receipt.reservation.volume, 19210);
    }
    let mut old = serde_json::to_value(replay.entry_receipts[0].reservation).unwrap();
    old.as_object_mut()
        .unwrap()
        .remove("stop_distance_price_units");
    let legacy: rust_strategy_factory::risk::Reservation =
        serde_json::from_value(old.clone()).unwrap();
    assert_eq!(legacy.stop_distance_price_units, None);
    assert_eq!(serde_json::to_value(legacy).unwrap(), old);
}
#[test]
fn unavailable_atr_and_excess_distance_never_fall_back_or_silently_clamp() {
    let mut s = strategy();
    s.stop_policy = Some(Policy::Atr {
        window: 3,
        multiple_bps: 50000,
    });
    let replay = run(&market(), &s);
    assert!(replay.trades.is_empty());
    assert_eq!(replay.final_marked_micro_usd, 20_000_000);
    assert!(
        replay
            .rejections
            .iter()
            .any(|r| r.reason.contains("ATR stop warmup"))
    );
    let mut c = bars();
    c[3].bid.high = 30_000_120;
    c[3].ask.high = 30_000_140;
    s.stop_ticks = 2;
    s.stop_policy = Some(Policy::Atr {
        window: 2,
        multiple_bps: 100000,
    });
    let p = Profile::synthetic("EURUSD").unwrap();
    let prepared = stops::prepare(&s, &p, &c).unwrap();
    assert_eq!(prepared.at(1), Distance::Ready(80));
    assert_eq!(prepared.at(3), Distance::ExceedsBudget);
    s.stop_policy = Some(Policy::Atr {
        window: 1,
        multiple_bps: 20000,
    });
    assert!(s.canonical().is_err());
    assert!(indicators::atr_outward(&c, 0).is_err());
}

#[test]
fn atr_policy_appears_in_autonomous_regions_and_frozen_parameter_probes() {
    let count = rust_strategy_factory::strategies::family_templates().len() as u64;
    for slot in 0..count {
        let (_, s) = rust_strategy_factory::research::candidate_at(10 * count + slot, 42).unwrap();
        assert!(matches!(
            s.stop_policy,
            Some(Policy::Atr {
                window: 14,
                multiple_bps: 20000
            })
        ));
    }
    let s = strategy();
    let cases = rust_strategy_factory::robustness::cases(&s, &Costs::default()).unwrap();
    assert!(cases.iter().any(|c| c.name == "atr-window-higher"));
    assert!(cases.iter().any(|c| c.name == "atr-multiple-lower"));
    assert!(cases.iter().any(|c| c.name == "atr-multiple-higher"));
    assert!(!cases.iter().any(|c| c.name == "atr-window-lower"));
    for case in cases {
        case.strategy.canonical().unwrap();
    }
}

#[test]
fn misspelled_stop_or_rule_fields_cannot_silently_select_a_different_model() {
    let mut value = serde_json::to_value(strategy()).unwrap();
    value.as_object_mut().unwrap().remove("stop_policy");
    value["stopPolicy"] = serde_json::json!({"Atr":{"window":2,"multiple_bps":50000}});
    assert!(serde_json::from_value::<Strategy>(value).is_err());
    assert!(
        serde_json::from_value::<Policy>(
            serde_json::json!({"Atr":{"window":2,"multiple_bps":50000,"ignored":true}})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<Rule>(
            serde_json::json!({"AboveMean":{"window":2,"ignored":true}})
        )
        .is_err()
    );
}
