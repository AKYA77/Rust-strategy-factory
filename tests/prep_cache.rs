//! Milestone 4 contract: exact scope identity, cold/warm parity, invalidation,
//! immutable content-addressed artifacts, byte accounting, deduplication,
//! corruption refusal and explicit partial-write recovery. No scientific gate
//! or qualification claim is implied by a cache hit.

use rust_strategy_factory::prep_cache::{
    Limits, MAX_ARTIFACT_BYTES, PrepCache, Protection, Scope, Stage, Status, decode, encode,
};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(1);
struct Temp(PathBuf);
impl Temp {
    fn new(tag: &str) -> Self {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rsf-prep-cache-{tag}-{}-{serial}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn h(n: u8) -> String {
    format!("{n:064x}")
}
fn scope(stage: Stage, protection: Protection) -> Scope {
    Scope {
        instrument: "EURUSD".into(),
        dataset_hash: h(1),
        strategy_hash: h(2),
        costs_hash: h(3),
        engine_version: "ordered-l1-v6-frozen-atr-stops".into(),
        config_hash: h(4),
        start_ms: 1_700_000_000_000,
        end_exclusive_ms: 1_700_000_060_000,
        stage,
        protection,
    }
}
fn payload(n: u8) -> Vec<u8> {
    encode(&json!({"candles":[n, n + 1, n + 2], "features":{"atr": n}})).unwrap()
}
fn variants() -> Vec<Scope> {
    let base = scope(Stage::Candle, Protection::Standard);
    let mut list = Vec::new();
    let mut s = base.clone();
    s.instrument = "GBPUSD".into();
    list.push(s);
    let mut s = base.clone();
    s.dataset_hash = h(9);
    list.push(s);
    let mut s = base.clone();
    s.strategy_hash = h(9);
    list.push(s);
    let mut s = base.clone();
    s.costs_hash = h(9);
    list.push(s);
    let mut s = base.clone();
    s.engine_version = "ordered-l1-v7".into();
    list.push(s);
    let mut s = base.clone();
    s.config_hash = h(9);
    list.push(s);
    let mut s = base.clone();
    s.start_ms += 1;
    list.push(s);
    let mut s = base.clone();
    s.end_exclusive_ms += 1;
    list.push(s);
    list.push(scope(Stage::Feature, Protection::Standard));
    list.push(scope(Stage::Candle, Protection::Protected));
    list
}

#[test]
fn scope_identity_is_exact_over_every_dimension() {
    let base = scope(Stage::Candle, Protection::Standard);
    let id = base.identity().unwrap();
    assert_eq!(id.len(), 64);
    assert_eq!(base.identity().unwrap(), id);
    let list = variants();
    assert_eq!(list.len(), 10);
    let mut identities = HashSet::new();
    identities.insert(id.clone());
    for (index, variant) in list.iter().enumerate() {
        let other = variant.identity().unwrap();
        assert_ne!(other, id, "variant {index} did not change identity");
        assert!(identities.insert(other), "variant {index} collided");
    }
    assert_eq!(identities.len(), 11);

    let mut bad = base.clone();
    bad.instrument = "XAUUSD".into();
    assert_eq!(
        bad.identity().unwrap_err(),
        "cache scope instrument outside the seven-instrument scope"
    );
    bad = base.clone();
    bad.dataset_hash = "abc".into();
    assert_eq!(
        bad.identity().unwrap_err(),
        "cache scope requires 64-hex content identities"
    );
    bad = base.clone();
    bad.engine_version = String::new();
    assert_eq!(
        bad.identity().unwrap_err(),
        "cache scope engine version is incomplete"
    );
    bad = base.clone();
    bad.start_ms = base.end_exclusive_ms;
    assert_eq!(
        bad.identity().unwrap_err(),
        "cache scope window is outside declared bounds"
    );
    bad = base.clone();
    bad.start_ms = -1;
    assert_eq!(
        bad.identity().unwrap_err(),
        "cache scope window is outside declared bounds"
    );
}

#[test]
fn cold_warm_parity_is_exact_for_candle_feature_and_decision() {
    let temp = Temp::new("parity");
    let mut cache = PrepCache::open(temp.path()).unwrap();
    for stage in [Stage::Candle, Stage::Feature, Stage::Decision] {
        let s = scope(stage, Protection::Standard);
        let data = payload(match stage {
            Stage::Candle => 1,
            Stage::Feature => 2,
            Stage::Decision => 3,
        });
        let calls = Cell::new(0usize);
        let (cold, status) = cache
            .get_or_fill(&s, || {
                calls.set(calls.get() + 1);
                Ok(data.clone())
            })
            .unwrap();
        assert_eq!(status, Status::Cold);
        assert_eq!(status.name(), "cold");
        assert_eq!(calls.get(), 1);
        assert_eq!(cold.data, data);

        let (warm, status) = cache
            .get_or_fill(&s, || -> Result<Vec<u8>, String> {
                panic!("producer must not run on a warm hit")
            })
            .unwrap();
        assert_eq!(status, Status::Warm);
        assert_eq!(status.name(), "warm");
        assert_eq!(warm.data, cold.data);
        assert_eq!(
            serde_json::to_value(&cold.receipt).unwrap(),
            serde_json::to_value(&warm.receipt).unwrap()
        );
        assert_eq!(
            serde_json::to_value(cache.receipt(&s).unwrap().unwrap()).unwrap(),
            serde_json::to_value(&cold.receipt).unwrap()
        );

        assert_eq!(cold.receipt.version, "prep-cache-receipt-v1");
        assert_eq!(cold.receipt.scope_id, s.identity().unwrap());
        assert_eq!(cold.receipt.stage, stage.name());
        assert_eq!(cold.receipt.protection, "standard");
        assert_eq!(cold.receipt.bytes, data.len() as u64);
        assert_eq!(cold.receipt.instrument, "EURUSD");
        assert_eq!(cold.receipt.dataset_hash, h(1));
        assert_eq!(cold.receipt.config_hash, h(4));
        assert_eq!(cold.receipt.start_ms, s.start_ms);
        assert_eq!(cold.receipt.end_exclusive_ms, s.end_exclusive_ms);
    }
    assert!(cache.integrity().is_ok());
    assert_eq!(cache.inventory().unwrap()["indexed_artifacts"], "3");
}

#[test]
fn artifacts_survive_reopen_with_exact_receipts() {
    let temp = Temp::new("reopen");
    let s = scope(Stage::Feature, Protection::Standard);
    let data = payload(12);
    let first = {
        let mut cache = PrepCache::open(temp.path()).unwrap();
        cache.put(&s, &data).unwrap()
    };
    let mut cache = PrepCache::open(temp.path()).unwrap();
    let hit = cache.get(&s).unwrap().unwrap();
    assert_eq!(hit.data, data);
    assert_eq!(hit.receipt, first);
    let (warm, status) = cache
        .get_or_fill(&s, || -> Result<Vec<u8>, String> {
            panic!("reopened cache must reuse the durable artifact")
        })
        .unwrap();
    assert_eq!(status, Status::Warm);
    assert_eq!(warm.receipt, first);
    assert!(cache.integrity().is_ok());
}

#[test]
fn changing_any_scope_dimension_invalidates_the_cache_entry() {
    let temp = Temp::new("invalidate");
    let mut cache = PrepCache::open(temp.path()).unwrap();
    let base = scope(Stage::Candle, Protection::Standard);
    let data = payload(5);
    let base_receipt = cache.put(&base, &data).unwrap();
    assert_eq!(cache.get(&base).unwrap().unwrap().data, data);
    let list = variants();
    let mut guarded = PrepCache::open_protected(temp.path()).unwrap();
    let mut ids = HashSet::new();
    ids.insert(base_receipt.scope_id.clone());
    for (index, variant) in list.iter().enumerate() {
        let handle = if variant.protection == Protection::Protected {
            &mut guarded
        } else {
            &mut cache
        };
        assert!(
            handle.get(variant).unwrap().is_none(),
            "variant {index} was served from another scope"
        );
        let receipt = handle.put(variant, &data).unwrap();
        assert_eq!(receipt.scope_id, variant.identity().unwrap());
        assert!(
            ids.insert(receipt.scope_id.clone()),
            "variant {index} collided"
        );
        assert_eq!(handle.get(variant).unwrap().unwrap().data, data);
    }
    assert_eq!(ids.len(), 11);
    assert_eq!(cache.inventory().unwrap()["indexed_artifacts"], "11");
    // Every scope keeps its own payload even though the raw bytes are identical.
    assert_eq!(cache.inventory().unwrap()["physical_files"], "2");
}

#[test]
fn corrupted_mismatched_or_truncated_artifacts_are_refused() {
    let temp = Temp::new("corrupt");
    let mut cache = PrepCache::open(temp.path()).unwrap();
    let s = scope(Stage::Feature, Protection::Standard);
    let data = payload(11);
    let receipt = cache.put(&s, &data).unwrap();
    let path = temp
        .path()
        .join("artifacts")
        .join("standard")
        .join(format!("{}.bin", receipt.content_hash));
    assert_eq!(fs::read(&path).unwrap(), data);
    assert!(cache.integrity().is_ok());

    let mut flipped = fs::read(&path).unwrap();
    flipped[0] ^= 0xff;
    fs::write(&path, &flipped).unwrap();
    let error = cache.get(&s).unwrap_err();
    assert!(error.contains("content hash mismatch"), "{error}");
    let error = cache.put(&s, &data).unwrap_err();
    assert!(error.contains("content hash mismatch"), "{error}");
    assert!(cache.integrity().is_err());

    fs::write(&path, &flipped[..flipped.len() - 1]).unwrap();
    let error = cache.get(&s).unwrap_err();
    assert!(error.contains("file type/size mismatch"), "{error}");
    let mut extended = flipped.clone();
    extended.push(0);
    fs::write(&path, &extended).unwrap();
    let error = cache.get(&s).unwrap_err();
    assert!(error.contains("file type/size mismatch"), "{error}");

    fs::write(&path, &data).unwrap();
    assert_eq!(cache.get(&s).unwrap().unwrap().data, data);
    fs::remove_file(&path).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(temp.path().join("artifacts"), &path).unwrap();
        let error = cache.get(&s).unwrap_err();
        assert!(error.contains("file type/size mismatch"), "{error}");
        fs::remove_file(&path).unwrap();
    }
    fs::write(&path, &data).unwrap();
    assert!(cache.integrity().is_ok());
}

#[test]
fn immutable_index_rows_reject_update_and_delete() {
    let temp = Temp::new("immutable");
    let mut cache = PrepCache::open(temp.path()).unwrap();
    let s = scope(Stage::Candle, Protection::Standard);
    cache.put(&s, &payload(6)).unwrap();
    let connection = rusqlite::Connection::open(temp.path().join("prep-cache.sqlite")).unwrap();
    let error = connection
        .execute("UPDATE prep_artifacts SET bytes=bytes+1", [])
        .unwrap_err()
        .to_string();
    assert!(error.contains("prep artifact immutable"), "{error}");
    let error = connection
        .execute("DELETE FROM prep_artifacts", [])
        .unwrap_err()
        .to_string();
    assert!(error.contains("prep artifact immutable"), "{error}");
    drop(connection);
    assert!(cache.integrity().is_ok());
}

#[test]
fn byte_caps_are_enforced_on_artifact_and_store() {
    let temp = Temp::new("caps");
    let limits = Limits {
        max_artifact_bytes: 64,
        max_store_bytes: 100,
        max_artifacts: 10,
    };
    let mut cache = PrepCache::open_with(temp.path(), Protection::Standard, limits).unwrap();
    assert_eq!(cache.limits(), limits);
    let s = scope(Stage::Candle, Protection::Standard);
    assert_eq!(
        cache.put(&s, &[b'x'; 65]).unwrap_err(),
        "cache artifact length outside declared budget"
    );
    assert_eq!(
        cache.put(&s, &[]).unwrap_err(),
        "cache artifact length outside declared budget"
    );
    assert!(cache.get(&s).unwrap().is_none());

    cache.put(&s, &[b'a'; 64]).unwrap();
    let mut second = s.clone();
    second.dataset_hash = h(8);
    assert_eq!(
        cache.put(&second, &[b'b'; 64]).unwrap_err(),
        "cache store budget exceeded"
    );
    assert_eq!(cache.inventory().unwrap()["indexed_artifacts"], "1");
    assert_eq!(cache.inventory().unwrap()["indexed_bytes"], "64");

    let limited_count = Limits {
        max_artifact_bytes: 64,
        max_store_bytes: 512,
        max_artifacts: 1,
    };
    let temp2 = Temp::new("count");
    let mut cache2 =
        PrepCache::open_with(temp2.path(), Protection::Standard, limited_count).unwrap();
    cache2.put(&s, &[b'a'; 64]).unwrap();
    assert_eq!(
        cache2.put(&second, &[b'b'; 64]).unwrap_err(),
        "cache store budget exceeded"
    );

    let outside = Limits {
        max_artifact_bytes: MAX_ARTIFACT_BYTES + 1,
        ..Limits::default()
    };
    assert_eq!(
        PrepCache::open_with(temp2.path(), Protection::Standard, outside).unwrap_err(),
        "cache limits outside declared bounds"
    );
    let inverted = Limits {
        max_store_bytes: 1,
        ..Limits::default()
    };
    assert_eq!(
        PrepCache::open_with(temp2.path(), Protection::Standard, inverted).unwrap_err(),
        "cache limits outside declared bounds"
    );
}

#[test]
fn identical_scopes_deduplicate_and_decode_to_equivalent_data() {
    let temp = Temp::new("dedupe");
    let mut cache = PrepCache::open(temp.path()).unwrap();
    let value: Value = json!({"candles":[1,2,3],"features":{"atr":42,"rsi":null}});
    let data = encode(&value).unwrap();
    let a = scope(Stage::Feature, Protection::Standard);
    let mut b = a.clone();
    b.strategy_hash = h(77);
    let first = cache.put(&a, &data).unwrap();
    let second = cache.put(&b, &data).unwrap();
    assert_eq!(first.content_hash, second.content_hash);
    assert_ne!(first.scope_id, second.scope_id);
    let inventory = cache.inventory().unwrap();
    assert_eq!(inventory["indexed_artifacts"], "2");
    assert_eq!(inventory["physical_files"], "1");
    assert_eq!(inventory["physical_bytes"], data.len().to_string());
    assert_eq!(inventory["indexed_bytes"], (2 * data.len()).to_string());

    let again = cache.put(&a, &data).unwrap();
    assert_eq!(again, first);
    let inventory = cache.inventory().unwrap();
    assert_eq!(inventory["indexed_artifacts"], "2");
    assert_eq!(inventory["physical_files"], "1");

    let hit = cache.get(&b).unwrap().unwrap();
    assert_eq!(hit.data, data);
    assert_eq!(decode(&hit.data, &hit.receipt.content_hash).unwrap(), value);
    assert_eq!(
        decode(&hit.data, &"0".repeat(64)).unwrap_err(),
        "cache decoded content hash mismatch"
    );
    assert!(cache.integrity().is_ok());
}

#[test]
fn protected_scopes_are_physically_and_logically_isolated() {
    let temp = Temp::new("isolation");
    let mut shared = PrepCache::open(temp.path()).unwrap();
    let mut protected = PrepCache::open_protected(temp.path()).unwrap();
    assert_eq!(shared.policy(), Protection::Standard);
    assert_eq!(protected.policy(), Protection::Protected);
    let plain = scope(Stage::Decision, Protection::Standard);
    let guarded = scope(Stage::Decision, Protection::Protected);
    assert_ne!(plain.identity().unwrap(), guarded.identity().unwrap());
    let data = payload(9);

    assert_eq!(
        shared.put(&guarded, &data).unwrap_err(),
        "cache protection isolation violation"
    );
    assert_eq!(
        protected.put(&plain, &data).unwrap_err(),
        "cache protection isolation violation"
    );
    assert_eq!(
        shared.get(&guarded).unwrap_err().to_string(),
        "cache protection isolation violation"
    );
    assert_eq!(
        protected.get(&plain).unwrap_err().to_string(),
        "cache protection isolation violation"
    );

    let ordinary = shared.put(&plain, &data).unwrap();
    let guarded_receipt = protected.put(&guarded, &data).unwrap();
    assert_eq!(ordinary.content_hash, guarded_receipt.content_hash);
    assert_ne!(ordinary.scope_id, guarded_receipt.scope_id);
    assert_eq!(
        serde_json::to_value(&ordinary).unwrap()["protection"],
        "standard"
    );
    assert_eq!(
        serde_json::to_value(&guarded_receipt).unwrap()["protection"],
        "protected"
    );
    assert_eq!(
        shared.get(&guarded).unwrap_err().to_string(),
        "cache protection isolation violation"
    );
    assert_eq!(shared.get(&plain).unwrap().unwrap().data, data);
    assert_eq!(protected.get(&guarded).unwrap().unwrap().data, data);
    let artifacts = temp.path().join("artifacts");
    assert!(
        artifacts
            .join("standard")
            .join(format!("{}.bin", ordinary.content_hash))
            .is_file()
    );
    assert!(
        artifacts
            .join("protected")
            .join(format!("{}.bin", guarded_receipt.content_hash))
            .is_file()
    );
    assert_eq!(shared.inventory().unwrap()["standard_files"], "1");
    assert_eq!(shared.inventory().unwrap()["protected_files"], "1");
    assert_eq!(shared.inventory().unwrap()["indexed_artifacts"], "2");
    assert_eq!(protected.inventory().unwrap()["indexed_artifacts"], "2");
    assert!(shared.integrity().is_ok());
    assert!(protected.integrity().is_ok());
}

#[test]
fn partial_writes_are_never_served_and_recovery_is_explicit() {
    let temp = Temp::new("recovery");
    let mut cache = PrepCache::open(temp.path()).unwrap();
    let s = scope(Stage::Candle, Protection::Standard);
    // A cache miss is always a miss; a staged file is never a candidate.
    assert!(cache.get(&s).unwrap().is_none());
    let artifacts = temp.path().join("artifacts");
    let partial = artifacts.join(format!(".partial-{}", "a".repeat(64)));
    fs::write(&partial, b"incomplete").unwrap();
    assert_eq!(cache.inventory().unwrap()["partial_files"], "1");
    assert!(cache.get(&s).unwrap().is_none());
    assert_eq!(cache.recover_partial().unwrap(), 1);
    assert!(!partial.exists());
    assert_eq!(cache.inventory().unwrap()["partial_files"], "0");
    assert_eq!(cache.recover_partial().unwrap(), 0);

    // Unrecognized entries are refused rather than cleaned by guesswork.
    let stranger = artifacts.join("unexpected.txt");
    fs::write(&stranger, b"x").unwrap();
    assert!(
        cache
            .inventory()
            .unwrap_err()
            .contains("unrecognized cache staging entry")
    );
    assert!(
        cache
            .recover_partial()
            .unwrap_err()
            .to_string()
            .contains("unrecognized cache staging entry")
    );
    assert!(cache.integrity().is_err());
    fs::remove_file(&stranger).unwrap();

    // A valid artifact survives recovery of an unrelated partial remnant.
    let data = payload(4);
    let receipt = cache.put(&s, &data).unwrap();
    fs::write(artifacts.join(format!(".partial-{}", "b".repeat(64))), b"x").unwrap();
    assert_eq!(cache.recover_partial().unwrap(), 1);
    assert_eq!(cache.get(&s).unwrap().unwrap().data, data);
    assert_eq!(cache.receipt(&s).unwrap().unwrap(), receipt);
    assert!(cache.integrity().is_ok());
}
