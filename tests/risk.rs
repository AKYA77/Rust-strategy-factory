use rust_strategy_factory::{
    contracts::{Conversion, Profile, Side},
    risk::{self, Request},
    tick_engine::Costs,
};
fn request() -> Request {
    Request {
        equity: 20_000_000,
        entry: 100090,
        mark: 100050,
        side: Side::Long,
        stop_with_exit_slip: 70,
        conversion: Conversion::usd(),
    }
}
#[test]
fn initial_margin_includes_entry_mark_and_largest_feasible_grid_volume() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let costs = Costs::default();
    let r = risk::size(&p, &costs, request()).unwrap();
    // Each micro-lot consumes 1000.9 margin units plus 40 mark-loss units.
    // floor(20,000,000 / 1040.9 / 10)*10 = 19,210 micro-lots.
    assert_eq!(r.volume, 19210);
    assert_eq!(r.initial_margin_micro_usd, 19_227_289);
    assert_eq!(r.equity_after_entry_mark_micro_usd, 19_231_600);
    assert!(risk::reserve(&p, &costs, request(), 19220).is_err());
    assert!(risk::reserve(&p, &costs, request(), 19980).is_err());
}
#[test]
fn commissions_reduce_size_instead_of_rejecting_a_smaller_feasible_order() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let costs = Costs {
        commission_micro_usd_per_lot_per_fill: 10_000_000,
        ..Costs::default()
    };
    let r = risk::size(&p, &costs, request()).unwrap();
    assert!(r.volume < 19210);
    assert!(r.intended_loss_micro_usd <= 2_000_000);
    assert!(risk::reserve(&p, &costs, request(), r.volume + p.step_micro_lots).is_err());
}
#[test]
fn minimum_volume_is_not_mistaken_for_a_nonmonotone_binary_search() {
    let mut p = Profile::synthetic("EURUSD").unwrap();
    p.min_micro_lots = 10000;
    p.step_micro_lots = 10000;
    assert_eq!(
        risk::size(&p, &Costs::default(), request()).unwrap().volume,
        10000
    );
    p.min_micro_lots = 20000;
    assert!(risk::size(&p, &Costs::default(), request()).is_err());
}
#[test]
fn gold_and_jpy_margin_use_distinct_contract_scales_and_conversion() {
    let gold = Profile::synthetic("XAUUSD").unwrap();
    assert_eq!(
        risk::margin(&gold, 250000, 1000, Conversion::usd()).unwrap(),
        2_500_000
    );
    let jpy = Profile::synthetic("USDJPY").unwrap();
    let fx = Conversion {
        usd_num: 1,
        quote_den: 150,
        as_of_ms: 0,
    };
    assert_eq!(risk::margin(&jpy, 150000, 10000, fx).unwrap(), 10_000_000);
    assert!(risk::margin(&jpy, 150000, 10000, Conversion { quote_den: 0, ..fx }).is_err());
}

#[test]
fn protective_distance_is_measured_from_executable_mark_including_minimum_grid() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let mut r = request();
    r.stop_with_exit_slip = 60;
    assert!(risk::reserve(&p, &Costs::default(), r, 1000).is_err());
    r.stop_with_exit_slip = 61;
    assert!(risk::reserve(&p, &Costs::default(), r, 1000).is_ok());
    let mut stricter = p;
    stricter.min_stop_ticks = 11;
    assert!(risk::reserve(&stricter, &Costs::default(), r, 1000).is_err());
}
