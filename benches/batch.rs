//! Scalar/shared compilation comparison with exact new/cache/result accounting.
use rust_strategy_factory::{
    candles::{self, Timeframe},
    contracts::{Conversion, Profile, SYMBOLS, Side},
    data::{self, Tick},
    research::{self, BatchOptions, EvaluationOptions, Input},
    storage::{Claim, Registry},
    strategies::{Features, Rule, Strategy},
    tick_engine::Costs,
};
use serde_json::json;
use std::{fs, time::Instant};
fn candidates() -> Vec<Strategy> {
    let mut out = Vec::new();
    for window in [5, 10, 20] {
        for stop_ticks in [50, 100] {
            for side in [Side::Long, Side::Short] {
                out.push(Strategy {
                    timeframe: Timeframe::M1,
                    side,
                    stop_ticks,
                    entry: None,
                    stop_policy: None,
                    rule: Rule::All(vec![
                        Rule::AboveMean { window },
                        Rule::RsiThreshold {
                            window,
                            basis_points: 6000,
                            above: true,
                        },
                    ]),
                });
            }
        }
    }
    out
}
fn options() -> EvaluationOptions<'static> {
    EvaluationOptions {
        feature_cells: 1_000_000,
        evaluation: None,
        shadow_policy: None,
        proof_policy: None,
    }
}
fn main() {
    let directory =
        std::env::temp_dir().join(format!("factory-batch-benchmark-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let inputs = SYMBOLS
        .iter()
        .map(|symbol| {
            let p = Profile::synthetic(symbol).unwrap();
            let base = if *symbol == "XAUUSD" {
                250000
            } else if *symbol == "USDJPY" {
                150000
            } else {
                100000
            };
            let ticks = (0..12000)
                .map(|i| {
                    let n = i % 120;
                    let wave = if n < 60 { n } else { 120 - n };
                    let bid = base + wave * p.pip * 5;
                    Tick {
                        timestamp_ms: 1735689600000 + i * 10000,
                        sequence: i as u64,
                        bid,
                        ask: bid + p.pip,
                    }
                })
                .collect();
            let dataset = data::dataset(
                p,
                ticks,
                "SYNTHETIC_TEST_ONLY shared-batch benchmark",
                "generated".into(),
                true,
            )
            .unwrap();
            let conversions = dataset
                .ticks
                .iter()
                .map(|t| {
                    if dataset.profile.quote == "USD" {
                        Conversion::usd()
                    } else {
                        Conversion {
                            usd_num: 1,
                            quote_den: if *symbol == "USDJPY" { 150 } else { 1 },
                            as_of_ms: t.timestamp_ms,
                        }
                    }
                })
                .collect();
            Input {
                dataset,
                conversions,
            }
        })
        .collect::<Vec<_>>();
    let candidates = candidates();
    let revision = research::compute_revision().unwrap();
    let costs = Costs::default();
    let mut component = Vec::new();
    for input in &inputs {
        let bars = candles::build(&input.dataset, Timeframe::M1).unwrap();
        let start = Instant::now();
        let mut scalar_builds = 0;
        let mut masks = Vec::new();
        for strategy in &candidates {
            let mut f =
                Features::with_context(&bars, 1_000_000, &input.dataset, Timeframe::M1).unwrap();
            masks.push(data::hash(&f.signal(&strategy.rule).unwrap()).unwrap());
            scalar_builds += f.feature_builds;
        }
        let scalar_ns = start.elapsed().as_nanos();
        let start = Instant::now();
        let mut shared =
            Features::with_context(&bars, 1_000_000, &input.dataset, Timeframe::M1).unwrap();
        for (i, s) in candidates.iter().enumerate() {
            assert_eq!(
                data::hash(&shared.signal(&s.rule).unwrap()).unwrap(),
                masks[i]
            );
        }
        component.push(json!({"symbol":input.dataset.profile.symbol,"scalar_ns":scalar_ns,"shared_ns":start.elapsed().as_nanos(),"scalar_feature_builds":scalar_builds,"shared_feature_builds":shared.feature_builds,"shared_cache_hits":shared.cache_hits,"mask_fingerprint":data::hash(&masks).unwrap()}));
    }
    let mut measurements = Vec::new();
    let mut fingerprint = None;
    for repetition in 0..5 {
        let order = if repetition % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        };
        for shared in order {
            let mut registry =
                Registry::open(&directory.join(format!("{repetition}-{shared}.sqlite"))).unwrap();
            let start = Instant::now();
            let mut executions = Vec::new();
            let mut shared_builds = 0;
            let mut shared_hits = 0;
            for input in &inputs {
                if shared {
                    let report = research::evaluate_batch_report(
                        &mut registry,
                        input,
                        &candidates,
                        &costs,
                        &revision,
                        BatchOptions {
                            evaluation: options(),
                            state_budget_bytes: 256 * 1024 * 1024,
                        },
                    )
                    .unwrap();
                    shared_builds += report.shared_feature_builds;
                    shared_hits += report.shared_cache_hits;
                    executions.extend(report.executions);
                } else {
                    for strategy in &candidates {
                        executions.push(
                            research::evaluate_configured(
                                &mut registry,
                                input,
                                strategy,
                                &costs,
                                &revision,
                                options(),
                            )
                            .unwrap(),
                        );
                    }
                }
            }
            let new_ns = start.elapsed().as_nanos();
            let mut trades = 0;
            for e in &executions {
                assert!(!e.reused);
                assert_eq!(e.outcome, "completed");
                let n = e.result["tick_replay"]["trades"].as_array().unwrap().len();
                assert!(n > 0);
                trades += n;
            }
            let identity = data::hash(&executions).unwrap();
            if let Some(reference) = &fingerprint {
                assert_eq!(&identity, reference)
            } else {
                fingerprint = Some(identity.clone())
            }
            assert_eq!(registry.counts().unwrap().attempts, 96);
            let start = Instant::now();
            for e in &executions {
                assert!(matches!(
                    registry
                        .claim(
                            &e.experiment_id,
                            chrono::Utc::now().timestamp_millis(),
                            300000
                        )
                        .unwrap(),
                    Claim::Reused(_)
                ));
            }
            let cached_ns = start.elapsed().as_nanos();
            assert_eq!(registry.counts().unwrap().attempts, 96);
            measurements.push(json!({"repetition":repetition,"mode":if shared{"shared-batch"}else{"scalar"},"new_evaluation_ns":new_ns,"cached_registry_reads_ns":cached_ns,"new_attempts":96,"cached_reads":96,"actual_closed_trades":trades,"fingerprint":identity,"shared_feature_builds":if shared{Some(shared_builds)}else{None},"shared_cache_hits":if shared{Some(shared_hits)}else{None}}));
        }
    }
    fs::remove_dir_all(&directory).unwrap();
    println!(
        "{}",
        json!({"schema":1,"compute_revision":revision,"engine":rust_strategy_factory::ENGINE_VERSION,
        "workload":"SYNTHETIC_TEST_ONLY 8 instruments x12 unique same-frame candidates x12000 quotes; same feature budget/default costs; five rotated new registry batches",
        "timing":"New clocks include complete scalar/shared evaluation including features/current M1 diagnostics/authoritative replay/persistence. Corpus/registry creation and final result equality checks outside new clocks but inside whole-child resources. Cache reads separately timed. Component includes mask hashing/checks and is not full research throughput.",
        "limitations":"No full-history/real/VPS acceptance, parallelism or default scheduler promotion. Shared batches still revalidate authoritative signals/replay; M1 model reconstruction is not shared. Repeated identical candidate sets are not independent scientific trials.","component":component,"measurements":measurements})
    );
}
