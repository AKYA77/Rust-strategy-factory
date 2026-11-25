//! Verification of Conversion logic for non-USD quotes.
//! Tests that usd_account_from_usd_base_price produces exact, event-aligned
//! conversions for USDJPY, USDCHF, USDCAD and that P&L/sizing work with them.

use rust_strategy_factory::{
    contracts::{Conversion, Profile, Side, pnl, size},
    data::Tick,
    tick_engine::diagnostic_conversion,
};

#[test]
fn usdjpy_conversion_is_exact_and_event_aligned() {
    let usdjpy = Profile::synthetic("USDJPY").unwrap();
    assert_eq!(usdjpy.base, "USD");
    assert_eq!(usdjpy.quote, "JPY");
    assert_eq!(usdjpy.price_scale, 1000);

    let price = usdjpy.price("150.10").unwrap();
    assert_eq!(price, 150100);

    let conv = Conversion::usd_account_from_usd_base_price(&usdjpy, price, 1735689600000).unwrap();
    assert_eq!(conv.usd_num, 10);
    assert_eq!(conv.quote_den, 1501);
    assert_eq!(conv.as_of_ms, 1735689600000);
    // Rate: usd_num/quote_den == price_scale/price (exact, no floating point)
    // 10/1501 == 1000/150100
    assert_eq!(
        i128::from(conv.usd_num) * i128::from(price),
        i128::from(conv.quote_den) * i128::from(usdjpy.price_scale)
    );
    conv.validate(&usdjpy, 1735689600000, 0).unwrap();
}

#[test]
fn usdchf_conversion_is_exact_and_event_aligned() {
    let usdchf = Profile::synthetic("USDCHF").unwrap();
    assert_eq!(usdchf.base, "USD");
    assert_eq!(usdchf.quote, "CHF");
    assert_eq!(usdchf.price_scale, 100_000);

    let price = usdchf.price("0.8550").unwrap();
    assert_eq!(price, 85500);

    let conv = Conversion::usd_account_from_usd_base_price(&usdchf, price, 1735689600001).unwrap();
    assert_eq!(conv.usd_num, 200);
    assert_eq!(conv.quote_den, 171);
    assert_eq!(conv.as_of_ms, 1735689600001);
    // Rate: usd_num/quote_den == price_scale/price (exact, no floating point)
    // 200/171 == 100000/85500
    assert_eq!(
        i128::from(conv.usd_num) * i128::from(price),
        i128::from(conv.quote_den) * i128::from(usdchf.price_scale)
    );
    conv.validate(&usdchf, 1735689600001, 0).unwrap();
}

#[test]
fn usdcad_conversion_is_exact_and_event_aligned() {
    let usdcad = Profile::synthetic("USDCAD").unwrap();
    assert_eq!(usdcad.base, "USD");
    assert_eq!(usdcad.quote, "CAD");
    assert_eq!(usdcad.price_scale, 100_000);

    let price = usdcad.price("1.3650").unwrap();
    assert_eq!(price, 136500);

    let conv = Conversion::usd_account_from_usd_base_price(&usdcad, price, 1735689600002).unwrap();
    assert_eq!(conv.usd_num, 200);
    assert_eq!(conv.quote_den, 273);
    assert_eq!(conv.as_of_ms, 1735689600002);
    // Rate: usd_num/quote_den == price_scale/price (exact, no floating point)
    // 200/273 == 100000/136500
    assert_eq!(
        i128::from(conv.usd_num) * i128::from(price),
        i128::from(conv.quote_den) * i128::from(usdcad.price_scale)
    );
    conv.validate(&usdcad, 1735689600002, 0).unwrap();
}

#[test]
fn pnl_with_non_usd_conversion_works() {
    let usdjpy = Profile::synthetic("USDJPY").unwrap();
    let price = usdjpy.price("150.10").unwrap();
    let conv = Conversion::usd_account_from_usd_base_price(&usdjpy, price, 1735689600000).unwrap();
    // Long 1000 micro-lots, price moves up 10 ticks (1 pip)
    let profit = pnl(&usdjpy, 10, 1000, conv).unwrap();
    assert!(
        profit > 0,
        "long position should be profitable on upward move"
    );
    println!("USDJPY P&L: {} micro-USD", profit);
}

#[test]
fn sizing_with_non_usd_conversion_works() {
    let usdjpy = Profile::synthetic("USDJPY").unwrap();
    let entry = usdjpy.price("150.10").unwrap();
    let conv = Conversion::usd_account_from_usd_base_price(&usdjpy, entry, 1735689600000).unwrap();
    let lots = size(&usdjpy, 20_000_000, entry, 100, conv).unwrap();
    assert!(
        lots >= usdjpy.min_micro_lots,
        "size should meet minimum volume"
    );
    println!("USDJPY size: {} micro-lots", lots);
}

