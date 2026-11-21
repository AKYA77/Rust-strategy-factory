//! Linux process/host observations and checkpoint admission backpressure.
//! A sampled threshold is not a hard OS memory/CPU limit.
use crate::Result;
use serde::{Deserialize, Serialize};
use std::io::Read;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Snapshot {
    pub process_rss_bytes: u64,
    pub host_available_memory_bytes: u64,
    pub available_parallelism: usize,
    pub scope: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Guard {
    pub maximum_process_rss_bytes: u64,
    pub minimum_host_available_memory_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Decision {
    pub allowed: bool,
    pub reasons: Vec<String>,
    pub observed: Snapshot,
}
fn kilobytes(text: &str, name: &str) -> Result<u64> {
    if text.len() > 65536 {
        return Err("resource observation exceeds budget".into());
    }
    let mut result = None;
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() != Some(name) {
            continue;
        }
        if result.is_some() {
            return Err("duplicate resource observation".into());
        }
        let n = fields
            .next()
            .ok_or("missing resource quantity")?
            .parse::<u64>()
            .map_err(|_| "invalid resource quantity")?;
        if fields.next() != Some("kB") || fields.next().is_some() {
            return Err("unsupported resource unit".into());
        }
        result = Some(n.checked_mul(1024).ok_or("resource observation overflow")?);
    }
    result.ok_or_else(|| "required resource observation unavailable".into())
}
fn bounded_read(path: &str) -> Result<String> {
    let file =
        std::fs::File::open(path).map_err(|e| format!("resource observation unavailable: {e}"))?;
    let mut text = String::new();
    file.take(65537)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() > 65536 {
        return Err("resource observation exceeds budget".into());
    }
    Ok(text)
}
pub fn observe() -> Result<Snapshot> {
    let status = bounded_read("/proc/self/status")?;
    let memory = bounded_read("/proc/meminfo")?;
    Ok(Snapshot{process_rss_bytes:kilobytes(&status,"VmRSS:")?,host_available_memory_bytes:kilobytes(&memory,"MemAvailable:")?,available_parallelism:std::thread::available_parallelism().map_err(|e|e.to_string())?.get(),scope:"Current process RSS and host available memory; host availability is not a cgroup allocation or a worker-process inventory. Sampled observations are not hard OS enforcement.".into()})
}
impl Guard {
    pub fn validate(&self) -> Result<()> {
        if self.maximum_process_rss_bytes == 0 {
            return Err("resource RSS threshold must be positive".into());
        }
        Ok(())
    }
    pub fn assess(&self, observed: Snapshot) -> Result<Decision> {
        self.validate()?;
        let mut reasons = Vec::new();
        if observed.process_rss_bytes >= self.maximum_process_rss_bytes {
            reasons.push("process-rss-at-or-above-threshold".into());
        }
        if observed.host_available_memory_bytes < self.minimum_host_available_memory_bytes {
            reasons.push("host-memory-below-reserve".into());
        }
        Ok(Decision {
            allowed: reasons.is_empty(),
            reasons,
            observed,
        })
    }
    pub fn check(&self) -> Result<Decision> {
        self.assess(observe()?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_observations_require_exact_fields_units_and_checked_bytes() {
        assert_eq!(
            kilobytes("Name: app\nVmRSS:  42 kB\n", "VmRSS:").unwrap(),
            43008
        );
        for bad in [
            "VmRSS: 1 B",
            "VmRSS: -1 kB",
            "VmRSS: 18446744073709551615 kB",
            "VmRSS: 1 kB\nVmRSS: 2 kB",
            "VmRSS: 1 kB extra",
            "Other: 1 kB",
        ] {
            assert!(kilobytes(bad, "VmRSS:").is_err());
        }
        assert!(kilobytes(&"x".repeat(65537), "VmRSS:").is_err());
    }
    #[test]
    fn pressure_boundaries_preserve_unknown_and_do_not_claim_hard_enforcement() {
        let guard = Guard {
            maximum_process_rss_bytes: 1024,
            minimum_host_available_memory_bytes: 4096,
        };
        let sample = Snapshot {
            process_rss_bytes: 1023,
            host_available_memory_bytes: 4096,
            available_parallelism: 2,
            scope: "fixture".into(),
        };
        assert!(guard.assess(sample.clone()).unwrap().allowed);
        let mut at = sample.clone();
        at.process_rss_bytes = 1024;
        let blocked = guard.assess(at).unwrap();
        assert!(!blocked.allowed);
        assert_eq!(blocked.reasons, vec!["process-rss-at-or-above-threshold"]);
        let mut low = sample;
        low.host_available_memory_bytes = 4095;
        assert!(!guard.assess(low).unwrap().allowed);
        assert!(
            Guard {
                maximum_process_rss_bytes: 0,
                minimum_host_available_memory_bytes: 0
            }
            .validate()
            .is_err()
        );
    }
}

/// Linux process counters. Clock ticks are raw kernel accounting units, not milliseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSample {
    pub pid: u32,
    pub rss_bytes: u64,
    pub user_ticks: u64,
    pub system_ticks: u64,
    pub start_ticks: u64,
}
fn process_stat(text: &str, pid: u32, rss: u64) -> Result<ProcessSample> {
    let (user_ticks, system_ticks, start_ticks) = stat_counters(text, pid)?;
    Ok(ProcessSample {
        pid,
        rss_bytes: rss,
        user_ticks,
        system_ticks,
        start_ticks,
    })
}
pub fn observe_process(pid: u32) -> Result<ProcessSample> {
    if pid == 0 {
        return Err("process pid must be positive".into());
    }
    let stat_path = format!("/proc/{pid}/stat");
    let before = bounded_read(&stat_path)?;
    let status = bounded_read(&format!("/proc/{pid}/status"))?;
    let sample = process_stat(&before, pid, kilobytes(&status, "VmRSS:")?)?;
    let after = process_stat(&bounded_read(&stat_path)?, pid, sample.rss_bytes)?;
    if after.start_ticks != sample.start_ticks {
        return Err("process identity changed during observation".into());
    }
    Ok(sample)
}
#[cfg(test)]
mod process_tests {
    use super::*;
    fn stat() -> String {
        let mut fields = vec!["0"; 20];
        fields[0] = "S";
        fields[11] = "18446744073709551600";
        fields[12] = "10";
        fields[19] = "42";
        format!("123 (odd ) name) {}", fields.join(" "))
    }
    #[test]
    fn process_counters_preserve_command_parentheses_full_u64_and_exact_identity() {
        let s = process_stat(&stat(), 123, 4096).unwrap();
        assert_eq!(s.user_ticks, 18446744073709551600);
        assert_eq!(s.system_ticks, 10);
        assert_eq!(s.start_ticks, 42);
        assert_eq!(s.rss_bytes, 4096);
        assert!(process_stat(&stat(), 124, 4096).is_err());
        assert!(
            process_stat(
                &stat().replace("18446744073709551600", "18446744073709551615"),
                123,
                4096
            )
            .is_err()
        );
        assert!(process_stat("123 bad", 123, 4096).is_err());
        assert!(process_stat(&"x".repeat(65537), 123, 4096).is_err());
    }
    #[test]
    fn actual_current_process_rss_and_cpu_observations_are_positive_and_stable() {
        let a = observe_process(std::process::id()).unwrap();
        let b = observe_process(std::process::id()).unwrap();
        assert!(a.rss_bytes > 0);
        assert_eq!(a.start_ticks, b.start_ticks);
        assert!(b.user_ticks + b.system_ticks >= a.user_ticks + a.system_ticks);
        assert!(observe_process(0).is_err());
        assert!(observe_process(u32::MAX).is_err());
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessObservation {
    pub samples: u64,
    pub peak_rss_bytes: u64,
    pub last_cpu_ticks: Option<u64>,
    pub start_ticks: Option<u64>,
    pub observed_at_ms: i64,
    pub observation_error: Option<String>,
}
impl ProcessObservation {
    pub fn validate(&self) -> Result<()> {
        if self.samples > 1_000_000
            || chrono::DateTime::from_timestamp_millis(self.observed_at_ms).is_none()
            || self
                .observation_error
                .as_ref()
                .is_some_and(|s| s.chars().count() > 256)
            || (self.samples == 0
                && (self.start_ticks.is_some()
                    || self.last_cpu_ticks.is_some()
                    || self.peak_rss_bytes != 0))
            || (self.samples > 0 && (self.start_ticks.is_none() || self.last_cpu_ticks.is_none()))
        {
            return Err("invalid owned resource observation".into());
        }
        Ok(())
    }
    pub fn exact(&self) -> Result<serde_json::Value> {
        self.validate()?;
        Ok(
            serde_json::json!({"samples":self.samples.to_string(),"peak_rss_bytes":self.peak_rss_bytes.to_string(),"last_cpu_ticks":self.last_cpu_ticks.map(|n|n.to_string()),"start_ticks":self.start_ticks.map(|n|n.to_string()),"observed_at_ms":self.observed_at_ms.to_string(),"observation_error":self.observation_error}),
        )
    }
}

/// Kernel high-water RSS for the current process; not an aggregate or an enforced limit.
pub fn observe_peak_rss() -> Result<u64> {
    kilobytes(&bounded_read("/proc/self/status")?, "VmHWM:")
}

/// Aggregate host CPU counters from /proc/stat, in native kernel clock ticks.
/// These are machine-wide counters as visible in this mount namespace, not a
/// cgroup allocation; no tick-to-time conversion is implied or performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostCpu {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}
impl HostCpu {
    fn as_array(self) -> [u64; 8] {
        [
            self.user,
            self.nice,
            self.system,
            self.idle,
            self.iowait,
            self.irq,
            self.softirq,
            self.steal,
        ]
    }
    fn zero() -> Self {
        Self {
            user: 0,
            nice: 0,
            system: 0,
            idle: 0,
            iowait: 0,
            irq: 0,
            softirq: 0,
            steal: 0,
        }
    }
    /// Busy excludes idle and iowait; checked sums, never wrapping.
    pub fn busy_ticks(&self) -> Result<u64> {
        [
            self.user,
            self.nice,
            self.system,
            self.irq,
            self.softirq,
            self.steal,
        ]
        .iter()
        .try_fold(0u64, |acc, n| acc.checked_add(*n))
        .ok_or_else(|| "host CPU busy overflow".into())
    }
    pub fn total_ticks(&self) -> Result<u64> {
        self.as_array()
            .iter()
            .try_fold(0u64, |acc, n| acc.checked_add(*n))
            .ok_or_else(|| "host CPU total overflow".into())
    }
}
pub(crate) fn host_cpu(text: &str) -> Result<HostCpu> {
    if text.len() > 65536 {
        return Err("resource observation exceeds budget".into());
    }
    let mut result = None;
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() != Some("cpu") {
            continue;
        }
        if result.is_some() {
            return Err("duplicate host CPU observation".into());
        }
        let mut values = [0u64; 8];
        for slot in &mut values {
            *slot = fields
                .next()
                .ok_or("missing host CPU counter")?
                .parse::<u64>()
                .map_err(|_| "invalid host CPU counter")?;
        }
        // Kernels may append guest and guest_nice (fields 9/10); the kernel
        // already accounts them inside user/nice, so they are accepted and
        // ignored. Anything beyond those is unsupported.
        let mut extra = 0;
        for value in fields {
            extra += 1;
            if extra > 2 {
                return Err("unexpected host CPU counter".into());
            }
            value
                .parse::<u64>()
                .map_err(|_| "invalid host CPU counter")?;
        }
        result = Some(HostCpu {
            user: values[0],
            nice: values[1],
            system: values[2],
            idle: values[3],
            iowait: values[4],
            irq: values[5],
            softirq: values[6],
            steal: values[7],
        });
    }
    result.ok_or_else(|| "required host CPU observation unavailable".into())
}
pub fn observe_host_cpu() -> Result<HostCpu> {
    host_cpu(&bounded_read("/proc/stat")?)
}
/// Two host observations; deltas require every counter to be monotonic.
#[derive(Debug, Clone, Copy)]
pub struct HostCpuWindow {
    pub before: HostCpu,
    pub after: HostCpu,
}
impl HostCpuWindow {
    fn deltas(&self) -> Result<(u64, u64)> {
        let before = self.before.as_array();
        let after = self.after.as_array();
        let mut busy = 0u64;
        let mut total = 0u64;
        for (index, (b, a)) in before.iter().zip(after.iter()).enumerate() {
            let delta = a.checked_sub(*b).ok_or("host CPU counters regressed")?;
            total = total.checked_add(delta).ok_or("host CPU delta overflow")?;
            if index != 3 && index != 4 {
                busy = busy.checked_add(delta).ok_or("host CPU delta overflow")?;
            }
        }
        Ok((busy, total))
    }
    pub fn busy_delta(&self) -> Result<u64> {
        Ok(self.deltas()?.0)
    }
    pub fn total_delta(&self) -> Result<u64> {
        Ok(self.deltas()?.1)
    }
}

/// Per-thread CPU counters from /proc/self/task/<tid>/stat (same format as process stat).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadCpu {
    pub tid: u32,
    pub user_ticks: u64,
    pub system_ticks: u64,
    pub start_ticks: u64,
}
fn stat_counters(text: &str, id: u32) -> Result<(u64, u64, u64)> {
    if text.len() > 65536 || id == 0 {
        return Err("invalid process observation bound".into());
    }
    let (prefix, fields) = text
        .rsplit_once(") ")
        .ok_or("process stat command terminator missing")?;
    let observed = prefix
        .split_once(" (")
        .ok_or("process stat identity missing")?
        .0
        .parse::<u32>()
        .map_err(|_| "process stat pid invalid")?;
    if observed != id {
        return Err("process stat identity mismatch".into());
    }
    let fields = fields.split_whitespace().collect::<Vec<_>>();
    let number = |index: usize| -> Result<u64> {
        fields
            .get(index)
            .ok_or("short process stat")?
            .parse::<u64>()
            .map_err(|_| "invalid process stat counter".into())
    };
    let user_ticks = number(11)?;
    let system_ticks = number(12)?;
    user_ticks
        .checked_add(system_ticks)
        .ok_or("process CPU tick overflow")?;
    Ok((user_ticks, system_ticks, number(19)?))
}
pub(crate) fn thread_stat(text: &str, tid: u32) -> Result<ThreadCpu> {
    let (user_ticks, system_ticks, start_ticks) = stat_counters(text, tid)?;
    Ok(ThreadCpu {
        tid,
        user_ticks,
        system_ticks,
        start_ticks,
    })
}
pub fn observe_thread_cpu(tid: u32) -> Result<ThreadCpu> {
    if tid == 0 {
        return Err("thread tid must be positive".into());
    }
    let path = format!("/proc/self/task/{tid}/stat");
    let sample = thread_stat(&bounded_read(&path)?, tid)?;
    let after = thread_stat(&bounded_read(&path)?, tid)?;
    if after.start_ticks != sample.start_ticks {
        return Err("thread identity changed during observation".into());
    }
    Ok(sample)
}
/// Live threads whose comm starts with the prefix (kernel comm is capped at 15 bytes).
/// Threads that exit mid-enumeration are skipped, never fabricated.
pub fn named_worker_threads(prefix: &str) -> Result<Vec<(u32, String)>> {
    if prefix.is_empty() || prefix.len() > 15 {
        return Err("invalid worker thread name prefix".into());
    }
    let entries = std::fs::read_dir("/proc/self/task")
        .map_err(|e| format!("task enumeration unavailable: {e}"))?;
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(tid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if tid == 0 {
            continue;
        }
        let Ok(comm) = bounded_read(&format!("/proc/self/task/{tid}/comm")) else {
            continue;
        };
        let comm = comm.trim_end_matches('\n');
        if comm.starts_with(prefix) {
            found.push((tid, comm.to_string()));
            if found.len() > 4096 {
                return Err("worker thread inventory exceeds budget".into());
            }
        }
    }
    found.sort_unstable_by_key(|(tid, _)| *tid);
    Ok(found)
}

/// One simultaneous observation of host counters and live worker-thread counters.
#[derive(Debug, Clone)]
pub struct AllocationSample {
    pub host: HostCpu,
    pub workers: Vec<ThreadCpu>,
    pub observation_error: Option<String>,
}
fn observe_allocation(prefix: &str) -> AllocationSample {
    let host = match observe_host_cpu() {
        Ok(host) => host,
        Err(error) => {
            return AllocationSample {
                host: HostCpu::zero(),
                workers: Vec::new(),
                observation_error: Some(error),
            };
        }
    };
    let mut workers = Vec::new();
    let mut observation_error = None;
    match named_worker_threads(prefix) {
        Ok(list) => {
            for (tid, _) in list {
                match observe_thread_cpu(tid) {
                    Ok(sample) => workers.push(sample),
                    Err(error) => observation_error = Some(error),
                }
            }
        }
        Err(error) => observation_error = Some(error),
    }
    AllocationSample {
        host,
        workers,
        observation_error,
    }
}
/// Measured per-worker CPU allocation over a window. Shares are exact ratios of
/// native kernel tick deltas; unknown stays unknown and shares are never equalised.
#[derive(Debug, Clone, Serialize)]
pub struct WorkerAllocation {
    pub tid: u32,
    pub status: String,
    pub cpu_ticks_delta: Option<u64>,
    pub share_of_host_total: Option<(u64, u64)>,
    pub share_of_host_busy: Option<(u64, u64)>,
    pub reason: Option<String>,
}
#[derive(Debug, Clone)]
pub struct AllocationWindow {
    pub host_total_delta: Option<u64>,
    pub host_busy_delta: Option<u64>,
    pub workers: Vec<WorkerAllocation>,
    pub scope: String,
}
const ALLOCATION_SCOPE: &str = "Native kernel user+system CPU ticks sampled from procfs; shares are exact tick ratios of the observed window, not time conversions, not cgroup allocations, not equal CPU shares and not hard OS enforcement. The window brackets the first/last samples that observed live worker threads; up to one sampling interval of head/tail CPU lies outside it.";
impl AllocationWindow {
    pub fn between(start: &AllocationSample, end: &AllocationSample) -> Self {
        let tainted = start.observation_error.is_some() || end.observation_error.is_some();
        let host_window = HostCpuWindow {
            before: start.host,
            after: end.host,
        };
        // A regressed host window invalidates the whole observation window:
        // worker deltas from the same anomalous window are not trusted either.
        let host_deltas = if tainted {
            None
        } else {
            host_window.deltas().ok()
        };
        let (host_busy_delta, host_total_delta) = match host_deltas {
            Some((busy, total)) => (Some(busy), Some(total)),
            None => (None, None),
        };
        let window_valid = host_deltas.is_some();
        let mut tids = std::collections::BTreeSet::new();
        for sample in [start, end] {
            for worker in &sample.workers {
                tids.insert(worker.tid);
            }
        }
        let mut workers = Vec::new();
        for tid in tids {
            let before = start.workers.iter().find(|w| w.tid == tid);
            let after = end.workers.iter().find(|w| w.tid == tid);
            let mut allocation = WorkerAllocation {
                tid,
                status: "unknown".into(),
                cpu_ticks_delta: None,
                share_of_host_total: None,
                share_of_host_busy: None,
                reason: None,
            };
            if !window_valid {
                allocation.reason = Some(
                    if tainted {
                        "observation-unavailable"
                    } else {
                        "host-counters-regressed"
                    }
                    .into(),
                );
                workers.push(allocation);
                continue;
            }
            let (Some(before), Some(after)) = (before, after) else {
                allocation.reason = Some(
                    if before.is_none() {
                        "worker-not-observed-at-start"
                    } else {
                        "worker-not-observed-at-end"
                    }
                    .into(),
                );
                workers.push(allocation);
                continue;
            };
            if before.start_ticks != after.start_ticks {
                allocation.reason = Some("worker-identity-changed".into());
                workers.push(allocation);
                continue;
            }
            let before_cpu = before.user_ticks + before.system_ticks;
            let after_cpu = after.user_ticks + after.system_ticks;
            let Some(delta) = after_cpu.checked_sub(before_cpu) else {
                allocation.reason = Some("worker-counters-regressed".into());
                workers.push(allocation);
                continue;
            };
            allocation.status = "observed".into();
            allocation.cpu_ticks_delta = Some(delta);
            match host_total_delta {
                Some(total) if total > 0 => {
                    allocation.share_of_host_total = Some((delta, total));
                }
                Some(0) => allocation.reason = Some("host-window-zero".into()),
                _ => allocation.reason = Some("host-window-unknown".into()),
            }
            match host_busy_delta {
                Some(busy) if busy > 0 => {
                    allocation.share_of_host_busy = Some((delta, busy));
                }
                Some(0) => {
                    if allocation.reason.is_none() {
                        allocation.reason = Some("host-busy-window-zero".into());
                    }
                }
                _ => {
                    if allocation.reason.is_none() {
                        allocation.reason = Some("host-window-unknown".into());
                    }
                }
            }
            workers.push(allocation);
        }
        Self {
            host_total_delta,
            host_busy_delta,
            workers,
            scope: ALLOCATION_SCOPE.into(),
        }
    }
    /// Bracket the window with the first/last samples that actually observed workers;
    /// with no worker samples the host window spans the whole series.
    pub fn from_samples(samples: &[AllocationSample]) -> Self {
        let Some(first) = samples.first() else {
            return Self {
                host_total_delta: None,
                host_busy_delta: None,
                workers: Vec::new(),
                scope: ALLOCATION_SCOPE.into(),
            };
        };
        let last = samples.last().unwrap_or(first);
        let with_workers: Vec<usize> = samples
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.workers.is_empty())
            .map(|(i, _)| i)
            .collect();
        match (with_workers.first(), with_workers.last()) {
            (Some(start), Some(end)) => Self::between(&samples[*start], &samples[*end]),
            _ => Self::between(first, last),
        }
    }
    pub fn exact(&self) -> serde_json::Value {
        let share = |s: Option<(u64, u64)>| {
            s.map(|(n, d)| {
                serde_json::json!({"numerator": n.to_string(), "denominator": d.to_string()})
            })
        };
        serde_json::json!({
            "host_total_delta": self.host_total_delta.map(|n| n.to_string()),
            "host_busy_delta": self.host_busy_delta.map(|n| n.to_string()),
            "workers": self.workers.iter().map(|w| serde_json::json!({
                "tid": w.tid.to_string(),
                "status": w.status,
                "cpu_ticks_delta": w.cpu_ticks_delta.map(|n| n.to_string()),
                "share_of_host_total": share(w.share_of_host_total),
                "share_of_host_busy": share(w.share_of_host_busy),
                "reason": w.reason,
            })).collect::<Vec<_>>(),
            "scope": self.scope,
        })
    }
}

/// Bounded background sampler of live worker threads; finish() joins and returns
/// every sample taken between spawn and finish (at most 20000).
pub struct SamplerHandle {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: std::thread::JoinHandle<Vec<AllocationSample>>,
}
pub fn spawn_sampler(prefix: &str, interval_ms: u64) -> Result<SamplerHandle> {
    if prefix.is_empty() || prefix.len() > 15 || !(10..=1000).contains(&interval_ms) {
        return Err("invalid worker sampler bounds".into());
    }
    let prefix = prefix.to_string();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stop.clone();
    let handle = std::thread::Builder::new()
        .name("rsf-alloc-sampler".into())
        .spawn(move || {
            let mut samples = Vec::new();
            let mut first = true;
            while !flag.load(std::sync::atomic::Ordering::Acquire) && samples.len() < 20_000 {
                if !first {
                    std::thread::sleep(std::time::Duration::from_millis(interval_ms));
                }
                first = false;
                samples.push(observe_allocation(&prefix));
            }
            samples
        })
        .map_err(|e| e.to_string())?;
    Ok(SamplerHandle { stop, handle })
}
impl SamplerHandle {
    pub fn finish(self) -> Result<Vec<AllocationSample>> {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        self.handle
            .join()
            .map_err(|_| "worker sampler thread failed".into())
    }
}

/// Sampled host CPU-share admission. A refusal is a scheduling decision, not a
/// hard OS limit; observation failure fails closed at the caller.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CpuGuard {
    pub maximum_host_busy_share_numerator: u64,
    pub maximum_host_busy_share_denominator: u64,
    pub observation_window_ms: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct CpuDecision {
    pub allowed: bool,
    pub reasons: Vec<String>,
    pub busy_delta: Option<u64>,
    pub total_delta: Option<u64>,
    pub scope: String,
}
impl CpuGuard {
    pub fn validate(&self) -> Result<()> {
        if self.maximum_host_busy_share_denominator == 0
            || self.maximum_host_busy_share_numerator > self.maximum_host_busy_share_denominator
            || !(10..=1000).contains(&self.observation_window_ms)
        {
            return Err("invalid CPU guard bounds".into());
        }
        Ok(())
    }
    pub fn assess(&self, window: HostCpuWindow) -> Result<CpuDecision> {
        self.validate()?;
        let mut reasons = Vec::new();
        let (busy_delta, total_delta) = match window.deltas() {
            Ok((busy, total)) if total > 0 => {
                // busy/total >= num/den iff busy*den >= total*num, in u128.
                if (busy as u128) * (self.maximum_host_busy_share_denominator as u128)
                    >= (total as u128) * (self.maximum_host_busy_share_numerator as u128)
                {
                    reasons.push("host-cpu-busy-at-or-above-threshold".into());
                }
                (Some(busy), Some(total))
            }
            Ok((busy, total)) => {
                reasons.push("host-cpu-window-empty".into());
                (Some(busy), Some(total))
            }
            Err(_) => {
                reasons.push("host-cpu-counters-regressed".into());
                (None, None)
            }
        };
        Ok(CpuDecision {
            allowed: reasons.is_empty(),
            reasons,
            busy_delta,
            total_delta,
            scope: "Sampled host-wide busy share of native kernel CPU ticks over the configured window; a scheduling threshold, not a cgroup limit or hard OS enforcement.".into(),
        })
    }
    pub fn check(&self) -> Result<CpuDecision> {
        self.validate()?;
        let before = observe_host_cpu()?;
        std::thread::sleep(std::time::Duration::from_millis(self.observation_window_ms));
        let after = observe_host_cpu()?;
        self.assess(HostCpuWindow { before, after })
    }
}

#[cfg(test)]
mod host_cpu_tests {
    use super::*;
    const STAT: &str = "cpu  100 20 30 4000 50 6 7 8\ncpu0 50 10 15 2000 25 3 4 4\ncpu1 50 10 15 2000 25 3 4 4\nintr 1\nctxt 2\n";
    #[test]
    fn host_cpu_parses_exact_aggregate_line_and_rejects_malformed() {
        let cpu = host_cpu(STAT).unwrap();
        assert_eq!(
            cpu,
            HostCpu {
                user: 100,
                nice: 20,
                system: 30,
                idle: 4000,
                iowait: 50,
                irq: 6,
                softirq: 7,
                steal: 8
            }
        );
        // Kernels append guest/guest_nice (already counted in user/nice); the
        // first eight counters must parse identically with 9 or 10 fields.
        assert_eq!(host_cpu("cpu  100 20 30 4000 50 6 7 8 0\n").unwrap(), cpu);
        assert_eq!(host_cpu("cpu  100 20 30 4000 50 6 7 8 0 0\n").unwrap(), cpu);
        for bad in [
            "cpu0 1 2 3 4 5 6 7 8\n",
            "cpu  1 2 3 4 5 6 7\n",
            "cpu  1 2 3 4 5 6 7 8 9 10 11\n",
            "cpu  1 2 3 4 5 6 7 x\n",
            "cpu  1 2 3 4 5 6 7 8 x\n",
            "cpu  -1 2 3 4 5 6 7 8\n",
            "cpu  1 2 3 4 5 6 7 8\ncpu  1 2 3 4 5 6 7 8\n",
            "",
        ] {
            assert!(host_cpu(bad).is_err(), "accepted {bad:?}");
        }
        assert!(host_cpu(&"x".repeat(65537)).is_err());
    }
    #[test]
    fn host_cpu_busy_and_total_are_checked_sums() {
        let cpu = host_cpu(STAT).unwrap();
        assert_eq!(cpu.busy_ticks().unwrap(), 100 + 20 + 30 + 6 + 7 + 8);
        assert_eq!(
            cpu.total_ticks().unwrap(),
            100 + 20 + 30 + 4000 + 50 + 6 + 7 + 8
        );
        let max = HostCpu {
            user: u64::MAX,
            nice: 1,
            system: 0,
            idle: 0,
            iowait: 0,
            irq: 0,
            softirq: 0,
            steal: 0,
        };
        assert!(max.busy_ticks().is_err());
        assert!(max.total_ticks().is_err());
    }
    #[test]
    fn host_cpu_window_requires_monotonic_counters() {
        let before = host_cpu(STAT).unwrap();
        let after = HostCpu {
            user: 150,
            nice: 20,
            system: 40,
            idle: 4200,
            iowait: 50,
            irq: 6,
            softirq: 7,
            steal: 8,
        };
        let window = HostCpuWindow { before, after };
        assert_eq!(window.busy_delta().unwrap(), 60);
        assert_eq!(window.total_delta().unwrap(), 260);
        let regressed = HostCpuWindow {
            before: after,
            after: before,
        };
        assert!(regressed.busy_delta().is_err());
        assert!(regressed.total_delta().is_err());
        let same = HostCpuWindow {
            before,
            after: before,
        };
        assert_eq!(same.busy_delta().unwrap(), 0);
        assert_eq!(same.total_delta().unwrap(), 0);
    }
    #[test]
    fn observe_host_cpu_returns_positive_monotonic_totals() {
        let a = observe_host_cpu().unwrap();
        let b = observe_host_cpu().unwrap();
        assert!(a.total_ticks().unwrap() > 0);
        assert!(a.busy_ticks().unwrap() <= a.total_ticks().unwrap());
        let window = HostCpuWindow {
            before: a,
            after: b,
        };
        // Host counters never regress; the delta must be computable.
        assert!(window.total_delta().is_ok());
    }
}

/// Sets the stop flag on EVERY scope exit path, including panic unwind, so the
/// scoped workers can never be joined while still spinning on done=false.
#[cfg(test)]
struct DoneOnDrop(std::sync::Arc<std::sync::atomic::AtomicBool>);
#[cfg(test)]
impl Drop for DoneOnDrop {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
mod thread_cpu_tests {
    use super::*;
    fn stat(tid: u32) -> String {
        let mut fields = vec!["0"; 20];
        fields[0] = "S";
        fields[11] = "18446744073709551600";
        fields[12] = "10";
        fields[19] = "42";
        format!("{tid} (odd ) name) {}", fields.join(" "))
    }
    #[test]
    fn thread_stat_reuses_strict_identity_and_full_u64_counters() {
        let s = thread_stat(&stat(777), 777).unwrap();
        assert_eq!(s.tid, 777);
        assert_eq!(s.user_ticks, 18446744073709551600);
        assert_eq!(s.system_ticks, 10);
        assert_eq!(s.start_ticks, 42);
        assert!(thread_stat(&stat(777), 778).is_err());
        assert!(thread_stat("777 bad", 777).is_err());
        assert!(thread_stat(&"x".repeat(65537), 777).is_err());
        assert!(thread_stat(&stat(0), 0).is_err());
    }
    #[test]
    fn observe_thread_cpu_reads_the_current_main_thread_monotonically() {
        let pid = std::process::id();
        let a = observe_thread_cpu(pid).unwrap();
        let b = observe_thread_cpu(pid).unwrap();
        assert_eq!(a.tid, pid);
        assert!(a.start_ticks > 0);
        assert_eq!(a.start_ticks, b.start_ticks);
        assert!(b.user_ticks + b.system_ticks >= a.user_ticks + a.system_ticks);
        assert!(observe_thread_cpu(0).is_err());
        assert!(observe_thread_cpu(u32::MAX).is_err());
    }
    #[test]
    fn named_worker_threads_find_exact_live_prefix_matches_only() {
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            // Panic-safe: any unwind below still releases the workers.
            let _release = DoneOnDrop(done.clone());
            for i in 0..3 {
                let done = done.clone();
                let send = send.clone();
                std::thread::Builder::new()
                    .name(format!("rsf-test-w{i}"))
                    .spawn_scoped(scope, move || {
                        // comm can lag thread start; retry TIME-bounded well inside
                        // the parent's 15s recv deadline, then report the result.
                        let deadline =
                            std::time::Instant::now() + std::time::Duration::from_secs(5);
                        let mut own = Vec::new();
                        while std::time::Instant::now() < deadline {
                            own = named_worker_threads(&format!("rsf-test-w{i}")).unwrap();
                            if own.len() == 1 {
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        send.send(own).unwrap();
                        while !done.load(std::sync::atomic::Ordering::SeqCst) {
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                    })
                    .unwrap();
            }
            drop(send);
            // Exactly three readiness signals, each bounded so a failed worker
            // surfaces as an assertion instead of an infinite wait.
            let mut expected: Vec<u32> = Vec::new();
            for _ in 0..3 {
                let own = receive
                    .recv_timeout(std::time::Duration::from_secs(15))
                    .unwrap();
                assert_eq!(own.len(), 1, "worker never enumerated its own comm");
                expected.push(own[0].0);
            }
            expected.sort_unstable();
            assert_eq!(expected.len(), 3);
            let found = named_worker_threads("rsf-test-w").unwrap();
            let mut tids: Vec<u32> = found.iter().map(|(tid, _)| *tid).collect();
            tids.sort_unstable();
            assert_eq!(tids, expected);
            for (_, name) in &found {
                assert!(name.starts_with("rsf-test-w"));
            }
            assert!(named_worker_threads("rsf-absent").unwrap().is_empty());
            done.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        assert!(named_worker_threads("").is_err());
        assert!(named_worker_threads("x".repeat(16).as_str()).is_err());
    }
}

#[cfg(test)]
mod allocation_tests {
    use super::*;
    fn host(user: u64, idle: u64) -> HostCpu {
        HostCpu {
            user,
            nice: 0,
            system: 0,
            idle,
            iowait: 0,
            irq: 0,
            softirq: 0,
            steal: 0,
        }
    }
    fn worker(tid: u32, user: u64, start: u64) -> ThreadCpu {
        ThreadCpu {
            tid,
            user_ticks: user,
            system_ticks: 0,
            start_ticks: start,
        }
    }
    fn sample(host: HostCpu, workers: Vec<ThreadCpu>) -> AllocationSample {
        AllocationSample {
            host,
            workers,
            observation_error: None,
        }
    }
    #[test]
    fn allocation_reports_exact_unequal_shares_without_equalising() {
        let start = sample(host(100, 4000), vec![worker(1, 10, 42), worker(2, 20, 43)]);
        let end = sample(host(160, 4200), vec![worker(1, 40, 42), worker(2, 110, 43)]);
        let window = AllocationWindow::between(&start, &end);
        assert_eq!(window.host_total_delta, Some(260));
        assert_eq!(window.host_busy_delta, Some(60));
        assert_eq!(window.workers.len(), 2);
        let one = &window.workers[0];
        assert_eq!(one.status, "observed");
        assert_eq!(one.cpu_ticks_delta, Some(30));
        assert_eq!(one.share_of_host_total, Some((30, 260)));
        assert_eq!(one.share_of_host_busy, Some((30, 60)));
        let two = &window.workers[1];
        assert_eq!(two.cpu_ticks_delta, Some(90));
        assert_eq!(two.share_of_host_total, Some((90, 260)));
        assert_ne!(one.share_of_host_total, two.share_of_host_total);
    }
    #[test]
    fn allocation_keeps_unknown_workers_unknown() {
        let start = sample(
            host(100, 4000),
            vec![worker(1, 10, 42), worker(2, 20, 43), worker(3, 5, 44)],
        );
        // worker 1 vanished, worker 2 identity changed, worker 3 counters regressed,
        // worker 4 only appears at the end.
        let end = sample(
            host(160, 4200),
            vec![worker(2, 110, 99), worker(3, 4, 44), worker(4, 7, 45)],
        );
        let window = AllocationWindow::between(&start, &end);
        assert_eq!(window.workers.len(), 4);
        for w in &window.workers {
            assert_eq!(w.status, "unknown", "worker {} {:?}", w.tid, w.reason);
            assert_eq!(w.cpu_ticks_delta, None);
            assert_eq!(w.share_of_host_total, None);
            assert!(w.reason.is_some());
        }
        let reasons: Vec<&str> = window
            .workers
            .iter()
            .map(|w| w.reason.as_deref().unwrap())
            .collect();
        assert!(reasons.contains(&"worker-not-observed-at-end"));
        assert!(reasons.contains(&"worker-identity-changed"));
        assert!(reasons.contains(&"worker-counters-regressed"));
        assert!(reasons.contains(&"worker-not-observed-at-start"));
    }
    #[test]
    fn allocation_zero_host_window_and_observation_errors_stay_unknown() {
        let start = sample(host(100, 4000), vec![worker(1, 10, 42)]);
        let same = sample(host(100, 4000), vec![worker(1, 10, 42)]);
        let window = AllocationWindow::between(&start, &same);
        assert_eq!(window.host_total_delta, Some(0));
        assert_eq!(window.workers[0].status, "observed");
        assert_eq!(window.workers[0].cpu_ticks_delta, Some(0));
        assert_eq!(window.workers[0].share_of_host_total, None);
        assert_eq!(
            window.workers[0].reason.as_deref(),
            Some("host-window-zero")
        );
        let mut errored = sample(host(160, 4200), vec![worker(1, 40, 42)]);
        errored.observation_error = Some("procfs unavailable".into());
        let window = AllocationWindow::between(&start, &errored);
        assert_eq!(window.host_total_delta, None);
        assert_eq!(window.host_busy_delta, None);
        assert_eq!(window.workers[0].status, "unknown");
        let regressed = sample(host(90, 3999), vec![worker(1, 40, 42)]);
        let window = AllocationWindow::between(&start, &regressed);
        assert_eq!(window.host_total_delta, None);
        assert_eq!(window.workers[0].status, "unknown");
    }
    #[test]
    fn from_samples_brackets_the_observed_worker_lifetime() {
        // Workers appear after the first sample and vanish before the last:
        // the window must bracket first/last samples that actually saw them.
        let samples = vec![
            sample(host(100, 4000), vec![]),
            sample(host(110, 4010), vec![worker(1, 10, 42)]),
            sample(host(140, 4040), vec![worker(1, 30, 42)]),
            sample(host(160, 4060), vec![worker(1, 50, 42)]),
            sample(host(200, 4100), vec![]),
        ];
        let window = AllocationWindow::from_samples(&samples);
        // Host window spans samples 1..=3, not the empty edges.
        assert_eq!(window.host_total_delta, Some(100));
        assert_eq!(window.host_busy_delta, Some(50));
        assert_eq!(window.workers.len(), 1);
        assert_eq!(window.workers[0].status, "observed");
        assert_eq!(window.workers[0].cpu_ticks_delta, Some(40));
        assert_eq!(window.workers[0].share_of_host_total, Some((40, 100)));
        // No worker ever observed: host window still measured, workers empty.
        let empty = vec![
            sample(host(100, 4000), vec![]),
            sample(host(160, 4200), vec![]),
        ];
        let window = AllocationWindow::from_samples(&empty);
        assert!(window.workers.is_empty());
        assert_eq!(window.host_total_delta, Some(260));
        // A single worker sample gives a zero-length window, never a fabricated delta.
        let single = vec![sample(host(100, 4000), vec![worker(1, 10, 42)])];
        let window = AllocationWindow::from_samples(&single);
        assert_eq!(window.host_total_delta, Some(0));
        assert_eq!(window.workers[0].cpu_ticks_delta, Some(0));
        assert_eq!(window.workers[0].share_of_host_total, None);
        // No samples at all => unknown host window.
        let window = AllocationWindow::from_samples(&[]);
        assert_eq!(window.host_total_delta, None);
        assert!(window.workers.is_empty());
    }
}

#[cfg(test)]
mod cpu_guard_tests {
    use super::*;
    fn guard(num: u64, den: u64) -> CpuGuard {
        CpuGuard {
            maximum_host_busy_share_numerator: num,
            maximum_host_busy_share_denominator: den,
            observation_window_ms: 50,
        }
    }
    fn window(busy: u64, idle: u64) -> HostCpuWindow {
        let before = HostCpu {
            user: 0,
            nice: 0,
            system: 0,
            idle: 0,
            iowait: 0,
            irq: 0,
            softirq: 0,
            steal: 0,
        };
        let after = HostCpu {
            user: busy,
            idle,
            ..before
        };
        HostCpuWindow { before, after }
    }
    #[test]
    fn cpu_guard_validates_exact_share_bounds_and_window() {
        assert!(guard(1, 4).validate().is_ok());
        assert!(guard(0, 1).validate().is_ok());
        assert!(guard(1, 1).validate().is_ok());
        assert!(guard(1, 0).validate().is_err());
        assert!(guard(2, 1).validate().is_err());
        let mut too_short = guard(1, 4);
        too_short.observation_window_ms = 9;
        assert!(too_short.validate().is_err());
        let mut too_long = guard(1, 4);
        too_long.observation_window_ms = 1001;
        assert!(too_long.validate().is_err());
    }
    #[test]
    fn cpu_guard_compares_exact_shares_with_checked_cross_products() {
        // busy 60 / total 260 vs 1/4: 240 < 260 => allowed.
        assert!(guard(1, 4).assess(window(60, 200)).unwrap().allowed);
        // vs 1/5: 300 >= 260 => refused at or above.
        let refused = guard(1, 5).assess(window(60, 200)).unwrap();
        assert!(!refused.allowed);
        assert_eq!(refused.reasons, vec!["host-cpu-busy-at-or-above-threshold"]);
        // Exact boundary refuses (>=).
        assert!(!guard(3, 13).assess(window(60, 200)).unwrap().allowed);
        // Near-u64-MAX values must not overflow the comparison.
        let huge = window(u64::MAX - 1, 1);
        let verdict = guard(1, 2).assess(huge).unwrap();
        assert!(!verdict.allowed);
        assert!(guard(1, 1).assess(huge).unwrap().allowed);
    }
    #[test]
    fn cpu_guard_fails_closed_on_empty_or_regressed_windows() {
        let empty = window(0, 0);
        let decision = guard(1, 2).assess(empty).unwrap();
        assert!(!decision.allowed);
        assert_eq!(decision.reasons, vec!["host-cpu-window-empty"]);
        let regressed = HostCpuWindow {
            before: window(60, 200).after,
            after: window(60, 200).before,
        };
        let decision = guard(1, 2).assess(regressed).unwrap();
        assert!(!decision.allowed);
        assert_eq!(decision.reasons, vec!["host-cpu-counters-regressed"]);
        assert_eq!(decision.busy_delta, None);
        assert_eq!(decision.total_delta, None);
    }
    #[test]
    fn cpu_guard_check_refuses_deterministically_at_zero_threshold() {
        let decision = guard(0, 1).check().unwrap();
        assert!(!decision.allowed);
        assert!(
            decision
                .reasons
                .contains(&"host-cpu-busy-at-or-above-threshold".to_string())
                || decision
                    .reasons
                    .contains(&"host-cpu-window-empty".to_string())
        );
    }
}

#[cfg(test)]
mod sampler_tests {
    use super::*;
    #[test]
    fn sampler_observes_live_burner_threads_with_positive_deltas() {
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (ready_send, ready_recv) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            // Panic-safe: any unwind below still releases the burners.
            let _release = DoneOnDrop(done.clone());
            for i in 0..2 {
                let done = done.clone();
                let ready_send = ready_send.clone();
                std::thread::Builder::new()
                    .name(format!("corpus-smp-t{i}"))
                    .spawn_scoped(scope, move || {
                        // Readiness: the thread has run and its comm is set, so the
                        // sampler's first sample cannot miss it. comm can lag thread
                        // start; retry TIME-bounded inside the parent's recv deadline.
                        let deadline =
                            std::time::Instant::now() + std::time::Duration::from_secs(5);
                        let mut own = Vec::new();
                        while std::time::Instant::now() < deadline {
                            own = named_worker_threads(&format!("corpus-smp-t{i}")).unwrap();
                            if own.len() == 1 {
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        ready_send.send(own).unwrap();
                        let mut x = 0u64;
                        while !done.load(std::sync::atomic::Ordering::SeqCst) {
                            for _ in 0..10000 {
                                x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                            }
                            std::hint::black_box(x);
                        }
                    })
                    .unwrap();
            }
            drop(ready_send);
            let mut ready: Vec<u32> = Vec::new();
            for _ in 0..2 {
                let own = ready_recv
                    .recv_timeout(std::time::Duration::from_secs(15))
                    .unwrap();
                assert_eq!(own.len(), 1, "burner never enumerated its own comm");
                ready.push(own[0].0);
            }
            ready.sort_unstable();
            let sampler = spawn_sampler("corpus-smp-t", 20).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(150));
            let samples = sampler.finish().unwrap();
            done.store(true, std::sync::atomic::Ordering::SeqCst);
            assert!(samples.len() >= 2);
            let window = AllocationWindow::from_samples(&samples);
            assert!(window.host_total_delta.unwrap_or(0) > 0);
            assert_eq!(window.workers.len(), 2);
            let mut tids: Vec<u32> = window.workers.iter().map(|w| w.tid).collect();
            tids.sort_unstable();
            assert_eq!(tids, ready);
            for w in &window.workers {
                assert_eq!(w.status, "observed", "{} {:?}", w.tid, w.reason);
                assert!(w.cpu_ticks_delta.unwrap_or(0) > 0);
            }
            // Unequal burners are reported individually, never equalised.
            assert!(
                window
                    .workers
                    .iter()
                    .all(|w| w.share_of_host_total.is_some())
            );
        });
    }
    #[test]
    fn sampler_with_no_matching_threads_reports_empty_windows() {
        let sampler = spawn_sampler("corpus-absent", 20).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(60));
        let samples = sampler.finish().unwrap();
        assert!(samples.iter().all(|s| s.workers.is_empty()));
        assert!(samples.iter().all(|s| s.observation_error.is_none()));
        let window = AllocationWindow::from_samples(&samples);
        assert!(window.workers.is_empty());
        assert!(window.host_total_delta.is_some());
        assert!(spawn_sampler("", 20).is_err());
        assert!(spawn_sampler("corpus-x", 0).is_err());
        assert!(spawn_sampler("corpus-x", 1001).is_err());
        assert!(spawn_sampler(&"x".repeat(16), 20).is_err());
    }
}
