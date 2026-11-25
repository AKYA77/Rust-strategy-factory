//! Milestone 3: durable crash-safe stage control and bounded telemetry.
use rust_strategy_factory::{
    stage_machine,
    storage::{ExperimentKey, Registry, Stage},
    telemetry,
};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn path(tag: &str) -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("rsf-stage-{tag}-{}-{n}.sqlite", std::process::id()))
}

fn key(stage: Stage, dataset: &str, protocol: &str, start: i64, end: i64) -> ExperimentKey {
    ExperimentKey {
        concept_hash: "a".repeat(64),
        instrument: "EURUSD".into(),
        dataset_hash: dataset.into(),
        start_ms: start,
        end_exclusive_ms: end,
        signal_version: "signal-v1".into(),
        indicator_version: "indicator-v1".into(),
        cost_hash: "b".repeat(64),
        fill_policy: "market-at-open".into(),
        profile_hash: "c".repeat(64),
        engine_version: "engine-v1".into(),
        protocol_hash: protocol.into(),
        code_revision: "rev-v1".into(),
        configuration_hash: "d".repeat(64),
        stage,
    }
}

#[test]
fn stage_advances_durably_and_resumes_from_lineage() {
    let db = path("advance");
    let mut registry = Registry::open(&db).unwrap();
    // Candidates are registered at the exact-replay entry stage.
    let k = key(
        Stage::TickVerification,
        "dataset-a",
        "protocol-a",
        1000,
        2000,
    );
    let id = registry.register(&k).unwrap();

    // First advance records the CandleScreen -> TickVerification entry edge.
    let first = stage_machine::advance(&mut registry, &k, 1_000).unwrap();
    assert_eq!(first, Some(Stage::TickVerification));
    assert_eq!(stage_machine::history(&registry, &id).unwrap().len(), 1);
    stage_machine::validate(&registry, &id).unwrap();

    // A fresh handle sees the same durable state: no in-memory resume.
    let mut reopened = Registry::open(&db).unwrap();
    assert_eq!(stage_machine::history(&reopened, &id).unwrap().len(), 1);
    // Independent Validation is protected and cannot be entered without a
    // pre-reserved concept/dataset/window/protocol holdout.
    assert!(stage_machine::advance(&mut reopened, &k, 2_000).is_err());
    assert_eq!(stage_machine::history(&reopened, &id).unwrap().len(), 1);

    // Bounded telemetry replay for the experiment (entry event from research).
    let events = telemetry::page(&reopened, &id, 0).unwrap();
    assert!(!events.is_empty());
    assert!(events.windows(2).all(|w| w[0].id < w[1].id));
    assert_eq!(
        telemetry::count(&reopened, &id).unwrap(),
        events.len() as i64
    );
    assert_eq!(
        telemetry::latest_id(&reopened, &id).unwrap(),
        events.last().unwrap().id
    );
}

#[test]
fn reserved_holdout_authorizes_protected_stage() {
    let db = path("holdout");
    let mut registry = Registry::open(&db).unwrap();
    let k = key(
        Stage::TickVerification,
        "dataset-a",
        "protocol-a",
        1000,
        2000,
    );
    let id = registry.register(&k).unwrap();

    // Insert the pre-reserved holdout directly to isolate the stage machine's
    // own protected-stage gate from registration policy.
    {
        let connection = rusqlite::Connection::open(&db).unwrap();
        connection
            .execute(
                "INSERT INTO instrument_holdouts(dataset_hash,instrument,start_ms,end_ms,protocol_hash,selected_json)
                 VALUES ('dataset-a','EURUSD',1000,2000,'protocol-a','[\"ORB-NY\"]')",
                [],
            )
            .unwrap();
    }

    let first = stage_machine::advance(&mut registry, &k, 1_000).unwrap();
    assert_eq!(first, Some(Stage::TickVerification));
    let second = stage_machine::advance(&mut registry, &k, 2_000).unwrap();
    assert_eq!(second, Some(Stage::IndependentValidation));
    assert_eq!(stage_machine::history(&registry, &id).unwrap().len(), 2);
    stage_machine::validate(&registry, &id).unwrap();

    // Terminal stage: no further transitions.
    assert_eq!(
        stage_machine::advance(&mut registry, &k, 3_000).unwrap(),
        None
    );
    assert_eq!(stage_machine::history(&registry, &id).unwrap().len(), 2);
}

