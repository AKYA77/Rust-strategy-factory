use rust_strategy_factory::{blocked_native, candles::Timeframe, corpus, data, native};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    spec: corpus::Specification,
}
impl Fixture {
    fn new(format: u8) -> Self {
        let root = std::env::temp_dir().join(format!(
            "verified-source-rehash-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let data = data::synthetic_fixture("EURUSD").unwrap();
        let mut paths = Vec::new();
        for (i, ticks) in data.ticks.chunks(600).enumerate() {
            let part = data::dataset(
                data.profile.clone(),
                ticks.to_vec(),
                &data.manifest.source,
                data.manifest.archive_hash.clone(),
                true,
            )
            .unwrap();
            let blocked = format == 1 || (format == 2 && i % 2 == 1);
            let path = root.join(format!("{i}.{}", if blocked { "rsfb" } else { "rsf" }));
            if blocked {
                blocked_native::write(&path, &part, 64).unwrap();
            } else {
                native::write(&path, &part).unwrap();
            }
            paths.push(path);
        }
        let spec = corpus::describe(&paths, if format == 0 { 100000 } else { 2000000 }).unwrap();
        Self { root, spec }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn retained_proofs_rehash_every_current_native_blocked_and_mixed_artifact_byte() {
    for format in 0..3 {
        let f = Fixture::new(format);
        let facts = corpus::source_facts(&f.root, &f.spec).unwrap();
        let prepared = corpus::prepare(&f.root, &f.spec, Timeframe::M1, 10000).unwrap();
        let original_json = serde_json::to_value(&facts).unwrap();
        assert!(original_json.get("max_artifact_events").is_none());
        assert_eq!(original_json["events"], prepared.report().events);
        facts.verify_current(&f.root, &f.spec).unwrap();
        prepared.verify_current(&f.root, &f.spec).unwrap();
        for chunk in &f.spec.chunks {
            let path = f.root.join(&chunk.path);
            let original = fs::read(&path).unwrap();
            for index in [0, original.len() / 2, original.len() - 1] {
                let mut changed = original.clone();
                changed[index] ^= 1;
                fs::write(&path, &changed).unwrap();
                assert!(facts.verify_current(&f.root, &f.spec).is_err());
                assert!(prepared.verify_current(&f.root, &f.spec).is_err());
                assert!(corpus::source_facts(&f.root, &f.spec).is_err());
                fs::write(&path, &original).unwrap();
            }
            let mut appended = original.clone();
            appended.push(0);
            fs::write(&path, appended).unwrap();
            assert!(facts.verify_current(&f.root, &f.spec).is_err());
            fs::write(&path, &original[..original.len() - 1]).unwrap();
            assert!(prepared.verify_current(&f.root, &f.spec).is_err());
            fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_len(256 * 1024 * 1024)
                .unwrap();
            assert!(
                facts
                    .verify_current(&f.root, &f.spec)
                    .unwrap_err()
                    .contains("byte budget")
            );
            fs::write(&path, &original).unwrap();
            facts.verify_current(&f.root, &f.spec).unwrap();
            prepared.verify_current(&f.root, &f.spec).unwrap();
        }
        assert_eq!(serde_json::to_value(&facts).unwrap(), original_json);
    }
}
#[test]
fn rehash_proofs_enforce_artifact_event_caps_and_resolved_paths_not_only_semantic_identity() {
    for format in 0..3 {
        let f = Fixture::new(format);
        let facts = corpus::source_facts(&f.root, &f.spec).unwrap();
        let prepared = corpus::verified_inventory(&f.root, &f.spec).unwrap();
        let mut limited = f.spec.clone();
        limited.max_chunk_events = if format == 1 {
            facts.max_resident_quote_events()
        } else {
            1
        };
        assert_eq!(limited.identity().unwrap(), f.spec.identity().unwrap());
        assert!(
            facts
                .verify_current(&f.root, &limited)
                .unwrap_err()
                .contains("event budget")
        );
        assert!(prepared.verify_current(&f.root, &limited).is_err());
        assert!(corpus::source_facts(&f.root, &limited).is_err());
        let mut relocated = f.spec.clone();
        for (i, chunk) in relocated.chunks.iter_mut().enumerate() {
            let path = f.root.join(format!("copy-{i}"));
            fs::copy(&chunk.path, &path).unwrap();
            chunk.path = path.to_str().unwrap().into();
        }
        assert_eq!(relocated.identity().unwrap(), f.spec.identity().unwrap());
        facts.verify_current(&f.root, &relocated).unwrap();
        prepared.verify_current(&f.root, &relocated).unwrap();
        fs::remove_file(&relocated.chunks.last().unwrap().path).unwrap();
        assert!(facts.verify_current(&f.root, &relocated).is_err());
        assert!(prepared.verify_current(&f.root, &relocated).is_err());
        let mut reordered = f.spec.clone();
        reordered.chunks.reverse();
        assert_ne!(reordered.identity().unwrap(), f.spec.identity().unwrap());
        assert!(facts.verify_current(&f.root, &reordered).is_err());
        if format != 0 {
            let mut changed = f.spec.clone();
            let chunk = changed
                .chunks
                .iter_mut()
                .find(|c| c.block_root.is_some())
                .unwrap();
            chunk.block_root = Some("0".repeat(64));
            assert!(prepared.verify_current(&f.root, &changed).is_err());
        }
    }
}
