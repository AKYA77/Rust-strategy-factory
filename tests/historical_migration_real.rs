//! Real-corpus integration check. Skips cleanly when the retired factory's data is absent
//! (for example in hosted CI), so it never invents or downloads quotes.

use rust_strategy_factory::contracts::Profile;
use rust_strategy_factory::historical_migration::{self, Config};
use std::path::{Path, PathBuf};

fn migration_profile(symbol: &str) -> Profile {
    let tick = if symbol == "USDJPY" || symbol == "XAUUSD" {
        1_000
    } else {
        10
    };
    Profile {
        symbol: symbol.into(),
        version: "migration-profile-v1".into(),
        base: symbol[..3].into(),
        quote: symbol[3..].into(),
        price_scale: 1_000_000,
        tick,
        pip: tick * 10,
        contract_units: if symbol == "XAUUSD" { 100 } else { 100_000 },
        min_micro_lots: 10,
        step_micro_lots: 10,
        max_micro_lots: 1_000_000,
        leverage: 100,
        min_stop_ticks: 1,
        provenance: "MIGRATION_PROFILE_V1; source fixed-point scale 1000000; tick equals observed source quantum; pip is not an approved specification; not research eligible".into(),
        research_only: true,
    }
}

#[test]
fn real_samples_convert_and_verify() {
    let catalog = Path::new("/var/lib/hermes-factory/catalog");
    let samples: [(&str, &str, &str); 2] = [
        (
            "NZDUSD-202512",
            "093cec5a3fb6f81d3ed80f90e7dc19739e71da6bfc7f3905df3feda62952a8b6",
            "/var/lib/hermes-factory/catalog/import-receipts/adf264eeb86d4818bccd399449f081f4f09ec5f79de1e4f66893cbd9b3b9cca9.json",
        ),
        (
            "USDCAD-202607",
            "f1db4528d7ad1af9f3c4915c2fea598b7bf59dd84c7c7829a45a03c2475293c3",
            "/var/lib/hermes-factory/catalog/import-receipts/6f7ff56380bcdb9113ec9f0f1a5f08cd376f5d918a465b189fd495684841a1d1.json",
        ),
    ];
    let output_root = std::env::temp_dir().join(format!(
        "rsf-historical-real-integration-{}",
        std::process::id()
    ));
    let mut checked = 0;
    for (name, hash, receipt) in samples {
        let parquet = catalog.join(hash).join("quotes.parquet");
        if !parquet.is_file() {
            continue;
        }
        let manifest = catalog.join(hash).join("manifest.json");
        let output = output_root.join(name);
        let symbol = name.split('-').next().unwrap_or("NZDUSD");
        let mut config = Config::new(&parquet, &output, migration_profile(symbol))
            .unwrap()
            .with_manifest(&manifest)
            .unwrap()
            .with_chunk_events(100_000)
            .unwrap();
        let receipt_path = PathBuf::from(receipt);
        if receipt_path.is_file() {
            config = config.with_import_receipt(&receipt_path).unwrap();
        }
        let converted = historical_migration::convert(&config).unwrap();
        let verified = historical_migration::verify_output(&output, Some(&parquet)).unwrap();
        assert_eq!(converted.output_hash, verified.output_hash);
        assert_eq!(converted.events, verified.events);
        checked += 1;
    }
    if checked == 0 {
        eprintln!("skipping real-corpus integration: /var/lib/hermes-factory catalog absent");
    }
}
