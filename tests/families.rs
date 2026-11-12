use rust_strategy_factory::{
    candles::{Candle, Ohlc, Timeframe},
    contracts::Side,
    indicators,
    strategies::{self, Family, Features, Rule},
};
use std::collections::{BTreeMap, BTreeSet};
fn fixture() -> Vec<Candle> {
    (0..1200)
        .map(|i| {
            let phase = i % 80;
            let wave = if phase < 40 { phase } else { 80 - phase };
            let close = 100000 + wave * 50;
            let wick = if i % 97 == 0 { 1000 } else { 20 };
            Candle {
                start_ms: 1735689600000 + i * 60000,
                available_ms: 1735689660000 + i * 60000,
                publication_sequence: i as u64,
                bid: Ohlc {
                    open: close,
                    high: close + wick,
                    low: close - wick,
                    close,
                },
                ask: Ohlc {
                    open: close + 20,
                    high: close + wick + 20,
                    low: close - wick + 20,
                    close: close + 20,
                },
                events: 6,
            }
        })
        .collect()
}
#[test]
fn every_family_has_valid_unique_templates_and_actual_nonzero_signal_cohorts() {
    let candidates = strategies::family_templates();
    let mut ids = BTreeSet::new();
    let mut counts = BTreeMap::new();
    let c = fixture();
    let mut f = Features::new(&c, 1200 * 40).unwrap();
    for candidate in candidates
        .iter()
        .filter(|c| c.strategy.timeframe == Timeframe::M1 && c.strategy.side == Side::Long)
    {
        candidate.strategy.canonical().unwrap();
        let n = f.signal(&candidate.strategy.rule).unwrap().count();
        *counts.entry(candidate.family).or_insert(0) += n;
    }
    for family in Family::ALL {
        assert!(counts[&family] > 0, "no signal cohort for {family:?}");
    }
    for candidate in &candidates {
        assert!(
            ids.insert(candidate.strategy.identity().unwrap()),
            "duplicate template"
        );
    }
    assert!(f.cache_hits > 0);
    assert!(f.feature_builds > 0);
}
#[test]
fn all_family_signals_are_prefix_causal_and_reusable() {
    let full = fixture();
    let prefix = &full[..600];
    let mut f = Features::new(&full, 1200 * 40).unwrap();
    let mut p = Features::new(prefix, 600 * 40).unwrap();
    for candidate in strategies::family_templates()
        .into_iter()
        .filter(|c| c.strategy.timeframe == Timeframe::M1 && c.strategy.stop_ticks == 30)
    {
        let a = f.signal(&candidate.strategy.rule).unwrap();
        let b = p.signal(&candidate.strategy.rule).unwrap();
        for i in 0..prefix.len() {
            assert_eq!(
                a.get(i),
                b.get(i),
                "future leakage in {:?}",
                candidate.family
            );
        }
        assert_eq!(a, f.signal(&candidate.strategy.rule).unwrap());
    }
}
#[test]
fn prior_extrema_exclude_current_bar_and_match_brute_force() {
    let high = [10, 9, 12, 7, 1000, 6, 8, 9];
    let low = [5, 4, 7, 2, -1000, 1, 3, 4];
    for window in 1..=4 {
        let (upper, lower) = indicators::prior_extrema(&high, &low, window).unwrap();
        for i in 0..high.len() {
            assert_eq!(
                upper[i],
                if i >= window {
                    high[i - window..i].iter().max().copied()
                } else {
                    None
                }
            );
            assert_eq!(
                lower[i],
                if i >= window {
                    low[i - window..i].iter().min().copied()
                } else {
                    None
                }
            );
        }
    }
}
#[test]
fn shared_cache_evicts_without_changing_results_and_rejects_oversized_roots() {
    let c = fixture();
    let mut tiny = Features::new(&c, c.len() * 3).unwrap();
    for window in 2..50 {
        let r = Rule::AboveMean { window };
        let a = tiny.signal(&r).unwrap();
        let b = Features::new(&c, c.len() * 3).unwrap().signal(&r).unwrap();
        assert_eq!(a, b);
    }
    let r = Rule::All(vec![
        Rule::AboveMean { window: 3 },
        Rule::RsiThreshold {
            window: 4,
            basis_points: 5000,
            above: true,
        },
        Rule::Donchian {
            window: 5,
            above: true,
        },
    ]);
    assert!(tiny.signal(&r).is_err());
}
#[test]
fn utc_session_masks_use_publication_hour_and_weekday_and_support_overnight() {
    let mut c = fixture();
    let time = |s| {
        chrono::DateTime::parse_from_rfc3339(s)
            .unwrap()
            .timestamp_millis()
    };
    for (bar, s) in c.iter_mut().zip([
        "2026-11-09T21:59:00Z",
        "2026-11-09T22:00:00Z",
        "2026-11-10T01:59:00Z",
        "2026-11-10T02:00:00Z",
        "2026-11-14T23:00:00Z",
    ]) {
        bar.available_ms = time(s);
    }
    c.truncate(5);
    let m = Features::new(&c, 100)
        .unwrap()
        .signal(&Rule::UtcSession {
            start_hour: 22,
            end_hour: 2,
            weekdays: 31,
        })
        .unwrap();
    assert_eq!(
        (0..5).map(|i| m.get(i)).collect::<Vec<_>>(),
        vec![false, true, true, false, false]
    );
    assert!(
        Rule::UtcSession {
            start_hour: 24,
            end_hour: 2,
            weekdays: 31
        }
        .canonical()
        .is_err()
    );
}

#[test]
fn every_family_reaches_nonzero_closed_tick_trades_on_actual_synthetic_events() {
    use rust_strategy_factory::{
        candles,
        contracts::{Conversion, Profile},
        data::{self, Tick},
        tick_engine::{self, Costs},
    };
    let bars = fixture();
    let mut ticks = Vec::new();
    for bar in &bars {
        for (offset, price) in [
            (0, bar.bid.open),
            (10000, bar.bid.high),
            (20000, bar.bid.low),
            (50000, bar.bid.close),
        ] {
            ticks.push(Tick {
                timestamp_ms: bar.start_ms + offset,
                sequence: ticks.len() as u64,
                bid: price,
                ask: price + 20,
            });
        }
    }
    ticks.push(Tick {
        timestamp_ms: bars.last().unwrap().start_ms + 60000,
        sequence: ticks.len() as u64,
        bid: 100000,
        ask: 100020,
    });
    let d = data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        ticks,
        "SYNTHETIC_TEST_ONLY family replay",
        "generated".into(),
        true,
    )
    .unwrap();
    let c = candles::build(&d, Timeframe::M1).unwrap();
    let mut f = Features::new(&c, c.len() * 40).unwrap();
    let candidates = strategies::family_templates();
    for family in Family::ALL {
        let strategy = &candidates
            .iter()
            .find(|candidate| {
                candidate.family == family
                    && candidate.strategy.timeframe == Timeframe::M1
                    && candidate.strategy.side == Side::Long
                    && candidate.strategy.stop_ticks == 100
                    && f.signal(&candidate.strategy.rule).unwrap().count() > 0
            })
            .unwrap()
            .strategy;
        let mask = f.signal(&strategy.rule).unwrap();
        let r = tick_engine::replay(
            &d,
            &c,
            &mask,
            strategy,
            &Costs::default(),
            &vec![Conversion::usd(); d.ticks.len()],
        )
        .unwrap();
        assert!(!r.trades.is_empty(), "no closed trades for {family:?}");
    }
}
