use rust_strategy_factory::{
    catalog::{Catalog, Registration},
    data::{self, DataClass, Dataset},
};
use std::{fs, path::PathBuf};
struct Fixture {
    root: PathBuf,
    path: PathBuf,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("factory-catalog-{name}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let path = root.join("catalog.sqlite");
        Self { root, path }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn dataset() -> Dataset {
    data::synthetic_fixture("EURUSD").unwrap()
}
#[test]
fn immutable_registration_reuses_reimports_and_separates_provenance_revisions() {
    let f = Fixture::new("identity");
    let mut c = Catalog::open(&f.path).unwrap();
    let mut d = dataset();
    // Catalog statistics derive from events, not stale non-identity manifest counters.
    d.manifest.gaps_over_hour = 999;
    let first = c.register(&d).unwrap();
    assert!(!first.reused);
    assert_eq!(first.entry.receipt.facts.gaps_over_hour, 1);
    assert_eq!(
        first.entry.receipt.facts.declared_class,
        DataClass::SyntheticTestOnly
    );
    d.manifest.imported_at = "a later reimport observation".into();
    let repeated = c.register(&d).unwrap();
    assert!(repeated.reused);
    assert_eq!(repeated.entry.hash, first.entry.hash);
    let original_id = first.entry.receipt.facts.dataset_identity.clone();
    d.manifest.archive_hash = "distinct declared archive revision".into();
    let revision = c.register(&d).unwrap();
    assert!(!revision.reused);
    assert_ne!(revision.entry.receipt.facts.dataset_identity, original_id);
    assert_eq!(
        revision.entry.receipt.facts.content_hash,
        first.entry.receipt.facts.content_hash
    );
    drop(c);
    let c = Catalog::existing(&f.path).unwrap();
    assert_eq!(
        c.entry(&original_id).unwrap().unwrap().hash,
        first.entry.hash
    );
    assert_eq!(c.page(None, 100).unwrap().entries.len(), 2);
    assert!(
        first
            .entry
            .receipt
            .limitations
            .contains("not catalog approval")
    );
    let sql = rusqlite::Connection::open(&f.path).unwrap();
    assert!(
        sql.execute("UPDATE catalog_entries SET hash=hash", [])
            .is_err()
    );
    assert!(sql.execute("DELETE FROM catalog_entries", []).is_err());
}
#[test]
fn exact_sequence_boundaries_and_stable_identity_pagination_survive_insertions() {
    let f = Fixture::new("page");
    let mut c = Catalog::open(&f.path).unwrap();
    let mut d = dataset();
    for (i, t) in d.ticks.iter_mut().enumerate() {
        t.sequence = u64::MAX - 1999 + i as u64;
    }
    d.manifest.content_hash = data::hash(&d.ticks).unwrap();
    let mut ids = Vec::new();
    for i in 0..7 {
        d.manifest.source = format!("SYNTHETIC_TEST_ONLY revision{i}");
        let r = c.register(&d).unwrap();
        assert_eq!(
            r.entry.receipt.facts.first_sequence,
            (u64::MAX - 1999).to_string()
        );
        assert_eq!(r.entry.receipt.facts.last_sequence, u64::MAX.to_string());
        ids.push(r.entry.receipt.facts.dataset_identity);
    }
    ids.sort();
    let first = c.page(None, 2).unwrap();
    assert_eq!(
        first
            .entries
            .iter()
            .map(|e| e.receipt.facts.dataset_identity.clone())
            .collect::<Vec<_>>(),
        ids[..2]
    );
    let cursor = first.next_cursor.unwrap();
    d.manifest.source = "SYNTHETIC_TEST_ONLY concurrent append".into();
    c.register(&d).unwrap();
    let all = c.page(None, 100).unwrap();
    let expected = all
        .entries
        .iter()
        .map(|e| e.receipt.facts.dataset_identity.clone())
        .filter(|id| id > &cursor)
        .collect::<Vec<_>>();
    let rest = c.page(Some(&cursor), 100).unwrap();
    assert_eq!(
        rest.entries
            .iter()
            .map(|e| e.receipt.facts.dataset_identity.clone())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(rest.next_cursor.is_none());
    assert!(c.page(None, 0).is_err());
    assert!(c.page(None, 101).is_err());
    assert!(c.page(Some("not-an-identity"), 1).is_err());
    assert!(c.entry("../escape").is_err());
    assert!(c.entry(&"f".repeat(64)).unwrap().is_none());
}
#[test]
fn invalid_data_metadata_and_tampered_receipts_refuse_without_overwriting() {
    let f = Fixture::new("corrupt");
    let mut c = Catalog::open(&f.path).unwrap();
    let d = dataset();
    let r = c.register(&d).unwrap();
    let mut bad = d.clone();
    bad.ticks[0].bid += 1;
    assert!(c.register(&bad).is_err());
    let mut large = d.clone();
    large.manifest.source = "s".repeat(4097);
    assert!(c.register(&large).is_err());
    assert_eq!(c.page(None, 100).unwrap().entries.len(), 1);
    let sql = rusqlite::Connection::open(&f.path).unwrap();
    sql.execute_batch("DROP TRIGGER catalog_no_update;UPDATE catalog_entries SET body=CAST(replace(CAST(body AS TEXT),'Local structural','False structural') AS BLOB);").unwrap();
    assert!(c.entry(&r.entry.receipt.facts.dataset_identity).is_err());
    assert!(c.page(None, 25).is_err());
    assert!(c.register(&d).is_err());
}
#[test]
fn existing_mode_preserves_unrelated_databases_and_never_creates_missing_files() {
    let f = Fixture::new("unrelated");
    assert!(Catalog::existing(&f.path).is_err());
    assert!(!f.path.exists());
    let sql = rusqlite::Connection::open(&f.path).unwrap();
    sql.execute_batch("CREATE TABLE unrelated(value);INSERT INTO unrelated VALUES('preserve');")
        .unwrap();
    assert!(Catalog::open(&f.path).is_err());
    assert!(Catalog::existing(&f.path).is_err());
    assert_eq!(
        sql.query_row("SELECT value FROM unrelated", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "preserve"
    );
}
#[test]
fn metadata_quota_refusal_preserves_existing_rows() {
    let f = Fixture::new("quota");
    let mut c = Catalog::open(&f.path).unwrap();
    let sql = rusqlite::Connection::open(&f.path).unwrap();
    // Explicit metadata-only boundary fixture, not valid data/evidence receipts.
    sql.execute_batch("WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<9999) INSERT INTO catalog_entries SELECT printf('%064x',x),x'7b7d',printf('%064x',0) FROM n;").unwrap();
    assert!(c.register(&dataset()).is_err());
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM catalog_entries", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        10000
    );
}
#[test]
fn actual_cli_record_list_detail_and_reimport_reuse() {
    let f = Fixture::new("cli");
    let path = f.root.join("dataset.json");
    fs::write(&path, serde_json::to_vec(&dataset()).unwrap()).unwrap();
    let exe = env!("CARGO_BIN_EXE_rust-strategy-factory");
    let run = |args: &[&str]| {
        let out = std::process::Command::new(exe).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    };
    let args = [
        "catalog-record",
        path.to_str().unwrap(),
        f.path.to_str().unwrap(),
    ];
    let first: Registration = serde_json::from_slice(&run(&args)).unwrap();
    assert!(!first.reused);
    let again: Registration = serde_json::from_slice(&run(&args)).unwrap();
    assert!(again.reused);
    assert_eq!(again.entry.hash, first.entry.hash);
    let id = first.entry.receipt.facts.dataset_identity;
    let entry: serde_json::Value =
        serde_json::from_slice(&run(&["catalog-entry", f.path.to_str().unwrap(), &id])).unwrap();
    assert_eq!(entry["hash"], first.entry.hash);
    let page: serde_json::Value =
        serde_json::from_slice(&run(&["catalog-list", f.path.to_str().unwrap(), "-", "1"]))
            .unwrap();
    assert_eq!(page["entries"].as_array().unwrap().len(), 1);
    assert!(page["next_cursor"].is_null());
}
