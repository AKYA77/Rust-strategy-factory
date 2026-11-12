use rust_strategy_factory::{
    candles::{Candle, Ohlc, Timeframe},
    contracts::{Profile, Side},
    data::{self, Tick},
    screen::{self, Position, Possible},
    tick_engine::Costs,
};
fn bar() -> Candle {
    Candle {
        start_ms: 60000,
        available_ms: 120000,
        publication_sequence: 10,
        bid: Ohlc {
            open: 100000,
            high: 100200,
            low: 99900,
            close: 100100,
        },
        ask: Ohlc {
            open: 100020,
            high: 100220,
            low: 99920,
            close: 100120,
        },
        events: 4,
    }
}
#[test]
fn collisions_are_not_resolved_by_guessing_ohlc_order() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let pos = Position {
        side: Side::Long,
        entry_ms: 0,
        stop: 99950,
        target: 100150,
        deadline_ms: 200000,
    };
    let b = screen::barrier_bounds(&p, &Costs::default(), &bar(), pos).unwrap();
    assert!(b.sl_tp_collision);
    assert_eq!(
        b.possible,
        vec![Possible::StopMarket, Possible::TargetLimit]
    );
    let b = screen::barrier_bounds(
        &p,
        &Costs::default(),
        &bar(),
        Position {
            target: 100300,
            ..pos
        },
    )
    .unwrap();
    assert_eq!(b.possible, vec![Possible::StopMarket]);
    let b = screen::barrier_bounds(
        &p,
        &Costs::default(),
        &bar(),
        Position { stop: 99800, ..pos },
    )
    .unwrap();
    assert_eq!(b.possible, vec![Possible::TargetLimit]);
}
#[test]
fn entry_and_deadline_ambiguity_preserve_all_possible_outcomes() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let pos = Position {
        side: Side::Long,
        entry_ms: 60000,
        stop: 99950,
        target: 100150,
        deadline_ms: 90000,
    };
    let b = screen::barrier_bounds(&p, &Costs::default(), &bar(), pos).unwrap();
    assert!(b.entry_or_deadline_uncertain);
    assert!(b.possible.contains(&Possible::NoHit));
    assert!(b.possible.contains(&Possible::DeadlineMarket));
    let b = screen::barrier_bounds(
        &p,
        &Costs::default(),
        &bar(),
        Position {
            entry_ms: 0,
            deadline_ms: 60000,
            ..pos
        },
    )
    .unwrap();
    assert_eq!(b.possible, vec![Possible::DeadlineMarket]);
}
#[test]
fn ask_floor_minimum_does_not_assume_unknown_bid_ask_correlation() {
    let mut c = bar();
    c.bid = Ohlc {
        open: 110,
        high: 110,
        low: 80,
        close: 110,
    };
    c.ask = Ohlc {
        open: 120,
        high: 130,
        low: 100,
        close: 120,
    };
    let mut p = Profile::synthetic("EURUSD").unwrap();
    p.pip = 10;
    let b = screen::barrier_bounds(
        &p,
        &Costs::default(),
        &c,
        Position {
            side: Side::Short,
            entry_ms: 0,
            stop: 200,
            target: 105,
            deadline_ms: 200000,
        },
    )
    .unwrap();
    assert_eq!(b.possible, vec![Possible::TargetLimit, Possible::NoHit]);
}
#[test]
fn conservative_sets_contain_sequential_first_hits_for_seeded_quote_streams() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let costs = Costs::default();
    let mut rng = 123_u64;
    let mut next = || {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        rng
    };
    for _ in 0..100 {
        let ticks = (0..40)
            .map(|i| {
                let bid = 100000 + (next() % 500) as i64;
                Tick {
                    timestamp_ms: 60000 + i * 1000,
                    sequence: i as u64,
                    bid,
                    ask: bid + (next() % 80) as i64,
                }
            })
            .chain(std::iter::once(Tick {
                timestamp_ms: 120000,
                sequence: 40,
                bid: 100000,
                ask: 100020,
            }))
            .collect();
        let d = data::dataset(
            p.clone(),
            ticks,
            "SYNTHETIC_TEST_ONLY bounds fixture",
            "generated".into(),
            true,
        )
        .unwrap();
        let bars = rust_strategy_factory::candles::build(&d, Timeframe::M1).unwrap();
        for side in [Side::Long, Side::Short] {
            for _ in 0..20 {
                let (stop, target) = if side == Side::Long {
                    (
                        99900 + (next() % 500) as i64,
                        100450 + (next() % 300) as i64,
                    )
                } else {
                    (
                        100450 + (next() % 300) as i64,
                        99900 + (next() % 500) as i64,
                    )
                };
                let pos = Position {
                    side,
                    entry_ms: 0,
                    stop,
                    target,
                    deadline_ms: 200000,
                };
                let set = screen::barrier_bounds(&p, &costs, &bars[0], pos).unwrap();
                let hit = d.ticks[..40]
                    .iter()
                    .find_map(|t| {
                        let (bid, ask) = costs.effective(&p, *t).unwrap();
                        if side == Side::Long && bid <= stop || side == Side::Short && ask >= stop {
                            Some(Possible::StopMarket)
                        } else if side == Side::Long && bid >= target
                            || side == Side::Short && ask <= target
                        {
                            Some(Possible::TargetLimit)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(Possible::NoHit);
                assert!(
                    set.possible.contains(&hit),
                    "excluded actual first hit: {set:?} {hit:?}"
                );
            }
        }
    }
}
