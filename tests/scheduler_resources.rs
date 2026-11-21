//! Scheduler/resource-control integration: measured admission refusal ordering,
//! adaptive worker selection in corpus-batch, and reproducible OS-enforcement evidence.
use rust_strategy_factory::{
    contracts::{Conversion, Profile},
    data::{self, Tick},
    research::{Config, Driver, Input},
    resource,
    storage::Registry,
};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "scheduler-resources-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&p).unwrap();
    p
}
fn fleet_fixture(tag: &str, cpu_guard: Option<serde_json::Value>) -> (PathBuf, PathBuf, String) {
    let root = dir(tag);
    let dataset = data::synthetic_fixture("EURUSD").unwrap();
    let native = root.join("source.rsf");
    rust_strategy_factory::native::write(&native, &dataset).unwrap();
    let spec = rust_strategy_factory::corpus::describe(&[native], 10000).unwrap();
    fs::write(root.join("corpus.json"), serde_json::to_vec(&spec).unwrap()).unwrap();
    fs::write(
        root.join("search.json"),
        serde_json::to_vec(&serde_json::json!({
            "specification":"corpus.json","seed":42,"maximum_candidates":2,"queue_width":2,
            "budget":{"max_candles":10000,"feature_cells":1000000,"max_receipts":10000},
            "limits":{"workers":2,"max_parallel_candles":20000,"max_parallel_feature_cells":2000000,"max_parallel_receipts":20000,"max_new_chunks":64,"lease_ms":60000}
        }))
        .unwrap(),
    )
    .unwrap();
    let mut fleet = serde_json::json!({
        "sources":[{"name":"one","config":"search.json"}],
        "coordinator_lease_ms":60000
    });
    if let Some(guard) = cpu_guard {
        fleet["cpu_guard"] = guard;
    }
    let file = root.join("fleet.json");
    fs::write(&file, serde_json::to_vec(&fleet).unwrap()).unwrap();
    let db = root.join("state.sqlite");
    let id = rust_strategy_factory::corpus_fleet::register_file(&file, &db).unwrap();
    (root, db, id)
}
#[test]
fn fleet_cpu_guard_refuses_before_any_lease_claim() {
    let (root, db, id) = fleet_fixture(
        "cpu-refusal",
        Some(serde_json::json!({
            "maximum_host_busy_share_numerator":0,
            "maximum_host_busy_share_denominator":1,
            "observation_window_ms":10
        })),
    );
    let step = rust_strategy_factory::corpus_fleet::step(&db, &id).unwrap();
    assert_eq!(step.kind, "cpu-pressure");
    assert!(step.visit.is_none());
    let cpu = step.cpu_admission.expect("cpu admission evidence");
    assert!(!cpu.allowed);
    let registry = Registry::open(&db).unwrap();
    // Refusal happened BEFORE any coordinator claim: no attempt, no visit, no lease.
    let status = rust_strategy_factory::corpus_fleet::status(&registry, &id).unwrap();
    assert_eq!(status["visits"], "0");
    assert_eq!(status["attempt"], "0");
    assert_eq!(status["lease_active"], false);
    assert_eq!(status["sources"][0]["last_kind"], "unvisited");
    registry.integrity().unwrap();
    drop(registry);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn fleet_without_cpu_guard_keeps_legacy_definition_bytes() {
    let (root, db, id) = fleet_fixture("legacy", None);
    let registry = Registry::open(&db).unwrap();
    let status = rust_strategy_factory::corpus_fleet::status(&registry, &id).unwrap();
    assert_eq!(status["state"], "running");
    // The frozen definition is re-serialized through status; a legacy fleet must
    // not gain a cpu_guard declaration (its identity bytes are asserted in the
    // crate-internal legacy-byte test).
    assert!(status.get("cpu_guard").is_none());
    drop(registry);
    fs::remove_dir_all(root).unwrap();
}
fn research_input() -> Input {
    let ticks = (0..1000)
        .map(|i| {
            let phase = i % 80;
            let wave = if phase < 40 { phase } else { 80 - phase };
            let bid = 100000 + wave * 50;
            Tick {
                timestamp_ms: 1735689600000 + i * 10000,
                sequence: i as u64,
                bid,
                ask: bid + 20,
            }
        })
        .collect::<Vec<_>>();
    let n = ticks.len();
    Input {
        dataset: data::dataset(
            Profile::synthetic("EURUSD").unwrap(),
            ticks,
            "SYNTHETIC_TEST_ONLY scheduler fixture",
            "generated".into(),
            true,
        )
        .unwrap(),
        conversions: vec![Conversion::usd(); n],
    }
}
#[test]
fn driver_resource_guard_pauses_before_registering_experiments() {
    let root = dir("driver-guard");
    let path = root.join("state.sqlite");
    let config = Config {
        seed: 10,
        resource_guard: Some(resource::Guard {
            maximum_process_rss_bytes: 1,
            minimum_host_available_memory_bytes: 0,
        }),
        ..Config::default()
    };
    // Legacy identities must not gain the new optional field.
    assert!(
        !serde_json::to_string(&Config::default())
            .unwrap()
            .contains("resource_guard")
    );
    let mut driver = Driver::open(
        &path,
        vec![research_input()],
        config,
        "fixed-test-revision".into(),
    )
    .unwrap();
    driver.acquire(0).unwrap();
    let progressed = driver.step(1).unwrap();
    assert!(!progressed);
    let status = rust_strategy_factory::research::status(&driver.registry, &driver.id, 2).unwrap();
    assert_eq!(status.state, "paused");
    assert!(
        status
            .last_error
            .unwrap_or_default()
            .contains("resource-pressure")
    );
    assert_eq!(status.cursor, 0);
    let counts = driver.registry.counts().unwrap();
    assert_eq!(counts.registered, 0);
    drop(driver);
    fs::remove_dir_all(root).unwrap();
}
fn batch_corpus(root: &std::path::Path) {
    let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
    let invoke = |args: &[&str]| -> serde_json::Value {
        let out = Command::new(binary).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
            panic!(
                "{args:?} produced invalid JSON: {e}; stdout={:?}; stderr={:?}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        })
    };
    let profile = root.join("profile.json");
    fs::write(
        &profile,
        serde_json::to_vec(&Profile::synthetic("EURUSD").unwrap()).unwrap(),
    )
    .unwrap();
    let mut native = Vec::new();
    for chunk in 0..4 {
        let csv = root.join(format!("{chunk}.csv"));
        let dataset = root.join(format!("{chunk}.json"));
        let artifact = root.join(format!("{chunk}.rsf"));
        let mut rows = String::from("timestamp_ms,sequence,bid,ask\n");
        for i in 0..1000 {
            let event = chunk * 1000 + i;
            let bid = 100000
                + if (event / 240) % 2 == 0 {
                    event % 240
                } else {
                    240 - event % 240
                };
            rows.push_str(&format!(
                "{},{},{:.5},{:.5}\n",
                1735689600000i64 + event as i64 * 60000,
                18446744073709547000u64 + event as u64,
                bid as f64 / 100000.0,
                (bid + 10) as f64 / 100000.0
            ));
        }
        fs::write(&csv, rows).unwrap();
        invoke(&[
            "import",
            csv.to_str().unwrap(),
            profile.to_str().unwrap(),
            "SYNTHETIC_TEST_ONLY scheduler batch fixture",
            dataset.to_str().unwrap(),
            "synthetic",
        ]);
        invoke(&[
            "pack-native",
            dataset.to_str().unwrap(),
            artifact.to_str().unwrap(),
        ]);
        native.push(artifact);
    }
    let spec = root.join("corpus.json");
    let mut args: Vec<&str> = vec!["corpus-create", spec.to_str().unwrap()];
    let owned: Vec<String> = native
        .iter()
        .map(|p| p.to_str().unwrap().to_string())
        .collect();
    for p in &owned {
        args.push(p);
    }
    // corpus-create publishes the specification file and prints nothing on
    // success; only its exit status is contractual. The spec's content is
    // exercised end-to-end by the corpus-batch runs below.
    let out = Command::new(binary).args(&args).output().unwrap();
    assert!(
        out.status.success(),
        "corpus-create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
fn batch_config(
    root: &std::path::Path,
    workers: usize,
    policy: Option<serde_json::Value>,
) -> PathBuf {
    let strategies: Vec<serde_json::Value> = (80..160)
        .step_by(10)
        .map(|n| {
            serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":n,"rule":{"AboveMean":{"window":2}}})
        })
        .collect();
    let mut config = serde_json::json!({
        "specification":"corpus.json",
        "strategies":strategies,
        "budget":{"max_candles":10000,"feature_cells":100000,"max_receipts":10000},
        "limits":{"workers":workers,"max_parallel_candles":workers*10000,"max_parallel_feature_cells":workers*100000,"max_parallel_receipts":workers*10000,"max_new_chunks":1,"lease_ms":600000}
    });
    let has_policy = policy.is_some();
    if let Some(policy) = policy {
        config["worker_policy"] = policy;
    }
    let path = root.join(format!("config-{workers}-{has_policy}.json"));
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    path
}
fn completed_hashes(report: &serde_json::Value) -> Vec<String> {
    let mut hashes: Vec<String> = report["last"]["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|j| {
            j["Ok"]["Completed"]["result"]["result_hash"]
                .as_str()
                .map(str::to_owned)
        })
        .collect();
    hashes.sort();
    hashes
}
#[test]
fn adaptive_batch_measures_cpu_and_adapts_workers_without_changing_receipts() {
    let root = dir("adaptive-batch");
    batch_corpus(&root);
    let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
    // Control: fixed single worker.
    let control_config = batch_config(&root, 1, None);
    let control_db = root.join("control.sqlite");
    let out = Command::new(binary)
        .args([
            "corpus-batch",
            control_config.to_str().unwrap(),
            control_db.to_str().unwrap(),
            "10",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let control: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(control["stop_reason"], "complete");
    assert!(
        control.get("worker_selection").is_none(),
        "legacy output gained keys"
    );
    let expected = completed_hashes(&control);
    assert_eq!(expected.len(), 8);
    // Adaptive: declared 4 workers, forced sustained downscale (any busy tick is
    // at or above 1/u64::MAX; upscale below 0 is impossible), hysteresis 1
    // => 4,3,2,1,1,... while receipts stay identical.
    let policy = serde_json::json!({
        "minimum_workers":1,"maximum_workers":4,
        "scale_up_busy_below_numerator":0,"scale_up_busy_below_denominator":1,
        "scale_down_busy_at_or_above_numerator":1,"scale_down_busy_at_or_above_denominator":18446744073709551615u64,
        "hysteresis_windows":1
    });
    let adaptive_config = batch_config(&root, 4, Some(policy));
    let adaptive_db = root.join("adaptive.sqlite");
    let out = Command::new(binary)
        .args([
            "corpus-batch",
            adaptive_config.to_str().unwrap(),
            adaptive_db.to_str().unwrap(),
            "10",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let adaptive: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(adaptive["stop_reason"], "complete");
    assert_eq!(completed_hashes(&adaptive), expected);
    let selection = &adaptive["worker_selection"];
    let rounds = selection["rounds"].as_array().unwrap();
    assert!(rounds.len() >= 4);
    let used: Vec<u64> = rounds
        .iter()
        .map(|r| r["workers_used"].as_str().unwrap().parse().unwrap())
        .collect();
    assert_eq!(used[0], 4);
    assert!(used.contains(&1), "never downscaled: {used:?}");
    assert!(
        used.windows(2).all(|w| w[1] <= w[0]),
        "non-monotone under forced pressure: {used:?}"
    );
    // Every round carries measured host evidence or an explicit unknown.
    for round in rounds {
        let evidence = &round["allocation"];
        assert!(evidence["host_total_delta"].is_string() || evidence["host_total_delta"].is_null());
        assert!(evidence["scope"].as_str().unwrap().contains("not"));
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn resource_enforcement_cli_reports_valid_evidence_for_deploy_templates() {
    let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for unit in [
        "deploy/rust-strategy-factory-research.service",
        "deploy/rust-strategy-factory-dashboard.service",
    ] {
        let out = Command::new(binary)
            .args(["resource-enforcement", repo.join(unit).to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let knobs = report["knobs"].as_array().unwrap();
        assert_eq!(knobs.len(), 4);
        for knob in knobs {
            let verdict = knob["verdict"].as_str().unwrap();
            assert!(matches!(
                verdict,
                "enforced" | "weaker-than-declared" | "declared-only" | "not-declared" | "unknown"
            ));
            // An enforcement claim must carry a finite observed kernel value.
            if verdict == "enforced" {
                assert!(knob["observed"].is_string());
            }
        }
        assert!(report["scope"].as_str().unwrap().contains("cgroup"));
    }
}
#[test]
fn resource_allocation_cli_measures_a_bounded_live_window() {
    let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
    let out = Command::new(binary)
        .args(["resource-allocation", "rust-strategy-f", "100"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let total = report["host_total_delta"]
        .as_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert!(total > 0);
    assert!(report["scope"].as_str().unwrap().contains("ticks"));
    // Invalid bounds are refused, never defaulted.
    for bad in [
        vec!["resource-allocation", "", "100"],
        vec!["resource-allocation", "corpus-", "0"],
        vec!["resource-allocation", "corpus-", "60001"],
    ] {
        let out = Command::new(binary).args(&bad).output().unwrap();
        assert!(!out.status.success());
    }
}
