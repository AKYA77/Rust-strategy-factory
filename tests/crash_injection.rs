//! Supervised crash / disk-full injection for the registry and receipt store.
//!
//! Committed checkpoints must survive abrupt process death and exhausted
//! filesystems, and jobs must resume cleanly once the fault is cleared.
//! Disk-full is induced on a small tmpfs so the shared disk is never filled and
//! the failure is deterministic. When tmpfs mounting is unavailable in the host
//! environment the affected tests skip rather than fail.
use rust_strategy_factory::storage::{Claim, ExperimentKey, Outcome, Registry, Stage};
use serde_json::json;
use std::{
    fs,
    io::Write,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn key(concept: &str) -> ExperimentKey {
    ExperimentKey {
        concept_hash: concept.into(),
        instrument: "EURUSD".into(),
        dataset_hash: "crash-dataset".into(),
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

/// A small tmpfs-backed filesystem. Disk-full (ENOSPC) is induced by filling it;
/// the mount is torn down on drop. Skips (returns None) when mounting is
/// unavailable in the host environment.
struct Tmpfs {
    mount_point: PathBuf,
}
impl Tmpfs {
    fn mebibytes(mb: u64) -> Option<Self> {
        let mount_point =
            std::env::temp_dir().join(format!("rsf-crash-{}-{}", std::process::id(), nanos()));
        fs::create_dir_all(&mount_point).ok()?;
        let status = Command::new("mount")
            .args([
                "-t",
                "tmpfs",
                "-o",
                &format!("size={mb}m"),
                "tmpfs",
                mount_point.to_str()?,
            ])
            .status()
            .ok()?;
        if !status.success() {
            return None;
        }
        Some(Self { mount_point })
    }
    fn path(&self, name: &str) -> PathBuf {
        self.mount_point.join(name)
    }
    fn dir(&self) -> &Path {
        &self.mount_point
    }
}
impl Drop for Tmpfs {
    fn drop(&mut self) {
        let _ = Command::new("umount").arg(&self.mount_point).status();
        let _ = fs::remove_dir(&self.mount_point);
    }
}

/// Fill `dir` until no more space is available, keeping the filler files so the
/// filesystem stays full. Returns bytes written.
fn fill_until_full(dir: &Path) -> u64 {
    let mut total = 0u64;
    let mut index = 0u64;
    let buf = [0u8; 1 << 20];
    loop {
        let path = dir.join(format!("filler-{index}"));
        index += 1;
        let Ok(mut file) = fs::File::create(&path) else {
            break;
        };
        let mut wrote_this = 0u64;
        for _ in 0..1024 {
            if file.write_all(&buf).is_err() {
                break;
            }
            wrote_this += buf.len() as u64;
        }
        let _ = file.sync_all();
        let len = file.metadata().map(|m| m.len()).unwrap_or(wrote_this);
        total = total.saturating_add(len);
        if wrote_this < buf.len() as u64 {
            // Hit ENOSPC mid-write: this partial file keeps the disk full.
            break;
        }
    }
    total
}

/// Fill `dir` until only `reserve_bytes` remain, actually consuming space by
/// writing data. Returns the filler path so the caller can delete it to free
/// space again.
fn fill_to_reserve(dir: &Path, reserve_bytes: u64) -> PathBuf {
    let big = [0u8; 1 << 20];
    let small = [0u8; 4 << 10];
    let filler = dir.join("filler");
    let mut file = fs::File::create(&filler).unwrap();
    // Bulk: stop while at least 2 MiB plus the reserve remain, so the final
    // 1 MiB write can never overshoot below the reserve.
    loop {
        if fs2::available_space(dir).unwrap_or(0) <= reserve_bytes + (1 << 21) {
            break;
        }
        if file.write_all(&big).is_err() {
            break;
        }
    }
    // Fine: 4 KiB writes land within 4 KiB of the reserve.
    loop {
        if fs2::available_space(dir).unwrap_or(0) <= reserve_bytes {
            break;
        }
        if file.write_all(&small).is_err() {
            break;
        }
    }
    let _ = file.sync_all();
    drop(file);
    filler
}

#[test]
#[cfg(unix)]
fn disk_full_receipt_publication_rolls_back_and_resumes_after_space_freed() {
    let Some(disk) = Tmpfs::mebibytes(8) else {
        println!("skip: tmpfs mount unavailable for disk-full test");
        return;
    };
    let db = disk.path("registry.sqlite");
    let mut reg = Registry::open(&db).unwrap();

    // Committed baseline while space is plentiful.
    let id_a = reg.register(&key("baseline")).unwrap();
    let Claim::Acquired(lease_a) = reg.claim(&id_a, 0, 100000).unwrap() else {
        panic!("claim baseline")
    };
    reg.finish(&lease_a, 1, Outcome::Completed, &json!({"baseline": true}))
        .unwrap();
    reg.integrity().unwrap();

    // A second, still-running experiment whose large external receipt cannot be
    // published once the disk is exhausted.
    let id_b = reg.register(&key("victim")).unwrap();
    let Claim::Acquired(lease_b) = reg.claim(&id_b, 2, 100000).unwrap() else {
        panic!("claim victim")
    };

    // Leave a 128 KiB reserve: the writer-transaction WAL write fits, but the
    // ~300 KiB external receipt publication does not.
    let filler = fill_to_reserve(disk.dir(), 128 * 1024);

    let large = json!({"payload": "x".repeat(300 * 1024), "event_id": 7});
    let err = reg
        .finish(&lease_b, 3, Outcome::Completed, &large)
        .unwrap_err();
    assert!(
        reg.result(&id_b).unwrap().is_none(),
        "no result may commit after a failed publication: {err}"
    );
    reg.integrity().unwrap();

    // Free the reserve and the same job resumes and publishes cleanly.
    fs::remove_file(&filler).unwrap();
    reg.finish(&lease_b, 4, Outcome::Completed, &large).unwrap();
    let stored = reg.result(&id_b).unwrap().unwrap();
    assert_eq!(stored.result, large);
    reg.integrity().unwrap();
    // The baseline committed earlier is untouched by the failure and recovery.
    assert_eq!(
        reg.result(&id_a).unwrap().unwrap().result,
        json!({"baseline": true})
    );
}

#[test]
#[cfg(unix)]
fn disk_full_wal_growth_preserves_committed_data_and_resumes() {
    let Some(disk) = Tmpfs::mebibytes(4) else {
        println!("skip: tmpfs mount unavailable for disk-full test");
        return;
    };
    let db = disk.path("registry.sqlite");
    let mut reg = Registry::open(&db).unwrap();
    let id = reg.register(&key("baseline")).unwrap();
    let Claim::Acquired(lease) = reg.claim(&id, 0, 100000).unwrap() else {
        panic!("claim")
    };
    reg.finish(&lease, 1, Outcome::Completed, &json!({"baseline": true}))
        .unwrap();
    reg.integrity().unwrap();

    // Exhaust the filesystem so the WAL cannot grow for any new write.
    let written = fill_until_full(disk.dir());
    assert!(written > 0, "disk must be fillable to induce disk-full");

    // A new registry write cannot complete while the disk is full.
    assert!(
        reg.register(&key("other")).is_err(),
        "expected disk-full write refusal"
    );

    // Committed data survives the exhaustion.
    let base = reg.result(&id).unwrap().unwrap();
    assert_eq!(base.result, json!({"baseline": true}));
    reg.integrity().unwrap();

    // Clear the exhaustion and work resumes cleanly.
    for e in fs::read_dir(disk.dir()).unwrap() {
        let e = e.unwrap();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with("filler-") {
            fs::remove_file(e.path()).unwrap();
        }
    }
    let id2 = reg.register(&key("resumed")).unwrap();
    let Claim::Acquired(lease2) = reg.claim(&id2, 2, 100000).unwrap() else {
        panic!("claim resume")
    };
    reg.finish(&lease2, 3, Outcome::Completed, &json!({"resumed": true}))
        .unwrap();
    reg.integrity().unwrap();
    assert_eq!(
        reg.result(&id2).unwrap().unwrap().result,
        json!({"resumed": true})
    );
    assert_eq!(
        reg.result(&id).unwrap().unwrap().result,
        json!({"baseline": true})
    );
}

#[test]
#[cfg(unix)]
fn sigkill_during_checkpoint_write_preserves_chain_and_resumes() {
    use rust_strategy_factory::storage::Registry;
    let f = std::env::temp_dir().join(format!("rsf-sigkill-{}", std::process::id()));
    fs::create_dir(&f).unwrap();
    let spec_path = f.join("spec.json");
    let strategy = f.join("strategy.json");
    let db = f.join("kill.sqlite");

    // Build a synthetic multi-chunk corpus so the step writes checkpoints as it
    // advances through chunks (WAL/checkpoint growth).
    let p = rust_strategy_factory::contracts::Profile::synthetic("EURUSD").unwrap();
    let mut ticks = Vec::new();
    for i in 0..37u64 {
        let timestamp_ms = if i < 17 {
            (i / 2) as i64 * 60000
        } else {
            86400000 * 5 + (i / 2) as i64 * 60000
        };
        ticks.push(rust_strategy_factory::data::Tick {
            timestamp_ms,
            sequence: u64::MAX - 40 + i,
            bid: 10000 + i as i64 % 5,
            ask: 10010 + i as i64 % 7,
        });
    }
    let data = rust_strategy_factory::data::dataset(
        p.clone(),
        ticks,
        "synthetic corpus fixture",
        "fixture archive".into(),
        true,
    )
    .unwrap();
    let mut paths = Vec::new();
    for (i, ticks) in data.ticks.chunks(7).enumerate() {
        let d = rust_strategy_factory::data::dataset(
            p.clone(),
            ticks.to_vec(),
            &data.manifest.source,
            data.manifest.archive_hash.clone(),
            true,
        )
        .unwrap();
        let path = f.join(format!("{i}.rsf"));
        rust_strategy_factory::native::write(&path, &d).unwrap();
        paths.push(path);
    }
    let spec = rust_strategy_factory::corpus::describe(&paths, 100).unwrap();
    fs::write(&spec_path, serde_json::to_vec(&spec).unwrap()).unwrap();
    fs::write(
        &strategy,
        r#"{"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":2}}}"#,
    )
    .unwrap();
    let args = [
        "corpus-step",
        spec_path.to_str().unwrap(),
        strategy.to_str().unwrap(),
        db.to_str().unwrap(),
        "100",
        "10000",
        "1000",
        "1",
        "60000",
    ];
    let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
    let run = || -> serde_json::Value {
        let out = Command::new(binary).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    };
    let first = run();
    let id = first["Pending"]["experiment_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = Registry::open(&db).unwrap();
    let hash = r.replay_checkpoint(&id).unwrap().unwrap().1;
    drop(r);

    // Kill the next step while it is writing its checkpoint (WAL growth): hold
    // the write lock, spawn, then SIGKILL before it can commit.
    let lock = rusqlite::Connection::open(&db).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut child = Command::new(binary)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    child.kill().unwrap();
    assert_eq!(child.wait().unwrap().signal(), Some(9));
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);

    // Committed checkpoint chain is preserved; nothing is partially visible.
    let r = Registry::open(&db).unwrap();
    assert_eq!(r.replay_checkpoint(&id).unwrap().unwrap().1, hash);
    r.integrity().unwrap();
    drop(r);

    // The job resumes cleanly and advances past the killed attempt.
    let next = run();
    assert_eq!(next["Pending"]["cursor"], 2);

    fs::remove_dir_all(f).unwrap();
}
