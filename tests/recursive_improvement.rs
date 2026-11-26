use rust_strategy_factory::{
    contracts::{Conversion, Profile},
    data::{self, Tick},
    research::{self, Config, Input},
    storage::Registry,
    training_review,
};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "factory-recursive-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).unwrap();
    path
}

fn input() -> Input {
    let ticks = [
        (0, 100_000),
        (60_000, 100_020),
        (120_000, 100_040),
        (120_000, 100_050),
        (121_000, 100_300),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (offset, bid))| Tick {
        timestamp_ms: 1_735_689_600_000 + offset,
        sequence: index as u64,
        bid,
        ask: bid + 10,
    })
    .collect();
    Input {
        dataset: data::dataset(
            Profile::synthetic("EURUSD").unwrap(),
            ticks,
            "synthetic recursive improvement fixture",
            "generated".into(),
            true,
        )
        .unwrap(),
        conversions: vec![Conversion::usd(); 5],
        economic_calendar: None,
    }
}

fn driver(path: &Path, children: usize, depth: usize) -> research::Driver {
    research::Driver::open(
        path,
        vec![input()],
        Config {
            seed: 0,
            feature_cells: 10_000,
            improvement_children: children,
            improvement_max_depth: depth,
            ..Default::default()
        },
        research::compute_revision().unwrap(),
    )
    .unwrap()
}

/// Advance durable work until `target` improvement jobs have completed.
fn drive_until(driver: &mut research::Driver, target: i64) {
    let mut now = 1_i64;
    loop {
        let status = research::status(&driver.registry, &driver.id, now).unwrap();
        if status.improvements.completed >= target {
            break;
        }
        assert!(now < 1_000_000, "recursive improvement did not converge");
        assert!(driver.step(now).unwrap(), "driver paused at {now}");
        now += 1;
    }
}

