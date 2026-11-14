use rust_strategy_factory::{
    candles::{self, Timeframe},
    contracts::Profile,
    corpus::{self, Specification},
    data::{self, Dataset, Tick},
    native,
};
use std::{fs, path::PathBuf, process::Command};
struct Fixture {
    root: PathBuf,
    data: Dataset,
    spec: Specification,
}
impl Fixture {
    fn new(name: &str, symbol: &str, split: usize) -> Self {
        let root =
            std::env::temp_dir().join(format!("corpus-{name}-{symbol}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let p = Profile::synthetic(symbol).unwrap();
        let mut ticks = Vec::new();
        for i in 0..37u64 {
            let timestamp_ms = if i < 17 {
                (i / 2) as i64 * 60000
            } else {
                86400000 * 5 + (i / 2) as i64 * 60000
            };
            ticks.push(Tick {
                timestamp_ms,
                sequence: u64::MAX - 40 + i,
                bid: 10000 + i as i64 % 5,
                ask: 10010 + i as i64 % 7,
            });
        }
        let data = data::dataset(
            p.clone(),
            ticks,
            "synthetic corpus fixture",
            "fixture archive".into(),
            true,
        )
        .unwrap();
        let mut paths = Vec::new();
        for (i, ticks) in data.ticks.chunks(split).enumerate() {
            let d = data::dataset(
                p.clone(),
                ticks.to_vec(),
                &data.manifest.source,
                data.manifest.archive_hash.clone(),
                true,
            )
            .unwrap();
            let path = root.join(format!("{i}.rsf"));
            native::write(&path, &d).unwrap();
            paths.push(path);
        }
        let spec = corpus::describe(&paths, 100).unwrap();
        Self { root, data, spec }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn all_profiles_and_chunk_boundaries_preserve_exact_events_and_seven_candle_streams() {
    for symbol in rust_strategy_factory::contracts::SYMBOLS {
        for split in [1, 2, 7, 16, 37] {
            let f = Fixture::new(&format!("equivalence-{split}"), symbol, split);
            let report = corpus::inspect(&f.root, &f.spec).unwrap();
            assert_eq!(
                report.ordered_events_hash,
                data::hash(&f.data.ticks).unwrap()
            );
            assert_eq!(report.events, 37);
            assert_eq!(report.declared_class, data::DataClass::SyntheticTestOnly);
            assert_eq!(report.max_resident_chunk_events, split.min(37));
            assert_eq!(report.latest.sequence, (u64::MAX - 4).to_string());
            assert_eq!(report.gaps_over_hour, 1);
            for summary in report.candles {
                let expected = candles::build(&f.data, summary.timeframe).unwrap();
                assert_eq!(summary.closed_candles, expected.len() as u64);
                assert_eq!(summary.content_hash, data::hash(&expected).unwrap());
            }
        }
    }
}
#[test]
fn stream_refuses_bad_quotes_order_and_dates_without_advancing_state() {
    let f = Fixture::new("stream", "EURUSD", 7);
    for tf in Timeframe::ALL {
        let mut stream = candles::Stream::new(f.data.profile.clone(), tf).unwrap();
        let mut result = Vec::new();
        for tick in &f.data.ticks {
            let mut crossed = *tick;
            crossed.ask = 0;
            assert!(stream.push(crossed).is_err());
            let mut invalid_time = *tick;
            invalid_time.timestamp_ms = i64::MAX;
            assert!(stream.push(invalid_time).is_err());
            if let Some(c) = stream.push(*tick).unwrap() {
                result.push(c);
            }
            assert!(stream.push(*tick).is_err());
            let mut backwards = *tick;
            backwards.sequence += 1;
            backwards.timestamp_ms -= 1;
            assert!(stream.push(backwards).is_err());
        }
        assert_eq!(result, candles::build(&f.data, tf).unwrap());
    }
}
#[test]
fn source_revisions_reordering_mixed_profiles_and_oversized_chunks_fail_closed() {
    let f = Fixture::new("refusal", "EURUSD", 7);
    let mut spec = f.spec.clone();
    spec.chunks.swap(0, 1);
    assert!(corpus::inspect(&f.root, &spec).is_err());
    let mut spec = f.spec.clone();
    spec.chunks[0].file_hash = "0".repeat(64);
    assert!(corpus::inspect(&f.root, &spec).is_err());
    let mut spec = f.spec.clone();
    spec.chunks[0].dataset_identity = "0".repeat(64);
    assert!(corpus::inspect(&f.root, &spec).is_err());
    let mut spec = f.spec.clone();
    spec.max_chunk_events = 6;
    assert!(corpus::inspect(&f.root, &spec).is_err());
    for cap in [0, 100001, usize::MAX] {
        spec.max_chunk_events = cap;
        assert!(corpus::inspect(&f.root, &spec).is_err());
    }
    let second = PathBuf::from(&f.spec.chunks[1].path);
    let mut d = native::read(&second).unwrap();
    d.profile.research_only = false;
    d.manifest.profile_hash = data::hash(&d.profile).unwrap();
    let changed = f.root.join("profile-change.rsf");
    native::write(&changed, &d).unwrap();
    let mut spec = f.spec.clone();
    spec.chunks[1] = corpus::describe(&[changed], 100).unwrap().chunks.remove(0);
    assert!(corpus::inspect(&f.root, &spec).is_err());
    for (name, source_change) in [("source", true), ("class", false)] {
        let mut d = native::read(&second).unwrap();
        if source_change {
            d.manifest.source = "different source".into();
        } else {
            d.manifest.class = data::DataClass::RealStructurallyValid;
        }
        let path = f.root.join(format!("mixed-{name}.rsf"));
        native::write(&path, &d).unwrap();
        let mut spec = f.spec.clone();
        spec.chunks[1] = corpus::describe(&[path], 100).unwrap().chunks.remove(0);
        assert!(corpus::inspect(&f.root, &spec).is_err());
    }
    let bytes = fs::read(&second).unwrap();
    let mut changed_bytes = bytes.clone();
    changed_bytes[30] ^= 1;
    fs::write(&second, changed_bytes).unwrap();
    assert!(corpus::inspect(&f.root, &f.spec).is_err());
    fs::write(&second, bytes).unwrap();
    let mut truncated = f.spec.clone();
    truncated.chunks.clear();
    assert!(corpus::inspect(&f.root, &truncated).is_err());
}
#[test]
fn actual_cli_creation_inventory_and_relocation_preserve_identity_and_original_artifacts() {
    let f = Fixture::new("cli", "XAUUSD", 7);
    let spec_path = f.root.join("corpus.json");
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args(["corpus-create", spec_path.to_str().unwrap()])
        .args(f.spec.chunks.iter().map(|c| &c.path))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let before = fs::read(&spec_path).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args(["corpus-inspect", spec_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["ordered_events_hash"],
        data::hash(&f.data.ticks).unwrap()
    );
    assert_eq!(fs::read(&spec_path).unwrap(), before);
    let loaded = corpus::load(&spec_path).unwrap();
    let mut moved = loaded.clone();
    let id = loaded.identity().unwrap();
    let sub = f.root.join("moved");
    fs::create_dir(&sub).unwrap();
    for (i, c) in moved.chunks.iter_mut().enumerate() {
        let new = sub.join(format!("{i}.rsf"));
        fs::copy(&c.path, &new).unwrap();
        c.path = new.to_str().unwrap().into();
    }
    assert_eq!(moved.identity().unwrap(), id);
    assert_eq!(
        corpus::inspect(&f.root, &moved)
            .unwrap()
            .ordered_events_hash,
        report["ordered_events_hash"]
    );
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args(["corpus-create", spec_path.to_str().unwrap()])
        .args(f.spec.chunks.iter().map(|c| &c.path))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(fs::read(&spec_path).unwrap(), before);
    let refused = f.root.join("refused.json");
    let mut reversed = f.spec.chunks.iter().map(|c| &c.path).collect::<Vec<_>>();
    reversed.reverse();
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args(["corpus-create", refused.to_str().unwrap()])
        .args(reversed)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!refused.exists());
}
#[test]
fn native_caller_cap_precedes_allocation_and_consumed_file_fingerprint_matches() {
    let f = Fixture::new("native-budget", "USDJPY", 37);
    let path = PathBuf::from(&f.spec.chunks[0].path);
    assert!(native::read_bounded(&path, 36).is_err());
    let (d, h) = native::read_bounded(&path, 37).unwrap();
    assert_eq!(h, data::file_hash(&path).unwrap());
    assert_eq!(d.ticks, f.data.ticks);
    let mut bytes = fs::read(&path).unwrap();
    bytes[8..16].copy_from_slice(&2000000u64.to_le_bytes());
    let hostile = f.root.join("huge.rsf");
    fs::write(&hostile, bytes).unwrap();
    assert!(
        native::read_bounded(&hostile, 100)
            .unwrap_err()
            .contains("budget")
    );
}

#[test]
fn chunk_visitor_preserves_state_and_stops_before_invalid_or_failed_callbacks() {
    let f = Fixture::new("visitor", "EURUSD", 7);
    let mut stream = candles::Stream::new(f.data.profile.clone(), Timeframe::M1).unwrap();
    let mut candles = Vec::new();
    let mut count = 0;
    corpus::visit_chunks(&f.root, &f.spec, |_, d| {
        count += d.ticks.len();
        for tick in &d.ticks {
            if let Some(c) = stream.push(*tick)? {
                candles.push(c);
            }
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(count, f.data.ticks.len());
    assert_eq!(candles, candles::build(&f.data, Timeframe::M1).unwrap());
    let mut bad = f.spec.clone();
    bad.chunks[1].file_hash = "0".repeat(64);
    let mut visited = Vec::new();
    assert!(
        corpus::visit_chunks(&f.root, &bad, |i, _| {
            visited.push(i);
            Ok(())
        })
        .is_err()
    );
    assert_eq!(visited, vec![0]);
    visited.clear();
    assert!(
        corpus::visit_chunks(&f.root, &f.spec, |i, _| {
            visited.push(i);
            Err("consumer failure".into())
        })
        .is_err()
    );
    assert_eq!(visited, vec![0]);
}

#[test]
fn bounded_candle_context_matches_all_batch_frames_and_refuses_partial_outputs() {
    let f = Fixture::new("context", "EURUSD", 2);
    for tf in Timeframe::ALL {
        let expected = candles::build(&f.data, tf).unwrap();
        assert_eq!(
            corpus::candle_context(&f.root, &f.spec, tf, 100).unwrap(),
            expected
        );
        if !expected.is_empty() {
            assert_eq!(
                corpus::candle_context(&f.root, &f.spec, tf, expected.len()).unwrap(),
                expected
            );
            if expected.len() > 1 {
                assert!(corpus::candle_context(&f.root, &f.spec, tf, expected.len() - 1).is_err());
            }
        }
    }
    for cap in [0, 1_000_001, usize::MAX] {
        assert!(corpus::candle_context(&f.root, &f.spec, Timeframe::M1, cap).is_err());
    }
    let spec = f.root.join("spec.json");
    fs::write(&spec, serde_json::to_vec(&f.spec).unwrap()).unwrap();
    let output = f.root.join("candles.json");
    let result = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-candles",
            spec.to_str().unwrap(),
            "M1",
            "1",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!output.exists());
    let result = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-candles",
            spec.to_str().unwrap(),
            "M1",
            "100",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    let actual: Vec<candles::Candle> = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(actual, candles::build(&f.data, Timeframe::M1).unwrap());
}

#[test]
fn corpus_primary_signals_equal_batch_compilation_and_enforce_feature_budget() {
    use rust_strategy_factory::strategies::{Features, Rule, Strategy};
    let f = Fixture::new("signals", "EURUSD", 2);
    let strategy:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    let candles = candles::build(&f.data, Timeframe::M1).unwrap();
    let expected = Features::new(&candles, 10000)
        .unwrap()
        .signal(&strategy.rule)
        .unwrap();
    let actual = corpus::signals(&f.root, &f.spec, &strategy, 100, 10000).unwrap();
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(corpus::signals(&f.root, &f.spec, &strategy, 100, 1).is_err());
    let mut higher = strategy;
    higher.rule = Rule::HigherTimeframe {
        timeframe: Timeframe::H1,
        rule: Box::new(Rule::AboveMean { window: 2 }),
    };
    assert!(corpus::signals(&f.root, &f.spec, &higher, 100, 10000).is_err());
}
