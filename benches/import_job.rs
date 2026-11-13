//! Local synthetic import cost, not acquisition or financial research throughput.
use rust_strategy_factory::{contracts::Profile, data, import_job, native, research};
use serde_json::json;
use std::{fs, time::Instant};
fn main() {
    let root = std::env::temp_dir().join(format!("factory-import-bench-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let input = root.join("input.csv");
    let mut csv = String::from("timestamp_ms,sequence,bid,ask\n");
    for i in 0..20000u64 {
        let time = 1735689600000i64 + (i / 2) as i64 * 1000;
        if i % 2 == 0 {
            csv.push_str(&format!("{time},{i},1.00000,1.00010\n"));
        } else {
            csv.push_str(&format!("{time},{i},1.00020,1.00030\n"));
        }
    }
    fs::write(&input, &csv).unwrap();
    let source_hash = blake3::hash(csv.as_bytes()).to_hex().to_string();
    let mut measurements = Vec::new();
    let mut expected = None;
    for repetition in 0..5 {
        let modes = if repetition % 2 == 0 {
            ["monolithic", "checkpointed"]
        } else {
            ["checkpointed", "monolithic"]
        };
        for mode in modes {
            let directory = root.join(format!("{repetition}-{mode}"));
            fs::create_dir(&directory).unwrap();
            let started = Instant::now();
            let mut calls = 0;
            if mode == "monolithic" {
                let d = data::import_csv(
                    &input,
                    Profile::synthetic("EURUSD").unwrap(),
                    "SYNTHETIC_TEST_ONLY benchmark",
                    true,
                    64 * 1024 * 1024,
                    2_000_000,
                )
                .unwrap();
                native::write(&directory.join("whole.rsf"), &d).unwrap();
            } else {
                loop {
                    let p = import_job::step(
                        &input,
                        &directory,
                        Profile::synthetic("EURUSD").unwrap(),
                        "SYNTHETIC_TEST_ONLY benchmark",
                        true,
                        5000,
                    )
                    .unwrap();
                    calls += 1;
                    assert!(calls <= 5);
                    if p.complete {
                        break;
                    }
                }
            }
            let new_ms = started.elapsed().as_secs_f64() * 1000.;
            let mut complete_call_ms = None;
            let files = if mode == "monolithic" {
                vec![directory.join("whole.rsf")]
            } else {
                let started = Instant::now();
                let p = import_job::step(
                    &input,
                    &directory,
                    Profile::synthetic("EURUSD").unwrap(),
                    "SYNTHETIC_TEST_ONLY benchmark",
                    true,
                    5000,
                )
                .unwrap();
                complete_call_ms = Some(started.elapsed().as_secs_f64() * 1000.);
                assert!(p.complete);
                assert_eq!(p.events, 20000);
                p.chunks.iter().map(|c| directory.join(&c.file)).collect()
            };
            let mut ticks = Vec::new();
            let mut output_bytes = 0;
            for file in &files {
                let d = native::read(file).unwrap();
                assert_eq!(d.manifest.archive_hash, source_hash);
                assert_eq!(d.manifest.class, data::DataClass::SyntheticTestOnly);
                output_bytes += fs::metadata(file).unwrap().len();
                ticks.extend(d.ticks);
            }
            assert_eq!(ticks.len(), 20000);
            let hash = data::hash(&ticks).unwrap();
            if let Some(expected) = &expected {
                assert_eq!(&hash, expected);
            } else {
                expected = Some(hash.clone());
            }
            measurements.push(json!({"repetition":repetition,"mode":mode,"new_ms":new_ms,"completed_revalidation_ms":complete_call_ms,"step_calls":calls,"artifacts":files.len(),"artifact_bytes":output_bytes,"events":ticks.len(),"ordered_events_hash":hash}));
        }
    }
    assert_eq!(fs::read(&input).unwrap(), csv.as_bytes());
    println!(
        "{}",
        json!({"class":"LOCAL_SYNTHETIC_IMPORT_NOT_RESEARCH_THROUGHPUT","compute_revision":research::compute_revision().unwrap(),"engine":rust_strategy_factory::ENGINE_VERSION,"events_per_source":20000,"archive_bytes":csv.len(),"archive_hash":source_hash,"measurements":measurements,"limitations":"Five rotated warm local repetitions of one synthetic source, not independent market evidence. New clock includes parsing/snapshot/hash, validation, SQLite/checkpoints when selected, native publication/fsync. Checkpointed calls reopen the ledger and revalidate committed chunks; monolithic has one artifact and no ledger. Layout/parser identities differ; only combined ordered quote streams/archive hash/class match. Final output reading/hash and fixture creation excluded from new clock; completed revalidation reported separately. Whole-process resources include both paths, validation and fixture creation; no per-path RSS, physical-cold I/O, acquisition, full-history, crash/disk-full or factory speedup acceptance."})
    );
    fs::remove_dir_all(root).unwrap();
}
