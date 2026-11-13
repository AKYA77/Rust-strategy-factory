use rust_strategy_factory::{
    contracts::Side,
    tick_engine::{ExitReason, Rejection, Replay, Trade},
    training_diagnostics,
    validation::Interval,
};
fn time(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .unwrap()
        .timestamp_millis()
}
fn window() -> Interval {
    Interval {
        start_ms: time("2025-01-01T00:00:00Z"),
        end_exclusive_ms: time("2027-01-01T00:00:00Z"),
    }
}
fn replay() -> Replay {
    Replay {
        version: "isolated metadata fixture".into(),
        strategy_id: "fixture".into(),
        dataset_id: "fixture".into(),
        profile_id: "fixture".into(),
        cost_id: "fixture".into(),
        conversion_id: "fixture".into(),
        fill_assumption: "fixture".into(),
        initial_micro_usd: 20_000_000,
        realized_micro_usd: 7,
        final_marked_micro_usd: 8,
        maximum_drawdown_micro_usd: 9,
        trades: vec![],
        rejections: vec![],
        unresolved: None,
        pending_entry: None,
        entry_receipts: vec![],
        evaluation_window: None,
    }
}
fn trade(i: u64, entry: &str, exit: &str, net: i128) -> Trade {
    Trade {
        entry_event: i * 2,
        exit_event: i * 2 + 1,
        entry_ms: time(entry),
        exit_ms: time(exit),
        side: Side::Long,
        entry: 100,
        exit: 101,
        stop: 90,
        target: 130,
        volume_micro_lots: 1,
        net_micro_usd: net,
        commission_micro_usd: 123,
        reason: ExitReason::StopMarket,
        mfe_price_units: 1,
        mae_price_units: 0,
        crossed_gap_ms: 0,
    }
}
#[test]
fn training_breakdown_uses_exit_year_entry_bins_and_strict_loss_streaks() {
    let mut r = replay();
    r.trades = vec![
        trade(
            0,
            "2025-12-31T23:00:00Z",
            "2026-01-01T00:00:00Z",
            -3_000_000,
        ),
        trade(
            1,
            "2026-01-01T01:00:00Z",
            "2026-01-01T02:00:00Z",
            -4_000_000,
        ),
        trade(2, "2026-01-01T06:00:00Z", "2026-01-01T07:00:00Z", 0),
        trade(
            3,
            "2026-01-01T12:00:00Z",
            "2026-01-01T13:00:00Z",
            -5_000_000,
        ),
        trade(4, "2026-01-01T18:00:00Z", "2026-01-01T19:00:00Z", 2_000_000),
    ];
    let e = training_diagnostics::summarize(&r, &window()).unwrap();
    assert_eq!(
        (
            e.closed.resolved,
            e.closed.wins,
            e.closed.losses,
            e.closed.breakevens
        ),
        (5, 1, 3, 1)
    );
    assert_eq!(e.closed.sum_net_micro_usd, -10_000_000);
    assert_eq!(e.maximum_consecutive_losses, 2);
    assert_eq!(e.closed_commission_micro_usd, 615);
    assert_eq!(e.exit_years.len(), 1);
    assert_eq!(e.exit_years[&2026].resolved, 5);
    assert_eq!(e.entry_utc_six_hour_bins[&3].sum_net_micro_usd, -1_000_000);
    assert_eq!(e.exit_reasons["StopMarket"], 5);
    assert_eq!(
        (
            e.recorded_realized_cash_micro_usd,
            e.recorded_final_marked_micro_usd,
            e.recorded_maximum_drawdown_micro_usd
        ),
        (7, 8, 9)
    );
    assert_eq!(e.status, "INSUFFICIENT_EVIDENCE");
}
#[test]
fn empty_overflow_chronology_scope_and_reason_budgets_fail_explicitly() {
    let mut r = replay();
    let e = training_diagnostics::summarize(&r, &window()).unwrap();
    assert_eq!(e.closed.resolved, 0);
    assert!(e.evidence_limits.iter().any(|s| s.contains("undefined")));
    r.trades = vec![
        trade(0, "2026-01-01T01:00:00Z", "2026-01-01T02:00:00Z", i128::MAX),
        trade(1, "2026-01-01T03:00:00Z", "2026-01-01T04:00:00Z", 1),
    ];
    assert!(
        training_diagnostics::summarize(&r, &window())
            .unwrap_err()
            .contains("overflow")
    );
    r.trades[0].net_micro_usd = 0;
    r.trades[0].commission_micro_usd = i128::MAX;
    assert!(
        training_diagnostics::summarize(&r, &window())
            .unwrap_err()
            .contains("commission overflow")
    );
    r.trades[0].commission_micro_usd = 0;
    r.trades[1].entry_event = 0;
    assert!(training_diagnostics::summarize(&r, &window()).is_err());
    r.trades.truncate(1);
    let short = Interval {
        start_ms: time("2026-01-01T01:00:00Z"),
        end_exclusive_ms: time("2026-01-01T02:00:00Z"),
    };
    assert!(training_diagnostics::summarize(&r, &short).is_err());
    r.trades.clear();
    r.rejections = (0..65)
        .map(|i| Rejection {
            event: i,
            reason: format!("distinct {i}"),
        })
        .collect();
    assert!(
        training_diagnostics::summarize(&r, &window())
            .unwrap_err()
            .contains("reason budget")
    );
}
