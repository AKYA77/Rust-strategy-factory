use rust_strategy_factory::{
    calendar_evidence::{self, Declaration},
    catalog::Catalog,
    contracts::Profile,
    data::{self, Tick},
    validation::Interval,
};
use std::fs;
#[test]
fn exact_half_open_calendar_counts_match_point_oracle_and_bind_receipts() {
    let root = std::env::temp_dir().join(format!("calendar-evidence-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let d = data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        [0, 5, 5, 10, 15, 20]
            .iter()
            .enumerate()
            .map(|(i, &t)| Tick {
                timestamp_ms: t,
                sequence: u64::MAX - 5 + i as u64,
                bid: 100000,
                ask: 100010,
            })
            .collect(),
        "SYNTHETIC_TEST_ONLY calendar evidence",
        "fixture".into(),
        true,
    )
    .unwrap();
    let mut c = Catalog::open(&root.join("catalog.sqlite")).unwrap();
    let entry = c.register(&d).unwrap().entry;
    for end in [6, 11, 21] {
        let open = [
            Interval {
                start_ms: 1,
                end_exclusive_ms: 6,
            },
            Interval {
                start_ms: 7,
                end_exclusive_ms: 9,
            },
        ];
        let decl = Declaration {
            reference: "fixture declaration; not reviewed".into(),
            scope: Interval {
                start_ms: 0,
                end_exclusive_ms: end,
            },
            open: open
                .iter()
                .filter(|w| w.end_exclusive_ms <= end)
                .cloned()
                .collect(),
        };
        let expected = d
            .ticks
            .iter()
            .filter(|t| {
                t.timestamp_ms < end && decl.open.iter().any(|w| w.contains(t.timestamp_ms))
            })
            .count();
        let report = calendar_evidence::assess(&d, &entry, decl).unwrap();
        assert_eq!(report.evidence.observations_in_open, expected);
        assert_eq!(
            report.evidence.observations_in_open + report.evidence.observations_outside_open,
            report.evidence.observed.events
        );
        assert_eq!(report.evidence.catalog_hash, entry.hash);
        assert!(report.evidence.limitations.contains("unapproved"));
        if end == 21 {
            assert_eq!(
                report.evidence.outside_examples.last().unwrap().sequence,
                u64::MAX.to_string()
            );
            assert_eq!(report.evidence.unobserved_open_intervals, 1);
        }
    }
    let decl = Declaration {
        reference: "fixture".into(),
        scope: Interval {
            start_ms: 0,
            end_exclusive_ms: 21,
        },
        open: vec![
            Interval {
                start_ms: 0,
                end_exclusive_ms: 10,
            },
            Interval {
                start_ms: 5,
                end_exclusive_ms: 15,
            },
        ],
    };
    assert!(calendar_evidence::assess(&d, &entry, decl).is_err());
    let mut changed = d.clone();
    changed.manifest.source = "different provenance".into();
    assert!(
        calendar_evidence::assess(
            &changed,
            &entry,
            Declaration {
                reference: "fixture".into(),
                scope: Interval {
                    start_ms: 0,
                    end_exclusive_ms: 21
                },
                open: vec![]
            }
        )
        .is_err()
    );
    drop(c);
    fs::remove_dir_all(root).unwrap();
}
