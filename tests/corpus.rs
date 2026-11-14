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
    let expected = Features::with_context(&candles, 10000, &f.data, Timeframe::M1)
        .unwrap()
        .signal(&higher.rule)
        .unwrap();
    let actual = corpus::signals(&f.root, &f.spec, &higher, 100, 10000).unwrap();
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(corpus::signals(&f.root, &f.spec, &higher, 1, 10000).is_err());
    higher.rule = Rule::HigherTimeframe {
        timeframe: Timeframe::H1,
        rule: Box::new(Rule::UtcSession {
            start_hour: 0,
            end_hour: 23,
            weekdays: 127,
        }),
    };
    let expected = Features::with_context(&candles, 10000, &f.data, Timeframe::M1)
        .unwrap()
        .signal(&higher.rule)
        .unwrap();
    let actual = corpus::signals(&f.root, &f.spec, &higher, 100, 10000).unwrap();
    assert!(actual.count() > 0);
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[test]
fn corpus_window_keeps_prior_warmup_and_excludes_end_publication() {
    use rust_strategy_factory::{
        strategies::{Features, Strategy},
        validation::Interval,
    };
    let f = Fixture::new("window-context", "EURUSD", 2);
    let strategy:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    let all = candles::build(&f.data, Timeframe::M1).unwrap();
    for boundary in [1, 3, 8] {
        let end = all[boundary].available_ms;
        let prefix = &all[..all.partition_point(|c| c.available_ms < end)];
        let window = Interval {
            start_ms: end - 1,
            end_exclusive_ms: end,
        };
        let actual =
            corpus::signals_window(&f.root, &f.spec, &strategy, 100, 10000, &window).unwrap();
        let expected = Features::new(prefix, 10000)
            .unwrap()
            .signal(&strategy.rule)
            .unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
    assert!(
        corpus::signals_window(
            &f.root,
            &f.spec,
            &strategy,
            100,
            10000,
            &Interval {
                start_ms: 1,
                end_exclusive_ms: 1
            }
        )
        .is_err()
    );
}

#[test]
fn corpus_account_replay_matches_all_materialized_receipt_fields() {
    use rust_strategy_factory::{
        contracts::Conversion,
        strategies::{EntryPlan, Features, Strategy},
        tick_engine::{self, CorpusBudget, Costs},
    };
    let mut trades = 0;
    for symbol in ["EURUSD", "XAUUSD"] {
        let f = Fixture::new("account-parity", symbol, 2);
        for entry in [
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
        ] {
            let mut strategy:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
            strategy.entry = entry;
            let candles = candles::build(&f.data, Timeframe::M1).unwrap();
            let mask = Features::new(&candles, 10000)
                .unwrap()
                .signal(&strategy.rule)
                .unwrap();
            let reference = tick_engine::replay(
                &f.data,
                &candles,
                &mask,
                &strategy,
                &Costs::default(),
                &vec![Conversion::usd(); f.data.ticks.len()],
            )
            .unwrap();
            let actual = tick_engine::replay_corpus(
                &f.root,
                &f.spec,
                &strategy,
                &Costs::default(),
                CorpusBudget {
                    max_candles: 100,
                    feature_cells: 10000,
                    max_receipts: 10000,
                },
            )
            .unwrap();
            trades += actual.trades.len();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(reference).unwrap()
            );
            assert!(
                tick_engine::replay_corpus(
                    &f.root,
                    &f.spec,
                    &strategy,
                    &Costs::default(),
                    CorpusBudget {
                        max_candles: 100,
                        feature_cells: 1,
                        max_receipts: 10000
                    }
                )
                .is_err()
            );
        }
    }
    assert!(trades > 0);
    let f = Fixture::new("account-cli", "USDJPY", 2);
    let strategy:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    assert!(
        tick_engine::replay_corpus(
            &f.root,
            &f.spec,
            &strategy,
            &Costs::default(),
            CorpusBudget {
                max_candles: 100,
                feature_cells: 10000,
                max_receipts: 1000
            }
        )
        .is_err()
    );
    let spec = f.root.join("spec.json");
    let rule = f.root.join("rule.json");
    let result = f.root.join("result.json");
    fs::write(&spec, serde_json::to_vec(&f.spec).unwrap()).unwrap();
    fs::write(&rule, serde_json::to_vec(&strategy).unwrap()).unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-replay",
            spec.to_str().unwrap(),
            rule.to_str().unwrap(),
            "100",
            "10000",
            "1000",
            result.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!status.status.success());
    assert!(!result.exists());
}

