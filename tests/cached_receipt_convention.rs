//! Regression guard for the bid-only conversion convention on retained diagnostic evidence.
//!
//! The cached full-corpus diagnostic reports were produced by a LONG-ONLY strategy
//! (`configs/historical-migration/strategies/m1-long-above-mean-v1.json`). For Long fills the
//! superseded direction-based rule (Long -> bid, Short -> ask) and the active bid-only rule
//! apply the identical bid conversion, so relabelling those receipts from
//! `diagnostic_reciprocal_closing_rate_v1_long_bid_short_ask` to
//! `diagnostic_reciprocal_closing_rate_v1_bid_only` changes no numeric result and no receipt
//! hash. This test pins that claim: the declared labels must match the code's active policy,
//! and the recorded strategy must remain long-only. A stale label or a two-sided cached
//! strategy fails closed. All figures remain `diagnostic_simulation` / `none`; nothing here
//! grants eligibility or scientific qualification.

use rust_strategy_factory::contracts::Profile;
use rust_strategy_factory::historical_corpus::{
    CONVERSION_POLICY_IDENTITY, CONVERSION_POLICY_RECIPROCAL, conversion_policy,
};
use std::path::PathBuf;

const USD_QUOTED: [&str; 4] = ["EURUSD", "GBPUSD", "AUDUSD", "NZDUSD"];
const NON_USD_QUOTE: [&str; 3] = ["USDJPY", "USDCHF", "USDCAD"];

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn assert_report_labels(instrument: &str, variant: &str, expected: &str) {
    let relative =
        format!("docs/historical-migration/diagnostic-cache-v1/{instrument}.{variant}.json");
    let path = repo_path(&relative);
    let bytes =
        std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let report: serde_json::Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("parse {}: {error}", relative));

    assert_eq!(
        report["instrument"].as_str(),
        Some(instrument),
        "{relative}"
    );
    assert_eq!(
        report["classification"].as_str(),
        Some("diagnostic_simulation"),
        "{relative}"
    );
    assert_eq!(report["eligibility"].as_str(), Some("none"), "{relative}");
    assert_eq!(
        report["conversion_policy"].as_str(),
        Some(expected),
        "top-level policy in {relative}"
    );

    let expected_from_code = conversion_policy(&Profile::synthetic(instrument).unwrap());
    assert_eq!(
        expected, expected_from_code,
        "active code policy for {instrument}"
    );

    let months = report["months"].as_array().expect("months array");
    assert!(!months.is_empty(), "{relative} has no months");
    for month in months {
        assert_eq!(
            month["conversion_policy"].as_str(),
            Some(expected),
            "month policy in {relative}"
        );
        assert_eq!(
            month["classification"].as_str(),
            Some("diagnostic_simulation")
        );
        assert_eq!(month["eligibility"].as_str(), Some("none"));
    }
}

#[test]
fn active_conversion_policy_constants_are_bid_only() {
    assert_eq!(CONVERSION_POLICY_IDENTITY, "identity_usd_v1");
    assert_eq!(
        CONVERSION_POLICY_RECIPROCAL,
        "diagnostic_reciprocal_closing_rate_v1_bid_only"
    );
    for symbol in USD_QUOTED {
        assert_eq!(
            conversion_policy(&Profile::synthetic(symbol).unwrap()),
            CONVERSION_POLICY_IDENTITY,
            "{symbol} is USD-quoted"
        );
    }
    for symbol in NON_USD_QUOTE {
        assert_eq!(
            conversion_policy(&Profile::synthetic(symbol).unwrap()),
            CONVERSION_POLICY_RECIPROCAL,
            "{symbol} is USD-base non-USD-quote"
        );
    }
}

#[test]
fn cached_diagnostic_receipt_labels_match_active_convention() {
    for symbol in USD_QUOTED {
        for variant in ["cold", "warm"] {
            assert_report_labels(symbol, variant, CONVERSION_POLICY_IDENTITY);
        }
    }
    for symbol in NON_USD_QUOTE {
        for variant in ["cold", "warm"] {
            assert_report_labels(symbol, variant, CONVERSION_POLICY_RECIPROCAL);
        }
    }
}

#[test]
fn cached_diagnostic_execution_is_long_only_so_relabel_preserves_numbers() {
    let path = repo_path("configs/historical-migration/strategies/m1-long-above-mean-v1.json");
    let bytes =
        std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let config: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        config["side"].as_str(),
        Some("Long"),
        "cached diagnostic execution must remain long-only: a Short side would make the \
         direction-based and bid-only conventions produce different numbers, so the label \
         alone could not preserve numeric validity"
    );
}
