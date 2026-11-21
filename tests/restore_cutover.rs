//! Restore cutover: staged new-path restore from verified backups, cutover from
//! the old registry to the restored copy, lease/actor fencing during cutover,
//! and rollback on failure. The original registry is never deleted or modified
//! by a restore; the restored copy is an independent, fenced registry.
use rust_strategy_factory::recovery;
use rust_strategy_factory::storage::{Claim, ExperimentKey, Outcome, Registry, Stage};
use serde_json::json;
use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn key() -> ExperimentKey {
    ExperimentKey {
        concept_hash: "cutover-concept".into(),
        instrument: "EURUSD".into(),
        dataset_hash: "cutover-dataset".into(),
        start_ms: 0,
        end_exclusive_ms: 1000,
        signal_version: "s1".into(),
        indicator_version: "i1".into(),
        cost_hash: "cost".into(),
        fill_policy: "fill".into(),
        profile_hash: "profile".into(),
        engine_version: "e1".into(),
        protocol_hash: "test-only".into(),
        code_revision: "revision".into(),
        configuration_hash: "config".into(),
        stage: Stage::TickVerification,
    }
}

#[test]
fn restore_cutover_fences_copied_actors_and_revives_on_restored_copy() {
    let root = std::env::temp_dir().join(format!("rsf-cutover-{}-{}", std::process::id(), nanos()));
    fs::create_dir(&root).unwrap();
    let db = root.join("state.sqlite");
    let mut reg = Registry::open(&db).unwrap();
    let id = reg.register(&key()).unwrap();
    let Claim::Acquired(lease) = reg.claim(&id, 0, 100000).unwrap() else {
        panic!("claim")
    };
    // Simulate an active actor: a running lease that must be fenced at cutover.
    assert_eq!(reg.counts().unwrap().running, 1);

    // Staged verified backup of the pre-cutover registry.
    let backup = recovery::backup(&db, "pre-cutover").unwrap();
    assert_eq!(backup["fresh_content_verified"], true);
    assert_eq!(backup["activation_performed"], false);
    let sealed = backup["database_hash"].clone();

    // Cutover: staged new-path restore from the verified backup. The restored
    // copy becomes the active registry; copied leases are revoked (fenced).
    let copied = recovery::restore(&db, "pre-cutover", "post-cutover").unwrap();
    assert_eq!(copied["activation_performed"], false);
    assert_eq!(copied["parent_id"], "pre-cutover");
    assert_eq!(copied["revoked_leases"], "1");
    assert_eq!(copied["counts"]["running"], json!("0"));
    // The source snapshot stays sealed and untouched by the cutover.
    assert_eq!(
        recovery::verify(&db, "pre-cutover").unwrap()["database_hash"],
        sealed
    );
    // The original registry still carries its running lease (unchanged).
    assert_eq!(reg.counts().unwrap().running, 1);

    // The restored copy is independently usable: the old (copied) actor is
    // fenced and a new actor can reclaim and complete the experiment.
    let restored_path = recovery::path(&db, "post-cutover").unwrap();
    let mut restored = Registry::open(&restored_path).unwrap();
    restored.integrity().unwrap();
    let Claim::Acquired(new_lease) = restored.claim(&id, 1001, 100000).unwrap() else {
        panic!("reclaim")
    };
    assert_eq!(new_lease.attempt, lease.attempt + 1);
    restored
        .finish(
            &new_lease,
            1002,
            Outcome::Completed,
            &json!({"cutover": true}),
        )
        .unwrap();
    restored.integrity().unwrap();
    assert_eq!(
        restored.result(&id).unwrap().unwrap().result,
        json!({"cutover": true})
    );
    // The original registry is unaffected by activity on the restored copy.
    assert_eq!(reg.counts().unwrap().running, 1);
    drop(reg);
    drop(restored);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn restore_cutover_rollback_on_failure_leaves_original_untouched() {
    let root =
        std::env::temp_dir().join(format!("rsf-rollback-{}-{}", std::process::id(), nanos()));
    fs::create_dir(&root).unwrap();
    let db = root.join("state.sqlite");
    let mut reg = Registry::open(&db).unwrap();
    let id = reg.register(&key()).unwrap();
    let Claim::Acquired(lease) = reg.claim(&id, 0, 100000).unwrap() else {
        panic!("claim")
    };
    reg.finish(&lease, 1, Outcome::Completed, &json!({"committed": true}))
        .unwrap();
    let before = rust_strategy_factory::data::file_hash(&db).unwrap();

    let backup = recovery::backup(&db, "one").unwrap();
    assert_eq!(backup["fresh_content_verified"], true);
    let snapshot = recovery::path(&db, "one").unwrap();

    // Corrupt the sealed snapshot: verification must refuse before any restore.
    let mut bytes = fs::read(&snapshot).unwrap();
    bytes[0] ^= 1;
    fs::write(&snapshot, &bytes).unwrap();
    assert!(recovery::verify(&db, "one").is_err());
    // A restore of a corrupt snapshot is refused and creates no destination.
    assert!(recovery::restore(&db, "one", "rejected-copy").is_err());
    let described = recovery::describe(&db, "rejected-copy").unwrap();
    assert!(described.is_none(), "no restore destination may be created");

    // A reviewed-manifest restore with a mismatched manifest is refused too.
    assert!(recovery::restore_expected(&db, "one", "stale", Some(&"0".repeat(64))).is_err());
    assert!(recovery::describe(&db, "stale").unwrap().is_none());

    // The original registry is completely untouched: no rollback residue.
    assert_eq!(rust_strategy_factory::data::file_hash(&db).unwrap(), before);
    reg.integrity().unwrap();
    assert_eq!(
        reg.result(&id).unwrap().unwrap().result,
        json!({"committed": true})
    );
    drop(reg);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cli_restore_cutover_registry_check_passes_on_restored_copy() {
    let root = std::env::temp_dir().join(format!(
        "rsf-cutover-cli-{}-{}",
        std::process::id(),
        nanos()
    ));
    fs::create_dir(&root).unwrap();
    let db = root.join("state.sqlite");
    drop(Registry::open(&db).unwrap());
    let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
    let command = |args: &[&str]| -> serde_json::Value {
        let out = Command::new(binary).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    };
    command(&["registry-check", db.to_str().unwrap()]);
    let one = command(&["recovery-backup", db.to_str().unwrap(), "cli-one"]);
    assert_eq!(one["fresh_content_verified"], true);
    let copied = command(&[
        "recovery-restore",
        db.to_str().unwrap(),
        "cli-one",
        "new-copy",
    ]);
    assert_eq!(copied["activation_performed"], false);
    assert_eq!(copied["revoked_leases"], "0");
    // Cutover target: the restored copy passes integrity as the active registry.
    let path = command(&["recovery-path", db.to_str().unwrap(), "new-copy"])["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let checked = command(&["registry-check", &path]);
    assert_eq!(checked["running"], json!(0));
    assert_eq!(
        command(&["recovery-list", db.to_str().unwrap()])["rows"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // The original primary stays inactive and private (no activation performed).
    assert_eq!(one["activation_performed"], false);
    fs::remove_dir_all(root).unwrap();
}
