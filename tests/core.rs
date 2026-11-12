use rust_strategy_factory::{
    candles::{self, Timeframe},
    contracts::{self, Conversion, Profile, Side},
    data::{self, DataClass, Tick},
    indicators::{self, Mask},
    strategies::{self, Features, Rule, Strategy},
    tick_engine::{self, Costs, ExitReason},
};

fn tick(ms: i64, seq: u64, bid: i64, ask: i64) -> Tick {
    Tick {
        timestamp_ms: ms,
        sequence: seq,
        bid,
        ask,
    }
}
fn sample(last_bid: i64, last_ask: i64, last_ms: i64) -> data::Dataset {
    data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        vec![
            tick(0, 0, 100000, 100010),
            tick(60000, 1, 100020, 100030),
            tick(120000, 2, 100040, 100050),
            tick(120000, 3, 100050, 100060),
            tick(last_ms, 4, last_bid, last_ask),
        ],
        "synthetic deterministic unit fixture",
        "not-an-archive".into(),
        true,
    )
    .unwrap()
}
fn strategy(side: Side) -> Strategy {
    Strategy {
        timeframe: Timeframe::M1,
        side,
        stop_ticks: 30,
        rule: Rule::AboveMean { window: 2 },
    }
}
fn run(d: &data::Dataset, side: Side) -> tick_engine::Replay {
    let s = strategy(side);
    let c = candles::build(d, s.timeframe).unwrap();
    let m = Features::new(&c, 10000).unwrap().signal(&s.rule).unwrap();
    tick_engine::replay(
        d,
        &c,
        &m,
        &s,
        &Costs::default(),
        &vec![Conversion::usd(); d.ticks.len()],
    )
    .unwrap()
}
#[test]
fn profiles_and_exact_decimals() {
    for symbol in contracts::SYMBOLS {
        let p = Profile::synthetic(symbol).unwrap();
        p.validate().unwrap();
        assert!(p.research_only);
        assert!(p.price("1.000000001").is_err());
        assert!(p.price("-1").is_err());
        assert!(p.price("1e3").is_err());
        assert!(p.price("NaN").is_err());
    }
    assert_eq!(
        Profile::synthetic("USDJPY")
            .unwrap()
            .price("150.123")
            .unwrap(),
        150123
    );
    let gold = Profile::synthetic("XAUUSD").unwrap();
    assert_eq!(gold.price("2500.12").unwrap(), 250012);
    assert_eq!(
        contracts::pnl(&gold, 100, 1_000_000, Conversion::usd()).unwrap(),
        100_000_000
    );
    let eur = Profile::synthetic("EURUSD").unwrap();
    assert_eq!(
        contracts::pnl(&eur, 100, 10_000, Conversion::usd()).unwrap(),
        1_000_000
    );
    assert!(Profile::synthetic("BTCUSD").is_err());
}
#[test]
fn minimum_lot_and_adverse_rounding() {
    let mut p = Profile::synthetic("EURUSD").unwrap();
    p.min_micro_lots = 10000;
    p.step_micro_lots = 10000;
    assert!(contracts::size(&p, 20_000_000, 100000, 1000, Conversion::usd()).is_err());
    p.price_scale = 100_000;
    p.contract_units = 1;
    assert_eq!(contracts::pnl(&p, -1, 1, Conversion::usd()).unwrap(), -1);
    assert_eq!(contracts::pnl(&p, 1, 1, Conversion::usd()).unwrap(), 0);
}
#[test]
fn conversion_never_uses_future_or_stale_quotes() {
    let p = Profile::synthetic("USDJPY").unwrap();
    assert!(Conversion::usd().validate(&p, 0, 60000).is_err());
    let mut fx = Conversion {
        usd_num: 1,
        quote_den: 150,
        as_of_ms: 100,
    };
    assert!(fx.validate(&p, 99, 1000).is_err());
    assert!(fx.validate(&p, 1101, 1000).is_err());
    fx.validate(&p, 1000, 1000).unwrap();
    assert_eq!(contracts::pnl(&p, 150, 1_000_000, fx).unwrap(), 100_000_000);
    fx.quote_den = 0;
    assert!(fx.validate(&p, 1000, 1000).is_err());
}
#[test]
fn quarantine_preserves_bad_events_and_timestamp_ties() {
    let d = sample(100200, 100210, 121000);
    assert_eq!(d.manifest.timestamp_ties, 1);
    d.verify().unwrap();
    let mut ticks = d.ticks.clone();
    ticks[4].sequence = 3;
    let bad = data::dataset(d.profile.clone(), ticks, "bad", "x".into(), false).unwrap();
    assert_eq!(bad.manifest.class, DataClass::Quarantined);
    assert!(bad.verify().is_err());
    let mut altered = d.clone();
    altered.ticks[0].bid += 1;
    assert!(altered.verify().is_err());
    let real = data::dataset(
        d.profile,
        d.ticks,
        "unreviewed real source",
        "x".into(),
        false,
    )
    .unwrap();
    assert_eq!(real.manifest.class, DataClass::RealStructurallyValid);
}
#[test]
fn candles_are_closed_causal_and_never_fill_gaps() {
    let d = sample(100200, 100210, 121000);
    let c = candles::build(&d, Timeframe::M1).unwrap();
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].bid.close, 100000);
    assert!(candles::available(&c, 59999, 100).is_none());
    assert!(candles::available(&c, 60000, 0).is_none());
    assert_eq!(candles::available(&c, 60000, 1).unwrap().start_ms, 0);
    for tf in Timeframe::ALL {
        let long = data::dataset(
            d.profile.clone(),
            vec![
                tick(0, 0, 100000, 100010),
                tick(86400000 * 3, 1, 110000, 110010),
            ],
            "synthetic",
            "x".into(),
            true,
        )
        .unwrap();
        let c = candles::build(&long, tf).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].available_ms, 86400000 * 3);
    }
}
#[test]
fn indicators_are_causal_and_masks_preserve_tail() {
    assert_eq!(
        indicators::sma(&[1, 2, 3, 4], 3).unwrap(),
        vec![None, None, Some(2), Some(3)]
    );
    let base = indicators::ema(&[1, 2, 3, 4], 2).unwrap();
    assert_eq!(
        base,
        indicators::ema(&[1, 2, 3, 4, 999999], 2).unwrap()[..4]
    );
    assert_eq!(indicators::rsi(&[1, 2, 3, 4], 2).unwrap()[3], Some(100.0));
    assert_eq!(indicators::rsi(&[4, 3, 2, 1], 2).unwrap()[3], Some(0.0));
    let a = Mask::from_bools((0..65).map(|_| true));
    assert_eq!(a.count(), 65);
    assert!(!a.get(65));
    let b = Mask::from_bools((0..65).map(|i| i % 2 == 0));
    assert_eq!(a.combine(&b, true).unwrap().count(), 33);
    assert!(a.combine(&Mask::from_bools([true]), true).is_err());
}
#[test]
fn canonical_identity_and_bounded_shared_features() {
    let a = Rule::AboveMean { window: 5 };
    let b = Rule::Rising { lookback: 2 };
    let mut s = strategy(Side::Long);
    s.rule = Rule::All(vec![a.clone(), b.clone(), a.clone()]);
    let id = s.identity().unwrap();
    s.rule = Rule::All(vec![b, a]);
    assert_eq!(id, s.identity().unwrap());
    s.stop_ticks += 1;
    assert_ne!(id, s.identity().unwrap());
    let grid = strategies::generate();
    let identities: std::collections::BTreeSet<_> =
        grid.iter().map(|s| s.identity().unwrap()).collect();
    assert_eq!(grid.len(), identities.len());
    let c = candles::build(&sample(100200, 100210, 121000), Timeframe::M1).unwrap();
    assert!(Features::new(&c, 1).is_err());
    let mut f = Features::new(&c, 2).unwrap();
    assert!(f.signal(&Rule::AboveMean { window: 2 }).is_err());
    let mut f = Features::new(&c, 100).unwrap();
    let sc = strategies::screen(&mut f, &strategy(Side::Long)).unwrap();
    assert!(sc.requires_tick_verification);
    assert_eq!(sc.signals, 1);
}
#[test]
fn spread_floor_never_improves_prices_and_wider_spread_remains() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let c = Costs::default();
    assert_eq!(
        c.effective(&p, tick(0, 0, 100000, 100005)).unwrap(),
        (100000, 100020)
    );
    assert_eq!(
        c.effective(&p, tick(0, 0, 100000, 100050)).unwrap(),
        (100000, 100050)
    );
    assert!(c.effective(&p, tick(0, 0, 100000, 99999)).is_err());
}
#[test]
fn long_target_uses_subsequent_tied_event_and_no_limit_slippage() {
    let r = run(&sample(100200, 100210, 121000), Side::Long);
    assert_eq!(r.trades.len(), 1);
    let t = &r.trades[0];
    assert_eq!((t.entry_event, t.exit_event), (3, 4));
    assert_eq!(
        (t.entry, t.stop, t.target, t.exit),
        (100090, 100060, 100180, 100180)
    );
    assert_eq!(t.reason, ExitReason::TargetLimit);
    assert_eq!(t.volume_micro_lots, 19980);
    assert_eq!(t.net_micro_usd, 1_798_200);
    assert_eq!(r.realized_micro_usd, 21_798_200);
    assert!(r.unresolved.is_none());
}
#[test]
fn gap_stop_losses_are_not_capped_to_ten_percent() {
    let r = run(&sample(100000, 100010, 121000), Side::Long);
    let t = &r.trades[0];
    assert_eq!(t.exit, 99980);
    assert_eq!(t.reason, ExitReason::StopMarket);
    assert_eq!(t.net_micro_usd, -2_197_800);
    assert!(t.net_micro_usd < -2_000_000);
    assert_eq!(r.realized_micro_usd, 20_000_000 + t.net_micro_usd);
}
#[test]
fn short_uses_ask_for_exit_and_adverse_sell_slippage() {
    let r = run(&sample(99900, 99930, 121000), Side::Short);
    let t = &r.trades[0];
    assert_eq!(
        (t.entry, t.stop, t.target, t.exit),
        (100030, 100060, 99940, 99940)
    );
    assert_eq!(t.reason, ExitReason::TargetLimit);
    let r = run(&sample(100100, 100150, 121000), Side::Short);
    assert_eq!(r.trades[0].exit, 100170);
}
#[test]
fn deadline_requires_a_real_subsequent_event_and_unresolved_stays_open() {
    let r = run(&sample(100080, 100090, 121000), Side::Long);
    assert!(r.unresolved.is_some());
    assert!(r.trades.is_empty());
    let r = run(
        &sample(100200, 100210, 120000 + contracts::MAX_HOLD_MS),
        Side::Long,
    );
    assert_eq!(r.trades[0].reason, ExitReason::DeadlineMarket);
    assert_eq!(r.trades[0].exit, 100180);
}
#[test]
fn replay_rejects_candle_and_signal_tampering() {
    let d = sample(100200, 100210, 121000);
    let s = strategy(Side::Long);
    let mut c = candles::build(&d, s.timeframe).unwrap();
    let m = Features::new(&c, 100).unwrap().signal(&s.rule).unwrap();
    c[0].bid.close += 1;
    assert!(
        tick_engine::replay(
            &d,
            &c,
            &m,
            &s,
            &Costs::default(),
            &vec![Conversion::usd(); d.ticks.len()]
        )
        .is_err()
    );
}
