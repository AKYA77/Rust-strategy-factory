//! Real historical-data structural pipeline acceptance.
//!
//! Path: verified native corpus → observed source facts → closed M1 candles → causal
//! signals, for all seven instruments on genuine VPS data. This is structural/diagnostic
//! only: it does not execute replay or grant eligibility. Corpus replay remains scoped to
//! synthetic literal-USD inputs; real/FX conversion streams are not integrated.

use rust_strategy_factory::candles::Timeframe;
use rust_strategy_factory::contracts::Side;
use rust_strategy_factory::corpus::{self};
use rust_strategy_factory::historical_corpus::{self, EXPECTED_MONTHS, ResearchWindow};
use rust_strategy_factory::strategies::{Rule, Strategy};
use rust_strategy_factory::tick_engine::{CorpusBudget, Costs};
use std::path::{Path, PathBuf};

const INSTRUMENTS: [&str; 7] = [
    "EURUSD", "GBPUSD", "USDJPY", "USDCHF", "USDCAD", "AUDUSD", "NZDUSD",
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
    let spec = historical_corpus::register_research_window(
        catalog_root,
        output,
        "EURUSD",
        &ResearchWindow::OWNER_SELECTED,
    )
    .unwrap();
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
    assert_eq!((study.months[0].year, study.months[0].month), (2025, 1));
    assert_eq!(
        (
            study.months[study.months.len() - 1].year,
            study.months[study.months.len() - 1].month
        ),
        (2026, 10)
    );
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
/// eligibility and is never registered research evidence. All eight instruments execute in
/// diagnostic mode: USD-quoted instruments use the identity conversion, and USD-base
/// non-USD-quote instruments (USDJPY/USDCHF/USDCAD) use the explicit direction-based
/// reciprocal diagnostic convention (long -> bid, short -> ask; USD per quote currency =
/// price_scale / price).
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
    let mut executed = 0usize;
    let mut identity = 0usize;
    let mut reciprocal = 0usize;
    let mut aggregate = ExecutedAggregate::default();
    // Bounded per-month sample (2025-10) across every instrument: proves the bridge for all
    // eight instruments without replaying all 176 months per instrument. Registration uses the
    // owner-selected 22-month pin, so every instrument must resolve exactly 2025-01..2026-10.
    for instrument in INSTRUMENTS {
        let spec = historical_corpus::register_research_window(
            catalog_root,
            output,
            instrument,
            &ResearchWindow::OWNER_SELECTED,
        )
        .unwrap();
        assert_eq!(spec.months.len(), EXPECTED_MONTHS, "{instrument}");
        assert_eq!(
            (spec.months[0].year, spec.months[0].month),
            (2025, 1),
            "{instrument}"
        );
        assert_eq!(
            (
                spec.months[spec.months.len() - 1].year,
                spec.months[spec.months.len() - 1].month
            ),
            (2026, 10),
            "{instrument}"
        );
        assert_eq!(
            (spec.months[9].year, spec.months[9].month),
            (2025, 10),
            "{instrument}: sample index must be the pinned 2025-10 month"
        );
        let sample = historical_corpus::execute_month(&spec, 9, &strategy, &costs, budget)
            .unwrap_or_else(|e| panic!("{instrument} 2025-10 diagnostic execution failed: {e}"));
        assert_eq!(sample.year, 2025);
        assert_eq!(sample.month, 10);
        assert_eq!(sample.declared_class, "RealStructurallyValid");
        assert_eq!(sample.corpus_identity.len(), 64);
        assert_eq!(sample.receipt_hash.len(), 64);
        let again = historical_corpus::execute_month(&spec, 9, &strategy, &costs, budget)
            .unwrap_or_else(|e| panic!("{instrument} 2025-10 replay failed: {e}"));
        assert_eq!(
            serde_json::to_string(&sample).unwrap(),
            serde_json::to_string(&again).unwrap(),
            "{instrument}: diagnostic execution must be deterministic"
        );
        if sample.conversion_policy == crate::historical_corpus::CONVERSION_POLICY_IDENTITY {
            identity += 1;
        } else if sample.conversion_policy == crate::historical_corpus::CONVERSION_POLICY_RECIPROCAL
        {
            reciprocal += 1;
        } else {
            panic!("{instrument}: unexpected conversion policy");
        }
        assert!(
            sample.trades + sample.rejections > 0,
            "{instrument}: sample month must produce engine activity"
        );
        assert_eq!(
            sample.outcomes.resolved,
            sample.outcomes.wins + sample.outcomes.losses + sample.outcomes.breakevens,
            "{instrument}: outcome counts must reconcile with resolved trades"
        );
        assert_eq!(sample.outcomes.resolved, sample.trades);
        assert!(sample.outcomes.max_losing_streak <= sample.outcomes.resolved);
        executed += 1;
        aggregate.trades += sample.trades;
        aggregate.rejections += sample.rejections;
        aggregate.outcome_wins += sample.outcomes.wins;
        aggregate.outcome_losses += sample.outcomes.losses;
        aggregate.outcome_breakevens += sample.outcomes.breakevens;
        aggregate.outcome_max_streak = aggregate
            .outcome_max_streak
            .max(sample.outcomes.max_losing_streak);
        aggregate.pnl_micro_usd += sample.final_marked_micro_usd - sample.initial_micro_usd;
        aggregate.maximum_drawdown_micro_usd = aggregate
            .maximum_drawdown_micro_usd
            .max(sample.maximum_drawdown_micro_usd);
    }
    // Every active instrument executes; the diagnostic simulation never refuses a real
    // structurally-valid literal-USD corpus (the old 5/3 split was the pre-change gate).
    // XAUUSD is out of scope under the V2 mandate: only the seven FX instruments remain.
    assert_eq!(executed, 7, "all seven active FX instruments must execute");
    assert_eq!(
        identity, 4,
        "USD-quoted FX instruments use the identity conversion"
    );
    assert_eq!(
        reciprocal, 3,
        "USD-base non-USD-quote instruments use the reciprocal convention"
    );
    assert!(
        aggregate.trades > 0,
        "aggregate must contain engine activity"
    );
    assert!(
        aggregate.rejections > 0,
        "aggregate must contain engine rejections"
    );
    assert_eq!(
        aggregate.trades,
        aggregate.outcome_wins + aggregate.outcome_losses + aggregate.outcome_breakevens,
        "aggregate outcome counts must reconcile with resolved trades"
    );
    assert!(aggregate.outcome_max_streak <= aggregate.trades);
}

#[derive(Default)]
struct ExecutedAggregate {
    trades: usize,
    rejections: usize,
    pnl_micro_usd: i128,
    maximum_drawdown_micro_usd: i128,
    outcome_wins: usize,
    outcome_losses: usize,
    outcome_breakevens: usize,
    outcome_max_streak: usize,
}
