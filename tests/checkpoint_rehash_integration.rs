use rust_strategy_factory::{
    blocked_native, corpus, data, native,
    storage::{Registry, ReplayCheckpoint},
    strategies::{EntryPlan, Strategy},
    tick_engine::{
        self, CorpusBatchLimits, CorpusBatchSession, CorpusBudget, CorpusProgress, CorpusRun, Costs,
    },
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    spec: corpus::Specification,
    costs: Costs,
    budget: CorpusBudget,
}
impl Fixture {
    fn new(format: u8) -> Self {
        let root = std::env::temp_dir().join(format!(
            "checkpoint-rehash-integration-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let data = data::synthetic_fixture("EURUSD").unwrap();
        let mut paths = Vec::new();
        for (i, ticks) in data.ticks.chunks(250).enumerate() {
            let part = data::dataset(
                data.profile.clone(),
                ticks.to_vec(),
                &data.manifest.source,
                data.manifest.archive_hash.clone(),
                true,
            )
            .unwrap();
            let blocked = format == 1 || (format == 2 && i % 2 == 1);
            let path = root.join(format!("{i}.{}", if blocked { "rsfb" } else { "rsf" }));
            if blocked {
                blocked_native::write(&path, &part, 64).unwrap();
            } else {
                native::write(&path, &part).unwrap();
            }
            paths.push(path);
        }
        let spec = corpus::describe(&paths, 100000).unwrap();
        Self {
            root,
            spec,
            costs: Costs::default(),
            budget: CorpusBudget {
                max_candles: 10000,
                feature_cells: 100000,
                max_receipts: 10000,
            },
        }
    }
    fn run<'a>(&'a self, strategy: &'a Strategy) -> CorpusRun<'a> {
        CorpusRun {
            base: &self.root,
            spec: &self.spec,
            strategy,
            costs: &self.costs,
            budget: self.budget,
        }
    }
    fn limits(&self) -> CorpusBatchLimits {
        CorpusBatchLimits {
            workers: 1,
            max_parallel_candles: 10000,
            max_parallel_feature_cells: 100000,
            max_parallel_receipts: 10000,
            max_new_chunks: 1,
            lease_ms: 60000,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn strategy(entry: Option<EntryPlan>) -> Strategy {
    let mut s:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    s.entry = entry;
    s
}
#[test]
fn combined_private_checkpoints_and_current_source_proofs_preserve_complete_order_receipts() {
    for format in 0..3 {
        let f = Fixture::new(format);
        for (index, entry) in [
            None,
            Some(EntryPlan::StopMarket {
                offset_ticks: 2,
                ttl_ms: 120000,
                cancel_on_signal_false: true,
            }),
            Some(EntryPlan::Limit {
                offset_ticks: 2,
                ttl_ms: 120000,
                cancel_on_signal_false: true,
            }),
        ]
        .into_iter()
        .enumerate()
        {
            let strategy = strategy(entry);
            let reference =
                tick_engine::replay_corpus(&f.root, &f.spec, &strategy, &f.costs, f.budget)
                    .unwrap();
            assert!(!reference.trades.is_empty());
            let db = f.root.join(format!("{index}.sqlite"));
            let mut session = CorpusBatchSession::default();
            let mut replayed = 0;
            let mut decoded = 0;
            let mut skipped = 0;
            let mut completed = None;
            for round in 0..f.spec.chunks.len() {
                let r = session.round(&db, &[f.run(&strategy)], f.limits()).unwrap();
                assert_eq!(r.worker_failures, 0);
                replayed += r.preparation.account_events_replayed;
                decoded += r.preparation.source_events_decoded;
                skipped += r.preparation.trusted_prefix_events_skipped;
                if round > 0 {
                    assert_eq!(r.preparation.current_byte_checks, 1);
                    assert!(r.preparation.trusted_prefix_events_skipped > 0);
                }
                match r.jobs.into_iter().next().unwrap().unwrap() {
                    CorpusProgress::Pending { .. } => {}
                    CorpusProgress::Completed { result, .. } => completed = Some(result),
                    other => panic!("{other:?}"),
                }
            }
            assert_eq!(
                replayed,
                corpus::source_facts(&f.root, &f.spec).unwrap().events()
            );
            assert_eq!(
                decoded, replayed,
                "warm traversal must decode each quote only once"
            );
            assert!(skipped > 0);
            assert_eq!(
                completed.unwrap().result["tick_replay"],
                serde_json::to_value(reference).unwrap()
            );
            Registry::open(&db).unwrap().integrity().unwrap();
        }
    }
}
#[test]
fn combined_reuse_never_authenticates_rehashed_checkpoint_json_or_changed_consumed_artifacts() {
    for format in 0..3 {
        for corrupt_source in [false, true] {
            let f = Fixture::new(format);
            let strategy = strategy(None);
            let db = f.root.join("state.sqlite");
            let mut session = CorpusBatchSession::default();
            session.round(&db, &[f.run(&strategy)], f.limits()).unwrap();
            let id = tick_engine::corpus_key(&f.root, &f.spec, &strategy, &f.costs, f.budget)
                .unwrap()
                .identity()
                .unwrap();
            let registry = Registry::open(&db).unwrap();
            let (mut record, prior) = registry.replay_checkpoint(&id).unwrap().unwrap();
            assert_eq!(record.cursor, 1);
            if corrupt_source {
                let path = &f.spec.chunks[0].path;
                let mut bytes = fs::read(path).unwrap();
                let last = bytes.len() - 1;
                bytes[last] ^= 1;
                fs::write(path, bytes).unwrap();
            } else {
                record.state["account"]["balance"] = 123456789.into();
                let changed: ReplayCheckpoint = record;
                let hash = data::hash(&changed).unwrap();
                assert_ne!(hash, prior);
                let sql = rusqlite::Connection::open(&db).unwrap();
                sql.execute_batch("DROP TRIGGER replay_checkpoint_no_update")
                    .unwrap();
                sql.execute(
                    "UPDATE replay_checkpoints SET body=?1,hash=?2 WHERE experiment_id=?3",
                    rusqlite::params![serde_json::to_vec(&changed).unwrap(), hash, id],
                )
                .unwrap();
                registry.integrity().unwrap();
            }
            let head = registry.replay_checkpoint(&id).unwrap().unwrap().1;
            match session.round(&db, &[f.run(&strategy)], f.limits()) {
                Err(e) => {
                    assert!(corrupt_source);
                    assert!(e.contains("checksum"));
                }
                Ok(r) => {
                    assert!(!corrupt_source);
                    assert!(
                        r.jobs[0]
                            .as_ref()
                            .unwrap_err()
                            .contains("account state differs")
                    );
                }
            }
            assert_eq!(session.retained_candles(), 0);
            assert_eq!(registry.replay_checkpoint(&id).unwrap().unwrap().1, head);
            assert!(registry.result(&id).unwrap().is_none());
        }
    }
}

#[test]
fn cold_range_continuation_recomputes_account_prefix_but_does_not_decode_future_artifacts() {
    for format in 0..3 {
        let f = Fixture::new(format);
        let strategy = strategy(None);
        let db = f.root.join("cold.sqlite");
        let mut session = CorpusBatchSession::default();
        let first = session.round(&db, &[f.run(&strategy)], f.limits()).unwrap();
        assert!(first.jobs[0].is_ok());
        assert_eq!(first.preparation.source_events_decoded, 250);
        session.release();
        let second = session.round(&db, &[f.run(&strategy)], f.limits()).unwrap();
        assert!(second.jobs[0].is_ok());
        assert_eq!(second.preparation.trusted_prefix_events_skipped, 0);
        assert_eq!(second.preparation.account_events_replayed, 500);
        assert_eq!(second.preparation.source_events_decoded, 500);
        let expected = serde_json::to_value(
            tick_engine::replay_corpus(&f.root, &f.spec, &strategy, &f.costs, f.budget).unwrap(),
        )
        .unwrap();
        loop {
            session.release();
            let r = session.round(&db, &[f.run(&strategy)], f.limits()).unwrap();
            match r.jobs.into_iter().next().unwrap().unwrap() {
                CorpusProgress::Pending { .. } => {}
                CorpusProgress::Completed { result, .. } => {
                    assert_eq!(result.result["tick_replay"], expected);
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
    }
}
