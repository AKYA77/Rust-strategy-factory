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
    fs::write(d.join("config.json"),serde_json::to_vec(&serde_json::json!({"inputs":[{"dataset":"input.json"}],"limits":{"feature_cells":100000}})).unwrap()).unwrap();
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