#[test]
fn controlled_recursion_advances_depths_and_stops_at_cap() {
    let d = dir();
    let path = d.join("state.sqlite");
    let mut driver = driver(&path, 4, 2);
    driver.acquire(0).unwrap();
    assert!(driver.step(1).unwrap());
    let jobs = research::improvement_jobs(&driver.registry, &driver.id).unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].depth, 0);
    assert!(jobs[0].accepted_edge_id.is_none());

    drive_until(&mut driver, 1);
    let status = research::status(&driver.registry, &driver.id, 2).unwrap();
    assert_eq!(status.improvements.completed, 1);
    assert_eq!(status.improvements.max_depth, 1);
    assert_eq!(status.improvements.accepted_descendants, 1);
    assert_eq!(status.improvements.queued, 1);
    let jobs = research::improvement_jobs(&driver.registry, &driver.id).unwrap();
    assert_eq!(jobs.len(), 2);
    let parent = jobs.iter().find(|job| job.depth == 0).unwrap();
    let child = jobs.iter().find(|job| job.depth == 1).unwrap();
    let accepted_edge = child
        .accepted_edge_id
        .clone()
        .expect("descendant must record its accepted proposal edge");
    let report = training_review::read_report(
        &driver.registry,
        parent.review_id.as_ref().expect("parent review"),
    )
    .unwrap()
    .unwrap();
    assert!(
        report
            .children
            .iter()
            .any(|child| child.proposal_edge_id == accepted_edge)
    );
    // Selection is the seeded rank over resolved children only; it does not
    // read the net delta or any protected/OOS evidence.
    let mut ranked: Vec<(String, String)> = report
        .children
        .iter()
        .filter(|child| child.evidence.resolved_net_micro_usd.is_some())
        .map(|child| {
            (
                data::hash(&(0_u64, &child.proposal_edge_id)).unwrap(),
                child.proposal_edge_id.clone(),
            )
        })
        .collect();
    ranked.sort();
    assert_eq!(ranked[0].1, accepted_edge);

    drive_until(&mut driver, 3);
    let status = research::status(&driver.registry, &driver.id, 2).unwrap();
    assert_eq!(status.improvements.completed, 3);
    assert_eq!(status.improvements.queued, 0);
    assert_eq!(status.improvements.max_depth, 2);
    assert_eq!(status.improvements.accepted_descendants, 2);
    let jobs = research::improvement_jobs(&driver.registry, &driver.id).unwrap();
    assert_eq!(jobs.len(), 3);
    assert!(jobs.iter().all(|job| job.depth <= 2));
    driver.registry.integrity().unwrap();
    drop(driver);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn depth_zero_preserves_the_historical_no_descendant_behaviour() {
    let d = dir();
    let path = d.join("state.sqlite");
    let mut driver = driver(&path, 2, 0);
    driver.acquire(0).unwrap();
    assert!(driver.step(1).unwrap());
    drive_until(&mut driver, 1);
    let status = research::status(&driver.registry, &driver.id, 2).unwrap();
    assert_eq!(status.improvements.completed, 1);
    assert_eq!(status.improvements.max_depth, 0);
    assert_eq!(status.improvements.accepted_descendants, 0);
    let jobs = research::improvement_jobs(&driver.registry, &driver.id).unwrap();
    assert_eq!(jobs.len(), 1);
    driver.registry.integrity().unwrap();
    drop(driver);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn recursive_configuration_bounds_are_enforced() {
    let d = dir();
    let revision = research::compute_revision().unwrap();
    assert!(
        research::Driver::open(
            &d.join("too-deep.sqlite"),
            vec![input()],
            Config {
                improvement_children: 1,
                improvement_max_depth: 3,
                ..Default::default()
            },
            revision.clone(),
        )
        .is_err()
    );
    assert!(
        research::Driver::open(
            &d.join("no-children.sqlite"),
            vec![input()],
            Config {
                improvement_children: 0,
                improvement_max_depth: 1,
                ..Default::default()
            },
            revision,
        )
        .is_err()
    );
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn queued_recursive_generation_survives_reopen() {
    let d = dir();
    let path = d.join("state.sqlite");
    {
        let mut driver = driver(&path, 3, 2);
        driver.acquire(0).unwrap();
        assert!(driver.step(1).unwrap());
        drive_until(&mut driver, 1);
        let status = research::status(&driver.registry, &driver.id, 2).unwrap();
        assert_eq!(status.improvements.queued, 1);
        assert_eq!(status.improvements.max_depth, 1);
    }
    let mut driver = driver(&path, 3, 2);
    driver.acquire(10).unwrap();
    drive_until(&mut driver, 3);
    let status = research::status(&driver.registry, &driver.id, 11).unwrap();
    assert_eq!(status.improvements.completed, 3);
    assert_eq!(status.improvements.max_depth, 2);
    assert_eq!(status.improvements.accepted_descendants, 2);
    driver.registry.integrity().unwrap();
    drop(driver);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn schema23_jobs_migrate_additively_with_depth_zero() {
    let d = dir();
    let path = d.join("migrate.sqlite");
    {
        let mut driver = driver(&path, 2, 0);
        driver.acquire(0).unwrap();
        assert!(driver.step(1).unwrap());
        let jobs = research::improvement_jobs(&driver.registry, &driver.id).unwrap();
        assert_eq!(jobs.len(), 1);
    }
    // Replace the v24 table with the historical v9 definition and force the
    // registry back to schema 23; reopening must rebuild it without losing
    // the durable job row.
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "ALTER TABLE improvement_jobs RENAME TO improvement_jobs_v24;
            CREATE TABLE improvement_jobs(
                run_id TEXT NOT NULL REFERENCES research_runs(id), parent_cursor INTEGER NOT NULL,
                input_index INTEGER NOT NULL CHECK(input_index>=0), parent_id TEXT NOT NULL REFERENCES experiments(id),
                state TEXT NOT NULL DEFAULT 'queued' CHECK(state IN ('queued','completed','failed')),
                diagnosis_id TEXT REFERENCES training_diagnoses(id), review_id TEXT REFERENCES training_reviews(id), error TEXT,
                PRIMARY KEY(run_id,parent_cursor), FOREIGN KEY(run_id,parent_cursor) REFERENCES research_steps(run_id,cursor)
            );
            INSERT INTO improvement_jobs SELECT run_id,parent_cursor,input_index,parent_id,state,diagnosis_id,review_id,error FROM improvement_jobs_v24;
            DROP TABLE improvement_jobs_v24;
            PRAGMA user_version=23;",
        )
        .unwrap();
    drop(connection);
    let registry = Registry::open(&path).unwrap();
    registry.integrity().unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    let (depth, state): (i64, String) = connection
        .query_row(
            "SELECT depth,state FROM improvement_jobs WHERE parent_cursor=0",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(depth, 0);
    assert_eq!(state, "queued");
    let columns: String = connection
        .query_row(
            "SELECT group_concat(name) FROM pragma_table_info('improvement_jobs')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(columns.contains("accepted_edge_id"));
    drop(connection);
    drop(registry);
    std::fs::remove_dir_all(d).unwrap();
}
