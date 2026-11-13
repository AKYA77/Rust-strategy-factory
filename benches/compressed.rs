//! Synthetic codec/file component including validation/I/O; not market-replay throughput.
use rust_strategy_factory::{
    compressed,
    contracts::Profile,
    data::{self, Tick},
    native, research,
};
use std::{fs, time::Instant};
fn main() {
    let directory =
        std::env::temp_dir().join(format!("factory-codec-bench-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let dataset = data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        (0..20000)
            .map(|i| {
                let bid = 100000 + (i % 100) * 5;
                Tick {
                    timestamp_ms: 1735689600000 + i * 1000,
                    sequence: i as u64,
                    bid,
                    ask: bid + 20,
                }
            })
            .collect(),
        "SYNTHETIC_TEST_ONLY codec component",
        "generated".into(),
        true,
    )
    .unwrap();
    let plain = directory.join("source.rsf");
    native::write(&plain, &dataset).unwrap();
    let expected = serde_json::to_vec(&dataset).unwrap();
    let mut measurements = Vec::new();
    for repetition in 0..5 {
        let packed = directory.join(format!("{repetition}.rsfz"));
        let restored = directory.join(format!("{repetition}.rsf"));
        let start = Instant::now();
        let native_loaded = native::read(&plain).unwrap();
        let native_decode_validate_ns = start.elapsed().as_nanos();
        assert_eq!(serde_json::to_vec(&native_loaded).unwrap(), expected);
        let start = Instant::now();
        compressed::compress(&plain, &packed).unwrap();
        let encode_ns = start.elapsed().as_nanos();
        let start = Instant::now();
        let loaded = compressed::read(&packed).unwrap();
        let decode_validate_ns = start.elapsed().as_nanos();
        assert_eq!(serde_json::to_vec(&loaded).unwrap(), expected);
        let start = Instant::now();
        compressed::decompress(&packed, &restored).unwrap();
        let restore_validate_ns = start.elapsed().as_nanos();
        assert_eq!(fs::read(&restored).unwrap(), fs::read(&plain).unwrap());
        measurements.push(serde_json::json!({"repetition":repetition,"native_decode_validate_ns":native_decode_validate_ns,"encode_validate_publish_ns":encode_ns,"decode_validate_ns":decode_validate_ns,"restore_validate_publish_ns":restore_validate_ns,"native_bytes":fs::metadata(&plain).unwrap().len(),"compressed_bytes":fs::metadata(&packed).unwrap().len()}));
    }
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"class":"SYNTHETIC_CODEC_COMPONENT_NOT_RESEARCH_THROUGHPUT","events":dataset.ticks.len(),"dataset_hash":dataset.manifest.content_hash,"compute_revision":research::compute_revision().unwrap(),"codec":"zstd0.13.3 level3/window23 native wrapper v1","measurements":measurements,"limitations":"Warm local files, validation/allocation and encode/restore atomic publication included in timers; fixture generation excluded. No cold-I/O, Parquet comparison, full-history streaming or real market data; no selected production codec or end-to-end replay speed claim."})).unwrap());
    fs::remove_dir_all(directory).unwrap();
}