#[test]
fn registered_corpus_identity_binds_exact_artifacts_policy_and_budgets() {
    use rust_strategy_factory::{
        storage::Registry,
        strategies::Strategy,
        tick_engine::{self, CorpusBudget, Costs},
    };
    let f = Fixture::new("frozen-run", "EURUSD", 2);
    let strategy:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    let budget = CorpusBudget {
        max_candles: 100,
        feature_cells: 10000,
        max_receipts: 1000,
    };
    let k =
        tick_engine::corpus_key(&f.root, &f.spec, &strategy, &Costs::default(), budget).unwrap();
    let mut r = Registry::open(&f.root.join("run.sqlite")).unwrap();
    assert_eq!(r.register(&k).unwrap(), r.register(&k).unwrap());
    let changed = tick_engine::corpus_key(
        &f.root,
        &f.spec,
        &strategy,
        &Costs::default(),
        CorpusBudget {
            feature_cells: 10001,
            ..budget
        },
    )
    .unwrap();
    assert_ne!(k.identity().unwrap(), changed.identity().unwrap());
    assert!(
        tick_engine::corpus_key(
            &f.root,
            &f.spec,
            &strategy,
            &Costs::default(),
            CorpusBudget {
                max_receipts: 0,
                ..budget
            }
        )
        .is_err()
    );
}

#[test]
fn source_bound_account_steps_reopen_and_match_complete_replay() {
    use rust_strategy_factory::{
        storage::{Claim, Registry},
        strategies::Strategy,
        tick_engine::{self, CorpusBudget, CorpusRun, Costs},
    };
    let f = Fixture::new("resume-steps", "EURUSD", 7);
    let costs = Costs::default();
    let strategy:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    let budget = CorpusBudget {
        max_candles: 100,
        feature_cells: 10000,
        max_receipts: 1000,
    };
    let key = tick_engine::corpus_key(&f.root, &f.spec, &strategy, &costs, budget).unwrap();
    let path = f.root.join("resume.sqlite");
    let mut r = Registry::open(&path).unwrap();
    let id = r.register(&key).unwrap();
    let Claim::Acquired(first) = r.claim(&id, 0, 10).unwrap() else {
        panic!()
    };
    let run = || CorpusRun {
        base: &f.root,
        spec: &f.spec,
        strategy: &strategy,
        costs: &costs,
        budget,
    };
    assert!(
        tick_engine::advance_corpus(&mut r, &first, run(), 1, || 1)
            .unwrap()
            .is_none()
    );
    drop(r);
    let mut r = Registry::open(&path).unwrap();
    let Claim::Acquired(lease) = r.claim(&id, 10, 10000).unwrap() else {
        panic!()
    };
    assert!(tick_engine::advance_corpus(&mut r, &first, run(), 1, || 11).is_err());
    let mut result = None;
    for _ in 0..f.spec.chunks.len() {
        result = tick_engine::advance_corpus(&mut r, &lease, run(), 1, || 11).unwrap();
        if result.is_some() {
            break;
        }
    }
    let reference =
        tick_engine::replay_corpus(&f.root, &f.spec, &strategy, &costs, budget).unwrap();
    assert_eq!(
        serde_json::to_value(result.unwrap()).unwrap(),
        serde_json::to_value(reference).unwrap()
    );
    assert_eq!(
        r.replay_checkpoint(&id).unwrap().unwrap().0.cursor as usize,
        f.spec.chunks.len()
    );
}
#[test]
fn hash_consistent_but_fabricated_account_checkpoint_is_not_trusted() {
    use rust_strategy_factory::{
        storage::{Claim, Registry},
        strategies::Strategy,
        tick_engine::{self, CorpusBudget, CorpusRun, Costs},
    };
    let f = Fixture::new("forged-account", "EURUSD", 7);
    let costs = Costs::default();
    let strategy:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    let budget = CorpusBudget {
        max_candles: 100,
        feature_cells: 10000,
        max_receipts: 1000,
    };
    let mut r = Registry::open(&f.root.join("forged.sqlite")).unwrap();
    let id = r
        .register(&tick_engine::corpus_key(&f.root, &f.spec, &strategy, &costs, budget).unwrap())
        .unwrap();
    let Claim::Acquired(lease) = r.claim(&id, 0, 10000).unwrap() else {
        panic!()
    };
    r.save_replay_checkpoint(
        &lease,
        1,
        0,
        1,
        serde_json::json!({"account":{"balance":999999999}}),
    )
    .unwrap();
    assert!(
        tick_engine::advance_corpus(
            &mut r,
            &lease,
            CorpusRun {
                base: &f.root,
                spec: &f.spec,
                strategy: &strategy,
                costs: &costs,
                budget
            },
            1,
            || 2
        )
        .is_err()
    );
    assert_eq!(r.replay_checkpoint(&id).unwrap().unwrap().0.cursor, 1);
}