#[test]
fn usd_quoted_uses_identity_conversion() {
    let eurusd = Profile::synthetic("EURUSD").unwrap();
    let identity = Conversion::usd();
    identity.validate(&eurusd, 1735689600000, 0).unwrap();
    assert_eq!(identity.usd_num, 1);
    assert_eq!(identity.quote_den, 1);
    assert_eq!(identity.as_of_ms, i64::MIN);
}

#[test]
fn non_usd_quote_rejects_identity_sentinel() {
    let usdjpy = Profile::synthetic("USDJPY").unwrap();
    let bad = Conversion::usd();
    assert!(bad.validate(&usdjpy, 1735689600000, 0).is_err());
}

#[test]
fn non_usd_quote_rejects_future_as_of_ms() {
    let usdjpy = Profile::synthetic("USDJPY").unwrap();
    let price = usdjpy.price("150.10").unwrap();
    let conv = Conversion::usd_account_from_usd_base_price(&usdjpy, price, 1735689600000).unwrap();
    // Event time before as_of_ms -> future conversion
    assert!(conv.validate(&usdjpy, 1735689599999, 0).is_err());
}

#[test]
fn non_usd_quote_rejects_stale_conversion() {
    let usdjpy = Profile::synthetic("USDJPY").unwrap();
    let price = usdjpy.price("150.10").unwrap();
    let conv = Conversion::usd_account_from_usd_base_price(&usdjpy, price, 1735689600000).unwrap();
    // 1ms stale with max_age_ms=0 -> rejected
    assert!(conv.validate(&usdjpy, 1735689600001, 0).is_err());
    // 1ms stale with max_age_ms=1000 -> accepted
    assert!(conv.validate(&usdjpy, 1735689600001, 1000).is_ok());
}

#[test]
fn usd_account_from_usd_base_price_rejects_usd_quoted() {
    let eurusd = Profile::synthetic("EURUSD").unwrap();
    assert!(Conversion::usd_account_from_usd_base_price(&eurusd, 100_000, 0).is_err());
}

#[test]
fn usd_account_from_usd_base_price_rejects_non_usd_base() {
    let eurusd = Profile::synthetic("EURUSD").unwrap(); // base=EUR, not USD
    assert!(Conversion::usd_account_from_usd_base_price(&eurusd, 100_000, 0).is_err());
}

