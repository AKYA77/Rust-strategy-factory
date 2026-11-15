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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_child_rss_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_child_cpu_ticks: Option<u64>,
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        if !(1..=86_400_000).contains(&self.maximum_wall_ms)
            || self.shutdown_grace_ms > 60_000
            || !(1024..=1024 * 1024).contains(&self.maximum_output_bytes)
            || self.maximum_child_rss_bytes == Some(0)
            || self.maximum_child_cpu_ticks == Some(0)
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
/// Exact parent-reviewed inputs, checked by the same-executable child before any registry writes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputPin {
    version: String,
    operator_hash: String,
    effective_operator_hash: String,
    policy_hash: String,
    source_id: String,
    start_index: Option<u64>,
}
impl InputPin {
    pub fn freeze(
        loaded: &operator_config::LoadedSearch,
        policy: &retry_policy::Policy,
        start_index: Option<u64>,
    ) -> Result<Self> {
        policy.validate()?;
        let mut run = loaded.run();
        if let Some(start_index) = start_index {
            run.start_index = start_index;
        }
        let source_id = crate::corpus_search::identity(&run)?;
        let mut effective = loaded.file.clone();
        if let Some(start_index) = start_index {
            effective.start_index = start_index;
        }
        Ok(Self {
            version: "owned-input-pin-v1".into(),
            operator_hash: crate::data::hash(&loaded.file)?,
            effective_operator_hash: crate::data::hash(&effective)?,
            policy_hash: crate::data::hash(policy)?,
            source_id,
            start_index,
        })
    }
    pub(crate) fn validate_binding(
        &self,
        source: &str,
        effective_operator_hash: &str,
        policy_hash: &str,
    ) -> Result<()> {
        if self.version != "owned-input-pin-v1"
            || !crate::machine_evidence::hex(&self.operator_hash)
            || !crate::machine_evidence::hex(&self.effective_operator_hash)
            || !crate::machine_evidence::hex(&self.policy_hash)
            || !crate::machine_evidence::hex(&self.source_id)
            || self.source_id != source
            || self.effective_operator_hash != effective_operator_hash
            || self.policy_hash != policy_hash
            || self.start_index.is_none() && self.operator_hash != self.effective_operator_hash
        {
            return Err("owned input pin binding mismatch".into());
        }
        Ok(())
    }
    pub fn validate_and_apply(
        &self,
        loaded: &mut operator_config::LoadedSearch,
        policy: &retry_policy::Policy,
    ) -> Result<()> {
        let current = Self::freeze(loaded, policy, self.start_index)?;
        if self != &current {
            return Err("owned child inputs differ from reviewed parent snapshot".into());
        }
        if let Some(start_index) = self.start_index {
            loaded.file.start_index = start_index;
        }
        Ok(())
    }
}
pub struct RegionInput<'a> {
    pub config: &'a Path,
    pub start_index: u64,
    pub expected_source_id: &'a str,
}
struct RunInput<'a> {
    config: &'a Path,
    start_index: Option<u64>,
    expected_source_id: Option<&'a str>,
}
pub fn run_region(
    input: RegionInput<'_>,
    policy_path: &Path,
    database: &Path,
    rounds: usize,
    wait_ms: u64,
    limits: &Limits,
    shutdown: Arc<AtomicBool>,
) -> Result<serde_json::Value> {
    run_input(
        RunInput {
            config: input.config,
            start_index: Some(input.start_index),
            expected_source_id: Some(input.expected_source_id),
        },
        policy_path,
        database,
        rounds,
        wait_ms,
        limits,
        shutdown,
    )
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
    run_input(
        RunInput {
            config,
            start_index: None,
            expected_source_id: None,
        },
        policy_path,
        database,
        rounds,
        wait_ms,
        limits,
        shutdown,
    )
}
fn run_input(
    input: RunInput<'_>,
    policy_path: &Path,
    database: &Path,
    rounds: usize,
    wait_ms: u64,
    limits: &Limits,
    shutdown: Arc<AtomicBool>,
) -> Result<serde_json::Value> {
    let config = input.config;
    limits.validate()?;
    if input
        .expected_source_id
        .is_some_and(|id| !crate::machine_evidence::hex(id))
    {
        return Err("invalid expected region source identity".into());
    }
    if !(1..=10000).contains(&rounds) || wait_ms > 3_600_000 {
        return Err("invalid child round/wait limits".into());
    }
    let mut loaded = operator_config::load_search(config)?;
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
    let pin = InputPin::freeze(&loaded, &policy, input.start_index)?;
    if input
        .expected_source_id
        .is_some_and(|id| id != pin.source_id)
    {
        return Err("owned region does not match frozen source identity".into());
    }
    let pin_json = serde_json::to_string(&pin).map_err(|e| e.to_string())?;
    pin.validate_and_apply(&mut loaded, &policy)?;
    let source = crate::corpus_search::prepare_supervision(database, &loaded.run(), &policy)?;
    let mut registry = Registry::open(database)?;
    registry
        .connection
        .busy_timeout(Duration::from_millis(10))
        .map_err(|e| e.to_string())?;
    let invocation = crate::owned_history::start(
        &mut registry,
        &source,
        &crate::data::hash(&loaded.file)?,
        &crate::data::hash(&policy)?,
        rounds,
        wait_ms,
        limits,
    )?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let started_at = chrono::Utc::now().timestamp_millis();
    let start = Instant::now();
    let mut owned = Owned(
        Command::new(executable)
            .arg("corpus-supervise-pinned")
            .arg(config)
            .arg(policy_path)
            .arg(database)
            .arg(rounds.to_string())
            .arg(wait_ms.to_string())
            .arg(pin_json)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?,
    );
    let pid = owned.0.id();
    crate::owned_history::attach(&mut registry, &invocation, pid)?;
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
    let mut sample_at = Instant::now();
    let mut sample_count = 0u64;
    let mut peak_rss = 0u64;
    let mut last_cpu_ticks = None;
    let mut child_start_ticks = None;
    let mut observation_error = None;
    let mut resource_stop = None;
    let mut resource_journal_at = Instant::now();
    let mut resource_journal_error = None;
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
        if Instant::now() >= sample_at && cancellation.is_none() {
            sample_at = Instant::now() + Duration::from_millis(100);
            match crate::resource::observe_process(pid) {
                Ok(sample) if child_start_ticks.is_none_or(|n| n == sample.start_ticks) => {
                    sample_count += 1;
                    child_start_ticks = Some(sample.start_ticks);
                    peak_rss = peak_rss.max(sample.rss_bytes);
                    let cpu = sample.user_ticks + sample.system_ticks;
                    last_cpu_ticks = Some(cpu);
                    observation_error = None;
                    if limits
                        .maximum_child_rss_bytes
                        .is_some_and(|n| sample.rss_bytes >= n)
                    {
                        resource_stop = Some("child-rss-pressure");
                    }
                    if limits.maximum_child_cpu_ticks.is_some_and(|n| cpu >= n) {
                        resource_stop = Some("child-cpu-budget");
                    }
                }
                result => {
                    if let Some(status) = owned.0.try_wait().map_err(|e| e.to_string())? {
                        break status;
                    }
                    observation_error = Some(
                        result
                            .err()
                            .unwrap_or_else(|| "owned child start identity changed".into()),
                    );
                    if limits.maximum_child_rss_bytes.is_some()
                        || limits.maximum_child_cpu_ticks.is_some()
                    {
                        resource_stop = Some("child-resource-observation-unavailable");
                    }
                }
            }
        }
        if Instant::now() >= resource_journal_at {
            resource_journal_at = Instant::now() + Duration::from_secs(1);
            let observation = crate::resource::ProcessObservation {
                samples: sample_count,
                peak_rss_bytes: peak_rss,
                last_cpu_ticks,
                start_ticks: child_start_ticks,
                observed_at_ms: chrono::Utc::now().timestamp_millis(),
                observation_error: observation_error.clone(),
            };
            if let Err(e) =
                crate::owned_history::observe_resources(&mut registry, &invocation, &observation)
            {
                resource_journal_error = Some(e);
                resource_stop = Some("parent-resource-journal-unavailable");
            }
        }
        if cancellation.is_none() {
            let reason = if resource_stop.is_some() {
                resource_stop
            } else if shutdown.load(Ordering::Acquire) {
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
    let observation = crate::resource::ProcessObservation {
        samples: sample_count,
        peak_rss_bytes: peak_rss,
        last_cpu_ticks,
        start_ticks: child_start_ticks,
        observed_at_ms: chrono::Utc::now().timestamp_millis(),
        observation_error: observation_error.clone(),
    };
    if let Err(e) =
        crate::owned_history::observe_resources(&mut registry, &invocation, &observation)
    {
        resource_journal_error = Some(e);
    }
    let mut report = serde_json::json!({"invocation_id":invocation.id,"version":"owned-corpus-process-v1","input_pin":pin,"resource_journal_error":resource_journal_error,"child_resources":{"samples":sample_count.to_string(),"peak_observed_rss_bytes":peak_rss.to_string(),"last_observed_cpu_ticks":last_cpu_ticks.map(|n|n.to_string()),"start_ticks":child_start_ticks.map(|n|n.to_string()),"observation_error":observation_error,"scope":"Owned child only; sampled RSS and native kernel user+system CPU ticks. No tick-to-time conversion, unsampled peak, cgroup aggregate or hard OS limit is implied."},"search_id":source,"pid":pid.to_string(),"started_at_ms":started_at.to_string(),"finished_at_ms":chrono::Utc::now().timestamp_millis().to_string(),"elapsed_ms":start.elapsed().as_millis().to_string(),"stop_reason":cancellation.map(|v|v.0).unwrap_or(if status.success(){"child-exited"}else{"child-failed"}),"reaped":true,"forced_kill":forced,"stop_request_delivered":stop_delivered,"stop_error":stop_error,"exit_success":status.success(),"exit_code":status.code().map(|n|n.to_string()),"stdout_bytes":output.total.to_string(),"stderr_bytes":errors.total.to_string(),"stdout_truncated":output.total>cap as u64,"stderr_truncated":errors.total>cap as u64,"stdout_read_error":output.error,"stderr_read_error":errors.error,"child_report":child_report,"session_id":session,"session_status":session_status,"session_status_error":session_status_error,"limits":limits,"limitations":"Explicit foreground parent, same executable only; sampled wall budget plus cooperative grace. Forced termination can leave an unclosed session and fenced work. No hard kernel/filesystem deadline or survival after parent death is claimed."});

    report["invocation_status"] =
        crate::owned_history::finish(&mut registry, &invocation, &report)?;
    Ok(report)
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
                maximum_wall_ms: 1,
                shutdown_grace_ms: 0,
                maximum_output_bytes: 1024,
                maximum_child_rss_bytes: Some(0),
                maximum_child_cpu_ticks: None,
            },
            Limits {
                maximum_wall_ms: 1,
                shutdown_grace_ms: 0,
                maximum_output_bytes: 1024,
                maximum_child_rss_bytes: None,
                maximum_child_cpu_ticks: Some(0),
            },
            Limits {
                maximum_wall_ms: 0,
                shutdown_grace_ms: 0,
                maximum_child_rss_bytes: None,
                maximum_child_cpu_ticks: None,
                maximum_output_bytes: 1024,
            },
            Limits {
                maximum_wall_ms: 1,
                shutdown_grace_ms: 60001,
                maximum_child_rss_bytes: None,
                maximum_child_cpu_ticks: None,
                maximum_output_bytes: 1024,
            },
            Limits {
                maximum_wall_ms: 1,
                shutdown_grace_ms: 0,
                maximum_child_rss_bytes: None,
                maximum_child_cpu_ticks: None,
                maximum_output_bytes: 1024 * 1024 + 1,
            },
        ] {
            assert!(limits.validate().is_err());
        }
    }
}
