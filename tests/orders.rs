use rust_strategy_factory::{
    contracts::{Profile, Side},
    data::Tick,
    orders::{Kind, Order, State},
    tick_engine::Costs,
};
fn t(ms: i64, seq: u64, bid: i64, ask: i64) -> Tick {
    Tick {
        timestamp_ms: ms,
        sequence: seq,
        bid,
        ask,
    }
}
#[test]
fn market_orders_wait_for_next_event_even_when_timestamps_tie() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let c = Costs::default();
    let submitted = t(100, 5, 100000, 100010);
    for (side, expected) in [(Side::Long, 100040), (Side::Short, 99980)] {
        let mut order = Order::submit(&p, &c, side, Kind::Market, 1000, submitted, 1000).unwrap();
        assert!(order.on_quote(&p, &c, submitted).unwrap().is_none());
        let event = t(100, 6, 100000, 100010);
        let fill = order.on_quote(&p, &c, event).unwrap().unwrap();
        assert_eq!(fill.price, expected);
        assert!(order.on_quote(&p, &c, event).unwrap().is_none());
        assert!(!order.cancel(100).unwrap());
    }
}
#[test]
fn stops_trigger_on_correct_quote_side_and_gap_fill_is_not_capped() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let c = Costs::default();
    let mut buy = Order::submit(
        &p,
        &c,
        Side::Long,
        Kind::StopMarket { trigger: 100100 },
        1000,
        t(0, 0, 100000, 100010),
        10000,
    )
    .unwrap();
    assert!(
        buy.on_quote(&p, &c, t(1, 1, 100050, 100060))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        buy.on_quote(&p, &c, t(2, 2, 100200, 100250))
            .unwrap()
            .unwrap()
            .price,
        100270
    );
    let mut sell = Order::submit(
        &p,
        &c,
        Side::Short,
        Kind::StopMarket { trigger: 99900 },
        1000,
        t(0, 0, 100000, 100010),
        10000,
    )
    .unwrap();
    assert!(
        sell.on_quote(&p, &c, t(1, 1, 99910, 99920))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        sell.on_quote(&p, &c, t(2, 2, 99800, 99820))
            .unwrap()
            .unwrap()
            .price,
        99780
    );
}
#[test]
fn limits_require_executable_quote_and_do_not_receive_market_slippage() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let c = Costs::default();
    let mut buy = Order::submit(
        &p,
        &c,
        Side::Long,
        Kind::Limit { limit: 100000 },
        1000,
        t(0, 0, 100100, 100120),
        10000,
    )
    .unwrap();
    // Recorded ask crosses, but the pessimistic spread floor still prevents eligibility.
    assert!(
        buy.on_quote(&p, &c, t(1, 1, 99990, 99995))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        buy.on_quote(&p, &c, t(2, 2, 99950, 99960))
            .unwrap()
            .unwrap()
            .price,
        99970
    );
    let mut sell = Order::submit(
        &p,
        &c,
        Side::Short,
        Kind::Limit { limit: 100100 },
        1000,
        t(0, 0, 100000, 100010),
        10000,
    )
    .unwrap();
    assert!(
        sell.on_quote(&p, &c, t(1, 1, 100090, 100200))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        sell.on_quote(&p, &c, t(2, 2, 100110, 100120))
            .unwrap()
            .unwrap()
            .price,
        100110
    );
}
#[test]
fn expiry_and_cancellation_never_fabricate_fills() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let c = Costs::default();
    let create = || {
        Order::submit(
            &p,
            &c,
            Side::Long,
            Kind::Market,
            1000,
            t(0, 0, 100000, 100010),
            100,
        )
        .unwrap()
    };
    let mut o = create();
    o.advance_clock(100).unwrap();
    assert!(matches!(o.state(), State::Expired { .. }));
    assert!(
        o.on_quote(&p, &c, t(100, 1, 100000, 100010))
            .unwrap()
            .is_none()
    );
    let mut o = create();
    assert!(o.cancel(99).unwrap());
    assert!(!o.cancel(100).unwrap());
    assert!(
        o.on_quote(&p, &c, t(100, 1, 100000, 100010))
            .unwrap()
            .is_none()
    );
    let mut o = create();
    assert!(
        o.on_quote(&p, &c, t(100, 1, 100000, 100010))
            .unwrap()
            .is_none()
    );
    assert!(matches!(o.state(), State::Expired { .. }));
}
#[test]
fn order_contracts_and_quote_chronology_are_immutable() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let c = Costs::default();
    let submitted = t(0, 0, 100000, 100010);
    assert!(Order::submit(&p, &c, Side::Long, Kind::Market, 1, submitted, 100).is_err());
    assert!(
        Order::submit(
            &p,
            &c,
            Side::Long,
            Kind::Limit { limit: 0 },
            1000,
            submitted,
            100
        )
        .is_err()
    );
    let mut order = Order::submit(
        &p,
        &c,
        Side::Long,
        Kind::StopMarket { trigger: 200000 },
        1000,
        submitted,
        100,
    )
    .unwrap();
    order.on_quote(&p, &c, t(2, 2, 100000, 100010)).unwrap();
    assert!(order.on_quote(&p, &c, t(1, 1, 100000, 100010)).is_err());
    let mut changed = c.clone();
    changed.slippage_pips = 3;
    assert!(
        order
            .on_quote(&p, &changed, t(3, 3, 100000, 100010))
            .is_err()
    );
    assert!(order.cancel(1).is_err());
}

#[test]
fn repeated_sequence_with_conflicting_prices_is_rejected() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let costs = Costs::default();
    let initial = t(0, 0, 100000, 100010);
    let mut order =
        Order::submit(&p, &costs, Side::Long, Kind::Market, 1000, initial, 100).unwrap();
    assert!(order.on_quote(&p, &costs, t(0, 0, 100001, 100011)).is_err());
}

#[test]
fn pending_and_filled_order_snapshots_resume_without_duplicate_fills() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let costs = Costs::default();
    let pending = Order::submit(
        &p,
        &costs,
        Side::Long,
        Kind::StopMarket { trigger: 100100 },
        1000,
        t(0, 0, 100000, 100010),
        100,
    )
    .unwrap();
    let mut restored: Order =
        serde_json::from_slice(&serde_json::to_vec(&pending).unwrap()).unwrap();
    let event = t(1, 1, 100100, 100110);
    let fill = restored.on_quote(&p, &costs, event).unwrap().unwrap();
    assert_eq!(fill.price, 100140);
    let mut restored: Order =
        serde_json::from_slice(&serde_json::to_vec(&restored).unwrap()).unwrap();
    assert!(restored.on_quote(&p, &costs, event).unwrap().is_none());
    assert!(
        restored
            .on_quote(&p, &costs, t(2, 2, 100110, 100120))
            .unwrap()
            .is_none()
    );
}
