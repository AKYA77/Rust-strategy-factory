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
    if text.len() > 65536 || pid == 0 {
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
    if observed != pid {
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
    Ok(ProcessSample {
        pid,
        rss_bytes: rss,
        user_ticks,
        system_ticks,
        start_ticks: number(19)?,
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
