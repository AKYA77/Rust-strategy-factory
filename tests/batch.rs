use rust_strategy_factory::{
    candles::Timeframe,
    contracts::{Conversion, Profile, SYMBOLS, Side},
    data::{self, Tick},
    proof_screen,
    research::{self, BatchOptions, Evaluation, EvaluationOptions, Input},
    storage::{Registry, Stage},
    strategies::{EntryPlan, Rule, Strategy},
    tick_engine::Costs,
    validation::Interval,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn directory() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "factory-batch-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&d).unwrap();
    d
}
fn input(symbol: &str) -> Input {
    let p = Profile::synthetic(symbol).unwrap();
    let base = if symbol == "XAUUSD" { 250000 } else { 100000 };
    let ticks = (0..1200)
        .map(|i| {
            let n = i % 120;
            let wave = if n < 60 { n } else { 120 - n };
            let bid = base + wave * p.pip * 5;
            Tick {
                timestamp_ms: 1735689600000 + i * 10000,
                sequence: i as u64,
                bid,
                ask: bid + p.pip,
            }
        })
        .collect();
    let dataset = data::dataset(
        p,
        ticks,
        "SYNTHETIC_TEST_ONLY shared batch",
        "generated".into(),
        true,
    )
    .unwrap();
    let conversions = dataset
        .ticks
        .iter()
        .map(|t| {
            if dataset.profile.quote == "USD" {
                Conversion::usd()
            } else {
                Conversion {
                    usd_num: 1,
                    quote_den: if symbol == "USDJPY" { 150 } else { 1 },
                    as_of_ms: t.timestamp_ms,
                }
            }
        })
        .collect();
    Input {
        dataset,
        conversions,
    }
}
fn strategies() -> Vec<Strategy> {
    [
        Rule::AboveMean { window: 5 },
        Rule::All(vec![
            Rule::AboveMean { window: 5 },
            Rule::Rising { lookback: 1 },
        ]),
        Rule::HigherTimeframe {
            timeframe: Timeframe::M5,
            rule: Box::new(Rule::Rising { lookback: 1 }),
        },
        Rule::MacdCross {
            fast: 2,
            slow: 5,
            signal_window: 2,
            above: true,
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(i, rule)| Strategy {
        timeframe: Timeframe::M1,
        side: if i % 2 == 0 { Side::Long } else { Side::Short },
        stop_ticks: 100,
        rule,
        entry: None,
        stop_policy: None,
    })
    .collect()
}
fn options<'a>(
    evaluation: Option<&'a Evaluation>,
    proof_policy: Option<&'a proof_screen::Policy>,
) -> BatchOptions<'a> {
    BatchOptions {
        evaluation: EvaluationOptions {
            feature_cells: 100000,
            evaluation,
            shadow_policy: None,
            proof_policy,
            indexed_policy: None,
        },
        state_budget_bytes: 256 * 1024 * 1024,
    }
}
#[test]
fn shared_batches_equal_independent_scalar_receipts_for_all_instruments_and_reuse() {
    let d = directory();
    let mut scalar = Registry::open(&d.join("scalar.sqlite")).unwrap();
    let mut batch = Registry::open(&d.join("batch.sqlite")).unwrap();
    let costs = Costs {
        commission_micro_usd_per_lot_per_fill: 1_000_000,
        ..Default::default()
    };
    let candidates = strategies();
    let revision = research::compute_revision().unwrap();
    let mut trades = 0;
    for symbol in SYMBOLS {
        let input = input(symbol);
        let settings = options(None, None);
        let reference = candidates
            .iter()
            .map(|s| {
                research::evaluate_configured(
                    &mut scalar,
                    &input,
                    s,
                    &costs,
                    &revision,
                    settings.evaluation,
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        let actual =
            research::evaluate_batch(&mut batch, &input, &candidates, &costs, &revision, settings)
                .unwrap();
        for (a, b) in actual.iter().zip(&reference) {
            assert!(!a.reused);
            assert_eq!(a.experiment_id, b.experiment_id);
            assert_eq!(a.outcome, b.outcome);
            assert_eq!(a.result, b.result);
            assert_ne!(a.outcome, "failed");
            trades += a.result["tick_replay"]["trades"]
                .as_array()
                .map_or(0, Vec::len);
        }
        let repeated = research::evaluate_batch(
            &mut batch,
            &input,
            &candidates,
            &costs,
            &revision,
            options(None, None),
        )
        .unwrap();
        assert!(repeated.iter().all(|e| e.reused));
    }
    assert!(trades > 0);
    assert_eq!(batch.counts().unwrap().attempts, 32);
    assert_eq!(scalar.counts().unwrap().attempts, 32);
    scalar.integrity().unwrap();
    batch.integrity().unwrap();
    drop(scalar);
    drop(batch);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn shared_window_atr_pending_and_proof_contexts_keep_exact_scalar_results() {
    let d = directory();
    let input = input("EURUSD");
    let mut scalar = Registry::open(&d.join("scalar.sqlite")).unwrap();
    let mut batch = Registry::open(&d.join("batch.sqlite")).unwrap();
    let window = Interval {
        start_ms: input.dataset.manifest.first_ms + 600000,
        end_exclusive_ms: input.dataset.manifest.first_ms + 7_000_000,
    };
    let evaluation = Evaluation {
        protocol_hash: data::hash(&("training-window-v1", &window)).unwrap(),
        window,
        stage: Stage::TickVerification,
    };
    let policy = proof_screen::Policy::default();
    let mut candidates = strategies();
    candidates[0].stop_ticks = 30;
    candidates[1].stop_policy = Some(rust_strategy_factory::stops::Policy::Atr {
        window: 3,
        multiple_bps: 15000,
    });
    candidates[2].entry = Some(EntryPlan::Limit {
        offset_ticks: 30,
        ttl_ms: 600000,
        cancel_on_signal_false: true,
    });
    let costs = Costs::default();
    let revision = research::compute_revision().unwrap();
    let expected = candidates
        .iter()
        .map(|s| {
            research::evaluate_configured(
                &mut scalar,
                &input,
                s,
                &costs,
                &revision,
                options(Some(&evaluation), Some(&policy)).evaluation,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let actual = research::evaluate_batch(
        &mut batch,
        &input,
        &candidates,
        &costs,
        &revision,
        options(Some(&evaluation), Some(&policy)),
    )
    .unwrap();
    for (a, b) in actual.iter().zip(&expected) {
        assert_eq!(a.experiment_id, b.experiment_id);
        assert_eq!(a.result, b.result);
        assert_ne!(a.outcome, "failed");
    }
    assert_eq!(actual[0].result["proof_rejection"], true);
    drop(scalar);
    drop(batch);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn shared_batch_preflight_refuses_invalid_budgets_or_mixed_primary_frames() {
    let d = directory();
    let mut registry = Registry::open(&d.join("state.sqlite")).unwrap();
    let input = input("EURUSD");
    let costs = Costs::default();
    let candidates = strategies();
    assert!(
        research::evaluate_batch(
            &mut registry,
            &input,
            &[],
            &costs,
            "batch-budget",
            options(None, None)
        )
        .is_err()
    );
    assert!(
        research::evaluate_batch(
            &mut registry,
            &input,
            &vec![candidates[0].clone(); 65],
            &costs,
            "batch-budget",
            options(None, None)
        )
        .is_err()
    );
    let mut mixed = candidates.clone();
    mixed[1].timeframe = Timeframe::M5;
    assert!(
        research::evaluate_batch(
            &mut registry,
            &input,
            &mixed,
            &costs,
            "batch-budget",
            options(None, None)
        )
        .is_err()
    );
    let mut invalid = options(None, None);
    invalid.evaluation.feature_cells = 0;
    assert!(
        research::evaluate_batch(
            &mut registry,
            &input,
            &candidates,
            &costs,
            "batch-budget",
            invalid
        )
        .is_err()
    );
    assert_eq!(registry.counts().unwrap().registered, 0);
    let mut corrupted = Input {
        dataset: input.dataset.clone(),
        conversions: input.conversions.clone(),
    };
    corrupted.dataset.ticks[0].bid += corrupted.dataset.profile.tick;
    assert!(
        research::evaluate_batch(
            &mut registry,
            &corrupted,
            &candidates,
            &costs,
            "batch-corruption",
            options(None, None)
        )
        .is_err()
    );
    assert_eq!(registry.counts().unwrap().registered, 0);
    drop(registry);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn busy_member_preserves_committed_prefix_and_retry_reuses_other_worker_receipt() {
    use rust_strategy_factory::storage::{Claim, Outcome};
    let d = directory();
    let input = input("EURUSD");
    let candidates = strategies();
    let costs = Costs::default();
    let revision = research::compute_revision().unwrap();
    let mut other = Registry::open(&d.join("other.sqlite")).unwrap();
    let reference = research::evaluate_configured(
        &mut other,
        &input,
        &candidates[1],
        &costs,
        &revision,
        options(None, None).evaluation,
    )
    .unwrap();
    let key = other
        .experiment_key(&reference.experiment_id)
        .unwrap()
        .unwrap();
    let mut registry = Registry::open(&d.join("batch.sqlite")).unwrap();
    let id = registry.register(&key).unwrap();
    let Claim::Acquired(lease) = registry
        .claim(&id, chrono::Utc::now().timestamp_millis(), 300000)
        .unwrap()
    else {
        panic!("new other-worker lease")
    };
    assert!(
        research::evaluate_batch(
            &mut registry,
            &input,
            &candidates,
            &costs,
            &revision,
            options(None, None)
        )
        .unwrap_err()
        .contains("leased")
    );
    assert_eq!(registry.counts().unwrap().completed, 1);
    // Another worker publishes the exact independently computed receipt for that key.
    registry
        .finish(
            &lease,
            chrono::Utc::now().timestamp_millis(),
            Outcome::Completed,
            &reference.result,
        )
        .unwrap();
    let results = research::evaluate_batch(
        &mut registry,
        &input,
        &candidates,
        &costs,
        &revision,
        options(None, None),
    )
    .unwrap();
    assert!(results[0].reused && results[1].reused);
    assert!(results[2..].iter().all(|e| !e.reused));
    assert_eq!(registry.counts().unwrap().attempts, 4);
    registry.integrity().unwrap();
    drop(other);
    drop(registry);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn actual_batch_cli_reports_shared_compilation_and_separate_cached_reuse() {
    use std::{fs, process::Command};
    let d = directory();
    let input = input("EURUSD");
    let candidates = strategies();
    fs::write(
        d.join("input.json"),
        serde_json::to_vec(&input.dataset).unwrap(),
    )
    .unwrap();
    fs::write(
        d.join("candidates.json"),
        serde_json::to_vec(&candidates).unwrap(),
    )
    .unwrap();
    fs::write(d.join("config.json"),serde_json::to_vec(&serde_json::json!({"inputs":[{"dataset":"input.json"}],"limits":{"feature_cells":100000,"indexed_policy":{"version":"training-shared-index-v1","audit_bps":10000,"seed":42}}})).unwrap()).unwrap();
    let invoke = || {
        let output = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args([
                "batch-experiments",
                d.join("config.json").to_str().unwrap(),
                d.join("candidates.json").to_str().unwrap(),
                d.join("state.sqlite").to_str().unwrap(),
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
    let first = invoke();
    assert_eq!(first["new_attempts"], 4);
    assert_eq!(first["reused"], 0);
    assert!(first["shared_feature_builds"].as_u64().unwrap() > 0);
    assert!(
        first["executions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["result"]["indexed_replay"]["reference_audited"] == true)
    );
    let second = invoke();
    assert_eq!(second["new_attempts"], 0);
    assert_eq!(second["reused"], 4);
    assert_eq!(second["shared_feature_builds"], 0);
    for (a, b) in first["executions"]
        .as_array()
        .unwrap()
        .iter()
        .zip(second["executions"].as_array().unwrap())
    {
        assert_eq!(a["experiment_id"], b["experiment_id"]);
        assert_eq!(a["result"], b["result"]);
    }
    let registry = Registry::open(&d.join("state.sqlite")).unwrap();
    assert_eq!(registry.counts().unwrap().attempts, 4);
    drop(registry);
    fs::remove_dir_all(d).unwrap();
}

#[test]
fn indexed_training_batches_preserve_financial_receipts_and_audit_supported_cases() {
    use rust_strategy_factory::indexed_policy::Policy;
    let d = directory();
    let mut registry = Registry::open(&d.join("state.sqlite")).unwrap();
    let candidates = strategies();
    let costs = Costs::default();
    let revision = research::compute_revision().unwrap();
    let policy = Policy {
        audit_bps: 10000,
        ..Default::default()
    };
    let mut fast = 0;
    let mut fallback = 0;
    for symbol in SYMBOLS {
        let input = input(symbol);
        let expected = research::evaluate_batch(
            &mut registry,
            &input,
            &candidates,
            &costs,
            &revision,
            options(None, None),
        )
        .unwrap();
        let mut configured = options(None, None);
        configured.evaluation.indexed_policy = Some(&policy);
        let actual = research::evaluate_batch(
            &mut registry,
            &input,
            &candidates,
            &costs,
            &revision,
            configured,
        )
        .unwrap();
        for (a, b) in actual.iter().zip(expected) {
            assert_ne!(a.experiment_id, b.experiment_id);
            assert_eq!(a.result["tick_replay"], b.result["tick_replay"]);
            assert_eq!(a.outcome, b.outcome);
            if a.result["indexed_replay"]["used_index"] == true {
                fast += 1;
                assert_eq!(a.result["indexed_replay"]["reference_audited"], true);
                let hashes = a.result["indexed_replay"]["reference_hashes"]
                    .as_array()
                    .unwrap();
                assert_eq!(hashes[0], hashes[1]);
            } else {
                fallback += 1;
                assert_eq!(a.result["indexed_replay"]["reference_audited"], false);
            }
        }
        let mut configured = options(None, None);
        configured.evaluation.indexed_policy = Some(&policy);
        assert!(
            research::evaluate_batch(
                &mut registry,
                &input,
                &candidates,
                &costs,
                &revision,
                configured
            )
            .unwrap()
            .iter()
            .all(|e| e.reused)
        );
    }
    assert_eq!((fast, fallback), (20, 12));
    assert_eq!(registry.counts().unwrap().attempts, 64);
    registry.integrity().unwrap();
    drop(registry);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn indexed_policy_health_blocks_cached_and_pending_publication_and_protects_science() {
    use rust_strategy_factory::{
        indexed_policy::{self, Policy},
        storage::{Claim, Outcome},
    };
    let d = directory();
    let mut registry = Registry::open(&d.join("state.sqlite")).unwrap();
    let input = input("EURUSD");
    let candidates = strategies();
    let costs = Costs::default();
    let revision = research::compute_revision().unwrap();
    let policy = Policy {
        audit_bps: 10000,
        ..Default::default()
    };
    let mut settings = options(None, None);
    settings.evaluation.indexed_policy = Some(&policy);
    let reference = research::evaluate_configured(
        &mut registry,
        &input,
        &candidates[0],
        &costs,
        &revision,
        options(None, None).evaluation,
    )
    .unwrap();
    let execution = research::evaluate_configured(
        &mut registry,
        &input,
        &candidates[0],
        &costs,
        &revision,
        settings.evaluation,
    )
    .unwrap();
    let actual: rust_strategy_factory::tick_engine::Replay =
        serde_json::from_value(execution.result["tick_replay"].clone()).unwrap();
    let mut altered = actual.clone();
    altered.maximum_drawdown_micro_usd += 1;
    assert!(indexed_policy::compare(&actual, &altered).is_err());
    let key = registry
        .experiment_key(&execution.experiment_id)
        .unwrap()
        .unwrap();
    let mut pending = key.clone();
    pending.code_revision = "indexed-publication-race-fixture".into();
    let id = registry.register(&pending).unwrap();
    let Claim::Acquired(lease) = registry.claim(&id, 0, 300000).unwrap() else {
        panic!("new pending fixture")
    };
    let c = rusqlite::Connection::open(d.join("state.sqlite")).unwrap();
    c.execute("INSERT INTO events(experiment_id,at_ms,action,attempt) VALUES (?1,0,'indexed-audit-contradiction-v1',1)",[&execution.experiment_id]).unwrap();
    drop(c);
    assert!(
        registry
            .finish(&lease, 1, Outcome::Completed, &execution.result)
            .unwrap_err()
            .contains(indexed_policy::BLOCKED)
    );
    registry
        .finish(
            &lease,
            1,
            Outcome::Failed,
            &serde_json::json!({"error":"injected index failure"}),
        )
        .unwrap();
    assert!(
        research::evaluate_configured(
            &mut registry,
            &input,
            &candidates[0],
            &costs,
            &revision,
            settings.evaluation
        )
        .unwrap_err()
        .contains(indexed_policy::BLOCKED)
    );
    assert_eq!(
        research::evaluate_configured(
            &mut registry,
            &input,
            &candidates[0],
            &costs,
            &revision,
            options(None, None).evaluation
        )
        .unwrap()
        .experiment_id,
        reference.experiment_id
    );
    let w = Interval {
        start_ms: input.dataset.manifest.first_ms,
        end_exclusive_ms: input.dataset.manifest.last_ms + 1,
    };
    let evaluation = Evaluation {
        window: w,
        stage: Stage::IndependentValidation,
        protocol_hash: "scientific".into(),
    };
    let mut science = options(Some(&evaluation), None);
    science.evaluation.indexed_policy = Some(&policy);
    // Use a clean registry so this refusal is authorization, not the failure marker.
    let mut clean = Registry::open(&d.join("clean.sqlite")).unwrap();
    assert!(
        research::evaluate_configured(
            &mut clean,
            &input,
            &candidates[0],
            &costs,
            &revision,
            science.evaluation
        )
        .unwrap_err()
        .contains("adaptation requires")
    );
    assert_eq!(clean.counts().unwrap().registered, 0);
    assert!(
        research::Driver::open(
            &d.join("driver.sqlite"),
            vec![input],
            research::Config {
                indexed_policy: Some(policy),
                ..Default::default()
            },
            revision
        )
        .is_err()
    );
    drop(clean);
    drop(registry);
    std::fs::remove_dir_all(d).unwrap();
}
