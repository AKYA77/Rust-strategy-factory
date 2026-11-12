use rust_strategy_factory::{
    contracts::Side,
    diagnostics::{Breakdown, FoldRisk},
    tick_engine::{ExitReason, Trade},
};
fn time(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .unwrap()
        .timestamp_millis()
}
fn trade(entry: &str, exit: &str, net: i128) -> Trade {
    Trade {
        entry_event: 0,
        exit_event: 1,
        entry_ms: time(entry),
        exit_ms: time(exit),
        side: Side::Long,
        entry: 100,
        exit: 101,
        stop: 90,
        target: 130,
        volume_micro_lots: 1,
        net_micro_usd: net,
        commission_micro_usd: 0,
        reason: ExitReason::TargetLimit,
        mfe_price_units: 1,
        mae_price_units: 0,
        crossed_gap_ms: 0,
    }
}
#[test]
fn exact_year_session_breakdown_and_uncapped_realized_cash_drawdown() {
    let mut b = Breakdown::default();
    let mut f = FoldRisk::new("fold-0".into());
    for t in [
        trade("2025-12-31T23:00:00Z", "2026-01-01T00:00:00Z", 5_000_000),
        trade("2026-01-01T01:00:00Z", "2026-01-01T02:00:00Z", -7_000_000),
        trade("2026-01-01T06:00:00Z", "2026-01-01T07:00:00Z", -30_000_000),
        trade("2026-01-01T12:00:00Z", "2026-01-01T13:00:00Z", 2_000_000),
    ] {
        b.record(&t, &mut f).unwrap();
    }
    assert_eq!(f.ending_realized_cash_micro_usd, -10_000_000);
    assert_eq!(f.peak_realized_cash_micro_usd, 25_000_000);
    assert_eq!(f.maximum_realized_cash_drawdown_micro_usd, 37_000_000);
    assert!(f.negative_realized_cash);
    assert_eq!(b.exit_years[&2026].resolved, 4);
    assert!(!b.exit_years.contains_key(&2025));
    assert_eq!(b.entry_utc_six_hour_bins[&3].sum_net_micro_usd, 5_000_000);
    assert_eq!(b.entry_utc_six_hour_bins[&0].losses, 1);
    assert_eq!(
        b.entry_utc_six_hour_bins
            .values()
            .map(|s| s.sum_net_micro_usd)
            .sum::<i128>(),
        -30_000_000
    );
    let fresh = FoldRisk::new("fold-1".into());
    assert_eq!(fresh.ending_realized_cash_micro_usd, 20_000_000);
    assert_eq!(fresh.maximum_realized_cash_drawdown_micro_usd, 0);
    assert!(!fresh.replay_present);
}
#[test]
fn empty_breakeven_order_and_overflow_remain_explicit() {
    let mut b = Breakdown::default();
    assert!(b.exit_years.is_empty());
    let mut f = FoldRisk::new("fold-0".into());
    let mut t = trade("2025-01-01T00:00:00Z", "2025-01-01T01:00:00Z", 0);
    b.record(&t, &mut f).unwrap();
    assert_eq!(b.exit_years[&2025].breakevens, 1);
    assert!(b.record(&t, &mut f).is_err());
    t.entry_ms = t.exit_ms + 1;
    t.exit_ms = t.entry_ms + 1;
    t.net_micro_usd = i128::MAX;
    let before = serde_json::to_value((&b, &f)).unwrap();
    assert!(b.record(&t, &mut f).is_err());
    assert_eq!(before, serde_json::to_value((&b, &f)).unwrap());
}
