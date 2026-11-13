use rust_strategy_factory::{
    candidate_screen,
    candles::{self, Timeframe},
    contracts::{Profile, Side},
    data::{self, Dataset, Tick},
    indicators::Mask,
    strategies::{EntryPlan, Rule, Strategy},
    tick_engine::Costs,
};
fn strategy() -> Strategy {
    Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks: 100,
        rule: Rule::Rising { lookback: 1 },
        entry: None,
        stop_policy: None,
    }
}
fn dataset(hit: i64) -> Dataset {
    let ticks = [
        (0, 100000),
        (60000, 100000),
        (60001, 100000),
        (61000, hit),
        (120000, hit),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (timestamp_ms, bid))| Tick {
        timestamp_ms,
        sequence: i as u64,
        bid,
        ask: bid + 20,
    })
    .collect();
    data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        ticks,
        "SYNTHETIC_TEST_ONLY M1 model",
        "generated".into(),
        true,
    )
    .unwrap()
}
fn estimate(d: &Dataset, s: &Strategy, costs: &Costs) -> candidate_screen::Estimate {
    let bars = candles::build(d, Timeframe::M1).unwrap();
    let mask = Mask::from_bools((0..bars.len()).map(|i| i == 0));
    candidate_screen::estimate(d, &bars, &mask, s, costs, None).unwrap()
}
#[test]
fn target_model_uses_next_quote_entry_two_fees_and_net_wins() {
    let d = dataset(100350);
    let costs = Costs {
        commission_micro_usd_per_lot_per_fill: 1_000_000,
        ..Default::default()
    };
    let e = estimate(&d, &strategy(), &costs);
    // Independent arithmetic: entry100040, target100340, stop100units +20exit slip;
    // 2,000,000 risk /122 =>16,390 volume on10 grid, net300*16,390 -2*16,390.
    assert_eq!(e.estimated_closed_trades, 1);
    assert_eq!(e.estimated_net_wins, 1);
    assert_eq!(e.estimated_net_micro_usd, 4_884_220);
    assert_eq!(e.estimated_net_win_rate_bps, Some(10000));
    assert!(e.uncertainties.contains("entry-or-deadline-partial-bar"));
    assert!(e.promotes_to_tick);
    // Positive price movement is not necessarily a net win with commissions.
    let costs = Costs {
        commission_micro_usd_per_lot_per_fill: 200_000_000,
        ..Default::default()
    };
    let e = estimate(&d, &strategy(), &costs);
    assert_eq!(e.estimated_net_wins, 0);
    assert!(e.estimated_net_micro_usd < 0);
}
#[test]
fn adverse_stop_proxy_keeps_uncapped_gap_loss_and_collision_uncertainty() {
    let d = dataset(99000);
    let e = estimate(&d, &strategy(), &Costs::default());
    assert!(e.estimated_net_micro_usd < -2_000_000);
    assert!(
        e.uncertainties
            .contains("stop-first-quote-unknown-adverse-extremum-proxy")
    );
    let mut ticks = d.ticks.clone();
    ticks.insert(
        4,
        Tick {
            timestamp_ms: 62000,
            sequence: 4,
            bid: 100400,
            ask: 100420,
        },
    );
    ticks[5].sequence = 5;
    let d = data::dataset(
        d.profile,
        ticks,
        "SYNTHETIC_TEST_ONLY collision",
        "generated".into(),
        true,
    )
    .unwrap();
    let e = estimate(&d, &strategy(), &Costs::default());
    assert!(
        e.uncertainties
            .contains("SL-TP-order-unknown-stop-proxy-selected")
    );
    assert_eq!(e.estimated_net_wins, 0);
    assert!(e.promotes_to_tick);
}
#[test]
fn unsupported_and_unclosed_models_escalate_without_fabricating_trades() {
    let d = dataset(100000);
    let e = estimate(&d, &strategy(), &Costs::default());
    assert_eq!(e.status, "unresolved-partial");
    assert_eq!(e.estimated_closed_trades, 0);
    assert_eq!(e.estimated_net_win_rate_bps, None);
    let mut s = strategy();
    s.entry = Some(EntryPlan::Limit {
        offset_ticks: 1,
        ttl_ms: 60000,
        cancel_on_signal_false: false,
    });
    let e = estimate(&d, &s, &Costs::default());
    assert_eq!(e.status, "unsupported");
    assert!(e.promotes_to_tick);
    let mut d = d;
    d.profile = Profile::synthetic("USDJPY").unwrap();
    let d = data::dataset(
        d.profile,
        d.ticks,
        "SYNTHETIC_TEST_ONLY FX",
        "generated".into(),
        true,
    )
    .unwrap();
    assert_eq!(
        estimate(&d, &strategy(), &Costs::default()).status,
        "unsupported"
    );
}
#[test]
fn exclusive_window_does_not_consume_future_bar_or_exit() {
    let d = dataset(100350);
    let bars = candles::build(&d, Timeframe::M1).unwrap();
    let mask = Mask::from_bools([true, false]);
    let w = rust_strategy_factory::validation::Interval {
        start_ms: 60000,
        end_exclusive_ms: 120000,
    };
    let e = candidate_screen::estimate(&d, &bars, &mask, &strategy(), &Costs::default(), Some(&w))
        .unwrap();
    assert_eq!(e.estimated_closed_trades, 0);
    assert_eq!(e.status, "unresolved-partial");
    // The bar published exactly at the exclusive end must not supply an estimate.
    assert!(e.promotes_to_tick);
}

