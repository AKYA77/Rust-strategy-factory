use rust_strategy_factory::{
    candles::Timeframe,
    contracts::{Conversion, MAX_HOLD_MS, Profile, Side},
    data::{self, Tick},
    research::{self, Evaluation, Input},
    science::{self, Plan},
    storage::{Registry, Stage},
    strategies::{Rule, Strategy},
    validation::{Fold, Interval},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn dir() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "factory-science-{}-{}",
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
fn plan() -> Plan {
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
        candidates: vec![Strategy {
            stop_policy: None,
            timeframe: Timeframe::M1,
            side: Side::Long,
            stop_ticks: 100,
            rule: Rule::Rising { lookback: 2 },
            entry: None,
        }],
        costs: Default::default(),
        trial_budget: 1000,
    }
}
fn input() -> Input {
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
            let bid = 100000 + wave * 50;
            ticks.push(Tick {
                timestamp_ms: time(date) + i * 10000,
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
            "SYNTHETIC_TEST_ONLY sparse quarter protocol fixture",
            "generated".into(),
            true,
        )
        .unwrap(),
        conversions: vec![Conversion::usd(); n],
        economic_calendar: None,
    }
}
#[test]
fn frozen_protocol_executes_all_windows_and_reuses_an_immutable_report() {
    let dir = dir();
    let path = dir.join("state.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    let input = input();
    let p = plan();
    let first = science::execute(
        &mut registry,
        &input,
        &p,
        "fixed-code",
        &science::Budget {
            feature_cells: 100000,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!first.reused);
    assert_eq!(first.report.status, "INSUFFICIENT_EVIDENCE");
    assert_eq!(first.report.candidates[0].experiments.len(), 6);
    assert!(
        first.report.candidates[0]
            .pooled_oos_reset_fold_stats
            .closed
            > 0
    );
    assert_eq!(first.report.candidates[0].failed_windows, 0);
    assert_eq!(first.report.candidates[0].robustness.len(), 5);
    for probe in &first.report.candidates[0].robustness {
        assert_eq!(probe.experiments.len(), 2);
        assert_eq!(probe.status, "DESCRIPTIVE_ONLY");
        assert_eq!(probe.failed_windows, 0);
        assert!(probe.pooled_reset_fold_stats.closed > 0);
        let conn = rusqlite::Connection::open(&path).unwrap();
        for experiment in &probe.experiments {
            let json: String = conn
                .query_row(
                    "SELECT key_json FROM experiments WHERE id=?1",
                    [&experiment.experiment_id],
                    |r| r.get(0),
                )
                .unwrap();
            let key: rust_strategy_factory::storage::ExperimentKey =
                serde_json::from_str(&json).unwrap();
            assert_eq!(key.stage, Stage::TickVerification);
            assert_eq!(key.concept_hash, probe.concept_id);
            assert_eq!(key.cost_hash, probe.cost_hash);
            assert_eq!(key.protocol_hash, first.report.protocol_id);
            assert!(p.folds.iter().any(|fold| fold.test.start_ms == key.start_ms
                && fold.test.end_exclusive_ms == key.end_exclusive_ms));
            assert!(key.end_exclusive_ms < p.holdout.start_ms);
        }
    }
    let candidate = &first.report.candidates[0];
    let breakdown = candidate.breakdown.as_ref().unwrap();
    assert_eq!(breakdown.folds.len(), 2);
    assert_eq!(
        breakdown
            .entry_regimes
            .values()
            .map(|b| b.resolved)
            .sum::<usize>(),
        candidate.pooled_oos_reset_fold_stats.closed
    );
    assert_eq!(
        breakdown
            .entry_momentum
            .values()
            .map(|b| b.resolved)
            .sum::<usize>(),
        candidate.pooled_oos_reset_fold_stats.closed
    );
    assert!(!breakdown.entry_momentum.is_empty());
    assert_eq!(
        breakdown
            .exit_years
            .values()
            .map(|b| b.resolved)
            .sum::<usize>(),
        candidate.pooled_oos_reset_fold_stats.closed
    );
    assert_eq!(
        breakdown
            .entry_utc_six_hour_bins
            .values()
            .map(|b| b.sum_net_micro_usd)
            .sum::<i128>(),
        candidate.pooled_oos_reset_fold_stats.sum_net_micro_usd
    );
    let uncertainty = candidate.uncertainty.as_ref().unwrap();
    assert_eq!(
        uncertainty.observations,
        candidate.pooled_oos_reset_fold_stats.closed
    );
    assert_eq!(
        uncertainty.net_wins,
        candidate.pooled_oos_reset_fold_stats.wins
    );
    assert_eq!(uncertainty.declared_trials, 1000);
    assert!(!uncertainty.assumptions_verified);
    let before = registry.counts().unwrap();
    drop(registry);
    let mut registry = Registry::open(&path).unwrap();
    let again = science::execute(
        &mut registry,
        &input,
        &p,
        "fixed-code",
        &science::Budget {
            feature_cells: 100000,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(again.reused);
    assert_eq!(first.report.protocol_id, again.report.protocol_id);
    assert_eq!(before.attempts, registry.counts().unwrap().attempts);
    let mut retuned = p;
    retuned.candidates[0].stop_ticks += 1;
    assert!(
        science::execute(
            &mut registry,
            &input,
            &retuned,
            "fixed-code",
            &science::Budget {
                feature_cells: 100000,
                ..Default::default()
            }
        )
        .is_err()
    );
    registry.integrity().unwrap();
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn retrospective_oos_exposure_is_rejected_before_final_reservation() {
    let dir = dir();
    let mut registry = Registry::open(&dir.join("state.sqlite")).unwrap();
    let input = input();
    let p = plan();
    research::evaluate_window(
        &mut registry,
        &input,
        &p.candidates[0],
        &p.costs,
        "fixed-code",
        100000,
        &Evaluation {
            window: p.folds[0].test.clone(),
            stage: Stage::TickVerification,
            protocol_hash: "earlier-unfrozen-exploration".into(),
        },
    )
    .unwrap();
    assert!(
        science::execute(
            &mut registry,
            &input,
            &p,
            "fixed-code",
            &science::Budget {
                feature_cells: 100000,
                ..Default::default()
            }
        )
        .is_err()
    );
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn revised_quote_history_cannot_hide_prior_out_of_sample_exposure() {
    let dir = dir();
    let mut registry = Registry::open(&dir.join("state.sqlite")).unwrap();
    let original = input();
    let p = plan();
    research::evaluate_window(
        &mut registry,
        &original,
        &p.candidates[0],
        &p.costs,
        "fixed-code",
        100000,
        &Evaluation {
            window: p.folds[0].test.clone(),
            stage: Stage::TickVerification,
            protocol_hash: "earlier-exploration".into(),
        },
    )
    .unwrap();
    let mut revised = input();
    revised.dataset.ticks[0].bid -= 1;
    revised.dataset.manifest.content_hash = data::hash(&revised.dataset.ticks).unwrap();
    revised.dataset.manifest.archive_hash = "revised-archive".into();
    revised.dataset.verify().unwrap();
    assert_ne!(
        original.dataset.manifest.content_hash,
        revised.dataset.manifest.content_hash
    );
    let error = science::execute(
        &mut registry,
        &revised,
        &p,
        "fixed-code",
        &science::Budget::default(),
    )
    .unwrap_err();
    assert!(error.contains("already exposed"));
    assert_eq!(registry.counts().unwrap().registered, 1);
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn bad_calendar_fold_embargo_selection_and_overlap_fail_structural_validation() {
    let mut p = plan();
    p.folds[0].test.end_exclusive_ms -= 1;
    assert!(p.canonical().is_err());
    let mut p = plan();
    p.folds[0].embargo_ms = 0;
    assert!(p.canonical().is_err());
    let mut p = plan();
    p.candidates.push(p.candidates[0].clone());
    assert!(p.canonical().is_err());
    let mut p = plan();
    p.holdout.start_ms = p.folds[1].test.end_exclusive_ms;
    assert!(p.canonical().is_err());
    let mut p = plan();
    p.trial_budget = 0;
    assert!(p.canonical().is_err());
    let mut p = plan();
    p.trial_budget = 3;
    assert!(p.canonical().unwrap_err().contains("robustness"));
}
#[test]
fn report_tampering_is_detected_and_equal_reservations_are_idempotent() {
    let dir = dir();
    let path = dir.join("state.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    let input = input();
    let p = plan();
    let result = science::execute(
        &mut registry,
        &input,
        &p,
        "fixed-code",
        &science::Budget {
            feature_cells: 100000,
            ..Default::default()
        },
    )
    .unwrap();
    registry
        .reserve_instrument_holdout(
            &input.dataset.manifest.content_hash,
            &input.dataset.profile.symbol,
            &p.holdout,
            &result.report.protocol_id,
            &[p.candidates[0].identity().unwrap()],
        )
        .unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    // Historical reports omit fields added later; their exact typed hash must remain readable.
    let mut legacy = result.report.clone();
    for candidate in &mut legacy.candidates {
        candidate.uncertainty = None;
        candidate.breakdown = None;
        candidate.robustness.clear();
    }
    let json = serde_json::to_string(&legacy).unwrap();
    assert!(!json.contains("\"uncertainty\":"));
    assert!(!json.contains("\"robustness\":"));
    conn.execute(
        "UPDATE scientific_protocols SET report_json=?1,report_hash=?2",
        rusqlite::params![json, data::hash(&legacy).unwrap()],
    )
    .unwrap();
    assert!(
        science::read_report(&registry, &result.report.protocol_id)
            .unwrap()
            .is_some()
    );
    conn.execute("UPDATE scientific_protocols SET report_json='{}'", [])
        .unwrap();
    assert!(science::read_report(&registry, &result.report.protocol_id).is_err());
    assert!(registry.integrity().is_err());
    drop(conn);
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn identical_numeric_histories_on_different_instruments_have_independent_holdouts() {
    let dir = dir();
    let mut registry = Registry::open(&dir.join("state.sqlite")).unwrap();
    let a = input();
    let dataset = data::dataset(
        Profile::synthetic("GBPUSD").unwrap(),
        a.dataset.ticks.clone(),
        &a.dataset.manifest.source,
        a.dataset.manifest.archive_hash.clone(),
        true,
    )
    .unwrap();
    let b = Input {
        dataset,
        conversions: a.conversions.clone(),
        economic_calendar: None,
    };
    assert_eq!(
        a.dataset.manifest.content_hash,
        b.dataset.manifest.content_hash
    );
    let budget = science::Budget {
        feature_cells: 100000,
        ..Default::default()
    };
    let p = plan();
    let eur = science::execute(&mut registry, &a, &p, "code", &budget).unwrap();
    let gbp = science::execute(&mut registry, &b, &p, "code", &budget).unwrap();
    assert_ne!(eur.report.protocol_id, gbp.report.protocol_id);
    assert_eq!(registry.counts().unwrap().registered, 32);
    registry.integrity().unwrap();
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn incomplete_protocol_cannot_spend_real_protected_final_data() {
    let dir = dir();
    let path = dir.join("state.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    let mut input = input();
    input.dataset.manifest.class = rust_strategy_factory::data::DataClass::ResearchEligible;
    let error = science::execute(
        &mut registry,
        &input,
        &plan(),
        "fixed-code",
        &science::Budget::default(),
    )
    .unwrap_err();
    assert!(error.contains("real protected-final execution blocked"));
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM scientific_protocols", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM instrument_holdouts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    drop(conn);
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn frozen_protocol_replays_actual_higher_timeframe_candidates() {
    let dir = dir();
    let mut registry = Registry::open(&dir.join("state.sqlite")).unwrap();
    let input = input();
    let mut plan = plan();
    plan.candidates[0].rule = Rule::All(vec![
        Rule::Rising { lookback: 1 },
        Rule::HigherTimeframe {
            timeframe: Timeframe::M5,
            rule: Box::new(Rule::Rising { lookback: 1 }),
        },
    ]);
    let result = science::execute(
        &mut registry,
        &input,
        &plan,
        "higher-context-version",
        &science::Budget::default(),
    )
    .unwrap();
    assert_eq!(result.report.status, "INSUFFICIENT_EVIDENCE");
    assert!(
        result.report.candidates[0]
            .pooled_oos_reset_fold_stats
            .closed
            > 0
    );
    assert_eq!(result.report.candidates[0].failed_windows, 0);
    registry.integrity().unwrap();
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn frozen_protocol_executes_atr_stops_and_includes_all_policy_probes_in_trial_budget() {
    let dir = dir();
    let mut registry = Registry::open(&dir.join("state.sqlite")).unwrap();
    let input = input();
    let mut plan = plan();
    plan.candidates[0].stop_policy = Some(rust_strategy_factory::stops::Policy::Atr {
        window: 2,
        multiple_bps: 15000,
    });
    let expected = rust_strategy_factory::robustness::cases(&plan.candidates[0], &plan.costs)
        .unwrap()
        .len();
    let mut insufficient = plan.clone();
    insufficient.trial_budget = expected as u64;
    assert!(insufficient.canonical().is_err());
    let report = science::execute(
        &mut registry,
        &input,
        &plan,
        "atr-policy-version",
        &science::Budget::default(),
    )
    .unwrap()
    .report;
    assert_eq!(report.status, "INSUFFICIENT_EVIDENCE");
    assert_eq!(report.candidates[0].robustness.len(), expected);
    assert_eq!(report.candidates[0].failed_windows, 0);
    assert!(report.candidates[0].pooled_oos_reset_fold_stats.closed > 0);
    registry.integrity().unwrap();
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn frozen_time_blocks_bind_protocol_identity_and_preserve_conditional_status() {
    let directory = dir();
    let path = directory.join("blocks.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    let input = input();
    let mut p = plan();
    assert!(
        !serde_json::to_string(&p)
            .unwrap()
            .contains("uncertainty_block_ms")
    );
    p.uncertainty_block_ms = Some(86_400_000);
    let budget = science::Budget {
        feature_cells: 100000,
        ..Default::default()
    };
    let result = science::execute(&mut registry, &input, &p, "block-build", &budget).unwrap();
    let candidate = &result.report.candidates[0];
    let block = candidate.block_uncertainty.as_ref().unwrap();
    let scalar = candidate.uncertainty.as_ref().unwrap();
    assert_eq!(block.observations, scalar.observations);
    assert_eq!(block.net_wins, scalar.net_wins);
    assert!(block.observations > 0);
    assert!(!block.assumptions_verified);
    assert!(block.conditional_lower_net_win_rate <= scalar.conditional_lower_net_win_rate);
    assert_eq!(result.report.status, "INSUFFICIENT_EVIDENCE");
    drop(registry);
    let mut registry = Registry::open(&path).unwrap();
    let again = science::execute(&mut registry, &input, &p, "block-build", &budget).unwrap();
    assert!(again.reused);
    assert_eq!(
        again.report.candidates[0].block_uncertainty,
        Some(block.clone())
    );
    p.uncertainty_block_ms = Some(7 * 86_400_000);
    assert!(science::execute(&mut registry, &input, &p, "block-build", &budget).is_err());
    p.uncertainty_block_ms = None;
    assert!(science::execute(&mut registry, &input, &p, "block-build", &budget).is_err());
    p.uncertainty_block_ms = Some(0);
    assert!(p.canonical().is_err());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn robustness_policy_is_frozen_before_oos_and_survives_reopen() {
    let directory = dir();
    let path = directory.join("policy.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    let input = input();
    let mut p = plan();
    p.robustness_policy = Some(rust_strategy_factory::robustness::ProbePolicy {
        minimum_resolved_per_probe: 1,
        minimum_net_win_rate_bps: 5000,
        minimum_sum_net_micro_usd: 0,
        maximum_consecutive_losses: 5,
    });
    let budget = science::Budget {
        feature_cells: 100000,
        ..Default::default()
    };
    let first = science::execute(&mut registry, &input, &p, "policy-build", &budget).unwrap();
    let assessment = first.report.candidates[0]
        .robustness_assessment
        .clone()
        .unwrap();
    assert!(!assessment.qualification_granted);
    assert_eq!(
        assessment.cases.len(),
        first.report.candidates[0].robustness.len()
    );
    assert_eq!(first.report.status, "INSUFFICIENT_EVIDENCE");
    drop(registry);
    let mut registry = Registry::open(&path).unwrap();
    let again = science::execute(&mut registry, &input, &p, "policy-build", &budget).unwrap();
    assert!(again.reused);
    assert_eq!(
        again.report.candidates[0].robustness_assessment,
        Some(assessment)
    );
    p.robustness_policy
        .as_mut()
        .unwrap()
        .minimum_resolved_per_probe = 2;
    assert!(science::execute(&mut registry, &input, &p, "policy-build", &budget).is_err());
    p.robustness_policy = None;
    assert!(science::execute(&mut registry, &input, &p, "policy-build", &budget).is_err());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn frozen_conditional_win_policy_binds_actual_oos_receipts_reopen_and_prevents_retuning() {
    use rust_strategy_factory::{tick_engine::Trade, validation::Observation, win_evidence};
    let directory = dir();
    let path = directory.join("win-evidence.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    let input = input();
    let mut p = plan();
    p.win_evidence_policy = Some(win_evidence::Policy {
        stake_basis_points: 1000,
        family_error_basis_points: 500,
        minimum_resolved: 100,
    });
    let budget = science::Budget {
        feature_cells: 100000,
        ..Default::default()
    };
    let first = science::execute(&mut registry, &input, &p, "win-evidence-build", &budget).unwrap();
    let candidate = &first.report.candidates[0];
    let diagnostic = candidate.win_evidence.as_ref().unwrap();
    assert_eq!(
        diagnostic.observations,
        candidate.uncertainty.as_ref().unwrap().observations
    );
    assert_eq!(
        diagnostic.net_wins,
        candidate.pooled_oos_reset_fold_stats.wins
    );
    assert!(!diagnostic.qualification_granted);
    assert_eq!(first.report.status, "INSUFFICIENT_EVIDENCE");
    let mut observations = Vec::new();
    for evidence in candidate
        .experiments
        .iter()
        .filter(|e| e.phase.ends_with("out-of-sample"))
    {
        let result = registry.result(&evidence.experiment_id).unwrap().unwrap();
        for raw in result.result["tick_replay"]["trades"].as_array().unwrap() {
            let t: Trade = serde_json::from_value(raw.clone()).unwrap();
            observations.push(Observation {
                exit_ms: t.exit_ms,
                net_micro_usd: t.net_micro_usd,
            });
        }
    }
    assert_eq!(
        win_evidence::diagnose(
            &observations,
            p.trial_budget,
            p.win_evidence_policy.as_ref().unwrap()
        )
        .unwrap(),
        *diagnostic
    );
    drop(registry);
    let mut registry = Registry::open(&path).unwrap();
    let again = science::execute(&mut registry, &input, &p, "win-evidence-build", &budget).unwrap();
    assert!(again.reused);
    assert_eq!(
        again.report.candidates[0].win_evidence,
        Some(diagnostic.clone())
    );
    p.win_evidence_policy.as_mut().unwrap().stake_basis_points = 2000;
    assert!(science::execute(&mut registry, &input, &p, "win-evidence-build", &budget).is_err());
    p.win_evidence_policy = None;
    assert!(science::execute(&mut registry, &input, &p, "win-evidence-build", &budget).is_err());
    registry.integrity().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn absent_win_policy_preserves_legacy_shapes_and_invalid_policy_creates_no_evidence() {
    let directory = dir();
    let mut registry = Registry::open(&directory.join("legacy-win.sqlite")).unwrap();
    let input = input();
    let mut p = plan();
    assert!(!serde_json::to_string(&p).unwrap().contains("win_evidence"));
    p.win_evidence_policy = Some(rust_strategy_factory::win_evidence::Policy {
        stake_basis_points: 0,
        family_error_basis_points: 500,
        minimum_resolved: 100,
    });
    assert!(
        science::execute(
            &mut registry,
            &input,
            &p,
            "invalid-win",
            &science::Budget::default()
        )
        .is_err()
    );
    assert_eq!(registry.counts().unwrap().registered, 0);
    p.win_evidence_policy = None;
    let result = science::execute(
        &mut registry,
        &input,
        &p,
        "legacy-win",
        &science::Budget {
            feature_cells: 100000,
            ..Default::default()
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&result.report).unwrap();
    assert!(!encoded.contains("win_evidence"));
    let decoded: science::Report = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        data::hash(&decoded).unwrap(),
        data::hash(&result.report).unwrap()
    );
    assert!(decoded.candidates[0].win_evidence.is_none());
    registry.integrity().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn frozen_oos_evidence_gates_hold_real_receipt_shortfalls_and_preserve_reopen_policy() {
    let directory = dir();
    let path = directory.join("gates.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    let input = input();
    let mut p = plan();
    p.evidence_gate_policy = Some(rust_strategy_factory::evidence_gates::Policy {
        minimum_folds: 2,
        minimum_resolved_per_fold: 20,
        minimum_resolved_per_year: 50,
        minimum_net_win_rate_bps: 5000,
        maximum_consecutive_losses: 5,
        minimum_net_micro_usd: 1,
    });
    assert!(p.canonical().is_err());
    p.win_evidence_policy = Some(rust_strategy_factory::win_evidence::Policy {
        stake_basis_points: 1000,
        family_error_basis_points: 500,
        minimum_resolved: 100,
    });
    p.robustness_policy = Some(rust_strategy_factory::robustness::ProbePolicy {
        minimum_resolved_per_probe: 20,
        minimum_net_win_rate_bps: 5000,
        minimum_sum_net_micro_usd: 1,
        maximum_consecutive_losses: 5,
    });
    let budget = science::Budget {
        feature_cells: 100000,
        ..Default::default()
    };
    let first = science::execute(&mut registry, &input, &p, "gate-build", &budget).unwrap();
    let gate = first.report.candidates[0].evidence_gate.as_ref().unwrap();
    assert_eq!(gate.state, "HELD");
    assert!(!gate.criteria_passed);
    assert!(!gate.qualification_granted);
    assert!(
        gate.decisions
            .iter()
            .any(|d| d.criterion == "year-2026-samples-net-and-win-rate" && !d.passed)
    );
    assert_eq!(first.report.status, "INSUFFICIENT_EVIDENCE");
    drop(registry);
    let mut registry = Registry::open(&path).unwrap();
    let again = science::execute(&mut registry, &input, &p, "gate-build", &budget).unwrap();
    assert!(again.reused);
    assert_eq!(again.report.candidates[0].evidence_gate, Some(gate.clone()));
    p.evidence_gate_policy
        .as_mut()
        .unwrap()
        .minimum_resolved_per_year = 51;
    assert!(science::execute(&mut registry, &input, &p, "gate-build", &budget).is_err());
    registry.integrity().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn scientific_summary_preserves_full_width_counts_and_net_without_qualification() {
    let directory = dir();
    let mut registry = Registry::open(&directory.join("summary.sqlite")).unwrap();
    let mut p = plan();
    p.trial_budget = u64::MAX;
    let mut report = science::execute(
        &mut registry,
        &input(),
        &p,
        "summary-build",
        &science::Budget {
            feature_cells: 100000,
            ..Default::default()
        },
    )
    .unwrap()
    .report;
    report.candidates[0]
        .pooled_oos_reset_fold_stats
        .sum_net_micro_usd = i128::MIN;
    let summary = science::summary(&report).unwrap();
    assert_eq!(summary["declared_trials"], u64::MAX.to_string());
    assert_eq!(
        summary["candidates"][0]["reset_fold_net_micro_usd"],
        i128::MIN.to_string()
    );
    assert_eq!(summary["report_hash"], data::hash(&report).unwrap());
    assert_eq!(summary["qualification_granted"], false);
    assert!(summary["candidates"][0]["evidence_gate"].is_null());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn real_structurally_valid_data_enters_frozen_protocol_pipeline() {
    let directory = dir();
    let path = directory.join("real-data.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    let mut input = input();
    input.dataset.manifest.class = data::DataClass::RealStructurallyValid;
    input.dataset.manifest.archive_hash = "real-archive-v1".into();
    input.dataset.manifest.content_hash = data::hash(&input.dataset.ticks).unwrap();
    input.dataset.verify().unwrap();
    assert_eq!(
        input.dataset.manifest.class,
        data::DataClass::RealStructurallyValid
    );

    let p = plan();
    let budget = science::Budget {
        feature_cells: 100000,
        ..Default::default()
    };
    let result =
        science::execute(&mut registry, &input, &p, "real-data-revision", &budget).unwrap();
    assert!(!result.reused);
    assert_eq!(result.report.status, "INSUFFICIENT_EVIDENCE");
    assert_eq!(
        result.report.data_class,
        data::DataClass::RealStructurallyValid
    );
    assert_eq!(result.report.candidates[0].experiments.len(), 6);
    assert!(
        result.report.candidates[0]
            .pooled_oos_reset_fold_stats
            .closed
            > 0
    );
    assert_eq!(result.report.candidates[0].failed_windows, 0);
    assert_eq!(result.report.candidates[0].robustness.len(), 5);
    // Numerical execution alone never qualifies; the report states what is still missing.
    assert!(!result.report.missing_requirements.is_empty());
    // Reproducibility: identical re-execution reuses the immutable frozen report.
    let before = registry.counts().unwrap();
    let again = science::execute(&mut registry, &input, &p, "real-data-revision", &budget).unwrap();
    assert!(again.reused);
    assert_eq!(result.report.protocol_id, again.report.protocol_id);
    assert_eq!(before.attempts, registry.counts().unwrap().attempts);
    registry.integrity().unwrap();
    drop(registry);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn real_data_below_the_structural_bar_is_still_refused() {
    let directory = dir();
    let path = directory.join("real-refused.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    let mut input = input();
    input.dataset.manifest.class = data::DataClass::RealUnreviewed;
    input.dataset.verify().unwrap();
    let error = science::execute(
        &mut registry,
        &input,
        &plan(),
        "fixed-code",
        &science::Budget::default(),
    )
    .unwrap_err();
    assert!(error.contains("real protected-final execution blocked"));
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM scientific_protocols", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM instrument_holdouts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    drop(conn);
    drop(registry);
    std::fs::remove_dir_all(directory).unwrap();
}
