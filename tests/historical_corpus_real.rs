//! Real-corpus end-to-end integration: registration, catalog status, cross-month
//! ordering and causal candle assembly. Skips cleanly when the VPS data is absent.
//! This test reads the full EURUSD corpus, so run it separately on the VPS.

use rust_strategy_factory::candles::{self, Timeframe};
use rust_strategy_factory::historical_catalog::{self};
use rust_strategy_factory::historical_corpus::{self, EXPECTED_MONTHS};
use std::path::Path;

const INSTRUMENTS: [&str; 8] = [
    "EURUSD", "GBPUSD", "USDJPY", "USDCHF", "USDCAD", "AUDUSD", "NZDUSD", "XAUUSD",
];

#[test]
fn real_corpus_end_to_end() {
    let catalog_root = Path::new("/var/lib/hermes-factory");
    let output = Path::new("/root/workspace/historical-migration-output/full-100k");
    if !catalog_root.join("catalog").is_dir() || !output.is_dir() {
        eprintln!("skipping real historical corpus: catalog or converted output absent");
        return;
    }
    let spec =
        historical_corpus::register(catalog_root, output, "EURUSD", EXPECTED_MONTHS).unwrap();
    assert_eq!(spec.months.len(), EXPECTED_MONTHS);
    assert_eq!(spec.instrument, "EURUSD");
    let expected_rows: u64 = spec.months.iter().map(|m| m.rows).sum();
    historical_corpus::verify(&spec).unwrap();

    let catalog =
        historical_catalog::scan_catalog(catalog_root, output, &INSTRUMENTS, EXPECTED_MONTHS)
            .unwrap();
    catalog.verify().unwrap();
    assert_eq!(catalog.instruments.len(), 8);
    assert!(catalog.instruments.iter().all(|i| i.structural_valid));
    assert!(catalog.instruments.iter().all(|i| !i.eligible_for_research));
    let usdcad = catalog
        .instruments
        .iter()
        .find(|i| i.instrument == "USDCAD")
        .unwrap();
    assert!(usdcad.months.iter().any(|m| m.quarantine_records == 353));

    // Cross-month ordering from the registered identities.
    for pair in spec.months.windows(2) {
        assert!(
            pair[1].first_ms >= pair[0].last_ms,
            "cross-month timestamp order"
        );
    }

    // Traverse the first and last month with causal candle assembly; each month restarts
    // its sequence space at zero and keeps its own source identity.
    let mut visited = 0u64;
    let mut candles_total = 0usize;
    for index in [0, spec.months.len() - 1] {
        let mut expected_sequence = 0u64;
        historical_corpus::visit_month(&spec, index, |dataset| {
            assert_eq!(dataset.ticks[0].sequence, expected_sequence);
            expected_sequence += dataset.ticks.len() as u64;
            assert!(
                dataset
                    .manifest
                    .source
                    .contains(&spec.months[index].dataset_hash)
            );
            let candles = candles::build(dataset, Timeframe::M1)?;
            candles_total += candles.len();
            visited += dataset.ticks.len() as u64;
            Ok(())
        })
        .unwrap();
    }
    assert!(visited > 0);
    assert!(candles_total > 0);
    assert!(expected_rows > visited);

    // Registration is deterministic.
    let again =
        historical_corpus::register(catalog_root, output, "EURUSD", EXPECTED_MONTHS).unwrap();
    assert_eq!(again.identity, spec.identity);

    // A truncated chunk inventory must be refused even if the identity is recomputed.
    let mut truncated = spec.clone();
    truncated.months[0].chunk_files.pop();
    truncated.months[0].chunk_hashes.pop();
    truncated.identity =
        historical_corpus::identity(&truncated.instrument, &truncated.profile, &truncated.months)
            .unwrap();
    assert!(historical_corpus::verify(&truncated).is_err());
}

#[test]
fn catalog_refuses_foreign_receipt() {
    let catalog_root = Path::new("/var/lib/hermes-factory");
    let output = Path::new("/root/workspace/historical-migration-output/full-100k");
    if !catalog_root.join("catalog").is_dir() || !output.is_dir() {
        eprintln!("skipping real catalog foreign-receipt check: data absent");
        return;
    }
    let tmp = std::env::temp_dir().join(format!("rsf-catalog-foreign-{}", std::process::id()));
    let dir = tmp.join("EURUSD-202501");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(
        output.join("AUDUSD-202501/manifest.json"),
        dir.join("manifest.json"),
    )
    .unwrap();
    let status =
        historical_catalog::scan_instrument(catalog_root, &tmp, "EURUSD", EXPECTED_MONTHS).unwrap();
    let january = status
        .months
        .iter()
        .find(|m| m.year == 2025 && m.month == 1)
        .unwrap();
    assert_eq!(
        january.state,
        rust_strategy_factory::historical_catalog::PartitionState::LoadFailure
    );
    assert!(!status.structural_valid);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// The blueprint's exact 24-completed-month research window must be configurable in software and
/// must report the precise shortfall rather than pad or fabricate periods.
#[test]
fn configurable_window_refuses_shortfall_and_selects_latest_months() {
    let catalog_root = Path::new("/var/lib/hermes-factory");
    let output = Path::new("/root/workspace/historical-migration-output/full-100k");
    if !catalog_root.join("catalog").is_dir() || !output.is_dir() {
        eprintln!("skipping real historical window: data absent");
        return;
    }
    let shortfall = historical_corpus::register_window(catalog_root, output, "EURUSD", 24)
        .expect_err("a 24-month window must be refused while fewer completed months exist");
    assert!(
        shortfall.contains("only 22 completed monthly partitions"),
        "{shortfall}"
    );
    assert!(shortfall.contains("24-month window"), "{shortfall}");
    assert!(historical_corpus::register_window(catalog_root, output, "EURUSD", 0).is_err());

    let window =
        historical_corpus::register_window(catalog_root, output, "EURUSD", EXPECTED_MONTHS)
            .unwrap();
    assert_eq!(window.months.len(), EXPECTED_MONTHS);
    assert_eq!((window.months[0].year, window.months[0].month), (2025, 1));
    let latest = historical_corpus::register_window(catalog_root, output, "EURUSD", 1).unwrap();
    assert_eq!(latest.months.len(), 1);
    assert_eq!((latest.months[0].year, latest.months[0].month), (2026, 10));
    assert_eq!(
        latest.months[0].dataset_hash,
        window.months.last().unwrap().dataset_hash
    );
    assert_eq!(
        latest.months[0].chunk_hashes,
        window.months.last().unwrap().chunk_hashes
    );
}
