//! Registered full-period real-data execution and its qualification-gate binding.
//!
//! Every fixture here is synthetic tick data carrying a real data class. The fixtures prove the
//! software path only: no eligibility, approval or qualification is granted, and synthetic or
//! diagnostic evidence can never qualify.

use rust_strategy_factory::{
    contracts::Side,
    data::{self, DataClass, Tick},
    evidence_gates,
    historical_corpus::{
        self, EXPECTED_MONTHS, REGISTERED_FULL_PERIOD_CLASSIFICATION,
        REGISTERED_FULL_PERIOD_ELIGIBILITY,
    },
    qualification::{
        AccountingEvidence, CoverageEvidence, DataApprovalEvidence, EvidenceBundle,
        ExecutionIdentity, GateState, Policy, ScienceEvidence,
    },
    strategies::{Rule, Strategy},
    tick_engine::{CorpusBudget, Costs},
    validation::{Interval, PeriodStats},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

const SUBMITTER: &str = "fixture-preparer";
const SOURCE: &str = "DISPOSABLE_TEST_SOURCE";

fn hex64(c: char) -> String {
    std::iter::repeat_n(c, 64).collect()
}

fn month_start_ms(year: i32, month: u32) -> i64 {
    chrono::NaiveDate::from_ymd_opt(year, month, 1)
        .expect("valid fixture month")
        .and_hms_opt(0, 0, 0)
        .expect("valid fixture midnight")
        .and_utc()
        .timestamp_millis()
}

fn cutoff_2026() -> i64 {
    chrono::NaiveDate::from_ymd_opt(2026, 10, 31)
        .expect("valid cutoff date")
        .and_hms_opt(23, 59, 59)
        .expect("valid cutoff time")
        .and_utc()
        .timestamp_millis()
}

fn stats(
    resolved: usize,
    wins: usize,
    losses: usize,
    breakeven: usize,
    streak: usize,
) -> PeriodStats {
    PeriodStats {
        resolved,
        wins,
        losses,
        breakeven,
        max_losing_streak: streak,
    }
}

fn strategy() -> Strategy {
    Strategy {
        timeframe: rust_strategy_factory::candles::Timeframe::M1,
        side: Side::Long,
        stop_ticks: 100,
        rule: Rule::AboveMean { window: 2 },
        entry: None,
        stop_policy: None,
    }
}

/// Build one fixture month of deterministic synthetic quotes labelled with `class`.
fn build_month(root: &Path, index: usize, class: DataClass) -> historical_corpus::MonthlySource {
    let year = 2025 + i32::try_from(index).unwrap() / 12;
    let month = u32::try_from(index).unwrap() % 12 + 1;
    let dir = root.join(format!("{year:04}{month:02}"));
    fs::create_dir_all(&dir).unwrap();
    let profile = rust_strategy_factory::contracts::Profile::synthetic("EURUSD").unwrap();
    let base = profile.price_scale;
    let ticks: Vec<Tick> = (0..2000)
        .map(|i| {
            let phase = i % 80;
            let wave = if phase < 40 { phase } else { 80 - phase };
            let bid = base + wave * 5 * profile.pip;
            Tick {
                timestamp_ms: month_start_ms(year, month) + i * 10_000,
                sequence: i as u64,
                bid,
                ask: bid + profile.pip,
            }
        })
        .collect();
    let mut dataset = data::dataset(
        profile.clone(),
        ticks,
        &format!("FIXTURE_REAL_STRUCTURALLY_VALID {year:04}-{month:02}"),
        format!("fixture-{year:04}-{month:02}"),
        false,
    )
    .unwrap();
    dataset.manifest.class = class;
    let file = "chunk-000000.rsf".to_string();
    rust_strategy_factory::native::write(&dir.join(&file), &dataset).unwrap();
    historical_corpus::MonthlySource {
        instrument: "EURUSD".into(),
        year: u32::try_from(year).unwrap(),
        month,
        dataset_hash: data::dataset_identity(&dataset.manifest).unwrap(),
        source_sha256: "a".repeat(64),
        parquet_sha256: "b".repeat(64),
        rows: dataset.ticks.len() as u64,
        first_ms: dataset.manifest.first_ms,
        last_ms: dataset.manifest.last_ms,
        output_dir: dir.to_string_lossy().to_string(),
        parquet_path: dir.join("source.parquet").to_string_lossy().to_string(),
        chunk_files: vec![file],
        chunk_hashes: vec![data::hash(&dataset).unwrap()],
    }
}

/// A complete 22-month fixture corpus on disk with the requested declared class. The guard
/// removes the fixture directory when the test finishes.
struct FixtureDir(PathBuf);

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture(class: DataClass) -> (FixtureDir, historical_corpus::HistoricalCorpus) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "rsf-registered-full-period-{}-{class:?}-{serial}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let profile = rust_strategy_factory::contracts::Profile::synthetic("EURUSD").unwrap();
    let months: Vec<historical_corpus::MonthlySource> = (0..EXPECTED_MONTHS)
        .map(|index| build_month(&root, index, class))
        .collect();
    let identity = historical_corpus::identity("EURUSD", &profile, &months).unwrap();
    let spec = historical_corpus::HistoricalCorpus {
        version: historical_corpus::VERSION.into(),
        instrument: "EURUSD".into(),
        profile,
        months,
        identity,
        limitations: "Synthetic integration fixture; no market evidence or eligibility.".into(),
    };
    (FixtureDir(root), spec)
}

