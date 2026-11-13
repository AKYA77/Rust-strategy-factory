use rust_strategy_factory::{
    contracts::Profile,
    data::{self, Tick},
};
use std::{
    fs,
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    time::Duration,
};
struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
#[test]
fn independent_rust_process_runs_pauses_resumes_and_stops_at_durable_checkpoints() {
    let dir = std::env::temp_dir().join(format!("factory-research-cli-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let dataset = dir.join("data.json");
    let config = dir.join("config.json");
    let db = dir.join("state.sqlite");
    let ticks = (0..5000)
        .map(|i| {
            let phase = i % 80;
            let wave = if phase < 40 { phase } else { 80 - phase };
            let bid = 100000 + wave * 50;
            Tick {
                timestamp_ms: 1735689600000 + i * 10000,
                sequence: i as u64,
                bid,
                ask: bid + 20,
            }
        })
        .collect();
    let data = data::dataset(
        Profile::synthetic("EURUSD").unwrap(),
        ticks,
        "SYNTHETIC_TEST_ONLY process control",
        "generated".into(),
        true,
    )
    .unwrap();
    fs::write(&dataset, serde_json::to_vec(&data).unwrap()).unwrap();
    fs::write(&config,serde_json::to_vec(&serde_json::json!({"limits":{"seed":10,"lease_ms":5000,"max_regions":10000,"feature_cells":100000,"shadow_policy":{"version":"m1-shadow-promotion-v1","threshold_bps":7000,"minimum_closed_trades":20,"promote_uncertain":true,"audit_sample_bps":1000,"seed":19}},"inputs":[{"dataset":"data.json"}]})).unwrap()).unwrap();
    let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
    let child = Command::new(binary)
        .args(["research", config.to_str().unwrap(), db.to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut guarded = ChildGuard(child);
    let child = &mut guarded.0;
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let first: serde_json::Value = serde_json::from_str(&line).unwrap();
    let id = first["run_id"].as_str().unwrap();
    let control = |state: &str| {
        assert!(
            Command::new(binary)
                .args(["research-control", db.to_str().unwrap(), id, state])
                .status()
                .unwrap()
                .success()
        )
    };
    let status = || {
        let o = Command::new(binary)
            .args(["research-status", db.to_str().unwrap(), id])
            .output()
            .unwrap();
        assert!(o.status.success());
        serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()
    };
    control("paused");
    let mut paused = status();
    for _ in 0..500 {
        if paused["active"] == false {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        paused = status();
    }
    assert_eq!(paused["state"], "paused");
    assert_eq!(paused["active"], false);
    let cursor = paused["cursor"].as_u64().unwrap();
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(status()["cursor"], cursor);
    control("running");
    let mut advanced = false;
    for _ in 0..500 {
        if status()["cursor"].as_u64().unwrap() > cursor {
            advanced = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    control("stopped");
    assert!(advanced);
    let mut exited = None;
    for _ in 0..500 {
        exited = child.try_wait().unwrap();
        if exited.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if exited.is_none() {
        child.kill().unwrap();
        child.wait().unwrap();
        panic!("research worker did not stop");
    }
    assert!(exited.unwrap().success());
    assert_eq!(status()["state"], "stopped");
    assert_eq!(status()["active"], false);
    assert!(
        Command::new(binary)
            .args(["registry-check", db.to_str().unwrap()])
            .output()
            .unwrap()
            .status
            .success()
    );
    let registry =
        rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let result: String = registry
        .query_row(
            "SELECT result_json FROM experiments WHERE state='completed' ORDER BY rowid LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["shadow_policy"]["threshold_bps"], 7000);
    assert_eq!(result["shadow_policy"]["seed"], 19);
    drop(registry);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn signals_release_the_coordinator_and_restart_running_or_paused_state_immediately() {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let binary = env!("CARGO_BIN_EXE_rust-strategy-factory");
    for signal in ["-INT", "-TERM"] {
        for paused in [false, true] {
            let dir = std::env::temp_dir().join(format!(
                "factory-cli-signal-{}-{signal}-{paused}",
                std::process::id()
            ));
            fs::create_dir(&dir).unwrap();
            let _cleanup = Cleanup(dir.clone());
            let dataset = dir.join("input.json");
            let config = dir.join("config.json");
            let database = dir.join("state.sqlite");
            assert!(
                Command::new(binary)
                    .args(["fixture", "EURUSD", dataset.to_str().unwrap()])
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
            fs::write(&config,serde_json::to_vec(&serde_json::json!({"limits":{"lease_ms":300000,"feature_cells":1000000,"improvement_children":2},"inputs":[{"dataset":"input.json"}]})).unwrap()).unwrap();
            let process = Command::new(binary)
                .args([
                    "research",
                    config.to_str().unwrap(),
                    database.to_str().unwrap(),
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut guard = ChildGuard(process);
            let mut stdout = BufReader::new(guard.0.stdout.take().unwrap());
            let mut line = String::new();
            stdout.read_line(&mut line).unwrap();
            let started: serde_json::Value = serde_json::from_str(&line).unwrap();
            let id = started["run_id"].as_str().unwrap();
            if paused {
                assert!(
                    Command::new(binary)
                        .args(["research-control", database.to_str().unwrap(), id, "paused"])
                        .output()
                        .unwrap()
                        .status
                        .success()
                );
            }
            assert!(
                Command::new("kill")
                    .args([signal, &guard.0.id().to_string()])
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
            let mut exited = None;
            for _ in 0..1000 {
                exited = guard.0.try_wait().unwrap();
                if exited.is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                exited
                    .expect("signal must finish at a bounded checkpoint")
                    .success(),
                "{signal} paused={paused}"
            );
            let status = Command::new(binary)
                .args(["research-status", database.to_str().unwrap(), id])
                .output()
                .unwrap();
            assert!(status.status.success());
            let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
            assert_eq!(status["state"], if paused { "paused" } else { "running" });
            assert_eq!(status["active"], false);
            let integrity = Command::new(binary)
                .args(["registry-check", database.to_str().unwrap()])
                .output()
                .unwrap();
            assert!(integrity.status.success());
            let counts: serde_json::Value = serde_json::from_slice(&integrity.stdout).unwrap();
            assert_eq!(
                counts["running"], 0,
                "signal exit must not leave a claimed experiment"
            );
            if paused {
                assert!(
                    Command::new(binary)
                        .args([
                            "research-control",
                            database.to_str().unwrap(),
                            id,
                            "running"
                        ])
                        .output()
                        .unwrap()
                        .status
                        .success()
                );
            }
            // No 300-second lease wait: the same run reclaims its released coordinator immediately.
            let resumed = Command::new(binary)
                .args([
                    "research",
                    config.to_str().unwrap(),
                    database.to_str().unwrap(),
                    "1",
                ])
                .output()
                .unwrap();
            assert!(
                resumed.status.success(),
                "{}",
                String::from_utf8_lossy(&resumed.stderr)
            );
            let first = resumed.stdout.split(|b| *b == b'\n').next().unwrap();
            let restarted: serde_json::Value = serde_json::from_slice(first).unwrap();
            assert_eq!(restarted["run_id"], started["run_id"]);
        }
    }
}
