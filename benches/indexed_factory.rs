//! Entire shared-batch factory flow, including lazy index and reference audits.
use rust_strategy_factory::{
    candles::Timeframe,
    contracts::{Conversion, Profile, SYMBOLS, Side},
    data::{self, Tick},
    indexed_policy::Policy,
    research::{self, BatchOptions, EvaluationOptions, Input},
    storage::{Claim, Registry},
    strategies::{Rule, Strategy},
    tick_engine::Costs,
};
use serde_json::json;
use std::{fs, time::Instant};
fn main() {
    let directory =
        std::env::temp_dir().join(format!("factory-indexed-pipeline-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let corpus = SYMBOLS
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
            let dataset = data::dataset(
                p,
                ticks,
                "SYNTHETIC_TEST_ONLY indexed factory pipeline",
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
    let mut candidates = Vec::new();
    for side in [Side::Long, Side::Short] {
        for stop_ticks in [500, 5000, 15000] {
            candidates.push(Strategy {
                timeframe: Timeframe::M1,
                side,
                stop_ticks,
                rule: Rule::AboveMean { window: 5 },
                entry: None,
                stop_policy: None,
            });
        }
    }
    let costs = Costs {
        commission_micro_usd_per_lot_per_fill: 1_000_000,
        ..Default::default()
    };
    let policy = Policy::default();
    let revision = research::compute_revision().unwrap();
    let mut measurements = Vec::new();
    let mut fingerprint = None;
    for repetition in 0..5 {
        let modes = if repetition % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        };
        for indexed in modes {
            let mut registry =
                Registry::open(&directory.join(format!("{repetition}-{indexed}.sqlite"))).unwrap();
            let start = Instant::now();
            let mut executions = Vec::new();
            let mut initialized = 0;
            for input in &corpus {
                let report = research::evaluate_batch_report(
                    &mut registry,
                    input,
                    &candidates,
                    &costs,
                    &revision,
                    BatchOptions {
                        evaluation: EvaluationOptions {
                            feature_cells: 1_000_000,
                            evaluation: None,
                            shadow_policy: None,
                            proof_policy: None,
                            indexed_policy: indexed.then_some(&policy),
                        },
                        state_budget_bytes: 256 * 1024 * 1024,
                    },
                )
                .unwrap();
                initialized += usize::from(report.shared_index_initialized);
                executions.extend(report.executions);
            }
            let elapsed = start.elapsed().as_nanos();
            let mut trades = 0;
            let mut used = 0;
            let mut audited = 0;
            let mut fallback = 0;
            let mut comparable = Vec::new();
            for e in &executions {
                assert!(!e.reused);
                assert_eq!(e.outcome, "completed");
                let n = e.result["tick_replay"]["trades"].as_array().unwrap().len();
                assert!(n > 0);
                trades += n;
                used += usize::from(e.result["indexed_replay"]["used_index"] == true);
                audited += usize::from(e.result["indexed_replay"]["reference_audited"] == true);
                fallback +=
                    usize::from(indexed && e.result["indexed_replay"]["used_index"] == false);
                comparable.push(json!({"replay":e.result["tick_replay"],"screen":e.result["screen"],"bounds_audit":e.result["screen_audit"],"candle_audit":e.result["candidate_screen_audit"]}));
            }
            let result_hash = data::hash(&comparable).unwrap();
            if let Some(expected) = &fingerprint {
                assert_eq!(&result_hash, expected)
            } else {
                fingerprint = Some(result_hash.clone())
            }
            assert_eq!(registry.counts().unwrap().attempts, 48);
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
            let cache_ns = start.elapsed().as_nanos();
            assert_eq!(registry.counts().unwrap().attempts, 48);
            measurements.push(json!({"repetition":repetition,"mode":if indexed{"indexed-audited-batch"}else{"sequential-batch"},"new_evaluation_ns":elapsed,"cached_registry_reads_ns":cache_ns,"new_attempts":48,"cached_reads":48,"closed_trades":trades,"used_index":used,"sequential_fallbacks":fallback,"independent_reference_audits":audited,"lazy_index_inputs_initialized":initialized,"comparable_fingerprint":result_hash}));
        }
    }
    fs::remove_dir_all(&directory).unwrap();
    println!(
        "{}",
        json!({"schema":1,"class":"CURRENT_FACTORY_PIPELINE_WITH_SEPARATE_CACHE_READS","compute_revision":revision,"engine":rust_strategy_factory::ENGINE_VERSION,
        "workload":"SYNTHETIC_TEST_ONLY 8 profiles x50000 quotes x6 same-frame candidates;5 rotated shared-batch sequential/indexed pairs,48 new attempts/fresh registry/mode",
        "policy":policy,"timing":"New clock includes immutable input preparation, features, lazy index setup, M1 diagnostics, full replay, deterministic sequential reference audits where selected, persistence and batch report. Corpus/registry setup and final result/hash checks outside new clock; all in whole-child resources. Raw registry cache reads separately timed; not cached batch-API latency.",
        "comparison":"Exact whole replay, screen, conditional and M1 audits match; requested policy/method/audit metadata and experiment identities deliberately differ and are not falsely required equal.",
        "limitations":"Repeated48 case identities are not independent scientific trials. No real/physical-cold/full-history/VPS acceptance; manual experimental training batches only, no autonomous/default promotion.","measurements":measurements})
    );
}
