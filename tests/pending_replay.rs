use rust_strategy_factory::{
    candles::{self, Timeframe},
    contracts::{Conversion, Profile, Side},
    data::{self, Tick},
    strategies::{EntryPlan, Features, Rule, Strategy},
    tick_engine::{self, Costs, EntryStatus, ExitReason},
};
fn t(ms: i64, seq: u64, bid: i64, ask: i64) -> Tick {
    Tick {
        timestamp_ms: ms,
        sequence: seq,
        bid,
        ask,
    }
}
fn run(tail: Vec<Tick>, side: Side, entry: EntryPlan) -> tick_engine::Replay {
    let mut ticks = vec![
        t(0, 0, 100000, 100010),
        t(60000, 1, 100020, 100030),
        t(120000, 2, 100040, 100050),
    ];
    ticks.extend(tail);
    let d = data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        ticks,
        "SYNTHETIC_TEST_ONLY pending fixture",
        "generated".into(),
        true,
    )
    .unwrap();
    let s = Strategy {
        stop_policy: None,
        timeframe: Timeframe::M1,
        side,
        stop_ticks: 50,
        rule: Rule::AboveMean { window: 2 },
        entry: Some(entry),
    };
    let c = candles::build(&d, s.timeframe).unwrap();
    let m = Features::new(&c, 1000).unwrap().signal(&s.rule).unwrap();
    tick_engine::replay(
        &d,
        &c,
        &m,
        &s,
        &Costs::default(),
        &vec![Conversion::usd(); d.ticks.len()],
    )
    .unwrap()
}
fn stop() -> EntryPlan {
    EntryPlan::StopMarket {
        offset_ticks: 30,
        ttl_ms: 60000,
        cancel_on_signal_false: false,
    }
}
fn limit() -> EntryPlan {
    EntryPlan::Limit {
        offset_ticks: 30,
        ttl_ms: 60000,
        cancel_on_signal_false: false,
    }
}
#[test]
fn long_pending_stop_reserves_at_publication_then_trades_with_3r_accounting() {
    let r = run(
        vec![t(120000, 3, 100070, 100080), t(121000, 4, 100350, 100360)],
        Side::Long,
        stop(),
    );
    assert_eq!(r.trades.len(), 1);
    let trade = &r.trades[0];
    assert_eq!(
        (
            trade.entry_event,
            trade.entry,
            trade.stop,
            trade.target,
            trade.exit
        ),
        (3, 100110, 100060, 100260, 100260)
    );
    assert_eq!(trade.reason, ExitReason::TargetLimit);
    let statuses: Vec<_> = r.entry_receipts.iter().map(|r| r.status.clone()).collect();
    assert_eq!(statuses, vec![EntryStatus::Placed, EntryStatus::Filled]);
    assert_eq!(r.entry_receipts[0].signal_event, 2);
    assert_eq!(r.entry_receipts[0].event, 2);
    assert_eq!(
        trade.volume_micro_lots,
        r.entry_receipts[0].reservation.volume
    );
    assert_eq!(r.realized_micro_usd, 20_000_000 + trade.net_micro_usd);
}
#[test]
fn pending_limit_uses_observed_quote_without_entry_slippage() {
    let r = run(
        vec![t(120001, 3, 100000, 100010), t(120002, 4, 100200, 100210)],
        Side::Long,
        limit(),
    );
    assert_eq!(r.trades.len(), 1);
    assert_eq!(r.trades[0].entry, 100020);
    assert_eq!(r.trades[0].exit, 100170);
    assert_eq!(r.trades[0].reason, ExitReason::TargetLimit);
}
#[test]
fn short_stop_and_limit_use_bid_entry_ask_exit() {
    let r = run(
        vec![t(120001, 3, 100010, 100020), t(120002, 4, 99800, 99810)],
        Side::Short,
        stop(),
    );
    assert_eq!(
        (r.trades[0].entry, r.trades[0].target, r.trades[0].exit),
        (99990, 99840, 99840)
    );
    let r = run(
        vec![t(120001, 3, 100080, 100090), t(120002, 4, 99800, 99810)],
        Side::Short,
        limit(),
    );
    assert_eq!((r.trades[0].entry, r.trades[0].exit), (100080, 99930));
}
#[test]
fn no_quote_does_not_fill_and_deadline_precedes_trigger() {
    let r = run(vec![], Side::Long, stop());
    assert!(r.trades.is_empty());
    assert!(r.pending_entry.is_some());
    let r = run(vec![t(180000, 3, 100070, 100080)], Side::Long, stop());
    assert!(r.trades.is_empty());
    assert!(
        r.entry_receipts
            .iter()
            .any(|r| r.status == EntryStatus::Expired)
    );
}
#[test]
fn changed_closed_signal_cancels_surviving_intent_after_quote_processing() {
    let plan = EntryPlan::StopMarket {
        offset_ticks: 30,
        ttl_ms: 600000,
        cancel_on_signal_false: true,
    };
    let r = run(
        vec![t(120001, 3, 99000, 99010), t(180000, 4, 99010, 99020)],
        Side::Long,
        plan,
    );
    assert!(r.trades.is_empty());
    assert!(r.pending_entry.is_none());
    assert!(
        r.entry_receipts
            .iter()
            .any(|r| r.status == EntryStatus::Cancelled)
    );
}
#[test]
fn adverse_entry_gap_cannot_silently_resize_reserved_quantity() {
    let r = run(vec![t(120001, 3, 200000, 200010)], Side::Long, stop());
    assert!(r.unresolved.is_none());
    assert!(r.trades.is_empty());
    assert!(
        r.entry_receipts
            .iter()
            .any(|r| r.status == EntryStatus::RiskRejected)
    );
}
#[test]
fn legacy_market_identity_is_unchanged_and_entry_policy_is_not_deduplicated() {
    let old =
        r#"{"timeframe":"M1","side":"Long","stop_ticks":30,"rule":{"AboveMean":{"window":2}}}"#;
    let s: Strategy = serde_json::from_str(old).unwrap();
    #[derive(serde::Serialize)]
    struct Legacy {
        timeframe: Timeframe,
        side: Side,
        stop_ticks: i64,
        rule: Rule,
    }
    let legacy = Legacy {
        timeframe: s.timeframe,
        side: s.side,
        stop_ticks: s.stop_ticks,
        rule: s.rule.clone(),
    };
    assert_eq!(
        s.identity().unwrap(),
        data::hash(&("strategy-v1", legacy)).unwrap()
    );
    let mut changed = s.clone();
    changed.entry = Some(stop());
    assert_ne!(s.identity().unwrap(), changed.identity().unwrap());
    changed.entry = Some(EntryPlan::Limit {
        offset_ticks: 0,
        ttl_ms: 1,
        cancel_on_signal_false: false,
    });
    assert!(changed.canonical().is_err());
}

#[test]
fn cancellation_decided_from_publication_quote_cannot_undo_its_triggered_fill() {
    let plan = EntryPlan::StopMarket {
        offset_ticks: 30,
        ttl_ms: 600000,
        cancel_on_signal_false: true,
    };
    let r = run(
        vec![t(120001, 3, 99000, 99010), t(180000, 4, 100070, 100080)],
        Side::Long,
        plan,
    );
    assert_eq!(r.unresolved.unwrap().entry_event, 4);
    assert!(
        r.entry_receipts
            .iter()
            .any(|r| r.status == EntryStatus::Filled)
    );
    assert!(
        !r.entry_receipts
            .iter()
            .any(|r| r.status == EntryStatus::Cancelled)
    );
}
