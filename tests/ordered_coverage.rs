//! Ordered full-history coverage contract: first-chunk configs are pointers, not
//! complete instrument history; validation is fail-closed and scope-bounded.

use rust_strategy_factory::ordered_coverage::{MonthSpec, from_config};
use std::{fs, path::Path};

fn config(dir: &Path, name: &str, datasets: &[&str]) -> String {
    let path = dir.join(name);
    let bodies = datasets
        .iter()
        .map(|dataset| format!(r#"{{"dataset":"{dataset}","conversions":null}}"#))
        .collect::<Vec<_>>()
        .join(",");
    fs::write(&path, format!(r#"{{"limits":{{}},"inputs":[{bodies}]}}"#)).unwrap();
    path.display().to_string()
}

#[test]
fn month_spec_requires_one_of_the_seven_instruments() {
    let spec = MonthSpec::from_dataset("/corpus/full-100k/EURUSD-202501/chunk-000000.rsf").unwrap();
    assert_eq!(spec.instrument, "EURUSD");
    assert_eq!(spec.month, "202501");
    assert_eq!(spec.configured_chunk, "chunk-000000.rsf");
    assert!(spec.directory.ends_with("EURUSD-202501"));

    for bad in [
        "/corpus/full-100k/XAUUSD-202501/chunk-000000.rsf",
        "/corpus/full-100k/GBPJPY-202501/chunk-000000.rsf",
        "/corpus/full-100k/EURUSD-202513/chunk-000000.rsf",
        "/corpus/full-100k/EURUSD-2025/chunk-000000.rsf",
        "/corpus/full-100k/EURUSD-202501/history.json",
    ] {
        assert!(MonthSpec::from_dataset(bad).is_err(), "accepted {bad}");
    }
}

#[test]
fn ordered_coverage_refuses_duplicate_months_and_gold() {
    let dir = std::env::temp_dir().join(format!("rsf-ordered-coverage-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let duplicate = config(
        &dir,
        "duplicate.json",
        &[
            "/corpus/full-100k/EURUSD-202501/chunk-000000.rsf",
            "/corpus/full-100k/EURUSD-202501/chunk-000001.rsf",
        ],
    );
    let error = from_config(Path::new(&duplicate), "candidate", None).unwrap_err();
    assert!(error.contains("duplicate"), "{error}");

    let gold = config(
        &dir,
        "gold.json",
        &["/corpus/full-100k/XAUUSD-202501/chunk-000000.rsf"],
    );
    let error = from_config(Path::new(&gold), "candidate", None).unwrap_err();
    assert!(error.contains("XAUUSD"), "{error}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn ordered_coverage_rejects_bad_stage_and_month_limit() {
    let dir = std::env::temp_dir().join(format!(
        "rsf-ordered-coverage-limits-{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = config(
        &dir,
        "limits.json",
        &["/corpus/full-100k/EURUSD-202501/chunk-000000.rsf"],
    );
    assert!(from_config(Path::new(&path), "", None).is_err());
    assert!(from_config(Path::new(&path), "candidate", Some(0)).is_err());
    assert!(from_config(Path::new(&path), "candidate", Some(2)).is_err());
    fs::remove_dir_all(&dir).unwrap();
}
