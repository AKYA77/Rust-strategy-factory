use rust_strategy_factory::{contracts::Profile, data, native};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn dir() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "factory-native-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&p).unwrap();
    p
}
#[test]
fn native_roundtrip_preserves_all_eight_profiles_provenance_and_tied_order() {
    let dir = dir();
    for symbol in rust_strategy_factory::contracts::SYMBOLS {
        let mut input = data::synthetic_fixture(symbol).unwrap();
        input.ticks[1].timestamp_ms = input.ticks[0].timestamp_ms;
        input.manifest.content_hash = data::hash(&input.ticks).unwrap();
        input.manifest.timestamp_ties += 1;
        let path = dir.join(format!("{symbol}.rsf"));
        native::write(&path, &input).unwrap();
        let restored = native::read(&path).unwrap();
        assert_eq!(data::hash(&input).unwrap(), data::hash(&restored).unwrap());
        assert_eq!(
            data::dataset_identity(&input.manifest).unwrap(),
            data::dataset_identity(&restored.manifest).unwrap()
        );
        assert!(native::write(&path, &input).is_err());
        assert_eq!(
            restored.ticks[0].timestamp_ms,
            restored.ticks[1].timestamp_ms
        );
        assert!(restored.ticks[0].sequence < restored.ticks[1].sequence);
    }
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 8);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn native_rejects_corruption_truncation_trailing_bytes_and_hostile_lengths() {
    let dir = dir();
    let good = dir.join("good.rsf");
    native::write(&good, &data::synthetic_fixture("EURUSD").unwrap()).unwrap();
    let bytes = std::fs::read(&good).unwrap();
    let bad = dir.join("bad.rsf");
    for offset in [0, 8, 16, 30, bytes.len() - 33, bytes.len() - 1] {
        let mut changed = bytes.clone();
        changed[offset] ^= 0x40;
        std::fs::write(&bad, changed).unwrap();
        assert!(native::read(&bad).is_err(), "offset {offset}");
    }
    for n in [0, 8, 19, bytes.len() - 1] {
        std::fs::write(&bad, &bytes[..n]).unwrap();
        assert!(native::read(&bad).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    std::fs::write(&bad, trailing).unwrap();
    assert!(native::read(&bad).unwrap_err().contains("length"));
    let mut huge = bytes;
    huge[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    std::fs::write(&bad, huge).unwrap();
    assert!(native::read(&bad).unwrap_err().contains("budget"));
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn checksum_does_not_replace_event_and_profile_validation() {
    let dir = dir();
    let path = dir.join("input.rsf");
    native::write(&path, &data::synthetic_fixture("EURUSD").unwrap()).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    let end = bytes.len() - 32;
    let count = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    let meta = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let bid = 20 + meta + count * 16;
    bytes[bid..bid + 8].copy_from_slice(&(-1i64).to_le_bytes());
    let checksum = blake3::hash(&bytes[..end]);
    bytes[end..].copy_from_slice(checksum.as_bytes());
    std::fs::write(&path, bytes).unwrap();
    assert!(native::read(&path).is_err());
    let mut invalid = data::synthetic_fixture("EURUSD").unwrap();
    invalid.profile = Profile::synthetic("GBPUSD").unwrap();
    assert!(native::write(&dir.join("invalid.rsf"), &invalid).is_err());
    assert!(!dir.join("invalid.rsf").exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn column_chunk_boundaries_and_full_unsigned_sequences_roundtrip_exactly() {
    let dir = dir();
    let path = dir.join("chunks.rsf");
    let count = 17001;
    let ticks = (0..count)
        .map(|i| data::Tick {
            timestamp_ms: (i / 2) * 1000,
            sequence: u64::MAX - count as u64 + i as u64,
            bid: 100000 + i % 100,
            ask: 100020 + i % 100,
        })
        .collect();
    let input = data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        ticks,
        "SYNTHETIC_TEST_ONLY chunk boundaries",
        "generated".into(),
        true,
    )
    .unwrap();
    native::write(&path, &input).unwrap();
    let restored = native::read(&path).unwrap();
    assert_eq!(input.ticks, restored.ticks);
    assert_eq!(restored.ticks.last().unwrap().sequence, u64::MAX - 1);
    std::fs::remove_dir_all(dir).unwrap();
}
