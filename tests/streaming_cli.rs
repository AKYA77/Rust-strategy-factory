//! End-to-end tests for the full-history streaming CLI commands. Streams native
//! artifacts only; reads no external data and infers no calendar/profile/source
//! completeness, approval, licensing, scientific eligibility or research
//! eligibility.

use rust_strategy_factory::contracts::Profile;
use rust_strategy_factory::streaming;
use std::fs;
use std::path::Path;

fn temp_dir() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "factory-stream-cli-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        n
    ))
}

fn tick(t: i64, seq: u64, bid_base: i64) -> rust_strategy_factory::data::Tick {
    rust_strategy_factory::data::Tick {
        timestamp_ms: t,
        sequence: seq,
        bid: bid_base,
        ask: bid_base + 10,
    }
}

fn write_native(
    dir: &Path,
    name: &str,
    ticks: &[rust_strategy_factory::data::Tick],
    profile: &Profile,
    source: &str,
) -> std::path::PathBuf {
    let archive_hash = rust_strategy_factory::data::hash(&ticks.to_vec()).unwrap();
    let ds = rust_strategy_factory::data::dataset(
        profile.clone(),
        ticks.to_vec(),
        source,
        archive_hash,
        false,
    )
    .unwrap();
    let path = dir.join(name);
    rust_strategy_factory::native::write(&path, &ds).unwrap();
    path
}

fn chunk_ticks(
    start_ms: i64,
    start_seq: u64,
    count: usize,
    bid_base: i64,
) -> Vec<rust_strategy_factory::data::Tick> {
    (0..count)
        .map(|i| {
            let t = start_ms + (i as i64) * 60_000;
            tick(t, start_seq + i as u64, bid_base + (i as i64) * 10)
        })
        .collect()
}

// Build a CLI command from a heterogeneous list of `Display` arguments (paths,
// strings, numbers). Each argument is converted through `to_string()` so the
// helper accepts `&Path`, `&str`, `String`, `u64` and friends without arrays.
// Normalize CLI arguments: `Display` values (paths, strings, numbers) into
// `OsString`. Paths are converted losslessly via `to_os_string` (avoiding the
// non-UTF8 panic of the default formatter).
trait ToArg {
    fn to_arg(&self) -> std::ffi::OsString;
}
impl ToArg for String {
    fn to_arg(&self) -> std::ffi::OsString {
        self.clone().into()
    }
}
impl ToArg for &str {
    fn to_arg(&self) -> std::ffi::OsString {
        (*self).to_string().into()
    }
}
impl ToArg for u64 {
    fn to_arg(&self) -> std::ffi::OsString {
        self.to_string().into()
    }
}
impl ToArg for &u64 {
    fn to_arg(&self) -> std::ffi::OsString {
        (*self).to_string().into()
    }
}
impl ToArg for Path {
    fn to_arg(&self) -> std::ffi::OsString {
        self.as_os_str().to_os_string()
    }
}
impl ToArg for &Path {
    fn to_arg(&self) -> std::ffi::OsString {
        self.as_os_str().to_os_string()
    }
}
impl ToArg for std::path::PathBuf {
    fn to_arg(&self) -> std::ffi::OsString {
        self.as_os_str().to_os_string()
    }
}
impl ToArg for &std::path::PathBuf {
    fn to_arg(&self) -> std::ffi::OsString {
        self.as_os_str().to_os_string()
    }
}

macro_rules! run {
    ($bin:expr, $($arg:expr),*) => {{
        let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
        let mut args = vec![std::ffi::OsString::from($bin.to_string())];
        $(
            args.push($arg.to_arg());
        )*
        std::process::Command::new(&binary)
            .args(&args)
            .output()
            .unwrap()
    }};
}

