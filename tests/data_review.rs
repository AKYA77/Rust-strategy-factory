use rust_strategy_factory::{
    catalog::Catalog,
    data,
    data_review::{Area, Assessment, Decision, Report, Request, State},
};
use std::{fs, path::PathBuf};
struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("factory-review-{name}-{}", std::process::id()));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn request(c: &mut Catalog) -> Request {
    let e = c
        .register(&data::synthetic_fixture("EURUSD").unwrap())
        .unwrap()
        .entry;
    Request {
        catalog_identity: e.receipt.facts.dataset_identity,
        catalog_hash: e.hash,
        reviewer: "fixture asserted reviewer; not authenticated authority".into(),
        assessments: [
            Area::Source,
            Area::Profile,
            Area::Calendar,
            Area::Quality,
            Area::Coverage,
        ]
        .map(|area| Assessment {
            area,
            decision: Decision::Pending,
            explanation: String::new(),
            evidence_text: String::new(),
        }),
    }
}
#[test]
fn retained_evidence_states_are_immutable_and_survive_snapshot_without_granting_approval() {
    let f = Fixture::new("states");
    let path = f.0.join("catalog.sqlite");
    let mut c = Catalog::open(&path).unwrap();
    let mut r = request(&mut c);
    let pending = c.record_review(r.clone()).unwrap();
    assert_eq!(pending.state, State::Incomplete);
    assert_eq!(c.record_review(r.clone()).unwrap().hash, pending.hash);
    for a in &mut r.assessments {
        a.decision = Decision::AcceptedAssertion;
        a.explanation = "fixture claim only".into();
        a.evidence_text = "Retained synthetic evidence text, not provider proof.".into();
    }
    let accepted = c.record_review(r.clone()).unwrap();
    assert_eq!(accepted.state, State::AssertionsRecorded);
    assert!(
        accepted
            .limitations
            .contains("synthetic data cannot qualify")
    );
    assert_ne!(accepted.hash, pending.hash);
    r.assessments[3].decision = Decision::Rejected;
    let rejected = c.record_review(r).unwrap();
    assert_eq!(rejected.state, State::Rejected);
    let snapshot = f.0.join("snapshot.sqlite");
    c.backup(&snapshot).unwrap();
    let restored = Catalog::existing(&snapshot).unwrap();
    for report in [pending, accepted, rejected] {
        let actual = restored.review(&report.hash).unwrap().unwrap();
        assert_eq!(actual.state, report.state);
        assert_eq!(
            actual.record.request.assessments[0].evidence_text,
            report.record.request.assessments[0].evidence_text
        );
    }
    let sql = rusqlite::Connection::open(path).unwrap();
    assert!(
        sql.execute("UPDATE catalog_reviews SET hash=hash", [])
            .is_err()
    );
    assert!(sql.execute("DELETE FROM catalog_reviews", []).is_err());
}
#[test]
fn review_scope_and_evidence_integrity_refuse_missing_or_conflicting_inputs() {
    let f = Fixture::new("refusal");
    let mut c = Catalog::open(&f.0.join("catalog.sqlite")).unwrap();
    let r = request(&mut c);
    let mut bad = r.clone();
    bad.catalog_hash = "f".repeat(64);
    assert!(c.record_review(bad).is_err());
    let mut bad = r.clone();
    bad.assessments[1].area = Area::Source;
    assert!(c.record_review(bad).is_err());
    let mut bad = r.clone();
    bad.assessments[0].decision = Decision::AcceptedAssertion;
    assert!(c.record_review(bad).is_err());
    let mut bad = r.clone();
    bad.assessments[0].evidence_text = "x".repeat(65537);
    assert!(c.record_review(bad).is_err());
    let original = Report::create(r).unwrap();
    let mut corrupted = original.record;
    corrupted.request.assessments[0].evidence_text = "changed".into();
    assert!(Report::verified(corrupted, original.hash).is_err());
}
#[test]
fn legacy_catalog_upgrade_and_actual_cli_review_roundtrip() {
    let f = Fixture::new("cli");
    let path = f.0.join("catalog.sqlite");
    let mut c = Catalog::open(&path).unwrap();
    let r = request(&mut c);
    drop(c);
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch("DROP TABLE catalog_reviews").unwrap();
    drop(sql);
    assert!(
        Catalog::existing(&path)
            .unwrap()
            .review(&"f".repeat(64))
            .unwrap()
            .is_none()
    );
    let input = f.0.join("request.json");
    fs::write(&input, serde_json::to_vec(&r).unwrap()).unwrap();
    let exe = env!("CARGO_BIN_EXE_rust-strategy-factory");
    let out = std::process::Command::new(exe)
        .args([
            "catalog-review",
            path.to_str().unwrap(),
            input.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Report = serde_json::from_slice(&out.stdout).unwrap();
    let out = std::process::Command::new(exe)
        .args([
            "catalog-review-report",
            path.to_str().unwrap(),
            &report.hash,
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let read: Report = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(read.hash, report.hash);
    assert_eq!(read.state, State::Incomplete);
}
