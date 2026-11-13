//! Component-only comparison, never a candidate/tick research throughput metric.
use rust_strategy_factory::{Result, data, indicators::Mask, research};
use std::{hint::black_box, time::Instant};
fn scalar(a: &Mask, b: &Mask, count: usize) -> Result<bool> {
    a.validate()?;
    b.validate()?;
    if count > a.len() || count > b.len() {
        return Err("prefix outside masks".into());
    }
    Ok((0..count).all(|i| a.get(i) == b.get(i)))
}
fn main() {
    let size = 1_000_000;
    let prefix = size - 17;
    let iterations = 25;
    let mut seed = 42u64;
    let a = Mask::from_bools((0..size).map(|_| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        seed >> 63 != 0
    }));
    let b = a.clone();
    assert!(scalar(&a, &b, prefix).unwrap());
    assert!(a.matches_prefix(&b, prefix).unwrap());
    let mut measurements = Vec::new();
    for repetition in 0..5 {
        let order = if repetition % 2 == 0 {
            vec!["scalar", "packed"]
        } else {
            vec!["packed", "scalar"]
        };
        for algorithm in order {
            let start = Instant::now();
            let mut matched = 0;
            for _ in 0..iterations {
                let (a, b, n) = (black_box(&a), black_box(&b), black_box(prefix));
                let same = if algorithm == "scalar" {
                    scalar(a, b, n)
                } else {
                    a.matches_prefix(b, n)
                }
                .unwrap();
                matched += usize::from(black_box(same));
            }
            let elapsed = start.elapsed().as_nanos();
            assert_eq!(matched, iterations);
            measurements.push(serde_json::json!({"algorithm":algorithm,"repetition":repetition,"iterations":iterations,"elapsed_ns":elapsed,"matched":matched}));
        }
    }
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"class":"COMPONENT_MICROBENCHMARK_NOT_RESEARCH_THROUGHPUT","operation":"equal-mask prefix validation with a partial final word","cache":"warm resident independent cloned packed masks; allocation/generation excluded from operation timing","bits_per_mask":size,"prefix_bits":prefix,"input_hash":data::hash(&a).unwrap(),"compute_revision":research::compute_revision().unwrap(),"engine":rust_strategy_factory::ENGINE_VERSION,"measurements":measurements,"limitations":"This validates bit masks only. It is not a candidate screen, market replay, full-history result, codec or claim of end-to-end research improvement."})).unwrap());
}