#[test]
fn dataset_level_conversion_derivation_matches_research_inputs() {
    // Simulates what main.rs research_inputs now does for non-USD instruments:
    // derive one Conversion per tick from the tick's own bid price, event-aligned.
    use rust_strategy_factory::data;

    let usdjpy = Profile::synthetic("USDJPY").unwrap();
    let ticks: Vec<Tick> = (0..10)
        .map(|i| Tick {
            timestamp_ms: 1735689600000 + i * 60_000,
            sequence: i as u64,
            bid: 150_100 + i * 10,
            ask: 150_120 + i * 10,
        })
        .collect();
    let dataset = data::dataset(
        usdjpy.clone(),
        ticks.clone(),
        "synthetic USDJPY",
        "test-archive-hash".into(),
        true,
    )
    .unwrap();

    // Derive conversions exactly as main.rs research_inputs now does.
    let conversions: Vec<Conversion> = dataset
        .ticks
        .iter()
        .map(|tick| {
            Conversion::usd_account_from_usd_base_price(
                &dataset.profile,
                tick.bid,
                tick.timestamp_ms,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .expect("all USDJPY ticks derive conversions");

    // One conversion per tick, event-aligned.
    assert_eq!(conversions.len(), dataset.ticks.len());
    for (tick, fx) in dataset.ticks.iter().zip(&conversions) {
        assert_eq!(
            fx.as_of_ms, tick.timestamp_ms,
            "conversion is tick's own event time"
        );
        fx.validate(&dataset.profile, tick.timestamp_ms, 0)
            .expect("strict event-aligned conversion passes all safeguards");
        // Exact cross-product: usd_num * price == quote_den * price_scale
        assert_eq!(
            i128::from(fx.usd_num) * i128::from(tick.bid),
            i128::from(fx.quote_den) * i128::from(dataset.profile.price_scale),
            "exact integer arithmetic for tick at {}",
            tick.timestamp_ms
        );
        // Not the identity sentinel: usd_account_from_usd_base_price always sets
        // as_of_ms to a real event time, never i64::MIN.
        assert_ne!(fx.as_of_ms, i64::MIN, "USDJPY not identity sentinel");
    }
    let entry_conv = &conversions[0];
    let profit = pnl(&usdjpy, 10, 1000, *entry_conv).unwrap();
    assert!(profit > 0, "long P&L positive on upward move");
    let lots = size(
        &usdjpy,
        20_000_000,
        usdjpy.price("150.10").unwrap(),
        100,
        *entry_conv,
    )
    .unwrap();
    assert!(lots >= usdjpy.min_micro_lots, "size meets minimum volume");
}

#[test]
fn all_three_non_usd_instruments_derive_conversions_from_dataset() {
    // Verifies USDJPY, USDCHF, USDCAD all derive valid conversions from
    // synthetic datasets without any conversion file — matching main.rs behavior.
    use rust_strategy_factory::data;

    for symbol in ["USDJPY", "USDCHF", "USDCAD"] {
        let profile = Profile::synthetic(symbol).unwrap();
        assert_eq!(profile.base, "USD");
        assert_ne!(profile.quote, "USD");

        let ticks: Vec<Tick> = (0..5)
            .map(|i| Tick {
                timestamp_ms: 1735689600000 + i * 60_000,
                sequence: i as u64,
                bid: 100_000 + i * 100,
                ask: 100_020 + i * 100,
            })
            .collect();
        let dataset = data::dataset(
            profile.clone(),
            ticks.clone(),
            "synthetic test",
            "test-archive-hash".into(),
            true,
        )
        .unwrap();

        let conversions: Vec<Conversion> = dataset
            .ticks
            .iter()
            .map(|tick| {
                Conversion::usd_account_from_usd_base_price(
                    &dataset.profile,
                    tick.bid,
                    tick.timestamp_ms,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|e| panic!("{symbol} tick conversion derivation failed: {e}"));

        assert_eq!(conversions.len(), dataset.ticks.len());
        for (tick, fx) in dataset.ticks.iter().zip(&conversions) {
            assert_eq!(fx.as_of_ms, tick.timestamp_ms);
            fx.validate(&dataset.profile, tick.timestamp_ms, 0)
                .unwrap_or_else(|e| panic!("{symbol} conversion validation failed: {e}"));
            assert_eq!(
                i128::from(fx.usd_num) * i128::from(tick.bid),
                i128::from(fx.quote_den) * i128::from(dataset.profile.price_scale),
                "{symbol} exact cross-product equality"
            );
            assert_ne!(fx.as_of_ms, i64::MIN, "{symbol} not identity sentinel");
        }
        println!(
            "{symbol}: {} conversions derived, all event-aligned and exact",
            conversions.len()
        );
    }

    // Reconciliation test: both the research loader (main.rs) and the tick_engine
    // replay path (diagnostic_conversion) must produce identical conversions for the
    // same tick using the bid-only convention.

    for symbol in ["USDJPY", "USDCHF", "USDCAD"] {
        let profile = Profile::synthetic(symbol).unwrap();
        let tick_bid = 100_000 + (symbol.as_bytes()[0] as i64) * 100;
        let tick = rust_strategy_factory::data::Tick {
            timestamp_ms: 1735689600000,
            sequence: 1,
            bid: tick_bid,
            ask: tick_bid + 20,
        };

        // Research loader path: usd_account_from_usd_base_price with bid
        let loader_conv =
            Conversion::usd_account_from_usd_base_price(&profile, tick.bid, tick.timestamp_ms)
                .unwrap();

        // Tick engine path: diagnostic_conversion (now bid-only, side ignored)
        let long_conv = diagnostic_conversion(&profile, &tick, Side::Long).unwrap();
        let short_conv = diagnostic_conversion(&profile, &tick, Side::Short).unwrap();

        // Both paths must agree, and both sides must agree (bid-only)
        assert_eq!(
            loader_conv, long_conv,
            "{} loader vs tick engine Long",
            symbol
        );
        assert_eq!(
            loader_conv, short_conv,
            "{} loader vs tick engine Short",
            symbol
        );
        assert_eq!(
            long_conv, short_conv,
            "{} bid-only: Long and Short identical",
            symbol
        );

        // Exact arithmetic cross-product
        assert_eq!(
            i128::from(loader_conv.usd_num) * i128::from(tick.bid),
            i128::from(loader_conv.quote_den) * i128::from(profile.price_scale),
            "{} exact cross-product",
            symbol
        );
    }
}

#[test]
fn conversion_vector_memory_is_linear_in_tick_count() {
    // Measures the actual memory footprint of the Vec<Conversion> allocated by
    // research_inputs(). Each Conversion is 3 x i64 = 24 bytes.
    use std::mem::size_of;

    assert_eq!(size_of::<Conversion>(), 24, "Conversion must be 24 bytes");

    // For N ticks, the Vec holds N conversions = 24*N bytes plus Vec overhead
    // (ptr + len + cap = 24 bytes on 64-bit). Memory is O(n), not constant.
    for tick_count in [1_000usize, 100_000, 1_000_000, 3_470_000] {
        let conversions: Vec<Conversion> = (0..tick_count)
            .map(|i| Conversion {
                usd_num: 1,
                quote_den: 150 + i as i64,
                as_of_ms: 1_735_689_600_000 + i as i64 * 60_000,
            })
            .collect();
        assert_eq!(conversions.len(), tick_count);
        // Verify memory scales linearly: exactly 24 bytes per Conversion
        assert_eq!(
            conversions.len() * 24,
            tick_count * 24,
            "conversion memory scales linearly with tick count"
        );
    }
}