fn budget() -> CorpusBudget {
    CorpusBudget {
        max_candles: 10_000,
        feature_cells: 10_000,
        max_receipts: 10_000,
    }
}

fn training_window() -> Interval {
    Interval {
        start_ms: month_start_ms(2025, 1),
        end_exclusive_ms: month_start_ms(2025, 7),
    }
}

fn passing_bundle() -> EvidenceBundle {
    let strategy = strategy();
    let costs = Costs::default();
    let policy = Policy::owner_selected().unwrap();
    let declared_class = DataClass::RealStructurallyValid;
    EvidenceBundle {
        instrument: "EURUSD".into(),
        concept_id: strategy.identity().unwrap(),
        strategy,
        declared_class,
        profile_research_only: false,
        dataset_identity: hex64('a'),
        dataset_hash: hex64('b'),
        source: SOURCE.into(),
        submitter: SUBMITTER.into(),
        profile_hash: hex64('c'),
        cost_hash: data::hash(&costs).unwrap(),
        costs,
        execution: ExecutionIdentity {
            engine: rust_strategy_factory::ENGINE_VERSION.into(),
            indicators: rust_strategy_factory::INDICATOR_VERSION.into(),
            signals: rust_strategy_factory::SIGNAL_VERSION.into(),
            compute_revision: hex64('3'),
        },
        data_approval: DataApprovalEvidence {
            catalog_identity: hex64('6'),
            machine_id: hex64('5'),
            state: "DATA_APPROVED_FOR_SCOPE".into(),
            reasons: Vec::new(),
            review_head: Some(hex64('4')),
            live_machine_revalidated: true,
            scope: Interval {
                start_ms: policy.required_coverage_start_ms,
                end_exclusive_ms: policy.required_coverage_end_exclusive_ms,
            },
        },
        coverage: CoverageEvidence {
            receipt_hash: hex64('7'),
            catalog_identity: hex64('6'),
            start_ms: policy.required_coverage_start_ms,
            end_exclusive_ms: policy.required_coverage_end_exclusive_ms,
            months: policy.required_months().unwrap(),
            complete: true,
            events: 1_000_000,
        },
        accounting: AccountingEvidence {
            experiment_id: hex64('e'),
            result_hash: hex64('f'),
            classification: rust_strategy_factory::qualification::FULL_PERIOD_CLASSIFICATION.into(),
            cutoff_ms: cutoff_2026(),
            pooled: stats(200, 120, 60, 20, 3),
            year_2025: stats(100, 60, 30, 10, 3),
            year_2026: stats(100, 60, 30, 10, 3),
            net_micro_usd: 600_000_000,
            ending_micro_usd: 620_000_000,
            max_drawdown_micro_usd: 50_000_000,
            unresolved: 0,
        },
        science: ScienceEvidence {
            protocol_id: hex64('1'),
            report_hash: hex64('2'),
            report_data_class: declared_class,
            gate: evidence_gates::Assessment {
                method: evidence_gates::VERSION.into(),
                policy: evidence_gates::Policy {
                    minimum_folds: 2,
                    minimum_resolved_per_fold: 20,
                    minimum_resolved_per_year: 50,
                    minimum_net_win_rate_bps: 5000,
                    maximum_consecutive_losses: 5,
                    minimum_net_micro_usd: 1,
                },
                state: "NUMERICAL_EVIDENCE_ONLY".into(),
                criteria_passed: true,
                qualification_granted: false,
                decisions: Vec::new(),
                remaining_blockers: vec!["abstract fixture; no market evidence".into()],
            },
            robustness_cases_passed: 4,
            robustness_cases_total: 4,
            uncertainty_present: true,
            block_uncertainty_present: true,
        },
    }
}

