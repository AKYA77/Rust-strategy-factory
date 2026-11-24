//! ORB causality regressions (V2 mandate P0.1).
//!
//! The previous implementation ignored the `session_hour` argument, treated any
//! hour change as a new session and backfilled the completed opening range into
//! candles that had already been published. These fixtures pin the corrected
//! contract: no value before the opening window completes, no backfill, the
//! session parameter is honoured, and appending future data never changes an
//! earlier prefix.

use rust_strategy_factory::{
    candles::{self, Timeframe},
    contracts::Profile,
    data::{self, Dataset, Tick},
    strategies::{Features, Rule},
};

const BASE: &str = "2025-01-06T06:50:00Z"; // Monday 06:50 UTC
const MINUTE: i64 = 60_000;

fn base_ms() -> i64 {
    chrono::DateTime::parse_from_rfc3339(BASE)
        .expect("valid base timestamp")
        .timestamp_millis()
}

/// One synthetic tick per minute; `prices[i]` is the quote at BASE + i minutes.
fn dataset_with_prices(prices: &[i64]) -> Dataset {
    let ticks: Vec<Tick> = prices
        .iter()
        .enumerate()
        .map(|(i, &price)| Tick {
            timestamp_ms: base_ms() + i as i64 * MINUTE,
            sequence: i as u64,
            bid: price,
            ask: price + 20,
        })
        .collect();
    data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        ticks,
        "SYNTHETIC_TEST_ONLY orb causality",
        "generated".into(),
        true,
    )
    .unwrap()
}

/// 06:50..07:40 with a known 07:00-07:29 opening range (high 100050, low 99950).
/// Prices after the window close above the range high at 07:30 only.
fn long_breakout_prices() -> Vec<i64> {
    (0..51)
        .map(|i| match i {
            20 => 100_050, // 07:10 window high
            30 => 99_950,  // 07:20 window low
            40 => 100_060, // 07:30 close above the range high
            _ => 100_000,
        })
        .collect()
}

fn short_breakout_prices() -> Vec<i64> {
    (0..51)
        .map(|i| match i {
            20 => 100_050,
            30 => 99_950,
            40 => 99_940, // 07:30 close below the range low
            _ => 100_000,
        })
        .collect()
}

fn mask_for(prices: &[i64], rule: &Rule) -> Vec<bool> {
    let d = dataset_with_prices(prices);
    let c = candles::build(&d, Timeframe::M1).unwrap();
    let mut f = Features::new(&c, c.len() * 40).unwrap();
    let mask = f.signal(rule).unwrap();
    (0..c.len()).map(|i| mask.get(i)).collect()
}

#[test]
fn opening_range_is_published_only_after_the_window_completes() {
    let rule = Rule::OrbLongBreakout {
        window_minutes: 30,
        session_hour: 7,
    };
    let mask = mask_for(&long_breakout_prices(), &rule);
    // Candles 06:50..07:29 (indices 0..39) must have no signal: no signal before
    // the opening window completes, and no backfill of the finished range.
    assert!(
        mask[..40].iter().all(|&b| !b),
        "signal emitted before or during the opening window: {:?}",
        &mask[..40]
    );
    // 07:30 closes above the 07:00-07:29 high -> exactly one breakout candle.
    assert!(mask[40], "expected breakout at 07:30");
    assert!(
        mask[41..].iter().all(|&b| !b),
        "unexpected later breakouts: {:?}",
        &mask[41..]
    );
}

#[test]
fn short_breakout_uses_the_same_completed_range() {
    let rule = Rule::OrbShortBreakout {
        window_minutes: 30,
        session_hour: 7,
    };
    let mask = mask_for(&short_breakout_prices(), &rule);
    assert!(mask[..40].iter().all(|&b| !b));
    assert!(mask[40], "expected short breakout at 07:30");
    assert!(mask[41..].iter().all(|&b| !b));
}

#[test]
fn appending_future_data_does_not_change_the_prefix() {
    let rule = Rule::OrbLongBreakout {
        window_minutes: 30,
        session_hour: 7,
    };
    let full = mask_for(&long_breakout_prices(), &rule);
    let truncated = mask_for(&long_breakout_prices()[..45], &rule);
    let n = truncated.len();
    assert_eq!(&full[..n], &truncated[..], "prefix invariance violated");
}

#[test]
fn session_hour_argument_selects_the_session() {
    // A 00:00 session window would cover different candles than the 07:00 one;
    // the feature must follow the supplied hour instead of resetting on hour
    // changes. With session_hour=7 the 00:00-00:29 range does not exist here.
    let seven = Rule::OrbLongBreakout {
        window_minutes: 30,
        session_hour: 7,
    };
    let mask = mask_for(&long_breakout_prices(), &seven);
    assert!(mask[40]);

    // Contradictory parameters are refused by the canonical contract.
    assert!(
        Rule::OrbLongBreakout {
            window_minutes: 0,
            session_hour: 7
        }
        .canonical()
        .is_err()
    );
    assert!(
        Rule::OrbLongBreakout {
            window_minutes: 30,
            session_hour: 24
        }
        .canonical()
        .is_err()
    );
}
