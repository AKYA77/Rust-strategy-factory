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
    let native = dir.join("dataset.rsf");
    let restored = dir.join("restored.json");
    assert!(
        invoke(&[
            "pack-native",
            dataset.to_str().unwrap(),
            native.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert!(
        !invoke(&[
            "pack-native",
            dataset.to_str().unwrap(),
            native.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert!(
        invoke(&[
            "unpack-native",
            native.to_str().unwrap(),
            restored.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert_eq!(fs::read(&dataset).unwrap(), fs::read(&restored).unwrap());
    assert!(
        invoke(&["inspect", native.to_str().unwrap()])
            .status
            .success()
    );
    let d: Dataset = serde_json::from_slice(&fs::read(&dataset).unwrap()).unwrap();
    d.verify().unwrap();
    let compressed = dir.join("coverage.rsfz");
    let parquet = dir.join("coverage.parquet");
    for args in [
        vec![
            "compress-native",
            native.to_str().unwrap(),
            compressed.to_str().unwrap(),
        ],
        vec![
            "pack-parquet",
            compressed.to_str().unwrap(),
            parquet.to_str().unwrap(),
        ],
    ] {
        let out = invoke(&args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let mut reports = Vec::new();
    for path in [&dataset, &native, &compressed, &parquet] {
        let bytes = fs::read(path).unwrap();
        let out = invoke(&["coverage", path.to_str().unwrap(), "0", "121000", "59999"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(report["events"], 4);
        assert_eq!(report["latest"]["sequence"], "3");
        assert_eq!(report["gap_count"], 2);
        assert_eq!(fs::read(path).unwrap(), bytes);
        reports.push(report);
    }
    assert!(reports.windows(2).all(|pair| pair[0] == pair[1]));
    assert!(
        !invoke(&["coverage", dataset.to_str().unwrap(), "0", "0", "1"])
            .status
            .success()
    );
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
            native.to_str().unwrap(),
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
    let second = invoke(&[
        "experiment",
        native.to_str().unwrap(),
        strategy.to_str().unwrap(),
        registry.to_str().unwrap(),
    ]);
    assert!(second.status.success());
    let second: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(second["reused"], true);
    assert_eq!(first["experiment_id"], second["experiment_id"]);
    let compressed = dir.join("dataset.rsfz");
    let decompressed = dir.join("decompressed.rsf");
    assert!(
        invoke(&[
            "compress-native",
            native.to_str().unwrap(),
            compressed.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert!(
        invoke(&[
            "decompress-native",
            compressed.to_str().unwrap(),
            decompressed.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert_eq!(fs::read(&native).unwrap(), fs::read(&decompressed).unwrap());
    let compressed_result = invoke(&[
        "experiment",
        compressed.to_str().unwrap(),
        strategy.to_str().unwrap(),
        registry.to_str().unwrap(),
    ]);
    assert!(compressed_result.status.success());
    let compressed_result: serde_json::Value =
        serde_json::from_slice(&compressed_result.stdout).unwrap();
    assert_eq!(compressed_result["reused"], true);
    assert_eq!(compressed_result["experiment_id"], first["experiment_id"]);
    let parquet = dir.join("dataset.parquet");
    let parquet_json = dir.join("parquet-restored.json");
    assert!(
        invoke(&[
            "pack-parquet",
            compressed.to_str().unwrap(),
            parquet.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert!(
        invoke(&[
            "unpack-parquet",
            parquet.to_str().unwrap(),
            parquet_json.to_str().unwrap()
        ])
        .status
        .success()
    );
    let original: Dataset = serde_json::from_slice(&fs::read(&dataset).unwrap()).unwrap();
    let restored: Dataset = serde_json::from_slice(&fs::read(&parquet_json).unwrap()).unwrap();
    assert_eq!(original.ticks, restored.ticks);
    let parquet_result = invoke(&[
        "experiment",
        parquet.to_str().unwrap(),
        strategy.to_str().unwrap(),
        registry.to_str().unwrap(),
    ]);
    assert!(
        parquet_result.status.success(),
        "{}",
        String::from_utf8_lossy(&parquet_result.stderr)
    );
    let parquet_result: serde_json::Value = serde_json::from_slice(&parquet_result.stdout).unwrap();
    assert_eq!(parquet_result["reused"], true);
    assert_eq!(parquet_result["experiment_id"], first["experiment_id"]);
    let diagnosis = invoke(&[
        "diagnose",
        registry.to_str().unwrap(),
        first["experiment_id"].as_str().unwrap(),
    ]);
    assert!(
        diagnosis.status.success(),
        "{}",
        String::from_utf8_lossy(&diagnosis.stderr)
    );
    let diagnosis: serde_json::Value = serde_json::from_slice(&diagnosis.stdout).unwrap();
    assert_eq!(diagnosis["reused"], false);
    assert_eq!(diagnosis["report"]["status"], "TRAINING_ONLY_UNEVALUATED");
    let receipt = invoke(&[
        "diagnosis-report",
        registry.to_str().unwrap(),
        diagnosis["report"]["id"].as_str().unwrap(),
    ]);
    assert!(receipt.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&receipt.stdout).unwrap(),
        diagnosis["report"]
    );
    let counts = invoke(&["registry-check", registry.to_str().unwrap()]);
    assert!(counts.status.success());
    let counts: serde_json::Value = serde_json::from_slice(&counts.stdout).unwrap();
    assert_eq!(counts["completed"], 1);
    assert_eq!(counts["attempts"], 1);
    let review_config = dir.join("review-input.json");
    fs::write(&review_config,serde_json::to_vec(&serde_json::json!({"inputs":[{"dataset":parquet.file_name().unwrap().to_str().unwrap()}],"limits":{"feature_cells":4_000_000}})).unwrap()).unwrap();
    let review = invoke(&[
        "review-training",
        review_config.to_str().unwrap(),
        registry.to_str().unwrap(),
        diagnosis["report"]["id"].as_str().unwrap(),
    ]);
    assert!(
        review.status.success(),
        "{}",
        String::from_utf8_lossy(&review.stderr)
    );
    let review: serde_json::Value = serde_json::from_slice(&review.stdout).unwrap();
    assert_eq!(review["report"]["status"], "DESCRIPTIVE_TRAINING_ONLY");
    let repeat = invoke(&[
        "review-training",
        review_config.to_str().unwrap(),
        registry.to_str().unwrap(),
        diagnosis["report"]["id"].as_str().unwrap(),
    ]);
    assert!(repeat.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&repeat.stdout).unwrap()["reused"],
        true
    );
    let read = invoke(&[
        "training-review-report",
        registry.to_str().unwrap(),
        review["report"]["id"].as_str().unwrap(),
    ]);
    assert!(read.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&read.stdout).unwrap(),
        review["report"]
    );

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
