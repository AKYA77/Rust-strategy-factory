use rust_strategy_factory::storage::{Claim, ExperimentKey, Registry, Stage};
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
fn seed_checkpoint_history(db: &std::path::Path, id: &str, count: i64) -> String {
    use rust_strategy_factory::storage::ReplayCheckpoint;
    let mut sql = rusqlite::Connection::open(db).unwrap();
    let tx = sql.transaction().unwrap();
    let mut previous = None;
    for cursor in 1..=count {
        let record = ReplayCheckpoint {
            experiment_id: id.into(),
            configuration_hash: key().configuration_hash,
            cursor,
            previous_hash: previous.clone(),
            state: json!({"fixture":"SYNTHETIC journal metadata only; not account evidence","cursor":cursor}),
        };
        let digest = rust_strategy_factory::data::hash(&record).unwrap();
        tx.execute(
            "INSERT INTO replay_checkpoints VALUES(?1,?2,?3,?4)",
            rusqlite::params![id, cursor, serde_json::to_vec(&record).unwrap(), digest],
        )
        .unwrap();
        previous = Some(digest);
    }
    tx.commit().unwrap();
    previous.unwrap()
}
#[test]
fn checkpoint_history_continues_after1000_with_complete_chain_backup_and_old_corruption_refusal() {
    let dir = directory();
    let db = dir.join("long-checkpoints.sqlite");
    let mut registry = Registry::open(&db).unwrap();
    let id = registry.register(&key()).unwrap();
    let prior = seed_checkpoint_history(&db, &id, 1000);
    registry.integrity().unwrap();
    let Claim::Acquired(lease) = registry.claim(&id, 0, 60000).unwrap() else {
        panic!()
    };
    let mut head = prior;
    for cursor in 1001..=1025 {
        if cursor == 1002 {
            // Joint regression: a legitimate history beyond the former reader
            // ceiling must not weaken the attempt fence for its next append.
            let mut wrong = lease.clone();
            wrong.attempt += 1;
            let error = registry
                .save_replay_checkpoint(
                    &wrong,
                    cursor,
                    cursor - 1,
                    cursor,
                    json!({"fixture":"SYNTHETIC wrong attempt beyond1000"}),
                )
                .unwrap_err();
            assert!(error.contains("lease stale"), "{error}");
            assert_eq!(registry.replay_checkpoint(&id).unwrap().unwrap().1, head);
            registry.validate_lease(&lease, cursor).unwrap();
            assert_eq!(registry.counts().unwrap().attempts, 1);
        }
        head = registry
            .save_replay_checkpoint(
                &lease,
                cursor,
                cursor - 1,
                cursor,
                json!({"fixture":"SYNTHETIC metadata only","cursor":cursor}),
            )
            .unwrap();
    }
    let (record, digest) = registry.replay_checkpoint(&id).unwrap().unwrap();
    assert_eq!(record.cursor, 1025);
    assert_eq!(digest, head);
    assert!(
        registry
            .save_replay_checkpoint(
                &lease,
                1026,
                1025,
                1026,
                json!({"oversized":"x".repeat(4194304)})
            )
            .is_err()
    );
    assert_eq!(registry.replay_checkpoint(&id).unwrap().unwrap().1, head);
    let backup = dir.join("backup.sqlite");
    registry.backup(&backup).unwrap();
    assert_eq!(
        Registry::open(&backup)
            .unwrap()
            .replay_checkpoint(&id)
            .unwrap()
            .unwrap()
            .1,
        head
    );
    registry.yield_lease(&lease, 1027).unwrap();
    drop(registry);
    let registry = Registry::open(&db).unwrap();
    registry.integrity().unwrap();
    let sql = rusqlite::Connection::open(&db).unwrap();
    sql.execute_batch("DROP TRIGGER replay_checkpoint_no_update")
        .unwrap();
    sql.execute(
        "UPDATE replay_checkpoints SET hash=?1 WHERE cursor=1",
        ["0".repeat(64)],
    )
    .unwrap();
    drop(sql);
    assert!(registry.replay_checkpoint(&id).is_err());
    assert!(registry.integrity().is_err());
    let refused = dir.join("corrupt-backup.sqlite");
    assert!(registry.backup(&refused).is_err());
    assert!(Registry::open(&refused).unwrap().integrity().is_err());
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn checkpoint_global_row_limit_remains_atomic_at10000_and_full_history_is_verified() {
    let dir = directory();
    let db = dir.join("full-checkpoints.sqlite");
    let mut registry = Registry::open(&db).unwrap();
    let id = registry.register(&key()).unwrap();
    let digest = seed_checkpoint_history(&db, &id, 10000);
    let head = registry.replay_checkpoint(&id).unwrap().unwrap();
    assert_eq!(head.0.cursor, 10000);
    assert_eq!(head.1, digest);
    registry.integrity().unwrap();
    let Claim::Acquired(lease) = registry.claim(&id, 0, 60000).unwrap() else {
        panic!()
    };
    // Even a full journal rejects mismatched authority before reporting quota;
    // the legitimate current actor remains valid and the immutable head stays put.
    let mut wrong = lease.clone();
    wrong.attempt = 0;
    let error = registry
        .save_replay_checkpoint(
            &wrong,
            1,
            10000,
            10001,
            json!({"wrong_attempt_at_quota":true}),
        )
        .unwrap_err();
    assert!(error.contains("lease stale"), "{error}");
    assert_eq!(registry.replay_checkpoint(&id).unwrap().unwrap().1, digest);
    assert!(
        registry
            .save_replay_checkpoint(&lease, 1, 10000, 10001, json!({"overflow":true}))
            .unwrap_err()
            .contains("metadata budget")
    );
    assert_eq!(registry.replay_checkpoint(&id).unwrap().unwrap().1, digest);
    registry.validate_lease(&lease, 2).unwrap();
    let sql = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        sql.query_row("SELECT count(*) FROM replay_checkpoints", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        10000
    );
    drop(sql);
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}
