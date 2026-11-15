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

#[test]
fn paused_corpus_backup_restores_without_mutating_original_job() {
    use rust_strategy_factory::storage::Registry;
    let f = Fixture::new("paused-backup", "EURUSD", 7);
    let spec = f.root.join("spec.json");
    let strategy = f.root.join("strategy.json");
    let db = f.root.join("original.sqlite");
    let backup = f.root.join("backup.sqlite");
    fs::write(&spec, serde_json::to_vec(&f.spec).unwrap()).unwrap();
    fs::write(
        &strategy,
        r#"{"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}}"#,
    )
    .unwrap();
    let call = |db: &std::path::Path| {
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
    let first = call(&db);
    let id = first["Pending"]["experiment_id"].as_str().unwrap();
    let mut r = Registry::open(&db).unwrap();
    r.set_corpus_control(id, "paused", 1).unwrap();
    let hash = r.replay_checkpoint(id).unwrap().unwrap().1;
    r.backup(&backup).unwrap();
    let held = call(&backup);
    assert_eq!(held["Held"]["state"], "paused");
    let mut restored = Registry::open(&backup).unwrap();
    assert_eq!(restored.replay_checkpoint(id).unwrap().unwrap().1, hash);
    restored.set_corpus_control(id, "running", 2).unwrap();
    drop(restored);
    let mut done = false;
    for _ in 0..f.spec.chunks.len() {
        if !call(&backup)["Completed"].is_null() {
            done = true;
            break;
        }
    }
    assert!(done);
    assert_eq!(r.corpus_control(id).unwrap(), "paused");
    assert_eq!(r.replay_checkpoint(id).unwrap().unwrap().0.cursor, 1);
}
#[test]
#[cfg(unix)]
fn killed_corpus_process_waiting_for_writer_preserves_committed_checkpoint() {
    use rust_strategy_factory::storage::Registry;
    use std::os::unix::process::ExitStatusExt;
    let f = Fixture::new("killed-resume", "EURUSD", 7);
    let spec = f.root.join("spec.json");
    let strategy = f.root.join("strategy.json");
    let db = f.root.join("kill.sqlite");
    fs::write(&spec, serde_json::to_vec(&f.spec).unwrap()).unwrap();
    fs::write(
        &strategy,
        r#"{"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}}"#,
    )
    .unwrap();
    let args = [
        "corpus-step",
        spec.to_str().unwrap(),
        strategy.to_str().unwrap(),
        db.to_str().unwrap(),
        "100",
        "10000",
        "1000",
        "1",
        "60000",
    ];
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success());
    let first: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = first["Pending"]["experiment_id"].as_str().unwrap();
    let r = Registry::open(&db).unwrap();
    let hash = r.replay_checkpoint(id).unwrap().unwrap().1;
    drop(r);
    let lock = rusqlite::Connection::open(&db).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    child.kill().unwrap();
    assert_eq!(child.wait().unwrap().signal(), Some(9));
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);
    let r = Registry::open(&db).unwrap();
    assert_eq!(r.replay_checkpoint(id).unwrap().unwrap().1, hash);
    r.integrity().unwrap();
    drop(r);
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success());
    let next: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(next["Pending"]["cursor"], 2);
}