#[test]
fn registered_corpus_driver_yields_publishes_once_and_reuses_exact_receipt() {
    use rust_strategy_factory::{
        storage::Registry,
        strategies::Strategy,
        tick_engine::{self, CorpusBudget, CorpusProgress, CorpusRun, Costs},
    };
    let f = Fixture::new("published-driver", "EURUSD", 7);
    let costs = Costs::default();
    let budget = CorpusBudget {
        max_candles: 100,
        feature_cells: 10000,
        max_receipts: 1000,
    };
    let strategy:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    let path = f.root.join("driver.sqlite");
    let mut result_hash = None;
    for _ in 0..f.spec.chunks.len() {
        let mut r = Registry::open(&path).unwrap();
        match tick_engine::execute_corpus_step(
            &mut r,
            CorpusRun {
                base: &f.root,
                spec: &f.spec,
                strategy: &strategy,
                costs: &costs,
                budget,
            },
            1,
            10000,
            || 1,
        )
        .unwrap()
        {
            CorpusProgress::Pending { experiment_id, .. } => {
                assert_eq!(r.counts().unwrap().running, 0, "yielded {experiment_id}")
            }
            CorpusProgress::Completed { cached, result, .. } => {
                assert!(!cached);
                result_hash = Some(result.result_hash);
                break;
            }
            _ => panic!("unexpected busy"),
        }
    }
    let mut r = Registry::open(&path).unwrap();
    let before = r.counts().unwrap().attempts;
    let CorpusProgress::Completed { cached, result, .. } = tick_engine::execute_corpus_step(
        &mut r,
        CorpusRun {
            base: &f.root,
            spec: &f.spec,
            strategy: &strategy,
            costs: &costs,
            budget,
        },
        1,
        10000,
        || 1,
    )
    .unwrap() else {
        panic!()
    };
    assert!(cached);
    assert_eq!(Some(result.result_hash), result_hash);
    assert_eq!(r.counts().unwrap().attempts, before);
    r.integrity().unwrap();
}

#[test]
fn separate_cli_processes_resume_registered_corpus_and_reuse_completion() {
    let f = Fixture::new("cli-step", "EURUSD", 7);
    let spec = f.root.join("spec.json");
    let strategy = f.root.join("strategy.json");
    let db = f.root.join("cli.sqlite");
    fs::write(&spec, serde_json::to_vec(&f.spec).unwrap()).unwrap();
    fs::write(
        &strategy,
        r#"{"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}}"#,
    )
    .unwrap();
    let call = || {
        let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args([
                "corpus-step",
                spec.to_str().unwrap(),
                strategy.to_str().unwrap(),
                db.to_str().unwrap(),
                "100",
                "10000",
                "1000",
                "1",
                "60000",
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let first = call();
    assert_eq!(first["Pending"]["cursor"], 1);
    let mut complete = None;
    for _ in 0..f.spec.chunks.len() {
        let v = call();
        if !v["Completed"].is_null() {
            complete = Some(v);
            break;
        }
    }
    let done = complete.unwrap();
    assert_eq!(done["Completed"]["cached"], false);
    let cached = call();
    assert_eq!(cached["Completed"]["cached"], true);
    assert_eq!(cached["Completed"]["result"], done["Completed"]["result"]);
}

#[test]
fn corpus_terminal_failures_are_not_success_and_invalid_budgets_do_not_register() {
    use rust_strategy_factory::{
        storage::{Claim, Outcome, Registry},
        strategies::Strategy,
        tick_engine::{self, CorpusBudget, CorpusProgress, CorpusRun, Costs},
    };
    let f = Fixture::new("terminal-driver", "EURUSD", 7);
    let costs = Costs::default();
    let budget = CorpusBudget {
        max_candles: 100,
        feature_cells: 10000,
        max_receipts: 1000,
    };
    let strategy:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    let run = || CorpusRun {
        base: &f.root,
        spec: &f.spec,
        strategy: &strategy,
        costs: &costs,
        budget,
    };
    let mut r = Registry::open(&f.root.join("terminal.sqlite")).unwrap();
    assert!(tick_engine::execute_corpus_step(&mut r, run(), 0, 1000, || 1).is_err());
    assert_eq!(r.counts().unwrap().registered, 0);
    let id = r
        .register(&tick_engine::corpus_key(&f.root, &f.spec, &strategy, &costs, budget).unwrap())
        .unwrap();
    let Claim::Acquired(lease) = r.claim(&id, 0, 1000).unwrap() else {
        panic!()
    };
    r.finish(
        &lease,
        1,
        Outcome::Failed,
        &serde_json::json!({"error":"fixture failure"}),
    )
    .unwrap();
    let CorpusProgress::Terminal { outcome, .. } =
        tick_engine::execute_corpus_step(&mut r, run(), 1, 1000, || 2).unwrap()
    else {
        panic!()
    };
    assert_eq!(outcome, "failed");
    assert_eq!(r.counts().unwrap().attempts, 1);
}
