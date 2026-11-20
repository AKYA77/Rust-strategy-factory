//! Evidence of what the OS actually enforces versus what service templates only declare.
//! A verdict of `enforced` requires a readable, finite kernel limit at or below the
//! declared value in this process's own cgroup; nothing else counts. cgroup v1 and
//! unreadable state stay `unknown`; an unlimited or absent kernel file is `declared-only`.
//! Even `enforced` describes the configured kernel limit, not observed throttling.
use crate::Result;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitDirectives {
    pub memory_max_bytes: Option<u64>,
    /// (quota microseconds, period microseconds); systemd CPUQuotaPeriodSec default 100ms.
    pub cpu_quota: Option<(u64, u64)>,
    pub tasks_max: Option<u64>,
    pub limit_nofile: Option<u64>,
}
fn parse_memory(value: &str) -> Result<Option<u64>> {
    if value == "infinity" {
        return Ok(None);
    }
    let (digits, scale) = match value.chars().last() {
        Some('K') => (&value[..value.len() - 1], 1024u64),
        Some('M') => (&value[..value.len() - 1], 1024 * 1024),
        Some('G') => (&value[..value.len() - 1], 1024 * 1024 * 1024),
        Some('T') => (&value[..value.len() - 1], 1024u64 * 1024 * 1024 * 1024),
        _ => (value, 1),
    };
    let n = digits
        .parse::<u64>()
        .map_err(|_| "invalid unit memory value")?;
    if n == 0 {
        return Err("unit memory value must be positive".into());
    }
    Ok(Some(
        n.checked_mul(scale).ok_or("unit memory value overflow")?,
    ))
}
fn parse_quota(value: &str) -> Result<Option<(u64, u64)>> {
    if value == "infinity" {
        return Ok(None);
    }
    let percent = value
        .strip_suffix('%')
        .ok_or("CPUQuota must be a percentage")?
        .parse::<u64>()
        .map_err(|_| "invalid CPUQuota percentage")?;
    if percent == 0 {
        return Err("CPUQuota percentage must be positive".into());
    }
    let quota = percent
        .checked_mul(1000)
        .ok_or("CPUQuota percentage overflow")?;
    Ok(Some((quota, 100_000)))
}
fn parse_count(value: &str) -> Result<Option<u64>> {
    if value == "infinity" {
        return Ok(None);
    }
    let n = value
        .parse::<u64>()
        .map_err(|_| "invalid unit count value")?;
    if n == 0 {
        return Err("unit count value must be positive".into());
    }
    Ok(Some(n))
}
pub fn parse_unit(text: &str) -> Result<UnitDirectives> {
    if text.len() > 65536 {
        return Err("unit file exceeds budget".into());
    }
    let mut out = UnitDirectives::default();
    let mut seen = std::collections::BTreeSet::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if !matches!(key, "MemoryMax" | "CPUQuota" | "TasksMax" | "LimitNOFILE") {
            continue;
        }
        if !seen.insert(key) {
            return Err("duplicate unit directive".into());
        }
        if key == "MemoryMax" {
            out.memory_max_bytes = parse_memory(value)?;
        } else if key == "CPUQuota" {
            out.cpu_quota = parse_quota(value)?;
        } else if key == "TasksMax" {
            out.tasks_max = parse_count(value)?;
        } else {
            out.limit_nofile = parse_count(value)?;
        }
    }
    Ok(out)
}
pub fn read_unit(path: &Path) -> Result<UnitDirectives> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("unit file must be a regular file".into());
    }
    use std::io::Read;
    let mut text = String::new();
    file.take(65537)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    parse_unit(&text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CpuStat {
    pub nr_periods: u64,
    pub nr_throttled: u64,
    pub throttled_usec: u64,
}
pub fn parse_cpu_stat(text: &str) -> Result<CpuStat> {
    if text.len() > 65536 {
        return Err("cpu.stat exceeds budget".into());
    }
    let mut nr_periods = None;
    let mut nr_throttled = None;
    let mut throttled_usec = None;
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(key) = fields.next() else {
            continue;
        };
        if !matches!(key, "nr_periods" | "nr_throttled" | "throttled_usec") {
            continue;
        }
        let value = fields
            .next()
            .ok_or("missing cpu.stat counter")?
            .parse::<u64>()
            .map_err(|_| "invalid cpu.stat counter")?;
        if fields.next().is_some() {
            return Err("unexpected cpu.stat field".into());
        }
        let slot = match key {
            "nr_periods" => &mut nr_periods,
            "nr_throttled" => &mut nr_throttled,
            _ => &mut throttled_usec,
        };
        if slot.is_some() {
            return Err("duplicate cpu.stat counter".into());
        }
        *slot = Some(value);
    }
    Ok(CpuStat {
        nr_periods: nr_periods.ok_or("cpu.stat nr_periods unavailable")?,
        nr_throttled: nr_throttled.ok_or("cpu.stat nr_throttled unavailable")?,
        throttled_usec: throttled_usec.ok_or("cpu.stat throttled_usec unavailable")?,
    })
}
pub fn throttle_delta(before: &CpuStat, after: &CpuStat) -> Result<CpuStat> {
    Ok(CpuStat {
        nr_periods: after
            .nr_periods
            .checked_sub(before.nr_periods)
            .ok_or("cpu.stat counters regressed")?,
        nr_throttled: after
            .nr_throttled
            .checked_sub(before.nr_throttled)
            .ok_or("cpu.stat counters regressed")?,
        throttled_usec: after
            .throttled_usec
            .checked_sub(before.throttled_usec)
            .ok_or("cpu.stat counters regressed")?,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct CgroupObservation {
    pub version: String,
    pub relative_path: String,
    pub controllers: Option<String>,
    pub cpu_max: Option<String>,
    pub memory_max: Option<String>,
    pub pids_max: Option<String>,
    pub cpu_stat: Option<CpuStat>,
    pub max_open_files_soft: Option<u64>,
    pub observation_error: Option<String>,
}
fn read_trimmed(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    use std::io::Read;
    let mut text = String::new();
    file.take(65537).read_to_string(&mut text).ok()?;
    if text.len() > 65536 {
        return None;
    }
    Some(text.trim().to_string())
}
pub fn observe_cgroup(proc_root: &Path, cgroup_root: &Path) -> CgroupObservation {
    let mut observed = CgroupObservation {
        version: "unknown".into(),
        relative_path: String::new(),
        controllers: None,
        cpu_max: None,
        memory_max: None,
        pids_max: None,
        cpu_stat: None,
        max_open_files_soft: None,
        observation_error: None,
    };
    let membership = match read_trimmed(&proc_root.join("self/cgroup")) {
        Some(text) => text,
        None => {
            observed.observation_error = Some("cgroup membership unavailable".into());
            return observed;
        }
    };
    let mut unified = None;
    let mut v1 = false;
    for line in membership.lines() {
        let mut parts = line.splitn(3, ':');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("0"), Some(""), Some(path)) => unified = Some(path.to_string()),
            (Some(hierarchy), Some(controllers), Some(_))
                if hierarchy != "0" && !controllers.is_empty() =>
            {
                v1 = true;
            }
            _ => {
                observed.observation_error = Some("malformed cgroup membership line".into());
                return observed;
            }
        }
    }
    let Some(relative) = unified else {
        observed.version = if v1 { "v1".into() } else { "unknown".into() };
        observed.observation_error = Some(
            "cgroup v2 unified membership unavailable; v1 hierarchies are not measured".into(),
        );
        return observed;
    };
    if relative.split('/').any(|part| part == "..") {
        observed.observation_error = Some("cgroup path traversal refused".into());
        return observed;
    }
    observed.version = "v2".into();
    observed.relative_path = relative.clone();
    let dir = cgroup_root.join(relative.trim_start_matches('/'));
    if !dir.is_dir() {
        observed.observation_error = Some("cgroup directory unavailable".into());
        return observed;
    }
    observed.controllers = read_trimmed(&dir.join("cgroup.controllers"));
    observed.cpu_max = read_trimmed(&dir.join("cpu.max"));
    observed.memory_max = read_trimmed(&dir.join("memory.max"));
    observed.pids_max = read_trimmed(&dir.join("pids.max"));
    observed.cpu_stat = read_trimmed(&dir.join("cpu.stat")).and_then(|t| parse_cpu_stat(&t).ok());
    if let Some(limits) = read_trimmed(&proc_root.join("self/limits")) {
        for line in limits.lines() {
            if line.starts_with("Max open files") {
                let fields: Vec<&str> = line.split_whitespace().collect();
                observed.max_open_files_soft = fields.get(3).and_then(|v| v.parse::<u64>().ok());
            }
        }
    }
    observed
}

#[derive(Debug, Clone, Serialize)]
pub struct KnobEvidence {
    pub knob: String,
    pub declared: Option<String>,
    pub observed: Option<String>,
    pub verdict: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct EnforcementReport {
    pub knobs: Vec<KnobEvidence>,
    pub cgroup: CgroupObservation,
    pub scope: String,
}
fn unavailable(knob: &str, declared: Option<String>, observed: Option<String>) -> KnobEvidence {
    KnobEvidence {
        knob: knob.into(),
        declared,
        observed,
        verdict: "unknown".into(),
        reason: "cgroup state unavailable".into(),
    }
}
fn not_declared(knob: &str, observed: Option<String>) -> KnobEvidence {
    KnobEvidence {
        knob: knob.into(),
        declared: None,
        observed,
        verdict: "not-declared".into(),
        reason: "no declaration for this knob in the unit template".into(),
    }
}
fn declared_only(
    knob: &str,
    declared: String,
    observed: Option<String>,
    reason: &str,
) -> KnobEvidence {
    KnobEvidence {
        knob: knob.into(),
        declared: Some(declared),
        observed,
        verdict: "declared-only".into(),
        reason: reason.into(),
    }
}
fn finite_verdict(
    knob: &str,
    declared: String,
    observed: String,
    at_or_below: bool,
) -> KnobEvidence {
    KnobEvidence {
        knob: knob.into(),
        declared: Some(declared),
        observed: Some(observed),
        verdict: if at_or_below {
            "enforced".into()
        } else {
            "weaker-than-declared".into()
        },
        reason: if at_or_below {
            "readable finite kernel limit at or below the declaration".into()
        } else {
            "kernel limit is weaker than the declaration".into()
        },
    }
}
pub fn evaluate(declared: &UnitDirectives, observed: &CgroupObservation) -> EnforcementReport {
    let usable = observed.version == "v2" && observed.observation_error.is_none();
    let mut knobs = Vec::new();
    // CPU quota: compare quota/period fractions exactly.
    let cpu_observed = observed.cpu_max.clone();
    knobs.push(
        match (declared.cpu_quota, usable, cpu_observed.as_deref()) {
            (None, _, _) => not_declared("cpu", cpu_observed),
            (Some(_), false, _) => unavailable(
                "cpu",
                declared.cpu_quota.map(|(q, p)| format!("{q} {p}")),
                cpu_observed,
            ),
            (Some((quota, period)), true, None) => declared_only(
                "cpu",
                format!("{quota} {period}"),
                None,
                "kernel cpu.max unavailable",
            ),
            (Some((quota, period)), true, Some(raw)) => {
                let declaration = format!("{quota} {period}");
                let mut fields = raw.split_whitespace();
                match (fields.next(), fields.next(), fields.next()) {
                    (Some("max"), Some(_), None) => declared_only(
                        "cpu",
                        declaration,
                        Some(raw.into()),
                        "kernel cpu quota unlimited",
                    ),
                    (Some(q), Some(p), None) => match (q.parse::<u64>(), p.parse::<u64>()) {
                        (Ok(q), Ok(p)) if p > 0 => {
                            let at_or_below =
                                (q as u128) * (period as u128) <= (quota as u128) * (p as u128);
                            finite_verdict("cpu", declaration, raw.into(), at_or_below)
                        }
                        _ => KnobEvidence {
                            knob: "cpu".into(),
                            declared: Some(declaration),
                            observed: Some(raw.into()),
                            verdict: "unknown".into(),
                            reason: "kernel cpu.max unparsable".into(),
                        },
                    },
                    _ => KnobEvidence {
                        knob: "cpu".into(),
                        declared: Some(declaration),
                        observed: Some(raw.into()),
                        verdict: "unknown".into(),
                        reason: "kernel cpu.max unparsable".into(),
                    },
                }
            }
        },
    );
    // Memory and pids: finite integer comparisons.
    for (knob, declared_value, raw) in [
        (
            "memory",
            declared.memory_max_bytes,
            observed.memory_max.clone(),
        ),
        ("pids", declared.tasks_max, observed.pids_max.clone()),
    ] {
        let declaration = declared_value.map(|n| n.to_string());
        knobs.push(match (declared_value, usable, raw.as_deref()) {
            (None, _, _) => not_declared(knob, raw),
            (Some(_), false, _) => unavailable(knob, declaration, raw),
            (Some(limit), true, None) => declared_only(
                knob,
                limit.to_string(),
                None,
                "kernel limit file unavailable",
            ),
            (Some(limit), true, Some("max")) => declared_only(
                knob,
                limit.to_string(),
                Some("max".into()),
                "kernel limit unlimited",
            ),
            (Some(limit), true, Some(raw)) => match raw.parse::<u64>() {
                Ok(value) => finite_verdict(knob, limit.to_string(), raw.into(), value <= limit),
                Err(_) => KnobEvidence {
                    knob: knob.into(),
                    declared: Some(limit.to_string()),
                    observed: Some(raw.into()),
                    verdict: "unknown".into(),
                    reason: "kernel limit unparsable".into(),
                },
            },
        });
    }
    // Open-file rlimit as visible in /proc/self/limits.
    let nofile_observed = observed.max_open_files_soft.map(|n| n.to_string());
    knobs.push(
        match (declared.limit_nofile, usable, observed.max_open_files_soft) {
            (None, _, _) => not_declared("nofile", nofile_observed),
            (Some(_), false, _) => unavailable(
                "nofile",
                declared.limit_nofile.map(|n| n.to_string()),
                nofile_observed,
            ),
            (Some(limit), true, None) => declared_only(
                "nofile",
                limit.to_string(),
                None,
                "process open-file rlimit unavailable",
            ),
            (Some(limit), true, Some(value)) => finite_verdict(
                "nofile",
                limit.to_string(),
                value.to_string(),
                value <= limit,
            ),
        },
    );
    EnforcementReport {
        knobs,
        cgroup: observed.clone(),
        scope: "Measured from this process's own cgroup v2 files and open-file rlimit as visible in its mount namespace; enforced means a readable finite kernel limit at or below the unit declaration, not observed throttling behaviour; declared-only and unknown are not enforcement claims.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    const RESEARCH_UNIT: &str = "[Unit]\nDescription=test\n\n[Service]\nType=exec\nMemoryMax=6G\nCPUQuota=300%\nTasksMax=64\nLimitNOFILE=4096\n\n[Install]\nWantedBy=default.target\n";
    const DASHBOARD_UNIT: &str =
        "[Service]\nMemoryMax=512M\nCPUQuota=50%\nTasksMax=32\nLimitNOFILE=4096\n";
    fn root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("os-enforcement-unit-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }
    fn fixture_v2(
        name: &str,
        cpu_max: &str,
        memory_max: &str,
        pids_max: &str,
    ) -> (PathBuf, PathBuf) {
        let base = root(name);
        let proc_root = base.join("proc");
        let cgroup_root = base.join("cgroup");
        std::fs::create_dir_all(proc_root.join("self")).unwrap();
        std::fs::write(proc_root.join("self/cgroup"), "0::/test.slice\n").unwrap();
        std::fs::write(
            proc_root.join("self/limits"),
            "Limit                     Soft Limit           Hard Limit           Units\nMax open files            4096                 4096                 files\n",
        )
        .unwrap();
        let dir = cgroup_root.join("test.slice");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cgroup.controllers"), "cpu memory pids\n").unwrap();
        std::fs::write(dir.join("cpu.max"), cpu_max).unwrap();
        std::fs::write(dir.join("memory.max"), memory_max).unwrap();
        std::fs::write(dir.join("pids.max"), pids_max).unwrap();
        std::fs::write(
            dir.join("cpu.stat"),
            "usage_usec 1000\nuser_usec 800\nsystem_usec 200\nnr_periods 10\nnr_throttled 2\nthrottled_usec 500\n",
        )
        .unwrap();
        (proc_root, cgroup_root)
    }
    #[test]
    fn unit_templates_parse_to_exact_declared_limits() {
        let research = parse_unit(RESEARCH_UNIT).unwrap();
        assert_eq!(research.memory_max_bytes, Some(6 * 1024 * 1024 * 1024));
        assert_eq!(research.cpu_quota, Some((300_000, 100_000)));
        assert_eq!(research.tasks_max, Some(64));
        assert_eq!(research.limit_nofile, Some(4096));
        let dashboard = parse_unit(DASHBOARD_UNIT).unwrap();
        assert_eq!(dashboard.memory_max_bytes, Some(512 * 1024 * 1024));
        assert_eq!(dashboard.cpu_quota, Some((50_000, 100_000)));
        assert_eq!(dashboard.tasks_max, Some(32));
        let empty = parse_unit("[Service]\nType=exec\n").unwrap();
        assert_eq!(empty.memory_max_bytes, None);
        assert_eq!(empty.cpu_quota, None);
        assert_eq!(empty.tasks_max, None);
        let unlimited =
            parse_unit("[Service]\nMemoryMax=infinity\nCPUQuota=infinity\nTasksMax=infinity\n")
                .unwrap();
        assert_eq!(unlimited.memory_max_bytes, None);
        assert_eq!(unlimited.cpu_quota, None);
        assert_eq!(unlimited.tasks_max, None);
        for bad in [
            "[Service]\nMemoryMax=6X\n",
            "[Service]\nMemoryMax=-1\n",
            "[Service]\nMemoryMax=0\n",
            "[Service]\nMemoryMax=6G\nMemoryMax=7G\n",
            "[Service]\nCPUQuota=300\n",
            "[Service]\nCPUQuota=0%\n",
            "[Service]\nCPUQuota=x%\n",
            "[Service]\nTasksMax=0\n",
            "[Service]\nTasksMax=abc\n",
            "[Service]\nLimitNOFILE=-2\n",
        ] {
            assert!(parse_unit(bad).is_err(), "accepted {bad:?}");
        }
        assert!(parse_unit(&"x".repeat(65537)).is_err());
    }
    #[test]
    fn cpu_stat_parses_required_counters_and_delta_requires_monotonicity() {
        let stat =
            parse_cpu_stat("usage_usec 1000\nnr_periods 10\nnr_throttled 2\nthrottled_usec 500\n")
                .unwrap();
        assert_eq!(
            stat,
            CpuStat {
                nr_periods: 10,
                nr_throttled: 2,
                throttled_usec: 500
            }
        );
        assert!(parse_cpu_stat("nr_periods 10\n").is_err());
        assert!(parse_cpu_stat("nr_periods x\nnr_throttled 2\nthrottled_usec 500\n").is_err());
        assert!(
            parse_cpu_stat("nr_periods 1\nnr_periods 2\nnr_throttled 0\nthrottled_usec 0\n")
                .is_err()
        );
        let after = CpuStat {
            nr_periods: 20,
            nr_throttled: 5,
            throttled_usec: 900,
        };
        let delta = throttle_delta(&stat, &after).unwrap();
        assert_eq!(
            delta,
            CpuStat {
                nr_periods: 10,
                nr_throttled: 3,
                throttled_usec: 400
            }
        );
        assert!(throttle_delta(&after, &stat).is_err());
        assert!(parse_cpu_stat(&"x".repeat(65537)).is_err());
    }
    #[test]
    fn cgroup_observation_reads_v2_files_and_refuses_v1_or_missing_state() {
        let (proc_root, cgroup_root) =
            fixture_v2("observe", "300000 100000\n", "6442450944\n", "64\n");
        let observed = observe_cgroup(&proc_root, &cgroup_root);
        assert_eq!(observed.version, "v2");
        assert_eq!(observed.relative_path, "/test.slice");
        assert_eq!(observed.cpu_max.as_deref(), Some("300000 100000"));
        assert_eq!(observed.memory_max.as_deref(), Some("6442450944"));
        assert_eq!(observed.pids_max.as_deref(), Some("64"));
        assert_eq!(observed.controllers.as_deref(), Some("cpu memory pids"));
        assert_eq!(observed.max_open_files_soft, Some(4096));
        assert_eq!(
            observed.cpu_stat,
            Some(CpuStat {
                nr_periods: 10,
                nr_throttled: 2,
                throttled_usec: 500
            })
        );
        assert_eq!(observed.observation_error, None);
        std::fs::remove_dir_all(proc_root.parent().unwrap()).unwrap();
        // cgroup v1 hierarchies are explicitly not measured.
        let base = root("v1");
        let proc_root = base.join("proc");
        std::fs::create_dir_all(proc_root.join("self")).unwrap();
        std::fs::write(
            proc_root.join("self/cgroup"),
            "12:memory:/user.slice\n11:cpu,cpuacct:/user.slice\n",
        )
        .unwrap();
        let observed = observe_cgroup(&proc_root, &base.join("cgroup"));
        assert_eq!(observed.version, "v1");
        assert!(observed.observation_error.is_some());
        assert_eq!(observed.cpu_max, None);
        std::fs::remove_dir_all(&base).unwrap();
        // Missing cgroup directory => unknown, never a fabricated limit.
        let base = root("missing");
        let proc_root = base.join("proc");
        std::fs::create_dir_all(proc_root.join("self")).unwrap();
        std::fs::write(proc_root.join("self/cgroup"), "0::/absent.slice\n").unwrap();
        let observed = observe_cgroup(&proc_root, &base.join("cgroup"));
        assert_eq!(observed.version, "v2");
        assert!(observed.observation_error.is_some());
        assert_eq!(observed.cpu_max, None);
        assert_eq!(observed.memory_max, None);
        std::fs::remove_dir_all(&base).unwrap();
    }
    #[test]
    fn verdicts_require_readable_finite_kernel_limits_at_or_below_declared() {
        let declared = parse_unit(RESEARCH_UNIT).unwrap();
        let (proc_root, cgroup_root) =
            fixture_v2("enforced", "300000 100000\n", "6442450944\n", "64\n");
        let report = evaluate(&declared, &observe_cgroup(&proc_root, &cgroup_root));
        let verdict = |knob: &str| {
            report
                .knobs
                .iter()
                .find(|k| k.knob == knob)
                .unwrap()
                .verdict
                .clone()
        };
        assert_eq!(verdict("cpu"), "enforced");
        assert_eq!(verdict("memory"), "enforced");
        assert_eq!(verdict("pids"), "enforced");
        assert_eq!(verdict("nofile"), "enforced");
        std::fs::remove_dir_all(proc_root.parent().unwrap()).unwrap();
        // Stricter-than-declared kernel limits are still enforced.
        let (proc_root, cgroup_root) =
            fixture_v2("stricter", "100000 100000\n", "1073741824\n", "32\n");
        let report = evaluate(&declared, &observe_cgroup(&proc_root, &cgroup_root));
        assert!(report.knobs.iter().all(|k| k.verdict == "enforced"));
        std::fs::remove_dir_all(proc_root.parent().unwrap()).unwrap();
        // Weaker kernel limits are NOT enforcement of the declaration.
        let (proc_root, cgroup_root) = fixture_v2("weaker", "max 100000\n", "max\n", "max\n");
        let report = evaluate(&declared, &observe_cgroup(&proc_root, &cgroup_root));
        for knob in ["cpu", "memory", "pids"] {
            let evidence = report.knobs.iter().find(|k| k.knob == knob).unwrap();
            assert_eq!(evidence.verdict, "declared-only", "{knob}");
        }
        std::fs::remove_dir_all(proc_root.parent().unwrap()).unwrap();
        // Finite but larger than declared => weaker-than-declared.
        let (proc_root, cgroup_root) =
            fixture_v2("larger", "400000 100000\n", "8589934592\n", "128\n");
        let report = evaluate(&declared, &observe_cgroup(&proc_root, &cgroup_root));
        for knob in ["cpu", "memory", "pids"] {
            let evidence = report.knobs.iter().find(|k| k.knob == knob).unwrap();
            assert_eq!(evidence.verdict, "weaker-than-declared", "{knob}");
        }
        std::fs::remove_dir_all(proc_root.parent().unwrap()).unwrap();
        // Missing kernel files => declared-only, never enforced.
        let base = root("nofiles");
        let proc_root = base.join("proc");
        let cgroup_root = base.join("cgroup");
        std::fs::create_dir_all(proc_root.join("self")).unwrap();
        std::fs::write(proc_root.join("self/cgroup"), "0::/test.slice\n").unwrap();
        std::fs::create_dir_all(cgroup_root.join("test.slice")).unwrap();
        let report = evaluate(&declared, &observe_cgroup(&proc_root, &cgroup_root));
        for knob in ["cpu", "memory", "pids"] {
            let evidence = report.knobs.iter().find(|k| k.knob == knob).unwrap();
            assert_eq!(evidence.verdict, "declared-only", "{knob}");
        }
        std::fs::remove_dir_all(&base).unwrap();
        // Unreadable/v1 cgroup state => unknown for every declared knob.
        let base = root("unknown");
        let proc_root = base.join("proc");
        std::fs::create_dir_all(proc_root.join("self")).unwrap();
        std::fs::write(proc_root.join("self/cgroup"), "11:cpu:/x\n").unwrap();
        let report = evaluate(&declared, &observe_cgroup(&proc_root, &base.join("cg")));
        for evidence in &report.knobs {
            assert_eq!(evidence.verdict, "unknown", "{}", evidence.knob);
        }
        std::fs::remove_dir_all(&base).unwrap();
        // Nothing declared => not-declared, with observed values still reported.
        let empty = parse_unit("[Service]\nType=exec\n").unwrap();
        let (proc_root, cgroup_root) =
            fixture_v2("undeclared", "300000 100000\n", "6442450944\n", "64\n");
        let report = evaluate(&empty, &observe_cgroup(&proc_root, &cgroup_root));
        for evidence in &report.knobs {
            assert_eq!(evidence.verdict, "not-declared", "{}", evidence.knob);
            assert!(evidence.declared.is_none());
        }
        std::fs::remove_dir_all(proc_root.parent().unwrap()).unwrap();
    }
    #[test]
    fn live_cgroup_observation_is_bounded_and_never_panics() {
        let observed = observe_cgroup(Path::new("/proc"), Path::new("/sys/fs/cgroup"));
        assert!(matches!(observed.version.as_str(), "v1" | "v2" | "unknown"));
        let declared = parse_unit(RESEARCH_UNIT).unwrap();
        let report = evaluate(&declared, &observed);
        assert_eq!(report.knobs.len(), 4);
        for evidence in &report.knobs {
            assert!(matches!(
                evidence.verdict.as_str(),
                "enforced" | "weaker-than-declared" | "declared-only" | "not-declared" | "unknown"
            ));
        }
    }
}
