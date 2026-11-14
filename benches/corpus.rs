//! Bounded chunk/candle inventory; not screening, replay or research throughput.
use rust_strategy_factory::{
    candles::{self, Candle, Ohlc, Timeframe},
    contracts::Profile,
    corpus,
    data::{self, Dataset, Tick},
    native, research,
};
use serde_json::json;
use std::{fs, time::Instant};
// Frozen pre-stream batch implementation for a same-input component comparison.
fn baseline(d: &Dataset, tf: Timeframe) -> Vec<Candle> {
    d.verify().unwrap();
    let make = |t: Tick| {
        let start = t.timestamp_ms.div_euclid(tf.millis()) * tf.millis();
        Candle {
            start_ms: start,
            available_ms: start + tf.millis(),
            publication_sequence: t.sequence,
            bid: Ohlc {
                open: t.bid,
                high: t.bid,
                low: t.bid,
                close: t.bid,
            },
            ask: Ohlc {
                open: t.ask,
                high: t.ask,
                low: t.ask,
                close: t.ask,
            },
            events: 1,
        }
    };
    let mut out = Vec::new();
    let mut c = make(d.ticks[0]);
    for t in &d.ticks[1..] {
        if t.timestamp_ms >= c.available_ms {
            c.available_ms = t.timestamp_ms;
            c.publication_sequence = t.sequence;
            out.push(c);
            c = make(*t);
        } else {
            c.bid.high = c.bid.high.max(t.bid);
            c.bid.low = c.bid.low.min(t.bid);
            c.bid.close = t.bid;
            c.ask.high = c.ask.high.max(t.ask);
            c.ask.low = c.ask.low.min(t.ask);
            c.ask.close = t.ask;
            c.events += 1;
        }
    }
    out
}
fn main() {
    let root = std::env::temp_dir().join(format!("corpus-bench-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let p = Profile::synthetic("EURUSD").unwrap();
    let mut paths = Vec::new();
    let mut expected = blake3::Hasher::new();
    expected.update(b"[");
    let mut sample = None;
    let mut total = 0u64;
    let chunk_events = 65536;
    for chunk in 0..33u64 {
        let mut ticks = Vec::with_capacity(chunk_events);
        for i in 0..chunk_events as u64 {
            let n = chunk * chunk_events as u64 + i;
            let tick = Tick {
                timestamp_ms: 1735689600000
                    + (n / 2) as i64 * 500
                    + if n >= 1000000 { 172800000 } else { 0 },
                sequence: n,
                bid: 100000 + (n % 1000) as i64 * 2,
                ask: 100010 + (n % 1000) as i64 * 2 + (n % 7) as i64,
            };
            if n > 0 {
                expected.update(b",");
            }
            // Independent explicit canonical representation of original observations.
            expected.update(
                format!(
                    "{{\"timestamp_ms\":{},\"sequence\":{},\"bid\":{},\"ask\":{}}}",
                    tick.timestamp_ms, tick.sequence, tick.bid, tick.ask
                )
                .as_bytes(),
            );
            ticks.push(tick);
        }
        let d = data::dataset(
            p.clone(),
            ticks,
            "SYNTHETIC_TEST_ONLY bounded corpus benchmark",
            format!("synthetic archive chunk {chunk}"),
            true,
        )
        .unwrap();
        let path = root.join(format!("{chunk}.rsf"));
        native::write(&path, &d).unwrap();
        paths.push(path);
        total += d.ticks.len() as u64;
        if sample.is_none() {
            sample = Some(d);
        }
    }
    expected.update(b"]");
    let expected = expected.finalize().to_hex().to_string();
    let spec = corpus::describe(&paths, chunk_events).unwrap();
    let bytes = paths
        .iter()
        .map(|p| fs::metadata(p).unwrap().len())
        .sum::<u64>();
    let mut component = Vec::new();
    let d = sample.unwrap();
    for tf in Timeframe::ALL {
        assert_eq!(baseline(&d, tf), candles::build(&d, tf).unwrap());
    }
    for repetition in 0..5 {
        let modes = if repetition % 2 == 0 {
            ["frozen_batch", "shared_transition_batch"]
        } else {
            ["shared_transition_batch", "frozen_batch"]
        };
        for mode in modes {
            let start = Instant::now();
            let mut closed = 0;
            for _ in 0..25 {
                let c = if mode == "frozen_batch" {
                    baseline(&d, Timeframe::M1)
                } else {
                    candles::build(&d, Timeframe::M1).unwrap()
                };
                closed += std::hint::black_box(c).len();
            }
            component.push(json!({"repetition":repetition,"mode":mode,"calls":25,"events_per_call":d.ticks.len(),"closed_candles":closed,"elapsed_ms":start.elapsed().as_secs_f64()*1000.}));
        }
    }
    drop(d);
    let mut scans = Vec::new();
    let mut candle_fingerprint = None;
    for repetition in 0..5 {
        let start = Instant::now();
        let report = corpus::inspect(&root, &spec).unwrap();
        let ms = start.elapsed().as_secs_f64() * 1000.;
        assert_eq!(report.events, total);
        assert_eq!(report.ordered_events_hash, expected);
        assert_eq!(report.gaps_over_hour, 1);
        assert_eq!(report.max_resident_chunk_events, chunk_events);
        let hash = data::hash(&report.candles).unwrap();
        if let Some(expected) = &candle_fingerprint {
            assert_eq!(&hash, expected);
        } else {
            candle_fingerprint = Some(hash.clone());
        }
        scans.push(json!({"repetition":repetition,"wall_ms":ms,"events":report.events,"max_resident_chunk_events":report.max_resident_chunk_events,"ordered_events_hash":report.ordered_events_hash,"candle_summary_hash":hash,"candles":report.candles}));
    }
    println!("{}",serde_json::to_string_pretty(&json!({"class":"SYNTHETIC_TEST_ONLY ordered corpus/candle inventory and batch-candle component","compute_revision":research::compute_revision().unwrap(),"engine":rust_strategy_factory::ENGINE_VERSION,"corpus_identity":spec.identity().unwrap(),"artifacts":paths.len(),"native_bytes":bytes,"events":total,"batch_component":component,"ordered_chunk_scans":scans,"limitations":"Five warm-file scans, one synthetic instrument. Whole-child resource receipt includes bounded per-chunk fixture creation, independent event-hash oracle, batch comparison and all scans. It is not per-scan RSS, physical-cold/full market-history acceptance, acquisition, tick replay, candidate screening or research throughput. No corpus concatenation; the retained sample during setup/component is one65536-event chunk. No general speedup claim."})).unwrap());
    fs::remove_dir_all(root).unwrap();
}
