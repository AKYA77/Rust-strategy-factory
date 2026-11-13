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

#[test]
fn reservation_rejects_off_grid_prices_and_distance_without_rounding_a_request() {
    let mut p = Profile::synthetic("EURUSD").unwrap();
    p.tick = 5;
    let costs = Costs::default();
    let r = request();
    assert_eq!(
        risk::reserve(&p, &costs, r, 1000)
            .unwrap()
            .stop_distance_price_units,
        Some(50)
    );
    for invalid in [
        Request {
            entry: r.entry + 1,
            ..r
        },
        Request {
            mark: r.mark + 1,
            ..r
        },
        Request {
            stop_with_exit_slip: r.stop_with_exit_slip + 1,
            ..r
        },
    ] {
        assert!(risk::reserve(&p, &costs, invalid, 1000).is_err());
    }
}

#[test]
fn all_eight_profiles_match_independent_exhaustive_grid_sizing_oracle() {
    // Deliberately small volume domains allow exhaustive enumeration independent of
    // the production binary search and contracts::size upper bound.
    for symbol in rust_strategy_factory::contracts::SYMBOLS {
        let mut p = Profile::synthetic(symbol).unwrap();
        p.max_micro_lots = 1000;
        let fx = match p.quote.as_str() {
            "JPY" => Conversion {
                usd_num: 1,
                quote_den: 150,
                as_of_ms: 0,
            },
            "CHF" => Conversion {
                usd_num: 10,
                quote_den: 9,
                as_of_ms: 0,
            },
            "CAD" => Conversion {
                usd_num: 5,
                quote_den: 7,
                as_of_ms: 0,
            },
            _ => Conversion::usd(),
        };
        let entry = if symbol == "XAUUSD" {
            250000
        } else if symbol == "USDJPY" {
            150000
        } else {
            100000
        };
        for side in [Side::Long, Side::Short] {
            for equity in [1_000i128, 100_000, 1_000_000, 20_000_000] {
                for commission in [0i64, 333_333, 10_000_000] {
                    let costs = Costs {
                        commission_micro_usd_per_lot_per_fill: commission,
                        ..Costs::default()
                    };
                    let friction = 4 * p.pip;
                    let distance = 10 * p.pip;
                    let mark = entry - side.sign() * friction;
                    let request = Request {
                        equity,
                        entry,
                        mark,
                        side,
                        stop_with_exit_slip: distance + 2 * p.pip,
                        conversion: fx,
                    };
                    let mut expected = None;
                    for volume in
                        (p.min_micro_lots..=p.max_micro_lots).step_by(p.step_micro_lots as usize)
                    {
                        let ceil = |n: i128, d: i128| (n + d - 1) / d;
                        let denominator = i128::from(p.price_scale) * i128::from(fx.quote_den);
                        let factor = i128::from(p.contract_units)
                            * i128::from(volume)
                            * i128::from(fx.usd_num);
                        let fee = ceil(i128::from(commission) * i128::from(volume), 1_000_000);
                        let loss =
                            ceil(i128::from(distance + 2 * p.pip) * factor, denominator) + 2 * fee;
                        let marked =
                            equity - ceil(i128::from(friction) * factor, denominator) - fee;
                        let margin = ceil(
                            i128::from(entry) * factor,
                            denominator * i128::from(p.leverage),
                        );
                        if loss <= equity / 10 && margin <= marked {
                            expected = Some((volume, loss, marked, margin, fee));
                        }
                    }
                    let actual = risk::size(&p, &costs, request);
                    match expected {
                        None => assert!(actual.is_err(), "{symbol} {side:?} {equity} {commission}"),
                        Some((volume, loss, marked, margin, fee)) => {
                            let actual = actual.unwrap();
                            assert_eq!(
                                (
                                    actual.volume,
                                    actual.intended_loss_micro_usd,
                                    actual.equity_after_entry_mark_micro_usd,
                                    actual.initial_margin_micro_usd,
                                    actual.entry_fee_micro_usd
                                ),
                                (volume, loss, marked, margin, fee),
                                "{symbol} {side:?} {equity} {commission}"
                            );
                        }
                    }
                }
            }
        }
    }
}
