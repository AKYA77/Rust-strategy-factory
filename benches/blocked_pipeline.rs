//! Synthetic complete streaming/checkpoint/new+cached pipeline, isolated fresh-process resource receipts.
use rust_strategy_factory::{
    blocked_native,
    candles::Timeframe,
    contracts::{Profile, Side},
    corpus,
    data::{self, Dataset, Tick},
    native, research, resource,
    storage::Registry,
    strategies::{Rule, Strategy},
    tick_engine::{self, CorpusBudget, CorpusProgress, CorpusRun, Costs},
};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command, time::Instant};
const SYMBOLS: [&str; 5] = ["EURUSD", "GBPUSD", "AUDUSD", "NZDUSD", "XAUUSD"];
const FORMATS: [&str; 3] = ["native", "block256", "block8192"];
fn fixture(symbol: &str) -> Dataset {
    let p = Profile::synthetic(symbol).unwrap();
    let base = if symbol == "XAUUSD" {
        2500 * p.price_scale
    } else {
        p.price_scale
    };
    let ticks = (0..20000u64)
        .map(|i| {
            let phase = i % 120;
            let offset = if phase < 60 { phase } else { 120 - phase };
            let bid = base + offset as i64 * p.pip * 5;
            Tick {
                timestamp_ms: 1735689600000 + i as i64 * 10000,
                sequence: u64::MAX - 19999 + i,
                bid,
                ask: bid + p.pip,
            }
        })
        .collect();
    data::dataset(
        p,
        ticks,
        "SYNTHETIC_TEST_ONLY blocked streaming pipeline",
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
fn io_bytes() -> Option<u64> {
    fs::read_to_string("/proc/self/io")
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("read_bytes: ").and_then(|v| v.parse().ok()))
}
fn child(root: &Path, format: &str, repetition: &str, cache: &str) -> Value {
    assert!(FORMATS.contains(&format));
    let costs = Costs::default();
    let budget = CorpusBudget {
        max_candles: 100000,
        feature_cells: 1000000,
        max_receipts: 100000,
    };
    let candidates = strategies();
    let start = Instant::now();
    let before_io = io_bytes();
    let cpu_start = resource::observe_process(std::process::id()).unwrap();
    let mut registry =
        Registry::open(&root.join(format!("{format}-{repetition}-{cache}.sqlite"))).unwrap();
    let mut receipts = Vec::new();
    let mut steps = 0usize;
    let mut closed = 0usize;
    let mut sources = Vec::new();
    for symbol in SYMBOLS {
        let spec = corpus::load(&root.join(format!("{format}-{symbol}.json"))).unwrap();
        for strategy in &candidates {
            loop {
                steps += 1;
                let progress = tick_engine::execute_corpus_step(
                    &mut registry,
                    CorpusRun {
                        base: root,
                        spec: &spec,
                        strategy,
                        costs: &costs,
                        budget,
                    },
                    1,
                    300000,
                    || chrono::Utc::now().timestamp_millis(),
                )
                .unwrap();
                match progress {
                    CorpusProgress::Pending { .. } => {}
                    CorpusProgress::Completed { cached, result, .. } => {
                        assert!(!cached);
                        let trades = result.result["tick_replay"]["trades"]
                            .as_array()
                            .unwrap()
                            .len();
                        assert!(trades > 0, "nonzero trades required for {symbol}");
                        closed += trades;
                        receipts.push(data::hash(&result.result["tick_replay"]).unwrap());
                        break;
                    }
                    _ => panic!("unexpected busy/held corpus benchmark"),
                }
            }
        }
        sources.push(spec);
    }
    registry.integrity().unwrap();
    let elapsed = start.elapsed().as_nanos();
    let after_io = io_bytes();
    let counts = registry.counts().unwrap();
    assert_eq!(counts.completed, 15);
    let cached = Instant::now();
    let before = counts.attempts;
    for spec in &sources {
        for strategy in &candidates {
            assert!(matches!(
                tick_engine::execute_corpus_step(
                    &mut registry,
                    CorpusRun {
                        base: root,
                        spec,
                        strategy,
                        costs: &costs,
                        budget
                    },
                    1,
                    300000,
                    || chrono::Utc::now().timestamp_millis()
                )
                .unwrap(),
                CorpusProgress::Completed { cached: true, .. }
            ));
        }
    }
    let cache_ns = cached.elapsed().as_nanos();
    assert_eq!(registry.counts().unwrap().attempts, before);
    let mut diagnostics = Vec::new();
    let mut peak_rows = 0;
    for spec in &sources {
        let start = Instant::now();
        let inventory = corpus::inspect(root, spec).unwrap();
        let inspect_ns = start.elapsed().as_nanos();
        peak_rows = peak_rows.max(inventory.max_resident_quote_events);
        let start = Instant::now();
        for strategy in &candidates {
            corpus::signals(root, spec, strategy, 100000, 1000000).unwrap();
        }
        diagnostics.push(json!({"warmed_inspect_ns":inspect_ns.to_string(),"warmed_three_signal_compilations_ns":start.elapsed().as_nanos().to_string()}));
    }
    let cpu_end = resource::observe_process(std::process::id()).unwrap();
    json!({"format":format,"repetition":repetition,"cache_condition":cache,"new_pipeline_ns":elapsed.to_string(),"cached_route_ns":cache_ns.to_string(),"new_completed_experiments":"15","cached_receipt_reads":"15","lease_acquisitions":counts.attempts.to_string(),"checkpoint_steps":steps.to_string(),"closed_synthetic_trades":closed.to_string(),"full_tick_receipts_hash":data::hash(&receipts).unwrap(),"maximum_decoded_quote_block_events":peak_rows.to_string(),"fresh_child_peak_rss_bytes":resource::observe_peak_rss().unwrap().to_string(),"new_pipeline_storage_read_bytes":before_io.zip(after_io).map(|(a,b)|b.saturating_sub(a).to_string()),"child_cpu_ticks":(cpu_end.user_ticks+cpu_end.system_ticks-cpu_start.user_ticks-cpu_start.system_ticks).to_string(),"post_pipeline_warm_stage_diagnostics":diagnostics,"physical_cold_verified":false})
}
fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() == 6 && args[1] == "--run" {
        println!(
            "{}",
            child(Path::new(&args[2]), &args[3], &args[4], &args[5])
        );
        return;
    }
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).unwrap();
    let root = std::env::temp_dir().join(format!(
        "blocked-pipeline-{}-{}",
        std::process::id(),
        u64::from_le_bytes(nonce)
    ));
    fs::create_dir(&root).unwrap();
    let mut encodings = Vec::new();
    for format in FORMATS {
        let start = Instant::now();
        let mut bytes = 0u64;
        for symbol in SYMBOLS {
            let d = fixture(symbol);
            let mut paths = Vec::new();
            for (index, ticks) in d.ticks.chunks(8192).enumerate() {
                let chunk = data::dataset(
                    d.profile.clone(),
                    ticks.to_vec(),
                    &d.manifest.source,
                    d.manifest.archive_hash.clone(),
                    true,
                )
                .unwrap();
                let path = root.join(format!(
                    "{format}-{symbol}-{index}.{}",
                    if format == "native" { "rsf" } else { "rsfb" }
                ));
                if format == "native" {
                    native::write(&path, &chunk).unwrap();
                } else {
                    blocked_native::write(
                        &path,
                        &chunk,
                        if format == "block256" { 256 } else { 8192 },
                    )
                    .unwrap();
                }
                bytes += fs::metadata(&path).unwrap().len();
                paths.push(path);
            }
            let spec =
                corpus::describe(&paths, if format == "native" { 8192 } else { 2000000 }).unwrap();
            fs::write(
                root.join(format!("{format}-{symbol}.json")),
                serde_json::to_vec(&spec).unwrap(),
            )
            .unwrap();
        }
        encodings.push(json!({"format":format,"encode_and_verified_describe_ns":start.elapsed().as_nanos().to_string(),"total_artifact_bytes":bytes.to_string()}));
    }
    let mut measurements = Vec::new();
    let mut reference = None;
    for repetition in 0..3 {
        for offset in 0..FORMATS.len() {
            let format = FORMATS[(repetition + offset) % FORMATS.len()];
            let paths = SYMBOLS
                .iter()
                .flat_map(|s| {
                    corpus::load(&root.join(format!("{format}-{s}.json")))
                        .unwrap()
                        .chunks
                        .into_iter()
                        .map(|c| c.path)
                })
                .collect::<Vec<_>>();
            for cache in ["advisory_eviction_requested", "warm"] {
                let evicted = if cache == "warm" {
                    for p in &paths {
                        data::file_hash(Path::new(p)).unwrap();
                    }
                    false
                } else {
                    Command::new("python3").args(["-c","import os,sys\nfor p in sys.argv[1:]:\n f=os.open(p,os.O_RDONLY)\n try: os.posix_fadvise(f,0,0,os.POSIX_FADV_DONTNEED)\n finally: os.close(f)"]).args(&paths).status().unwrap().success()
                };
                let out = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--run",
                        root.to_str().unwrap(),
                        format,
                        &repetition.to_string(),
                        cache,
                    ])
                    .output()
                    .unwrap();
                assert!(
                    out.status.success(),
                    "{}",
                    String::from_utf8_lossy(&out.stderr)
                );
                let mut row: Value = serde_json::from_slice(&out.stdout).unwrap();
                let signature = (
                    row["full_tick_receipts_hash"].clone(),
                    row["closed_synthetic_trades"].clone(),
                );
                if let Some(old) = &reference {
                    assert_eq!(&signature, old);
                } else {
                    reference = Some(signature);
                }
                row["eviction_syscall_succeeded"] = evicted.into();
                measurements.push(row);
            }
        }
    }
    let cpu = fs::read_to_string("/proc/cpuinfo")
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("model name\t: "))
        .unwrap_or("unavailable")
        .to_owned();
    let kernel = Command::new("uname").arg("-srmo").output().unwrap();
    println!("{}",serde_json::to_string_pretty(&json!({"class":"SYNTHETIC_FRESH_PROCESS_STREAMING_CHECKPOINT_PIPELINE","compute_revision":research::compute_revision().unwrap(),"symbols":SYMBOLS,"events_per_instrument":20000,"source_artifact_rows":8192,"repetitions":3,"host_cpu":cpu,"kernel":String::from_utf8_lossy(&kernel.stdout).trim(),"available_parallelism":std::thread::available_parallelism().unwrap().get(),"encodings":encodings,"measurements":measurements,"limitations":"Synthetic literal-USD profiles only; no real, full-history, scientific or target-VPS acceptance. Fresh child peak RSS includes new sessions, cached routes and warmed diagnostics, excluding parent artifact generation/cache helpers. Native kernel CPU ticks are not milliseconds. New-pipeline timer includes source verification, compilation, prefix reconstruction, checkpoint/replay/persistence and registry integrity. Cached routes still revalidate source context. Stage diagnostics run after the pipeline and are warm. Advisory eviction does not prove physical cold storage. Full financial receipt hashes/nonzero trade totals must match across all layouts; physical corpus/experiment identities intentionally differ. No format default or universal speedup claim."})).unwrap());
    fs::remove_dir_all(root).unwrap();
}
