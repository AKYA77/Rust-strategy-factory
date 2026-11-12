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
    fs::write(&config,serde_json::to_vec(&serde_json::json!({"limits":{"seed":10,"lease_ms":5000,"max_regions":10000,"feature_cells":100000},"inputs":[{"dataset":"data.json"}]})).unwrap()).unwrap();
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
    fs::remove_dir_all(dir).unwrap();
}
