//! Real-corpus multi-month integration check. Skips cleanly when the VPS data is absent.

use rust_strategy_factory::historical_catalog::{self};
use rust_strategy_factory::historical_corpus::{self, EXPECTED_MONTHS};
use std::path::Path;

#[test]
fn real_corpus_registers_verifies_and_traverses() {
    let catalog = Path::new("/var/lib/hermes-factory");
    let output = Path::new("/root/workspace/historical-migration-output/full-100k");
    if !catalog.join("catalog").is_dir() || !output.is_dir() {
        eprintln!("skipping real historical corpus: catalog or converted output absent");
        return;
    }
    let spec = historical_corpus::register(catalog, output, "EURUSD", EXPECTED_MONTHS).unwrap();
    assert_eq!(spec.months.len(), EXPECTED_MONTHS);
    assert_eq!(spec.instrument, "EURUSD");
    let expected_rows: u64 = spec.months.iter().map(|m| m.rows).sum();
    historical_corpus::verify(&spec).unwrap();
    let mut visited = 0u64;
    historical_corpus::visit_month(&spec, 0, |dataset| {
        visited += dataset.ticks.len() as u64;
        Ok(())
    })
    .unwrap();
    assert_eq!(visited, spec.months[0].rows);
    assert!(expected_rows > 0);

    let catalog = historical_catalog::scan_catalog(
        catalog,
        output,
        &[
            "EURUSD", "GBPUSD", "USDJPY", "USDCHF", "USDCAD", "AUDUSD", "NZDUSD", "XAUUSD",
        ],
        EXPECTED_MONTHS,
    )
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
}