#[test]
fn registered_full_period_real_data_run_emits_the_registered_classification() {
    let (_root, spec) = fixture(DataClass::RealStructurallyValid);
    let execution = historical_corpus::execute_registered_full_period(
        &spec,
        &strategy(),
        &Costs::default(),
        budget(),
        Some(training_window()),
    )
    .unwrap();
    assert_eq!(
        execution.classification,
        REGISTERED_FULL_PERIOD_CLASSIFICATION
    );
    assert_eq!(
        rust_strategy_factory::qualification::FULL_PERIOD_CLASSIFICATION,
        REGISTERED_FULL_PERIOD_CLASSIFICATION
    );
    assert_eq!(execution.eligibility, REGISTERED_FULL_PERIOD_ELIGIBILITY);
    assert_eq!(execution.months.len(), EXPECTED_MONTHS);
    assert_eq!(execution.totals.months_executed, EXPECTED_MONTHS);
    assert_eq!(execution.training_window, training_window());
    assert_eq!(
        execution.compute.engine,
        rust_strategy_factory::ENGINE_VERSION
    );
    assert_eq!(
        execution.compute.indicators,
        rust_strategy_factory::INDICATOR_VERSION
    );
    assert_eq!(
        execution.compute.signals,
        rust_strategy_factory::SIGNAL_VERSION
    );
    assert!(!execution.compute.compute_revision.is_empty());
    assert!(!execution.report_hash.is_empty());
    for month in &execution.months {
        assert_eq!(month.classification, REGISTERED_FULL_PERIOD_CLASSIFICATION);
        assert_eq!(month.eligibility, REGISTERED_FULL_PERIOD_ELIGIBILITY);
        assert!(!month.receipt_hash.is_empty());
        assert!(!month.corpus_identity.is_empty());
        assert!(!month.replay_dataset_id.is_empty());
    }
    // The exact accounting evidence conversion preserves the registered classification.
    let evidence = AccountingEvidence::from_registered_full_period(
        &execution,
        &"e".repeat(64),
        &"f".repeat(64),
        cutoff_2026(),
    )
    .unwrap();
    assert_eq!(
        evidence.classification,
        REGISTERED_FULL_PERIOD_CLASSIFICATION
    );
    // The accounting buckets are internally consistent and bound the continuous streak.
    let accounting = execution.accounting(cutoff_2026()).unwrap();
    assert_eq!(
        accounting.pooled.resolved,
        accounting.year_2025.resolved + accounting.year_2026.resolved
    );
    assert_eq!(
        accounting.pooled.wins,
        accounting.year_2025.wins + accounting.year_2026.wins
    );
    assert!(accounting.pooled.max_losing_streak <= accounting.pooled.losses);
    assert_eq!(accounting.unresolved, execution.totals.unresolved_positions);
    // A non-2026 cutoff is refused.
    assert!(execution.accounting(month_start_ms(2025, 1)).is_err());
}