#[test]
fn parallel_corpus_rounds_match_serial_receipts_and_reject_aggregate_overcommit() {
    use rust_strategy_factory::{
        storage::Registry,
        strategies::{Rule, Strategy},
        tick_engine::{self, CorpusBatchLimits, CorpusBudget, CorpusProgress, CorpusRun, Costs},
    };
    let f = Fixture::new("parallel-rounds", "EURUSD", 7);
    let costs = Costs::default();
    let budget = CorpusBudget {
        max_candles: 100,
        feature_cells: 10000,
        max_receipts: 1000,
    };
    let strategies: Vec<_> = [90, 100, 110]
        .into_iter()
        .map(|stop_ticks| Strategy {
            timeframe: Timeframe::M1,
            side: rust_strategy_factory::contracts::Side::Long,
            stop_ticks,
            rule: Rule::AboveMean { window: 2 },
            entry: None,
            stop_policy: None,
        })
        .collect();
    let runs: Vec<_> = strategies
        .iter()
        .map(|strategy| CorpusRun {
            base: &f.root,
            spec: &f.spec,
            strategy,
            costs: &costs,
            budget,
        })
        .collect();
    let db = f.root.join("batch.sqlite");
    let limits = CorpusBatchLimits {
        workers: 2,
        max_parallel_candles: 200,
        max_parallel_feature_cells: 20000,
        max_parallel_receipts: 2000,
        max_new_chunks: 1,
        lease_ms: 60000,
    };
    let mut bad = limits;
    bad.max_parallel_feature_cells = 19999;
    assert!(tick_engine::execute_corpus_batch(&db, &runs, bad).is_err());
    assert!(!db.exists());
    let duplicates = [
        CorpusRun {
            base: &f.root,
            spec: &f.spec,
            strategy: &strategies[0],
            costs: &costs,
            budget,
        },
        CorpusRun {
            base: &f.root,
            spec: &f.spec,
            strategy: &strategies[0],
            costs: &costs,
            budget,
        },
    ];
    assert!(tick_engine::execute_corpus_batch(&db, &duplicates, limits).is_err());
    assert!(!db.exists());
    for round in 0..f.spec.chunks.len() {
        let report = tick_engine::execute_corpus_batch(&db, &runs, limits).unwrap();
        assert_eq!(report.workers_started, 2);
        assert_eq!(report.worker_failures, 0);
        assert_eq!(report.jobs.len(), 3);
        for (i, outcome) in report.jobs.into_iter().enumerate() {
            match outcome.unwrap() {
                CorpusProgress::Pending { cursor, .. } => assert_eq!(cursor, round as i64 + 1),
                CorpusProgress::Completed { result, cached, .. } => {
                    assert!(!cached);
                    assert_eq!(
                        result.result["tick_replay"],
                        serde_json::to_value(
                            tick_engine::replay_corpus(
                                &f.root,
                                &f.spec,
                                &strategies[i],
                                &costs,
                                budget
                            )
                            .unwrap()
                        )
                        .unwrap()
                    );
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    }
    let cached = tick_engine::execute_corpus_batch(&db, &runs, limits).unwrap();
    assert!(
        cached
            .jobs
            .iter()
            .all(|r| matches!(r, Ok(CorpusProgress::Completed { cached: true, .. })))
    );
    Registry::open(&db).unwrap().integrity().unwrap();
}

#[test]
fn corpus_workers_report_job_failures_without_losing_other_checkpoints() {
    use rust_strategy_factory::{
        storage::Registry,
        strategies::{Rule, Strategy},
        tick_engine::{self, CorpusBatchLimits, CorpusBudget, CorpusProgress, CorpusRun, Costs},
    };
    let f = Fixture::new("parallel-errors", "EURUSD", 7);
    let costs = Costs::default();
    let strategy = Strategy {
        timeframe: Timeframe::M1,
        side: rust_strategy_factory::contracts::Side::Long,
        stop_ticks: 100,
        rule: Rule::AboveMean { window: 2 },
        entry: None,
        stop_policy: None,
    };
    let good = CorpusBudget {
        max_candles: 100,
        feature_cells: 10000,
        max_receipts: 1000,
    };
    let mut bad = good;
    bad.feature_cells = 1;
    let runs = [
        CorpusRun {
            base: &f.root,
            spec: &f.spec,
            strategy: &strategy,
            costs: &costs,
            budget: bad,
        },
        CorpusRun {
            base: &f.root,
            spec: &f.spec,
            strategy: &strategy,
            costs: &costs,
            budget: good,
        },
    ];
    let db = f.root.join("errors.sqlite");
    let report = tick_engine::execute_corpus_batch(
        &db,
        &runs,
        CorpusBatchLimits {
            workers: 2,
            max_parallel_candles: 200,
            max_parallel_feature_cells: 10001,
            max_parallel_receipts: 2000,
            max_new_chunks: 1,
            lease_ms: 60000,
        },
    )
    .unwrap();
    assert_eq!(report.worker_failures, 0);
    assert!(report.jobs[0].is_err());
    assert!(matches!(
        &report.jobs[1],
        Ok(CorpusProgress::Pending { cursor: 1, .. })
    ));
    Registry::open(&db).unwrap().integrity().unwrap();
}

fn corpus_batch_fixture(f: &Fixture) -> std::path::PathBuf {
    let spec = f.root.join("corpus.json");
    fs::write(&spec, serde_json::to_vec(&f.spec).unwrap()).unwrap();
    let config = f.root.join("batch.json");
    fs::write(&config,serde_json::to_vec(&serde_json::json!({"specification":"corpus.json","strategies":[{"timeframe":"M1","side":"Long","stop_ticks":90,"rule":{"AboveMean":{"window":2}}},{"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}}],"budget":{"max_candles":100,"feature_cells":10000,"max_receipts":1000},"limits":{"workers":2,"max_parallel_candles":200,"max_parallel_feature_cells":20000,"max_parallel_receipts":2000,"max_new_chunks":1,"lease_ms":60000}})).unwrap()).unwrap();
    config
}
#[test]
fn batch_cli_resumes_multiple_jobs_across_processes_and_stops_when_paused() {
    use rust_strategy_factory::storage::Registry;
    let f = Fixture::new("batch-cli", "EURUSD", 7);
    let config = corpus_batch_fixture(&f);
    let db = f.root.join("batch.sqlite");
    let call = |rounds: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args([
                "corpus-batch",
                config.to_str().unwrap(),
                db.to_str().unwrap(),
                rounds,
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
    let first = call("1");
    assert_eq!(first["stop_reason"], "round-limit");
    assert_eq!(first["rounds"], 1);
    let ids: Vec<_> = first["last"]["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| {
            j["Ok"]["Pending"]["experiment_id"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    let mut r = Registry::open(&db).unwrap();
    for id in &ids {
        r.set_corpus_control(id, "paused", 1).unwrap();
    }
    drop(r);
    let held = call("100");
    assert_eq!(held["rounds"], 1);
    assert_eq!(held["stop_reason"], "held-busy-or-terminal");
    let mut r = Registry::open(&db).unwrap();
    for id in &ids {
        assert_eq!(r.replay_checkpoint(id).unwrap().unwrap().0.cursor, 1);
        r.set_corpus_control(id, "running", 2).unwrap();
    }
    drop(r);
    let done = call("100");
    assert_eq!(done["stop_reason"], "complete");
    assert_eq!(done["rounds"], f.spec.chunks.len() - 1);
    let cached = call("100");
    assert_eq!(cached["rounds"], 1);
    assert!(
        cached["last"]["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|j| j["Ok"]["Completed"]["cached"] == true)
    );
}
#[test]
#[cfg(unix)]
fn batch_cli_sigterm_finishes_one_bounded_round_and_remains_resumable() {
    use rust_strategy_factory::storage::Registry;
    use std::io::BufRead;
    let f = Fixture::new("batch-signal", "EURUSD", 7);
    let config = corpus_batch_fixture(&f);
    let db = f.root.join("signal.sqlite");
    drop(Registry::open(&db).unwrap());
    let lock = rusqlite::Connection::open(&db).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-batch",
            config.to_str().unwrap(),
            db.to_str().unwrap(),
            "10000",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    std::io::BufReader::new(child.stderr.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert!(ready.contains("corpus batch ready"));
    assert!(
        Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    std::thread::sleep(std::time::Duration::from_millis(20));
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["stop_reason"], "signal");
    assert!(report["rounds"].as_u64().unwrap() <= 1);
    Registry::open(&db).unwrap().integrity().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-batch",
            config.to_str().unwrap(),
            db.to_str().unwrap(),
            "100",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["stop_reason"],
        "complete"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn corpus_resource_pressure_exits_before_claim_and_resumes_after_policy_change() {
    let f = Fixture::new("resource-guard", "EURUSD", 7);
    let config = corpus_batch_fixture(&f);
    let db = f.root.join("guard.sqlite");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    let run = || {
        let output = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args([
                "corpus-batch",
                config.to_str().unwrap(),
                db.to_str().unwrap(),
                "100",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    value["resource_guard"] =
        serde_json::json!({"maximum_process_rss_bytes":1,"minimum_host_available_memory_bytes":0});
    fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    let held = run();
    assert_eq!(held["stop_reason"], "resource-pressure");
    assert_eq!(held["rounds"], 0);
    assert_eq!(held["admission_observation"]["allowed"], false);
    assert!(!db.exists());
    value["resource_guard"] = serde_json::json!({"maximum_process_rss_bytes":u64::MAX,"minimum_host_available_memory_bytes":u64::MAX});
    fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    let held = run();
    assert_eq!(held["stop_reason"], "resource-pressure");
    assert_eq!(
        held["admission_observation"]["reasons"][0],
        "host-memory-below-reserve"
    );
    assert!(!db.exists());
    value["resource_guard"] = serde_json::json!({"maximum_process_rss_bytes":u64::MAX,"minimum_host_available_memory_bytes":0});
    fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    let done = run();
    assert_eq!(done["stop_reason"], "complete");
    assert_eq!(done["admission_observation"]["allowed"], true);
    assert!(
        done["admission_observation"]["observed"]["process_rss_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    rust_strategy_factory::storage::Registry::open(&db)
        .unwrap()
        .integrity()
        .unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .arg("resource-status")
        .output()
        .unwrap();
    assert!(status.status.success());
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert!(status["available_parallelism"].as_u64().unwrap() > 0);
    assert!(status["process_rss_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn durable_corpus_search_reopens_drains_then_replenishes_and_matches_scalar_receipts() {
    use rust_strategy_factory::{
        corpus_search::{self, SearchRun},
        storage::Registry,
        tick_engine::{self, CorpusBatchLimits, CorpusBudget, Costs},
    };
    let f = Fixture::new("durable-search", "EURUSD", 7);
    let db = f.root.join("search.sqlite");
    let costs = Costs::default();
    let budget = CorpusBudget {
        max_candles: 100,
        feature_cells: 10000,
        max_receipts: 1000,
    };
    let limits = CorpusBatchLimits {
        workers: 2,
        max_parallel_candles: 200,
        max_parallel_feature_cells: 20000,
        max_parallel_receipts: 2000,
        max_new_chunks: 1,
        lease_ms: 60000,
    };
    let run = || SearchRun {
        base: &f.root,
        specification: &f.spec,
        costs: &costs,
        budget,
        seed: 42,
        maximum_candidates: 4,
        queue_width: 2,
    };
    let first = corpus_search::step(&db, run(), limits).unwrap();
    let id = first.status["search_id"].as_str().unwrap().to_owned();
    assert_eq!(first.status["generated_slots"], "2");
    assert_eq!(first.status["slot_states"]["pending"], 2);
    assert_eq!(first.status["lease_active"], false);
    let mut registry = Registry::open(&db).unwrap();
    registry.integrity().unwrap();
    corpus_search::control(&mut registry, &id, "paused").unwrap();
    drop(registry);
    let held = corpus_search::step(&db, run(), limits).unwrap();
    assert_eq!(held.kind, "held");
    assert_eq!(held.status["generated_slots"], "2");
    let mut registry = Registry::open(&db).unwrap();
    corpus_search::control(&mut registry, &id, "running").unwrap();
    drop(registry);
    let mut exhausted = false;
    for _ in 0..30 {
        let step = corpus_search::step(&db, run(), limits).unwrap();
        if step.kind == "exhausted" {
            assert_eq!(step.status["generated_slots"], "4");
            assert_eq!(step.status["slot_states"]["completed"], 4);
            assert_eq!(step.status["qualification_granted"], false);
            exhausted = true;
            break;
        }
    }
    assert!(exhausted);
    let registry = Registry::open(&db).unwrap();
    let attempts = registry.counts().unwrap().attempts;
    for slot in 0..4 {
        let (_, strategy) = rust_strategy_factory::research::candidate_at(slot, 42).unwrap();
        let key = tick_engine::corpus_key(&f.root, &f.spec, &strategy, &costs, budget).unwrap();
        let result = registry.result(&key.identity().unwrap()).unwrap().unwrap();
        assert_eq!(
            result.result["tick_replay"],
            serde_json::to_value(
                tick_engine::replay_corpus(&f.root, &f.spec, &strategy, &costs, budget).unwrap()
            )
            .unwrap()
        );
    }
    registry.integrity().unwrap();
    drop(registry);
    assert_eq!(
        corpus_search::step(&db, run(), limits).unwrap().kind,
        "exhausted"
    );
    assert_eq!(
        Registry::open(&db).unwrap().counts().unwrap().attempts,
        attempts
    );
}
#[test]
fn durable_search_backup_and_terminal_controls_preserve_original_queue() {
    use rust_strategy_factory::{
        corpus_search::{self, SearchRun},
        storage::Registry,
        tick_engine::{CorpusBatchLimits, CorpusBudget, Costs},
    };
    let f = Fixture::new("search-backup", "EURUSD", 7);
    let db = f.root.join("original.sqlite");
    let backup = f.root.join("backup.sqlite");
    let costs = Costs::default();
    let budget = CorpusBudget {
        max_candles: 100,
        feature_cells: 10000,
        max_receipts: 1000,
    };
    let limits = CorpusBatchLimits {
        workers: 2,
        max_parallel_candles: 200,
        max_parallel_feature_cells: 20000,
        max_parallel_receipts: 2000,
        max_new_chunks: 1,
        lease_ms: 60000,
    };
    let run = || SearchRun {
        base: &f.root,
        specification: &f.spec,
        costs: &costs,
        budget,
        seed: 42,
        maximum_candidates: 2,
        queue_width: 2,
    };
    let first = corpus_search::step(&db, run(), limits).unwrap();
    let id = first.status["search_id"].as_str().unwrap();
    let mut registry = Registry::open(&db).unwrap();
    corpus_search::control(&mut registry, id, "paused").unwrap();
    registry.backup(&backup).unwrap();
    let mut restored = Registry::open(&backup).unwrap();
    assert_eq!(
        corpus_search::status(&restored, id).unwrap()["state"],
        "paused"
    );
    corpus_search::control(&mut restored, id, "running").unwrap();
    drop(restored);
    let mut done = false;
    for _ in 0..10 {
        if corpus_search::step(&backup, run(), limits).unwrap().kind == "exhausted" {
            done = true;
            break;
        }
    }
    assert!(done);
    assert_eq!(
        corpus_search::status(&registry, id).unwrap()["slot_states"]["pending"],
        2
    );
    corpus_search::control(&mut registry, id, "stopped").unwrap();
    assert!(corpus_search::control(&mut registry, id, "running").is_err());
    assert_eq!(
        corpus_search::step(&db, run(), limits).unwrap().kind,
        "held"
    );
    registry.integrity().unwrap();
}
#[test]
fn corpus_search_worker_errors_pause_without_becoming_completed_evidence() {
    use rust_strategy_factory::{
        corpus_search::{self, SearchRun},
        storage::Registry,
        tick_engine::{CorpusBatchLimits, CorpusBudget, Costs},
    };
    let f = Fixture::new("search-errors", "EURUSD", 7);
    let db = f.root.join("errors.sqlite");
    let costs = Costs::default();
    let progress = corpus_search::step(
        &db,
        SearchRun {
            base: &f.root,
            specification: &f.spec,
            costs: &costs,
            budget: CorpusBudget {
                max_candles: 100,
                feature_cells: 1,
                max_receipts: 1000,
            },
            seed: 42,
            maximum_candidates: 2,
            queue_width: 2,
        },
        CorpusBatchLimits {
            workers: 2,
            max_parallel_candles: 200,
            max_parallel_feature_cells: 2,
            max_parallel_receipts: 2000,
            max_new_chunks: 1,
            lease_ms: 60000,
        },
    )
    .unwrap();
    assert_eq!(progress.status["state"], "paused");
    assert_eq!(progress.status["control"]["pause_origin"], "worker_error");
    assert_eq!(progress.status["control"]["epoch"], "1");
    assert!(
        progress.status["recent_slots"]
            .as_array()
            .unwrap()
            .iter()
            .all(|j| j["error_kind"] == "unclassified" && j["retry_candidate"] == false)
    );
    assert_eq!(progress.status["slot_states"]["error"], 2);
    assert_eq!(progress.status["qualification_granted"], false);
    assert_eq!(progress.status["lease_active"], false);
    Registry::open(&db).unwrap().integrity().unwrap();
    let id = progress.status["search_id"].as_str().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args(["corpus-search-requeue", db.to_str().unwrap(), id, "1"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let queued: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(queued["requeued_slots"].as_array().unwrap().len(), 1);
    let mut registry = Registry::open(&db).unwrap();
    let snapshot = corpus_search::status(&registry, id).unwrap();
    assert_eq!(snapshot["requeue_records"], "1");
    assert!(snapshot["control"]["pause_origin"].is_null());
    assert_eq!(snapshot["slot_states"]["queued"], 1);
    assert_eq!(snapshot["slot_states"]["error"], 1);
    assert_eq!(snapshot["generated_slots"], "2");
    corpus_search::control(&mut registry, id, "stopped").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args(["corpus-search-requeue", db.to_str().unwrap(), id, "1"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    registry.integrity().unwrap();
}

fn corpus_search_fixture(f: &Fixture) -> std::path::PathBuf {
    let config = corpus_batch_fixture(f);
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("strategies");
    value["seed"] = serde_json::json!(42);
    value["maximum_candidates"] = serde_json::json!(4);
    value["queue_width"] = serde_json::json!(2);
    let path = f.root.join("search.json");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    path
}
#[test]
fn corpus_search_cli_reopens_controls_replenishes_and_exhausts_without_extra_attempts() {
    let f = Fixture::new("search-cli", "EURUSD", 7);
    let config = corpus_search_fixture(&f);
    let db = f.root.join("search.sqlite");
    let call = |rounds: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args([
                "corpus-search",
                config.to_str().unwrap(),
                db.to_str().unwrap(),
                rounds,
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
    let mut pressure: serde_json::Value =
        serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    pressure["resource_guard"] =
        serde_json::json!({"maximum_process_rss_bytes":1,"minimum_host_available_memory_bytes":0});
    fs::write(&config, serde_json::to_vec(&pressure).unwrap()).unwrap();
    let blocked = call("100");
    assert_eq!(blocked["stop_reason"], "resource-pressure");
    assert_eq!(blocked["rounds"], 0);
    assert!(!db.exists());
    pressure.as_object_mut().unwrap().remove("resource_guard");
    fs::write(&config, serde_json::to_vec(&pressure).unwrap()).unwrap();
    let first = call("1");
    let id = first["last"]["status"]["search_id"].as_str().unwrap();
    assert_eq!(first["last"]["status"]["generated_slots"], "2");
    assert_eq!(first["stop_reason"], "round-limit");
    let control = |state: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args(["corpus-search-control", db.to_str().unwrap(), id, state])
            .output()
            .unwrap();
        assert!(out.status.success());
    };
    control("paused");
    let held = call("100");
    assert_eq!(held["stop_reason"], "held");
    assert_eq!(held["rounds"], 1);
    control("running");
    let done = call("100");
    assert_eq!(done["stop_reason"], "exhausted");
    assert_eq!(done["last"]["status"]["slot_states"]["completed"], 4);
    let attempts = rust_strategy_factory::storage::Registry::open(&db)
        .unwrap()
        .counts()
        .unwrap()
        .attempts;
    assert_eq!(call("100")["rounds"], 1);
    assert_eq!(
        rust_strategy_factory::storage::Registry::open(&db)
            .unwrap()
            .counts()
            .unwrap()
            .attempts,
        attempts
    );
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args(["corpus-search-status", db.to_str().unwrap(), id])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["state"],
        "exhausted"
    );
}
#[test]
#[cfg(unix)]
fn killed_search_process_waiting_for_writer_preserves_generated_queue_and_checkpoints() {
    use rust_strategy_factory::storage::Registry;
    use std::io::BufRead;
    use std::os::unix::process::ExitStatusExt;
    let f = Fixture::new("search-kill", "EURUSD", 7);
    let config = corpus_search_fixture(&f);
    let db = f.root.join("kill.sqlite");
    let args = [
        "corpus-search",
        config.to_str().unwrap(),
        db.to_str().unwrap(),
        "1",
    ];
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success());
    let first: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = first["last"]["status"]["search_id"].as_str().unwrap();
    let before =
        rust_strategy_factory::corpus_search::status(&Registry::open(&db).unwrap(), id).unwrap();
    let lock = rusqlite::Connection::open(&db).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    std::io::BufReader::new(child.stderr.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert!(ready.contains("corpus search ready"));
    child.kill().unwrap();
    assert_eq!(child.wait().unwrap().signal(), Some(9));
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);
    let registry = Registry::open(&db).unwrap();
    assert_eq!(
        rust_strategy_factory::corpus_search::status(&registry, id).unwrap(),
        before
    );
    registry.integrity().unwrap();
    drop(registry);
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-search",
            config.to_str().unwrap(),
            db.to_str().unwrap(),
            "100",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["stop_reason"],
        "exhausted"
    );
}
#[test]
fn oversized_operator_configs_refuse_before_registry_creation() {
    let f = Fixture::new("config-budget", "EURUSD", 7);
    let config = corpus_search_fixture(&f);
    let mut bytes = fs::read(&config).unwrap();
    bytes.extend(std::iter::repeat_n(b' ', 1024 * 1024));
    fs::write(&config, bytes).unwrap();
    let db = f.root.join("absent.sqlite");
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-search",
            config.to_str().unwrap(),
            db.to_str().unwrap(),
            "1",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("1 MiB"));
    assert!(!db.exists());
}

#[test]
#[cfg(unix)]
fn generated_search_sigterm_exits_at_a_round_boundary_and_restarts() {
    use rust_strategy_factory::storage::Registry;
    use std::io::BufRead;
    let f = Fixture::new("search-signal", "EURUSD", 7);
    let config = corpus_search_fixture(&f);
    let db = f.root.join("signal.sqlite");
    drop(Registry::open(&db).unwrap());
    let lock = rusqlite::Connection::open(&db).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-search",
            config.to_str().unwrap(),
            db.to_str().unwrap(),
            "10000",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    std::io::BufReader::new(child.stderr.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert!(ready.contains("corpus search ready"));
    assert!(
        Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    std::thread::sleep(std::time::Duration::from_millis(20));
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["stop_reason"], "signal");
    assert!(report["rounds"].as_u64().unwrap() <= 1);
    Registry::open(&db).unwrap().integrity().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-search",
            config.to_str().unwrap(),
            db.to_str().unwrap(),
            "100",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["stop_reason"],
        "exhausted"
    );
}

fn corpus_fleet_fixture(first: &Fixture, second: &Fixture) -> std::path::PathBuf {
    let one = corpus_search_fixture(first);
    let two = corpus_search_fixture(second);
    let file = first.root.join("fleet.json");
    fs::write(&file,serde_json::to_vec(&serde_json::json!({"sources":[{"name":"first","config":one},{"name":"second","config":two}],"coordinator_lease_ms":60000})).unwrap()).unwrap();
    file
}
#[test]
fn multi_corpus_fleet_rotates_after_each_round_and_preserves_full_scalar_receipts() {
    use rust_strategy_factory::{corpus_fleet, operator_config, storage::Registry, tick_engine};
    let first = Fixture::new("fleet-parity", "EURUSD", 7);
    let second = Fixture::new("fleet-parity", "XAUUSD", 7);
    let file = corpus_fleet_fixture(&first, &second);
    let db = first.root.join("fleet.sqlite");
    let id = corpus_fleet::register_file(&file, &db).unwrap();
    let mut exhausted = false;
    for visit in 0..40 {
        let result = corpus_fleet::step(&db, &id).unwrap();
        assert_eq!(
            result.visit.as_ref().unwrap()["source_index"],
            (visit % 2).to_string()
        );
        assert_eq!(
            result.status["next_source_index"],
            ((visit + 1) % 2).to_string()
        );
        assert_eq!(result.status["lease_active"], false);
        if result.kind == "exhausted" {
            exhausted = true;
            break;
        }
    }
    assert!(exhausted);
    let registry = Registry::open(&db).unwrap();
    let before = registry.counts().unwrap().attempts;
    for config in [
        first.root.join("search.json"),
        second.root.join("search.json"),
    ] {
        let loaded = operator_config::load_search(&config).unwrap();
        for slot in 0..4 {
            let (_, strategy) = rust_strategy_factory::research::candidate_at(slot, 42).unwrap();
            let key = tick_engine::corpus_key(
                &loaded.base,
                &loaded.specification,
                &strategy,
                &loaded.costs,
                loaded.file.budget,
            )
            .unwrap();
            let result = registry.result(&key.identity().unwrap()).unwrap().unwrap();
            assert_eq!(
                result.result["tick_replay"],
                serde_json::to_value(
                    tick_engine::replay_corpus(
                        &loaded.base,
                        &loaded.specification,
                        &strategy,
                        &loaded.costs,
                        loaded.file.budget
                    )
                    .unwrap()
                )
                .unwrap()
            );
        }
    }
    registry.integrity().unwrap();
    drop(registry);
    assert_eq!(corpus_fleet::step(&db, &id).unwrap().kind, "exhausted");
    assert_eq!(
        Registry::open(&db).unwrap().counts().unwrap().attempts,
        before
    );
}
#[test]
fn missing_corpus_is_isolated_while_other_input_finishes_then_restores() {
    use rust_strategy_factory::{corpus_fleet, storage::Registry};
    let first = Fixture::new("fleet-blocked", "EURUSD", 7);
    let second = Fixture::new("fleet-blocked", "XAUUSD", 7);
    let file = corpus_fleet_fixture(&first, &second);
    let db = first.root.join("fleet.sqlite");
    let id = corpus_fleet::register_file(&file, &db).unwrap();
    corpus_fleet::step(&db, &id).unwrap();
    let artifact = std::path::Path::new(&second.spec.chunks[0].path);
    let saved = second.root.join("temporarily-unavailable.rsf");
    fs::rename(artifact, &saved).unwrap();
    let blocked = corpus_fleet::step(&db, &id).unwrap();
    assert_eq!(blocked.visit.as_ref().unwrap()["kind"], "blocked");
    assert_eq!(blocked.status["next_source_index"], "0");
    let mut held = false;
    for _ in 0..40 {
        let step = corpus_fleet::step(&db, &id).unwrap();
        if step.kind == "no-ready-sources" {
            held = true;
            assert_eq!(step.status["state"], "paused");
            assert_eq!(step.status["sources"][0]["last_kind"], "exhausted");
            assert_eq!(step.status["sources"][1]["last_kind"], "blocked");
            break;
        }
    }
    assert!(held);
    fs::rename(&saved, artifact).unwrap();
    let mut registry = Registry::open(&db).unwrap();
    corpus_fleet::control(&mut registry, &id, "running").unwrap();
    drop(registry);
    let mut done = false;
    for _ in 0..40 {
        if corpus_fleet::step(&db, &id).unwrap().kind == "exhausted" {
            done = true;
            break;
        }
    }
    assert!(done);
    let registry = Registry::open(&db).unwrap();
    assert!(
        corpus_fleet::status(&registry, &id).unwrap()["sources"][1]["last_obstacle"]
            .as_str()
            .is_some()
    );
    registry.integrity().unwrap();
}
#[test]
fn fleet_backup_rotation_and_terminal_control_are_durable() {
    use rust_strategy_factory::{corpus_fleet, storage::Registry};
    let first = Fixture::new("fleet-backup", "EURUSD", 7);
    let second = Fixture::new("fleet-backup", "XAUUSD", 7);
    let file = corpus_fleet_fixture(&first, &second);
    let db = first.root.join("original.sqlite");
    let backup = first.root.join("backup.sqlite");
    let id = corpus_fleet::register_file(&file, &db).unwrap();
    corpus_fleet::step(&db, &id).unwrap();
    let mut registry = Registry::open(&db).unwrap();
    corpus_fleet::control(&mut registry, &id, "paused").unwrap();
    let before = corpus_fleet::status(&registry, &id).unwrap();
    registry.backup(&backup).unwrap();
    let mut restored = Registry::open(&backup).unwrap();
    assert_eq!(corpus_fleet::status(&restored, &id).unwrap(), before);
    corpus_fleet::control(&mut restored, &id, "running").unwrap();
    drop(restored);
    let resumed = corpus_fleet::step(&backup, &id).unwrap();
    assert_eq!(resumed.visit.unwrap()["source_index"], "1");
    assert_eq!(corpus_fleet::status(&registry, &id).unwrap(), before);
    corpus_fleet::control(&mut registry, &id, "stopped").unwrap();
    assert!(corpus_fleet::control(&mut registry, &id, "running").is_err());
    assert_eq!(corpus_fleet::step(&db, &id).unwrap().kind, "held");
    registry.integrity().unwrap();
}

#[test]
fn fleet_source_identity_changes_block_one_input_but_runtime_worker_changes_do_not() {
    use rust_strategy_factory::{corpus_fleet, corpus_search, storage::Registry};
    let first = Fixture::new("fleet-config", "EURUSD", 7);
    let second = Fixture::new("fleet-config", "XAUUSD", 7);
    let file = corpus_fleet_fixture(&first, &second);
    let db = first.root.join("fleet.sqlite");
    let id = corpus_fleet::register_file(&file, &db).unwrap();
    corpus_fleet::step(&db, &id).unwrap();
    corpus_fleet::step(&db, &id).unwrap();
    let config = first.root.join("search.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    value["limits"]["workers"] = serde_json::json!(1);
    fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    let same = corpus_fleet::step(&db, &id).unwrap();
    assert_eq!(same.visit.unwrap()["kind"], "advanced");
    corpus_fleet::step(&db, &id).unwrap();
    value["seed"] = serde_json::json!(43);
    fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    let changed = corpus_fleet::step(&db, &id).unwrap();
    assert_eq!(changed.visit.as_ref().unwrap()["kind"], "blocked");
    assert!(
        changed.visit.unwrap()["obstacle"]
            .as_str()
            .unwrap()
            .contains("frozen")
    );
    let other = corpus_fleet::step(&db, &id).unwrap();
    assert_ne!(other.visit.unwrap()["kind"], "blocked");
    let registry = Registry::open(&db).unwrap();
    assert_eq!(corpus_search::list(&registry).unwrap().len(), 2);
    registry.integrity().unwrap();
}
#[test]
fn fleet_duplicate_inputs_and_global_resource_pressure_do_not_start_searches() {
    use rust_strategy_factory::{corpus_fleet, corpus_search, storage::Registry};
    let first = Fixture::new("fleet-bounds", "EURUSD", 7);
    let config = corpus_search_fixture(&first);
    let file = first.root.join("fleet.json");
    let db = first.root.join("fleet.sqlite");
    let mut value = serde_json::json!({"sources":[{"name":"one","config":config},{"name":"duplicate","config":config}],"coordinator_lease_ms":60000});
    fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(corpus_fleet::register_file(&file, &db).is_err());
    assert!(!db.exists());
    value["sources"].as_array_mut().unwrap().pop();
    value["resource_guard"] =
        serde_json::json!({"maximum_process_rss_bytes":1,"minimum_host_available_memory_bytes":0});
    fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
    let id = corpus_fleet::register_file(&file, &db).unwrap();
    let blocked = corpus_fleet::step(&db, &id).unwrap();
    assert_eq!(blocked.kind, "resource-pressure");
    assert_eq!(blocked.status["visits"], "0");
    assert_eq!(blocked.status["attempt"], "0");
    let registry = Registry::open(&db).unwrap();
    assert!(corpus_search::list(&registry).unwrap().is_empty());
    assert_eq!(registry.counts().unwrap().registered, 0);
    registry.integrity().unwrap();
}
#[test]
fn fleet_cli_restarts_at_next_input_and_stops_with_terminal_budget() {
    let first = Fixture::new("fleet-cli", "EURUSD", 7);
    let second = Fixture::new("fleet-cli", "XAUUSD", 7);
    let file = corpus_fleet_fixture(&first, &second);
    let db = first.root.join("fleet.sqlite");
    let initialized = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-fleet-init",
            file.to_str().unwrap(),
            db.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(initialized.status.success());
    let initialized: serde_json::Value = serde_json::from_slice(&initialized.stdout).unwrap();
    let id = initialized["fleet_id"].as_str().unwrap();
    let call = |maximum: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args(["corpus-fleet", db.to_str().unwrap(), id, maximum])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let one = call("1");
    assert_eq!(one["last"]["visit"]["source_index"], "0");
    let two = call("1");
    assert_eq!(two["last"]["visit"]["source_index"], "1");
    assert_eq!(two["last"]["status"]["next_source_index"], "0");
    assert_eq!(call("100")["stop_reason"], "exhausted");
    let status = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args(["corpus-fleet-status", db.to_str().unwrap(), id])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&status.stdout).unwrap()["state"],
        "exhausted"
    );
}

#[test]
fn disk_pressure_sessions_refuse_before_registration_and_resume_with_unchanged_keys() {
    use rust_strategy_factory::{corpus_search, operator_config};
    let first = Fixture::new("disk-process", "EURUSD", 7);
    for (command, config) in [
        ("corpus-batch", corpus_batch_fixture(&first)),
        ("corpus-search", corpus_search_fixture(&first)),
    ] {
        let db = first.root.join(format!("{command}.sqlite"));
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        let original_id = if command == "corpus-search" {
            Some(
                corpus_search::identity(&operator_config::load_search(&config).unwrap().run())
                    .unwrap(),
            )
        } else {
            None
        };
        value["disk_guard"] = serde_json::json!({"minimum_available_bytes":u64::MAX});
        fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
        let run = || {
            let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
                .args([
                    command,
                    config.to_str().unwrap(),
                    db.to_str().unwrap(),
                    "100",
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
        let blocked = run();
        assert_eq!(blocked["stop_reason"], "disk-pressure");
        assert_eq!(blocked["rounds"], 0);
        assert_eq!(blocked["disk_admission"]["allowed"], false);
        assert!(blocked["disk_admission"]["observed"]["available_bytes"].is_string());
        assert!(!db.exists());
        value["disk_guard"] = serde_json::json!({"minimum_available_bytes":1});
        fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
        if let Some(original_id) = original_id {
            assert_eq!(
                corpus_search::identity(&operator_config::load_search(&config).unwrap().run())
                    .unwrap(),
                original_id
            );
        }
        let completed = run();
        assert_eq!(
            completed["stop_reason"],
            if command == "corpus-search" {
                "exhausted"
            } else {
                "complete"
            }
        );
    }
}
#[test]
fn fleet_disk_guard_preserves_cursor_and_isolates_one_source_until_policy_restored() {
    use rust_strategy_factory::{corpus_fleet, storage::Registry};
    let first = Fixture::new("disk-fleet", "EURUSD", 7);
    let second = Fixture::new("disk-fleet", "XAUUSD", 7);
    let file = corpus_fleet_fixture(&first, &second);
    let db = first.root.join("fleet.sqlite");
    let mut config: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    config["disk_guard"] = serde_json::json!({"minimum_available_bytes":u64::MAX});
    fs::write(&file, serde_json::to_vec(&config).unwrap()).unwrap();
    let guarded = corpus_fleet::register_file(&file, &db).unwrap();
    let blocked = corpus_fleet::step(&db, &guarded).unwrap();
    assert_eq!(blocked.kind, "disk-pressure");
    assert_eq!(blocked.status["visits"], "0");
    assert_eq!(blocked.status["attempt"], "0");
    assert!(!blocked.disk_admission.unwrap().allowed);
    assert_eq!(Registry::open(&db).unwrap().counts().unwrap().registered, 0);
    config.as_object_mut().unwrap().remove("disk_guard");
    fs::write(&file, serde_json::to_vec(&config).unwrap()).unwrap();
    let id = corpus_fleet::register_file(&file, &db).unwrap();
    let source = config["sources"][0]["config"].as_str().unwrap();
    let mut source_config: serde_json::Value =
        serde_json::from_slice(&fs::read(source).unwrap()).unwrap();
    source_config["disk_guard"] = serde_json::json!({"minimum_available_bytes":u64::MAX});
    fs::write(source, serde_json::to_vec(&source_config).unwrap()).unwrap();
    let held = corpus_fleet::step(&db, &id).unwrap();
    assert_eq!(held.visit.unwrap()["kind"], "disk-pressure");
    let mut paused = false;
    for _ in 0..60 {
        let result = corpus_fleet::step(&db, &id).unwrap();
        if result.kind == "no-ready-sources" {
            assert_eq!(result.status["sources"][1]["last_kind"], "exhausted");
            paused = true;
            break;
        }
    }
    assert!(paused);
    source_config.as_object_mut().unwrap().remove("disk_guard");
    fs::write(source, serde_json::to_vec(&source_config).unwrap()).unwrap();
    let mut registry = Registry::open(&db).unwrap();
    corpus_fleet::control(&mut registry, &id, "running").unwrap();
    drop(registry);
    let mut exhausted = false;
    for _ in 0..60 {
        if corpus_fleet::step(&db, &id).unwrap().kind == "exhausted" {
            exhausted = true;
            break;
        }
    }
    assert!(exhausted);
    Registry::open(&db).unwrap().integrity().unwrap();
}
#[test]
fn actual_disk_observation_resolves_parent_and_refuses_unknown_targets() {
    use rust_strategy_factory::disk;
    let first = Fixture::new("disk-observe", "EURUSD", 7);
    let db = first.root.join("new.sqlite");
    let snapshot = disk::observe(&db).unwrap();
    assert!(snapshot.total_bytes > 0 && snapshot.available_bytes <= snapshot.total_bytes);
    assert!(!db.exists());
    assert!(disk::observe(&first.root.join("missing").join("new.sqlite")).is_err());
    assert!(disk::observe(&first.root).is_err());
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args(["disk-status", db.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(report["available_bytes"].is_string());
    assert!(!report.to_string().contains(first.root.to_str().unwrap()));
}

#[test]
fn actual_sqlite_writer_contention_has_typed_admission_failures_and_no_child_claims() {
    use rust_strategy_factory::{
        contracts::Side,
        corpus_failure::Kind,
        storage::Registry,
        strategies::{Rule, Strategy},
        tick_engine::{self, CorpusBatchLimits, CorpusBudget, CorpusRun, Costs},
    };
    let f = Fixture::new("typed-busy", "EURUSD", 7);
    let db = f.root.join("busy.sqlite");
    drop(Registry::open(&db).unwrap());
    let lock = rusqlite::Connection::open(&db).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let strategy = Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks: 100,
        rule: Rule::AboveMean { window: 2 },
        entry: None,
        stop_policy: None,
    };
    let costs = Costs::default();
    let runs = [CorpusRun {
        base: &f.root,
        spec: &f.spec,
        strategy: &strategy,
        costs: &costs,
        budget: CorpusBudget {
            max_candles: 100,
            feature_cells: 10000,
            max_receipts: 1000,
        },
    }];
    let limits = CorpusBatchLimits {
        workers: 1,
        max_parallel_candles: 100,
        max_parallel_feature_cells: 10000,
        max_parallel_receipts: 1000,
        max_new_chunks: 64,
        lease_ms: 60000,
    };
    let blocked = tick_engine::execute_corpus_batch(&db, &runs, limits).unwrap();
    assert_eq!(blocked.workers_started, 0);
    assert_eq!(
        blocked.failure_kinds,
        vec![Some(Kind::RegistryAdmissionBusy)]
    );
    assert!(blocked.jobs[0].is_err());
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);
    assert_eq!(Registry::open(&db).unwrap().counts().unwrap().registered, 0);
    let completed = tick_engine::execute_corpus_batch(&db, &runs, limits).unwrap();
    assert!(completed.jobs[0].is_ok());
    assert_eq!(completed.failure_kinds, vec![None]);
    Registry::open(&db).unwrap().integrity().unwrap();
}

#[test]
fn retry_policy_cli_is_bounded_frozen_and_never_resumes_operator_pause() {
    let f = Fixture::new("retry-cli", "EURUSD", 7);
    let config = corpus_search_fixture(&f);
    let db = f.root.join("retry.sqlite");
    let mut file: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    file["budget"]["feature_cells"] = 1.into();
    file["limits"]["max_parallel_feature_cells"] = 2.into();
    fs::write(&config, serde_json::to_vec(&file).unwrap()).unwrap();
    let run = |args: &[&str], success: bool| {
        let result = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(
            result.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        result.stdout
    };
    let first: serde_json::Value = serde_json::from_slice(&run(
        &[
            "corpus-search",
            config.to_str().unwrap(),
            db.to_str().unwrap(),
            "1",
        ],
        true,
    ))
    .unwrap();
    let id = first["last"]["status"]["search_id"].as_str().unwrap();
    let epoch = first["last"]["status"]["control"]["epoch"]
        .as_str()
        .unwrap();
    let policy = f.root.join("policy.json");
    fs::write(&policy, r#"{"backoff_ms":86400000,"maximum_slots":1}"#).unwrap();
    let configured = run(
        &[
            "corpus-retry-configure",
            db.to_str().unwrap(),
            id,
            policy.to_str().unwrap(),
        ],
        true,
    );
    assert_eq!(
        run(
            &[
                "corpus-retry-configure",
                db.to_str().unwrap(),
                id,
                policy.to_str().unwrap()
            ],
            true
        ),
        configured
    );
    let decision: serde_json::Value = serde_json::from_slice(&run(
        &["corpus-retry-admit", db.to_str().unwrap(), id, epoch],
        true,
    ))
    .unwrap();
    assert_eq!(decision["kind"], "backoff");
    assert_eq!(decision["search_id"], id);
    assert_eq!(decision["workers_started"], 0);
    run(
        &["corpus-retry-admit", db.to_str().unwrap(), id, "-1"],
        false,
    );
    fs::write(&policy, r#"{"backoff_ms":1,"maximum_slots":1}"#).unwrap();
    run(
        &[
            "corpus-retry-configure",
            db.to_str().unwrap(),
            id,
            policy.to_str().unwrap(),
        ],
        false,
    );
    run(
        &["corpus-search-control", db.to_str().unwrap(), id, "paused"],
        true,
    );
    let snapshot: serde_json::Value = serde_json::from_slice(&run(
        &["corpus-search-status", db.to_str().unwrap(), id],
        true,
    ))
    .unwrap();
    let next = snapshot["control"]["epoch"].as_str().unwrap();
    let held: serde_json::Value = serde_json::from_slice(&run(
        &["corpus-retry-admit", db.to_str().unwrap(), id, next],
        true,
    ))
    .unwrap();
    assert_eq!(held["reason"], "not-worker-error-pause");
    assert_eq!(snapshot["requeue_records"], "0");
}

fn foreground_config(f: &Fixture) -> (PathBuf, PathBuf, PathBuf, String) {
    let config = corpus_search_fixture(f);
    let db = f.root.join("foreground.sqlite");
    let policy = f.root.join("retry-policy.json");
    fs::write(&policy, r#"{"backoff_ms":86400000,"maximum_slots":1}"#).unwrap();
    let mut file: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    file["budget"]["feature_cells"] = 1.into();
    file["limits"]["max_parallel_feature_cells"] = 2.into();
    fs::write(&config, serde_json::to_vec(&file).unwrap()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-search",
            config.to_str().unwrap(),
            db.to_str().unwrap(),
            "1",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = report["last"]["status"]["search_id"]
        .as_str()
        .unwrap()
        .to_owned();
    // Synthetic fault metadata only: exercise waiting without performing a retry.
    let c = rusqlite::Connection::open(&db).unwrap();
    let rows = {
        let mut q = c
            .prepare("SELECT slot,hash,error FROM corpus_search_jobs WHERE search_id=?1")
            .unwrap();
        q.query_map([&id], |r| {
            Ok((
                r.get::<_, usize>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap()
    };
    for (slot, hash, error) in rows {
        let fingerprint = rust_strategy_factory::data::hash(&(
            "corpus-failure-v1",
            &id,
            slot,
            &hash,
            rust_strategy_factory::corpus_failure::Kind::RegistryAdmissionBusy,
            &error,
        ))
        .unwrap();
        c.execute("UPDATE corpus_search_jobs SET error_kind='registry_admission_busy',failure_hash=?3 WHERE search_id=?1 AND slot=?2",rusqlite::params![id,slot,fingerprint]).unwrap();
    }
    drop(c);
    (config, db, policy, id)
}
#[test]
fn foreground_supervision_bounds_wait_and_preserves_operator_pause_across_processes() {
    let f = Fixture::new("foreground-wait", "EURUSD", 7);
    let (config, db, policy, id) = foreground_config(&f);
    let invoke = |wait: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args([
                "corpus-supervise",
                config.to_str().unwrap(),
                policy.to_str().unwrap(),
                db.to_str().unwrap(),
                "10",
                wait,
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
    let first = invoke("100");
    assert_eq!(first["stop_reason"], "wait-budget");
    assert_eq!(first["round_attempts"], "0");
    assert_eq!(first["checks"], "1");
    let second = invoke("0");
    assert_eq!(second["stop_reason"], "retry-backoff");
    assert_eq!(
        second["last"]["decision"]["retry_at_ms"],
        first["last"]["decision"]["retry_at_ms"]
    );
    let mut registry = rust_strategy_factory::storage::Registry::open(&db).unwrap();
    rust_strategy_factory::corpus_search::control(&mut registry, &id, "paused").unwrap();
    let held = invoke("100");
    assert_eq!(held["stop_reason"], "retry-held");
    assert_eq!(held["round_attempts"], "0");
    assert_eq!(
        rust_strategy_factory::corpus_search::status(&registry, &id).unwrap()["requeue_records"],
        "0"
    );
    registry.integrity().unwrap();
}
#[test]
#[cfg(unix)]
fn foreground_supervision_sigterm_reaps_its_foreground_session_at_wait_boundary() {
    use std::io::BufRead;
    let f = Fixture::new("foreground-signal", "EURUSD", 7);
    let (config, db, policy, _id) = foreground_config(&f);
    let mut child = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-supervise",
            config.to_str().unwrap(),
            policy.to_str().unwrap(),
            db.to_str().unwrap(),
            "100",
            "10000",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    std::io::BufReader::new(child.stderr.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert!(ready.contains("corpus supervision ready"));
    let c = rusqlite::Connection::open(&db).unwrap();
    let mut observed = false;
    for _ in 0..500 {
        let count: i64 = c
            .query_row("SELECT count(*) FROM corpus_retry_decisions", [], |r| {
                r.get(0)
            })
            .unwrap();
        if count > 0 {
            observed = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !observed {
        child.kill().unwrap();
        child.wait().unwrap();
        panic!("foreground never reached a backoff observation");
    }
    assert!(
        Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["stop_reason"], "signal");
    assert_eq!(report["round_attempts"], "0");
    rust_strategy_factory::storage::Registry::open(&db)
        .unwrap()
        .integrity()
        .unwrap();
}

#[test]
fn foreground_pressure_and_argument_limits_refuse_before_registry_creation() {
    let f = Fixture::new("foreground-pressure", "EURUSD", 7);
    let config = corpus_search_fixture(&f);
    let policy = f.root.join("policy.json");
    fs::write(&policy, r#"{"backoff_ms":1,"maximum_slots":1}"#).unwrap();
    let mut file: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    for (name, guard) in [
        (
            "disk",
            serde_json::json!({"minimum_available_bytes":u64::MAX}),
        ),
        (
            "resource",
            serde_json::json!({"maximum_process_rss_bytes":1,"minimum_host_available_memory_bytes":0}),
        ),
    ] {
        file.as_object_mut().unwrap().remove("disk_guard");
        file.as_object_mut().unwrap().remove("resource_guard");
        file[format!("{name}_guard")] = guard;
        fs::write(&config, serde_json::to_vec(&file).unwrap()).unwrap();
        let db = f.root.join(format!("{name}.sqlite"));
        let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args([
                "corpus-supervise",
                config.to_str().unwrap(),
                policy.to_str().unwrap(),
                db.to_str().unwrap(),
                "10",
                "0",
            ])
            .output()
            .unwrap();
        assert!(out.status.success());
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["stop_reason"], format!("{name}-pressure"));
        assert_eq!(value["checks"], "0");
        assert!(!db.exists());
    }
    let db = f.root.join("bad-args.sqlite");
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-supervise",
            config.to_str().unwrap(),
            policy.to_str().unwrap(),
            db.to_str().unwrap(),
            "0",
            "3600001",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!db.exists());
}

#[test]
fn foreground_session_stop_is_cooperative_and_receipted() {
    foreground_session_interruption(false);
}
#[test]
fn foreground_session_crash_stays_unclosed_and_restart_gets_new_identity() {
    foreground_session_interruption(true);
}
fn foreground_session_interruption(crash: bool) {
    use std::io::BufRead;
    let f = Fixture::new(
        if crash {
            "session-crash"
        } else {
            "session-stop"
        },
        "EURUSD",
        7,
    );
    let (config, db, policy, source) = foreground_config(&f);
    let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
    let args = [
        "corpus-supervise",
        config.to_str().unwrap(),
        policy.to_str().unwrap(),
        db.to_str().unwrap(),
        "100",
        "10000",
    ];
    let mut child = Command::new(binary)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    std::io::BufReader::new(child.stderr.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert!(ready.contains("corpus supervision ready"));
    let c = rusqlite::Connection::open(&db).unwrap();
    let mut id = None;
    for _ in 0..500 {
        let count: i64 = c
            .query_row("SELECT count(*) FROM corpus_retry_decisions", [], |r| {
                r.get(0)
            })
            .unwrap();
        if count > 0 {
            id = Some(
                c.query_row("SELECT id FROM corpus_supervision_sessions", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
            );
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let Some(id) = id else {
        child.kill().unwrap();
        child.wait().unwrap();
        panic!("no session/backoff observed");
    };
    if crash {
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
    } else {
        let stop = Command::new(binary)
            .args(["corpus-supervision-stop", db.to_str().unwrap(), &id])
            .output()
            .unwrap();
        assert!(
            stop.status.success(),
            "{}",
            String::from_utf8_lossy(&stop.stderr)
        );
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(report["stop_reason"], "session-stop-request");
        assert_eq!(report["round_attempts"], "0");
        assert!(report["session_status"]["receipt_hash"].is_string());
    }
    let first = Command::new(binary)
        .args(["corpus-supervision-status", db.to_str().unwrap(), &id])
        .output()
        .unwrap();
    assert!(first.status.success());
    let old: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert!(old.get("token").is_none());
    assert_eq!(old["finished_at_ms"].is_null(), crash);
    if crash {
        assert_eq!(old["observation"], "unclosed; PID liveness not established");
    }
    let mut restart = args;
    restart[5] = "0";
    let out = Command::new(binary).args(restart).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_ne!(report["session_id"], id);
    assert_eq!(report["stop_reason"], "retry-backoff");
    let registry = rust_strategy_factory::storage::Registry::open(&db).unwrap();
    assert_eq!(
        rust_strategy_factory::supervision_session::status(&registry, &id).unwrap(),
        old
    );
    assert_eq!(
        rust_strategy_factory::corpus_search::status(&registry, &source).unwrap()["requeue_records"],
        "0"
    );
    registry.integrity().unwrap();
}
