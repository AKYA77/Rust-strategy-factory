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
    assert_eq!(first.report.candidates[0].robustness.len(), 3);
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
    assert_eq!(registry.counts().unwrap().registered, 24);
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
