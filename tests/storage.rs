use rust_strategy_factory::storage::{Claim, ExperimentKey, Outcome, Registry, Stage};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn directory() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "factory-registry-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&p).unwrap();
    p
}
fn key() -> ExperimentKey {
    ExperimentKey {
        concept_hash: "concept".into(),
        instrument: "EURUSD".into(),
        dataset_hash: "quotes".into(),
        start_ms: 0,
        end_exclusive_ms: 1000,
        signal_version: "s1".into(),
        indicator_version: "i1".into(),
        cost_hash: "cost".into(),
        fill_policy: "fill".into(),
        profile_hash: "profile".into(),
        engine_version: "e1".into(),
        protocol_hash: "protocol".into(),
        code_revision: "revision".into(),
        configuration_hash: "config".into(),
        stage: Stage::TickVerification,
    }
}
#[test]
fn identities_include_scientific_versions_and_stage() {
    let k = key();
    let id = k.identity().unwrap();
    let mut x = k.clone();
    x.dataset_hash = "other".into();
    assert_ne!(id, x.identity().unwrap());
    let mut x = k.clone();
    x.cost_hash = "other".into();
    assert_ne!(id, x.identity().unwrap());
    let mut x = k.clone();
    x.stage = Stage::IndependentValidation;
    assert_ne!(id, x.identity().unwrap());
    let mut x = k.clone();
    x.end_exclusive_ms = 0;
    assert!(x.identity().is_err());
}

