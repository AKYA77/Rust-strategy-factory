use rust_strategy_factory::{
    archive_evidence,
    calendar_evidence::{self, Declaration},
    catalog::Catalog,
    contracts::Profile,
    data,
    machine_evidence::Claim,
    validation::Interval,
};
use std::{fs, path::PathBuf, process::Command};
struct Fixture {
    root: PathBuf,
    archive: PathBuf,
    db: PathBuf,
    d: data::Dataset,
    id: String,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("machine-{name}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let archive = root.join("source.csv");
        fs::write(&archive,b"timestamp_ms,sequence,bid,ask\n0,18446744073709551614,1.00000,1.00010\n1000,18446744073709551615,1.00020,1.00030\n").unwrap();
        let d = data::import_csv(
            &archive,
            Profile::synthetic("EURUSD").unwrap(),
            "synthetic machine fixture",
            true,
            1 << 20,
            10,
        )
        .unwrap();
        let db = root.join("catalog.sqlite");
        let id = Catalog::open(&db)
            .unwrap()
            .register(&d)
            .unwrap()
            .entry
            .receipt
            .facts
            .dataset_identity;
        Self {
            root,
            archive,
            db,
            d,
            id,
        }
    }
    fn calendar(&self) -> Declaration {
        Declaration {
            reference: "unapproved fixture schedule".into(),
            scope: Interval {
                start_ms: 0,
                end_exclusive_ms: 1001,
            },
            open: vec![],
        }
    }
    fn claim(&self) -> Claim {
        let e = Catalog::existing(&self.db)
            .unwrap()
            .entry(&self.id)
            .unwrap()
            .unwrap();
        Claim {
            catalog_hash: e.hash.clone(),
            archive_report_hash: archive_evidence::assess(&self.archive, &self.d, &e)
                .unwrap()
                .hash,
            calendar_report_hash: calendar_evidence::assess(&self.d, &e, self.calendar())
                .unwrap()
                .hash,
        }
    }
    fn admit(&self) -> rust_strategy_factory::machine_evidence::Admission {
        Catalog::open(&self.db)
            .unwrap()
            .admit_machine(
                &self.archive,
                &self.d,
                &self.id,
                self.calendar(),
                &self.claim(),
            )
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn admission_is_exact_immutable_idempotent_and_survives_backup_and_process_readback() {
    let f = Fixture::new("persist");
    let a = f.admit();
    assert_eq!(a.hash, f.admit().hash);
    assert_eq!(
        a.record.binding.archive_bytes,
        fs::metadata(&f.archive).unwrap().len()
    );
    assert!(a.record.limitations.contains("Synthetic"));
    let c = Catalog::existing(&f.db).unwrap();
    c.revalidate_machine(&a.id, &f.archive, &f.d).unwrap();
    let backup = f.root.join("backup.sqlite");
    c.backup(&backup).unwrap();
    assert_eq!(
        Catalog::existing(&backup)
            .unwrap()
            .machine(&a.id)
            .unwrap()
            .unwrap()
            .hash,
        a.hash
    );
    let sql = rusqlite::Connection::open(&f.db).unwrap();
    assert!(
        sql.execute("UPDATE catalog_machine SET hash=hash", [])
            .is_err()
    );
    assert!(sql.execute("DELETE FROM catalog_machine", []).is_err());
    let dataset = f.root.join("data.json");
    fs::write(&dataset, serde_json::to_vec(&f.d).unwrap()).unwrap();
    for args in [
        vec!["machine-report", f.db.to_str().unwrap(), &a.id],
        vec![
            "machine-revalidate",
            f.db.to_str().unwrap(),
            &a.id,
            f.archive.to_str().unwrap(),
            dataset.to_str().unwrap(),
        ],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
#[test]
fn changed_sources_claims_calendars_and_revisions_cannot_reuse_old_admission() {
    let f = Fixture::new("revision");
    let a = f.admit();
    let mut c = Catalog::open(&f.db).unwrap();
    let mut claim = f.claim();
    claim.archive_report_hash = "0".repeat(64);
    assert!(
        c.admit_machine(&f.archive, &f.d, &f.id, f.calendar(), &claim)
            .is_err()
    );
    let mut schedule = f.calendar();
    schedule.reference = "changed declaration".into();
    assert!(
        c.admit_machine(&f.archive, &f.d, &f.id, schedule, &f.claim())
            .is_err()
    );
    let bytes = fs::read(&f.archive).unwrap();
    fs::write(&f.archive, [bytes.as_slice(), b"\n"].concat()).unwrap();
    assert!(c.revalidate_machine(&a.id, &f.archive, &f.d).is_err());
    assert_eq!(c.machine(&a.id).unwrap().unwrap().hash, a.hash);
    let d = data::import_csv(
        &f.archive,
        f.d.profile.clone(),
        &f.d.manifest.source,
        true,
        1 << 20,
        10,
    )
    .unwrap();
    let e = c
        .register_correction(&d, &f.id, "byte-only source revision")
        .unwrap()
        .entry;
    let claim = Claim {
        catalog_hash: e.hash.clone(),
        archive_report_hash: archive_evidence::assess(&f.archive, &d, &e).unwrap().hash,
        calendar_report_hash: calendar_evidence::assess(&d, &e, f.calendar())
            .unwrap()
            .hash,
    };
    assert!(
        c.admit_machine(&f.archive, &d, &f.id, f.calendar(), &claim)
            .is_err()
    );
    let new = c
        .admit_machine(
            &f.archive,
            &d,
            &e.receipt.facts.dataset_identity,
            f.calendar(),
            &claim,
        )
        .unwrap();
    assert_ne!(a.id, new.id);
    assert_ne!(a.hash, new.hash);
    assert!(c.revalidate_machine(&a.id, &f.archive, &d).is_err());
    c.revalidate_machine(&new.id, &f.archive, &d).unwrap();
}
#[test]
fn corruption_is_rejected_on_read_and_snapshot_without_partial_publication() {
    let f = Fixture::new("tamper");
    let a = f.admit();
    let sql = rusqlite::Connection::open(&f.db).unwrap();
    sql.execute_batch("DROP TRIGGER machine_no_update").unwrap();
    let mut r = a.record;
    r.binding.archive_bytes += 1;
    sql.execute(
        "UPDATE catalog_machine SET body=?1",
        [serde_json::to_vec(&r).unwrap()],
    )
    .unwrap();
    let c = Catalog::existing(&f.db).unwrap();
    assert!(c.machine(&a.id).is_err());
    let dest = f.root.join("refused.sqlite");
    assert!(c.backup(&dest).is_err());
    assert!(!dest.exists());
    assert!(!fs::read_dir(&f.root).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".catalog-backup")
    }));
}
#[test]
fn actual_cli_admission_requires_recomputation_and_preserves_source_bytes() {
    let f = Fixture::new("cli");
    let dataset = f.root.join("data.json");
    let calendar = f.root.join("calendar.json");
    let claim = f.root.join("claim.json");
    fs::write(&dataset, serde_json::to_vec(&f.d).unwrap()).unwrap();
    fs::write(&calendar, serde_json::to_vec(&f.calendar()).unwrap()).unwrap();
    fs::write(&claim, serde_json::to_vec(&f.claim()).unwrap()).unwrap();
    let original = fs::read(&f.archive).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "machine-admit",
            f.archive.to_str().unwrap(),
            dataset.to_str().unwrap(),
            f.db.to_str().unwrap(),
            &f.id,
            calendar.to_str().unwrap(),
            claim.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let a: rust_strategy_factory::machine_evidence::Admission =
        serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(a.hash, f.admit().hash);
    assert_eq!(fs::read(&f.archive).unwrap(), original);
}
