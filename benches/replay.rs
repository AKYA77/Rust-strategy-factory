//! Synthetic baseline, never a claim about real full-period research throughput.
use rust_strategy_factory::{
    candles::{self, Timeframe},
    contracts::{Conversion, Profile, SYMBOLS, Side},
    data::{self, Dataset, Tick},
    strategies::{Features, Rule, Strategy},
    tick_engine::{self, Costs},
};
use serde_json::json;
use std::{sync::Arc, time::Instant};
fn fixture(symbol: &str) -> Dataset {
    let p = Profile::synthetic(symbol).unwrap();
    let base = if symbol == "XAUUSD" {
        2500 * p.price_scale
    } else if symbol == "USDJPY" {
        150 * p.price_scale
    } else {
        p.price_scale
    };
    let ticks = (0..20000)
        .map(|i| {
            let phase = i % 120;
            let offset = if phase < 60 { phase } else { 120 - phase };
            let bid = base + offset * p.pip * 5;
            Tick {
                timestamp_ms: 1735689600000 + i * 10000,
                sequence: i as u64,
                bid,
                ask: bid + p.pip,
            }
        })
        .collect();
    data::dataset(
        p,
        ticks,
        "SYNTHETIC_TEST_ONLY triangle-wave benchmark",
        "generated-no-archive".into(),
        true,
    )
    .unwrap()
}
fn execute(d: &Dataset) -> usize {
    let s = Strategy {
        timeframe: Timeframe::M1,
        side: Side::Long,
        entry: None,
        stop_ticks: 100,
        rule: Rule::AboveMean { window: 5 },
    };
    let c = candles::build(d, s.timeframe).unwrap();
    let m = Features::new(&c, 100000).unwrap().signal(&s.rule).unwrap();
    let denominator = if d.profile.symbol == "USDJPY" { 150 } else { 1 };
    let fx = if d.profile.quote == "USD" {
        vec![Conversion::usd(); d.ticks.len()]
    } else {
        d.ticks
            .iter()
            .map(|t| Conversion {
                usd_num: 1,
                quote_den: denominator,
                as_of_ms: t.timestamp_ms,
            })
            .collect()
    };
    let r = tick_engine::replay(d, &c, &m, &s, &Costs::default(), &fx).unwrap();
    assert!(
        !r.trades.is_empty(),
        "benchmark requires a nonzero trade cohort for {}",
        d.profile.symbol
    );
    r.trades.len()
}
fn main() {
    let data = Arc::new(SYMBOLS.iter().map(|s| fixture(s)).collect::<Vec<_>>());
    let hashes: Vec<_> = data
        .iter()
        .map(|d| (&d.profile.symbol, &d.manifest.content_hash))
        .collect();
    let mut measurements = Vec::new();
    let mut reference = None;
    for workers in [1, 2, 3, 4] {
        let mut durations = Vec::new();
        let mut trades = 0;
        for _ in 0..5 {
            let start = Instant::now();
            let handles: Vec<_> = (0..workers)
                .map(|worker| {
                    let data = data.clone();
                    std::thread::spawn(move || {
                        (worker..data.len())
                            .step_by(workers)
                            .map(|i| execute(&data[i]))
                            .sum::<usize>()
                    })
                })
                .collect();
            trades = handles.into_iter().map(|h| h.join().unwrap()).sum();
            if let Some(expected) = reference {
                assert_eq!(trades, expected);
            } else {
                reference = Some(trades);
            }
            durations.push(start.elapsed().as_secs_f64());
        }
        durations.sort_by(f64::total_cmp);
        measurements.push(json!({"workers":workers,"repetitions":5,"wall_seconds":durations,"median_seconds":durations[2],"dispersion_max_minus_min":durations[4]-durations[0],"completed_synthetic_replays_per_repetition":8,"trades_per_repetition":trades,"replays_per_second_median":8.0/durations[2]}));
    }
    let cpu = std::fs::read_to_string("/proc/cpuinfo")
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("model name") || l.starts_with("flags"))
        .take(2)
        .collect::<Vec<_>>()
        .join("\n");
    let read = |p| std::fs::read_to_string(p).unwrap_or_else(|_| "unavailable".into());
    println!("{}",serde_json::to_string_pretty(&json!({"class":"SYNTHETIC_TEST_ONLY","coverage":"20000 events per instrument, eight instruments; NOT genuine quote data, not full historical coverage","cache":"resident fixtures; warm process; derived candles/features rebuilt each repetition","engine":"sequential ordered-l1-v2-marked-margin","build":"release thin LTO, codegen-units=1","available_parallelism":std::thread::available_parallelism().unwrap().get(),"cpu":cpu,"cgroup_cpu_max":read("/sys/fs/cgroup/cpu.max"),"cgroup_memory_max":read("/sys/fs/cgroup/memory.max"),"dataset_hashes":hashes,"measurements":measurements})).unwrap());
}