#[test]
fn compatible_event_lookup_upgrade_preserves_history_and_marker_semantics() {
    let dir = directory();
    let path = dir.join("state.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    let id = registry.register(&key()).unwrap();
    drop(registry);
    let mut c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("DROP INDEX event_action_lookup;").unwrap();
    {
        let tx = c.transaction().unwrap();
        {
            let mut insert=tx.prepare("INSERT INTO events(experiment_id,at_ms,action,attempt) VALUES (?1,0,'SYNTHETIC_METADATA_FIXTURE',0)").unwrap();
            for _ in 0..50000 {
                insert.execute([&id]).unwrap();
            }
        }
        tx.commit().unwrap();
    }
    let before: i64 = c
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
        .unwrap();
    let version: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 9);
    drop(c);
    let registry = Registry::open(&path).unwrap();
    registry.integrity().unwrap();
    drop(registry);
    let c = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        version
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM events", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        before
    );
    let definition: String = c
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='event_action_lookup'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(definition.contains(
        "WHERE action='proof-audit-contradiction-v1' OR action='indexed-audit-contradiction-v1'"
    ));
    for action in [
        "proof-audit-contradiction-v1",
        "indexed-audit-contradiction-v1",
    ] {
        let sql = "SELECT EXISTS(SELECT 1 FROM events WHERE action=?1)";
        assert!(!c.query_row(sql, [action], |r| r.get::<_, bool>(0)).unwrap());
        let plan = c
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap()
            .query_map([action], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join(" ");
        assert!(plan.contains("COVERING INDEX event_action_lookup"));
        c.execute(
            "INSERT INTO events(experiment_id,at_ms,action,attempt) VALUES (?1,0,?2,0)",
            rusqlite::params![id, action],
        )
        .unwrap();
        assert!(c.query_row(sql, [action], |r| r.get::<_, bool>(0)).unwrap());
    }
    drop(c);
    let reopened = Registry::open(&path).unwrap();
    reopened.integrity().unwrap();
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn expired_worker_is_fenced_and_duplicate_finish_is_idempotent() {
    let dir = directory();
    let path = dir.join("state.sqlite");
    let mut a = Registry::open(&path).unwrap();
    let mut b = Registry::open(&path).unwrap();
    let id = a.register(&key()).unwrap();
    assert_eq!(id, b.register(&key()).unwrap());
    let Claim::Acquired(old) = a.claim(&id, 0, 10).unwrap() else {
        panic!()
    };
    assert!(matches!(b.claim(&id, 9, 10).unwrap(), Claim::Busy));
    let Claim::Acquired(new) = b.claim(&id, 10, 10).unwrap() else {
        panic!()
    };
    assert_eq!(new.attempt, 2);
    assert_ne!(old.token, new.token);
    assert!(
        a.finish(&old, 11, Outcome::Completed, &json!({"trades":1}))
            .is_err()
    );
    assert!(
        b.finish(&new, 11, Outcome::Completed, &json!({"trades":1}))
            .unwrap()
    );
    assert!(
        !b.finish(&new, 25, Outcome::Completed, &json!({"trades":1}))
            .unwrap()
    );
    assert!(
        b.finish(&new, 12, Outcome::Completed, &json!({"trades":2}))
            .is_err()
    );
    assert!(matches!(a.claim(&id, 30, 10).unwrap(), Claim::Reused(_)));
    let counts = a.counts().unwrap();
    assert_eq!(
        (counts.registered, counts.completed, counts.attempts),
        (1, 1, 2)
    );
    a.integrity().unwrap();
    drop(a);
    drop(b);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn restart_backup_and_restore_preserve_committed_wal_and_results() {
    let dir = directory();
    let path = dir.join("state.sqlite");
    let backup = dir.join("backup.sqlite");
    let id;
    {
        let mut a = Registry::open(&path).unwrap();
        id = a.register(&key()).unwrap();
        let Claim::Acquired(lease) = a.claim(&id, 0, 100).unwrap() else {
            panic!()
        };
        assert!(
            a.finish(&lease, 100, Outcome::Completed, &json!(1))
                .is_err()
        );
    }
    let mut a = Registry::open(&path).unwrap();
    let Claim::Acquired(lease) = a.claim(&id, 100, 100).unwrap() else {
        panic!()
    };
    a.finish(
        &lease,
        101,
        Outcome::Rejected,
        &json!({"reason":"no evidence"}),
    )
    .unwrap();
    a.backup(&backup).unwrap();
    assert!(a.backup(&backup).is_err());
    let b = Registry::open(&backup).unwrap();
    b.integrity().unwrap();
    assert_eq!(b.result(&id).unwrap().unwrap().outcome, "rejected");
    assert_eq!(b.counts().unwrap().attempts, 2);
    drop(a);
    drop(b);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn concurrent_claims_never_both_acquire() {
    let dir = directory();
    let path = dir.join("state.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    let id = registry.register(&key()).unwrap();
    drop(registry);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let b = barrier.clone();
            let p = path.clone();
            let id = id.clone();
            std::thread::spawn(move || {
                let mut r = Registry::open(&p).unwrap();
                b.wait();
                matches!(r.claim(&id, 0, 100).unwrap(), Claim::Acquired(_))
            })
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .map(|h| usize::from(h.join().unwrap()))
            .sum::<usize>(),
        1
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn result_tampering_is_detected_on_read_and_integrity_check() {
    let dir = directory();
    let path = dir.join("state.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    let id = registry.register(&key()).unwrap();
    let Claim::Acquired(lease) = registry.claim(&id, 0, 100).unwrap() else {
        panic!()
    };
    registry
        .finish(&lease, 1, Outcome::Completed, &json!({"trades":1}))
        .unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute("UPDATE experiments SET result_json='{}' WHERE id=?1", [&id])
        .unwrap();
    assert!(registry.result(&id).is_err());
    assert!(registry.integrity().is_err());
    drop(connection);
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn frozen_holdout_blocks_leakage_and_repeated_tuning_after_restart() {
    use rust_strategy_factory::validation::Interval;
    let dir = directory();
    let path = dir.join("state.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    let train = key();
    registry.register(&train).unwrap();
    let interval = Interval {
        start_ms: 1000,
        end_exclusive_ms: 2000,
    };
    registry
        .reserve_holdout(
            &train.dataset_hash,
            &interval,
            &train.protocol_hash,
            std::slice::from_ref(&train.concept_hash),
        )
        .unwrap();
    let mut test = train.clone();
    test.start_ms = 1000;
    test.end_exclusive_ms = 2000;
    assert!(registry.register(&test).is_err());
    test.stage = Stage::IndependentValidation;
    let id = registry.register(&test).unwrap();
    assert_eq!(id, registry.register(&test).unwrap());
    let mut retuned = test.clone();
    retuned.cost_hash = "retuned".into();
    assert!(registry.register(&retuned).is_err());
    retuned = test.clone();
    retuned.concept_hash = "child".into();
    assert!(registry.register(&retuned).is_err());
    retuned = test.clone();
    retuned.protocol_hash = "weakened".into();
    assert!(registry.register(&retuned).is_err());
    drop(registry);
    let mut registry = Registry::open(&path).unwrap();
    assert!(registry.register(&retuned).is_err());
    assert!(
        registry
            .reserve_holdout(&train.dataset_hash, &interval, "reset", &["child".into()])
            .is_err()
    );
    registry.integrity().unwrap();
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn final_holdout_cannot_be_reserved_after_training_exposure() {
    use rust_strategy_factory::validation::Interval;
    let dir = directory();
    let path = dir.join("state.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    let k = key();
    registry.register(&k).unwrap();
    assert!(
        registry
            .reserve_holdout(
                &k.dataset_hash,
                &Interval {
                    start_ms: 500,
                    end_exclusive_ms: 1500
                },
                &k.protocol_hash,
                std::slice::from_ref(&k.concept_hash)
            )
            .is_err()
    );
    let mut independent = k;
    independent.stage = Stage::IndependentValidation;
    assert!(registry.register(&independent).is_err());
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn dataset_aliases_cannot_bypass_protection_or_retroactively_claim_independence() {
    use rust_strategy_factory::validation::Interval;
    let dir = directory();
    let path = dir.join("state.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    let k = key();
    let period = Interval {
        start_ms: 0,
        end_exclusive_ms: 1000,
    };
    registry
        .reserve_instrument_holdout(
            &k.dataset_hash,
            &k.instrument,
            &period,
            &k.protocol_hash,
            std::slice::from_ref(&k.concept_hash),
        )
        .unwrap();
    let mut alias = k.clone();
    alias.dataset_hash = "revised-history".into();
    assert!(
        registry
            .register(&alias)
            .unwrap_err()
            .contains("alias/revision")
    );
    alias.stage = Stage::IndependentValidation;
    assert!(registry.register(&alias).is_err());
    assert!(
        registry
            .reserve_instrument_holdout(
                &alias.dataset_hash,
                &alias.instrument,
                &period,
                "another-protocol",
                &["child".into()]
            )
            .is_err()
    );
    // Other symbols are separate; adjacent exclusive intervals do not touch.
    alias.stage = Stage::TickVerification;
    alias.instrument = "GBPUSD".into();
    registry.register(&alias).unwrap();
    alias.instrument = "EURUSD".into();
    alias.start_ms = 1000;
    alias.end_exclusive_ms = 2000;
    registry.register(&alias).unwrap();
    assert!(
        registry
            .reserve_instrument_holdout(
                "another-content-hash",
                "EURUSD",
                &Interval {
                    start_ms: 1500,
                    end_exclusive_ms: 2500
                },
                "later",
                &["child".into()]
            )
            .is_err()
    );
    drop(registry);
    let mut registry = Registry::open(&path).unwrap();
    alias.start_ms = 1;
    assert!(registry.register(&alias).is_err());
    assert_eq!(registry.counts().unwrap().registered, 2);
    registry.integrity().unwrap();
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn protected_oos_registration_and_reservation_are_exclusive_and_durable() {
    use rust_strategy_factory::validation::Interval;
    let dir = directory();
    let path = dir.join("state.sqlite");
    let mut a = Registry::open(&path).unwrap();
    let mut b = Registry::open(&path).unwrap();
    let k = key();
    let range = Interval {
        start_ms: 0,
        end_exclusive_ms: 1000,
    };
    a.reserve_protocol_ranges(
        &k.dataset_hash,
        &k.instrument,
        &k.protocol_hash,
        std::slice::from_ref(&range),
    )
    .unwrap();
    assert!(
        a.reserve_protocol_ranges(
            &k.dataset_hash,
            &k.instrument,
            &k.protocol_hash,
            &[
                range.clone(),
                Interval {
                    start_ms: 2000,
                    end_exclusive_ms: 3000
                }
            ]
        )
        .is_err()
    );
    a.register(&k).unwrap();
    let mut outsider = k.clone();
    outsider.protocol_hash = "other-study".into();
    assert!(b.register(&outsider).is_err());
    assert!(
        b.reserve_protocol_ranges(
            &k.dataset_hash,
            &k.instrument,
            "other-study",
            std::slice::from_ref(&range)
        )
        .is_err()
    );
    outsider.protocol_hash = k.protocol_hash.clone();
    outsider.dataset_hash = "alias".into();
    assert!(b.register(&outsider).is_err());
    outsider.instrument = "GBPUSD".into();
    b.register(&outsider).unwrap();
    assert!(
        b.reserve_instrument_holdout(
            "alias",
            "EURUSD",
            &range,
            "other-study",
            &["concept".into()]
        )
        .is_err()
    );
    assert!(
        b.reserve_holdout("alias", &range, "other-study", &["concept".into()])
            .is_err()
    );
    drop(a);
    drop(b);
    let mut a = Registry::open(&path).unwrap();
    a.reserve_protocol_ranges(
        &k.dataset_hash,
        &k.instrument,
        &k.protocol_hash,
        std::slice::from_ref(&range),
    )
    .unwrap();
    outsider.instrument = "EURUSD".into();
    assert!(a.register(&outsider).is_err());
    a.integrity().unwrap();
    drop(a);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn multi_range_reservation_rolls_back_when_a_later_range_is_exposed() {
    use rust_strategy_factory::validation::Interval;
    let dir = directory();
    let path = dir.join("state.sqlite");
    let mut a = Registry::open(&path).unwrap();
    let mut exposed = key();
    exposed.start_ms = 2000;
    exposed.end_exclusive_ms = 3000;
    a.register(&exposed).unwrap();
    let ranges = [
        Interval {
            start_ms: 0,
            end_exclusive_ms: 1000,
        },
        Interval {
            start_ms: 2000,
            end_exclusive_ms: 3000,
        },
    ];
    assert!(
        a.reserve_protocol_ranges("alias", "EURUSD", "new-study", &ranges)
            .is_err()
    );
    // First range must not have survived a partial transaction.
    a.register(&key()).unwrap();
    let connection = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM protocol_ranges", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    drop(connection);
    drop(a);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn concurrent_oos_reservation_and_outside_registration_cannot_both_succeed() {
    use rust_strategy_factory::validation::Interval;
    use std::sync::{Arc, Barrier};
    let dir = directory();
    let path = dir.join("state.sqlite");
    drop(Registry::open(&path).unwrap());
    let ready = Arc::new(Barrier::new(2));
    let (reserved, registered) = std::thread::scope(|scope| {
        let first_path = path.clone();
        let first_ready = ready.clone();
        let first = scope.spawn(move || {
            let mut registry = Registry::open(&first_path).unwrap();
            first_ready.wait();
            registry
                .reserve_protocol_ranges(
                    "quotes",
                    "EURUSD",
                    "frozen-study",
                    &[Interval {
                        start_ms: 0,
                        end_exclusive_ms: 1000,
                    }],
                )
                .is_ok()
        });
        let second_path = path.clone();
        let second_ready = ready.clone();
        let second = scope.spawn(move || {
            let mut registry = Registry::open(&second_path).unwrap();
            second_ready.wait();
            registry.register(&key()).is_ok()
        });
        (first.join().unwrap(), second.join().unwrap())
    });
    assert_ne!(reserved, registered);
    let registry = Registry::open(&path).unwrap();
    assert_eq!(registry.counts().unwrap().registered, i64::from(registered));
    registry.integrity().unwrap();
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn immutable_replay_checkpoints_fence_stale_workers_and_survive_restart() {
    let dir = directory();
    let path = dir.join("state.sqlite");
    let mut r = Registry::open(&path).unwrap();
    let id = r.register(&key()).unwrap();
    let Claim::Acquired(first) = r.claim(&id, 0, 10).unwrap() else {
        panic!()
    };
    let h = r
        .save_replay_checkpoint(
            &first,
            1,
            0,
            5,
            json!({"balance":"20000000","last_sequence":"18446744073709551615"}),
        )
        .unwrap();
    assert!(
        r.save_replay_checkpoint(&first, 2, 0, 6, json!({}))
            .is_err()
    );
    assert!(
        r.save_replay_checkpoint(&first, 10, 5, 6, json!({}))
            .is_err()
    );
    drop(r);
    let mut r = Registry::open(&path).unwrap();
    assert_eq!(r.replay_checkpoint(&id).unwrap().unwrap().1, h);
    let Claim::Acquired(second) = r.claim(&id, 10, 20).unwrap() else {
        panic!()
    };
    assert!(
        r.save_replay_checkpoint(&first, 11, 5, 6, json!({}))
            .is_err()
    );
    r.save_replay_checkpoint(&second, 11, 5, 7, json!({"balance":19000000}))
        .unwrap();
    r.integrity().unwrap();
    let snapshot = dir.join("checkpoint-snapshot.sqlite");
    r.backup(&snapshot).unwrap();
    let restored = Registry::open(&snapshot).unwrap();
    assert_eq!(
        restored.replay_checkpoint(&id).unwrap().unwrap().0.cursor,
        7
    );
    drop(restored);
    let damaged = rusqlite::Connection::open(&snapshot).unwrap();
    damaged
        .execute_batch("DROP TRIGGER replay_checkpoint_no_update")
        .unwrap();
    damaged
        .execute(
            "UPDATE replay_checkpoints SET hash=?1 WHERE cursor=5",
            ["0".repeat(64)],
        )
        .unwrap();
    drop(damaged);
    let damaged = Registry::open(&snapshot).unwrap();
    assert!(damaged.replay_checkpoint(&id).is_err());
    assert!(damaged.integrity().is_err());
    drop(damaged);
    let sql = rusqlite::Connection::open(&path).unwrap();
    assert!(sql.execute("DELETE FROM replay_checkpoints", []).is_err());
    assert!(
        sql.execute("UPDATE replay_checkpoints SET hash='x'", [])
            .is_err()
    );
    r.finish(&second, 12, Outcome::Completed, &json!({"fixture":true}))
        .unwrap();
    assert!(
        r.save_replay_checkpoint(&second, 13, 7, 8, json!({}))
            .is_err()
    );
    drop(r);
    drop(sql);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn lease_snapshot_validation_checks_attempt_expiry_and_terminal_state() {
    let dir = directory();
    let mut r = Registry::open(&dir.join("lease.sqlite")).unwrap();
    let id = r.register(&key()).unwrap();
    let Claim::Acquired(lease) = r.claim(&id, 0, 10).unwrap() else {
        panic!()
    };
    r.validate_lease(&lease, 9).unwrap();
    assert!(r.validate_lease(&lease, 10).is_err());
    let mut forged = lease.clone();
    forged.attempt += 1;
    assert!(r.validate_lease(&forged, 1).is_err());
    r.finish(&lease, 2, Outcome::Completed, &json!({"fixture":true}))
        .unwrap();
    assert!(r.validate_lease(&lease, 3).is_err());
    drop(r);
    std::fs::remove_dir_all(dir).unwrap();
}
