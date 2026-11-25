//! Verification of Conversion logic for non-USD quotes.
//! Tests that usd_account_from_usd_base_price produces exact, event-aligned
//! conversions for USDJPY, USDCHF, USDCAD and that P&L/sizing work with them.

use rust_strategy_factory::contracts::{Conversion, Profile, pnl, size};

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
