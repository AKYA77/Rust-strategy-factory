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
            bid: 100300,
            ask: 100310,
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
        stop_policy: None,
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks: 50,
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

#[test]
fn valid_future_numeric_overflow_cannot_change_an_earlier_window_or_higher_features() {
    fn history(extra: bool) -> Input {
        let mut ticks = (0..180)
            .flat_map(|i| {
                let phase = i % 40;
                let level = if phase < 20 {
                    0
                } else if phase < 30 {
                    (phase - 19) * 200
                } else {
                    (40 - phase) * 200
                };
                [
                    Tick {
                        timestamp_ms: i * 60000,
                        sequence: (i * 2) as u64,
                        bid: 100000 + level,
                        ask: 100020 + level,
                    },
                    Tick {
                        timestamp_ms: i * 60000 + 1000,
                        sequence: (i * 2 + 1) as u64,
                        bid: 100010 + level,
                        ask: 100030 + level,
                    },
                ]
            })
            .collect::<Vec<_>>();
        if extra {
            for minute in 200..212 {
                ticks.push(Tick {
                    timestamp_ms: minute * 60000,
                    sequence: ticks.len() as u64,
                    bid: 8_000_000_000_000_000_000,
                    ask: 8_000_000_000_000_000_020,
                });
            }
        }
        let n = ticks.len();
        Input {
            dataset: data::dataset(
                Profile::synthetic("EURUSD").unwrap(),
                ticks,
                "SYNTHETIC_TEST_ONLY future numeric boundary",
                "generated".into(),
                true,
            )
            .unwrap(),
            conversions: vec![Conversion::usd(); n],
        }
    }
    let base = history(false);
    let future = history(true);
    future.dataset.verify().unwrap();
    let mut s = strategy();
    s.stop_ticks = 100;
    s.rule = Rule::BollingerCross {
        window: 20,
        multiple_bps: 20000,
        upper: true,
    };
    let all = candles::build(&future.dataset, Timeframe::M1).unwrap();
    assert!(
        Features::with_context(&all, 100000, &future.dataset, Timeframe::M1)
            .unwrap()
            .signal(&s.rule)
            .unwrap_err()
            .contains("overflow")
    );
    let window = Interval {
        start_ms: 0,
        end_exclusive_ms: 60 * 60000,
    };
    let dir = std::env::temp_dir().join(format!("factory-feature-window-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let mut registry = Registry::open(&dir.join("state.sqlite")).unwrap();
    let evaluation = Evaluation {
        window: window.clone(),
        stage: Stage::TickVerification,
        protocol_hash: "causal-feature-regression".into(),
    };
    let a = research::evaluate_window(
        &mut registry,
        &base,
        &s,
        &Costs::default(),
        "causal-features",
        100000,
        &evaluation,
    )
    .unwrap();
    let b = research::evaluate_window(
        &mut registry,
        &future,
        &s,
        &Costs::default(),
        "causal-features",
        100000,
        &evaluation,
    )
    .unwrap();
    assert_eq!(a.outcome, "completed");
    assert_eq!(b.outcome, "completed");
    assert_eq!(
        a.result["tick_replay"]["trades"],
        b.result["tick_replay"]["trades"]
    );
    assert_eq!(
        b.result["tick_replay"]["trades"][0]["net_micro_usd"],
        4_998_000
    );
    assert_eq!(a.result["screen"]["signals"], b.result["screen"]["signals"]);
    let rule = Rule::HigherTimeframe {
        timeframe: Timeframe::M5,
        rule: Box::new(Rule::BollingerCross {
            window: 5,
            multiple_bps: 20000,
            upper: true,
        }),
    };
    assert!(
        Features::with_context(&all, 100000, &future.dataset, Timeframe::M1)
            .unwrap()
            .signal(&rule)
            .is_err()
    );
    let n = all.partition_point(|c| c.available_ms < window.end_exclusive_ms);
    let actual = Features::with_context(&all[..n], 100000, &future.dataset, Timeframe::M1)
        .unwrap()
        .signal(&rule)
        .unwrap();
    let prior = candles::build(&base.dataset, Timeframe::M1).unwrap();
    let expected = Features::with_context(&prior[..n], 100000, &base.dataset, Timeframe::M1)
        .unwrap()
        .signal(&rule)
        .unwrap();
    assert_eq!(actual, expected);
    registry.integrity().unwrap();
    drop(registry);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn malformed_serialized_masks_fail_validation_without_index_panics() {
    use rust_strategy_factory::indicators::Mask;
    let input = input(false);
    let s = strategy();
    let c = candles::build(&input.dataset, Timeframe::M1).unwrap();
    let window = Interval {
        start_ms: 0,
        end_exclusive_ms: 121001,
    };
    for value in [
        serde_json::json!({"words":[],"len":c.len()}),
        serde_json::json!({"words":[u64::MAX],"len":c.len()}),
    ] {
        let malformed: Mask = serde_json::from_value(value).unwrap();
        assert!(malformed.validate().is_err());
        assert!(
            tick_engine::replay_window(
                &input.dataset,
                &c,
                &malformed,
                &s,
                &Costs::default(),
                &input.conversions,
                &window
            )
            .is_err()
        );
    }
}
