//! Milestone 5 (full validation) contract tests.
//!
//! These tests pin the Milestone 5 acceptance contract using only local
//! synthetic fixtures and the public library API. They prove the software path
//! only: no external data, no live execution, and no qualification is granted.
//!
//! Requirements pinned:
//! 1. `REAL_STRUCTURALLY_VALID` is admitted to the frozen-protocol pipeline over
//!    the ten active families; synthetic evidence is never granted qualification.
//! 2. Prior-only training with a 24h purge embargo and boundary-leakage
//!    prevention; prior exposure is treated as contaminated, never unseen.
//! 3. Continuous chronological accounting with exact exit-year counts.
//! 4. All ten active families are evaluated; the annual resolved-trade floor is
//!    preserved and skips never count toward it.
//! 5. Exact scientific gates: synthetic excluded, >=50% net win rate, >=50
//!    resolved exits per calendar year, max loss streak 5, zero unresolved.

use rust_strategy_factory::{
    candles::Timeframe,
    contracts::{Conversion, INITIAL_EQUITY, MAX_HOLD_MS, Profile, Side},
    data::{self, DataClass, Tick},
    evidence_gates,
    qualification::{
        AccountingEvidence, CoverageEvidence, DataApprovalEvidence, EvidenceBundle,
        ExecutionIdentity, GateState, ScienceEvidence,
    },
    research::{self, Evaluation, Input},
    science::{self, Plan},
    storage::{Registry, Stage},
    strategies::{Family, Rule, Strategy, family_checkpoints},
    validation::{self, Fold, Interval, Observation},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
fn dir() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "factory-milestone5-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&p).unwrap();
    p
}
fn time(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .unwrap()
        .timestamp_millis()
}
fn span(start: &str, end: &str) -> Interval {
    Interval {
        start_ms: time(start),
        end_exclusive_ms: time(end),
    }
}
const CUTOFF: &str = "2026-11-12T00:00:00Z";

/// Fixture spanning the plan's four dated windows. Non-synthetic input carries
/// `REAL_STRUCTURALLY_VALID`; synthetic input carries `SYNTHETIC_TEST_ONLY`.
fn input(synthetic: bool) -> Input {
    let mut ticks = Vec::new();
    for date in [
        "2025-01-01T00:00:00Z",
        "2025-04-01T00:00:00Z",
        "2025-07-01T00:00:00Z",
        "2025-10-02T00:00:00Z",
    ] {
        for i in 0..500 {
            let phase = i % 80;
            let wave = if phase < 40 { phase } else { 80 - phase };
            let bid = 100_000 + wave * 50;
            ticks.push(Tick {
                timestamp_ms: time(date) + i * 10_000,
                sequence: ticks.len() as u64,
                bid,
                ask: bid + 20,
            });
        }
    }
    let n = ticks.len();
    Input {
        dataset: data::dataset(
            Profile::synthetic("EURUSD").unwrap(),
            ticks,
            if synthetic {
                "SYNTHETIC_TEST_ONLY milestone5 fixture"
            } else {
                "REAL_STRUCTURALLY_VALID milestone5 fixture; no market evidence"
            },
            "generated".into(),
            synthetic,
        )
        .unwrap(),
        conversions: vec![Conversion::usd(); n],
        economic_calendar: None,
    }
}

fn single_candidate() -> Vec<Strategy> {
    vec![Strategy {
        stop_policy: None,
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks: 100,
        rule: Rule::Rising { lookback: 2 },
        entry: None,
    }]
}

fn plan(candidates: Vec<Strategy>) -> Plan {
    Plan {
        win_evidence_policy: None,
        evidence_gate_policy: None,
        uncertainty_block_ms: None,
        robustness_policy: None,
        training: span("2025-01-01T00:00:00Z", "2025-03-01T00:00:00Z"),
        folds: vec![
            Fold {
                train: span("2025-01-01T00:00:00Z", "2025-03-31T00:00:00Z"),
                test: span("2025-04-01T00:00:00Z", "2025-07-01T00:00:00Z"),
                embargo_ms: MAX_HOLD_MS,
            },
            Fold {
                train: span("2025-01-01T00:00:00Z", "2025-06-30T00:00:00Z"),
                test: span("2025-07-01T00:00:00Z", "2025-10-01T00:00:00Z"),
                embargo_ms: MAX_HOLD_MS,
            },
        ],
        holdout: span("2025-10-02T00:00:00Z", "2026-01-02T00:00:00Z"),
        candidates,
        costs: Default::default(),
        trial_budget: u64::MAX,
    }
}

fn budget() -> science::Budget {
    science::Budget {
        feature_cells: 100_000,
        ..Default::default()
    }
}

