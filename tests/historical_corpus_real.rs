//! Real-corpus end-to-end integration: registration, catalog status, cross-month
//! ordering and causal candle assembly. Skips cleanly when the VPS data is absent.
//! This test reads the full EURUSD corpus, so run it separately on the VPS.

use rust_strategy_factory::candles::{self, Timeframe};
use rust_strategy_factory::historical_catalog::{self};
use rust_strategy_factory::historical_corpus::{self, EXPECTED_MONTHS, ResearchWindow};
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
    // The canonical corpus is exactly the owner-selected 22-month window.
    assert_eq!((spec.months[0].year, spec.months[0].month), (2025, 1));
    assert_eq!(
        (
            spec.months[spec.months.len() - 1].year,
            spec.months[spec.months.len() - 1].month
        ),
        (2026, 10)
    );
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

/// The configurable completed-month mechanism must report the precise shortfall rather than pad
/// or fabricate periods. The owner-selected active window is the explicit 22-month pin
/// 2025-01..2026-10 (owner decision 2026-11-19); 23- and 24-month windows exceed the available
/// completed months and must be refused.
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
    let shortfall23 = historical_corpus::register_window(catalog_root, output, "EURUSD", 23)
        .expect_err("a 23-month window must be refused while only 22 completed months exist");
    assert!(
        shortfall23.contains("only 22 completed monthly partitions"),
        "{shortfall23}"
    );
    assert!(shortfall23.contains("23-month window"), "{shortfall23}");
    assert!(historical_corpus::register_window(catalog_root, output, "EURUSD", 0).is_err());

    let window =
        historical_corpus::register_window(catalog_root, output, "EURUSD", EXPECTED_MONTHS)
            .unwrap();
    assert_eq!(window.months.len(), EXPECTED_MONTHS);
    assert_eq!((window.months[0].year, window.months[0].month), (2025, 1));
    assert_eq!(
        (
            window.months[window.months.len() - 1].year,
            window.months[window.months.len() - 1].month
        ),
        (2026, 10)
    );
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

/// The owner-selected research window is an explicit calendar pin: it must resolve to exactly
/// the 22 completed months 2025-01..2026-10 of the canonical corpus, bind the same sources as
/// the exact-count registration (unchanged scientific meaning), and pass full verification.
#[test]
fn owner_selected_research_window_pins_2025_01_to_2026_10() {
    let catalog_root = Path::new("/var/lib/hermes-factory");
    let output = Path::new("/root/workspace/historical-migration-output/full-100k");
    if !catalog_root.join("catalog").is_dir() || !output.is_dir() {
        eprintln!("skipping real owner-selected research window: data absent");
        return;
    }
    let window = ResearchWindow::OWNER_SELECTED;
    assert_eq!(window.label(), "2025-01..2026-10");
    assert_eq!(window.month_count().unwrap(), EXPECTED_MONTHS);
    let spec = historical_corpus::register_research_window(catalog_root, output, "EURUSD", &window)
        .unwrap();
    assert_eq!(spec.months.len(), 22);
    assert_eq!((spec.months[0].year, spec.months[0].month), (2025, 1));
    assert_eq!(
        (
            spec.months[spec.months.len() - 1].year,
            spec.months[spec.months.len() - 1].month
        ),
        (2026, 10)
    );
    // Every one of the 22 calendar months is present exactly once, in order.
    let months: Vec<(u32, u32)> = spec.months.iter().map(|m| (m.year, m.month)).collect();
    assert_eq!(months, window.months().unwrap());
    historical_corpus::verify(&spec).unwrap();

    // The pin binds the identical canonical sources as the exact-count registration and the
    // latest-22 selection: no reinterpretation of completed results.
    let counted =
        historical_corpus::register(catalog_root, output, "EURUSD", EXPECTED_MONTHS).unwrap();
    assert_eq!(spec.identity, counted.identity);
    let latest22 =
        historical_corpus::register_window(catalog_root, output, "EURUSD", EXPECTED_MONTHS)
            .unwrap();
    assert_eq!(spec.identity, latest22.identity);
}

/// Windows extending beyond the available completed months must be refused with the exact
/// missing months — no November/December 2026, no padding, no backfill. Valid sub-windows of
/// the completed corpus still resolve exactly.
#[test]
fn research_window_refuses_months_beyond_completed_corpus() {
    let catalog_root = Path::new("/var/lib/hermes-factory");
    let output = Path::new("/root/workspace/historical-migration-output/full-100k");
    if !catalog_root.join("catalog").is_dir() || !output.is_dir() {
        eprintln!("skipping real research-window refusal checks: data absent");
        return;
    }
    let register = |window: ResearchWindow| {
        historical_corpus::register_research_window(catalog_root, output, "EURUSD", &window)
    };

    // Extends past the last completed month (2026-10): both missing months are named.
    let beyond = ResearchWindow {
        start_year: 2025,
        start_month: 1,
        end_year: 2026,
        end_month: 12,
    };
    let error = register(beyond).expect_err("2026-11/2026-12 are not completed/available");
    assert!(error.contains("2025-01..2026-12"), "{error}");
    assert!(error.contains("requires 24 completed months"), "{error}");
    assert!(error.contains("only 22 available"), "{error}");
    assert!(error.contains("missing: 2026-11, 2026-12"), "{error}");

    // A shifted 22-month window still demands the unavailable 2026-11.
    let shifted = ResearchWindow {
        start_year: 2025,
        start_month: 2,
        end_year: 2026,
        end_month: 11,
    };
    let error = register(shifted).expect_err("2026-11 is not completed/available");
    assert!(error.contains("missing: 2026-11"), "{error}");

    // Starts before the first completed month.
    let early = ResearchWindow {
        start_year: 2024,
        start_month: 12,
        end_year: 2026,
        end_month: 10,
    };
    let error = register(early).expect_err("2024-12 is not part of the canonical corpus");
    assert!(error.contains("missing: 2024-12"), "{error}");

    // Entirely beyond the corpus.
    let future = ResearchWindow {
        start_year: 2026,
        start_month: 11,
        end_year: 2026,
        end_month: 12,
    };
    let error = register(future).expect_err("no completed month exists in 2026-11..2026-12");
    assert!(error.contains("missing: 2026-11, 2026-12"), "{error}");

    // Valid sub-windows resolve exactly, with explicit boundaries.
    let sub = ResearchWindow {
        start_year: 2025,
        start_month: 3,
        end_year: 2025,
        end_month: 5,
    };
    let spec = register(sub).unwrap();
    assert_eq!(spec.months.len(), 3);
    assert_eq!((spec.months[0].year, spec.months[0].month), (2025, 3));
    assert_eq!((spec.months[2].year, spec.months[2].month), (2025, 5));
    let single = ResearchWindow {
        start_year: 2026,
        start_month: 10,
        end_year: 2026,
        end_month: 10,
    };
    let spec = register(single).unwrap();
    assert_eq!(spec.months.len(), 1);
    assert_eq!((spec.months[0].year, spec.months[0].month), (2026, 10));
}
