use rust_strategy_factory::{
    corpus, data, native,
    storage::{Claim, Lease, Registry},
    strategies::Strategy,
    tick_engine::{self, CorpusBudget, CorpusProgress, CorpusRun, Costs},
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    path: PathBuf,
    spec: corpus::Specification,
    strategy: Strategy,
    costs: Costs,
    budget: CorpusBudget,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "corpus-error-lifecycle-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("source.rsf");
        native::write(&path, &data::synthetic_fixture("EURUSD").unwrap()).unwrap();
        let spec = corpus::describe(std::slice::from_ref(&path), 100000).unwrap();
        let strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
        Self {
            root,
            path,
            spec,
            strategy,
            costs: Costs::default(),
            budget: CorpusBudget {
                max_candles: 10000,
                feature_cells: 100000,
                max_receipts: 10000,
            },
        }
    }
    fn run(&self) -> CorpusRun<'_> {
        CorpusRun {
            base: &self.root,
            spec: &self.spec,
            strategy: &self.strategy,
            costs: &self.costs,
            budget: self.budget,
        }
    }
    fn id(&self) -> String {
        tick_engine::corpus_key(
            &self.root,
            &self.spec,
            &self.strategy,
            &self.costs,
            self.budget,
        )
        .unwrap()
        .identity()
        .unwrap()
    }
    fn corrupt(&self) {
        let mut bytes = fs::read(&self.path).unwrap();
        let n = bytes.len();
        bytes[n - 1] ^= 1;
        fs::write(&self.path, bytes).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn returned_postclaim_source_failure_yields_current_lease_for_immediate_exact_retry() {
    let f = Fixture::new();
    let db = f.root.join("state.sqlite");
    let mut registry = Registry::open(&db).unwrap();
    let id = f.id();
    let bytes = fs::read(&f.path).unwrap();
    let mut calls = 0;
    let error = tick_engine::execute_corpus_step(&mut registry, f.run(), 64, 60000, || {
        calls += 1;
        if calls == 2 {
            f.corrupt();
        }
        10
    })
    .unwrap_err();
    assert!(error.contains("checksum"), "{error}");
    let sql = rusqlite::Connection::open(&db).unwrap();
    let (state, token): (String, Option<String>) = sql
        .query_row(
            "SELECT state,token FROM experiments WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, "queued");
    assert!(token.is_none());
    drop(sql);
    assert_eq!(registry.counts().unwrap().attempts, 1);
    assert!(registry.replay_checkpoint(&id).unwrap().is_none());
    assert!(registry.result(&id).unwrap().is_none());
    fs::write(&f.path, bytes).unwrap();
    let progress =
        tick_engine::execute_corpus_step(&mut registry, f.run(), 64, 60000, || 11).unwrap();
    let CorpusProgress::Completed {
        result,
        cached: false,
        ..
    } = progress
    else {
        panic!("{progress:?}")
    };
    let reference =
        tick_engine::replay_corpus(&f.root, &f.spec, &f.strategy, &f.costs, f.budget).unwrap();
    assert!(!reference.trades.is_empty());
    assert_eq!(
        result.result["tick_replay"],
        serde_json::to_value(reference).unwrap()
    );
    assert_eq!(registry.counts().unwrap().attempts, 2);
    registry.integrity().unwrap();
}
#[test]
fn returned_old_worker_failure_never_yields_replacement_actor_or_masks_lease_error() {
    let f = Fixture::new();
    let db = f.root.join("replacement.sqlite");
    let mut registry = Registry::open(&db).unwrap();
    let id = f.id();
    registry
        .register(
            &tick_engine::corpus_key(&f.root, &f.spec, &f.strategy, &f.costs, f.budget).unwrap(),
        )
        .unwrap();
    let mut other = Registry::open(&db).unwrap();
    let mut replacement: Option<Lease> = None;
    let mut calls = 0;
    let error = tick_engine::execute_corpus_step(&mut registry, f.run(), 64, 10, || {
        calls += 1;
        if calls == 1 {
            return 0;
        }
        if calls == 2 {
            let Claim::Acquired(lease) = other.claim(&id, 11, 1000).unwrap() else {
                panic!()
            };
            replacement = Some(lease);
        }
        12
    })
    .unwrap_err();
    assert_eq!(error, "lease stale, expired or terminal");
    let current = replacement.unwrap();
    registry.validate_lease(&current, 12).unwrap();
    assert_eq!(registry.counts().unwrap().attempts, 2);
    assert!(registry.replay_checkpoint(&id).unwrap().is_none());
    registry.yield_lease(&current, 13).unwrap();
    registry.integrity().unwrap();
}
#[test]
fn cleanup_database_refusal_preserves_original_error_and_expiry_recovery() {
    let f = Fixture::new();
    let db = f.root.join("cleanup-refusal.sqlite");
    let mut registry = Registry::open(&db).unwrap();
    let id = f.id();
    let sql = rusqlite::Connection::open(&db).unwrap();
    sql.execute_batch("CREATE TRIGGER injected_yield_refusal BEFORE UPDATE OF state ON experiments WHEN NEW.state='queued' BEGIN SELECT RAISE(ABORT,'injected yield refusal');END;").unwrap();
    let bytes = fs::read(&f.path).unwrap();
    let mut calls = 0;
    let error = tick_engine::execute_corpus_step(&mut registry, f.run(), 64, 10, || {
        calls += 1;
        if calls == 2 {
            f.corrupt();
        }
        0
    })
    .unwrap_err();
    assert!(error.contains("checksum"));
    assert!(!error.contains("injected yield refusal"));
    assert_eq!(registry.counts().unwrap().attempts, 1);
    assert!(registry.replay_checkpoint(&id).unwrap().is_none());
    fs::write(&f.path, bytes).unwrap();
    sql.execute_batch("DROP TRIGGER injected_yield_refusal")
        .unwrap();
    let Claim::Acquired(current) = registry.claim(&id, 11, 1000).unwrap() else {
        panic!()
    };
    registry.validate_lease(&current, 11).unwrap();
    registry.integrity().unwrap();
}
