use rust_strategy_factory::{
    contracts::Profile,
    coverage,
    data::{self, Dataset, Tick},
    validation::Interval,
};
fn ms(text: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(text)
        .unwrap()
        .timestamp_millis()
}
fn fixture() -> Dataset {
    let times = [
        "2025-01-31T23:59:00Z",
        "2025-02-01T00:00:00Z",
        "2025-02-01T00:00:00Z",
        "2025-04-01T12:00:00Z",
        "2025-04-02T00:00:00Z",
    ];
    data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        times
            .iter()
            .enumerate()
            .map(|(i, t)| Tick {
                timestamp_ms: ms(t),
                sequence: u64::MAX - 4 + i as u64,
                bid: 100000,
                ask: 100010,
            })
            .collect(),
        "SYNTHETIC_TEST_ONLY coverage fixture",
        "fixture-not-acquired".into(),
        true,
    )
    .unwrap()
}
#[test]
fn observed_partial_months_empty_month_ties_and_exact_latest_sequence() {
    let d = fixture();
    let scope = Interval {
        start_ms: ms("2025-01-31T23:59:00Z"),
        end_exclusive_ms: ms("2025-04-02T00:00:00Z"),
    };
    let r = coverage::report(&d, scope.clone(), 60000, 1).unwrap();
    assert_eq!(r.events, 4);
    assert_eq!(r.months.len(), 4);
    assert_eq!(
        r.months.iter().map(|m| m.events).collect::<Vec<_>>(),
        vec![1, 2, 0, 1]
    );
    assert_eq!(r.months[0].requested.start_ms, scope.start_ms);
    assert_eq!(
        r.months[3].requested.end_exclusive_ms,
        scope.end_exclusive_ms
    );
    assert_eq!(
        r.months[1].first.as_ref().unwrap().sequence,
        (u64::MAX - 3).to_string()
    );
    assert_eq!(r.latest.unwrap().sequence, (u64::MAX - 1).to_string());
    assert!(r.months[2].first.is_none());
    assert_eq!(r.gap_count, 1);
    assert_eq!(r.gaps.len(), 1);
    assert!(!r.gap_details_truncated);
    assert_eq!(r.gaps[0].previous.sequence, (u64::MAX - 2).to_string());
    assert_eq!(r.gaps[0].next.sequence, (u64::MAX - 1).to_string());
    assert!(r.limitations.contains("do not prove missing"));
    assert_eq!(r.manifest.class, data::DataClass::SyntheticTestOnly);
}
#[test]
fn scoped_counts_and_gaps_match_independent_filtered_oracle() {
    let d = fixture();
    let low = d.ticks[0].timestamp_ms;
    let high = d.ticks[4].timestamp_ms + 1;
    let boundaries = [
        low - 1,
        low,
        low + 1,
        d.ticks[1].timestamp_ms,
        d.ticks[1].timestamp_ms + 1,
        d.ticks[3].timestamp_ms,
        high,
    ];
    for &a in &boundaries {
        for &b in &boundaries {
            if a >= b {
                continue;
            }
            for threshold in [1, 60000, 86400000] {
                for limit in [0, 1, 1000] {
                    let r = coverage::report(
                        &d,
                        Interval {
                            start_ms: a,
                            end_exclusive_ms: b,
                        },
                        threshold,
                        limit,
                    )
                    .unwrap();
                    let expected = d
                        .ticks
                        .iter()
                        .filter(|t| a <= t.timestamp_ms && t.timestamp_ms < b)
                        .collect::<Vec<_>>();
                    let count = expected
                        .windows(2)
                        .filter(|p| p[1].timestamp_ms - p[0].timestamp_ms > threshold)
                        .count();
                    assert_eq!(r.events, expected.len());
                    assert_eq!(
                        r.months.iter().map(|m| m.events).sum::<usize>(),
                        expected.len()
                    );
                    assert_eq!(r.gap_count, count);
                    assert_eq!(r.gaps.len(), count.min(limit));
                    assert_eq!(r.gap_details_truncated, count > limit);
                    assert_eq!(
                        r.latest.as_ref().map(|t| t.sequence.clone()),
                        expected.last().map(|t| t.sequence.to_string())
                    );
                }
            }
        }
    }
}
#[test]
fn empty_scope_bounds_tamper_and_calendar_refusals() {
    let mut d = fixture();
    let empty = Interval {
        start_ms: ms("2025-03-01T00:00:00Z"),
        end_exclusive_ms: ms("2025-04-01T00:00:00Z"),
    };
    let r = coverage::report(&d, empty.clone(), 1, 0).unwrap();
    assert_eq!(r.events, 0);
    assert!(r.first.is_none());
    assert!(r.latest.is_none());
    assert_eq!(r.months.len(), 1);
    assert_eq!(r.gap_count, 0);
    assert!(coverage::report(&d, empty.clone(), 0, 1).is_err());
    assert!(coverage::report(&d, empty.clone(), 1, 1001).is_err());
    assert!(
        coverage::report(
            &d,
            Interval {
                start_ms: ms("2000-01-01T00:00:00Z"),
                end_exclusive_ms: ms("2010-02-01T00:00:00Z")
            },
            1,
            0
        )
        .is_err()
    );
    assert!(
        coverage::report(
            &d,
            Interval {
                start_ms: 1,
                end_exclusive_ms: 1
            },
            1,
            0
        )
        .is_err()
    );
    d.ticks[0].bid += 1;
    assert!(coverage::report(&d, empty, 1, 0).is_err());
}
