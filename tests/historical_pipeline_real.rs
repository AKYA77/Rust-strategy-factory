//! Real historical-data structural pipeline acceptance.
//!
//! Path: verified native corpus → observed source facts → closed M1 candles → causal
//! signals, for all eight instruments on genuine VPS data. This is structural/diagnostic
//! only: it does not execute replay or grant eligibility. Corpus replay remains scoped to
//! synthetic literal-USD inputs; real/FX conversion streams are not integrated.

use rust_strategy_factory::candles::Timeframe;
use rust_strategy_factory::contracts::Side;
use rust_strategy_factory::corpus::{self};
use rust_strategy_factory::historical_corpus::{self, EXPECTED_MONTHS};
use rust_strategy_factory::strategies::{Rule, Strategy};
use rust_strategy_factory::tick_engine::{CorpusBudget, Costs};
use std::path::{Path, PathBuf};

const INSTRUMENTS: [&str; 8] = [
    "EURUSD", "GBPUSD", "USDJPY", "USDCHF", "USDCAD", "AUDUSD", "NZDUSD", "XAUUSD",
];
const MONTH: &str = "202510";

#[test]
fn real_historical_structural_pipeline() {
    let output = Path::new("/root/workspace/historical-migration-output/full-100k");
    if !output.is_dir() {
        eprintln!("skipping real historical structural pipeline: converted output absent");
        return;
    }
    let strategy = Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks: 100,
        rule: Rule::AboveMean { window: 2 },
        entry: None,
        stop_policy: None,
    };
    let mut total_events = 0u64;
    let mut total_candles = 0usize;
    let mut total_signals = 0usize;
    for instrument in INSTRUMENTS {
        let dir = output.join(format!("{instrument}-{MONTH}"));
        let mut chunks: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|_| panic!("missing {dir:?}"))
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "rsf"))
            .collect();
        chunks.sort();
        assert!(!chunks.is_empty(), "{instrument} has no native chunks");

        let spec = corpus::describe(&chunks, 100_000).unwrap();
        corpus::inspect(output, &spec).unwrap();
        let facts = corpus::source_facts(output, &spec).unwrap();
        assert_eq!(facts.profile().symbol, instrument);
        assert!(facts.events() > 0);

        let candles = corpus::candle_context(output, &spec, Timeframe::M1, 1_000_000).unwrap();
        assert!(!candles.is_empty());
        let mask = corpus::signals(output, &spec, &strategy, 1_000_000, 1_000_000).unwrap();
        assert!(mask.count() > 0, "{instrument} produced no signals");

        total_events += facts.events();
        total_candles += candles.len();
        total_signals += mask.count();
    }
    assert!(total_events > 0 && total_candles > 0 && total_signals > 0);
}

#[test]
fn real_historical_study_report() {
    let catalog_root = Path::new("/var/lib/hermes-factory");
    let output = Path::new("/root/workspace/historical-migration-output/full-100k");
    if !catalog_root.join("catalog").is_dir() || !output.is_dir() {
        eprintln!("skipping real historical study: data absent");
        return;
    }
    let spec =
        historical_corpus::register(catalog_root, output, "EURUSD", EXPECTED_MONTHS).unwrap();
    let strategy = Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks: 100,
        rule: Rule::AboveMean { window: 2 },
        entry: None,
        stop_policy: None,
    };
    let study = historical_corpus::study(&spec, &strategy, 1_000_000, 1_000_000).unwrap();
    assert_eq!(study.months.len(), EXPECTED_MONTHS);
    assert_eq!(
        study.events,
        spec.months.iter().map(|month| month.rows).sum::<u64>()
    );
    assert!(study.candles > 0 && study.signals > 0);
    let again = historical_corpus::study(&spec, &strategy, 1_000_000, 1_000_000).unwrap();
    assert_eq!(
        serde_json::to_string(&study).unwrap(),
        serde_json::to_string(&again).unwrap(),
        "study report must be deterministic"
    );
}

/// Diagnostic bridge from verified historical source data to the authoritative sequential engine.
/// Structural/diagnostic only: the report is labelled `diagnostic_simulation` with `none`
/// eligibility and is never registered research evidence.
#[test]
fn real_historical_diagnostic_execution() {
    let catalog_root = Path::new("/var/lib/hermes-factory");
    let output = Path::new("/root/workspace/historical-migration-output/full-100k");
    if !catalog_root.join("catalog").is_dir() || !output.is_dir() {
        eprintln!("skipping real historical diagnostic execution: data absent");
        return;
    }
    let strategy = Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks: 100,
        rule: Rule::AboveMean { window: 2 },
        entry: None,
        stop_policy: None,
    };
    let costs = Costs::default();
    let budget = CorpusBudget {
        max_candles: 1_000_000,
        feature_cells: 1_000_000,
        max_receipts: 1_000_000,
    };
    let mut usd_quote = 0usize;
    let mut refused = 0usize;
    // Bounded per-month sample (2025-10) across every instrument: proves the bridge and the
    // refusal classification without replaying all 176 months per instrument.
    let mut eurusd_sample = None;
    for instrument in INSTRUMENTS {
        let spec =
            historical_corpus::register(catalog_root, output, instrument, EXPECTED_MONTHS).unwrap();
        let sample = historical_corpus::execute_month(&spec, 9, &strategy, &costs, budget);
        if spec.profile.quote == "USD" {
            usd_quote += 1;
            let executed = sample.unwrap_or_else(|e| {
                panic!("{instrument} 2025-10 diagnostic execution failed: {e}")
            });
            assert_eq!(executed.year, 2025);
            assert_eq!(executed.month, 10);
            assert_eq!(executed.declared_class, "RealStructurallyValid");
            assert_eq!(executed.corpus_identity.len(), 64);
            assert_eq!(executed.receipt_hash.len(), 64);
            assert!(
                executed.trades + executed.rejections > 0,
                "{instrument}: sample month produced no engine activity"
            );
            if instrument == "EURUSD" {
                let repeated =
                    historical_corpus::execute_month(&spec, 9, &strategy, &costs, budget).unwrap();
                assert_eq!(
                    serde_json::to_string(&repeated).unwrap(),
                    serde_json::to_string(&executed).unwrap(),
                    "diagnostic execution must be deterministic"
                );
                eurusd_sample = Some(executed);
            }
        } else {
            refused += 1;
            let error = sample.expect_err("{instrument} must refuse a non-USD quote currency");
            assert!(error.contains("literal-USD"), "{instrument}: {error}");
        }
    }
    assert_eq!(usd_quote, 5, "five USD-quoted instruments");
    assert_eq!(
        refused, 3,
        "three non-USD-quote instruments refuse FX conversion"
    );

    // The full-corpus aggregate path is exercised by `historical-corpus-execute` and recorded in
    // docs/historical-migration/DIAGNOSTIC_EXECUTION_RECEIPT.json; running all 22 months here as
    // well would dominate the suite's runtime. The aggregate accounting itself is unit-tested.
    assert!(eurusd_sample.is_some());
}
