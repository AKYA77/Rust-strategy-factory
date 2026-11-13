//! Candidate estimates vs authoritative ticks, all synthetic. No storage/cache timing.
use rust_strategy_factory::{
    candidate_screen::{self, ShadowPolicy},
    candles::{self, Timeframe},
    contracts::{Conversion, Profile, SYMBOLS, Side},
    data::{self, Tick},
    research,
    strategies::{Features, Rule, Strategy},
    tick_engine::{self, Costs},
};
use serde_json::json;
use std::time::Instant;
fn main() {
    let mut trials = Vec::new();
    let mut reference = None;
    for repetition in 0..5 {
        let mut results = Vec::new();
        let mut estimate_ns = 0;
        let mut replay_ns = 0;
        for symbol in SYMBOLS {
            let p = Profile::synthetic(symbol).unwrap();
            let base = if symbol == "XAUUSD" {
                2500 * p.price_scale
            } else if symbol == "USDJPY" {
                150 * p.price_scale
            } else {
                p.price_scale
            };
            let ticks = (0..12000)
                .map(|i| {
                    let phase = i % 120;
                    let wave = if phase < 60 { phase } else { 120 - phase };
                    let bid = base + wave * p.pip * 5;
                    Tick {
                        timestamp_ms: 1735689600000 + i * 10000,
                        sequence: i as u64,
                        bid,
                        ask: bid + p.pip,
                    }
                })
                .collect();
            let d = data::dataset(
                p,
                ticks,
                "SYNTHETIC_TEST_ONLY candidate model benchmark",
                "generated".into(),
                true,
            )
            .unwrap();
            let conversions = d
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
            let bars = candles::build(&d, Timeframe::M1).unwrap();
            let mut features = Features::with_context(&bars, 1_000_000, &d, Timeframe::M1).unwrap();
            let rule = Rule::AboveMean { window: 5 };
            let mask = features.signal(&rule).unwrap();
            for side in [Side::Long, Side::Short] {
                for stop_ticks in [100, 300, 800] {
                    let strategy = Strategy {
                        timeframe: Timeframe::M1,
                        side,
                        stop_ticks,
                        rule: rule.clone(),
                        entry: None,
                        stop_policy: None,
                    };
                    let costs = Costs::default();
                    let start = Instant::now();
                    let estimate =
                        candidate_screen::estimate(&d, &bars, &mask, &strategy, &costs, None)
                            .unwrap();
                    estimate_ns += start.elapsed().as_nanos();
                    let start = Instant::now();
                    let replay =
                        tick_engine::replay(&d, &bars, &mask, &strategy, &costs, &conversions)
                            .unwrap();
                    replay_ns += start.elapsed().as_nanos();
                    assert!(!replay.trades.is_empty());
                    let identity = data::hash(&(
                        d.manifest.content_hash.clone(),
                        strategy.identity().unwrap(),
                    ))
                    .unwrap();
                    let policy = ShadowPolicy {
                        promote_uncertain: false,
                        ..Default::default()
                    };
                    results.push(json!({"symbol":symbol,"side":side,"stop_ticks":stop_ticks,"dataset_hash":d.manifest.content_hash,
                        "replay_hash":data::hash(&replay).unwrap(),"audit":candidate_screen::compare(&estimate,&replay,&identity,&policy).unwrap()}));
                }
            }
        }
        let fingerprint = data::hash(&results).unwrap();
        if let Some(expected) = &reference {
            assert_eq!(&fingerprint, expected)
        } else {
            reference = Some(fingerprint.clone())
        }
        trials.push(json!({"repetition":repetition,"estimate_ns":estimate_ns,"authoritative_replay_ns":replay_ns,"fingerprint":fingerprint,"candidates":results}));
    }
    println!(
        "{}",
        json!({"schema":1,"engine_version":rust_strategy_factory::ENGINE_VERSION,"compute_revision":research::compute_revision().unwrap(),
        "workload":"SYNTHETIC_TEST_ONLY 8 instruments x 2 sides x 3 stop distances x 12000 quotes, five identical repetitions",
        "timing":"Estimate includes validation/M1 rebuild/risk model; tick includes validation/features/accounting. Initial corpus/primary-feature creation and audit hashing excluded from phase clocks. Process resources cover everything.",
        "policy":"Experimental shadow policy permits uncertainty; every candidate actually tick replayed. Sample flags diagnostic; false negatives are resolved candidate disagreement with numeric gate only, not qualification.",
        "limitations":"No real samples, physical cold storage, default speedup or scientific acceptance. Unsupported FX candidates still tick replayed. Partial/unresolved estimates never shadow reject.","trials":trials})
    );
}