#[test]
fn deadline_uses_first_actual_quote_without_inventing_a_tick_at_24_hours() {
    let mut d = dataset(100000);
    let deadline = 60001 + 86_400_000;
    // A gap across the deadline: first actual later quote closes the pre-gap bar.
    d.ticks.push(Tick {
        timestamp_ms: deadline + 5000,
        sequence: 5,
        bid: 100050,
        ask: 100070,
    });
    d.ticks.push(Tick {
        timestamp_ms: deadline + 65000,
        sequence: 6,
        bid: 100050,
        ask: 100070,
    });
    let d = data::dataset(
        d.profile,
        d.ticks,
        "SYNTHETIC_TEST_ONLY deadline",
        "generated".into(),
        true,
    )
    .unwrap();
    let e = estimate(&d, &strategy(), &Costs::default());
    assert_eq!(e.estimated_closed_trades, 1);
    // Volume16660; entry100040, deadline market sell100030 => -166600.
    assert_eq!(e.estimated_net_micro_usd, -166600);
    assert!(
        e.uncertainties
            .contains("deadline-partial-bar-prior-barrier-order-unknown")
    );
}

#[test]
fn model_budget_exhaustion_remains_a_partial_promoted_result() {
    let ticks = (0..26010)
        .map(|i| Tick {
            timestamp_ms: i * 60000,
            sequence: i as u64,
            bid: 100000,
            ask: 100020,
        })
        .collect();
    let d = data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        ticks,
        "SYNTHETIC_TEST_ONLY bounded model",
        "generated".into(),
        true,
    )
    .unwrap();
    let bars = candles::build(&d, Timeframe::M1).unwrap();
    let mask = Mask::from_bools((0..bars.len()).map(|_| true));
    let e =
        candidate_screen::estimate(&d, &bars, &mask, &strategy(), &Costs::default(), None).unwrap();
    assert_eq!(e.bar_visits, candidate_screen::MAX_BAR_VISITS);
    assert_eq!(e.status, "budget-exhausted-partial");
    assert!(e.promotes_to_tick);
    assert!(e.estimated_closed_trades > 0);
}

#[test]
fn shadow_policy_preserves_uncertainty_and_reproducibly_selects_audits() {
    let e = estimate(&dataset(99000), &strategy(), &Costs::default());
    let conservative = candidate_screen::ShadowPolicy {
        minimum_closed_trades: 1,
        ..Default::default()
    };
    assert_eq!(
        conservative.decision(&e, "candidate").unwrap()["would_reject"],
        false
    );
    let experimental = candidate_screen::ShadowPolicy {
        promote_uncertain: false,
        ..conservative
    };
    let mut selected = 0;
    for i in 0..1000 {
        let a = experimental.decision(&e, &i.to_string()).unwrap();
        let b = experimental.decision(&e, &i.to_string()).unwrap();
        assert_eq!(a, b);
        assert_eq!(a["would_reject"], true);
        assert_eq!(a["proven_failure"], false);
        selected += usize::from(a["selected_for_sampled_audit"] == true);
    }
    assert!((70..130).contains(&selected));
    let mut partial = e.clone();
    partial.status = "unresolved-partial".into();
    assert_eq!(
        experimental.decision(&partial, "x").unwrap()["would_reject"],
        false
    );
    let invalid = candidate_screen::ShadowPolicy {
        audit_sample_bps: 0,
        ..experimental
    };
    assert!(invalid.decision(&e, "x").is_err());
}

#[test]
fn real_tick_target_before_same_bar_stop_is_a_detected_shadow_false_negative() {
    use rust_strategy_factory::{contracts::Conversion, strategies::Features, tick_engine};
    let ticks = [
        (0, 99900),
        (60000, 100000),
        (120000, 100000),
        (120001, 100000),
        (121000, 100350),
        (122000, 99000),
        (180000, 99000),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (timestamp_ms, bid))| Tick {
        timestamp_ms,
        sequence: i as u64,
        bid,
        ask: bid + 20,
    })
    .collect();
    let d = data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        ticks,
        "SYNTHETIC_TEST_ONLY false negative",
        "generated".into(),
        true,
    )
    .unwrap();
    let bars = candles::build(&d, Timeframe::M1).unwrap();
    let s = strategy();
    let costs = Costs::default();
    let mut features = Features::with_context(&bars, 10000, &d, Timeframe::M1).unwrap();
    let mask = features.signal(&s.rule).unwrap();
    let e = candidate_screen::estimate(&d, &bars, &mask, &s, &costs, None).unwrap();
    let replay = tick_engine::replay(
        &d,
        &bars,
        &mask,
        &s,
        &costs,
        &vec![Conversion::usd(); d.ticks.len()],
    )
    .unwrap();
    assert_eq!(replay.trades.len(), 1);
    assert!(replay.trades[0].net_micro_usd > 0);
    assert_eq!(e.estimated_closed_trades, 1);
    assert!(e.estimated_net_micro_usd < 0);
    let policy = candidate_screen::ShadowPolicy {
        promote_uncertain: false,
        minimum_closed_trades: 1,
        audit_sample_bps: 10000,
        ..Default::default()
    };
    let audit = candidate_screen::compare(&e, &replay, "fixture", &policy).unwrap();
    assert_eq!(audit["shadow_false_negative"], true);
    assert_eq!(audit["shadow_decision"]["selected_for_sampled_audit"], true);
    assert_eq!(audit["candidate_rejected"], false);
}
