//! Compute identity excludes embedded UI assets but binds all simulation/persistence code,
//! compiler/target/profile, build flags and locked dependency versions.
use std::{env, fs, process::Command};
fn main() {
    let mut hasher = blake3::Hasher::new();
    for path in [
        "build.rs",
        "Cargo.toml",
        "src/lib.rs",
        "Cargo.lock",
        "rust-toolchain.toml",
        "src/contracts.rs",
        "src/data.rs",
        "src/native.rs",
        "src/compressed.rs",
        "src/parquet_store.rs",
        "src/parquet_guard.rs",
        "src/candles.rs",
        "src/indicators.rs",
        "src/strategies.rs",
        "src/orders.rs",
        "src/risk.rs",
        "src/stops.rs",
        "src/tick_engine.rs",
        "src/indexed_replay.rs",
        "src/screen.rs",
        "src/candidate_screen.rs",
        "src/proof_screen.rs",
        "src/validation.rs",
        "src/science.rs",
        "src/uncertainty.rs",
        "src/robustness.rs",
        "src/improvement.rs",
        "src/exploration.rs",
        "src/training_diagnostics.rs",
        "src/training_review.rs",
        "src/neighborhood.rs",
        "src/diagnostics.rs",
        "src/regimes.rs",
        "src/research.rs",
        "src/storage.rs",
    ] {
        println!("cargo:rerun-if-changed={path}");
        hasher.update(path.as_bytes());
        hasher.update(&[0]);
        hasher.update(&fs::read(path).expect("compute identity input must exist"));
    }
    for name in [
        "TARGET",
        "PROFILE",
        "CARGO_CFG_TARGET_FEATURE",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
        hasher.update(name.as_bytes());
        hasher.update(&[0]);
        hasher.update(env::var(name).unwrap_or_default().as_bytes());
    }
    let compiler = Command::new(env::var("RUSTC").expect("Cargo supplies RUSTC"))
        .arg("-vV")
        .output()
        .expect("compiler identity available");
    assert!(compiler.status.success());
    hasher.update(&compiler.stdout);
    println!(
        "cargo:rustc-env=FACTORY_COMPUTE_REVISION=compute-blake3:{}",
        hasher.finalize().to_hex()
    );
}
