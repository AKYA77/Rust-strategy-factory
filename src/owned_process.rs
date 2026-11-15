//! Explicit parent lifecycle for this executable's foreground corpus worker.
use crate::{Result, operator_config, retry_policy, storage::Registry, supervision_session};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub maximum_wall_ms: u64,
    pub shutdown_grace_ms: u64,
    pub maximum_output_bytes: usize,
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        if !(1..=86_400_000).contains(&self.maximum_wall_ms)
            || self.shutdown_grace_ms > 60_000
            || !(1024..=1024 * 1024).contains(&self.maximum_output_bytes)
        {
            return Err("invalid owned-process limits".into());
        }
        Ok(())
    }
}
struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
struct Capture {
    bytes: Vec<u8>,
    total: u64,
    error: Option<String>,
}
fn capture(
    mut input: impl Read,
    cap: usize,
    fault: Arc<AtomicBool>,
    sessions: Option<mpsc::SyncSender<String>>,
) -> Capture {
    let mut output = Capture {
        bytes: Vec::new(),
        total: 0,
        error: None,
    };
    let mut block = [0u8; 4096];
    let mut line = Vec::new();
    loop {
        match input.read(&mut block) {
            Ok(0) => break,
            Ok(n) => {
                output.total = output.total.saturating_add(n as u64);
                let remaining = cap.saturating_sub(output.bytes.len());
                output.bytes.extend_from_slice(&block[..n.min(remaining)]);
                if output.total > cap as u64 {
                    fault.store(true, Ordering::Release);
                }
                if let Some(sender) = &sessions {
                    for &byte in &block[..n] {
                        if byte == b'\n' {
                            if line.len() <= 1024
                                && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&line)
                                && let Some(id) = v["supervision_session_id"].as_str()
                                && id.len() == 64
                                && id.bytes().all(|c| c.is_ascii_hexdigit())
                            {
                                let _ = sender.try_send(id.into());
                            }
                            line.clear();
                        } else if line.len() <= 1024 {
                            line.push(byte);
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                output.error = Some(e.to_string());
                fault.store(true, Ordering::Release);
                break;
            }
        }
    }
    output
}
pub fn run(
    config: &Path,
    policy_path: &Path,
    database: &Path,
    rounds: usize,
    wait_ms: u64,
    limits: &Limits,
    shutdown: Arc<AtomicBool>,
) -> Result<serde_json::Value> {
    limits.validate()?;
    if !(1..=10000).contains(&rounds) || wait_ms > 3_600_000 {
        return Err("invalid child round/wait limits".into());
    }
    let loaded = operator_config::load_search(config)?;
    let policy: retry_policy::Policy = operator_config::read(policy_path)?;
    policy.validate()?;
    if let Some(guard) = &loaded.file.disk_guard
        && !guard.check(database)?.allowed
    {
        return Ok(serde_json::json!({"workers_started":0,"stop_reason":"disk-pressure"}));
    }
    if let Some(guard) = &loaded.file.resource_guard
        && !guard.check()?.allowed
    {
        return Ok(serde_json::json!({"workers_started":0,"stop_reason":"resource-pressure"}));
    }
    let source = crate::corpus_search::prepare_supervision(database, &loaded.run(), &policy)?;
    let mut registry = Registry::open(database)?;
    registry
        .connection
        .busy_timeout(Duration::from_millis(10))
        .map_err(|e| e.to_string())?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let started_at = chrono::Utc::now().timestamp_millis();
    let start = Instant::now();
    let mut owned = Owned(
        Command::new(executable)
            .arg("corpus-supervise")
            .arg(config)
            .arg(policy_path)
            .arg(database)
            .arg(rounds.to_string())
            .arg(wait_ms.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    let pid = owned.0.id();
    let fault = Arc::new(AtomicBool::new(false));
    let (send, receive) = mpsc::sync_channel(1);
    let stdout = owned.0.stdout.take().ok_or("child stdout missing")?;
    let stderr = owned.0.stderr.take().ok_or("child stderr missing")?;
    let cap = limits.maximum_output_bytes;
    let f = fault.clone();
    let output = std::thread::spawn(move || capture(stdout, cap, f, None));
    let f = fault.clone();
    let errors = std::thread::spawn(move || capture(stderr, cap, f, Some(send)));
    let mut session = None;
    let mut cancellation: Option<(&str, Instant)> = None;
    let mut stop_delivered = false;
    let mut stop_error = None;
    let mut forced = false;
    let status = loop {
        if let Ok(id) = receive.try_recv() {
            session = Some(id);
        }
        if let Some(status) = owned.0.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if cancellation.is_none() {
            let reason = if shutdown.load(Ordering::Acquire) {
                Some("signal")
            } else if fault.load(Ordering::Acquire) {
                Some("output-limit-or-read-error")
            } else if start.elapsed() >= Duration::from_millis(limits.maximum_wall_ms) {
                Some("wall-budget")
            } else {
                None
            };
            if let Some(reason) = reason {
                cancellation = Some((reason, Instant::now()));
            }
        }
        if let Some((_, cancelled)) = cancellation {
            if !stop_delivered && let Some(id) = &session {
                match supervision_session::status(&registry, id) {
                    Ok(value)
                        if value["pid"].as_str().and_then(|s| s.parse::<u32>().ok())
                            == Some(pid)
                            && value["search_id"] == source =>
                    {
                        match supervision_session::request_stop(
                            &mut registry,
                            id,
                            chrono::Utc::now().timestamp_millis(),
                        ) {
                            Ok(_) => {
                                stop_delivered = true;
                                stop_error = None;
                            }
                            Err(e) => stop_error = Some(e),
                        }
                    }
                    Ok(_) => stop_error = Some("child session identity mismatch".into()),
                    Err(e) => stop_error = Some(e),
                }
            }
            if cancelled.elapsed() >= Duration::from_millis(limits.shutdown_grace_ms) {
                forced = true;
                let _ = owned.0.kill();
                break owned.0.wait().map_err(|e| e.to_string())?;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = output.join().map_err(|_| "stdout reader failed")?;
    let errors = errors.join().map_err(|_| "stderr reader failed")?;
    if session.is_none() {
        session = receive.try_recv().ok();
    }
    let child_report = if output.total <= cap as u64 {
        serde_json::from_slice::<serde_json::Value>(&output.bytes).ok()
    } else {
        None
    };
    let (session_status, session_status_error) = match session
        .as_ref()
        .map(|id| supervision_session::status(&registry, id))
        .transpose()
    {
        Ok(value) => (value, None),
        Err(e) => (None, Some(e)),
    };
    Ok(
        serde_json::json!({"version":"owned-corpus-process-v1","search_id":source,"pid":pid.to_string(),"started_at_ms":started_at.to_string(),"finished_at_ms":chrono::Utc::now().timestamp_millis().to_string(),"elapsed_ms":start.elapsed().as_millis().to_string(),"stop_reason":cancellation.map(|v|v.0).unwrap_or(if status.success(){"child-exited"}else{"child-failed"}),"reaped":true,"forced_kill":forced,"stop_request_delivered":stop_delivered,"stop_error":stop_error,"exit_success":status.success(),"exit_code":status.code().map(|n|n.to_string()),"stdout_bytes":output.total.to_string(),"stderr_bytes":errors.total.to_string(),"stdout_truncated":output.total>cap as u64,"stderr_truncated":errors.total>cap as u64,"stdout_read_error":output.error,"stderr_read_error":errors.error,"child_report":child_report,"session_id":session,"session_status":session_status,"session_status_error":session_status_error,"limits":limits,"limitations":"Explicit foreground parent, same executable only; sampled wall budget plus cooperative grace. Forced termination can leave an unclosed session and fenced work. No hard kernel/filesystem deadline or survival after parent death is claimed."}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_bounds_every_byte_and_preserves_split_handshake() {
        let fault = Arc::new(AtomicBool::new(false));
        let (s, r) = mpsc::sync_channel(1);
        let id = "a".repeat(64);
        let text = format!(
            "{}\n{{\"supervision_session_id\":\"{id}\"}}\n{}",
            "x".repeat(4080),
            "x".repeat(10000)
        );
        let captured = capture(text.as_bytes(), 1024, fault.clone(), Some(s));
        assert_eq!(captured.bytes.len(), 1024);
        assert_eq!(captured.total, text.len() as u64);
        assert!(fault.load(Ordering::Acquire));
        assert_eq!(r.recv().unwrap(), id);
    }
    #[test]
    fn owned_limits_are_bounded() {
        for limits in [
            Limits {
                maximum_wall_ms: 0,
                shutdown_grace_ms: 0,
                maximum_output_bytes: 1024,
            },
            Limits {
                maximum_wall_ms: 1,
                shutdown_grace_ms: 60001,
                maximum_output_bytes: 1024,
            },
            Limits {
                maximum_wall_ms: 1,
                shutdown_grace_ms: 0,
                maximum_output_bytes: 1024 * 1024 + 1,
            },
        ] {
            assert!(limits.validate().is_err());
        }
    }
}
