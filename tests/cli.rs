use rust_strategy_factory::{contracts::Profile, data::Dataset, tick_engine::Replay};
use std::{fs, process::Command};
#[test]
fn import_inspect_evaluate_and_overwrite_protection() {
    let dir = std::env::temp_dir().join(format!("factory-cli-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let csv = dir.join("quotes.csv");
    let profile = dir.join("profile.json");
    let dataset = dir.join("dataset.json");
    let strategy = dir.join("strategy.json");
    let result = dir.join("result.json");
    fs::write(&csv,"timestamp_ms,sequence,bid,ask\n0,0,1.00000,1.00010\n60000,1,1.00020,1.00030\n120000,2,1.00040,1.00050\n120000,3,1.00050,1.00060\n121000,4,1.00300,1.00310\n").unwrap();
    fs::write(
        &profile,
        serde_json::to_vec(&Profile::synthetic("EURUSD").unwrap()).unwrap(),
    )
    .unwrap();
    fs::write(
        &strategy,
        r#"{"timeframe":"M1","side":"Long","stop_ticks":50,"rule":{"AboveMean":{"window":2}}}"#,
    )
    .unwrap();
    let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
    let invoke = |args: &[&str]| Command::new(binary).args(args).output().unwrap();
    let args = [
        "import",
        csv.to_str().unwrap(),
        profile.to_str().unwrap(),
        "synthetic-cli-fixture",
        dataset.to_str().unwrap(),
        "synthetic",
    ];
    let out = invoke(&args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let d: Dataset = serde_json::from_slice(&fs::read(&dataset).unwrap()).unwrap();
    d.verify().unwrap();
    assert_eq!(
        d.manifest.archive_hash,
        blake3::hash(&fs::read(&csv).unwrap()).to_hex().to_string()
    );
    assert!(
        invoke(&["inspect", dataset.to_str().unwrap()])
            .status
            .success()
    );
    assert!(!invoke(&args).status.success());
    assert!(
        invoke(&[
            "evaluate",
            dataset.to_str().unwrap(),
            strategy.to_str().unwrap(),
            result.to_str().unwrap()
        ])
        .status
        .success()
    );
    let r: Replay = serde_json::from_slice(&fs::read(&result).unwrap()).unwrap();
    assert_eq!(r.trades.len(), 1);
    assert_eq!(r.trades[0].net_micro_usd, 2_881_500);
    let registry = dir.join("registry.sqlite");
    let backup = dir.join("backup.sqlite");
    let args = [
        "experiment",
        dataset.to_str().unwrap(),
        strategy.to_str().unwrap(),
        registry.to_str().unwrap(),
    ];
    let first = invoke(&args);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["reused"], false);
    let second = invoke(&args);
    assert!(second.status.success());
    let second: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(second["reused"], true);
    assert_eq!(first["experiment_id"], second["experiment_id"]);
    let counts = invoke(&["registry-check", registry.to_str().unwrap()]);
    assert!(counts.status.success());
    let counts: serde_json::Value = serde_json::from_slice(&counts.stdout).unwrap();
    assert_eq!(counts["completed"], 1);
    assert_eq!(counts["attempts"], 1);
    assert!(
        invoke(&[
            "backup",
            registry.to_str().unwrap(),
            backup.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert!(
        invoke(&["registry-check", backup.to_str().unwrap()])
            .status
            .success()
    );
    fs::remove_dir_all(dir).unwrap();
}
