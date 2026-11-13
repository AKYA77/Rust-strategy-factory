//! Component traversal only; candidate pipeline is measured separately.
use rust_strategy_factory::{data, indicators::Mask, research};
use std::{hint::black_box, time::Instant};
fn main() {
    let mut records = Vec::new();
    let len = 1_000_003;
    for (workload, step) in [("empty", 0), ("sparse", 1000), ("dense", 2), ("full", 1)] {
        let mask = Mask::from_bools((0..len).map(|i| step != 0 && i % step == 0));
        let expected = (0..len)
            .filter(|&i| mask.get(i))
            .fold((0usize, 0usize), |(n, sum), i| (n + 1, sum.wrapping_add(i)));
        for repetition in 0..5 {
            let order = if repetition % 2 == 0 {
                ["scalar", "packed"]
            } else {
                ["packed", "scalar"]
            };
            for algorithm in order {
                let start = Instant::now();
                let mut fingerprint = (0, 0);
                for _ in 0..20 {
                    let mask = black_box(&mask);
                    mask.validate().unwrap();
                    fingerprint = if algorithm == "scalar" {
                        (0..black_box(mask.len()))
                            .filter(|&i| mask.get(i))
                            .fold((0usize, 0usize), |(n, sum), i| (n + 1, sum.wrapping_add(i)))
                    } else {
                        mask.set_indices()
                            .unwrap()
                            .fold((0usize, 0usize), |(n, sum), i| (n + 1, sum.wrapping_add(i)))
                    };
                    assert_eq!(black_box(fingerprint), expected);
                }
                records.push(serde_json::json!({"workload":workload,"algorithm":algorithm,"repetition":repetition,"iterations":20,"elapsed_ns":start.elapsed().as_nanos(),"count_and_sum":fingerprint,"input_hash":data::hash(&mask).unwrap()}));
            }
        }
    }
    println!(
        "{}",
        serde_json::json!({"class":"COMPONENT_MICROBENCHMARK_NOT_RESEARCH_THROUGHPUT","compute_revision":research::compute_revision().unwrap(),"bits":len,"cache":"warm resident masks; generation excluded; input/count/output checks included; no expanded vector allocation","records":records,"limitations":"Only active signal traversal. No tick-engine speedup, shared candidate batch, real-data or full-history claim."})
    );
}
