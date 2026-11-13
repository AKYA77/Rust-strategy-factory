//! Complete financial engine comparison; explicitly not factory/registry throughput.
use rust_strategy_factory::{
    candles::{self, Timeframe},
    contracts::{Conversion, Profile, SYMBOLS, Side},
    data::{self, Tick},
    indexed_replay::{self, Index},
    research,
    strategies::{Features, Rule, Strategy},
    tick_engine::{self, Costs},
};
use serde_json::json;
use std::time::Instant;
fn main() {
    let costs = Costs {
        commission_micro_usd_per_lot_per_fill: 1_000_000,
        ..Default::default()
    };
    let mut corpus = Vec::new();
    for symbol in SYMBOLS {
        let p = Profile::synthetic(symbol).unwrap();
        let base = if symbol == "XAUUSD" {
            250000
        } else if symbol == "USDJPY" {
            150000
        } else {
            100000
        };
        let mut time = 1735689600000;
        let ticks = (0..50000)
            .map(|i| {
                time += if i % 9973 == 0 {
                    90_000_000
                } else if i % 19 == 0 {
                    0
                } else {
                    10000
                };
                let n = i % 300;
                let wave = if n < 150 { n } else { 300 - n };
                let bid = base + wave * p.pip * 5;
                Tick {
                    timestamp_ms: time,
                    sequence: i as u64,
                    bid,
                    ask: bid + p.pip + (i % 5) * p.pip,
                }
            })
            .collect();
        let d = data::dataset(
            p,
            ticks,
            "SYNTHETIC_TEST_ONLY indexed full engine",
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
            .collect::<Vec<_>>();
        let c = candles::build(&d, Timeframe::M1).unwrap();
        let mut f = Features::with_context(&c, 1_000_000, &d, Timeframe::M1).unwrap();
        let rule = Rule::AboveMean { window: 5 };
        let mask = f.signal(&rule).unwrap();
        corpus.push((d, fx, c, mask, rule));
    }
    let mut measurements = Vec::new();
    let mut reference = None;
    for repetition in 0..5 {
        let modes = match repetition % 3 {
            0 => ["sequential", "index-build-each", "shared-index"],
            1 => ["shared-index", "sequential", "index-build-each"],
            _ => ["index-build-each", "shared-index", "sequential"],
        };
        for mode in modes {
            let mut replays = Vec::new();
            let mut builds = 0;
            let mut replay_ns = 0;
            let mut build_ns = 0;
            let mut used = 0;
            let mut fallback = 0;
            let mut blocks = 0;
            for (d, fx, c, mask, rule) in &corpus {
                let start = Instant::now();
                let shared = if mode == "shared-index" {
                    builds += 1;
                    Some(Index::build(d, &costs, fx, None))
                } else {
                    None
                };
                build_ns += start.elapsed().as_nanos();
                for side in [Side::Long, Side::Short] {
                    for stop_ticks in [500, 5000, 15000] {
                        let strategy = Strategy {
                            timeframe: Timeframe::M1,
                            side,
                            stop_ticks,
                            rule: rule.clone(),
                            entry: None,
                            stop_policy: None,
                        };
                        let start = Instant::now();
                        let replay = if mode == "sequential" {
                            tick_engine::replay(d, c, mask, &strategy, &costs, fx).unwrap()
                        } else if let Some(Ok(index)) = &shared {
                            let result = index.replay(c, mask, &strategy).unwrap();
                            used += usize::from(result.used_index);
                            fallback += usize::from(!result.used_index);
                            blocks += result.work.summary_blocks + result.work.barrier_blocks;
                            result.replay
                        } else if shared.is_some() {
                            fallback += 1;
                            tick_engine::replay(d, c, mask, &strategy, &costs, fx).unwrap()
                        } else {
                            let result =
                                indexed_replay::replay(d, c, mask, &strategy, &costs, fx, None)
                                    .unwrap();
                            used += usize::from(result.used_index);
                            fallback += usize::from(!result.used_index);
                            blocks += result.work.summary_blocks + result.work.barrier_blocks;
                            result.replay
                        };
                        replay_ns += start.elapsed().as_nanos();
                        assert!(!replay.trades.is_empty());
                        replays.push(replay);
                    }
                }
            }
            let fingerprint = data::hash(&replays).unwrap();
            if let Some(prior) = &reference {
                assert_eq!(&fingerprint, prior)
            } else {
                reference = Some(fingerprint.clone())
            }
            let trades = replays.iter().map(|r| r.trades.len()).sum::<usize>();
            measurements.push(json!({"repetition":repetition,"mode":mode,"index_setup_ns":build_ns,"replay_phase_ns":replay_ns,"setup_plus_replay_ns":build_ns+replay_ns,"replays":48,"nonzero_closed_trades":trades,"index_build_attempts_outside_replay_clock":builds,"used_index":used,"sequential_fallbacks":fallback,"block_queries":blocks,"fingerprint":fingerprint}));
        }
    }
    println!(
        "{}",
        json!({"schema":1,"class":"FULL_ACCOUNTING_ENGINE_COMPARISON_NOT_FACTORY_PIPELINE","engine":rust_strategy_factory::ENGINE_VERSION,"compute_revision":research::compute_revision().unwrap(),
        "workload":"SYNTHETIC_TEST_ONLY 8 profiles x50000 quotes with ties/gaps x2 sides x3 distances, five rotated repetitions; exact replay receipts include all financial/excursion/gap/window fields",
        "timing":"Caller candle/signal preparation and final full-result hashes excluded equally; engine validation/accounting included. Build-each includes index attempt in replay clock; shared-index build attempts separately reported and included in setup+replay. Whole-child resources include all preparation/checks.",
        "limitations":"No registry/research-screen/persistence/cache throughput measurement, no default path promotion, no real/full-history/VPS acceptance. Non-USD/fractional/pending cases keep sequential fallback. Repeated identities are not independent scientific evidence.","measurements":measurements})
    );
}
