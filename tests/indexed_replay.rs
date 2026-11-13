use rust_strategy_factory::{
    candles::{self, Timeframe},
    contracts::{Conversion, Profile, SYMBOLS, Side},
    data::{self, Tick},
    indexed_replay,
    indicators::Mask,
    strategies::{EntryPlan, Features, Rule, Strategy},
    tick_engine::{self, Costs},
    validation::Interval,
};
fn fixture(symbol: &str, seed: u64, n: usize) -> (data::Dataset, Vec<Conversion>) {
    let p = Profile::synthetic(symbol).unwrap();
    let base = if symbol == "XAUUSD" { 250000 } else { 100000 };
    let mut rng = seed;
    let mut timestamp = 1735689600000i64;
    let ticks = (0..n)
        .map(|i| {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            timestamp += match rng % 100 {
                0 => 90_000_000,
                1..=10 => 0,
                _ => 10000,
            };
            let bid = base + (rng % 3000) as i64 * p.tick;
            Tick {
                timestamp_ms: timestamp,
                sequence: (i as u64) * 2,
                bid,
                ask: bid + (rng % 8) as i64 * p.pip,
            }
        })
        .collect();
    let d = data::dataset(
        p,
        ticks,
        "SYNTHETIC_TEST_ONLY indexed differential",
        "generated".into(),
        true,
    )
    .unwrap();
    let fx = d
        .ticks
        .iter()
        .map(|t| {
            if d.profile.quote == "USD" {
                Conversion::usd()
            } else {
                Conversion {
                    usd_num: 1,
                    quote_den: if symbol == "USDJPY" { 150 } else { 1 },
                    as_of_ms: t.timestamp_ms,
                }
            }
        })
        .collect();
    (d, fx)
}
fn strategy(side: Side, stop_ticks: i64) -> Strategy {
    Strategy {
        timeframe: Timeframe::M1,
        side,
        stop_ticks,
        rule: Rule::AboveMean { window: 2 },
        entry: None,
        stop_policy: None,
    }
}
fn mask(d: &data::Dataset, s: &Strategy, w: Option<&Interval>) -> (Vec<candles::Candle>, Mask) {
    let c = candles::build(d, s.timeframe).unwrap();
    let visible = w.map_or(c.len(), |w| {
        c.partition_point(|c| c.available_ms < w.end_exclusive_ms)
    });
    let mut f = Features::with_context(&c[..visible], 1_000_000, d, s.timeframe).unwrap();
    let prefix = f.signal(&s.rule).unwrap();
    let m = Mask::from_bools((0..c.len()).map(|i| i < visible && prefix.get(i)));
    (c, m)
}
fn reference(
    d: &data::Dataset,
    fx: &[Conversion],
    s: &Strategy,
    costs: &Costs,
    w: Option<&Interval>,
) -> tick_engine::Replay {
    let (c, m) = mask(d, s, w);
    match w {
        Some(w) => tick_engine::replay_window(d, &c, &m, s, costs, fx, w).unwrap(),
        None => tick_engine::replay(d, &c, &m, s, costs, fx).unwrap(),
    }
}
#[test]
fn seeded_full_accounting_matches_all_profiles_sides_costs_windows_and_atr() {
    let mut checked = 0;
    let mut trades = 0;
    for symbol in SYMBOLS {
        for seed in 0..8 {
            let (d, fx) = fixture(symbol, seed + 7, 1200);
            let costs = Costs {
                spread_floor_pips: 2 + (seed % 3) as i64,
                slippage_pips: 2 + (seed % 2) as i64,
                commission_micro_usd_per_lot_per_fill: (seed % 3) as i64 * 1_000_000,
                ..Default::default()
            };
            for side in [Side::Long, Side::Short] {
                for distance in [100, 5000] {
                    let mut s = strategy(side, distance);
                    if seed % 3 == 2 {
                        s.stop_policy = Some(rust_strategy_factory::stops::Policy::Atr {
                            window: 3,
                            multiple_bps: 15000,
                        });
                    }
                    let window = Interval {
                        start_ms: d.ticks[200].timestamp_ms,
                        end_exclusive_ms: d.ticks[1000].timestamp_ms,
                    };
                    for w in [None, Some(&window)] {
                        if w.is_some_and(|w| w.start_ms >= w.end_exclusive_ms) {
                            continue;
                        }
                        let (c, m) = mask(&d, &s, w);
                        let actual =
                            indexed_replay::replay(&d, &c, &m, &s, &costs, &fx, w).unwrap();
                        let expected = reference(&d, &fx, &s, &costs, w);
                        assert_eq!(
                            serde_json::to_value(&actual.replay).unwrap(),
                            serde_json::to_value(&expected).unwrap(),
                            "{symbol} seed{seed} {side:?} stop{distance} window{w:?}"
                        );
                        assert_eq!(actual.used_index, d.profile.quote == "USD");
                        trades += expected.trades.len();
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 512);
    assert!(trades > 0);
}
#[test]
fn long_unresolved_paths_use_ordered_block_drawdown_and_excursion_summaries() {
    for side in [Side::Long, Side::Short] {
        let p = Profile::synthetic("EURUSD").unwrap();
        let ticks = (0..5000)
            .map(|i| {
                let q = i % 200;
                let wave = if q < 100 { q } else { 200 - q };
                let bid = 100000 + wave * 20;
                Tick {
                    timestamp_ms: 1735689600000 + i * 1000,
                    sequence: i as u64,
                    bid,
                    ask: bid + 10 + (i % 4) * 10,
                }
            })
            .collect();
        let d = data::dataset(
            p,
            ticks,
            "SYNTHETIC_TEST_ONLY ordered long position",
            "generated".into(),
            true,
        )
        .unwrap();
        let fx = vec![Conversion::usd(); d.ticks.len()];
        let s = strategy(side, 5000);
        let costs = Costs {
            commission_micro_usd_per_lot_per_fill: 2_000_000,
            ..Default::default()
        };
        let (c, m) = mask(&d, &s, None);
        let actual = indexed_replay::replay(&d, &c, &m, &s, &costs, &fx, None).unwrap();
        let expected = reference(&d, &fx, &s, &costs, None);
        assert_eq!(
            serde_json::to_value(&actual.replay).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        assert!(actual.used_index);
        assert!(actual.replay.unresolved.is_some());
        assert!(actual.replay.maximum_drawdown_micro_usd > 0);
        assert!(actual.work.summary_blocks > 0 && actual.work.barrier_blocks > 0);
    }
}
#[test]
fn pending_fractional_and_nonliteral_usd_cases_use_exact_sequential_fallback() {
    let (d, mut fx) = fixture("EURUSD", 21, 1000);
    let mut s = strategy(Side::Long, 300);
    let costs = Costs::default();
    s.entry = Some(EntryPlan::Limit {
        offset_ticks: 30,
        ttl_ms: 300000,
        cancel_on_signal_false: true,
    });
    let (c, m) = mask(&d, &s, None);
    let actual = indexed_replay::replay(&d, &c, &m, &s, &costs, &fx, None).unwrap();
    assert!(!actual.used_index);
    assert_eq!(
        serde_json::to_value(actual.replay).unwrap(),
        serde_json::to_value(reference(&d, &fx, &s, &costs, None)).unwrap()
    );
    s.entry = None;
    for f in &mut fx {
        f.usd_num = 2;
        f.quote_den = 2;
    }
    let actual = indexed_replay::replay(&d, &c, &m, &s, &costs, &fx, None).unwrap();
    assert!(!actual.used_index);
    assert_eq!(
        serde_json::to_value(actual.replay).unwrap(),
        serde_json::to_value(reference(&d, &fx, &s, &costs, None)).unwrap()
    );
    let mut p = d.profile.clone();
    p.contract_units = 1;
    let d = data::dataset(
        p,
        d.ticks,
        "SYNTHETIC_TEST_ONLY fractional cash grid",
        "generated".into(),
        true,
    )
    .unwrap();
    let fx = vec![Conversion::usd(); d.ticks.len()];
    let (c, m) = mask(&d, &s, None);
    let actual = indexed_replay::replay(&d, &c, &m, &s, &costs, &fx, None).unwrap();
    assert!(!actual.used_index);
    assert!(actual.fallback_reason.unwrap().contains("integral"));
    assert_eq!(
        serde_json::to_value(actual.replay).unwrap(),
        serde_json::to_value(reference(&d, &fx, &s, &costs, None)).unwrap()
    );
}
#[test]
fn forged_masks_and_future_fx_scope_match_authoritative_boundaries() {
    let (d, mut fx) = fixture("EURUSD", 17, 1000);
    let s = strategy(Side::Long, 300);
    let costs = Costs::default();
    let (c, m) = mask(&d, &s, None);
    let forged = Mask::from_bools((0..c.len()).map(|i| !m.get(i)));
    assert!(indexed_replay::replay(&d, &c, &forged, &s, &costs, &fx, None).is_err());
    fx.last_mut().unwrap().as_of_ms = d.manifest.last_ms + 1;
    assert_eq!(
        indexed_replay::replay(&d, &c, &m, &s, &costs, &fx, None).unwrap_err(),
        tick_engine::replay(&d, &c, &m, &s, &costs, &fx).unwrap_err()
    );
    let w = Interval {
        start_ms: d.manifest.first_ms,
        end_exclusive_ms: d.ticks[700].timestamp_ms,
    };
    let (c, m) = mask(&d, &s, Some(&w));
    let actual = indexed_replay::replay(&d, &c, &m, &s, &costs, &fx, Some(&w)).unwrap();
    assert!(actual.used_index);
    assert_eq!(
        serde_json::to_value(actual.replay).unwrap(),
        serde_json::to_value(reference(&d, &fx, &s, &costs, Some(&w))).unwrap()
    );
}

#[test]
fn deadline_precedes_gap_target_and_full_u64_tied_event_order_is_preserved() {
    let deadline = 120000 + 86_400_000;
    let p = Profile::synthetic("EURUSD").unwrap();
    let ticks = [
        (0, 100000),
        (60000, 100020),
        (120000, 100040),
        (120000, 100050),
        (deadline, 101000),
        (deadline, 100000),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (timestamp_ms, bid))| Tick {
        timestamp_ms,
        sequence: u64::MAX - 5 + i as u64,
        bid,
        ask: bid + 10,
    })
    .collect();
    let d = data::dataset(
        p,
        ticks,
        "SYNTHETIC_TEST_ONLY deadline/u64 ties",
        "generated".into(),
        true,
    )
    .unwrap();
    let fx = vec![Conversion::usd(); d.ticks.len()];
    let s = strategy(Side::Long, 50);
    let costs = Costs::default();
    let (c, m) = mask(&d, &s, None);
    let actual = indexed_replay::replay(&d, &c, &m, &s, &costs, &fx, None).unwrap();
    let expected = reference(&d, &fx, &s, &costs, None);
    assert_eq!(
        serde_json::to_value(&actual.replay).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(actual.used_index);
    assert_eq!(
        actual.replay.trades[0].reason,
        tick_engine::ExitReason::DeadlineMarket
    );
    assert_eq!(actual.replay.trades[0].exit, 100980);
    assert_eq!(actual.replay.trades[0].crossed_gap_ms, 86_400_000);
    assert_eq!(actual.replay.trades[0].exit_event, u64::MAX - 1);
    assert_eq!(
        actual.replay.unresolved.as_ref().unwrap().entry_event,
        u64::MAX
    );
    let empty = Interval {
        start_ms: -1000,
        end_exclusive_ms: 0,
    };
    let (c, m) = mask(&d, &s, Some(&empty));
    let result = indexed_replay::replay(&d, &c, &m, &s, &costs, &fx, Some(&empty)).unwrap();
    assert_eq!(
        serde_json::to_value(result.replay).unwrap(),
        serde_json::to_value(reference(&d, &fx, &s, &costs, Some(&empty))).unwrap()
    );
}
