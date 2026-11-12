use rust_strategy_factory::{
    contracts::{Conversion, Profile},
    data::{self, Tick},
    research::{self, Config, Driver, Input},
    storage::Registry,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn dir() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "factory-research-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&p).unwrap();
    p
}
fn input(symbol: &str) -> Input {
    let ticks = (0..1000)
        .map(|i| {
            let phase = i % 80;
            let wave = if phase < 40 { phase } else { 80 - phase };
            let bid = 100000 + wave * 50;
            Tick {
                timestamp_ms: 1735689600000 + i * 10000,
                sequence: i as u64,
                bid,
                ask: bid + 20,
            }
        })
        .collect::<Vec<_>>();
    let n = ticks.len();
    Input {
        dataset: data::dataset(
            Profile::synthetic(symbol).unwrap(),
            ticks,
            "SYNTHETIC_TEST_ONLY loop fixture",
            "generated".into(),
            true,
        )
        .unwrap(),
        conversions: vec![Conversion::usd(); n],
    }
}
fn open(path: &std::path::Path) -> Driver {
    Driver::open(
        path,
        vec![input("EURUSD"), input("GBPUSD")],
        Config {
            seed: 10,
            ..Config::default()
        },
        "fixed-test-revision".into(),
    )
    .unwrap()
}
#[test]
fn pause_resume_stop_and_cursor_survive_restart() {
    let d = dir();
    let path = d.join("state.sqlite");
    let id;
    {
        let mut driver = open(&path);
        id = driver.id.clone();
        driver.acquire(0).unwrap();
        assert!(driver.step(1).unwrap());
        research::control(&driver.registry, &id, "paused").unwrap();
        assert!(!driver.step(2).unwrap());
        assert_eq!(
            research::status(&driver.registry, &id, 3).unwrap().cursor,
            1
        );
    }
    let mut driver = open(&path);
    assert_eq!(id, driver.id);
    driver.acquire(4).unwrap();
    assert!(!driver.step(5).unwrap());
    research::control(&driver.registry, &id, "running").unwrap();
    assert!(driver.step(6).unwrap());
    assert_eq!(
        research::status(&driver.registry, &id, 7).unwrap().cursor,
        2
    );
    research::control(&driver.registry, &id, "stopped").unwrap();
    assert!(!driver.step(8).unwrap());
    driver.registry.integrity().unwrap();
    drop(driver);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn coordinators_are_exclusive_and_stale_owner_cannot_advance_cursor() {
    let d = dir();
    let path = d.join("state.sqlite");
    let mut a = open(&path);
    let mut b = open(&path);
    a.acquire(0).unwrap();
    assert!(b.acquire(1).is_err());
    b.acquire(300000).unwrap();
    assert!(a.step(300001).is_err());
    assert!(b.step(300001).unwrap());
    drop(a);
    assert!(b.step(300002).unwrap());
    drop(b);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn completed_experiment_before_cursor_commit_is_reused_without_new_simulation() {
    let d = dir();
    let path = d.join("state.sqlite");
    let mut driver = open(&path);
    let (_, s) = research::candidate_at(0, 10).unwrap();
    let completed = research::evaluate(
        &mut driver.registry,
        &input("EURUSD"),
        &s,
        &Default::default(),
        "fixed-test-revision",
        1_000_000,
    )
    .unwrap();
    assert!(!completed.reused);
    assert_eq!(completed.outcome, "completed");
    assert!(
        !completed.result["tick_replay"]["trades"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let before = driver.registry.counts().unwrap();
    driver.acquire(0).unwrap();
    driver.step(1).unwrap();
    let status = research::status(&driver.registry, &driver.id, 2).unwrap();
    assert_eq!((status.cursor, status.reused, status.executed), (1, 1, 0));
    let after = driver.registry.counts().unwrap();
    assert_eq!(
        (before.completed, before.attempts),
        (after.completed, after.attempts)
    );
    drop(driver);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn bounds_reject_invalid_configuration_and_new_regions_are_not_repeats() {
    let d = dir();
    let path = d.join("state.sqlite");
    assert!(
        Driver::open(
            &path,
            vec![input("EURUSD")],
            Config {
                max_total_events: 10,
                ..Config::default()
            },
            "test".into()
        )
        .is_err()
    );
    assert!(
        Driver::open(
            &path,
            vec![input("EURUSD")],
            Config {
                max_regions: 0,
                ..Config::default()
            },
            "test".into()
        )
        .is_err()
    );
    let count = rust_strategy_factory::strategies::family_templates().len() as u64;
    let mut ids = std::collections::BTreeSet::new();
    for region in [0, 1, 2, 299, 300, 301, 9999] {
        for slot in 0..count {
            let (_, s) = research::candidate_at(region * count + slot, 42).unwrap();
            assert!(ids.insert(s.identity().unwrap()));
        }
    }
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn backup_restores_cursor_and_controls_with_fenced_lease_recovery() {
    let d = dir();
    let path = d.join("state.sqlite");
    let backup = d.join("backup.sqlite");
    let mut driver = open(&path);
    driver.acquire(0).unwrap();
    driver.step(1).unwrap();
    driver.release().unwrap();
    driver.registry.backup(&backup).unwrap();
    let mut restored = open(&backup);
    assert_eq!(driver.id, restored.id);
    restored.acquire(10).unwrap();
    assert_eq!(
        research::status(&restored.registry, &restored.id, 11)
            .unwrap()
            .cursor,
        1
    );
    restored.step(12).unwrap();
    Registry::open(&backup).unwrap().integrity().unwrap();
    drop(restored);
    drop(driver);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn state_pressure_pauses_before_new_experiment_and_preserves_cursor() {
    let d = dir();
    let path = d.join("state.sqlite");
    let mut driver = Driver::open(
        &path,
        vec![input("EURUSD")],
        Config {
            state_budget_bytes: 32 * 1024 * 1024,
            ..Config::default()
        },
        "test".into(),
    )
    .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TABLE pressure(payload BLOB); INSERT INTO pressure VALUES (zeroblob(20971520));").unwrap();
    driver.acquire(0).unwrap();
    assert!(!driver.step(1).unwrap());
    let status = research::status(&driver.registry, &driver.id, 2).unwrap();
    assert_eq!(status.state, "paused");
    assert_eq!(status.cursor, 0);
    assert!(!status.active);
    assert!(status.last_error.unwrap().contains("budget"));
    assert_eq!(driver.registry.counts().unwrap().registered, 0);
    drop(connection);
    drop(driver);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn dataset_provenance_changes_identity_but_reimport_time_does_not() {
    let d = dir();
    let mut registry = Registry::open(&d.join("state.sqlite")).unwrap();
    let (_, s) = research::candidate_at(0, 10).unwrap();
    let a = input("EURUSD");
    let first =
        research::evaluate(&mut registry, &a, &s, &Default::default(), "code", 100000).unwrap();
    let mut same = input("EURUSD");
    same.dataset.manifest.imported_at = "later re-import of same archive".into();
    let cached = research::evaluate(
        &mut registry,
        &same,
        &s,
        &Default::default(),
        "code",
        100000,
    )
    .unwrap();
    assert!(cached.reused);
    assert_eq!(first.experiment_id, cached.experiment_id);
    same.dataset.manifest.source = "different source provenance".into();
    let changed = research::evaluate(
        &mut registry,
        &same,
        &s,
        &Default::default(),
        "code",
        100000,
    )
    .unwrap();
    assert!(!changed.reused);
    assert_ne!(first.experiment_id, changed.experiment_id);
    drop(registry);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn real_data_loop_requires_explicit_chronological_training_scope() {
    let d = dir();
    let path = d.join("state.sqlite");
    let mut real = input("EURUSD");
    real.dataset.manifest.class = data::DataClass::RealStructurallyValid;
    assert!(Driver::open(&path, vec![real], Config::default(), "code".into()).is_err());
    std::fs::remove_dir_all(d).unwrap();
}
