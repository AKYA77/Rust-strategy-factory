// Disposable synthetic control-flow fixtures; no real source or scientific
// approval is asserted by exercising the real-parser/review branches.
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use rust_strategy_factory::{
    archive_evidence,
    calendar_evidence::{self, Declaration},
    catalog::Catalog,
    contracts::Profile,
    data,
    data_approval::{Decision, Request},
    data_review::Area,
    machine_evidence::Claim,
    review_authority::{Authority, Grant, Reviewer},
    validation::Interval,
};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
const PASSWORD: &str = "fixture-only-independent-reviewer";
const AREAS: [Area; 5] = [
    Area::Source,
    Area::Profile,
    Area::Calendar,
    Area::Quality,
    Area::Coverage,
];
struct Fixture {
    root: PathBuf,
    db: PathBuf,
    archive: PathBuf,
    secret: PathBuf,
    d: data::Dataset,
    id: String,
    machine: String,
}
impl Fixture {
    fn new(name: &str, synthetic: bool) -> Self {
        let root = std::env::temp_dir().join(format!("approval-{name}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let archive = root.join("source.csv");
        fs::write(
            &archive,
            b"timestamp_ms,sequence,bid,ask\n0,1,1.00000,1.00010\n1000,2,1.00020,1.00030\n",
        )
        .unwrap();
        let mut profile = Profile::synthetic("EURUSD").unwrap();
        if !synthetic {
            profile.research_only = false;
            profile.version = "DISPOSABLE_TEST_BRANCH".into();
            profile.provenance="fabricated profile solely to test refusal/authority control flow, not broker evidence".into();
        }
        let d = data::import_csv(
            &archive,
            profile,
            "DISPOSABLE_TEST_SOURCE",
            synthetic,
            1 << 20,
            10,
        )
        .unwrap();
        let db = root.join("catalog.sqlite");
        let mut c = Catalog::open(&db).unwrap();
        let e = c.register(&d).unwrap().entry;
        let id = e.receipt.facts.dataset_identity.clone();
        let calendar = Declaration {
            reference: "unapproved fixture calendar".into(),
            scope: Interval {
                start_ms: 0,
                end_exclusive_ms: 1001,
            },
            open: vec![Interval {
                start_ms: 0,
                end_exclusive_ms: 1001,
            }],
        };
        let claim = Claim {
            catalog_hash: e.hash.clone(),
            archive_report_hash: archive_evidence::assess(&archive, &d, &e).unwrap().hash,
            calendar_report_hash: calendar_evidence::assess(&d, &e, calendar.clone())
                .unwrap()
                .hash,
        };
        let machine = c
            .admit_machine(&archive, &d, &id, calendar, &claim)
            .unwrap()
            .id;
        let secret = root.join("authority.secret");
        Authority::initialize(&secret, vec![Self::reviewer()]).unwrap();
        Self {
            root,
            db,
            archive,
            secret,
            d,
            id,
            machine,
        }
    }
    fn reviewer() -> Reviewer {
        let salt = SaltString::encode_b64(b"fixture-only-salt").unwrap();
        Reviewer {
            name: "independent-reviewer".into(),
            password_hash: Argon2::default()
                .hash_password(PASSWORD.as_bytes(), &salt)
                .unwrap()
                .to_string(),
            grants: vec![Grant {
                symbol: "EURUSD".into(),
                source: "DISPOSABLE_TEST_SOURCE".into(),
                submitter: "fixture-preparer".into(),
                areas: AREAS.to_vec(),
                expires_ms: chrono::Utc::now().timestamp_millis() + 86400000,
            }],
        }
    }
    fn request(&self, c: &Catalog, a: &Authority, area: Area, decision: Decision) -> Request {
        Request {
            machine_id: self.machine.clone(),
            expected_head: c.review_state(&self.id, a).unwrap().head,
            area,
            decision,
            explanation: "TEST CONTROL FLOW ONLY; not market evidence".into(),
            evidence_text: "Disposable fixture reviewer decision".into(),
        }
    }
    fn approve(&self, c: &mut Catalog, a: &Authority) {
        let p = a.authenticate("independent-reviewer", PASSWORD).unwrap();
        for area in AREAS {
            let request = self.request(c, a, area, Decision::Approve);
            c.decide(a, &p, request).unwrap();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn exact_scoped_authentication_five_gates_revocation_and_artifact_changes_fail_closed() {
    let f = Fixture::new("gates", false);
    let a = Authority::load(&f.secret).unwrap();
    let mut c = Catalog::open(&f.db).unwrap();
    let gate = || {
        c.data_eligibility(&a, &f.machine, &f.archive, &f.d)
            .unwrap()
    };
    assert_eq!(gate().state, "DATA_BLOCKED");
    assert_eq!(gate().reasons.len(), 5);
    assert!(
        a.authenticate("independent-reviewer", "wrong-password-long-enough")
            .is_err()
    );
    assert!(a.authenticate("unknown", PASSWORD).is_err());
    let p = a.authenticate("independent-reviewer", PASSWORD).unwrap();
    for (i, area) in AREAS.into_iter().enumerate() {
        let r = f.request(&c, &a, area, Decision::Approve);
        c.decide(&a, &p, r).unwrap();
        let e = c
            .data_eligibility(&a, &f.machine, &f.archive, &f.d)
            .unwrap();
        assert_eq!(e.reasons.len(), 4 - i);
    }
    let gate = c
        .data_eligibility(&a, &f.machine, &f.archive, &f.d)
        .unwrap();
    assert_eq!(gate.state, "DATA_APPROVED_FOR_SCOPE");
    assert!(gate.live_machine_revalidated);
    assert_eq!(gate.scope.end_exclusive_ms, 1001);
    let stale = f.request(&c, &a, Area::Source, Decision::Approve);
    let r = f.request(&c, &a, Area::Source, Decision::Revoke);
    c.decide(&a, &p, r).unwrap();
    assert!(c.decide(&a, &p, stale).is_err());
    assert_eq!(
        c.data_eligibility(&a, &f.machine, &f.archive, &f.d)
            .unwrap()
            .state,
        "DATA_BLOCKED"
    );
    let r = f.request(&c, &a, Area::Source, Decision::Approve);
    c.decide(&a, &p, r).unwrap();
    let original = fs::read(&f.archive).unwrap();
    fs::write(&f.archive, [original.as_slice(), b"\n"].concat()).unwrap();
    let gate = c
        .data_eligibility(&a, &f.machine, &f.archive, &f.d)
        .unwrap();
    assert_eq!(gate.state, "DATA_BLOCKED");
    assert!(!gate.live_machine_revalidated);
    assert_eq!(c.review_state(&f.id, &a).unwrap().events.len(), 7);
}
#[test]
fn synthetic_data_and_research_profiles_cannot_receive_approval() {
    let f = Fixture::new("synthetic", true);
    let a = Authority::load(&f.secret).unwrap();
    let mut c = Catalog::open(&f.db).unwrap();
    let p = a.authenticate("independent-reviewer", PASSWORD).unwrap();
    for area in AREAS {
        assert!(
            c.decide(&a, &p, f.request(&c, &a, area, Decision::Approve))
                .is_err()
        );
    }
    c.decide(&a, &p, f.request(&c, &a, Area::Source, Decision::Reject))
        .unwrap();
    assert_eq!(
        c.data_eligibility(&a, &f.machine, &f.archive, &f.d)
            .unwrap()
            .state,
        "DATA_BLOCKED"
    );
    assert_eq!(f.d.manifest.class, data::DataClass::SyntheticTestOnly);
    let mut profile_only = f.d.clone();
    profile_only.manifest.class = data::DataClass::RealStructurallyValid;
    let e = c.register(&profile_only).unwrap().entry;
    assert!(e.receipt.facts.profile.research_only);
}
#[test]
fn permissions_are_exact_independent_expiring_and_policy_rotation_invalidates_old_approvals() {
    let f = Fixture::new("permissions", false);
    let mut reviewer = Fixture::reviewer();
    reviewer.grants[0].submitter = reviewer.name.clone();
    assert!(Authority::initialize(&f.root.join("invalid.secret"), vec![reviewer.clone()]).is_err());
    reviewer.grants[0].submitter = "other-preparer".into();
    reviewer.grants[0].areas = vec![Area::Source];
    reviewer.grants[0].source = "other-source".into();
    let wrong = f.root.join("wrong.secret");
    Authority::initialize(&wrong, vec![reviewer.clone()]).unwrap();
    let a = Authority::load(&wrong).unwrap();
    let p = a.authenticate(&reviewer.name, PASSWORD).unwrap();
    let mut c = Catalog::open(&f.db).unwrap();
    assert!(
        c.decide(&a, &p, f.request(&c, &a, Area::Source, Decision::Reject))
            .is_err()
    );
    reviewer.grants[0].source = "DISPOSABLE_TEST_SOURCE".into();
    reviewer.grants[0].expires_ms = 0;
    let expired = f.root.join("expired.secret");
    Authority::initialize(&expired, vec![reviewer]).unwrap();
    let a = Authority::load(&expired).unwrap();
    let p = a.authenticate("independent-reviewer", PASSWORD).unwrap();
    assert!(
        c.decide(&a, &p, f.request(&c, &a, Area::Source, Decision::Reject))
            .is_err()
    );
    let original = Authority::load(&f.secret).unwrap();
    f.approve(&mut c, &original);
    let new = f.root.join("rotated.secret");
    Authority::initialize(&new, vec![Fixture::reviewer()]).unwrap();
    let a = Authority::load(&new).unwrap();
    assert!(
        !c.review_state(&f.id, &a)
            .unwrap()
            .current_authority_authenticated
    );
    assert_eq!(
        c.data_eligibility(&a, &f.machine, &f.archive, &f.d)
            .unwrap()
            .state,
        "DATA_BLOCKED"
    );
    let old_principal = original
        .authenticate("independent-reviewer", PASSWORD)
        .unwrap();
    assert!(
        c.decide(
            &a,
            &old_principal,
            f.request(&c, &a, Area::Quality, Decision::Reject)
        )
        .is_err()
    );
}
#[test]
fn history_and_signatures_survive_snapshot_and_forgery_cannot_open_gate() {
    let f = Fixture::new("snapshot", false);
    let a = Authority::load(&f.secret).unwrap();
    let mut c = Catalog::open(&f.db).unwrap();
    f.approve(&mut c, &a);
    let before = c.review_state(&f.id, &a).unwrap();
    let backup = f.root.join("backup.sqlite");
    c.backup(&backup).unwrap();
    let restored = Catalog::existing(&backup).unwrap();
    assert_eq!(restored.review_state(&f.id, &a).unwrap().head, before.head);
    assert_eq!(
        restored
            .data_eligibility(&a, &f.machine, &f.archive, &f.d)
            .unwrap()
            .state,
        "DATA_APPROVED_FOR_SCOPE"
    );
    let sql = rusqlite::Connection::open(&f.db).unwrap();
    assert!(sql.execute("DELETE FROM catalog_decisions", []).is_err());
    assert!(
        sql.execute("UPDATE catalog_decisions SET id=id", [])
            .is_err()
    );
    sql.execute_batch("DROP TRIGGER decisions_no_update")
        .unwrap();
    sql.execute(
        "UPDATE catalog_decisions SET authenticator=?1 WHERE sequence=5",
        ["0".repeat(64)],
    )
    .unwrap();
    assert!(
        !c.review_state(&f.id, &a)
            .unwrap()
            .current_authority_authenticated
    );
    assert_eq!(
        c.data_eligibility(&a, &f.machine, &f.archive, &f.d)
            .unwrap()
            .state,
        "DATA_BLOCKED"
    );
}
#[test]
fn actual_cli_decisions_use_stdin_credentials_and_source_corrections_block_old_gates() {
    let f = Fixture::new("cli", false);
    let a = Authority::load(&f.secret).unwrap();
    let mut c = Catalog::open(&f.db).unwrap();
    let request = f.root.join("request.json");
    fs::write(
        &request,
        serde_json::to_vec(&f.request(&c, &a, Area::Source, Decision::Reject)).unwrap(),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rust-strategy-factory"))
        .args([
            "data-review-decide",
            f.db.to_str().unwrap(),
            f.secret.to_str().unwrap(),
            "independent-reviewer",
            request.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(PASSWORD.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(!text.contains(PASSWORD));
    assert!(!text.contains("password_hash"));
    f.approve(&mut c, &a);
    let mut revision = f.d.clone();
    revision.manifest.archive_hash = "revision".into();
    c.register_correction(&revision, &f.id, "test correction")
        .unwrap();
    let gate = c
        .data_eligibility(&a, &f.machine, &f.archive, &f.d)
        .unwrap();
    assert_eq!(gate.state, "DATA_BLOCKED");
    assert!(gate.reasons.iter().any(|s| s.contains("superseded")));
}
#[test]
fn concurrent_reviewers_cannot_overwrite_a_revocation_from_a_stale_head() {
    let f = Fixture::new("race", false);
    let a = Authority::load(&f.secret).unwrap();
    let c = Catalog::existing(&f.db).unwrap();
    let request = f.request(&c, &a, Area::Source, Decision::Reject);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut children = Vec::new();
    // Open connections before the race so schema setup is outside this test.
    let catalogs = [Catalog::open(&f.db).unwrap(), Catalog::open(&f.db).unwrap()];
    for mut c in catalogs {
        let authority = f.secret.clone();
        let barrier = barrier.clone();
        let request = request.clone();
        children.push(std::thread::spawn(move || {
            let a = Authority::load(&authority).unwrap();
            let p = a.authenticate("independent-reviewer", PASSWORD).unwrap();
            barrier.wait();
            c.decide(&a, &p, request).is_ok()
        }));
    }
    assert_eq!(
        children
            .into_iter()
            .filter_map(|t| t.join().ok())
            .filter(|v| *v)
            .count(),
        1
    );
    assert_eq!(c.review_state(&f.id, &a).unwrap().events.len(), 1);
}
#[test]
fn source_area_symbol_permissions_and_owner_only_secret_files_are_enforced() {
    let f = Fixture::new("permissions-boundary", false);
    let mut reviewer = Fixture::reviewer();
    reviewer.grants[0].areas = vec![Area::Source];
    let path = f.root.join("source-only.secret");
    Authority::initialize(&path, vec![reviewer.clone()]).unwrap();
    let a = Authority::load(&path).unwrap();
    let p = a.authenticate("independent-reviewer", PASSWORD).unwrap();
    let mut c = Catalog::open(&f.db).unwrap();
    assert!(
        c.decide(&a, &p, f.request(&c, &a, Area::Profile, Decision::Reject))
            .is_err()
    );
    c.decide(&a, &p, f.request(&c, &a, Area::Source, Decision::Reject))
        .unwrap();
    reviewer.grants[0].symbol = "XAUUSD".into();
    let path = f.root.join("wrong-symbol.secret");
    Authority::initialize(&path, vec![reviewer]).unwrap();
    let a = Authority::load(&path).unwrap();
    let p = a.authenticate("independent-reviewer", PASSWORD).unwrap();
    assert!(
        c.decide(&a, &p, f.request(&c, &a, Area::Source, Decision::Reject))
            .is_err()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Authority::load(&path).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let alias = f.root.join("alias.secret");
        symlink(&path, &alias).unwrap();
        assert!(Authority::load(&alias).is_err());
    }
    assert!(Authority::initialize(&f.secret, vec![Fixture::reviewer()]).is_err());
}