#[test]
fn qualification_gate_accepts_registered_classification_and_rejects_diagnostic_with_exact_mismatch()
{
    let policy = Policy::owner_selected().unwrap();
    let bundle = passing_bundle();
    let accepted = rust_strategy_factory::qualification::assess(&bundle, &policy).unwrap();
    assert_eq!(accepted.state, "QUALIFICATION_EVIDENCE_COMPLETE");
    assert!(!accepted.qualification_granted);
    let gate = accepted
        .gates
        .iter()
        .find(|gate| gate.gate == "full-period-accounting")
        .unwrap();
    assert_eq!(gate.state, GateState::Passed);

    for diagnostic in [
        historical_corpus::EXECUTION_CLASSIFICATION,
        "continuous_diagnostic",
    ] {
        let mut rejected = bundle.clone();
        rejected.accounting.classification = diagnostic.into();
        let assessment = rust_strategy_factory::qualification::assess(&rejected, &policy).unwrap();
        assert_eq!(assessment.state, "REJECTED");
        assert!(!assessment.qualification_granted);
        let gate = assessment
            .gates
            .iter()
            .find(|gate| gate.gate == "full-period-accounting")
            .unwrap();
        assert_eq!(gate.state, GateState::Rejected);
        assert!(
            gate.explanation
                .contains(REGISTERED_FULL_PERIOD_CLASSIFICATION),
            "{}",
            gate.explanation
        );
        assert!(
            gate.explanation.contains(diagnostic),
            "{}",
            gate.explanation
        );
    }
}

#[test]
fn diagnostic_output_cannot_be_relabelled_into_the_registered_path() {
    for diagnostic in [
        historical_corpus::EXECUTION_CLASSIFICATION,
        "continuous_diagnostic",
    ] {
        let mismatch =
            rust_strategy_factory::qualification::accounting_classification_mismatch(diagnostic)
                .expect("a diagnostic classification must be a mismatch");
        assert!(
            mismatch.contains(REGISTERED_FULL_PERIOD_CLASSIFICATION),
            "{mismatch}"
        );
        assert!(mismatch.contains(diagnostic), "{mismatch}");
    }
    assert!(
        rust_strategy_factory::qualification::accounting_classification_mismatch(
            REGISTERED_FULL_PERIOD_CLASSIFICATION
        )
        .is_none()
    );
}

#[test]
fn real_class_input_without_training_window_is_refused() {
    let (_root, spec) = fixture(DataClass::RealStructurallyValid);
    let error = historical_corpus::execute_registered_full_period(
        &spec,
        &strategy(),
        &Costs::default(),
        budget(),
        None,
    )
    .expect_err("real data without an explicit training window must be refused");
    assert!(error.contains("training_window"), "{error}");
    assert!(error.contains("unseen-evidence preservation"), "{error}");
}

#[test]
fn data_class_outside_the_four_allowed_classes_is_refused() {
    let (_root, spec) = fixture(DataClass::ResearchEligible);
    let error = historical_corpus::execute_registered_full_period(
        &spec,
        &strategy(),
        &Costs::default(),
        budget(),
        Some(training_window()),
    )
    .expect_err("a class outside the four allowed classes must be refused");
    assert!(error.contains("ResearchEligible"), "{error}");
    assert!(error.contains("RealStructurallyValid"), "{error}");
}

#[test]
fn training_window_outside_the_corpus_is_refused() {
    let (_root, spec) = fixture(DataClass::RealStructurallyValid);
    let outside = Interval {
        start_ms: month_start_ms(2024, 1),
        end_exclusive_ms: month_start_ms(2024, 2),
    };
    let error = historical_corpus::execute_registered_full_period(
        &spec,
        &strategy(),
        &Costs::default(),
        budget(),
        Some(outside),
    )
    .expect_err("a training window outside the corpus must be refused");
    assert!(error.contains("inside the evaluated corpus"), "{error}");
}
