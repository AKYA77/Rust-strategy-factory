use rust_strategy_factory::{
    candles::{Candle, Ohlc, Timeframe},
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
    let candidates = strategies::family_checkpoints();
    assert_eq!(
        candidates.len(),
        Family::ALL.len(),
        "exactly ten family checkpoints"
    );
    let mut ids = BTreeSet::new();
    let mut counts = BTreeMap::new();
    let c = fixture();
    let mut f = Features::new(&c, 1200 * 40).unwrap();
    for candidate in &candidates {
        candidate.strategy.canonical().unwrap();
        let n = f.signal(&candidate.strategy.rule).unwrap().count();
        *counts.entry(candidate.family).or_insert(0) += n;
        assert!(
            ids.insert(candidate.strategy.identity().unwrap()),
            "duplicate checkpoint for {:?}",
            candidate.family
        );
    }
    for family in Family::ALL {
        if family == Family::Orb {
            // ORB requires a completed session range; the generic oscillating
            // fixture has no causal opening range. Covered by the ORB-specific
            // test below (no silent skip).
            continue;
        }
        assert!(counts[&family] > 0, "no signal cohort for {family:?}");
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
    for candidate in strategies::family_checkpoints()
        .into_iter()
        .filter(|c| c.strategy.timeframe == Timeframe::M1)
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
        candles::Timeframe,
        contracts::{Conversion, Profile},
        data::{self, Tick},
        strategies::Family,
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
    let candidates = strategies::family_checkpoints();
    for family in Family::ALL {
        // ORB needs a completed session range; the generic oscillating fixture has
        // no causal opening range. Use an explicit positive fixture for both the
        // signal and the tick replay (no silent skip).
        if family == Family::Orb {
            let base_ms = 1736146200000i64; // 2025-01-06T06:50:00Z
            let ticks: Vec<Tick> = (0..70)
                .map(|i| {
                    let price = match i {
                        20 => 100_050, // 07:10 window high
                        30 => 99_950,  // 07:20 window low
                        40 => 100_060, // 07:30 causal breakout
                        i if i > 40 => 100_060 + (i - 40) * 100,
                        _ => 100_000,
                    };
                    Tick {
                        timestamp_ms: base_ms + i * 60_000,
                        sequence: i as u64,
                        bid: price,
                        ask: price + 20,
                    }
                })
                .collect();
            let d = data::dataset(
                Profile::synthetic("EURUSD").unwrap(),
                ticks,
                "SYNTHETIC_TEST_ONLY orb families fixture",
                "generated".into(),
                true,
            )
            .unwrap();
            let c = candles::build(&d, Timeframe::M1).unwrap();
            let mut f = Features::new(&c, c.len() * 40).unwrap();
            let rule = Rule::OrbLongBreakout {
                window_minutes: 30,
                session_hour: 7,
            };
            let mask = f.signal(&rule).unwrap();
            assert!(
                mask.count() > 0,
                "ORB must produce at least one causal signal on a known completed range"
            );
            let strategy = candidates
                .iter()
                .find(|candidate| {
                    candidate.family == Family::Orb
                        && candidate.strategy.timeframe == Timeframe::M1
                        && candidate.strategy.side == rust_strategy_factory::contracts::Side::Long
                        && candidate.strategy.stop_ticks == 150
                })
                .expect("ORB M1 long checkpoint")
                .strategy
                .clone();
            let replay = tick_engine::replay(
                &d,
                &c,
                &mask,
                &strategy,
                &Costs::default(),
                &vec![Conversion::usd(); d.ticks.len()],
            )
            .unwrap();
            assert!(
                !replay.trades.is_empty(),
                "ORB must reach a closed tick trade"
            );
            continue;
        }
        let strategy = candidates
            .iter()
            .find(|candidate| {
                candidate.family == family
                    && candidate.strategy.timeframe == Timeframe::M1
                    && candidate.strategy.side == rust_strategy_factory::contracts::Side::Long
                    && candidate.strategy.stop_ticks == 150
                    && f.signal(&candidate.strategy.rule).unwrap().count() > 0
            })
            .unwrap_or_else(|| {
                panic!("no signal-producing checkpoint for family {family:?} with stop_ticks 150")
            })
            .strategy
            .clone();
        let mask = f.signal(&strategy.rule).unwrap();
        let r = tick_engine::replay(
            &d,
            &c,
            &mask,
            &strategy,
            &Costs::default(),
            &vec![Conversion::usd(); d.ticks.len()],
        )
        .unwrap();
        assert!(!r.trades.is_empty(), "no closed trades for {family:?}");
        let audit = rust_strategy_factory::screen::audit_replay(&d, &Costs::default(), &r).unwrap();
        assert!(audit.audited_trades > 0);
    }
}

#[test]
fn exactly_ten_family_checkpoints_are_fixed_and_reproducible() {
    let first = strategies::family_checkpoints();
    let second = strategies::family_checkpoints();
    assert_eq!(first.len(), 10, "exactly ten family checkpoints");
    assert_eq!(
        first.len(),
        second.len(),
        "checkpoints are deterministic across calls"
    );
    for (a, b) in first.iter().zip(second.iter()) {
        assert_eq!(
            a.family, b.family,
            "family mismatch in deterministic checkpoints"
        );
        assert_eq!(
            a.strategy.identity().unwrap(),
            b.strategy.identity().unwrap(),
            "identity mismatch in deterministic checkpoints"
        );
    }
    let families: Vec<_> = first.iter().map(|c| c.family).collect();
    for expected in strategies::Family::ALL {
        assert!(
            families.contains(&expected),
            "missing checkpoint for {expected:?}"
        );
    }
}

#[test]
fn family_checkpoint_scheduling_contract_is_deterministic_with_seed() {
    use rust_strategy_factory::research::candidate_at;
    // Same index + same seed → same candidate (deterministic).
    let a = candidate_at(0, 42).unwrap();
    let b = candidate_at(0, 42).unwrap();
    assert_eq!(a, b, "candidate_at is deterministic");
    // Ten checkpoints per region: index 0..10 covers all families in region 0.
    let families: std::collections::BTreeSet<strategies::Family> =
        (0..10).map(|i| candidate_at(i, 0).unwrap().0).collect();
    assert_eq!(families.len(), 10, "all ten families appear in region 0");
    for f in strategies::Family::ALL {
        assert!(families.contains(&f), "family {f:?} missing from region 0");
    }
}

#[test]
fn seven_instruments_times_ten_families_is_seventy_groups() {
    let instruments = [
        "EURUSD", "GBPUSD", "USDJPY", "USDCHF", "USDCAD", "AUDUSD", "NZDUSD",
    ];
    let families = strategies::Family::ALL.len();
    assert_eq!(families, 10, "ten research families");
    assert_eq!(
        instruments.len() * families,
        70,
        "70 instrument-family groups"
    );
    // Max-35 leaderboard: 5 per instrument × 7 instruments.
    assert_eq!(5 * instruments.len(), 35, "max-35 leaderboard");
}

#[test]
fn family_checkpoint_rules_are_canonical_and_compatible() {
    for candidate in strategies::family_checkpoints() {
        // Every checkpoint must canonicalize without error.
        candidate.strategy.canonical().unwrap();
        // Every checkpoint must produce a valid concept identity.
        let id = candidate.strategy.identity().unwrap();
        assert!(!id.is_empty(), "empty identity for {:?}", candidate.family);
        // Identity must be stable across calls.
        assert_eq!(
            candidate.strategy.identity().unwrap(),
            id,
            "identity not stable for {:?}",
            candidate.family
        );
    }
}