#[test]
fn describe_then_decode_matches_library_roundtrip() {
    let dir = temp_dir();
    fs::create_dir(&dir).unwrap();
    let store_dir = temp_dir();
    fs::create_dir(&store_dir).unwrap();
    let profile = Profile::synthetic("AUDUSD").unwrap();

    let ticks1 = chunk_ticks(1_735_689_600_000, 0, 4_000, 1_000_000);
    let ticks2 = chunk_ticks(1_735_930_600_000, 4_000, 6_000, 1_000_010);
    let path1 = write_native(&dir, "chunk-000000.rsf", &ticks1, &profile, "source-v1");
    let path2 = write_native(&dir, "chunk-000001.rsf", &ticks2, &profile, "source-v1");

    // 1) CLI describe: binds exact per-artifact fingerprints and a deterministic identity.
    let paths = vec![path1.clone(), path2.clone()];
    let paths_json = dir.join("paths.json");
    fs::write(&paths_json, serde_json::to_string(&paths).unwrap()).unwrap();
    let spec_out = dir.join("spec.json");
    let out = run!(
        "historical-stream-describe",
        dir.to_string_lossy().as_ref(),
        paths_json,
        spec_out
    );
    assert!(
        out.status.success(),
        "describe failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let spec: streaming::StreamSpec =
        serde_json::from_slice(&fs::read(&spec_out).unwrap()).unwrap();
    assert_eq!(spec.instrument, "AUDUSD");
    assert_eq!(spec.events, 10_000);
    assert_eq!(spec.chunks.len(), 2);
    assert_eq!(spec.ordered_events_hash.len(), 64);

    // Identity is stable across re-serialization of the spec.
    let re_serialized = serde_json::to_vec(&spec).unwrap();
    let rein: streaming::StreamSpec = serde_json::from_slice(&re_serialized).unwrap();
    assert_eq!(
        streaming::identity(&spec).unwrap(),
        streaming::identity(&rein).unwrap()
    );
    assert!(streaming::verify(&dir, &spec).is_ok());

    // 2) CLI decode in bounded steps with a checkpoint store across process runs.
    let state_out = dir.join("state.json");
    for (i, step) in [4_000u64, 6_000].iter().enumerate() {
        let out = run!(
            "historical-stream-decode",
            dir,
            spec_out,
            store_dir,
            step,
            state_out
        );
        assert!(
            out.status.success(),
            "decode step {i} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let progress: streaming::StreamProgress =
            serde_json::from_slice(&fs::read(&state_out).unwrap()).unwrap();
        assert_eq!(progress.instrument, "AUDUSD");
        if i == 0 {
            assert_eq!(progress.consumed_chunks, 1);
            assert_eq!(progress.consumed_events, 4_000);
            assert!(!progress.complete);
        } else {
            assert_eq!(progress.consumed_chunks, 2);
            assert_eq!(progress.consumed_events, 10_000);
            assert!(progress.complete);
        }
    }

    // 3) Library-level report on the same spec must match the decoded tick count.
    let report = streaming::stream_report(&dir, &spec).unwrap();
    assert_eq!(report.events, 10_000);
    assert_eq!(report.chunks, 2);
    fs::remove_dir_all(&dir).unwrap();
    fs::remove_dir_all(&store_dir).unwrap();
}

#[test]
fn decode_state_persists_across_process_restarts() {
    let dir = temp_dir();
    fs::create_dir(&dir).unwrap();
    let store_dir = temp_dir();
    fs::create_dir(&store_dir).unwrap();
    let profile = Profile::synthetic("EURUSD").unwrap();
    // Two chunks: a 2-tick first chunk and a 3-tick second chunk (5 total).
    let ticks1 = chunk_ticks(1_735_689_600_000, 0, 2, 1_000_000);
    let ticks2 = chunk_ticks(1_735_689_600_000 + 2 * 60_000, 2, 3, 1_000_010);
    let path1 = write_native(&dir, "chunk-000000.rsf", &ticks1, &profile, "source-v1");
    let path2 = write_native(&dir, "chunk-000001.rsf", &ticks2, &profile, "source-v1");
    let paths = vec![path1, path2];
    let spec = streaming::describe(&dir, &paths).unwrap();
    let spec_out = dir.join("spec.json");
    fs::write(&spec_out, serde_json::to_vec(&spec).unwrap()).unwrap();

    // First process: consume the first (2-tick) chunk fully.
    let out = run!(
        "historical-stream-decode",
        dir,
        spec_out,
        store_dir,
        "2",
        dir.join("state.json")
    );
    assert!(out.status.success());
    let progress: streaming::StreamProgress =
        serde_json::from_slice(&fs::read(dir.join("state.json")).unwrap()).unwrap();
    assert_eq!(progress.consumed_chunks, 1);
    assert_eq!(progress.consumed_events, 2);
    assert!(!progress.complete);

    // Second process: resume with a fresh driver. Re-verifies the committed
    // prefix before any new work; must now complete the remaining chunk.
    let out2 = run!(
        "historical-stream-decode",
        dir,
        spec_out,
        store_dir,
        "10",
        dir.join("state2.json")
    );
    assert!(out2.status.success());
    let progress: streaming::StreamProgress =
        serde_json::from_slice(&fs::read(dir.join("state2.json")).unwrap()).unwrap();
    assert_eq!(progress.consumed_chunks, 2);
    assert_eq!(progress.consumed_events, 5);
    assert!(progress.complete);

    // Third process: already complete; the checkpoint chain must stay
    // intact and immutable (no re-advance).
    let out3 = run!(
        "historical-stream-decode",
        dir,
        spec_out,
        store_dir,
        "10",
        dir.join("state3.json")
    );
    assert!(out3.status.success());
    let progress: streaming::StreamProgress =
        serde_json::from_slice(&fs::read(dir.join("state3.json")).unwrap()).unwrap();
    assert_eq!(progress.consumed_chunks, 2);
    assert_eq!(progress.consumed_events, 5);
    assert!(progress.complete);
    let latest = streaming::CheckpointStore::open(&store_dir)
        .unwrap()
        .latest()
        .unwrap()
        .unwrap();
    assert_eq!(latest.consumed_chunks, 2);
    assert_eq!(latest.consumed_events, 5);
    assert!(!latest.previous.is_empty());
    fs::remove_dir_all(&dir).unwrap();
    fs::remove_dir_all(&store_dir).unwrap();
}

#[test]
fn decode_refuses_tampered_consumed_artifact_and_prefix() {
    let dir = temp_dir();
    fs::create_dir(&dir).unwrap();
    let store_dir = temp_dir();
    fs::create_dir(&store_dir).unwrap();
    let profile = Profile::synthetic("USDCHF").unwrap();
    let ticks = chunk_ticks(1_735_689_600_000, 0, 3, 1_000_000);
    let path1 = write_native(&dir, "chunk-000000.rsf", &ticks, &profile, "source-v1");
    // Second chunk must start after the first in time and sequence.
    let ticks2 = chunk_ticks(1_735_689_600_000 + 3 * 60_000, 3, 3, 1_000_010);
    let path2 = write_native(&dir, "chunk-000001.rsf", &ticks2, &profile, "source-v1");
    let spec = streaming::describe(&dir, &[path1.clone(), path2.clone()]).unwrap();
    let spec_out = dir.join("spec.json");
    fs::write(&spec_out, serde_json::to_vec(&spec).unwrap()).unwrap();

    // Decode one chunk through the CLI.
    let out = run!(
        "historical-stream-decode",
        dir,
        spec_out,
        store_dir,
        "3",
        dir.join("state.json")
    );
    assert!(out.status.success());
    let progress: streaming::StreamProgress =
        serde_json::from_slice(&fs::read(dir.join("state.json")).unwrap()).unwrap();
    assert_eq!(progress.consumed_chunks, 1);

    // Tamper the consumed artifact: the guarded resume refuses.
    let mut bytes = fs::read(&path1).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(&path1, bytes).unwrap();
    let out2 = run!(
        "historical-stream-decode",
        dir,
        spec_out,
        store_dir,
        "5",
        dir.join("state2.json")
    );
    assert!(
        !out2.status.success(),
        "tampered prefix unexpectedly advanced: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out2.stderr).contains("mismatch"),
        "unexpected error: {}",
        String::from_utf8_lossy(&out2.stderr)
    );

    // Describe with the tampered artifact must also refuse (fingerprint bound).
    let out3 = run!(
        "historical-stream-describe",
        dir,
        spec_out,
        dir.join("spec2.json")
    );
    assert!(
        !out3.status.success(),
        "tampered artifact unexpectedly described: {}",
        String::from_utf8_lossy(&out3.stderr)
    );
    fs::remove_dir_all(&dir).unwrap();
    fs::remove_dir_all(&store_dir).unwrap();
}
