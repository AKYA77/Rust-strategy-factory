use rust_strategy_factory::{blocked_native as blocked, contracts::SYMBOLS, data};
use std::{fs, path::PathBuf, process::Command};
struct Directory(PathBuf);
impl Directory {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("blocked-integration-{name}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn all_instruments_layouts_preserve_exact_identity_ties_full_width_order_and_ranges() {
    let dir = Directory::new("all");
    for symbol in SYMBOLS {
        let original = data::synthetic_fixture(symbol).unwrap();
        let mut ticks = original.ticks.clone();
        for (i, tick) in ticks.iter_mut().enumerate() {
            tick.sequence = u64::MAX - 1999 + i as u64;
            if i % 2 == 1 {
                tick.timestamp_ms -= 10000;
            }
        }
        let dataset = data::dataset(
            original.profile,
            ticks,
            &original.manifest.source,
            original.manifest.archive_hash,
            true,
        )
        .unwrap();
        for size in [1, 7, 8192] {
            let path = dir.0.join(format!("{symbol}-{size}.rsfb"));
            blocked::write(&path, &dataset, size).unwrap();
            let report = blocked::inspect(&path, 2000).unwrap();
            assert_eq!(
                report.dataset_identity,
                data::dataset_identity(&dataset.manifest).unwrap()
            );
            assert_eq!(report.file_hash, data::file_hash(&path).unwrap());
            assert!(report.maximum_resident_events.parse::<usize>().unwrap() <= size);
            let decoded = blocked::read(&path, 2000).unwrap();
            assert_eq!(
                serde_json::to_value(&decoded).unwrap(),
                serde_json::to_value(&dataset).unwrap()
            );
            let mut reader = blocked::Reader::open(&path, 2000, Some(&report.root)).unwrap();
            for (start, count) in [(0, 1), (6, 9), (1999, 1)] {
                assert_eq!(
                    reader.range(start, count).unwrap(),
                    dataset.ticks[start..start + count]
                );
            }
        }
    }
}
#[test]
fn actual_cli_roundtrip_and_pinned_range_export_keep_exact_integer_strings() {
    let dir = Directory::new("cli");
    let input = dir.0.join("source.json");
    let output = dir.0.join("output.rsfb");
    let unpacked = dir.0.join("unpacked.json");
    let mut dataset = data::synthetic_fixture("EURUSD").unwrap();
    for (i, tick) in dataset.ticks.iter_mut().enumerate() {
        tick.sequence = u64::MAX - 1999 + i as u64;
    }
    dataset.manifest.content_hash = data::hash(&dataset.ticks).unwrap();
    fs::write(&input, serde_json::to_vec(&dataset).unwrap()).unwrap();
    let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
    let packed = Command::new(binary)
        .args([
            "pack-blocks",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "7",
        ])
        .output()
        .unwrap();
    assert!(
        packed.status.success(),
        "{}",
        String::from_utf8_lossy(&packed.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&packed.stdout).unwrap();
    let root = report["root"].as_str().unwrap();
    let range = Command::new(binary)
        .args(["block-range", output.to_str().unwrap(), root, "6", "9"])
        .output()
        .unwrap();
    assert!(range.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&range.stdout).unwrap();
    assert_eq!(
        rows["events"][0]["sequence"],
        dataset.ticks[6].sequence.to_string()
    );
    assert_eq!(rows["events"][8]["ask"], dataset.ticks[14].ask.to_string());
    assert!(
        Command::new(binary)
            .args([
                "unpack-blocks",
                output.to_str().unwrap(),
                unpacked.to_str().unwrap()
            ])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(
        fs::read(&unpacked)
            .map(|b| serde_json::from_slice::<serde_json::Value>(&b).unwrap())
            .unwrap(),
        serde_json::to_value(&dataset).unwrap()
    );
}
