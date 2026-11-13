use rust_strategy_factory::{
    contracts::{Profile, SYMBOLS},
    data::{self, Dataset, Tick},
    parquet_store,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn directory() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "factory-parquet-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&p).unwrap();
    p
}
fn dataset(symbol: &str, count: usize) -> Dataset {
    let p = Profile::synthetic(symbol).unwrap();
    let ticks = (0..count)
        .map(|i| Tick {
            timestamp_ms: 1735689600000 + (i / 2) as i64,
            sequence: u64::MAX - count as u64 + 1 + i as u64,
            bid: 100 * p.price_scale + (i % 10) as i64 * p.tick,
            ask: 100 * p.price_scale + (i % 10) as i64 * p.tick + 2 * p.pip,
        })
        .collect();
    data::dataset(
        p,
        ticks,
        "SYNTHETIC_TEST_ONLY exact Parquet fixture",
        "generated".into(),
        true,
    )
    .unwrap()
}
#[test]
fn all_eight_profiles_preserve_exact_arrays_metadata_ties_and_unsigned_sequence() {
    let dir = directory();
    for symbol in SYMBOLS {
        let data = dataset(symbol, 19);
        let path = dir.join(format!("{symbol}.parquet"));
        parquet_store::write(&path, &data).unwrap();
        let restored = parquet_store::read(&path).unwrap();
        assert_eq!(restored.ticks, data.ticks);
        assert_eq!(
            serde_json::to_vec(&restored).unwrap(),
            serde_json::to_vec(&data).unwrap()
        );
    }
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn row_group_boundaries_are_lossless_and_publication_never_replaces_existing_paths() {
    let dir = directory();
    let data = dataset("EURUSD", 10001);
    let path = dir.join("multiple.parquet");
    parquet_store::write(&path, &data).unwrap();
    assert_eq!(parquet_store::read(&path).unwrap().ticks, data.ticks);
    let original = fs::read(&path).unwrap();
    assert!(parquet_store::write(&path, &data).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(fs::read_dir(&dir).unwrap().all(|e| {
        !e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".factory-parquet-")
    }));
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn truncation_magic_and_footer_budgets_fail_before_decoding() {
    let dir = directory();
    let source = dir.join("source.parquet");
    parquet_store::write(&source, &dataset("EURUSD", 100)).unwrap();
    let original = fs::read(&source).unwrap();
    let bad = dir.join("bad.parquet");
    for n in [0, 1, 4, 11, original.len() - 1] {
        fs::write(&bad, &original[..n]).unwrap();
        assert!(parquet_store::read(&bad).is_err());
    }
    let mut bytes = original.clone();
    let len = bytes.len();
    bytes[len - 8..len - 4].copy_from_slice(&u32::MAX.to_le_bytes());
    fs::write(&bad, bytes).unwrap();
    assert!(parquet_store::read(&bad).unwrap_err().contains("footer"));
    let footer = [0x29, 0xfc, 0xff, 0xff, 0xff, 0xff, 0x0f, 0];
    let mut fake = b"PAR1".to_vec();
    fake.extend(footer);
    fake.extend((footer.len() as u32).to_le_bytes());
    fake.extend(b"PAR1");
    fs::write(&bad, fake).unwrap();
    assert!(
        parquet_store::read(&bad)
            .unwrap_err()
            .contains("schema element budget")
    );
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn forged_page_allocation_sizes_are_rejected_before_the_parquet_decoder() {
    use parquet::file::{reader::FileReader, serialized_reader::SerializedFileReader};
    let dir = directory();
    let path = dir.join("source.parquet");
    parquet_store::write(&path, &dataset("EURUSD", 100)).unwrap();
    let reader = SerializedFileReader::new(fs::File::open(&path).unwrap()).unwrap();
    let offset = reader.metadata().row_group(0).column(0).data_page_offset() as usize;
    drop(reader);
    let mut bytes = fs::read(&path).unwrap();
    let header = [
        0x15, 0, 0x15, 0xfe, 0xff, 0xff, 0xff, 0x0f, 0x15, 0x20, 0x2c, 0x15, 2, 0x15, 0, 0, 0,
    ];
    bytes[offset..offset + header.len()].copy_from_slice(&header);
    fs::write(&path, bytes).unwrap();
    assert!(
        parquet_store::read(&path)
            .unwrap_err()
            .contains("page size/encoding budget")
    );
    fs::remove_dir_all(dir).unwrap();
}

fn external_file(path: &std::path::Path, data: &Dataset, schema: &str, include_metadata: bool) {
    use parquet::{
        basic::{Compression, Encoding, ZstdLevel},
        data_type::Int64Type,
        file::{metadata::KeyValue, properties::WriterProperties, writer::SerializedFileWriter},
        schema::parser::parse_message_type,
    };
    let metadata = serde_json::json!({"profile":data.profile,"manifest":data.manifest}).to_string();
    let props = WriterProperties::builder()
        .set_dictionary_enabled(false)
        .set_encoding(Encoding::PLAIN)
        .set_compression(Compression::ZSTD(ZstdLevel::try_new(3).unwrap()))
        .set_key_value_metadata(
            include_metadata.then(|| vec![KeyValue::new("rsf.dataset.v1".into(), metadata)]),
        )
        .build();
    let mut writer = SerializedFileWriter::new(
        fs::File::create(path).unwrap(),
        std::sync::Arc::new(parse_message_type(schema).unwrap()),
        std::sync::Arc::new(props),
    )
    .unwrap();
    let mut group = writer.next_row_group().unwrap();
    for c in 0..4 {
        let values = data
            .ticks
            .iter()
            .map(|t| match c {
                0 => t.timestamp_ms,
                1 => t.sequence as i64,
                2 => t.bid,
                _ => t.ask,
            })
            .collect::<Vec<_>>();
        let mut column = group.next_column().unwrap().unwrap();
        column
            .typed::<Int64Type>()
            .write_batch(&values, None, None)
            .unwrap();
        column.close().unwrap();
    }
    group.close().unwrap();
    writer.close().unwrap();
}
#[test]
fn exact_logical_schema_and_original_dataset_hash_are_required() {
    const SCHEMA: &str = "message rsf_ticks { REQUIRED INT64 timestamp_ms (TIMESTAMP(MILLIS,true)); REQUIRED INT64 sequence (UINT_64); REQUIRED INT64 bid; REQUIRED INT64 ask; }";
    let dir = directory();
    let path = dir.join("external.parquet");
    let data = dataset("EURUSD", 100);
    external_file(&path, &data, &SCHEMA.replace("UINT_64", "INT_64"), true);
    assert!(parquet_store::read(&path).unwrap_err().contains("schema"));
    external_file(&path, &data, SCHEMA, false);
    assert!(
        parquet_store::read(&path)
            .unwrap_err()
            .contains("metadata absent")
    );
    let mut bad = data.clone();
    bad.manifest.content_hash = "0".repeat(64);
    external_file(&path, &bad, SCHEMA, true);
    assert!(parquet_store::read(&path).is_err());
    bad = data.clone();
    bad.ticks[0].bid = -1;
    bad.manifest.content_hash = data::hash(&bad.ticks).unwrap();
    external_file(&path, &bad, SCHEMA, true);
    assert!(
        parquet_store::read(&path)
            .unwrap_err()
            .contains("nonpositive")
    );
    fs::remove_dir_all(dir).unwrap();
}