#[test]
fn stage_machine_refuses_tampered_lineage() {
    let db = path("tamper");
    let mut registry = Registry::open(&db).unwrap();
    let k = key(
        Stage::TickVerification,
        "dataset-b",
        "protocol-b",
        3000,
        4000,
    );
    let id = registry.register(&k).unwrap();

    // Insert an illegal jump CandleScreen -> IndependentValidation directly.
    {
        let connection = rusqlite::Connection::open(&db).unwrap();
        connection
            .execute(
                "INSERT INTO stage_transitions(experiment_id,sequence,from_stage,to_stage,at_ms,decision_hash)
                 VALUES (?1,1,'CandleScreen','IndependentValidation',1000,?2)",
                rusqlite::params![id, "a".repeat(64)],
            )
            .unwrap();
    }
    assert!(stage_machine::validate(&registry, &id).is_err());
    assert!(stage_machine::advance(&mut registry, &k, 2_000).is_err());
}

#[test]
fn stage_lineage_and_telemetry_are_immutable() {
    let db = path("immutable");
    let mut registry = Registry::open(&db).unwrap();
    let k = key(
        Stage::TickVerification,
        "dataset-c",
        "protocol-c",
        5000,
        6000,
    );
    let id = registry.register(&k).unwrap();
    stage_machine::advance(&mut registry, &k, 1_000).unwrap();
    telemetry::record(
        &registry,
        &id,
        "test",
        "stage-entered",
        1_000,
        &serde_json::json!({"stage": "TickVerification"}),
    )
    .unwrap();
    assert_eq!(telemetry::count(&registry, &id).unwrap(), 2);

    let connection = rusqlite::Connection::open(&db).unwrap();
    assert!(connection
        .execute(
            "UPDATE stage_transitions SET to_stage='IndependentValidation' WHERE experiment_id=?1",
            [&id]
        )
        .is_err());
    assert!(
        connection
            .execute(
                "DELETE FROM stage_transitions WHERE experiment_id=?1",
                [&id]
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE telemetry_events SET payload='{}' WHERE experiment_id=?1",
                [&id]
            )
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM telemetry_events WHERE experiment_id=?1", [&id])
            .is_err()
    );
}

#[test]
fn record_entry_advances_only_after_a_passing_stage1_gate() {
    let db = path("entry-gate");
    let mut registry = Registry::open(&db).unwrap();

    // Rejected candidate cannot continue: no CandleScreen -> TickVerification edge.
    let k_reject = key(Stage::CandleScreen, "ds-reject", "protocol-a", 1000, 2000);
    let id_reject = registry.register(&k_reject).unwrap();
    stage_machine::record_entry(&registry, &k_reject, false, "reject@39bps").unwrap();
    assert!(
        stage_machine::history(&registry, &id_reject)
            .unwrap()
            .is_empty()
    );

    // Held (uncertain) candidate is not mislabeled as passed: no edge either.
    let k_hold = key(Stage::CandleScreen, "ds-hold", "protocol-a", 1000, 2000);
    let id_hold = registry.register(&k_hold).unwrap();
    stage_machine::record_entry(&registry, &k_hold, false, "hold@40bps").unwrap();
    assert!(
        stage_machine::history(&registry, &id_hold)
            .unwrap()
            .is_empty()
    );

    // Accepted candidate advances, and the hash binds the Stage-1 gate evidence.
    let k_pass = key(Stage::CandleScreen, "ds-pass", "protocol-a", 1000, 2000);
    let id_pass = registry.register(&k_pass).unwrap();
    stage_machine::record_entry(&registry, &k_pass, true, "pass@40bps").unwrap();
    let history = stage_machine::history(&registry, &id_pass).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].from_stage, Stage::CandleScreen);
    assert_eq!(history[0].to_stage, Stage::TickVerification);
    let bound = stage_machine::entry_decision_hash(
        &k_pass,
        &Stage::CandleScreen,
        &Stage::TickVerification,
        "pass@40bps",
    )
    .unwrap();
    let other = stage_machine::entry_decision_hash(
        &k_pass,
        &Stage::CandleScreen,
        &Stage::TickVerification,
        "pass@39bps",
    )
    .unwrap();
    assert_eq!(history[0].decision_hash, bound);
    assert_ne!(bound, other, "hash must depend on the gate evidence");
}
