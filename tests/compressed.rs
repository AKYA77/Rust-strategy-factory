use rust_strategy_factory::{
    compressed,
    contracts::{Profile, SYMBOLS},
    data::{self, Tick},
    native,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn directory() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "factory-compressed-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&p).unwrap();
    p
}
fn frame(raw: &[u8]) -> Vec<u8> {
    let payload = zstd::stream::encode_all(raw, 3).unwrap();
    wrap(raw.len() as u64, &payload)
}
fn wrap(raw_len: u64, payload: &[u8]) -> Vec<u8> {
    let mut b = b"RSFZST01".to_vec();
    b.extend_from_slice(&raw_len.to_le_bytes());
    b.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    b.extend_from_slice(payload);
    b.extend_from_slice(blake3::hash(&b).as_bytes());
    b
}
#[test]
fn all_eight_profiles_round_trip_byte_exact_with_provenance_and_u64_sequences() {
    let dir = directory();
    for symbol in SYMBOLS {
        let profile = Profile::synthetic(symbol).unwrap();
        let bid = 100 * profile.price_scale;
        let ticks = (0..2)
            .map(|i| Tick {
                timestamp_ms: 1735689600000,
                sequence: u64::MAX - 1 + i,
                bid: bid + i as i64 * profile.tick,
                ask: bid + 2 * profile.pip + i as i64 * profile.tick,
            })
            .collect();
        let dataset = data::dataset(
            profile,
            ticks,
            "SYNTHETIC_TEST_ONLY compressed fixture",
            "generated".into(),
            true,
        )
        .unwrap();
        let plain = dir.join(format!("{symbol}.rsf"));
        let packed = dir.join(format!("{symbol}.rsfz"));
        let restored = dir.join(format!("{symbol}-restored.rsf"));
        native::write(&plain, &dataset).unwrap();
        compressed::compress(&plain, &packed).unwrap();
        assert_eq!(
            serde_json::to_vec(&compressed::read(&packed).unwrap()).unwrap(),
            serde_json::to_vec(&dataset).unwrap()
        );
        compressed::decompress(&packed, &restored).unwrap();
        assert_eq!(fs::read(plain).unwrap(), fs::read(restored).unwrap());
    }
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn malformed_outer_and_inner_checksums_lengths_frames_and_output_caps_are_refused() {
    let dir = directory();
    let plain = dir.join("source.rsf");
    native::write(&plain, &data::synthetic_fixture("EURUSD").unwrap()).unwrap();
    let raw = fs::read(&plain).unwrap();
    let valid = frame(&raw);
    let path = dir.join("invalid.rsfz");
    for length in [0, 1, 23, 24, 55, valid.len() - 1] {
        fs::write(&path, &valid[..length]).unwrap();
        assert!(compressed::read(&path).is_err());
    }
    let mut changed = valid.clone();
    changed[24] ^= 1;
    fs::write(&path, &changed).unwrap();
    assert!(compressed::read(&path).unwrap_err().contains("checksum"));
    for offset in [8, 16] {
        let mut changed = valid.clone();
        changed[offset..offset + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        fs::write(&path, changed).unwrap();
        assert!(compressed::read(&path).is_err());
    }
    // Correct outer hash cannot authorize a false decoded length or trailing frame.
    fs::write(&path, wrap(53, &valid[24..valid.len() - 32])).unwrap();
    assert!(compressed::read(&path).is_err());
    let mut frames = valid[24..valid.len() - 32].to_vec();
    frames.extend(zstd::stream::encode_all(&[][..], 3).unwrap());
    fs::write(&path, wrap(raw.len() as u64, &frames)).unwrap();
    assert!(
        compressed::read(&path)
            .unwrap_err()
            .contains("trailing frame")
    );
    fs::write(&path, wrap(raw.len() as u64, &valid[24..valid.len() - 33])).unwrap();
    assert!(
        compressed::read(&path).is_err(),
        "valid outer framing must not hide a truncated ZSTD frame"
    );
    let mut changed = raw.clone();
    *changed.last_mut().unwrap() ^= 1;
    fs::write(&path, frame(&changed)).unwrap();
    assert!(compressed::read(&path).unwrap_err().contains("checksum"));
    let mut extra = valid.clone();
    extra.push(0);
    fs::write(&path, extra).unwrap();
    assert!(compressed::read(&path).is_err());
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn structural_validation_and_no_replace_publication_remain_mandatory() {
    let dir = directory();
    let plain = dir.join("source.rsf");
    native::write(&plain, &data::synthetic_fixture("EURUSD").unwrap()).unwrap();
    let packed = dir.join("output.rsfz");
    compressed::compress(&plain, &packed).unwrap();
    let original = fs::read(&packed).unwrap();
    assert!(compressed::compress(&plain, &packed).is_err());
    assert_eq!(fs::read(&packed).unwrap(), original);
    assert!(compressed::decompress(&packed, &plain).is_err());
    let mut raw = fs::read(&plain).unwrap();
    let events = u64::from_le_bytes(raw[8..16].try_into().unwrap()) as usize;
    let meta = u32::from_le_bytes(raw[16..20].try_into().unwrap()) as usize;
    let bid_offset = 20 + meta + events * 16;
    raw[bid_offset..bid_offset + 8].copy_from_slice(&(-1i64).to_le_bytes());
    let end = raw.len() - 32;
    let hash = blake3::hash(&raw[..end]);
    raw[end..].copy_from_slice(hash.as_bytes());
    let invalid = dir.join("invalid.rsfz");
    fs::write(&invalid, frame(&raw)).unwrap();
    assert!(compressed::read(&invalid).is_err());
    let junk = dir.join("junk.rsf");
    fs::write(&junk, b"invalid").unwrap();
    let refused = dir.join("refused.rsfz");
    assert!(compressed::compress(&junk, &refused).is_err());
    assert!(!refused.exists());
    assert!(fs::read_dir(&dir).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".factory-zstd-")
    }));
    fs::remove_dir_all(dir).unwrap();
}
