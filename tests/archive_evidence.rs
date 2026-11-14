use rust_strategy_factory::{
    archive_evidence,
    catalog::Catalog,
    contracts::Profile,
    data::{self, DataClass},
    import_job,
};
use std::fs;
#[test]
fn consumed_archive_and_full_quote_consistency_survive_chunk_export_without_approval() {
    let root = std::env::temp_dir().join(format!("archive-evidence-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let archive = root.join("source.csv");
    let bytes=b"timestamp_ms,sequence,bid,ask\n0,18446744073709551614,1.00000,1.00010\n1000,18446744073709551615,1.00020,1.00030\n";
    fs::write(&archive, bytes).unwrap();
    let profile = Profile::synthetic("EURUSD").unwrap();
    let d = data::import_csv(
        &archive,
        profile.clone(),
        "SYNTHETIC_TEST_ONLY archive evidence",
        true,
        1 << 20,
        10,
    )
    .unwrap();
    let path = root.join("catalog.sqlite");
    let mut c = Catalog::open(&path).unwrap();
    let entry = c.register(&d).unwrap().entry;
    let r = archive_evidence::assess(&archive, &d, &entry).unwrap();
    assert_eq!(r.evidence.declared_class, DataClass::SyntheticTestOnly);
    assert_eq!(r.evidence.events, 2);
    assert_eq!(r.evidence.catalog_hash, entry.hash);
    let dataset = root.join("dataset.json");
    fs::write(&dataset, serde_json::to_vec(&d).unwrap()).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "archive-evidence",
            archive.to_str().unwrap(),
            dataset.to_str().unwrap(),
            path.to_str().unwrap(),
            &entry.receipt.facts.dataset_identity,
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let job = root.join("job");
    import_job::step(
        &archive,
        &job,
        profile.clone(),
        "SYNTHETIC_TEST_ONLY archive evidence",
        true,
        1,
    )
    .unwrap();
    let partial = rust_strategy_factory::native::read(&job.join("chunk-000000.rsf")).unwrap();
    let partial_entry = c.register(&partial).unwrap().entry;
    assert!(archive_evidence::assess(&archive, &partial, &partial_entry).is_err());
    import_job::step(
        &archive,
        &job,
        profile,
        "SYNTHETIC_TEST_ONLY archive evidence",
        true,
        1,
    )
    .unwrap();
    let full = import_job::assemble(&job).unwrap();
    let full_entry = c.register(&full).unwrap().entry;
    let report = archive_evidence::assess(&archive, &full, &full_entry).unwrap();
    assert_ne!(
        report.evidence.archive_parser,
        report.evidence.observed_parser
    );
    assert_eq!(fs::read(&archive).unwrap(), bytes);
    fs::write(&archive, [bytes.as_slice(), b"\n"].concat()).unwrap();
    assert!(archive_evidence::assess(&archive, &d, &entry).is_err());
    let changed=b"timestamp_ms,sequence,bid,ask\n0,18446744073709551614,1.00000,1.00010\n1000,18446744073709551615,1.00040,1.00050\n";
    fs::write(&archive, changed).unwrap();
    let mut forged = d.clone();
    forged.manifest.archive_hash = blake3::hash(changed).to_hex().to_string();
    let forged_entry = c.register(&forged).unwrap().entry;
    assert!(archive_evidence::assess(&archive, &forged, &forged_entry).is_err());
    drop(c);
    fs::remove_dir_all(root).unwrap();
}