fn observations() -> Vec<Observation> {
    (0..100)
        .map(|i| Observation {
            exit_ms: time(if i < 50 {
                "2025-06-01T00:00:00Z"
            } else {
                "2026-06-01T00:00:00Z"
            }) + i,
            net_micro_usd: if i % 2 == 0 { 11_000_000 } else { -1_000_000 },
        })
        .collect()
}

// ===== Requirement 1 =====

#[test]
fn req1_real_structurally_valid_is_admitted_and_never_granted_qualification() {
    let d = dir();
    let mut registry = Registry::open(&d.join("req1.sqlite")).unwrap();
    let p = plan(single_candidate());

    let real = science::execute(&mut registry, &input(false), &p, "m5-req1", &budget()).unwrap();
    assert_eq!(real.report.data_class, DataClass::RealStructurallyValid);
    assert_eq!(real.report.status, "INSUFFICIENT_EVIDENCE");
    assert_eq!(real.report.candidates.len(), 1);
    assert_eq!(real.report.candidates[0].failed_windows, 0);
    assert!(!real.report.missing_requirements.is_empty());
    let summary = science::summary(&real.report).unwrap();
    assert_eq!(summary["qualification_granted"], false);

    // Synthetic is permitted through the pipeline (software exercise) but is
    // never granted qualification either. Use a separate registry: the
    // identical tick stream in one registry would correctly be treated as
    // prior exposure rather than an unseen holdout.
    let d2 = dir();
    let mut registry2 = Registry::open(&d2.join("req1-synthetic.sqlite")).unwrap();
    let synthetic =
        science::execute(&mut registry2, &input(true), &p, "m5-req1", &budget()).unwrap();
    assert_eq!(synthetic.report.data_class, DataClass::SyntheticTestOnly);
    let synthetic_summary = science::summary(&synthetic.report).unwrap();
    assert_eq!(synthetic_summary["qualification_granted"], false);
    drop(registry);
    drop(registry2);
    std::fs::remove_dir_all(d).unwrap();
    std::fs::remove_dir_all(d2).unwrap();
}

#[test]
fn req1_classes_below_the_structural_bar_are_refused_before_reservation() {
    for class in [DataClass::RealUnreviewed, DataClass::ResearchEligible] {
        let d = dir();
        let mut registry = Registry::open(&d.join("refused.sqlite")).unwrap();
        let mut inp = input(false);
        inp.dataset.manifest.class = class;
        let err = science::execute(
            &mut registry,
            &inp,
            &plan(single_candidate()),
            "m5-refused",
            &budget(),
        )
        .unwrap_err();
        assert!(err.contains("blocked"), "{class:?}: {err}");
        drop(registry);
        std::fs::remove_dir_all(d).unwrap();
    }
}

// ===== Requirement 2 =====

#[test]
fn req2_walk_forward_folds_are_prior_only_with_a_24h_purge_embargo() {
    let folds = validation::walk_forward(
        time("2025-01-01T00:00:00Z"),
        time("2025-07-01T00:00:00Z"),
        time("2026-07-01T00:00:00Z"),
        MAX_HOLD_MS,
    )
    .unwrap();
    assert_eq!(folds.len(), 4);
    for f in &folds {
        assert_eq!(f.embargo_ms, MAX_HOLD_MS);
        // Training stops at least one maximum-holding horizon before testing.
        assert!(f.train.end_exclusive_ms.checked_add(MAX_HOLD_MS).unwrap() <= f.test.start_ms);
    }
    // Adjacent test windows share an exclusive boundary with no gap or overlap.
    assert_eq!(folds[0].test.end_exclusive_ms, folds[1].test.start_ms);
    // An embargo shorter than the maximum holding duration is refused.
    assert!(
        validation::walk_forward(
            time("2025-01-01T00:00:00Z"),
            time("2025-07-01T00:00:00Z"),
            time("2026-07-01T00:00:00Z"),
            MAX_HOLD_MS - 1,
        )
        .is_err()
    );
}

#[test]
fn req2_boundary_leakage_is_rejected_by_the_training_observation_contract() {
    let f = Fold {
        train: span("2025-01-01T00:00:00Z", "2025-03-31T00:00:00Z"),
        test: span("2025-04-01T00:00:00Z", "2025-07-01T00:00:00Z"),
        embargo_ms: MAX_HOLD_MS,
    };
    // A decision inside training whose exit falls outside it must not be claimed.
    assert!(!f.training_observation(time("2025-03-30T00:00:00Z"), time("2025-04-01T00:00:00Z")));
    // A decision and its exit both inside training is a valid observation.
    assert!(f.training_observation(time("2025-03-30T00:00:00Z"), time("2025-03-30T23:00:00Z")));
    // An exit before the decision is never a valid observation.
    assert!(!f.training_observation(time("2025-03-30T00:00:00Z"), time("2025-03-29T00:00:00Z")));
}

