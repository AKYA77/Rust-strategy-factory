use rust_strategy_factory::{
    candles::Timeframe,
    contracts::{Conversion, Profile, SYMBOLS, Side},
    data::Tick,
    risk::{self, Request},
    screen,
    strategies::{EntryPlan, Rule, Strategy},
    tick_engine::Costs,
};
#[test]
fn independent_necessary_condition_matches_reserve_for_all_profiles_sides_and_costs() {
    let mut cases = 0;
    for symbol in SYMBOLS {
        for side in [Side::Long, Side::Short] {
            for spread in [2, 4, 10] {
                let mut p = Profile::synthetic(symbol).unwrap();
                p.min_stop_ticks = 3;
                let costs = Costs {
                    spread_floor_pips: spread,
                    ..Default::default()
                };
                let required_ticks = 3 + (spread + 2) * p.pip / p.tick;
                let mut s = Strategy {
                    timeframe: Timeframe::M1,
                    side,
                    stop_ticks: required_ticks - 1,
                    rule: Rule::Rising { lookback: 1 },
                    entry: None,
                    stop_policy: None,
                };
                let proof = screen::market_stop_proof(&p, &costs, &s).unwrap().unwrap();
                assert_eq!(
                    proof.required_stop_price_units,
                    3 * p.tick + (spread + 2) * p.pip
                );
                screen::verify_market_stop_proof(&proof, &p, &costs, &s).unwrap();
                for observed_spread in [spread * p.pip, (spread + 5) * p.pip] {
                    let t = Tick {
                        timestamp_ms: 0,
                        sequence: 1,
                        bid: 1_000_000,
                        ask: 1_000_000 + observed_spread,
                    };
                    let (bid, ask) = costs.effective(&p, t).unwrap();
                    // Independent executable-side formula, not the proof implementation.
                    let entry = if side == Side::Long {
                        ask + 2 * p.pip
                    } else {
                        bid - 2 * p.pip
                    };
                    let mark = if side == Side::Long { bid } else { ask };
                    let request = Request {
                        equity: 20_000_000,
                        entry,
                        mark,
                        side,
                        stop_with_exit_slip: s.stop_ticks * p.tick + 2 * p.pip,
                        conversion: Conversion::usd(),
                    };
                    let err = risk::reserve(&p, &costs, request, p.min_micro_lots).unwrap_err();
                    assert!(err.contains("executable-side minimum distance"));
                    cases += 1;
                }
                // The necessary condition is strict: equality does not prove infeasibility.
                s.stop_ticks = required_ticks;
                assert!(screen::market_stop_proof(&p, &costs, &s).unwrap().is_none());
                assert!(screen::verify_market_stop_proof(&proof, &p, &costs, &s).is_err());
            }
        }
    }
    assert_eq!(cases, 96);
}
#[test]
fn proof_refuses_pending_adaptive_and_tampered_contexts() {
    let p = Profile::synthetic("EURUSD").unwrap();
    let costs = Costs::default();
    let mut s = Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        stop_ticks: 30,
        rule: Rule::Rising { lookback: 1 },
        entry: None,
        stop_policy: None,
    };
    let mut proof = screen::market_stop_proof(&p, &costs, &s).unwrap().unwrap();
    proof.fixed_stop_price_units += 1;
    assert!(screen::verify_market_stop_proof(&proof, &p, &costs, &s).is_err());
    s.entry = Some(EntryPlan::Limit {
        offset_ticks: 1,
        ttl_ms: 60000,
        cancel_on_signal_false: false,
    });
    assert!(screen::market_stop_proof(&p, &costs, &s).unwrap().is_none());
    s.entry = None;
    s.stop_policy = Some(rust_strategy_factory::stops::Policy::Atr {
        window: 5,
        multiple_bps: 10000,
    });
    assert!(screen::market_stop_proof(&p, &costs, &s).unwrap().is_none());
}

#[test]
fn proof_audits_are_deterministic_and_cannot_be_disabled() {
    use rust_strategy_factory::proof_screen::Policy;
    let policy = Policy::default();
    let mut sampled = 0;
    for i in 0..1000 {
        let id = i.to_string();
        let a = policy.audit(&id).unwrap();
        assert_eq!(a, policy.audit(&id).unwrap());
        sampled += usize::from(a);
    }
    assert!((70..130).contains(&sampled));
    for bps in [0, 10001] {
        assert!(
            Policy {
                audit_bps: bps,
                ..policy.clone()
            }
            .validate()
            .is_err()
        );
    }
    let all = Policy {
        audit_bps: 10000,
        ..policy
    };
    for i in 0..1000 {
        assert!(all.audit(&i.to_string()).unwrap());
    }
}
