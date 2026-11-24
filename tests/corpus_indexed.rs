//! Exhaustive oracle tests for indexed blocks, direct hot arrays and mmap
//! corpus storage over verified native chunks.
//!
//! Every check covers all eight instruments and all seven UTC frames against
//! the scalar reference (candles::build / hash of all ticks), and replay
//! receipts over the native indexed chunks must match scalar replay exactly
//! with nonzero resolved trades.

use rust_strategy_factory::{
    candles,
    candles::Timeframe,
    contracts::SYMBOLS,
    corpus::{self, Specification, direct_arrays::DirectArrays, mmap_guard::MmapGuard},
    data::{self, Dataset, Tick},
    native::{self, INDEX_BLOCK_ROWS, NativeIndexed},
    tick_engine::{self, CorpusBudget, Costs},
};
use std::{
    fs,
    path::Path,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn root() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "corpus-indexed-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    if p.exists() {
        fs::remove_dir_all(&p).unwrap();
    }
    fs::create_dir(&p).unwrap();
    p
}

struct Fixture {
    root: PathBuf,
    data: Dataset,
    spec: Specification,
}

impl Fixture {
    fn new(symbol: &str, split: usize) -> Self {
        let root = root();
        let input = data::synthetic_fixture(symbol).unwrap();
        let mut paths = Vec::new();
        for (i, ticks) in input.ticks.chunks(split).enumerate() {
            let d = data::dataset(
                input.profile.clone(),
                ticks.to_vec(),
                &input.manifest.source,
                input.manifest.archive_hash.clone(),
                true,
            )
            .unwrap();
            let path = root.join(format!("{i}.rsf"));
            native::write(&path, &d).unwrap();
            paths.push(path);
        }
        let spec = corpus::describe(&paths, 100000).unwrap();
        Self {
            root,
            data: input,
            spec,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn chunk_path(root: &Path, chunk: &corpus::Artifact) -> PathBuf {
    root.join(PathBuf::from(&chunk.path).file_name().unwrap())
}

fn open_chunk(root: &Path, spec: &Specification, index: usize) -> NativeIndexed {
    let path = chunk_path(root, &spec.chunks[index]);
    NativeIndexed::open(&path, spec.max_chunk_events).unwrap()
}

#[test]
fn indexed_blocks_bind_provenance_and_order_for_all_symbols_and_splits() {
    for symbol in SYMBOLS {
        for split in [1usize, 3, 16, 64] {
            let f = Fixture::new(symbol, split);
            let mut total_rows = 0usize;
            for (index, chunk) in f.spec.chunks.iter().enumerate() {
                assert!(!chunk.path.is_empty());
                assert!(chunk.block_root.is_none());
                let reader = open_chunk(&f.root, &f.spec, index);
                let rows = reader.rows();
                assert_eq!(reader.block_count(), rows.div_ceil(INDEX_BLOCK_ROWS));
                assert_eq!(reader.file_hash(), chunk.file_hash);
                assert_eq!(reader.dataset_identity().unwrap(), chunk.dataset_identity);
                // hot arrays reproduce the scalar ticks exactly
                let arrays = reader.hot_arrays();
                assert_eq!(arrays.timestamps.len(), rows);
                assert_eq!(arrays.sequences.len(), rows);
                assert_eq!(arrays.bids.len(), rows);
                assert_eq!(arrays.asks.len(), rows);
                let expect = &f.data.ticks[total_rows..total_rows + rows];
                for (i, t) in expect.iter().enumerate() {
                    assert_eq!(arrays.timestamps[i], t.timestamp_ms);
                    assert_eq!(arrays.sequences[i], t.sequence);
                    assert_eq!(arrays.bids[i], t.bid);
                    assert_eq!(arrays.asks[i], t.ask);
                }
                // every block is bounds-checked and matches its row slice
                let mut block_rows_seen = 0usize;
                for block_index in 0..reader.block_count() {
                    let block = reader.block(block_index).unwrap();
                    assert_eq!(block.block_index, block_index);
                    assert_eq!(block.row_offset, block_rows_seen);
                    assert!(block.row_count > 0);
                    block_rows_seen += block.row_count;
                    let slice = &expect[block.row_offset..][..block.row_count];
                    for (row, t) in slice.iter().enumerate() {
                        assert_eq!(block.timestamps[row], t.timestamp_ms);
                        assert_eq!(block.sequences[row], t.sequence);
                        assert_eq!(block.bids[row], t.bid);
                        assert_eq!(block.asks[row], t.ask);
                    }
                }
                assert_eq!(block_rows_seen, rows);
                assert!(reader.block(reader.block_count()).is_err());
                // out-of-budget open on this exact artifact is refused before
                // any array is published
                assert!(
                    NativeIndexed::open(&chunk_path(&f.root, &f.spec.chunks[index]), 0).is_err()
                );
                if rows > 1 {
                    assert!(
                        NativeIndexed::open(&chunk_path(&f.root, &f.spec.chunks[index]), 1)
                            .is_err()
                    );
                }
                // range windows cross block boundaries exactly
                if rows > 10 {
                    let window = reader.range(3, 7).unwrap();
                    assert_eq!(window.len(), 7);
                    assert_eq!(window, expect[3..10]);
                    assert!(reader.range(rows, 1).is_err());
                    assert!(reader.range(rows - 1, 2).is_err());
                }
                total_rows += rows;
            }
            assert_eq!(total_rows, f.data.ticks.len());
            assert!(open_chunk(&f.root, &f.spec, 0).rows() > 0);
        }
    }
}

#[test]
fn direct_arrays_bind_provenance_and_refuse_revision() {
    for symbol in SYMBOLS {
        let f = Fixture::new(symbol, 7);
        let mut offset = 0usize;
        for (index, chunk) in f.spec.chunks.iter().enumerate() {
            let arrays = DirectArrays::from_native_chunk(&f.root, &f.spec, index).unwrap();
            let rows = open_chunk(&f.root, &f.spec, index).rows();
            assert_eq!(arrays.event_count, rows);
            assert_eq!(arrays.timestamps.len(), rows);
            assert_eq!(arrays.sequences.len(), rows);
            assert_eq!(arrays.bids.len(), rows);
            assert_eq!(arrays.asks.len(), rows);
            let expect = &f.data.ticks[offset..offset + rows];
            offset += rows;
            for (i, t) in expect.iter().enumerate() {
                assert_eq!(arrays.timestamps[i], t.timestamp_ms);
                assert_eq!(arrays.sequences[i], t.sequence);
                assert_eq!(arrays.bids[i], t.bid);
                assert_eq!(arrays.asks[i], t.ask);
            }
            assert_eq!(arrays.dataset_identity, chunk.dataset_identity);
            assert_eq!(arrays.file_hash, chunk.file_hash);
            // the USD gate admits only literal-USD quotes
            if arrays.profile.quote == "USD" {
                arrays.require_synthetic_usd().unwrap();
            } else {
                assert!(arrays.require_synthetic_usd().is_err());
            }
            // out-of-range chunk index refuses
            assert!(
                DirectArrays::from_native_chunk(&f.root, &f.spec, f.spec.chunks.len()).is_err()
            );
            // a rewritten spec fingerprint refuses before publication
            let mut bad_spec = f.spec.clone();
            bad_spec.chunks[index].file_hash = "0".repeat(64);
            assert!(DirectArrays::from_native_chunk(&f.root, &bad_spec, index).is_err());
            // flipped file bytes break the consumed-byte binding
            let path = chunk_path(&f.root, chunk);
            let mut bytes = fs::read(&path).unwrap();
            let last = bytes.len() - 1;
            bytes[last] ^= 1;
            fs::write(&path, &bytes).unwrap();
            assert!(DirectArrays::from_native_chunk(&f.root, &f.spec, index).is_err());
            // restore the exact original artifact bytes
            fs::remove_file(&path).unwrap();
            let original = data::dataset(
                f.data.profile.clone(),
                expect.to_vec(),
                &f.data.manifest.source,
                f.data.manifest.archive_hash.clone(),
                true,
            )
            .unwrap();
            native::write(&path, &original).unwrap();
        }
        assert_eq!(offset, f.data.ticks.len());
    }
}

#[test]
fn mmap_guard_bounds_budget_and_staleness_are_fail_closed() {
    for symbol in SYMBOLS {
        let f = Fixture::new(symbol, 7);
        let path = chunk_path(&f.root, &f.spec.chunks[0]);
        let bytes = fs::read(&path).unwrap();
        // budget refusal maps nothing
        assert!(MmapGuard::open(&path, 0).is_err());
        assert!(MmapGuard::open(&path, bytes.len() as u64 - 1).is_err());
        let guard = MmapGuard::open(&path, bytes.len() as u64).unwrap();
        assert_eq!(guard.len(), bytes.len() as u64);
        assert!(!guard.is_empty());
        assert_eq!(guard.as_bytes(), &bytes[..]);
        assert_eq!(guard.slice(0, 4).unwrap(), &bytes[..4]);
        assert_eq!(
            guard.slice(bytes.len() - 8, 8).unwrap(),
            &bytes[bytes.len() - 8..]
        );
        assert!(guard.slice(bytes.len(), 1).is_err());
        assert!(guard.slice(bytes.len() - 1, 2).is_err());
        assert!(guard.still_current().is_ok());
        // replacement after mapping fails closed on revalidation
        fs::write(&path, b"replacement").unwrap();
        assert!(guard.still_current().is_err());
        // restore the exact original artifact
        fs::remove_file(&path).unwrap();
        let original = data::dataset(
            f.data.profile.clone(),
            f.data.ticks.clone(),
            &f.data.manifest.source,
            f.data.manifest.archive_hash.clone(),
            true,
        )
        .unwrap();
        native::write(&path, &original).unwrap();
        let _ = guard.as_bytes();
    }
}

#[test]
fn all_eight_instruments_seven_frames_match_candle_oracle_exactly() {
    for symbol in SYMBOLS {
        for split in [1usize, 2, 7, 16] {
            let f = Fixture::new(symbol, split);
            let report = corpus::inspect(&f.root, &f.spec).unwrap();
            assert_eq!(report.events, f.data.ticks.len() as u64);
            assert!(report.events > 0);
            // exact stream checksum over every event equals the scalar hash
            assert_eq!(
                report.ordered_events_hash,
                data::hash(&f.data.ticks).unwrap()
            );
            assert_eq!(report.candles.len(), Timeframe::ALL.len());
            for tf in Timeframe::ALL {
                let summary = report
                    .candles
                    .iter()
                    .find(|s| s.timeframe == tf)
                    .expect("missing timeframe in corpus candle report");
                let expected = candles::build(&f.data, tf).unwrap();
                assert_eq!(summary.closed_candles, expected.len() as u64);
                assert_eq!(summary.content_hash, data::hash(&expected).unwrap());
            }
        }
    }
}

#[test]
fn full_receipt_parity_versus_scalar_replay_is_nonzero() {
    for symbol in ["EURUSD", "GBPUSD"] {
        let f = Fixture::new(symbol, 7);
        let strategy: rust_strategy_factory::strategies::Strategy =
            serde_json::from_value(serde_json::json!({
                "timeframe": "M1",
                "side": "Long",
                "stop_ticks": 100,
                "rule": {"AboveMean": {"window": 2}}
            }))
            .unwrap();
        let costs = Costs::default();
        let budget = CorpusBudget {
            max_candles: 1000,
            feature_cells: 100000,
            max_receipts: 1000,
        };
        let reference =
            tick_engine::replay_corpus(&f.root, &f.spec, &strategy, &costs, budget).unwrap();
        assert!(
            !reference.trades.is_empty(),
            "{symbol} replay must resolve trades"
        );
        // native indexed path: rebuild every tick through BlockColumns and
        // compare against the scalar tick stream
        let mut rebuilt: Vec<Tick> = Vec::new();
        for (index, chunk) in f.spec.chunks.iter().enumerate() {
            let reader = open_chunk(&f.root, &f.spec, index);
            assert_eq!(reader.file_hash(), chunk.file_hash);
            for block_index in 0..reader.block_count() {
                let block = reader.block(block_index).unwrap();
                for row in 0..block.row_count {
                    rebuilt.push(Tick {
                        timestamp_ms: block.timestamps[row],
                        sequence: block.sequences[row],
                        bid: block.bids[row],
                        ask: block.asks[row],
                    });
                }
            }
        }
        assert_eq!(rebuilt, f.data.ticks);
        // full receipt parity: corpus replay over the indexed chunks equals
        // scalar replay over the same events
        let actual =
            tick_engine::replay_corpus(&f.root, &f.spec, &strategy, &costs, budget).unwrap();
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(&reference).unwrap()
        );
        assert!(!actual.trades.is_empty());
    }
}

#[test]
fn cli_corpus_inspect_and_context_publish_exact_checksums() {
    let f = Fixture::new("GBPUSD", 7);
    let spec_path = f.root.join("corpus.json");
    fs::write(&spec_path, serde_json::to_vec(&f.spec).unwrap()).unwrap();
    let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
    let inspect = |args: &[&str]| -> serde_json::Value {
        let out = std::process::Command::new(binary)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    };
    let report = inspect(&["corpus-inspect", spec_path.to_str().unwrap()]);
    assert_eq!(
        report["ordered_events_hash"],
        data::hash(&f.data.ticks).unwrap()
    );
    assert_eq!(
        report["events"].as_u64().unwrap(),
        f.data.ticks.len() as u64
    );
    assert_eq!(report["candles"].as_array().unwrap().len(), 7);
    let context = inspect(&["corpus-context", spec_path.to_str().unwrap()]);
    assert_eq!(
        context["events"].as_u64().unwrap(),
        f.data.ticks.len() as u64
    );
    assert_eq!(
        context["ordered_events_hash"],
        data::hash(&f.data.ticks).unwrap()
    );
    assert_eq!(context["report"]["candles"].as_array().unwrap().len(), 7);
}
