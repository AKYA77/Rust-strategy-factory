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
    for cap in [0, corpus::MAX_RETAINED_CANDLES + 1, usize::MAX] {
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
    let mut session = tick_engine::CorpusBatchSession::default();
    for round in 0..f.spec.chunks.len() {
        let report = session.round(&db, &runs, limits).unwrap();
        assert_eq!(report.workers_started, 2);
        assert_eq!(report.worker_failures, 0);
        assert_eq!(report.jobs.len(), 3);
        assert!(report.preparation.builds <= 2);
        if round == 0 {
            assert!(report.preparation.builds > 0);
        }
        assert!(session.retained_candles() <= limits.max_parallel_candles);
        assert_eq!(report.preparation.builds + report.preparation.reuses, 3);
        assert_eq!(
            report.preparation.current_byte_checks,
            report.preparation.reuses
        );
        assert!(report.preparation.peak_retained_candles_per_worker <= budget.max_candles);
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
    session.release();
    assert_eq!(session.retained_candles(), 0);
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
    let serial = tick_engine::execute_corpus_batch(
        &f.root.join("shared-errors.sqlite"),
        &runs,
        CorpusBatchLimits {
            workers: 1,
            max_parallel_candles: 100,
            max_parallel_feature_cells: 10000,
            max_parallel_receipts: 1000,
            max_new_chunks: 1,
            lease_ms: 60000,
        },
    )
    .unwrap();
    assert!(serial.jobs[0].is_err());
    assert!(matches!(
        &serial.jobs[1],
        Ok(CorpusProgress::Pending { cursor: 1, .. })
    ));
    assert_eq!(serial.preparation.builds, 1);
    assert_eq!(serial.preparation.reuses, 1);
    assert_eq!(serial.preparation.current_byte_checks, 1);
    Registry::open(&f.root.join("shared-errors.sqlite"))
        .unwrap()
        .integrity()
        .unwrap();
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
        start_index: 0,
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
        start_index: 0,
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
            start_index: 0,
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

#[test]
fn owned_foreground_child_exit_and_wall_budget_are_reaped_with_exact_receipts() {
    let f = Fixture::new("owned-wall", "EURUSD", 7);
    let (config, db, policy, source) = foreground_config(&f);
    let limits = f.root.join("parent.json");
    fs::write(
        &limits,
        r#"{"maximum_wall_ms":100,"shutdown_grace_ms":3000,"maximum_output_bytes":1048576}"#,
    )
    .unwrap();
    let invoke = |wait: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args([
                "corpus-supervise-owned",
                config.to_str().unwrap(),
                policy.to_str().unwrap(),
                db.to_str().unwrap(),
                "100",
                wait,
                limits.to_str().unwrap(),
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
    let first = invoke("10000");
    assert_eq!(first["stop_reason"], "wall-budget");
    assert_eq!(first["reaped"], true);
    assert_eq!(first["forced_kill"], false);
    assert_eq!(first["stop_request_delivered"], true);
    assert_eq!(first["exit_success"], true);
    assert_eq!(first["child_report"]["stop_reason"], "session-stop-request");
    assert_eq!(first["session_status"]["observation"], "finished");
    assert!(first["session_status"]["receipt_hash"].is_string());
    let mut hashed_report = first.clone();
    hashed_report
        .as_object_mut()
        .unwrap()
        .remove("invocation_status");
    assert_eq!(
        first["invocation_status"]["summary"]["report_hash"],
        rust_strategy_factory::data::hash(&hashed_report).unwrap()
    );
    let persisted = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-owned-status",
            db.to_str().unwrap(),
            first["invocation_id"].as_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(persisted.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&persisted.stdout).unwrap(),
        first["invocation_status"]
    );
    // Cancellation and normal exit test different contracts. A loaded CI runner
    // must not turn the normal-exit check into another 100 ms cancellation check.
    fs::write(
        &limits,
        r#"{"maximum_wall_ms":5000,"shutdown_grace_ms":3000,"maximum_output_bytes":1048576}"#,
    )
    .unwrap();
    let second = invoke("0");
    assert_eq!(second["stop_reason"], "child-exited");
    assert_eq!(second["child_report"]["stop_reason"], "retry-backoff");
    assert_ne!(second["session_id"], first["session_id"]);
    assert_eq!(
        second["child_report"]["last"]["decision"]["retry_at_ms"],
        first["child_report"]["last"]["decision"]["retry_at_ms"]
    );
    let registry = rust_strategy_factory::storage::Registry::open(&db).unwrap();
    assert_eq!(
        rust_strategy_factory::corpus_search::status(&registry, &source).unwrap()["requeue_records"],
        "0"
    );
    registry.integrity().unwrap();
}
#[test]
#[cfg(unix)]
fn owned_parent_signal_escalates_only_its_actual_child_and_reaps_it() {
    let f = Fixture::new("owned-signal", "EURUSD", 7);
    let (config, db, policy, _) = foreground_config(&f);
    let limits = f.root.join("parent.json");
    fs::write(
        &limits,
        r#"{"maximum_wall_ms":10000,"shutdown_grace_ms":0,"maximum_output_bytes":1048576}"#,
    )
    .unwrap();
    let mut parent = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-supervise-owned",
            config.to_str().unwrap(),
            policy.to_str().unwrap(),
            db.to_str().unwrap(),
            "100",
            "10000",
            limits.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let c = rusqlite::Connection::open(&db).unwrap();
    let mut ready = false;
    for _ in 0..500 {
        let n: i64 = c
            .query_row("SELECT count(*) FROM corpus_retry_decisions", [], |r| {
                r.get(0)
            })
            .unwrap();
        if n > 0 {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !ready {
        parent.kill().unwrap();
        parent.wait().unwrap();
        panic!("owned worker did not wait");
    }
    assert!(
        Command::new("kill")
            .args(["-TERM", &parent.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let out = parent.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["stop_reason"], "signal");
    assert_eq!(value["forced_kill"], true);
    assert_eq!(value["reaped"], true);
    assert_eq!(value["exit_success"], false);
    assert_eq!(
        value["session_status"]["observation"],
        "unclosed; PID liveness not established"
    );
    assert_eq!(value["child_report"], serde_json::Value::Null);
    assert!(!std::path::Path::new(&format!("/proc/{}", value["pid"].as_str().unwrap())).exists());
    rust_strategy_factory::storage::Registry::open(&db)
        .unwrap()
        .integrity()
        .unwrap();
}
#[test]
fn owned_parent_refuses_pressure_and_bad_limits_before_creating_registry_or_child() {
    let f = Fixture::new("owned-refuse", "EURUSD", 7);
    let config = corpus_search_fixture(&f);
    let mut file: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    file["disk_guard"] = serde_json::json!({"minimum_available_bytes":u64::MAX});
    fs::write(&config, serde_json::to_vec(&file).unwrap()).unwrap();
    let policy = f.root.join("policy.json");
    fs::write(&policy, r#"{"backoff_ms":1,"maximum_slots":1}"#).unwrap();
    let limits = f.root.join("parent.json");
    for (wall, success) in [(0, false), (100, true)] {
        fs::write(&limits,serde_json::to_vec(&serde_json::json!({"maximum_wall_ms":wall,"shutdown_grace_ms":0,"maximum_output_bytes":1024})).unwrap()).unwrap();
        let db = f.root.join(format!("refuse-{wall}.sqlite"));
        let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args([
                "corpus-supervise-owned",
                config.to_str().unwrap(),
                policy.to_str().unwrap(),
                db.to_str().unwrap(),
                "10",
                "0",
                limits.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(out.status.success(), success);
        assert!(!db.exists());
        if success {
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["stop_reason"],
                "disk-pressure"
            );
        }
    }
}

#[test]
fn owned_child_output_overflow_is_bounded_and_reaped_without_partial_json() {
    let f = Fixture::new("owned-output", "EURUSD", 7);
    let (config, db, policy, _) = foreground_config(&f);
    let limits = f.root.join("parent.json");
    fs::write(
        &limits,
        r#"{"maximum_wall_ms":10000,"shutdown_grace_ms":100,"maximum_output_bytes":1024}"#,
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-supervise-owned",
            config.to_str().unwrap(),
            policy.to_str().unwrap(),
            db.to_str().unwrap(),
            "1",
            "0",
            limits.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["stdout_truncated"], true);
    assert!(
        value["stdout_bytes"]
            .as_str()
            .unwrap()
            .parse::<usize>()
            .unwrap()
            > 1024
    );
    assert_eq!(value["child_report"], serde_json::Value::Null);
    assert_eq!(value["reaped"], true);
    assert!(out.stdout.len() < 16384);
    rust_strategy_factory::storage::Registry::open(&db)
        .unwrap()
        .integrity()
        .unwrap();
}

#[test]
fn owned_child_rss_threshold_cancels_and_preserves_actual_resource_observations() {
    let f = Fixture::new("owned-rss", "EURUSD", 7);
    let (config, db, policy, _) = foreground_config(&f);
    let limits = f.root.join("parent.json");
    fs::write(&limits,r#"{"maximum_wall_ms":10000,"shutdown_grace_ms":3000,"maximum_output_bytes":1048576,"maximum_child_rss_bytes":1}"#).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-supervise-owned",
            config.to_str().unwrap(),
            policy.to_str().unwrap(),
            db.to_str().unwrap(),
            "100",
            "10000",
            limits.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["stop_reason"], "child-rss-pressure");
    assert_eq!(report["reaped"], true);
    assert!(
        report["child_resources"]["peak_observed_rss_bytes"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 1
    );
    assert!(
        report["child_resources"]["samples"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 0
    );
    assert_eq!(
        report["invocation_status"]["resources"]["peak_rss_bytes"],
        report["child_resources"]["peak_observed_rss_bytes"]
    );
    assert_eq!(
        report["invocation_status"]["limits"]["maximum_child_rss_bytes"],
        "1"
    );
    let registry = rust_strategy_factory::storage::Registry::open(&db).unwrap();
    registry.integrity().unwrap();
}

fn owned_fleet_files(root: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let policy = root.join("owned-policy.json");
    let limits = root.join("owned-limits.json");
    fs::write(&policy, r#"{"backoff_ms":1,"maximum_slots":1}"#).unwrap();
    fs::write(
        &limits,
        r#"{"maximum_wall_ms":10000,"shutdown_grace_ms":100,"maximum_output_bytes":1048576}"#,
    )
    .unwrap();
    (policy, limits)
}
fn owned_fleet_call(
    db: &std::path::Path,
    id: &str,
    policy: &std::path::Path,
    limits: &std::path::Path,
    maximum: &str,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-fleet-supervise",
            db.to_str().unwrap(),
            id,
            policy.to_str().unwrap(),
            limits.to_str().unwrap(),
            maximum,
        ])
        .output()
        .unwrap()
}
/// Wait-capable fleet driver: scheduler-idle retry backoffs are waited out instead of
/// being treated as a terminal stop, so the bounded visit budget can actually be reached.
fn owned_fleet_loop_call(
    db: &std::path::Path,
    id: &str,
    policy: &std::path::Path,
    limits: &std::path::Path,
    maximum: &str,
    wait_ms: &str,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-fleet-loop",
            db.to_str().unwrap(),
            id,
            policy.to_str().unwrap(),
            limits.to_str().unwrap(),
            maximum,
            wait_ms,
        ])
        .output()
        .unwrap()
}
#[test]
fn owned_fleet_rotation_preserves_scalar_receipts_exact_cost_observations_and_backup() {
    use rust_strategy_factory::{corpus_fleet, operator_config, storage::Registry, tick_engine};
    let first = Fixture::new("owned-fleet-parity", "EURUSD", 7);
    let second = Fixture::new("owned-fleet-parity", "XAUUSD", 7);
    let file = corpus_fleet_fixture(&first, &second);
    let db = first.root.join("fleet.sqlite");
    let id = corpus_fleet::register_file(&file, &db).unwrap();
    let (policy, limits) = owned_fleet_files(&first.root);
    // Under full-suite parallel load a step can exceed the fixture's wall budget and leave
    // the fleet retry-held. Retry the wait-capable loop until it actually exhausts; the
    // exhausted assertion is unchanged.
    let report = {
        let mut attempts = 0;
        loop {
            let out = owned_fleet_loop_call(&db, &id, &policy, &limits, "100", "60000");
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            if value["stop_reason"] == "exhausted" {
                break value;
            }
            attempts += 1;
            assert!(
                attempts < 20,
                "fleet did not exhaust, stop_reason={}",
                value["stop_reason"]
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    };
    assert_eq!(report["stop_reason"], "exhausted");
    let registry = Registry::open(&db).unwrap();
    let status = corpus_fleet::status(&registry, &id).unwrap();
    assert_eq!(status["lease_active"], false);
    for observation in status["owned_observations"].as_array().unwrap() {
        let n = |field: &str| observation[field].as_str().unwrap().parse::<u64>().unwrap();
        assert!(n("owned_visits") > 0);
        assert_eq!(
            n("cpu_observed_visits") + n("cpu_unobserved_visits"),
            n("owned_visits")
        );
        assert!(n("observed_parent_elapsed_ms") > 0);
        assert!(observation["last_invocation_id"].is_string());
    }
    let c = rusqlite::Connection::open(&db).unwrap();
    let mut query=c.prepare("SELECT source_index,visit_sequence FROM corpus_fleet_owned_visits WHERE fleet_id=?1 ORDER BY visit_sequence").unwrap();
    let links = query
        .query_map([&id], |r| {
            Ok((r.get::<_, usize>(0)?, r.get::<_, usize>(1)?))
        })
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert!(!links.is_empty());
    for (index, sequence) in links {
        assert_eq!(index, (sequence - 1) % 2);
    }
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
    let backup = first.root.join("fleet-backup.sqlite");
    registry.backup(&backup).unwrap();
    let restored = Registry::open(&backup).unwrap();
    assert_eq!(corpus_fleet::status(&restored, &id).unwrap(), status);
    restored.integrity().unwrap();
    assert!(
        c.execute(
            "UPDATE corpus_fleet_owned_visits SET source_index=1 WHERE fleet_id=?1",
            [&id]
        )
        .is_err()
    );
    c.execute_batch("DROP TRIGGER fleet_owned_no_update;")
        .unwrap();
    c.execute(
        "UPDATE corpus_fleet_owned_visits SET visit_sequence=visit_sequence+1000 WHERE fleet_id=?1",
        [&id],
    )
    .unwrap();
    assert!(registry.integrity().is_err());
}
#[test]
fn owned_fleet_rejects_insufficient_coordinator_lease_before_claim_or_spawn() {
    use rust_strategy_factory::{corpus_fleet, storage::Registry};
    let first = Fixture::new("owned-fleet-lease", "EURUSD", 7);
    let second = Fixture::new("owned-fleet-lease", "XAUUSD", 7);
    let file = corpus_fleet_fixture(&first, &second);
    let db = first.root.join("fleet.sqlite");
    let id = corpus_fleet::register_file(&file, &db).unwrap();
    let (policy, limits) = owned_fleet_files(&first.root);
    fs::write(
        &limits,
        r#"{"maximum_wall_ms":50000,"shutdown_grace_ms":100,"maximum_output_bytes":1048576}"#,
    )
    .unwrap();
    let out = owned_fleet_call(&db, &id, &policy, &limits, "1");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("coordinator lease"));
    let registry = Registry::open(&db).unwrap();
    let s = corpus_fleet::status(&registry, &id).unwrap();
    assert_eq!(s["visits"], "0");
    assert_eq!(s["attempt"], "0");
    assert!(
        rust_strategy_factory::owned_history::list(&registry)
            .unwrap()
            .is_empty()
    );
    registry.integrity().unwrap();
}
#[test]
fn owned_fleet_link_failure_keeps_child_evidence_and_rolls_back_rotation_prefix() {
    use rust_strategy_factory::{corpus_fleet, storage::Registry};
    let first = Fixture::new("owned-fleet-link", "EURUSD", 7);
    let second = Fixture::new("owned-fleet-link", "XAUUSD", 7);
    let file = corpus_fleet_fixture(&first, &second);
    let db = first.root.join("fleet.sqlite");
    let id = corpus_fleet::register_file(&file, &db).unwrap();
    let (policy, limits) = owned_fleet_files(&first.root);
    let c = rusqlite::Connection::open(&db).unwrap();
    c.execute_batch("CREATE TRIGGER fail_fleet_link BEFORE INSERT ON corpus_fleet_owned_visits BEGIN SELECT RAISE(ABORT,'injected link failure');END;").unwrap();
    let out = owned_fleet_call(&db, &id, &policy, &limits, "1");
    assert!(!out.status.success());
    let registry = Registry::open(&db).unwrap();
    let s = corpus_fleet::status(&registry, &id).unwrap();
    assert_eq!(s["visits"], "0");
    assert_eq!(s["next_source_index"], "0");
    assert_eq!(s["lease_active"], false);
    let history = rust_strategy_factory::owned_history::list(&registry).unwrap();
    assert_eq!(history.len(), 1);
    assert!(history[0]["finished_at_ms"].is_string());
    registry.integrity().unwrap();
    c.execute_batch("DROP TRIGGER fail_fleet_link;").unwrap();
    let out = owned_fleet_call(&db, &id, &policy, &limits, "1");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["last"]["visit"]["source_index"], "0");
    let s = corpus_fleet::status(&registry, &id).unwrap();
    assert_eq!(s["visits"], "1");
    assert_eq!(s["owned_observations"][0]["owned_visits"], "1");
    assert_eq!(
        rust_strategy_factory::owned_history::list(&registry)
            .unwrap()
            .len(),
        2
    );
    registry.integrity().unwrap();
}

#[test]
fn owned_fleet_isolates_missing_input_and_never_overrides_operator_source_pause() {
    use rust_strategy_factory::{corpus_fleet, corpus_search, storage::Registry};
    let first = Fixture::new("owned-fleet-isolation", "EURUSD", 7);
    let second = Fixture::new("owned-fleet-isolation", "XAUUSD", 7);
    let file = corpus_fleet_fixture(&first, &second);
    let db = first.root.join("fleet.sqlite");
    let id = corpus_fleet::register_file(&file, &db).unwrap();
    let (policy, limits) = owned_fleet_files(&first.root);
    let artifact = std::path::Path::new(&first.spec.chunks[0].path);
    let hidden = first.root.join("unavailable.rsf");
    fs::rename(artifact, &hidden).unwrap();
    let invoke = || {
        let out = owned_fleet_call(&db, &id, &policy, &limits, "1");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let blocked = invoke();
    assert_eq!(blocked["last"]["visit"]["source_index"], "0");
    assert_eq!(blocked["last"]["visit"]["kind"], "blocked");
    let healthy = invoke();
    assert_eq!(healthy["last"]["visit"]["source_index"], "1");
    assert_eq!(
        healthy["last"]["status"]["owned_observations"][1]["owned_visits"],
        "1"
    );
    fs::rename(&hidden, artifact).unwrap();
    let recovered = invoke();
    let source = recovered["last"]["status"]["sources"][0]["search_id"]
        .as_str()
        .unwrap();
    let mut registry = Registry::open(&db).unwrap();
    corpus_search::control(&mut registry, source, "paused").unwrap();
    invoke();
    let held = invoke();
    assert_eq!(held["last"]["visit"]["kind"], "paused");
    assert_eq!(
        held["last"]["status"]["owned_observations"][0]["owned_visits"],
        "1"
    );
    assert_eq!(
        corpus_search::status(&registry, source).unwrap()["control"]["pause_origin"],
        "operator"
    );
    registry.integrity().unwrap();
}

// Development-only typed-admission fault metadata, without copying or deleting any child receipt.
fn retryable_fleet_fixture(
    f: &Fixture,
    backoff: i64,
) -> (PathBuf, PathBuf, PathBuf, String, String) {
    let (config, template, policy, source) = foreground_config(f);
    fs::write(
        &policy,
        serde_json::to_vec(&serde_json::json!({"backoff_ms":backoff,"maximum_slots":2})).unwrap(),
    )
    .unwrap();
    let db = f.root.join("loop.sqlite");
    drop(rust_strategy_factory::storage::Registry::open(&db).unwrap());
    let c = rusqlite::Connection::open(&db).unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    c.execute(
        "ATTACH DATABASE ?1 AS fixture",
        [template.to_str().unwrap()],
    )
    .unwrap();
    for table in [
        "corpus_searches",
        "corpus_search_runtime",
        "corpus_search_transitions",
        "corpus_search_jobs",
        "corpus_retry_audit_heads",
    ] {
        c.execute(
            &format!("INSERT INTO {table} SELECT * FROM fixture.{table}"),
            [],
        )
        .unwrap();
    }
    c.execute_batch("DETACH DATABASE fixture;").unwrap();
    let fleet = f.root.join("loop-fleet.json");
    fs::write(&fleet,serde_json::to_vec(&serde_json::json!({"sources":[{"name":"synthetic-admission-fault","config":config}],"coordinator_lease_ms":60000})).unwrap()).unwrap();
    let id = rust_strategy_factory::corpus_fleet::register_file(&fleet, &db).unwrap();
    let (_, limits) = owned_fleet_files(&f.root);
    rust_strategy_factory::storage::Registry::open(&db)
        .unwrap()
        .integrity()
        .unwrap();
    (db, policy, limits, id, source)
}
fn fleet_loop_command(
    db: &std::path::Path,
    id: &str,
    policy: &std::path::Path,
    limits: &std::path::Path,
    wait: &str,
) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"));
    c.args([
        "corpus-fleet-loop",
        db.to_str().unwrap(),
        id,
        policy.to_str().unwrap(),
        limits.to_str().unwrap(),
        "100",
        wait,
    ]);
    c
}
#[test]
fn fleet_loop_wait_budget_restart_and_pure_polling_preserve_frozen_deadline() {
    let f = Fixture::new("loop-wait", "EURUSD", 7);
    let (db, policy, limits, id, source) = retryable_fleet_fixture(&f, 86400000);
    let first = fleet_loop_command(&db, &id, &policy, &limits, "100")
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(report["stop_reason"], "wait-budget");
    assert_eq!(report["wakeups"], "0");
    assert_eq!(report["visits"], "1");
    let c = rusqlite::Connection::open(&db).unwrap();
    let count = || {
        c.query_row("SELECT count(*) FROM corpus_retry_decisions", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap()
    };
    assert_eq!(count(), 1);
    let second = fleet_loop_command(&db, &id, &policy, &limits, "0")
        .output()
        .unwrap();
    assert!(second.status.success());
    let next: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(next["stop_reason"], "retry-backoff");
    assert_eq!(next["last_wakeup"], report["last_wakeup"]);
    assert_eq!(next["visits"], "0");
    assert_eq!(count(), 1);
    let reg = rust_strategy_factory::storage::Registry::open(&db).unwrap();
    assert_eq!(
        rust_strategy_factory::corpus_search::status(&reg, &source).unwrap()["requeue_records"],
        "0"
    );
    assert_eq!(
        rust_strategy_factory::owned_history::list(&reg)
            .unwrap()
            .len(),
        1
    );
    reg.integrity().unwrap();
}
#[test]
fn fleet_loop_ready_retry_executes_once_then_preserves_new_permanent_failure_hold() {
    let f = Fixture::new("loop-ready", "EURUSD", 7);
    let (db, policy, limits, id, source) = retryable_fleet_fixture(&f, 1000);
    let out = fleet_loop_command(&db, &id, &policy, &limits, "10000")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["stop_reason"], "retry-held");
    assert_eq!(report["wakeups"], "1");
    let reg = rust_strategy_factory::storage::Registry::open(&db).unwrap();
    let status = rust_strategy_factory::corpus_search::status(&reg, &source).unwrap();
    assert_eq!(status["requeue_records"], "2");
    assert!(
        status["recent_slots"]
            .as_array()
            .unwrap()
            .iter()
            .all(|j| j["error_kind"] == "unclassified")
    );
    assert_eq!(
        rust_strategy_factory::owned_history::list(&reg)
            .unwrap()
            .len(),
        2
    );
    reg.integrity().unwrap();
}
#[test]
fn fleet_loop_operator_pause_interrupts_wait_without_resuming_or_spawning_more_children() {
    fleet_loop_intervention(false);
}
#[test]
#[cfg(unix)]
fn fleet_loop_sigterm_interrupts_wait_without_fabricating_progress() {
    fleet_loop_intervention(true);
}
fn fleet_loop_intervention(signal: bool) {
    use std::io::BufRead;
    let f = Fixture::new(
        if signal {
            "loop-signal"
        } else {
            "loop-operator"
        },
        "EURUSD",
        7,
    );
    let (db, policy, limits, id, _) = retryable_fleet_fixture(&f, 86400000);
    let mut child = fleet_loop_command(&db, &id, &policy, &limits, "10000")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufReader::new(child.stderr.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.contains("fleet supervision waiting"));
    if signal {
        assert!(
            Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .status()
                .unwrap()
                .success()
        );
    } else {
        rust_strategy_factory::corpus_fleet::control(
            &mut rust_strategy_factory::storage::Registry::open(&db).unwrap(),
            &id,
            "paused",
        )
        .unwrap();
    }
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["stop_reason"],
        if signal {
            "signal"
        } else {
            "fleet-control-changed"
        }
    );
    assert_eq!(report["wakeups"], "0");
    let reg = rust_strategy_factory::storage::Registry::open(&db).unwrap();
    assert_eq!(
        rust_strategy_factory::owned_history::list(&reg)
            .unwrap()
            .len(),
        1
    );
    if !signal {
        assert_eq!(report["status"]["control"]["pause_origin"], "operator");
    }
    reg.integrity().unwrap();
}

#[test]
fn fleet_loop_refuses_policy_retuning_and_invalid_wait_before_new_work() {
    let f = Fixture::new("loop-policy", "EURUSD", 7);
    let (db, policy, limits, id, _) = retryable_fleet_fixture(&f, 86400000);
    let out = fleet_loop_command(&db, &id, &policy, &limits, "0")
        .output()
        .unwrap();
    assert!(out.status.success());
    let reg = rust_strategy_factory::storage::Registry::open(&db).unwrap();
    let before = rust_strategy_factory::corpus_fleet::status(&reg, &id).unwrap();
    let owners = rust_strategy_factory::owned_history::list(&reg).unwrap();
    assert!(
        !fleet_loop_command(&db, &id, &policy, &limits, "3600001")
            .output()
            .unwrap()
            .status
            .success()
    );
    fs::write(&policy, r#"{"backoff_ms":1,"maximum_slots":2}"#).unwrap();
    let out = fleet_loop_command(&db, &id, &policy, &limits, "100")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("immutable source retry policy"));
    assert_eq!(
        rust_strategy_factory::corpus_fleet::status(&reg, &id).unwrap(),
        before
    );
    assert_eq!(
        rust_strategy_factory::owned_history::list(&reg).unwrap(),
        owners
    );
    reg.integrity().unwrap();
}

fn block_spec(f: &Fixture, mixed: bool) -> Specification {
    let paths = f
        .spec
        .chunks
        .iter()
        .enumerate()
        .map(|(i, chunk)| {
            let native = std::path::Path::new(&chunk.path);
            if mixed && i % 2 == 1 {
                return native.to_path_buf();
            }
            let path = f.root.join(format!("block-{i}.rsfb"));
            rust_strategy_factory::blocked_native::write(&path, &native::read(native).unwrap(), 2)
                .unwrap();
            path
        })
        .collect::<Vec<_>>();
    corpus::describe(&paths, 2000000).unwrap()
}
#[test]
fn block_corpora_preserve_all_instrument_contexts_signals_and_legacy_identity() {
    for symbol in rust_strategy_factory::contracts::SYMBOLS {
        let f = Fixture::new("blocks-context", symbol, 7);
        let spec = block_spec(&f, false);
        assert_eq!(spec.version, "ordered-block-corpus-v2");
        let actual = corpus::inspect(&f.root, &spec).unwrap();
        let reference = corpus::inspect(&f.root, &f.spec).unwrap();
        assert_eq!(actual.ordered_events_hash, reference.ordered_events_hash);
        assert_eq!(actual.events, reference.events);
        assert_eq!(actual.max_resident_quote_events, 2);
        assert_eq!(
            data::hash(&actual.candles).unwrap(),
            data::hash(&reference.candles).unwrap()
        );
        assert_eq!(
            f.spec.identity().unwrap(),
            data::hash(&(
                "ordered-native-corpus-v1",
                f.spec
                    .chunks
                    .iter()
                    .map(|c| (&c.dataset_identity, &c.file_hash))
                    .collect::<Vec<_>>()
            ))
            .unwrap()
        );
        assert!(
            !serde_json::to_string(&f.spec)
                .unwrap()
                .contains("block_root")
        );
        for slot in 0..18 {
            let (_, strategy) = rust_strategy_factory::research::candidate_at(slot, 42).unwrap();
            assert_eq!(
                data::hash(&corpus::signals(&f.root, &spec, &strategy, 10000, 100000).unwrap())
                    .unwrap(),
                data::hash(&corpus::signals(&f.root, &f.spec, &strategy, 10000, 100000).unwrap())
                    .unwrap()
            );
        }
        let mut changed = spec.clone();
        changed.chunks[0].block_root = Some("0".repeat(64));
        assert_ne!(changed.identity().unwrap(), spec.identity().unwrap());
        assert!(corpus::inspect(&f.root, &changed).is_err());
    }
}
#[test]
fn block_account_replay_preserves_every_market_stop_limit_receipt_and_nonzero_trades() {
    use rust_strategy_factory::{
        strategies::{EntryPlan, Strategy},
        tick_engine::{self, CorpusBudget, Costs},
    };
    let mut trades = 0;
    for symbol in ["EURUSD", "XAUUSD"] {
        let f = Fixture::new("blocks-account", symbol, 7);
        let spec = block_spec(&f, false);
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
            let budget = CorpusBudget {
                max_candles: 100,
                feature_cells: 10000,
                max_receipts: 1000,
            };
            let reference =
                tick_engine::replay_corpus(&f.root, &f.spec, &strategy, &Costs::default(), budget)
                    .unwrap();
            let actual =
                tick_engine::replay_corpus(&f.root, &spec, &strategy, &Costs::default(), budget)
                    .unwrap();
            trades += actual.trades.len();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(reference).unwrap()
            );
        }
    }
    assert!(trades > 0);
}
#[test]
fn mixed_block_native_reopen_checkpoints_advance_only_complete_artifacts() {
    use rust_strategy_factory::{
        storage::Registry,
        strategies::Strategy,
        tick_engine::{self, CorpusBudget, CorpusProgress, CorpusRun, Costs},
    };
    let f = Fixture::new("blocks-checkpoint", "EURUSD", 7);
    let spec = block_spec(&f, true);
    let strategy:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    let costs = Costs::default();
    let budget = CorpusBudget {
        max_candles: 100,
        feature_cells: 10000,
        max_receipts: 1000,
    };
    let db = f.root.join("blocks.sqlite");
    let mut completed = false;
    for index in 0..spec.chunks.len() {
        let mut reg = Registry::open(&db).unwrap();
        let progress = tick_engine::execute_corpus_step(
            &mut reg,
            CorpusRun {
                base: &f.root,
                spec: &spec,
                strategy: &strategy,
                costs: &costs,
                budget,
            },
            1,
            10000,
            || 1,
        )
        .unwrap();
        match progress {
            CorpusProgress::Pending { experiment_id, .. } => {
                assert_eq!(
                    reg.replay_checkpoint(&experiment_id)
                        .unwrap()
                        .unwrap()
                        .0
                        .cursor,
                    index as i64 + 1
                );
            }
            CorpusProgress::Completed { result, .. } => {
                assert_eq!(
                    result.result["tick_replay"],
                    serde_json::to_value(
                        tick_engine::replay_corpus(&f.root, &f.spec, &strategy, &costs, budget)
                            .unwrap()
                    )
                    .unwrap()
                );
                completed = true;
            }
            _ => panic!("unexpected held/busy"),
        };
        reg.integrity().unwrap();
    }
    assert!(completed);
    let mut visited = 0;
    corpus::visit_chunks(&f.root, &spec, |_, d| {
        d.verify()?;
        visited += d.ticks.len();
        Ok(())
    })
    .unwrap();
    assert_eq!(visited, f.data.ticks.len());
}
#[test]
fn blocked_late_corruption_preserves_committed_checkpoint_and_attempt_counts() {
    use rust_strategy_factory::{
        storage::Registry,
        strategies::Strategy,
        tick_engine::{self, CorpusBudget, CorpusProgress, CorpusRun, Costs},
    };
    let f = Fixture::new("blocks-corruption", "EURUSD", 7);
    let spec = block_spec(&f, false);
    let strategy:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    let costs = Costs::default();
    let budget = CorpusBudget {
        max_candles: 100,
        feature_cells: 10000,
        max_receipts: 1000,
    };
    let mut reg = Registry::open(&f.root.join("blocks.sqlite")).unwrap();
    let run = || CorpusRun {
        base: &f.root,
        spec: &spec,
        strategy: &strategy,
        costs: &costs,
        budget,
    };
    let CorpusProgress::Pending { experiment_id, .. } =
        tick_engine::execute_corpus_step(&mut reg, run(), 1, 10000, || 1).unwrap()
    else {
        panic!()
    };
    let saved = reg.replay_checkpoint(&experiment_id).unwrap().unwrap();
    let attempts = reg.counts().unwrap().attempts;
    let path = std::path::Path::new(&spec.chunks.last().unwrap().path);
    let mut bytes = fs::read(path).unwrap();
    let end = bytes.len() - 1;
    bytes[end] ^= 1;
    fs::write(path, bytes).unwrap();
    assert!(tick_engine::execute_corpus_step(&mut reg, run(), 1, 10000, || 2).is_err());
    let after = reg.replay_checkpoint(&experiment_id).unwrap().unwrap();
    assert_eq!(data::hash(&saved).unwrap(), data::hash(&after).unwrap());
    assert_eq!(reg.counts().unwrap().attempts, attempts);
    reg.integrity().unwrap();
}
#[test]
fn actual_block_corpus_cli_and_materialized_loader_preserve_canonical_experiment_reuse() {
    let f = Fixture::new("blocks-cli", "EURUSD", 7);
    let path = f.root.join("full.rsfb");
    rust_strategy_factory::blocked_native::write(&path, &f.data, 3).unwrap();
    let json = f.root.join("full.json");
    fs::write(&json, serde_json::to_vec(&f.data).unwrap()).unwrap();
    let strategy = f.root.join("strategy.json");
    fs::write(
        &strategy,
        r#"{"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}}"#,
    )
    .unwrap();
    let db = f.root.join("materialized.sqlite");
    let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
    let invoke = |args: &[&str]| {
        let out = Command::new(binary).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let first = invoke(&[
        "experiment",
        json.to_str().unwrap(),
        strategy.to_str().unwrap(),
        db.to_str().unwrap(),
    ]);
    let second = invoke(&[
        "experiment",
        path.to_str().unwrap(),
        strategy.to_str().unwrap(),
        db.to_str().unwrap(),
    ]);
    assert_eq!(first["experiment_id"], second["experiment_id"]);
    assert_eq!(second["reused"], true);
    let spec = f.root.join("block-corpus.json");
    let out = Command::new(binary)
        .args([
            "corpus-create-bounded",
            spec.to_str().unwrap(),
            "2000000",
            path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = invoke(&["corpus-inspect", spec.to_str().unwrap()]);
    assert_eq!(report["max_resident_quote_events"], 3);
    let source = corpus::load(&spec).unwrap();
    assert!(source.chunks[0].block_root.is_some());
}
#[test]
fn blocked_artifact_above_legacy_chunk_cap_has_bounded_resident_quotes() {
    let f = Fixture::new("blocks-large", "EURUSD", 7);
    let p = f.data.profile.clone();
    let ticks = (0..100001u64)
        .map(|i| Tick {
            timestamp_ms: 1735689600000 + (i / 2) as i64 * 1000,
            sequence: u64::MAX - 100000 + i,
            bid: p.price_scale + (i % 10) as i64 * p.pip,
            ask: p.price_scale + (i % 10) as i64 * p.pip + p.pip,
        })
        .collect();
    let d = data::dataset(
        p,
        ticks,
        "SYNTHETIC_TEST_ONLY block resident fixture",
        "generated".into(),
        true,
    )
    .unwrap();
    let path = f.root.join("large.rsfb");
    rust_strategy_factory::blocked_native::write(&path, &d, 256).unwrap();
    let spec = corpus::describe(std::slice::from_ref(&path), 2000000).unwrap();
    let report = corpus::inspect(&f.root, &spec).unwrap();
    assert_eq!(report.events, 100001);
    assert_eq!(report.max_resident_quote_events, 256);
    assert_eq!(report.ordered_events_hash, data::hash(&d.ticks).unwrap());
    let mut rows = 0;
    let mut endings = 0;
    corpus::visit_blocks(&f.root, &spec, |block| {
        assert!(block.ticks.len() <= 256);
        rows += block.ticks.len();
        endings += usize::from(block.last_in_chunk);
        Ok(())
    })
    .unwrap();
    assert_eq!(rows, 100001);
    assert_eq!(endings, 1);
    let old = f.root.join("large.rsf");
    native::write(&old, &d).unwrap();
    assert!(corpus::describe(&[old, path], 2000000).is_err());
}

#[test]
fn corpus_search_cli_later_range_reopens_and_reuses_exact_scalar_receipts() {
    use rust_strategy_factory::{
        corpus_search, operator_config, research, storage::Registry, tick_engine,
    };
    let f = Fixture::new("later-range-cli", "EURUSD", 7);
    let config = corpus_search_fixture(&f);
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    value["start_index"] = 12.into();
    value["maximum_candidates"] = 2.into();
    fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    let db = f.root.join("later.sqlite");
    let call = || {
        let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args([
                "corpus-search",
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
    let first = call();
    let loaded = operator_config::load_search(&config).unwrap();
    let id = corpus_search::identity(&loaded.run()).unwrap();
    let registry = Registry::open(&db).unwrap();
    let before = registry.counts().unwrap();
    let snapshot = corpus_search::status(&registry, &id).unwrap();
    assert_eq!(snapshot["state"], "exhausted", "{first}");
    assert_eq!(snapshot["start_index"], "12");
    for index in 12..14 {
        let strategy = research::candidate_at(index, 42).unwrap().1;
        let key = tick_engine::corpus_key(
            &loaded.base,
            &loaded.specification,
            &strategy,
            &loaded.costs,
            loaded.file.budget,
        )
        .unwrap();
        let stored = registry.result(&key.identity().unwrap()).unwrap().unwrap();
        assert_eq!(
            stored.result["tick_replay"],
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
    drop(registry);
    call();
    let registry = Registry::open(&db).unwrap();
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(registry.counts().unwrap()).unwrap()
    );
    registry.integrity().unwrap();
}

#[test]
fn frozen_campaign_cli_binds_complete_trial_scope_preserves_holds_and_rejects_retuning() {
    use rust_strategy_factory::{
        corpus_campaign, corpus_search, data, operator_config, research, storage::Registry,
        tick_engine,
    };
    let f = Fixture::new("campaign-freeze-cli", "EURUSD", 7);
    let config = corpus_search_fixture(&f);
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    value["seed"] = serde_json::json!(u64::MAX);
    value["start_index"] = 12.into();
    value["maximum_candidates"] = 2.into();
    fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    let db = f.root.join("campaign.sqlite");
    let call = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    let first = call(&[
        "corpus-campaign-freeze",
        config.to_str().unwrap(),
        db.to_str().unwrap(),
        "3",
    ]);
    let id = first["campaign_id"].as_str().unwrap();
    assert_eq!(first["seed"], u64::MAX.to_string());
    assert_eq!(first["declared_slots"], "6");
    assert_eq!(first["execution_supported"], true);
    assert_eq!(first["inputs_freshly_verified"], false);
    assert_eq!(
        first,
        call(&[
            "corpus-campaign-freeze",
            config.to_str().unwrap(),
            db.to_str().unwrap(),
            "3"
        ])
    );
    let registry = Registry::open(&db).unwrap();
    assert_eq!(registry.counts().unwrap().registered, 0);
    assert!(corpus_search::list(&registry).unwrap().is_empty());
    let loaded = operator_config::load_search(&config).unwrap();
    let mut expected = Vec::new();
    for index in 12..18 {
        let strategy = research::candidate_at(index, u64::MAX).unwrap().1;
        expected.push(
            tick_engine::corpus_key(
                &loaded.base,
                &loaded.specification,
                &strategy,
                &loaded.costs,
                loaded.file.budget,
            )
            .unwrap()
            .identity()
            .unwrap(),
        );
    }
    assert_eq!(first["trial_scope_hash"], data::hash(&expected).unwrap());
    assert_eq!(
        first["distinct_experiments"],
        expected
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            .to_string()
    );
    assert_eq!(
        corpus_campaign::verify_inputs(&registry, id).unwrap()["inputs_freshly_verified"],
        true
    );
    registry.backup(&f.root.join("backup.sqlite")).unwrap();
    let copied = Registry::open(&f.root.join("backup.sqlite")).unwrap();
    assert_eq!(corpus_campaign::status(&copied, id).unwrap(), first);
    let region = &first["regions"][1];
    assert_eq!(region["start_index"], "14");
    let mut run = loaded.run();
    run.start_index = 14;
    assert_eq!(region["search_id"], corpus_search::identity(&run).unwrap());
    let policy = rust_strategy_factory::retry_policy::Policy {
        backoff_ms: 1,
        maximum_slots: 1,
    };
    let search_id = corpus_search::prepare_supervision(&db, &run, &policy).unwrap();
    let mut held_registry = Registry::open(&db).unwrap();
    corpus_search::control(&mut held_registry, &search_id, "paused").unwrap();
    let held = corpus_search::status(&held_registry, &search_id).unwrap();
    assert_eq!(corpus_campaign::register_file(&config, &db, 3).unwrap(), id);
    corpus_campaign::verify_inputs(&held_registry, id).unwrap();
    assert_eq!(
        corpus_search::status(&held_registry, &search_id).unwrap(),
        held
    );
    assert_eq!(held_registry.counts().unwrap().registered, 0);
    value["seed"] = 0.into();
    fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(corpus_campaign::verify_inputs(&registry, id).is_err());
    assert_eq!(corpus_campaign::status(&registry, id).unwrap(), first);
    registry.integrity().unwrap();
    let invalid_db = f.root.join("invalid.sqlite");
    value["start_index"] = serde_json::json!(u64::MAX);
    fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(corpus_campaign::register_file(&config, &invalid_db, 3).is_err());
    assert!(!invalid_db.exists());
    assert!(corpus_campaign::register_file(&config, &invalid_db, 17).is_err());
    assert!(!invalid_db.exists());
}

#[test]
fn campaign_foreground_cli_replenishes_frozen_regions_preserves_holds_and_restarts() {
    use rust_strategy_factory::{
        corpus_campaign, corpus_search, operator_config, research, storage::Registry, tick_engine,
    };
    let f = Fixture::new("campaign-execution-cli", "EURUSD", 7);
    let config = corpus_search_fixture(&f);
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    value["start_index"] = 12.into();
    value["maximum_candidates"] = 1.into();
    value["queue_width"] = 1.into();
    fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    let db = f.root.join("campaign.sqlite");
    let id = corpus_campaign::register_file(&config, &db, 3).unwrap();
    let call = |rounds: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args(["corpus-campaign-run", db.to_str().unwrap(), &id, rounds])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
    };
    value["resource_guard"] =
        serde_json::json!({"maximum_process_rss_bytes":1,"minimum_host_available_memory_bytes":0});
    fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(call("100")["stop_reason"], "pressure");
    let mut registry = Registry::open(&db).unwrap();
    let before = corpus_campaign::status(&registry, &id).unwrap();
    assert_eq!(before["execution"]["attempt"], "0");
    assert!(corpus_search::list(&registry).unwrap().is_empty());
    value.as_object_mut().unwrap().remove("resource_guard");
    fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    call("1");
    corpus_campaign::control(&mut registry, &id, "paused").unwrap();
    let paused = corpus_campaign::status(&registry, &id).unwrap();
    let attempts = registry.counts().unwrap().attempts;
    assert_eq!(call("100")["stop_reason"], "held");
    assert_eq!(corpus_campaign::status(&registry, &id).unwrap(), paused);
    assert_eq!(registry.counts().unwrap().attempts, attempts);
    corpus_campaign::control(&mut registry, &id, "running").unwrap();
    let current = corpus_campaign::status(&registry, &id).unwrap()["execution"]["next_region"]
        .as_str()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let loaded = operator_config::load_search(&config).unwrap();
    let mut run = loaded.run();
    run.start_index = 12 + current as u64;
    let policy = rust_strategy_factory::retry_policy::Policy {
        backoff_ms: 1,
        maximum_slots: 1,
    };
    let search_id = corpus_search::prepare_supervision(&db, &run, &policy).unwrap();
    corpus_search::control(&mut registry, &search_id, "paused").unwrap();
    let child_held = corpus_search::status(&registry, &search_id).unwrap();
    assert_eq!(call("100")["stop_reason"], "held");
    assert_eq!(
        corpus_search::status(&registry, &search_id).unwrap(),
        child_held
    );
    corpus_search::control(&mut registry, &search_id, "running").unwrap();
    drop(registry);
    assert_eq!(call("100")["stop_reason"], "exhausted");
    let mut registry = Registry::open(&db).unwrap();
    let final_status = corpus_campaign::status(&registry, &id).unwrap();
    assert_eq!(final_status["execution"]["state"], "exhausted");
    assert_eq!(final_status["execution"]["next_region"], "3");
    assert_eq!(
        final_status["execution"]["completed_regions"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(final_status["execution"]["lease_active"], false);
    assert!(
        !serde_json::to_string(&final_status)
            .unwrap()
            .contains("token")
    );
    for index in 12..15 {
        let strategy = research::candidate_at(index, 42).unwrap().1;
        let key = tick_engine::corpus_key(
            &loaded.base,
            &loaded.specification,
            &strategy,
            &loaded.costs,
            loaded.file.budget,
        )
        .unwrap();
        let receipt = registry.result(&key.identity().unwrap()).unwrap().unwrap();
        assert_eq!(
            receipt.result["tick_replay"],
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
    let attempts = registry.counts().unwrap().attempts;
    assert_eq!(call("100")["stop_reason"], "exhausted");
    assert_eq!(attempts, registry.counts().unwrap().attempts);
    assert_eq!(
        corpus_campaign::status(&registry, &id).unwrap(),
        final_status
    );
    assert!(corpus_campaign::control(&mut registry, &id, "running").is_err());
    registry.integrity().unwrap();
    registry
        .backup(&f.root.join("completed-copy.sqlite"))
        .unwrap();
    let copied = Registry::open(&f.root.join("completed-copy.sqlite")).unwrap();
    assert_eq!(corpus_campaign::status(&copied, &id).unwrap(), final_status);
}

#[test]
fn pinned_child_rejects_retuned_inputs_before_registry_creation_and_keeps_operator_files() {
    use rust_strategy_factory::{
        corpus_search, operator_config, owned_process::InputPin, retry_policy::Policy,
        storage::Registry,
    };
    let f = Fixture::new("owned-input-pin", "EURUSD", 7);
    let config = corpus_search_fixture(&f);
    let original = fs::read(&config).unwrap();
    let policy = Policy {
        backoff_ms: 500,
        maximum_slots: 1,
    };
    let policy_path = f.root.join("policy.json");
    fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
    let loaded = operator_config::load_search(&config).unwrap();
    let pin = InputPin::freeze(&loaded, &policy, Some(12)).unwrap();
    let text = serde_json::to_string(&pin).unwrap();
    let db = f.root.join("must-not-exist.sqlite");
    let invoke = |pin_text: &str, database: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args([
                "corpus-supervise-pinned",
                config.to_str().unwrap(),
                policy_path.to_str().unwrap(),
                database.to_str().unwrap(),
                "1",
                "0",
                pin_text,
            ])
            .output()
            .unwrap()
    };
    let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    value["seed"] = 43.into();
    fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(!invoke(&text, &db).status.success());
    assert!(!db.exists());
    value = serde_json::from_slice(&original).unwrap();
    value["start_index"] = 100.into();
    fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    let mut current = operator_config::load_search(&config).unwrap();
    assert!(pin.validate_and_apply(&mut current, &policy).is_err());
    assert_eq!(current.file.start_index, 100);
    assert!(!invoke(&text, &db).status.success());
    assert!(!db.exists());
    value = serde_json::from_slice(&original).unwrap();
    value["limits"]["max_new_chunks"] = 2.into();
    fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(!invoke(&text, &db).status.success());
    assert!(!db.exists());
    fs::write(&config, &original).unwrap();
    fs::write(
        &policy_path,
        serde_json::to_vec(&Policy {
            backoff_ms: 501,
            maximum_slots: 1,
        })
        .unwrap(),
    )
    .unwrap();
    assert!(!invoke(&text, &db).status.success());
    assert!(!db.exists());
    fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
    let mut wrong = serde_json::to_value(&pin).unwrap();
    wrong["source_id"] = "f".repeat(64).into();
    assert!(
        !invoke(&serde_json::to_string(&wrong).unwrap(), &db)
            .status
            .success()
    );
    assert!(!db.exists());
    let mut unknown = serde_json::to_value(&pin).unwrap();
    unknown["metrics"] = serde_json::json!({"qualified":true});
    assert!(
        !invoke(&serde_json::to_string(&unknown).unwrap(), &db)
            .status
            .success()
    );
    assert!(!db.exists());
    let database = f.root.join("valid.sqlite");
    let out = invoke(&text, &database);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let registry = Registry::open(&database).unwrap();
    let mut expected = loaded.run();
    expected.start_index = 12;
    let expected = corpus_search::identity(&expected).unwrap();
    let rows = corpus_search::list(&registry).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["search_id"], expected);
    assert_eq!(fs::read(&config).unwrap(), original);
    registry.integrity().unwrap();
}
#[test]
fn owned_region_process_pins_effective_identity_and_preserves_base_operator_file() {
    use rust_strategy_factory::{corpus_search, operator_config, owned_history, storage::Registry};
    let f = Fixture::new("owned-region-process", "EURUSD", 7);
    let config = corpus_search_fixture(&f);
    let original = fs::read(&config).unwrap();
    let loaded = operator_config::load_search(&config).unwrap();
    let mut run = loaded.run();
    run.start_index = 12;
    let source_id = corpus_search::identity(&run).unwrap();
    let db = f.root.join("owned.sqlite");
    let policy = f.root.join("policy.json");
    let limits = f.root.join("parent.json");
    fs::write(&policy, br#"{"backoff_ms":1,"maximum_slots":1}"#).unwrap();
    fs::write(
        &limits,
        br#"{"maximum_wall_ms":30000,"shutdown_grace_ms":1000,"maximum_output_bytes":1048576}"#,
    )
    .unwrap();
    let invalid = f.root.join("invalid.sqlite");
    let invoke = |database: &std::path::Path, expected: &str| {
        Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args([
                "corpus-supervise-region-owned",
                config.to_str().unwrap(),
                policy.to_str().unwrap(),
                database.to_str().unwrap(),
                "1",
                "0",
                limits.to_str().unwrap(),
                "12",
                expected,
            ])
            .output()
            .unwrap()
    };
    assert!(!invoke(&invalid, &"f".repeat(64)).status.success());
    assert!(!invalid.exists());
    let output = invoke(&db, &source_id);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["exit_success"], true, "{report}");
    assert_eq!(report["reaped"], true);
    assert_eq!(report["search_id"], source_id);
    assert_eq!(report["input_pin"]["source_id"], source_id);
    assert_eq!(report["input_pin"]["start_index"], 12);
    assert_ne!(
        report["input_pin"]["operator_hash"],
        report["input_pin"]["effective_operator_hash"]
    );
    assert_eq!(report["session_status"]["search_id"], source_id);
    let registry = Registry::open(&db).unwrap();
    let rows = corpus_search::list(&registry).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["search_id"], source_id);
    let stored =
        owned_history::status(&registry, report["invocation_id"].as_str().unwrap()).unwrap();
    assert_eq!(stored["summary"]["input_pin"], report["input_pin"]);
    assert_eq!(fs::read(&config).unwrap(), original);
    registry.integrity().unwrap();
}

fn campaign_owned_fixture(
    f: &Fixture,
    regions: usize,
) -> (PathBuf, PathBuf, String, PathBuf, PathBuf) {
    let config = corpus_search_fixture(f);
    let mut file: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    file["start_index"] = 12.into();
    file["maximum_candidates"] = 1.into();
    file["queue_width"] = 1.into();
    fs::write(&config, serde_json::to_vec(&file).unwrap()).unwrap();
    let db = f.root.join("campaign-owned.sqlite");
    let id = rust_strategy_factory::corpus_campaign::register_file(&config, &db, regions).unwrap();
    let policy = f.root.join("policy.json");
    let limits = f.root.join("limits.json");
    fs::write(&policy, br#"{"backoff_ms":1,"maximum_slots":1}"#).unwrap();
    fs::write(
        &limits,
        br#"{"maximum_wall_ms":30000,"shutdown_grace_ms":1000,"maximum_output_bytes":1048576}"#,
    )
    .unwrap();
    (config, db, id, policy, limits)
}
fn campaign_owned_command(
    db: &std::path::Path,
    id: &str,
    policy: &std::path::Path,
    limits: &std::path::Path,
    maximum: &str,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-campaign-supervise",
            db.to_str().unwrap(),
            id,
            policy.to_str().unwrap(),
            limits.to_str().unwrap(),
            maximum,
        ])
        .output()
        .unwrap()
}
#[test]
fn campaign_owned_regions_link_actual_parents_and_scalar_receipts_with_restart_and_backup() {
    use rust_strategy_factory::{
        corpus_campaign, operator_config, owned_history, research, storage::Registry, tick_engine,
    };
    let f = Fixture::new("campaign-owned-complete", "EURUSD", 7);
    let (config, db, id, policy, limits) = campaign_owned_fixture(&f, 2);
    let original = fs::read(&config).unwrap();
    let first = campaign_owned_command(&db, &id, &policy, &limits, "1");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["children_started"], "1");
    // The first supervise starts an owned child; under full-suite parallel load the next
    // supervise can observe the campaign before the scheduler advances it. Retry the
    // non-waiting supervise until the campaign actually exhausts; the assertion is unchanged.
    let rest = {
        let mut attempts = 0;
        loop {
            let out = campaign_owned_command(&db, &id, &policy, &limits, "100");
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            if value["stop_reason"] == "exhausted" {
                break value;
            }
            attempts += 1;
            assert!(
                attempts < 50,
                "campaign did not exhaust, stop_reason={}",
                value["stop_reason"]
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    };
    assert_eq!(rest["stop_reason"], "exhausted");
    let registry = Registry::open(&db).unwrap();
    let status = corpus_campaign::status(&registry, &id).unwrap();
    let visits = status["execution"]["owned_visits"].as_array().unwrap();
    assert!(!visits.is_empty());
    assert_eq!(
        status["execution"]["completed_regions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        visits
            .iter()
            .filter(|v| !v["advance_hash"].is_null())
            .count(),
        2
    );
    assert_eq!(owned_history::list(&registry).unwrap().len(), visits.len());
    for visit in visits {
        let parent =
            owned_history::status(&registry, visit["invocation_id"].as_str().unwrap()).unwrap();
        let region = visit["region"].as_str().unwrap().parse::<usize>().unwrap();
        assert_eq!(parent["search_id"], status["regions"][region]["search_id"]);
        assert_eq!(
            parent["summary"]["input_pin"]["source_id"],
            parent["search_id"]
        );
        assert_eq!(parent["summary"]["reaped"], true);
        assert_eq!(parent["observation_hash"], visit["parent_observation_hash"]);
        assert_eq!(parent["summary"]["report_hash"], visit["report_hash"]);
    }
    let loaded = operator_config::load_search(&config).unwrap();
    for index in 12..14 {
        let strategy = research::candidate_at(index, 42).unwrap().1;
        let key = tick_engine::corpus_key(
            &loaded.base,
            &loaded.specification,
            &strategy,
            &loaded.costs,
            loaded.file.budget,
        )
        .unwrap();
        let stored = registry.result(&key.identity().unwrap()).unwrap().unwrap();
        assert_eq!(
            stored.result["tick_replay"],
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
    let attempts = registry.counts().unwrap().attempts;
    let repeated = campaign_owned_command(&db, &id, &policy, &limits, "100");
    assert!(repeated.status.success());
    let repeated: serde_json::Value = serde_json::from_slice(&repeated.stdout).unwrap();
    assert_eq!(repeated["children_started"], "0");
    assert_eq!(registry.counts().unwrap().attempts, attempts);
    assert_eq!(corpus_campaign::status(&registry, &id).unwrap(), status);
    assert_eq!(fs::read(&config).unwrap(), original);
    registry.integrity().unwrap();
    registry.backup(&f.root.join("copy.sqlite")).unwrap();
    let copied = Registry::open(&f.root.join("copy.sqlite")).unwrap();
    assert_eq!(corpus_campaign::status(&copied, &id).unwrap(), status);
    drop(copied);
    let c = rusqlite::Connection::open(&db).unwrap();
    assert!(
        c.execute(
            "DELETE FROM corpus_campaign_owned_visits WHERE campaign_id=?1",
            [&id]
        )
        .is_err()
    );
    c.execute(
        "UPDATE corpus_campaign_owned_heads SET hash=?2 WHERE campaign_id=?1",
        rusqlite::params![id, "0".repeat(64)],
    )
    .unwrap();
    assert!(corpus_campaign::status(&registry, &id).is_err());
    assert!(registry.integrity().is_err());
}
#[test]
fn campaign_owned_publication_failure_preserves_unlinked_parent_and_completed_source() {
    use rust_strategy_factory::{corpus_campaign, corpus_search, owned_history, storage::Registry};
    let f = Fixture::new("campaign-owned-rollback", "EURUSD", 1);
    let (_, db, id, policy, limits) = campaign_owned_fixture(&f, 1);
    let c = rusqlite::Connection::open(&db).unwrap();
    c.execute_batch("CREATE TRIGGER injected_campaign_owned BEFORE INSERT ON corpus_campaign_owned_visits WHEN NEW.advance_region IS NOT NULL BEGIN SELECT RAISE(ABORT,'injected owned link failure');END;").unwrap();
    let mut injected_failure = None;
    for _ in 0..100 {
        let attempt = campaign_owned_command(&db, &id, &policy, &limits, "100");
        if !attempt.status.success() {
            injected_failure = Some(attempt);
            break;
        }
        let report: serde_json::Value = serde_json::from_slice(&attempt.stdout).unwrap();
        let observed = Registry::open(&db).unwrap();
        let campaign = corpus_campaign::status(&observed, &id).unwrap();
        assert!(
            campaign["execution"]["completed_regions"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{report}"
        );
        let source = corpus_search::status(
            &observed,
            campaign["regions"][0]["search_id"].as_str().unwrap(),
        )
        .unwrap();
        let running = source["state"] == "running" && source["lease_active"] == false;
        let retryable = source["state"] == "paused"
            && source["control"]["pause_origin"] == "worker_error"
            && source["recent_slots"][0]["error_kind"] == "registry_admission_busy"
            && source["recent_slots"][0]["retry_candidate"] == true;
        assert!(
            running || retryable,
            "rollback fixture stopped before its fault: {report}; source {source}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let failed = injected_failure
        .expect("bounded source progress must exercise injected owned-link rollback");
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("injected owned link failure"));
    let registry = Registry::open(&db).unwrap();
    let before = corpus_campaign::status(&registry, &id).unwrap();
    assert_eq!(before["execution"]["next_region"], "0");
    assert!(
        before["execution"]["completed_regions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        before["execution"]["owned_visits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["advance_hash"].is_null())
    );
    assert_eq!(before["execution"]["lease_active"], false);
    let source = before["regions"][0]["search_id"].as_str().unwrap();
    assert_eq!(
        corpus_search::status(&registry, source).unwrap()["state"],
        "exhausted"
    );
    let parents = owned_history::list(&registry).unwrap();
    assert_eq!(
        parents.len(),
        before["execution"]["owned_visits"]
            .as_array()
            .unwrap()
            .len()
            + 1
    );
    assert_eq!(parents[0]["summary"]["reaped"], true);
    let attempts = registry.counts().unwrap().attempts;
    c.execute_batch("DROP TRIGGER injected_campaign_owned;")
        .unwrap();
    let resumed = campaign_owned_command(&db, &id, &policy, &limits, "100");
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let resumed: serde_json::Value = serde_json::from_slice(&resumed.stdout).unwrap();
    assert_eq!(resumed["children_started"], "0");
    assert_eq!(resumed["stop_reason"], "exhausted");
    assert_eq!(registry.counts().unwrap().attempts, attempts);
    assert_eq!(owned_history::list(&registry).unwrap(), parents);
    registry.integrity().unwrap();
}
#[test]
fn campaign_owned_lease_refusal_backoff_and_operator_holds_start_no_children() {
    use rust_strategy_factory::{corpus_campaign, corpus_search, owned_history, storage::Registry};
    let f = Fixture::new("campaign-owned-holds", "EURUSD", 7);
    let (_, db, id, policy, limits) = campaign_owned_fixture(&f, 1);
    fs::write(
        &limits,
        br#"{"maximum_wall_ms":60000,"shutdown_grace_ms":1000,"maximum_output_bytes":1048576}"#,
    )
    .unwrap();
    let refused = campaign_owned_command(&db, &id, &policy, &limits, "1");
    assert!(!refused.status.success());
    let mut registry = Registry::open(&db).unwrap();
    assert_eq!(
        corpus_campaign::status(&registry, &id).unwrap()["execution"]["attempt"],
        "0"
    );
    assert!(corpus_search::list(&registry).unwrap().is_empty());
    assert!(owned_history::list(&registry).unwrap().is_empty());
    fs::write(
        &limits,
        br#"{"maximum_wall_ms":30000,"shutdown_grace_ms":1000,"maximum_output_bytes":1048576}"#,
    )
    .unwrap();
    corpus_campaign::control(&mut registry, &id, "paused").unwrap();
    let paused = corpus_campaign::status(&registry, &id).unwrap();
    let held = campaign_owned_command(&db, &id, &policy, &limits, "100");
    assert!(held.status.success());
    assert_eq!(corpus_campaign::status(&registry, &id).unwrap(), paused);
    // Development-only typed fault metadata, not a genuine historical busy episode.
    let other = Fixture::new("campaign-owned-backoff", "EURUSD", 7);
    let (config, db, policy, source) = foreground_config(&other);
    let id = corpus_campaign::register_file(&config, &db, 1).unwrap();
    let held = campaign_owned_command(&db, &id, &policy, &limits, "100");
    assert!(
        held.status.success(),
        "{}",
        String::from_utf8_lossy(&held.stderr)
    );
    let held: serde_json::Value = serde_json::from_slice(&held.stdout).unwrap();
    assert_eq!(held["stop_reason"], "backoff");
    assert_eq!(held["children_started"], "0");
    let mut registry = Registry::open(&db).unwrap();
    assert!(owned_history::list(&registry).unwrap().is_empty());
    assert_eq!(
        corpus_search::status(&registry, &source).unwrap()["requeue_records"],
        "0"
    );
    corpus_search::control(&mut registry, &source, "paused").unwrap();
    let source_held = corpus_search::status(&registry, &source).unwrap();
    let held = campaign_owned_command(&db, &id, &policy, &limits, "100");
    assert!(held.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&held.stdout).unwrap()["children_started"],
        "0"
    );
    assert_eq!(
        corpus_search::status(&registry, &source).unwrap(),
        source_held
    );
    registry.integrity().unwrap();
}

#[test]
fn campaign_owned_permanent_execution_error_is_not_retried_or_relaunched() {
    use rust_strategy_factory::{corpus_campaign, corpus_search, owned_history, storage::Registry};
    let f = Fixture::new("campaign-owned-permanent", "EURUSD", 7);
    let config = corpus_search_fixture(&f);
    let mut file: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    file["maximum_candidates"] = 1.into();
    file["queue_width"] = 1.into();
    file["budget"]["feature_cells"] = 1.into();
    file["limits"]["max_parallel_feature_cells"] = 2.into();
    fs::write(&config, serde_json::to_vec(&file).unwrap()).unwrap();
    let db = f.root.join("permanent.sqlite");
    let id = corpus_campaign::register_file(&config, &db, 1).unwrap();
    let policy = f.root.join("policy.json");
    let limits = f.root.join("limits.json");
    fs::write(&policy, br#"{"backoff_ms":1,"maximum_slots":1}"#).unwrap();
    fs::write(
        &limits,
        br#"{"maximum_wall_ms":30000,"shutdown_grace_ms":1000,"maximum_output_bytes":1048576}"#,
    )
    .unwrap();
    let first = campaign_owned_command(&db, &id, &policy, &limits, "100");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["stop_reason"], "held");
    assert_eq!(first["children_started"], "1");
    let registry = Registry::open(&db).unwrap();
    let snapshot = corpus_campaign::status(&registry, &id).unwrap();
    let source = snapshot["regions"][0]["search_id"].as_str().unwrap();
    let held = corpus_search::status(&registry, source).unwrap();
    assert_eq!(held["state"], "paused");
    assert_eq!(held["control"]["pause_origin"], "worker_error");
    assert_eq!(held["recent_slots"][0]["retry_candidate"], false);
    let parents = owned_history::list(&registry).unwrap();
    assert_eq!(parents.len(), 1);
    let second = campaign_owned_command(&db, &id, &policy, &limits, "100");
    assert!(second.status.success());
    let second: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(second["children_started"], "0");
    assert_eq!(second["stop_reason"], "held");
    assert_eq!(owned_history::list(&registry).unwrap(), parents);
    assert_eq!(corpus_search::status(&registry, source).unwrap(), held);
    registry.integrity().unwrap();
}

fn rotation_fixture(
    label: &str,
) -> (
    Fixture,
    Fixture,
    PathBuf,
    String,
    String,
    String,
    PathBuf,
    PathBuf,
    PathBuf,
) {
    let euro = Fixture::new(&format!("rotation-euro-{label}"), "EURUSD", 3);
    let gold = Fixture::new(&format!("rotation-gold-{label}"), "XAUUSD", 2);
    let (_, db, first, policy, limits) = campaign_owned_fixture(&euro, 2);
    let config = corpus_search_fixture(&gold);
    let mut file: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    file["start_index"] = 20.into();
    file["maximum_candidates"] = 1.into();
    file["queue_width"] = 1.into();
    fs::write(&config, serde_json::to_vec(&file).unwrap()).unwrap();
    let second = rust_strategy_factory::corpus_campaign::register_file(&config, &db, 2).unwrap();
    let group = euro.root.join("rotation.json");
    fs::write(&group,serde_json::to_vec(&serde_json::json!({"campaigns":[{"name":"euro","campaign_id":first},{"name":"gold","campaign_id":second}],"coordinator_lease_ms":60000})).unwrap()).unwrap();
    let id = rust_strategy_factory::campaign_rotation::register_file(&group, &db).unwrap();
    (euro, gold, db, id, first, second, policy, limits, group)
}
fn rotation_command(
    db: &std::path::Path,
    id: &str,
    policy: &std::path::Path,
    limits: &std::path::Path,
    maximum: &str,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "campaign-rotation-run",
            db.to_str().unwrap(),
            id,
            policy.to_str().unwrap(),
            limits.to_str().unwrap(),
            maximum,
        ])
        .output()
        .unwrap()
}
/// Wait-capable rotation driver for the final recovery: it waits out scheduler-idle
/// retry backoffs so the restored source can be exhausted within the visit budget.
fn rotation_loop_command(
    db: &std::path::Path,
    id: &str,
    policy: &std::path::Path,
    limits: &std::path::Path,
    maximum: &str,
    wait_ms: &str,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "campaign-rotation-loop",
            db.to_str().unwrap(),
            id,
            policy.to_str().unwrap(),
            limits.to_str().unwrap(),
            maximum,
            wait_ms,
        ])
        .output()
        .unwrap()
}
#[test]
fn campaign_rotation_isolates_unavailable_input_preserves_fair_cursor_then_recovers_with_exact_costs()
 {
    use rust_strategy_factory::{
        campaign_rotation, corpus_campaign, operator_config, owned_history, research,
        storage::Registry, tick_engine,
    };
    let (_euro, gold, db, id, first, second, policy, limits, _) = rotation_fixture("recovery");
    let gold_config = gold.root.join("search.json");
    let missing = gold.root.join("search.held");
    fs::rename(&gold_config, &missing).unwrap();
    for _ in 0..2 {
        let out = rotation_command(&db, &id, &policy, &limits, "1");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let mut registry = Registry::open(&db).unwrap();
    let visited = campaign_rotation::status(&registry, &id).unwrap();
    assert_eq!(visited["visits"], "2");
    assert_eq!(visited["members"][0]["visits"], "1");
    assert_eq!(visited["members"][1]["last_kind"], "source_error");
    assert_eq!(visited["members"][1]["unknown_outcome_visits"], "1");
    assert_eq!(visited["next_member_index"], "0");
    let out = rotation_command(&db, &id, &policy, &limits, "100");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let idle = campaign_rotation::status(&registry, &id).unwrap();
    assert_eq!(idle["state"], "paused");
    assert_eq!(idle["pause_origin"], "scheduler_idle");
    assert_eq!(
        corpus_campaign::status(&registry, &first).unwrap()["execution"]["state"],
        "exhausted"
    );
    assert_eq!(
        corpus_campaign::status(&registry, &second).unwrap()["execution"]["state"],
        "running"
    );
    campaign_rotation::control(&mut registry, &id, "paused").unwrap();
    assert_eq!(
        campaign_rotation::status(&registry, &id).unwrap()["pause_origin"],
        "operator"
    );
    fs::rename(&missing, &gold_config).unwrap();
    let held = rotation_command(&db, &id, &policy, &limits, "100");
    assert!(held.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&held.stdout).unwrap()["visits"],
        "0"
    );
    campaign_rotation::control(&mut registry, &id, "running").unwrap();
    let out = rotation_loop_command(&db, &id, &policy, &limits, "100", "60000");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let final_status = campaign_rotation::status(&registry, &id).unwrap();
    assert_eq!(final_status["state"], "exhausted");
    let parents = owned_history::list(&registry).unwrap();
    let mut known = 0usize;
    for member in final_status["members"].as_array().unwrap() {
        let children = member["known_children"]
            .as_str()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let observed = member["cpu_observed_children"]
            .as_str()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let unknown = member["cpu_unobserved_children"]
            .as_str()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert_eq!(children, observed + unknown);
        known += children;
        assert!(
            member["observed_native_cpu_ticks"]
                .as_str()
                .unwrap()
                .parse::<u128>()
                .is_ok()
        );
    }
    assert_eq!(known, parents.len());
    let loaded = operator_config::load_search(&gold_config).unwrap();
    for index in 20..22 {
        let strategy = research::candidate_at(index, 42).unwrap().1;
        let key = tick_engine::corpus_key(
            &loaded.base,
            &loaded.specification,
            &strategy,
            &loaded.costs,
            loaded.file.budget,
        )
        .unwrap();
        let stored = registry.result(&key.identity().unwrap()).unwrap().unwrap();
        assert_eq!(
            stored.result["tick_replay"],
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
    registry.integrity().unwrap();
    registry
        .backup(&gold.root.join("rotation-copy.sqlite"))
        .unwrap();
    let copied = Registry::open(&gold.root.join("rotation-copy.sqlite")).unwrap();
    assert_eq!(
        campaign_rotation::status(&copied, &id).unwrap(),
        final_status
    );
    drop(copied);
    let repeated = rotation_command(&db, &id, &policy, &limits, "100");
    assert!(repeated.status.success());
    assert_eq!(
        campaign_rotation::status(&registry, &id).unwrap(),
        final_status
    );
    assert!(campaign_rotation::control(&mut registry, &id, "running").is_err());
}
#[test]
fn campaign_rotation_pressure_and_outer_publication_failure_preserve_inner_evidence() {
    use rust_strategy_factory::{
        campaign_rotation, corpus_campaign, owned_history, storage::Registry,
    };
    let (_euro, gold, db, id, first, _second, policy, limits, group) = rotation_fixture("rollback");
    let mut file: serde_json::Value = serde_json::from_slice(&fs::read(&group).unwrap()).unwrap();
    file["resource_guard"] =
        serde_json::json!({"maximum_process_rss_bytes":1,"minimum_host_available_memory_bytes":0});
    fs::write(&group, serde_json::to_vec(&file).unwrap()).unwrap();
    let pressure = campaign_rotation::register_file(&group, &db).unwrap();
    let out = rotation_command(&db, &pressure, &policy, &limits, "100");
    assert!(out.status.success());
    let registry = Registry::open(&db).unwrap();
    assert_eq!(
        campaign_rotation::status(&registry, &pressure).unwrap()["attempt"],
        "0"
    );
    assert!(owned_history::list(&registry).unwrap().is_empty());
    let c = rusqlite::Connection::open(&db).unwrap();
    c.execute_batch("CREATE TRIGGER injected_rotation BEFORE UPDATE OF body ON corpus_campaign_rotations BEGIN SELECT RAISE(ABORT,'injected rotation publication failure');END;").unwrap();
    let failed = rotation_command(&db, &id, &policy, &limits, "1");
    assert!(!failed.status.success());
    assert!(
        String::from_utf8_lossy(&failed.stderr).contains("injected rotation publication failure")
    );
    let group_before = campaign_rotation::status(&registry, &id).unwrap();
    assert_eq!(group_before["visits"], "0");
    assert_eq!(group_before["members"][0]["known_children"], "0");
    assert_eq!(group_before["next_member_index"], "0");
    assert_eq!(group_before["lease_active"], false);
    let inner = corpus_campaign::status(&registry, &first).unwrap();
    assert_eq!(
        inner["execution"]["owned_visits"].as_array().unwrap().len(),
        1
    );
    assert_eq!(owned_history::list(&registry).unwrap().len(), 1);
    c.execute_batch("DROP TRIGGER injected_rotation;").unwrap();
    let out = rotation_command(&db, &id, &policy, &limits, "1");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let after = campaign_rotation::status(&registry, &id).unwrap();
    assert_eq!(after["visits"], "1");
    assert_eq!(after["next_member_index"], "1");
    registry.integrity().unwrap();
    registry
        .backup(&gold.root.join("partial-copy.sqlite"))
        .unwrap();
}

// Typed-admission synthetic fault metadata; this is not an actual historical busy event.
fn rotation_retry_fixture(
    f: &Fixture,
    backoff: i64,
) -> (PathBuf, PathBuf, PathBuf, String, String) {
    let (db, policy, limits, _fleet, source) = retryable_fleet_fixture(f, backoff);
    let campaign =
        rust_strategy_factory::corpus_campaign::register_file(&f.root.join("search.json"), &db, 1)
            .unwrap();
    let file = f.root.join("retry-rotation.json");
    fs::write(&file,serde_json::to_vec(&serde_json::json!({"campaigns":[{"name":"typed-retry-fixture","campaign_id":campaign}],"coordinator_lease_ms":60000})).unwrap()).unwrap();
    let id = rust_strategy_factory::campaign_rotation::register_file(&file, &db).unwrap();
    (db, policy, limits, id, source)
}
fn rotation_wait_command(
    db: &std::path::Path,
    id: &str,
    policy: &std::path::Path,
    limits: &std::path::Path,
    maximum: &str,
    wait: &str,
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "campaign-rotation-loop",
            db.to_str().unwrap(),
            id,
            policy.to_str().unwrap(),
            limits.to_str().unwrap(),
            maximum,
            wait,
        ])
        .output()
        .unwrap()
}
#[test]
fn rotation_wait_budget_restarts_preserve_frozen_deadline_without_process_or_journal_spam() {
    use rust_strategy_factory::{
        campaign_rotation, corpus_search, owned_history, storage::Registry,
    };
    let f = Fixture::new("rotation-wait-budget", "EURUSD", 7);
    let (db, policy, limits, id, source) = rotation_retry_fixture(&f, 60000);
    let first = rotation_wait_command(&db, &id, &policy, &limits, "10", "0");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["stop_reason"], "backoff");
    let registry = Registry::open(&db).unwrap();
    let before = corpus_search::status(&registry, &source).unwrap();
    let epoch = campaign_rotation::status(&registry, &id).unwrap()["epoch"].clone();
    let second = rotation_wait_command(&db, &id, &policy, &limits, "10", "30");
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(second["stop_reason"], "wait_budget");
    assert_eq!(second["retry_at_ms"], first["retry_at_ms"]);
    assert!(
        second["waited_ms"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            >= 30
    );
    assert_eq!(corpus_search::status(&registry, &source).unwrap(), before);
    assert_eq!(
        campaign_rotation::status(&registry, &id).unwrap()["epoch"],
        epoch
    );
    assert!(owned_history::list(&registry).unwrap().is_empty());
    registry.integrity().unwrap();
}
#[test]
fn rotation_wait_wakes_once_then_holds_actual_permanent_execution_failure() {
    use rust_strategy_factory::{
        campaign_rotation, corpus_search, owned_history, storage::Registry,
    };
    let f = Fixture::new("rotation-wait-ready", "EURUSD", 7);
    let (db, policy, limits, id, source) = rotation_retry_fixture(&f, 800);
    let out = rotation_wait_command(&db, &id, &policy, &limits, "10", "3000");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["stop_reason"], "idle", "{report}");
    let registry = Registry::open(&db).unwrap();
    let held = corpus_search::status(&registry, &source).unwrap();
    assert_eq!(held["requeue_records"], "2");
    assert!(
        held["recent_slots"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["retry_candidate"] == false)
    );
    let parents = owned_history::list(&registry).unwrap();
    assert_eq!(parents.len(), 1);
    assert_eq!(
        campaign_rotation::status(&registry, &id).unwrap()["pause_origin"],
        "scheduler_idle"
    );
    let again = rotation_wait_command(&db, &id, &policy, &limits, "10", "3000");
    assert!(again.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&again.stdout).unwrap()["stop_reason"],
        "idle"
    );
    assert_eq!(owned_history::list(&registry).unwrap(), parents);
    assert_eq!(corpus_search::status(&registry, &source).unwrap(), held);
    registry.integrity().unwrap();
}
#[test]
fn rotation_wait_operator_and_sigterm_intervention_leave_retries_held() {
    use rust_strategy_factory::{
        campaign_rotation, corpus_search, owned_history, storage::Registry,
    };
    for signal in [false, true] {
        let f = Fixture::new(
            if signal {
                "rotation-wait-signal"
            } else {
                "rotation-wait-operator"
            },
            "EURUSD",
            7,
        );
        let (db, policy, limits, id, source) = rotation_retry_fixture(&f, 60000);
        let mut child = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args([
                "campaign-rotation-loop",
                db.to_str().unwrap(),
                &id,
                policy.to_str().unwrap(),
                limits.to_str().unwrap(),
                "10",
                "5000",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut registry = Registry::open(&db).unwrap();
        let start = std::time::Instant::now();
        loop {
            let current = campaign_rotation::status(&registry, &id).unwrap();
            if current["pause_origin"] == "scheduler_idle" {
                break;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
            assert!(child.try_wait().unwrap().is_none());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if signal {
            let status = Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .status()
                .unwrap();
            assert!(status.success());
        } else {
            campaign_rotation::control(&mut registry, &id, "paused").unwrap();
        }
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(
            report["stop_reason"],
            if signal { "signal" } else { "held" }
        );
        assert_eq!(
            corpus_search::status(&registry, &source).unwrap()["requeue_records"],
            "0"
        );
        assert!(owned_history::list(&registry).unwrap().is_empty());
        registry.integrity().unwrap();
    }
}

#[test]
fn prepared_corpus_matches_every_instrument_frame_and_aggregate_budget() {
    use rust_strategy_factory::contracts::SYMBOLS;
    for symbol in SYMBOLS {
        let f = Fixture::new("prepared-frames", symbol, 3);
        let inspected = corpus::inspect(&f.root, &f.spec).unwrap();
        for primary in Timeframe::ALL {
            let prepared = corpus::prepare(&f.root, &f.spec, primary, 1000).unwrap();
            assert_eq!(
                serde_json::to_value(prepared.report()).unwrap(),
                serde_json::to_value(&inspected).unwrap()
            );
            let mut total = 0;
            for tf in Timeframe::ALL {
                if tf.millis() >= primary.millis() {
                    let expected = candles::build(&f.data, tf).unwrap();
                    assert_eq!(prepared.candles(tf).unwrap(), expected);
                    total += expected.len();
                } else {
                    assert!(prepared.candles(tf).is_err());
                }
            }
            assert_eq!(prepared.retained_candles(), total);
            assert_eq!(
                corpus::prepare(&f.root, &f.spec, primary, total.max(1))
                    .unwrap()
                    .retained_candles(),
                total
            );
            if total > 0 {
                assert!(corpus::prepare(&f.root, &f.spec, primary, total - 1).is_err());
            }
        }
        for cap in [0, corpus::MAX_RETAINED_CANDLES + 1, usize::MAX] {
            assert!(corpus::prepare(&f.root, &f.spec, Timeframe::M1, cap).is_err());
        }
    }
}
#[test]
fn prepared_corpus_causal_signals_match_materialized_and_block_layouts() {
    use rust_strategy_factory::strategies::{Features, Rule, Strategy};
    let f = Fixture::new("prepared-signals", "EURUSD", 2);
    let prepared = corpus::prepare(&f.root, &f.spec, Timeframe::M1, 1000).unwrap();
    let path = f.root.join("whole.rsfb");
    rust_strategy_factory::blocked_native::write(&path, &f.data, 3).unwrap();
    let blocked_spec = corpus::describe(&[path], 100).unwrap();
    let blocked = corpus::prepare(&f.root, &blocked_spec, Timeframe::M1, 1000).unwrap();
    assert_eq!(
        prepared.report().ordered_events_hash,
        blocked.report().ordered_events_hash
    );
    for tf in Timeframe::ALL {
        assert_eq!(prepared.candles(tf).unwrap(), blocked.candles(tf).unwrap());
    }
    let primary = candles::build(&f.data, Timeframe::M1).unwrap();
    let mut strategy:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    for rule in [
        strategy.rule.clone(),
        Rule::HigherTimeframe {
            timeframe: Timeframe::H1,
            rule: Box::new(Rule::UtcSession {
                start_hour: 0,
                end_hour: 23,
                weekdays: 127,
            }),
        },
        Rule::All(vec![
            Rule::AboveMean { window: 2 },
            Rule::HigherTimeframe {
                timeframe: Timeframe::H1,
                rule: Box::new(Rule::AboveMean { window: 2 }),
            },
        ]),
    ] {
        strategy.rule = rule;
        let expected = Features::with_context(&primary, 10000, &f.data, Timeframe::M1)
            .unwrap()
            .signal(&strategy.rule)
            .unwrap();
        let actual = prepared.signals(&strategy, 10000).unwrap();
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(blocked.signals(&strategy, 10000).unwrap()).unwrap()
        );
    }
    assert!(prepared.signals(&strategy, 1).is_err());
    assert!(
        corpus::prepare(&f.root, &f.spec, Timeframe::H1, 1000)
            .unwrap()
            .signals(&strategy, 10000)
            .is_err()
    );
}
#[test]
fn prepared_snapshot_is_not_current_byte_eligibility_and_mutated_artifacts_refuse_replay() {
    use rust_strategy_factory::{
        strategies::Strategy,
        tick_engine::{self, CorpusBudget, Costs},
    };
    let f = Fixture::new("prepared-mutation", "EURUSD", 3);
    let prepared = corpus::prepare(&f.root, &f.spec, Timeframe::M1, 1000).unwrap();
    let original_report = serde_json::to_value(prepared.report()).unwrap();
    let path = f.root.join(&f.spec.chunks[0].path);
    let bytes = fs::read(&path).unwrap();
    let mut corrupt = bytes.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    fs::write(&path, corrupt).unwrap();
    assert_eq!(
        serde_json::to_value(prepared.report()).unwrap(),
        original_report
    );
    assert!(corpus::prepare(&f.root, &f.spec, Timeframe::M1, 1000).is_err());
    let strategy:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    assert!(
        tick_engine::replay_corpus(
            &f.root,
            &f.spec,
            &strategy,
            &Costs::default(),
            CorpusBudget {
                max_candles: 1000,
                feature_cells: 10000,
                max_receipts: 1000
            }
        )
        .is_err()
    );
    fs::write(&path, bytes).unwrap();
    assert_eq!(
        serde_json::to_value(
            corpus::prepare(&f.root, &f.spec, Timeframe::M1, 1000)
                .unwrap()
                .report()
        )
        .unwrap(),
        original_report
    );
}

#[test]
fn shared_worker_preparation_actual_cli_matches_full_order_receipts_and_cached_restart() {
    use rust_strategy_factory::{
        contracts::{Conversion, Side},
        indicators::Mask,
        strategies::{EntryPlan, Features, Rule, Strategy},
        tick_engine::{self, Costs},
    };
    let f = Fixture::new("worker-preparation-cli", "EURUSD", 37);
    let data = data::synthetic_fixture("EURUSD").unwrap();
    let artifact = f.root.join("complete.rsf");
    native::write(&artifact, &data).unwrap();
    let spec = corpus::describe(&[artifact], 10000).unwrap();
    fs::write(
        f.root.join("shared.json"),
        serde_json::to_vec(&spec).unwrap(),
    )
    .unwrap();
    let mut strategies: Vec<_> = [
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
    .map(|entry| Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks: 100,
        rule: Rule::AboveMean { window: 2 },
        entry,
        stop_policy: None,
    })
    .collect();
    let mut higher = strategies[0].clone();
    higher.timeframe = Timeframe::M5;
    strategies.push(higher);
    let config = f.root.join("shared-config.json");
    fs::write(&config, serde_json::to_vec(&serde_json::json!({"specification":"shared.json","strategies":strategies,"budget":{"max_candles":10000,"feature_cells":100000,"max_receipts":10000},"limits":{"workers":1,"max_parallel_candles":10000,"max_parallel_feature_cells":100000,"max_parallel_receipts":10000,"max_new_chunks":64,"lease_ms":60000}})).unwrap()).unwrap();
    let database = f.root.join("shared.sqlite");
    let call = || {
        let output = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args([
                "corpus-batch",
                config.to_str().unwrap(),
                database.to_str().unwrap(),
                "1",
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
    let first = call();
    assert_eq!(first["stop_reason"], "complete");
    assert_eq!(first["last"]["preparation"]["builds"], 2);
    assert_eq!(first["last"]["preparation"]["reuses"], 2);
    assert_eq!(first["last"]["preparation"]["current_byte_checks"], 2);
    for (index, strategy) in strategies.iter().enumerate() {
        let candles = candles::build(&data, strategy.timeframe).unwrap();
        let mask: Mask = Features::new(&candles, 100000)
            .unwrap()
            .signal(&strategy.rule)
            .unwrap();
        let reference = tick_engine::replay(
            &data,
            &candles,
            &mask,
            strategy,
            &Costs::default(),
            &vec![Conversion::usd(); data.ticks.len()],
        )
        .unwrap();
        assert!(!reference.trades.is_empty());
        let completed = &first["last"]["jobs"][index]["Ok"]["Completed"];
        assert_eq!(completed["cached"], false);
        assert_eq!(
            completed["result"]["result"]["tick_replay"],
            serde_json::to_value(reference).unwrap()
        );
    }
    let cached = call();
    assert_eq!(cached["stop_reason"], "complete");
    assert_eq!(cached["last"]["preparation"]["builds"], 0);
    assert_eq!(cached["last"]["preparation"]["reuses"], 0);
    for (index, original) in first["last"]["jobs"].as_array().unwrap().iter().enumerate() {
        let result = &cached["last"]["jobs"][index]["Ok"]["Completed"];
        assert_eq!(result["cached"], true);
        assert_eq!(result["result"], original["Ok"]["Completed"]["result"]);
    }
    rust_strategy_factory::storage::Registry::open(&database)
        .unwrap()
        .integrity()
        .unwrap();
}

#[test]
fn preparation_sessions_preserve_independent_ownership_holds_caps_and_current_sources() {
    use rust_strategy_factory::{
        storage::Registry,
        strategies::Strategy,
        tick_engine::{
            CorpusBatchLimits, CorpusBatchSession, CorpusBudget, CorpusProgress, CorpusRun, Costs,
        },
    };
    let f = Fixture::new("session-lifecycle", "EURUSD", 7);
    let costs = Costs::default();
    let strategies: Vec<Strategy> = [90,100].into_iter().map(|stop| serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":stop,"rule":{"AboveMean":{"window":2}}})).unwrap()).collect();
    let budget = CorpusBudget {
        max_candles: 100,
        feature_cells: 10000,
        max_receipts: 1000,
    };
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
    let limits = CorpusBatchLimits {
        workers: 1,
        max_parallel_candles: 100,
        max_parallel_feature_cells: 10000,
        max_parallel_receipts: 1000,
        max_new_chunks: 1,
        lease_ms: 60000,
    };
    let mut a = CorpusBatchSession::default();
    let mut b = CorpusBatchSession::default();
    let da = f.root.join("a.sqlite");
    let db = f.root.join("b.sqlite");
    let first = a.round(&da, &runs, limits).unwrap();
    assert_eq!(first.preparation.builds, 1);
    assert_eq!(first.preparation.reuses, 1);
    let other = b.round(&db, &runs, limits).unwrap();
    assert_eq!(other.preparation.builds, 1);
    assert_eq!(other.preparation.reuses, 1);
    let retained = b.retained_candles();
    assert!(retained > 0 && retained <= 100);
    let warm = a.round(&da, &runs, limits).unwrap();
    assert_eq!(warm.preparation.builds, 0);
    assert_eq!(warm.preparation.reuses, 2);
    assert_eq!(warm.preparation.current_byte_checks, 2);
    a.release();
    assert_eq!(a.retained_candles(), 0);
    assert_eq!(b.retained_candles(), retained);
    let ids: Vec<_> = other
        .jobs
        .iter()
        .map(|r| match r.as_ref().unwrap() {
            CorpusProgress::Pending { experiment_id, .. } => experiment_id.clone(),
            x => panic!("{x:?}"),
        })
        .collect();
    let mut registry = Registry::open(&db).unwrap();
    for id in &ids {
        registry.set_corpus_control(id, "paused", 1).unwrap();
    }
    let held = b.round(&db, &runs, limits).unwrap();
    assert!(
        held.jobs
            .iter()
            .all(|j| matches!(j, Ok(CorpusProgress::Held { .. })))
    );
    assert_eq!(held.preparation.builds, 0);
    assert_eq!(held.preparation.reuses, 0);
    assert_eq!(held.preparation.current_byte_checks, 2);
    for id in &ids {
        assert_eq!(registry.replay_checkpoint(id).unwrap().unwrap().0.cursor, 1);
        registry.set_corpus_control(id, "running", 2).unwrap();
    }
    let mut changed = limits;
    changed.max_new_chunks = 2;
    let rebuilt = b.round(&db, &runs, changed).unwrap();
    assert_eq!(rebuilt.preparation.builds, 1);
    assert_eq!(rebuilt.preparation.reuses, 1);
    for id in &ids {
        assert_eq!(registry.replay_checkpoint(id).unwrap().unwrap().0.cursor, 3);
    }
    let before = registry.replay_checkpoint(&ids[0]).unwrap().unwrap().1;
    let mut invalid = changed;
    invalid.max_parallel_candles = 99;
    assert!(b.round(&db, &runs, invalid).is_err());
    assert_eq!(b.retained_candles(), 0);
    assert_eq!(
        registry.replay_checkpoint(&ids[0]).unwrap().unwrap().1,
        before
    );
    b.round(&db, &runs, limits).unwrap();
    let before = registry.replay_checkpoint(&ids[0]).unwrap().unwrap().1;
    let attempts = registry.counts().unwrap().attempts;
    let path = &f.spec.chunks[0].path;
    let bytes = fs::read(path).unwrap();
    let mut bad = bytes.clone();
    let index = bad.len() / 2;
    bad[index] ^= 1;
    fs::write(path, bad).unwrap();
    assert!(b.round(&db, &runs, limits).is_err());
    assert_eq!(b.retained_candles(), 0);
    assert_eq!(
        registry.replay_checkpoint(&ids[0]).unwrap().unwrap().1,
        before
    );
    assert_eq!(registry.counts().unwrap().attempts, attempts);
    fs::write(path, bytes).unwrap();
    let restored = b.round(&db, &runs, limits).unwrap();
    assert_eq!(restored.preparation.builds, 1);
    assert!(restored.jobs.iter().all(Result::is_ok));
    assert!(b.retained_candles() <= 100);
    let mut bad_budget = budget;
    bad_budget.feature_cells = 1;
    let bad_runs = [CorpusRun {
        base: &f.root,
        spec: &f.spec,
        strategy: &strategies[0],
        costs: &costs,
        budget: bad_budget,
    }];
    let failed = b.round(&db, &bad_runs, limits).unwrap();
    assert!(failed.jobs[0].is_err());
    assert_eq!(b.retained_candles(), 0);
    assert_eq!(Registry::open(&da).unwrap().counts().unwrap().running, 0);
    registry.integrity().unwrap();
}

#[test]
fn warm_preparation_session_still_recomputes_and_refuses_self_hashed_account_forgery() {
    use rust_strategy_factory::{
        storage::{Claim, Registry},
        strategies::Strategy,
        tick_engine::{
            self, CorpusBatchLimits, CorpusBatchSession, CorpusBudget, CorpusRun, Costs,
        },
    };
    let f = Fixture::new("session-semantic-forgery", "EURUSD", 7);
    let strategy:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    let costs = Costs::default();
    let budget = CorpusBudget {
        max_candles: 100,
        feature_cells: 10000,
        max_receipts: 1000,
    };
    let runs = [CorpusRun {
        base: &f.root,
        spec: &f.spec,
        strategy: &strategy,
        costs: &costs,
        budget,
    }];
    let limits = CorpusBatchLimits {
        workers: 1,
        max_parallel_candles: 100,
        max_parallel_feature_cells: 10000,
        max_parallel_receipts: 1000,
        max_new_chunks: 1,
        lease_ms: 60000,
    };
    let db = f.root.join("forged.sqlite");
    let mut session = CorpusBatchSession::default();
    session.round(&db, &runs, limits).unwrap();
    assert!(session.retained_candles() > 0);
    let id = tick_engine::corpus_key(&f.root, &f.spec, &strategy, &costs, budget)
        .unwrap()
        .identity()
        .unwrap();
    let mut registry = Registry::open(&db).unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    let Claim::Acquired(lease) = registry.claim(&id, now, 60000).unwrap() else {
        panic!()
    };
    registry
        .save_replay_checkpoint(
            &lease,
            now,
            1,
            2,
            serde_json::json!({"account":{"balance":999999999}}),
        )
        .unwrap();
    registry.yield_lease(&lease, now).unwrap();
    let forged = registry.replay_checkpoint(&id).unwrap().unwrap().1;
    let report = session.round(&db, &runs, limits).unwrap();
    assert!(
        report.jobs[0]
            .as_ref()
            .unwrap_err()
            .contains("account state differs")
    );
    assert_eq!(report.preparation.builds, 0);
    assert_eq!(report.preparation.reuses, 1);
    assert_eq!(report.preparation.current_byte_checks, 1);
    assert_eq!(session.retained_candles(), 0);
    assert_eq!(registry.replay_checkpoint(&id).unwrap().unwrap().1, forged);
    assert!(registry.result(&id).unwrap().is_none());
    registry.integrity().unwrap();
}

#[test]
fn multi_round_cli_preparation_session_reuses_after_restart_and_preserves_full_receipts() {
    use rust_strategy_factory::{
        contracts::Conversion,
        strategies::{EntryPlan, Features, Strategy},
        tick_engine::{self, Costs},
    };
    let f = Fixture::new("session-cli", "EURUSD", 37);
    let data = data::synthetic_fixture("EURUSD").unwrap();
    let mut paths = Vec::new();
    for (i, ticks) in data.ticks.chunks(700).enumerate() {
        let part = data::dataset(
            data.profile.clone(),
            ticks.to_vec(),
            &data.manifest.source,
            data.manifest.archive_hash.clone(),
            true,
        )
        .unwrap();
        let path = f.root.join(format!("part-{i}.rsf"));
        native::write(&path, &part).unwrap();
        paths.push(path);
    }
    let spec = corpus::describe(&paths, 10000).unwrap();
    fs::write(
        f.root.join("session.json"),
        serde_json::to_vec(&spec).unwrap(),
    )
    .unwrap();
    let mut first:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    let mut second = first.clone();
    second.entry = Some(EntryPlan::Limit {
        offset_ticks: 2,
        ttl_ms: 120000,
        cancel_on_signal_false: true,
    });
    first.entry = Some(EntryPlan::StopMarket {
        offset_ticks: 2,
        ttl_ms: 120000,
        cancel_on_signal_false: true,
    });
    let strategies = [first, second];
    let config = f.root.join("session-config.json");
    let db = f.root.join("session.sqlite");
    fs::write(&config,serde_json::to_vec(&serde_json::json!({"specification":"session.json","strategies":strategies,"budget":{"max_candles":10000,"feature_cells":100000,"max_receipts":10000},"limits":{"workers":1,"max_parallel_candles":10000,"max_parallel_feature_cells":100000,"max_parallel_receipts":10000,"max_new_chunks":1,"lease_ms":60000}})).unwrap()).unwrap();
    let call = |rounds: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args([
                "corpus-batch",
                config.to_str().unwrap(),
                db.to_str().unwrap(),
                rounds,
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
    let initial = call("1");
    assert_eq!(initial["stop_reason"], "round-limit");
    assert_eq!(initial["last"]["preparation"]["builds"], 1);
    assert_eq!(initial["last"]["preparation"]["reuses"], 1);
    let completed = call("100");
    assert_eq!(completed["stop_reason"], "complete");
    assert_eq!(completed["rounds"], 2);
    assert_eq!(completed["last"]["preparation"]["builds"], 0);
    assert_eq!(completed["last"]["preparation"]["reuses"], 2);
    assert_eq!(completed["last"]["preparation"]["current_byte_checks"], 2);
    for (index, strategy) in strategies.iter().enumerate() {
        let candles = candles::build(&data, strategy.timeframe).unwrap();
        let mask = Features::new(&candles, 100000)
            .unwrap()
            .signal(&strategy.rule)
            .unwrap();
        let reference = tick_engine::replay(
            &data,
            &candles,
            &mask,
            strategy,
            &Costs::default(),
            &vec![Conversion::usd(); data.ticks.len()],
        )
        .unwrap();
        assert!(!reference.trades.is_empty());
        assert_eq!(
            completed["last"]["jobs"][index]["Ok"]["Completed"]["result"]["result"]["tick_replay"],
            serde_json::to_value(reference).unwrap()
        );
    }
    let cached = call("100");
    assert_eq!(cached["rounds"], 1);
    assert_eq!(cached["last"]["preparation"]["builds"], 0);
    assert_eq!(cached["last"]["preparation"]["reuses"], 0);
    rust_strategy_factory::storage::Registry::open(&db)
        .unwrap()
        .integrity()
        .unwrap();
}

#[test]
fn mixed_budget_sessions_drop_prior_slots_before_parallel_resume_without_new_refusal() {
    use rust_strategy_factory::{
        contracts::{Conversion, Side},
        strategies::{Features, Rule, Strategy},
        tick_engine::{
            self, CorpusBatchLimits, CorpusBatchSession, CorpusBudget, CorpusProgress, CorpusRun,
            Costs,
        },
    };
    let f = Fixture::new("mixed-reservation-resume", "EURUSD", 37);
    let data = data::synthetic_fixture("EURUSD").unwrap();
    let mut paths = Vec::new();
    for (index, ticks) in data.ticks.chunks(500).enumerate() {
        let part = data::dataset(
            data.profile.clone(),
            ticks.to_vec(),
            &data.manifest.source,
            data.manifest.archive_hash.clone(),
            true,
        )
        .unwrap();
        let path = f.root.join(format!("mixed-{index}.rsf"));
        native::write(&path, &part).unwrap();
        paths.push(path);
    }
    let spec = corpus::describe(&paths, 10000).unwrap();
    let strategies = [Timeframe::M1, Timeframe::H1].map(|timeframe| Strategy {
        timeframe,
        side: Side::Long,
        stop_ticks: 100,
        rule: Rule::AboveMean { window: 2 },
        entry: None,
        stop_policy: None,
    });
    let budgets = [500, 20].map(|max_candles| CorpusBudget {
        max_candles,
        feature_cells: 100000,
        max_receipts: 10000,
    });
    let prepared = corpus::prepare(&f.root, &spec, Timeframe::M1, 500).unwrap();
    assert!(
        prepared.retained_candles() * 2 > 520,
        "duplicating this actual large context would exceed the admitted top-two job budget"
    );
    drop(prepared);
    let costs = Costs::default();
    let runs: Vec<_> = strategies
        .iter()
        .zip(budgets)
        .map(|(strategy, budget)| CorpusRun {
            base: &f.root,
            spec: &spec,
            strategy,
            costs: &costs,
            budget,
        })
        .collect();
    let limits = CorpusBatchLimits {
        workers: 2,
        max_parallel_candles: 520,
        max_parallel_feature_cells: 200000,
        max_parallel_receipts: 20000,
        max_new_chunks: 1,
        lease_ms: 60000,
    };
    let mut session = CorpusBatchSession::default();
    let db = f.root.join("mixed.sqlite");
    for round in 0..spec.chunks.len() {
        let report = session.round(&db, &runs, limits).unwrap();
        assert_eq!(report.worker_failures, 0);
        assert_eq!(report.preparation.builds, 2);
        assert_eq!(report.preparation.reuses, 0);
        assert_eq!(report.preparation.current_byte_checks, 0);
        assert!(session.retained_candles() <= 520);
        for (index, outcome) in report.jobs.into_iter().enumerate() {
            match outcome.unwrap() {
                CorpusProgress::Pending { cursor, .. } => assert_eq!(cursor, round as i64 + 1),
                CorpusProgress::Completed {
                    result,
                    cached: false,
                    ..
                } => {
                    assert_eq!(round + 1, spec.chunks.len());
                    let candles = candles::build(&data, strategies[index].timeframe).unwrap();
                    let mask = Features::new(&candles, 100000)
                        .unwrap()
                        .signal(&strategies[index].rule)
                        .unwrap();
                    let reference = tick_engine::replay(
                        &data,
                        &candles,
                        &mask,
                        &strategies[index],
                        &costs,
                        &vec![Conversion::usd(); data.ticks.len()],
                    )
                    .unwrap();
                    if index == 0 {
                        assert!(!reference.trades.is_empty());
                    }
                    assert_eq!(
                        result.result["tick_replay"],
                        serde_json::to_value(reference).unwrap()
                    );
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    }
    session.release();
    assert_eq!(session.retained_candles(), 0);
    rust_strategy_factory::storage::Registry::open(&db)
        .unwrap()
        .integrity()
        .unwrap();
}

#[test]
fn campaign_ledgers_exceed_global128_without_cross_campaign_quota_or_extra_children_at_own_cap() {
    use rust_strategy_factory::{
        corpus_campaign, corpus_search, operator_config, owned_history, research,
        storage::Registry, tick_engine,
    };
    let f = Fixture::new("campaign-ledger-lifetime", "EURUSD", 37);
    let profile = Profile::synthetic("EURUSD").unwrap();
    let mut paths = Vec::new();
    for i in 0..129u64 {
        let phase = i % 20;
        let wave = if phase < 10 { phase } else { 20 - phase };
        let bid = 10000 + wave as i64 * 300;
        let part = data::dataset(
            profile.clone(),
            vec![Tick {
                timestamp_ms: 1735689600000 + i as i64 * 60000,
                sequence: i,
                bid,
                ask: bid + 10,
            }],
            "SYNTHETIC campaign ledger lifetime fixture",
            "generated-no-archive".into(),
            true,
        )
        .unwrap();
        let path = f.root.join(format!("lifetime-{i}.rsf"));
        native::write(&path, &part).unwrap();
        paths.push(path);
    }
    let spec = corpus::describe(&paths, 100).unwrap();
    fs::write(
        f.root.join("lifetime.json"),
        serde_json::to_vec(&spec).unwrap(),
    )
    .unwrap();
    let (db, policy, limits) = (
        f.root.join("lifetime.sqlite"),
        f.root.join("policy.json"),
        f.root.join("limits.json"),
    );
    fs::write(&policy, br#"{"backoff_ms":1,"maximum_slots":1}"#).unwrap();
    fs::write(
        &limits,
        br#"{"maximum_wall_ms":30000,"shutdown_grace_ms":1000,"maximum_output_bytes":1048576}"#,
    )
    .unwrap();
    let mut configs = Vec::new();
    let mut campaigns = Vec::new();
    for (index, chunks) in [1, 64].into_iter().enumerate() {
        let config = f.root.join(format!("lifetime-config-{index}.json"));
        fs::write(&config,serde_json::to_vec(&serde_json::json!({"specification":"lifetime.json","seed":42,"start_index":21,"maximum_candidates":1,"queue_width":1,"budget":{"max_candles":1000+index,"feature_cells":100000,"max_receipts":10000},"limits":{"workers":1,"max_parallel_candles":1001,"max_parallel_feature_cells":100000,"max_parallel_receipts":10000,"max_new_chunks":chunks,"lease_ms":60000}})).unwrap()).unwrap();
        campaigns.push(corpus_campaign::register_file(&config, &db, 1).unwrap());
        configs.push(config);
    }
    let invoke = |id: &str, n: &str| {
        let output = campaign_owned_command(&db, id, &policy, &limits, n);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    fn drive(
        database: &std::path::Path,
        id: &str,
        policy: &std::path::Path,
        limits: &std::path::Path,
        target: Option<usize>,
    ) {
        for _ in 0..100 {
            let registry = Registry::open(database).unwrap();
            let status = corpus_campaign::status(&registry, id).unwrap();
            let count = status["execution"]["owned_visits"]
                .as_array()
                .unwrap()
                .len();
            if target.is_some_and(|n| count == n)
                || (target.is_none() && status["execution"]["state"] == "exhausted")
            {
                return;
            }
            let remaining = target.map_or(100, |n| n.checked_sub(count).unwrap());
            let output =
                campaign_owned_command(database, id, policy, limits, &remaining.to_string());
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let current = corpus_campaign::status(&registry, id).unwrap();
            if target.is_some_and(|n| {
                current["execution"]["owned_visits"]
                    .as_array()
                    .unwrap()
                    .len()
                    == n
            }) || (target.is_none() && current["execution"]["state"] == "exhausted")
            {
                return;
            }
            let source = corpus_search::status(
                &registry,
                current["regions"][0]["search_id"].as_str().unwrap(),
            )
            .unwrap();
            let running = source["state"] == "running" && source["lease_active"] == false;
            let retryable = source["state"] == "paused"
                && source["control"]["pause_origin"] == "worker_error"
                && source["recent_slots"][0]["error_kind"] == "registry_admission_busy"
                && source["recent_slots"][0]["retry_candidate"] == true;
            assert!(
                running || retryable,
                "unexpected ledger fixture hold: {source}; {}",
                String::from_utf8_lossy(&output.stdout)
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("bounded ledger fixture did not reach its declared target");
    }
    drive(&db, &campaigns[0], &policy, &limits, Some(128));
    drive(&db, &campaigns[1], &policy, &limits, None);
    let mut registry = Registry::open(&db).unwrap();
    let a = corpus_campaign::status(&registry, &campaigns[0]).unwrap();
    let b = corpus_campaign::status(&registry, &campaigns[1]).unwrap();
    assert_eq!(
        a["execution"]["owned_visits"].as_array().unwrap().len(),
        128
    );
    let second_links = b["execution"]["owned_visits"].as_array().unwrap().len();
    assert!(second_links >= 3);
    let source = a["regions"][0]["search_id"].as_str().unwrap();
    fn parent_count(registry: &Registry) -> usize {
        let mut count = 0;
        let mut after = None;
        loop {
            let page = owned_history::page(registry, after.as_deref(), 128).unwrap();
            count += page.rows.len();
            let Some(next) = page.next_cursor else { break };
            after = Some(next);
        }
        count
    }
    let parents = parent_count(&registry);
    assert_eq!(parents, 128 + second_links);
    assert!(parents > 128);
    let attempts = registry.counts().unwrap().attempts;
    let blocked = campaign_owned_command(&db, &campaigns[0], &policy, &limits, "1");
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("before child admission"));
    assert_eq!(registry.counts().unwrap().attempts, attempts);
    assert_eq!(parent_count(&registry), parents);
    assert_eq!(
        corpus_campaign::status(&registry, &campaigns[0]).unwrap()["execution"]["owned_visits"],
        a["execution"]["owned_visits"]
    );
    corpus_search::control(&mut registry, source, "paused").unwrap();
    let held = invoke(&campaigns[0], "1");
    assert_eq!(held["children_started"], "0");
    corpus_search::control(&mut registry, source, "running").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "corpus-search",
            configs[0].to_str().unwrap(),
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
    let finalized = invoke(&campaigns[0], "8");
    assert_eq!(finalized["children_started"], "0");
    assert_eq!(finalized["stop_reason"], "exhausted");
    assert_eq!(parent_count(&registry), parents);
    let statuses: Vec<_> = campaigns
        .iter()
        .map(|id| corpus_campaign::status(&registry, id).unwrap())
        .collect();
    for status in &statuses {
        assert_eq!(
            status["execution"]["completed_regions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        for visit in status["execution"]["owned_visits"].as_array().unwrap() {
            let parent =
                owned_history::status(&registry, visit["invocation_id"].as_str().unwrap()).unwrap();
            assert_eq!(parent["summary"]["reaped"], true);
            assert_eq!(parent["observation_hash"], visit["parent_observation_hash"]);
            assert_eq!(parent["summary"]["report_hash"], visit["report_hash"]);
        }
    }
    for config in &configs {
        let loaded = operator_config::load_search(config).unwrap();
        let strategy = research::candidate_at(21, 42).unwrap().1;
        let key = tick_engine::corpus_key(
            &loaded.base,
            &loaded.specification,
            &strategy,
            &loaded.costs,
            loaded.file.budget,
        )
        .unwrap();
        let stored = registry.result(&key.identity().unwrap()).unwrap().unwrap();
        let reference = tick_engine::replay_corpus(
            &loaded.base,
            &loaded.specification,
            &strategy,
            &loaded.costs,
            loaded.file.budget,
        )
        .unwrap();
        assert!(!reference.trades.is_empty());
        assert_eq!(
            stored.result["tick_replay"],
            serde_json::to_value(reference).unwrap()
        );
    }
    registry.integrity().unwrap();
    let backup = f.root.join("lifetime-backup.sqlite");
    registry.backup(&backup).unwrap();
    let restored = Registry::open(&backup).unwrap();
    for (index, id) in campaigns.iter().enumerate() {
        assert_eq!(
            corpus_campaign::status(&restored, id).unwrap(),
            statuses[index]
        );
    }
    let c = rusqlite::Connection::open(&db).unwrap();
    c.execute(
        "UPDATE corpus_campaign_owned_heads SET hash=?2 WHERE campaign_id=?1",
        rusqlite::params![campaigns[1], "0".repeat(64)],
    )
    .unwrap();
    assert!(corpus_campaign::status(&registry, &campaigns[1]).is_err());
    assert_eq!(
        corpus_campaign::status(&registry, &campaigns[0]).unwrap(),
        statuses[0]
    );
    assert!(registry.integrity().is_err());
}

#[test]
fn declared_corpus_candle_ceiling_is_consistent_across_preparation_signals_and_registry_keys() {
    use rust_strategy_factory::{
        strategies::Strategy,
        tick_engine::{self, CorpusBudget, Costs},
    };
    let f = Fixture::new("declared-candle-cap", "EURUSD", 7);
    let strategy:Strategy=serde_json::from_value(serde_json::json!({"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}})).unwrap();
    let cap = corpus::MAX_RETAINED_CANDLES;
    assert_eq!(cap, 2_000_000);
    for budget in [0, cap + 1, usize::MAX] {
        assert!(corpus::prepare(&f.root, &f.spec, Timeframe::M1, budget).is_err());
        assert!(corpus::candle_context(&f.root, &f.spec, Timeframe::M1, budget).is_err());
        assert!(corpus::signals(&f.root, &f.spec, &strategy, budget, 10000).is_err());
        assert!(
            tick_engine::corpus_key(
                &f.root,
                &f.spec,
                &strategy,
                &Costs::default(),
                CorpusBudget {
                    max_candles: budget,
                    feature_cells: 10000,
                    max_receipts: 1000
                }
            )
            .is_err()
        );
    }
    let prepared = corpus::prepare(&f.root, &f.spec, Timeframe::M1, cap).unwrap();
    assert_eq!(
        prepared.candles(Timeframe::M1).unwrap(),
        corpus::candle_context(&f.root, &f.spec, Timeframe::M1, cap).unwrap()
    );
    assert_eq!(
        prepared.signals(&strategy, 10000).unwrap(),
        corpus::signals(&f.root, &f.spec, &strategy, cap, 10000).unwrap()
    );
    assert!(
        tick_engine::corpus_key(
            &f.root,
            &f.spec,
            &strategy,
            &Costs::default(),
            CorpusBudget {
                max_candles: cap,
                feature_cells: 10000,
                max_receipts: 1000
            }
        )
        .is_ok()
    );
}

#[test]
fn verified_no_frame_inventory_preserves_inspection_and_refuses_missing_context_or_changed_paths() {
    let f = Fixture::new("no-frame-inventory", "EURUSD", 7);
    let inventory = corpus::verified_inventory(&f.root, &f.spec).unwrap();
    assert_eq!(inventory.retained_candles(), 0);
    assert_eq!(
        serde_json::to_value(inventory.report()).unwrap(),
        serde_json::to_value(corpus::inspect(&f.root, &f.spec).unwrap()).unwrap()
    );
    for timeframe in Timeframe::ALL {
        assert!(inventory.candles(timeframe).is_err());
    }
    inventory.verify_current(&f.root, &f.spec).unwrap();
    let mut moved = f.spec.clone();
    moved.chunks[0].path = f.root.join("missing.rsf").to_str().unwrap().into();
    assert_eq!(moved.identity().unwrap(), f.spec.identity().unwrap());
    assert!(inventory.verify_current(&f.root, &moved).is_err());
    let mut limited = f.spec.clone();
    limited.max_chunk_events = 1;
    assert_eq!(limited.identity().unwrap(), f.spec.identity().unwrap());
    assert!(inventory.verify_current(&f.root, &limited).is_err());
    let path = &f.spec.chunks[0].path;
    let bytes = fs::read(path).unwrap();
    let mut bad = bytes.clone();
    let index = bad.len() / 2;
    bad[index] ^= 1;
    fs::write(path, bad).unwrap();
    assert!(inventory.verify_current(&f.root, &f.spec).is_err());
    fs::write(path, bytes).unwrap();
    inventory.verify_current(&f.root, &f.spec).unwrap();
}

#[test]
fn source_facts_match_full_inspection_all_symbols_native_blocked_and_exact_cli_keys() {
    use rust_strategy_factory::contracts::SYMBOLS;
    for symbol in SYMBOLS {
        let f = Fixture::new("source-facts-layout", symbol, 7);
        let block = f.root.join("whole.rsfb");
        rust_strategy_factory::blocked_native::write(&block, &f.data, 3).unwrap();
        let blocked = corpus::describe(&[block], 100).unwrap();
        for spec in [&f.spec, &blocked] {
            let facts = corpus::source_facts(&f.root, spec).unwrap();
            let report = corpus::inspect(&f.root, spec).unwrap();
            assert_eq!(facts.corpus_identity(), report.corpus_identity);
            assert_eq!(facts.profile(), &report.profile);
            assert_eq!(facts.declared_class(), report.declared_class);
            assert_eq!(facts.source(), report.source);
            assert_eq!(facts.events(), report.events);
            assert_eq!(facts.ordered_events_hash(), report.ordered_events_hash);
            assert_eq!(
                serde_json::to_value(facts.first()).unwrap(),
                serde_json::to_value(&report.first).unwrap()
            );
            assert_eq!(
                serde_json::to_value(facts.latest()).unwrap(),
                serde_json::to_value(&report.latest).unwrap()
            );
            assert_eq!(
                facts.max_resident_quote_events(),
                report.max_resident_quote_events
            );
            facts.verify_current(&f.root, spec).unwrap();
            let value = serde_json::to_value(&facts).unwrap();
            assert!(value.get("candles").is_none());
        }
        let spec = f.root.join("facts-spec.json");
        fs::write(&spec, serde_json::to_vec(&f.spec).unwrap()).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args(["corpus-source-facts", spec.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            serde_json::to_value(corpus::source_facts(&f.root, &f.spec).unwrap()).unwrap()
        );
        let path = &f.spec.chunks[0].path;
        let bytes = fs::read(path).unwrap();
        let mut bad = bytes.clone();
        let index = bad.len() / 2;
        bad[index] ^= 1;
        fs::write(path, bad).unwrap();
        assert!(corpus::source_facts(&f.root, &f.spec).is_err());
        fs::write(path, bytes).unwrap();
    }
}
