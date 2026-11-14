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
