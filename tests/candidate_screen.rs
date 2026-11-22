use rust_strategy_factory::{
    candidate_screen::{self, AuditCoverage, NetBounds},
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
    s.entry = Some(EntryPlan::StopMarket {
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

#[test]
fn net_bounds_bracket_the_adverse_resolution() {
    // Collision: both stop and target are possible in the exit bar, so the
    // adverse stop proxy is pessimistic and the requested target is optimistic.
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
        "SYNTHETIC_TEST_ONLY net bounds",
        "generated".into(),
        true,
    )
    .unwrap();
    let e = estimate(&d, &strategy(), &Costs::default());
    let bounds = e.net_bounds.expect("collision estimate resolves one trade");
    assert_eq!(bounds.closed_trades, 1);
    assert_eq!(bounds.pessimistic_net_micro_usd, e.estimated_net_micro_usd);
    assert_eq!(bounds.pessimistic_wins, e.estimated_net_wins);
    assert!(bounds.pessimistic_net_micro_usd < 0);
    assert_eq!(bounds.pessimistic_win_rate_bps, Some(0));
    // Best case assumes the target fills first; it must dominate the adverse proxy.
    assert!(bounds.optimistic_net_micro_usd > bounds.pessimistic_net_micro_usd);
    assert_eq!(bounds.optimistic_wins, 1);
    assert_eq!(bounds.optimistic_win_rate_bps, Some(10000));
    assert_eq!(
        bounds,
        NetBounds {
            version: "candidate-net-bounds-v1".into(),
            ..bounds.clone()
        }
    );
}

#[test]
fn net_bounds_stop_only_resolution_dominates_adverse_proxy() {
    let d = dataset(99000);
    let e = estimate(&d, &strategy(), &Costs::default());
    let bounds = e.net_bounds.expect("stop resolution closes one trade");
    assert_eq!(bounds.closed_trades, 1);
    assert!(bounds.pessimistic_net_micro_usd < 0);
    assert!(bounds.optimistic_net_micro_usd > bounds.pessimistic_net_micro_usd);
    assert!(bounds.optimistic_net_micro_usd < 0); // even the best case loses
    assert_eq!(bounds.pessimistic_wins, 0);
    assert_eq!(bounds.optimistic_wins, 0);
    assert_eq!(bounds.pessimistic_win_rate_bps, Some(0));
    assert_eq!(bounds.optimistic_win_rate_bps, Some(0));
}

#[test]
fn unresolved_estimate_has_no_net_bounds() {
    let d = dataset(100000);
    let e = estimate(&d, &strategy(), &Costs::default());
    assert_eq!(e.status, "unresolved-partial");
    assert_eq!(e.estimated_closed_trades, 0);
    assert_eq!(e.net_bounds, None);
}

#[test]
fn bound_proven_failure_uses_optimistic_bound_not_point_estimate() {
    let costs = Costs::default();
    let strict = candidate_screen::ShadowPolicy {
        promote_uncertain: false,
        minimum_closed_trades: 1,
        ..Default::default()
    };

    // Collision: point estimate loses (would_reject) but the optimistic bound
    // reaches the threshold, so failure is not proven and the candidate stays
    // uncertain/escalates to tick verification.
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
        "SYNTHETIC_TEST_ONLY collision",
        "generated".into(),
        true,
    )
    .unwrap();
    let e = estimate(&d, &strategy(), &costs);
    let dec = strict.decision(&e, "collision").unwrap();
    assert_eq!(dec["would_reject"], true);
    assert_eq!(dec["bound_proven_failure"], false);
    assert_eq!(dec["screen_classification"], "ScreenUncertain");

    // Stop-only with an adverse gap: even the optimistic bound misses the threshold.
    let d = dataset(99000);
    let e = estimate(&d, &strategy(), &costs);
    let dec = strict.decision(&e, "gap").unwrap();
    assert_eq!(dec["would_reject"], true);
    assert_eq!(dec["bound_proven_failure"], true);
    assert_eq!(dec["screen_classification"], "ProvenScreenFailure");
}

#[test]
fn audit_coverage_aggregates_bound_rejections_and_false_negatives() {
    let audit = |would_reject, proven, sampled, unresolved, negative| {
        serde_json::json!({
            "shadow_decision": {
                "would_reject": would_reject,
                "bound_proven_failure": proven,
                "selected_for_sampled_audit": sampled,
            },
            "actual_unresolved": unresolved,
            "shadow_false_negative": negative,
        })
    };
    let empty: AuditCoverage = candidate_screen::audit_coverage(&[]);
    assert_eq!(empty.candidates, 0);
    assert_eq!(empty.bound_rejections, 0);
    assert_eq!(empty.coverage_bps, 0);
    assert!(empty.false_negative_rate_bps.is_none());

    let batch = [
        audit(true, false, true, false, true), // rejected, sampled, audited, false negative
        audit(true, true, false, false, false), // proven failure, audited, not a false negative
        audit(false, false, false, false, false), // promoted to tick
        audit(true, false, false, true, false), // rejected but unresolved: not audited
    ];
    let c = candidate_screen::audit_coverage(&batch);
    assert_eq!(c.candidates, 4);
    assert_eq!(c.bound_rejections, 3);
    assert_eq!(c.proven_failures, 1);
    assert_eq!(c.promoted_to_tick, 1);
    assert_eq!(c.sampled_for_audit, 1);
    assert_eq!(c.audited, 2);
    assert_eq!(c.false_negatives, 1);
    assert_eq!(c.false_negative_rate_bps, Some(5000));
    assert_eq!(c.coverage_bps, 2 * 10000 / 3);
}

use rust_strategy_factory::storage::{Claim, ExperimentKey, Outcome, Registry, Stage};

fn registry_key(concept: &str) -> ExperimentKey {
    ExperimentKey {
        concept_hash: concept.into(),
        instrument: "EURUSD".into(),
        dataset_hash: "quotes".into(),
        start_ms: 0,
        end_exclusive_ms: 1000,
        signal_version: "s1".into(),
        indicator_version: "i1".into(),
        cost_hash: "cost".into(),
        fill_policy: "fill".into(),
        profile_hash: "profile".into(),
        engine_version: "e1".into(),
        protocol_hash: "protocol".into(),
        code_revision: "revision".into(),
        configuration_hash: "config".into(),
        stage: Stage::TickVerification,
    }
}

fn publish(registry: &mut Registry, concept: &str, result: serde_json::Value) {
    let id = registry.register(&registry_key(concept)).unwrap();
    let lease = match registry.claim(&id, 0, 60_000).unwrap() {
        Claim::Acquired(lease) => lease,
        other => panic!("expected an acquired lease, got {other:?}"),
    };
    assert!(
        registry
            .finish(&lease, 1, Outcome::Completed, &result)
            .unwrap()
    );
}

fn marker(
    would_reject: bool,
    sampled: bool,
    unresolved: bool,
    negative: bool,
) -> serde_json::Value {
    serde_json::json!({
        "shadow_decision": {
            "would_reject": would_reject,
            "bound_proven_failure": false,
            "selected_for_sampled_audit": sampled,
        },
        "actual_unresolved": unresolved,
        "shadow_false_negative": negative,
    })
}

#[test]
fn registry_coverage_reports_marker_absence_as_never_covered() {
    let dir = std::env::temp_dir().join(format!("factory-audit-coverage-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("state.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    publish(
        &mut registry,
        "no-marker",
        serde_json::json!({"kind": "synthetic-corpus-replay-v1"}),
    );
    publish(
        &mut registry,
        "rejected-false-negative",
        serde_json::json!({"candidate_screen_audit": marker(true, true, false, true)}),
    );
    publish(
        &mut registry,
        "promoted",
        serde_json::json!({"candidate_screen_audit": marker(false, false, false, false)}),
    );
    let coverage = candidate_screen::registry_coverage(
        &registry,
        candidate_screen::REGISTRY_COVERAGE_SCAN_LIMIT,
    )
    .unwrap();
    assert_eq!(coverage.version, "registry-audit-coverage-v1");
    assert_eq!(coverage.results_total, 3);
    assert_eq!(coverage.results_with_marker, 2);
    assert_eq!(coverage.results_without_marker, 1);
    assert_eq!(coverage.scanned_audits, 2);
    assert!(!coverage.truncated);
    assert_eq!(coverage.coverage.candidates, 2);
    assert_eq!(coverage.coverage.bound_rejections, 1);
    assert_eq!(coverage.coverage.promoted_to_tick, 1);
    assert_eq!(coverage.coverage.audited, 1);
    assert_eq!(coverage.coverage.false_negatives, 1);
    assert_eq!(coverage.coverage.false_negative_rate_bps, Some(10000));
    assert_eq!(coverage.coverage.coverage_bps, 10000);
    assert!(coverage.limitations.contains("never treated as coverage"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn registry_coverage_bound_is_explicit_and_corrupt_registry_refuses() {
    let dir = std::env::temp_dir().join(format!(
        "factory-audit-coverage-bound-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("state.sqlite");
    let mut registry = Registry::open(&path).unwrap();
    for concept in ["a", "b", "c"] {
        publish(
            &mut registry,
            concept,
            serde_json::json!({"candidate_screen_audit": marker(true, false, false, false)}),
        );
    }
    assert!(candidate_screen::registry_coverage(&registry, 0).is_err());
    assert!(
        candidate_screen::registry_coverage(
            &registry,
            candidate_screen::REGISTRY_COVERAGE_SCAN_LIMIT + 1
        )
        .is_err()
    );
    let bounded = candidate_screen::registry_coverage(&registry, 2).unwrap();
    assert!(bounded.truncated);
    assert_eq!(bounded.scanned_audits, 2);
    assert_eq!(bounded.results_with_marker, 3);
    assert_eq!(bounded.coverage.candidates, 2);
    let id = registry_key("a").identity().unwrap();
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute(
        "UPDATE experiments SET key_json=?1 WHERE id=?2",
        ("{}", &id),
    )
    .unwrap();
    drop(raw);
    assert!(candidate_screen::registry_coverage(&registry, 2).is_err());
    std::fs::remove_dir_all(&dir).ok();
}
