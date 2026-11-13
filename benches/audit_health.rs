//! Metadata-only equality lookup. Never a strategy/replay throughput claim.
use rusqlite::params;
use rust_strategy_factory::{
    research,
    storage::{ExperimentKey, Registry, Stage},
};
use serde_json::json;
use std::{fs, time::Instant};
fn main() {
    let directory =
        std::env::temp_dir().join(format!("factory-audit-health-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let mut measurements = Vec::new();
    for rows in [50000, 200000] {
        let path = directory.join(format!("{rows}.sqlite"));
        let mut registry = Registry::open(&path).unwrap();
        let key = ExperimentKey {
            concept_hash: "SYNTHETIC_METADATA_ONLY".into(),
            instrument: "EURUSD".into(),
            dataset_hash: "no-market-data".into(),
            start_ms: 0,
            end_exclusive_ms: 1,
            signal_version: "metadata".into(),
            indicator_version: "metadata".into(),
            cost_hash: "metadata".into(),
            fill_policy: "metadata".into(),
            profile_hash: "metadata".into(),
            engine_version: "metadata".into(),
            protocol_hash: "metadata".into(),
            code_revision: "metadata-fixture".into(),
            configuration_hash: "metadata-fixture".into(),
            stage: Stage::CandleScreen,
        };
        let id = registry.register(&key).unwrap();
        drop(registry);
        let mut c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON;DROP INDEX event_action_lookup;")
            .unwrap();
        {
            let tx = c.transaction().unwrap();
            {
                let mut insert=tx.prepare("INSERT INTO events(experiment_id,at_ms,action,attempt) VALUES (?1,0,'SYNTHETIC_METADATA_FIXTURE',0)").unwrap();
                for _ in 0..rows {
                    insert.execute([&id]).unwrap();
                }
            }
            tx.commit().unwrap();
        }
        for blocked in [false, true] {
            if blocked {
                for action in [
                    "proof-audit-contradiction-v1",
                    "indexed-audit-contradiction-v1",
                ] {
                    c.execute(
                        "INSERT INTO events(experiment_id,at_ms,action,attempt) VALUES (?1,0,?2,0)",
                        params![id, action],
                    )
                    .unwrap();
                }
            }
            for repetition in 0..5 {
                let order = if repetition % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                };
                for indexed in order {
                    let start = Instant::now();
                    if indexed {
                        c.execute_batch(
                            "CREATE INDEX IF NOT EXISTS event_action_lookup ON events(action) WHERE action='proof-audit-contradiction-v1' OR action='indexed-audit-contradiction-v1';",
                        )
                        .unwrap();
                    } else {
                        c.execute_batch("DROP INDEX IF EXISTS event_action_lookup;")
                            .unwrap();
                    }
                    let setup_ns = start.elapsed().as_nanos();
                    let plan=c.prepare("EXPLAIN QUERY PLAN SELECT EXISTS(SELECT 1 FROM events WHERE action=?1)").unwrap().query_map(["indexed-audit-contradiction-v1"],|r|r.get::<_,String>(3)).unwrap().collect::<Result<Vec<_>,_>>().unwrap().join(" ");
                    if indexed {
                        assert!(plan.contains("COVERING INDEX event_action_lookup"));
                    } else {
                        assert!(plan.contains("SCAN events"));
                    }
                    for action in [
                        "proof-audit-contradiction-v1",
                        "indexed-audit-contradiction-v1",
                    ] {
                        assert_eq!(
                            c.query_row(
                                "SELECT EXISTS(SELECT 1 FROM events WHERE action=?1)",
                                [action],
                                |r| r.get::<_, bool>(0)
                            )
                            .unwrap(),
                            blocked
                        );
                    }
                    let start = Instant::now();
                    for _ in 0..30 {
                        for action in [
                            "proof-audit-contradiction-v1",
                            "indexed-audit-contradiction-v1",
                        ] {
                            assert_eq!(
                                c.query_row(
                                    "SELECT EXISTS(SELECT 1 FROM events WHERE action=?1)",
                                    [action],
                                    |r| r.get::<_, bool>(0)
                                )
                                .unwrap(),
                                blocked
                            );
                        }
                    }
                    let elapsed = start.elapsed().as_nanos();
                    measurements.push(json!({"events_fixture":rows,"blocked_fixture":blocked,"indexed":indexed,"repetition":repetition,"lookup_count":60,"setup_ns":setup_ns,"lookup_ns":elapsed,"plan":plan}));
                }
            }
        }
        drop(c);
    }
    fs::remove_dir_all(&directory).unwrap();
    println!(
        "{}",
        json!({"schema":1,"class":"METADATA_ONLY_NOT_RESEARCH_THROUGHPUT","compute_revision":research::compute_revision().unwrap(),"workload":"Isolated synthetic event histories with no market data/results. Healthy absence and deliberately injected marker-at-tail presence;5 rotated scan/index repetitions for50k/200k rows.",
        "scope":"Warm equality queries with the same failure-marker predicate, parameter binding/statement setup/result checking included; index DDL separately timed. Whole-child resources include fixture creation and DDL. No actual audit failure or scientific result is asserted by fixture markers.",
        "limitations":"Component lookup only; not factory throughput, full-history acceptance, cold IO, event-write overhead or startup-index-build validation for a production-size database.","measurements":measurements})
    );
}