#[test]
fn req2_prior_exposure_is_treated_as_contaminated_never_unseen() {
    let d = dir();
    let mut registry = Registry::open(&d.join("exposed.sqlite")).unwrap();
    let p = plan(single_candidate());
    // An unfrozen exploration touches a future fold/test range first.
    research::evaluate_window(
        &mut registry,
        &input(false),
        &p.candidates[0],
        &p.costs,
        "m5-explore",
        100_000,
        &Evaluation {
            window: p.folds[0].test.clone(),
            stage: Stage::TickVerification,
            protocol_hash: "earlier-unfrozen-exploration".into(),
        },
    )
    .unwrap();
    // The frozen protocol must refuse rather than claim an unseen holdout.
    let err =
        science::execute(&mut registry, &input(false), &p, "m5-frozen", &budget()).unwrap_err();
    assert!(err.contains("already exposed"), "{err}");
    drop(registry);
    std::fs::remove_dir_all(d).unwrap();
}

// ===== Requirement 3 =====

#[test]
fn req3_continuous_accounting_reconciles_exact_exit_year_counts() {
    let a = validation::assess(&observations(), time(CUTOFF), 0).unwrap();
    // One chronological account: exit-year buckets sum exactly to the pool.
    assert_eq!(
        a.pooled.resolved,
        a.year_2025.resolved + a.year_2026.resolved
    );
    assert_eq!(a.pooled.wins, a.year_2025.wins + a.year_2026.wins);
    assert_eq!(a.pooled.losses, a.year_2025.losses + a.year_2026.losses);
    assert_eq!(
        a.pooled.breakeven,
        a.year_2025.breakeven + a.year_2026.breakeven
    );
    // Ending equity derives from the continuous initial-equity account, never a
    // sum of independently reset monthly balances.
    assert_eq!(a.net_micro_usd, 50 * 11_000_000 - 50 * 1_000_000);
    assert_eq!(
        a.ending_micro_usd,
        INITIAL_EQUITY.checked_add(a.net_micro_usd).unwrap()
    );
    // Non-chronological events are refused rather than silently sorted.
    let mut swapped = observations();
    swapped.swap(0, 99);
    assert!(validation::assess(&swapped, time(CUTOFF), 0).is_err());
    // A trade beyond the assessment cutoff is refused.
    let mut future = observations();
    future[99].exit_ms = time("2026-12-01T00:00:00Z");
    assert!(validation::assess(&future, time(CUTOFF), 0).is_err());
}

// ===== Requirement 4 =====

#[test]
fn req4_all_ten_active_families_are_evaluated_in_one_frozen_protocol() {
    let d = dir();
    let mut registry = Registry::open(&d.join("families.sqlite")).unwrap();

    let checkpoints = family_checkpoints();
    assert_eq!(checkpoints.len(), 10, "exactly ten active families");
    assert_eq!(Family::ALL.len(), 10);
    let mut seen = std::collections::BTreeSet::new();
    for c in &checkpoints {
        assert!(seen.insert(c.family), "duplicate family checkpoint");
        c.strategy.canonical().unwrap();
    }
    assert_eq!(seen.len(), 10);

    let p = plan(checkpoints.into_iter().map(|c| c.strategy).collect());
    let result =
        science::execute(&mut registry, &input(false), &p, "m5-families", &budget()).unwrap();
    let report = &result.report;
    assert_eq!(report.candidates.len(), 10);
    assert_eq!(report.status, "INSUFFICIENT_EVIDENCE");
    for c in &report.candidates {
        // selection-training + 2 x (prior-training, out-of-sample) + protected-final
        assert_eq!(c.experiments.len(), 6);
        assert_eq!(c.failed_windows, 0);
    }
    // At least one family produced actual resolved out-of-sample trades; empty
    // cohorts are honest, not silent skips.
    assert!(
        report
            .candidates
            .iter()
            .any(|c| c.pooled_oos_reset_fold_stats.closed > 0)
    );
    let summary = science::summary(report).unwrap();
    assert_eq!(summary["qualification_granted"], false);
    drop(registry);
    std::fs::remove_dir_all(d).unwrap();
}

// ===== Requirement 5 =====

