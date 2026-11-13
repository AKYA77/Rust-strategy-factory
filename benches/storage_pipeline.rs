//! Comparable synthetic storage -> signal-stage -> exact replay -> durable/cache workflow.
use rust_strategy_factory::{
    candles::Timeframe,
    compressed,
    contracts::{Conversion, Profile, SYMBOLS, Side},
    data::{self, Dataset, Tick},
    native, parquet_store,
    research::{self, Input},
    storage::Registry,
    strategies::{Rule, Strategy},
    tick_engine::Costs,
};
use serde_json::json;
use std::{fs, path::Path, process::Command, time::Instant};
fn fixture(symbol: &str) -> Dataset {
    let p = Profile::synthetic(symbol).unwrap();
    let base = if symbol == "XAUUSD" {
        2500 * p.price_scale
    } else if symbol == "USDJPY" {
        150 * p.price_scale
    } else {
        p.price_scale
    };
    let ticks = (0..20000)
        .map(|i| {
            let phase = i % 120;
            let offset = if phase < 60 { phase } else { 120 - phase };
            let bid = base + offset * p.pip * 5;
            Tick {
                timestamp_ms: 1735689600000 + i * 10000,
                sequence: i as u64,
                bid,
                ask: bid + p.pip,
            }
        })
        .collect();
    data::dataset(
        p,
        ticks,
        "SYNTHETIC_TEST_ONLY same-corpus storage pipeline",
        "generated-no-archive".into(),
        true,
    )
    .unwrap()
}
fn strategies() -> Vec<Strategy> {
    vec![
        Rule::AboveMean { window: 5 },
        Rule::All(vec![
            Rule::AboveMean { window: 5 },
            Rule::Rising { lookback: 2 },
        ]),
        Rule::All(vec![
            Rule::AboveMean { window: 5 },
            Rule::HigherTimeframe {
                timeframe: Timeframe::M5,
                rule: Box::new(Rule::Rising { lookback: 1 }),
            },
        ]),
    ]
    .into_iter()
    .map(|rule| Strategy {
        stop_policy: None,
        timeframe: Timeframe::M1,
        side: Side::Long,
        entry: None,
        stop_ticks: 100,
        rule,
    })
    .collect()
}
fn load(path: &Path, format: &str) -> Dataset {
    match format {
        "native" => native::read(path),
        "rsfz" => compressed::read(path),
        _ => parquet_store::read(path),
    }
    .unwrap()
}
fn read_bytes() -> u64 {
    fs::read_to_string("/proc/self/io")
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("read_bytes: ").and_then(|v| v.parse().ok()))
        .unwrap_or(0)
}
fn eviction(paths: &[std::path::PathBuf]) -> bool {
    Command::new("python3").args(["-c","import os,sys\nfor p in sys.argv[1:]:\n f=os.open(p,os.O_RDONLY)\n try: os.posix_fadvise(f,0,0,os.POSIX_FADV_DONTNEED)\n finally: os.close(f)"]).args(paths).status().is_ok_and(|s|s.success())
}
fn main() {
    let directory =
        std::env::temp_dir().join(format!("factory-storage-pipeline-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let datasets = SYMBOLS.iter().map(|s| fixture(s)).collect::<Vec<_>>();
    let expected = datasets
        .iter()
        .map(|d| data::hash(d).unwrap())
        .collect::<Vec<_>>();
    let revision = research::compute_revision().unwrap();
    let candidates = strategies();
    let mut encodings = Vec::new();
    let formats = ["native", "rsfz", "parquet"];
    for format in formats {
        let start = Instant::now();
        let mut bytes = 0;
        for data in &datasets {
            let path = directory.join(format!("{}.{format}", data.profile.symbol));
            match format {
                "native" => native::write(&path, data).unwrap(),
                "rsfz" => {
                    let intermediate =
                        directory.join(format!("{}.intermediate.rsf", data.profile.symbol));
                    native::write(&intermediate, data).unwrap();
                    compressed::compress(&intermediate, &path).unwrap();
                    fs::remove_file(intermediate).unwrap();
                }
                _ => parquet_store::write(&path, data).unwrap(),
            };
            bytes += fs::metadata(&path).unwrap().len();
        }
        encodings.push(
            json!({"format":format,"elapsed_ns":start.elapsed().as_nanos(),"total_bytes":bytes}),
        );
    }
    let mut reference = None;
    let mut measurements = Vec::new();
    for repetition in 0..5 {
        for offset in 0..3 {
            let format = formats[(repetition + offset) % 3];
            let paths = datasets
                .iter()
                .map(|d| directory.join(format!("{}.{format}", d.profile.symbol)))
                .collect::<Vec<_>>();
            for cache in ["advisory_eviction_requested", "warm"] {
                let eviction_requested = cache == "advisory_eviction_requested" && eviction(&paths);
                let start = Instant::now();
                let before_io = read_bytes();
                let loaded = paths.iter().map(|p| load(p, format)).collect::<Vec<_>>();
                let io_bytes = read_bytes().saturating_sub(before_io);
                let decode_ns = start.elapsed().as_nanos();
                for (index, data) in loaded.iter().enumerate() {
                    assert_eq!(data::hash(data).unwrap(), expected[index]);
                }
                let db = directory.join(format!("{format}-{repetition}-{cache}.sqlite"));
                let mut registry = Registry::open(&db).unwrap();
                let start = Instant::now();
                let mut identities = Vec::new();
                let mut result_hashes = Vec::new();
                let mut closed = 0;
                for data in loaded {
                    let conversion = if data.profile.quote == "USD" {
                        vec![Conversion::usd(); data.ticks.len()]
                    } else {
                        data.ticks
                            .iter()
                            .map(|t| Conversion {
                                usd_num: 1,
                                quote_den: if data.profile.symbol == "USDJPY" {
                                    150
                                } else {
                                    1
                                },
                                as_of_ms: t.timestamp_ms,
                            })
                            .collect()
                    };
                    let input = Input {
                        dataset: data,
                        conversions: conversion,
                    };
                    for candidate in &candidates {
                        let result = research::evaluate(
                            &mut registry,
                            &input,
                            candidate,
                            &Costs::default(),
                            &revision,
                            1_000_000,
                        )
                        .unwrap();
                        assert!(!result.reused);
                        assert_eq!(result.outcome, "completed");
                        let trades = result.result["tick_replay"]["trades"]
                            .as_array()
                            .unwrap()
                            .len();
                        assert!(
                            trades > 0,
                            "{} requires nonzero benchmark trades",
                            input.dataset.profile.symbol
                        );
                        closed += trades;
                        identities.push(result.experiment_id);
                        result_hashes.push(data::hash(&result.result).unwrap());
                    }
                }
                let evaluate_persist_ns = start.elapsed().as_nanos();
                let result_fingerprint = data::hash(&result_hashes).unwrap();
                let identity_fingerprint = data::hash(&identities).unwrap();
                if let Some((ids, results, trades)) = &reference {
                    assert_eq!(
                        (&identity_fingerprint, &result_fingerprint, closed),
                        (ids, results, *trades)
                    );
                } else {
                    reference = Some((
                        identity_fingerprint.clone(),
                        result_fingerprint.clone(),
                        closed,
                    ));
                }
                let before = registry.counts().unwrap();
                assert_eq!(before.attempts, 24);
                assert_eq!(before.completed, 24);
                let start = Instant::now();
                for id in &identities {
                    let reused = registry
                        .claim(id, chrono::Utc::now().timestamp_millis(), 300000)
                        .unwrap();
                    assert!(matches!(
                        reused,
                        rust_strategy_factory::storage::Claim::Reused(_)
                    ));
                }
                let cache_lookup_ns = start.elapsed().as_nanos();
                assert_eq!(registry.counts().unwrap().attempts, before.attempts);
                registry.integrity().unwrap();
                drop(registry);
                fs::remove_file(db).unwrap();
                measurements.push(json!({"format":format,"repetition":repetition,"cache_condition":cache,"eviction_syscall_succeeded":eviction_requested,"physical_cold_verified":false,"decode_validate_ns":decode_ns,"process_storage_read_bytes_during_load":io_bytes,"signal_stage_sequential_replay_persist_ns":evaluate_persist_ns,"total_load_and_new_pipeline_ns":decode_ns+evaluate_persist_ns,"new_completed_experiments":24,"closed_synthetic_trades":closed,"cached_result_reads":24,"cache_lookup_ns":cache_lookup_ns,"experiment_ids_hash":identity_fingerprint,"full_results_hash":result_fingerprint}));
            }
        }
    }
    println!("{}",serde_json::to_string_pretty(&json!({"class":"SYNTHETIC_SAME_CORPUS_STORAGE_AND_CURRENT_RESEARCH_PIPELINE","events_per_instrument":20000,"instruments":SYMBOLS,"compute_revision":revision,"strategies":candidates,"encodings":encodings,"measurements":measurements,"limitations":"Synthetic arbitrary profiles/conversions, no real or full-history/scientific eligibility. Current signal-only M1 rejection stage is not complete candidate-level net screening. All comparisons assert complete result/identity equality and nonzero trades; new executions and cached reads counted separately. File-local posix_fadvise requests eviction without proving physically cold storage; read-byte observations are reported. Rust load/evaluate timers exclude the development Python hint helper; process resource receipts include setup/checks/helpers and all repetitions. Encoder RSFZ includes intermediate native materialization; native/Parquet start from the same owned Dataset. No automatic production codec selection or universal speedup claim."})).unwrap());
    fs::remove_dir_all(directory).unwrap();
}
