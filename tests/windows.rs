use rust_strategy_factory::{
    candles::{self, Timeframe},
    contracts::{Conversion, Profile, Side},
    data::{self, Tick},
    research::{self, Evaluation, Input},
    storage::{Registry, Stage},
    strategies::{EntryPlan, Features, Rule, Strategy},
    tick_engine::{self, Costs},
    validation::Interval,
};
fn input(extra: bool) -> Input {
    let mut ticks = vec![
        Tick {
            timestamp_ms: 0,
            sequence: 0,
            bid: 100000,
            ask: 100010,
        },
        Tick {
            timestamp_ms: 60000,
            sequence: 1,
            bid: 100020,
            ask: 100030,
        },
        Tick {
            timestamp_ms: 120000,
            sequence: 2,
            bid: 100040,
            ask: 100050,
        },
        Tick {
            timestamp_ms: 120000,
            sequence: 3,
            bid: 100050,
            ask: 100060,
        },
        Tick {
            timestamp_ms: 121000,
            sequence: 4,
            bid: 100200,
            ask: 100210,
        },
    ];
    if extra {
        ticks.extend((5..100).map(|i| Tick {
            timestamp_ms: i * 60000,
            sequence: i as u64,
            bid: 90000 + i * 100,
            ask: 90020 + i * 100,
        }));
    }
    let n = ticks.len();
    Input {
        dataset: data::dataset(
            Profile::synthetic("EURUSD").unwrap(),
            ticks,
            "SYNTHETIC_TEST_ONLY chronological windows",
            "generated".into(),
            true,
        )
        .unwrap(),
        conversions: vec![Conversion::usd(); n],
    }
}
fn strategy() -> Strategy {
    Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks: 30,
        rule: Rule::AboveMean { window: 2 },
        entry: None,
    }
}
fn run(input: &Input, s: &Strategy, w: &Interval) -> tick_engine::Replay {
    let c = candles::build(&input.dataset, s.timeframe).unwrap();
    let m = Features::new(&c, c.len() * 40)
        .unwrap()
        .signal(&s.rule)
        .unwrap();
    tick_engine::replay_window(
        &input.dataset,
        &c,
        &m,
        s,
        &Costs::default(),
        &input.conversions,
        w,
    )
    .unwrap()
}
#[test]
fn prior_warmup_is_available_without_pre_window_positions() {
    let i = input(false);
    let s = strategy();
    let r = run(
        &i,
        &s,
        &Interval {
            start_ms: 120000,
            end_exclusive_ms: 122000,
        },
    );
    assert_eq!(r.trades.len(), 1);
    assert_eq!(r.trades[0].entry_event, 3);
    let r = run(
        &i,
        &s,
        &Interval {
            start_ms: 120001,
            end_exclusive_ms: 122000,
        },
    );
    assert!(r.trades.is_empty());
    assert!(r.unresolved.is_none());
    assert_eq!(r.final_marked_micro_usd, 20_000_000);
}
#[test]
fn end_is_exclusive_and_future_quotes_cannot_close_or_change_window_positions() {
    let w = Interval {
        start_ms: 120000,
        end_exclusive_ms: 121000,
    };
    let s = strategy();
    let a = run(&input(false), &s, &w);
    let b = run(&input(true), &s, &w);
    assert!(a.trades.is_empty());
    assert!(a.unresolved.is_some());
    assert_eq!(a.realized_micro_usd, b.realized_micro_usd);
    assert_eq!(a.final_marked_micro_usd, b.final_marked_micro_usd);
    assert_eq!(a.maximum_drawdown_micro_usd, b.maximum_drawdown_micro_usd);
    assert_eq!(
        serde_json::to_value(&a.unresolved).unwrap(),
        serde_json::to_value(&b.unresolved).unwrap()
    );
}
#[test]
fn pending_orders_cannot_be_placed_before_window_or_filled_after_it() {
    let mut s = strategy();
    s.entry = Some(EntryPlan::StopMarket {
        offset_ticks: 30,
        ttl_ms: 60000,
        cancel_on_signal_false: true,
    });
    let a = run(
        &input(false),
        &s,
        &Interval {
            start_ms: 120000,
            end_exclusive_ms: 120001,
        },
    );
    assert!(a.pending_entry.is_some());
    assert!(a.unresolved.is_none());
    assert!(a.trades.is_empty());
    let b = run(
        &input(false),
        &s,
        &Interval {
            start_ms: 120001,
            end_exclusive_ms: 122000,
        },
    );
    assert!(b.pending_entry.is_none());
    assert!(b.entry_receipts.is_empty());
}
#[test]
fn independent_window_uses_parent_dataset_identity_and_cannot_repeat_retuned_holdout() {
    let dir = std::env::temp_dir().join(format!("factory-window-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let mut registry = Registry::open(&dir.join("state.sqlite")).unwrap();
    let input = input(false);
    let s = strategy();
    let protocol = "frozen-protocol";
    let final_window = Interval {
        start_ms: 120000,
        end_exclusive_ms: 122000,
    };
    registry
        .reserve_holdout(
            &input.dataset.manifest.content_hash,
            &final_window,
            protocol,
            &[s.identity().unwrap()],
        )
        .unwrap();
    let training = Evaluation {
        window: Interval {
            start_ms: 0,
            end_exclusive_ms: 120000,
        },
        stage: Stage::TickVerification,
        protocol_hash: protocol.into(),
    };
    let train = research::evaluate_window(
        &mut registry,
        &input,
        &s,
        &Costs::default(),
        "code",
        100000,
        &training,
    )
    .unwrap();
    assert_eq!(train.outcome, "rejected");
    let evaluation = Evaluation {
        window: final_window,
        stage: Stage::IndependentValidation,
        protocol_hash: protocol.into(),
    };
    let first = research::evaluate_window(
        &mut registry,
        &input,
        &s,
        &Costs::default(),
        "code",
        100000,
        &evaluation,
    )
    .unwrap();
    assert_eq!(
        first.result["tick_replay"]["trades"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let cached = research::evaluate_window(
        &mut registry,
        &input,
        &s,
        &Costs::default(),
        "code",
        100000,
        &evaluation,
    )
    .unwrap();
    assert!(cached.reused);
    let retuned = Costs {
        slippage_pips: 3,
        ..Costs::default()
    };
    assert!(
        research::evaluate_window(
            &mut registry,
            &input,
            &s,
            &retuned,
            "code",
            100000,
            &evaluation
        )
        .is_err()
    );
    assert!(
        research::evaluate(&mut registry, &input, &s, &Costs::default(), "code", 100000).is_err()
    );
    registry.integrity().unwrap();
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}