#[test]
fn req5_scientific_gates_pin_the_owner_criteria_exactly() {
    // Exactly 50% wins and exactly 50 resolved exits in each year is numerically
    // sufficient, but numeric sufficiency is never qualification.
    let a = validation::assess(&observations(), time(CUTOFF), 0).unwrap();
    assert!(a.numeric_gates_passed);
    assert_eq!(a.status, "INSUFFICIENT_EVIDENCE");
    assert_eq!((a.pooled.wins, a.pooled.resolved), (50, 100));
    assert_eq!((a.year_2025.resolved, a.year_2026.resolved), (50, 50));

    // Below 50% net win rate fails.
    let mut low = observations();
    low[0].net_micro_usd = 0;
    assert!(
        !validation::assess(&low, time(CUTOFF), 0)
            .unwrap()
            .numeric_gates_passed
    );

    // Fewer than 50 resolved exits in a calendar year fails (skipped days do not
    // count toward the floor).
    let thin_2026: Vec<Observation> = (0..100)
        .map(|i| Observation {
            exit_ms: if i < 60 {
                time("2025-06-01T00:00:00Z") + i
            } else {
                time("2026-06-01T00:00:00Z") + i
            },
            net_micro_usd: 11_000_000,
        })
        .collect();
    let thin = validation::assess(&thin_2026, time(CUTOFF), 0).unwrap();
    assert_eq!((thin.year_2025.resolved, thin.year_2026.resolved), (60, 40));
    assert!(!thin.numeric_gates_passed);

    // A loss streak longer than five fails.
    let mut streak = observations();
    for t in &mut streak[0..6] {
        t.net_micro_usd = -1;
    }
    let s = validation::assess(&streak, time(CUTOFF), 0).unwrap();
    assert!(s.pooled.max_losing_streak >= 6);
    assert!(!s.numeric_gates_passed);

    // Any unresolved position fails.
    assert!(
        !validation::assess(&observations(), time(CUTOFF), 1)
            .unwrap()
            .numeric_gates_passed
    );

    // Frozen OOS/holdout only: the protocol refuses the absence of independent
    // evidence regardless of numerical results.
    assert!(
        a.reasons
            .last()
            .unwrap()
            .contains("independent protected validation")
    );
}

// ===== Requirement 5 (gate level): synthetic can never qualify =====

fn hex64(c: char) -> String {
    std::iter::repeat_n(c, 64).collect()
}

fn gate_bundle(declared_class: DataClass) -> EvidenceBundle {
    let strategy = single_candidate().remove(0);
    let costs = rust_strategy_factory::tick_engine::Costs::default();
    let policy = rust_strategy_factory::qualification::Policy::owner_selected().unwrap();
    let net = 600_000_000_i128;
    let stats = |resolved: usize, wins: usize| rust_strategy_factory::validation::PeriodStats {
        resolved,
        wins,
        losses: resolved - wins,
        breakeven: 0,
        max_losing_streak: 1,
    };
    EvidenceBundle {
        instrument: "EURUSD".into(),
        concept_id: strategy.identity().unwrap(),
        strategy,
        declared_class,
        profile_research_only: false,
        dataset_identity: hex64('a'),
        dataset_hash: hex64('b'),
        source: "MILESTONE5_GATE_FIXTURE".into(),
        submitter: "fixture".into(),
        profile_hash: hex64('c'),
        costs: costs.clone(),
        cost_hash: data::hash(&costs).unwrap(),
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
            cutoff_ms: time(CUTOFF),
            pooled: stats(200, 120),
            year_2025: stats(100, 60),
            year_2026: stats(100, 60),
            net_micro_usd: net,
            ending_micro_usd: INITIAL_EQUITY.checked_add(net).unwrap(),
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
                remaining_blockers: vec!["milestone5 abstract fixture; no market evidence".into()],
            },
            robustness_cases_passed: 4,
            robustness_cases_total: 4,
            uncertainty_present: true,
            block_uncertainty_present: true,
        },
    }
}

#[test]
fn req5_synthetic_evidence_is_rejected_at_the_gate_regardless_of_numbers() {
    let policy = rust_strategy_factory::qualification::Policy::owner_selected().unwrap();

    let synthetic = rust_strategy_factory::qualification::assess(
        &gate_bundle(DataClass::SyntheticTestOnly),
        &policy,
    )
    .unwrap();
    let class_gate = synthetic
        .gates
        .iter()
        .find(|g| g.gate == "data-class")
        .unwrap();
    assert_eq!(class_gate.state, GateState::Rejected);
    assert_eq!(synthetic.state, "REJECTED");
    assert!(!synthetic.qualification_granted);

    // A structurally valid declared class clears the data-class gate; with every
    // other evidence gate satisfied the assessment reaches
    // QUALIFICATION_EVIDENCE_COMPLETE, but qualification itself is never
    // self-granted -- it still requires an authenticated independent decision.
    let real = rust_strategy_factory::qualification::assess(
        &gate_bundle(DataClass::RealStructurallyValid),
        &policy,
    )
    .unwrap();
    let class_gate = real.gates.iter().find(|g| g.gate == "data-class").unwrap();
    assert_eq!(class_gate.state, GateState::Passed);
    assert_eq!(real.state, "QUALIFICATION_EVIDENCE_COMPLETE");
    assert!(!real.qualification_granted);
}
