//! Explicit bounded foreground research loop; no dashboard or LLM is needed once started.
use crate::{
    Result,
    corpus_fleet::{self, Supervision},
    fleet_control, operator_config, retry_policy,
    storage::Registry,
};
use std::{
    io::Write,
    path::Path,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
pub fn run(
    database: &Path,
    id: &str,
    supervision: &Supervision<'_>,
    maximum_visits: usize,
    maximum_wait_ms: u64,
) -> Result<serde_json::Value> {
    if !(1..=10000).contains(&maximum_visits) || maximum_wait_ms > 3_600_000 {
        return Err("fleet loop requires1..10000 visits and0..3600000 wait ms".into());
    }
    supervision.limits.validate()?;
    let policy: retry_policy::Policy = operator_config::read(supervision.policy)?;
    policy.validate()?;
    let mut registry = Registry::open(database)?;
    corpus_fleet::verify_supervision_policy(&registry, id, &policy)?;
    let mut visits = 0usize;
    let mut checks = 0usize;
    let mut wakeups = 0usize;
    let mut waited = Duration::ZERO;
    let budget = Duration::from_millis(maximum_wait_ms);
    let mut last = None;
    let mut last_wakeup = None;
    let mut reason = "visit-limit";
    'session: while visits < maximum_visits && checks < maximum_visits * 2 + 1 {
        if supervision.shutdown.load(Ordering::Acquire) {
            reason = "signal";
            break;
        }
        let result = corpus_fleet::supervised_step(database, id, supervision)?;
        checks += 1;
        visits += usize::from(result.visit.is_some());
        let kind = result.kind.clone();
        last = Some(result);
        if visits >= maximum_visits {
            break;
        }
        if kind == "visited" {
            continue;
        }
        if kind == "exhausted" {
            reason = "exhausted";
            break;
        }
        if !matches!(kind.as_str(), "held" | "no-ready-sources") {
            reason = "held-or-pressure";
            break;
        }
        let Some(hint) =
            corpus_fleet::retry_wakeup(&registry, id, chrono::Utc::now().timestamp_millis())?
        else {
            reason = "retry-held";
            break;
        };
        last_wakeup = Some(
            serde_json::json!({"fleet_epoch":hint.epoch.to_string(),"deadline_ms":hint.deadline_ms.to_string(),"sources":hint.sources}),
        );
        if budget.is_zero() {
            reason = "retry-backoff";
            break;
        }
        let _ = writeln!(
            std::io::stderr(),
            "fleet supervision waiting for retry at {}",
            hint.deadline_ms
        );
        let start = Instant::now();
        while chrono::Utc::now().timestamp_millis() < hint.deadline_ms {
            if supervision.shutdown.load(Ordering::Acquire) {
                waited += start.elapsed();
                reason = "signal";
                break 'session;
            }
            let elapsed = waited + start.elapsed();
            if elapsed >= budget {
                waited = elapsed;
                reason = "wait-budget";
                break 'session;
            }
            let control = corpus_fleet::control_status(&registry, id)?;
            if control["state"] != "paused"
                || control["pause_origin"] != "scheduler_idle"
                || control["epoch"]
                    .as_str()
                    .and_then(|v| v.parse::<i64>().ok())
                    != Some(hint.epoch)
            {
                waited += start.elapsed();
                reason = "fleet-control-changed";
                break 'session;
            }
            std::thread::sleep(Duration::from_millis(50).min(budget - elapsed));
        }
        waited += start.elapsed();
        if supervision.shutdown.load(Ordering::Acquire) {
            reason = "signal";
            break;
        }
        let now = chrono::Utc::now().timestamp_millis();
        let Some(current) = corpus_fleet::retry_wakeup(&registry, id, now)? else {
            reason = "retry-held";
            break;
        };
        if current.epoch != hint.epoch {
            reason = "fleet-control-changed";
            break;
        }
        if current.deadline_ms > now.checked_add(100).ok_or("fleet loop clock overflow")? {
            continue;
        }
        if fleet_control::resume_idle(&mut registry, id, hint.epoch, now).is_err() {
            reason = "fleet-control-changed";
            break;
        }
        wakeups += 1;
    }
    if checks > maximum_visits * 2 && visits < maximum_visits {
        reason = "check-limit";
    }
    if supervision.shutdown.load(Ordering::Acquire) {
        reason = "signal";
    }
    Ok(
        serde_json::json!({"visits":visits.to_string(),"checks":checks.to_string(),"wakeups":wakeups.to_string(),"waited_ms":waited.as_millis().to_string(),"stop_reason":reason,"last_wakeup":last_wakeup,"last":last,"status":corpus_fleet::status(&registry,id)?,"limitations":"Explicit bounded foreground loop. Only verified scheduler-idle epochs can resume; typed source admission rechecks eligibility. Wait polling writes no retry observations. Sampled child budgets are not hard OS limits; service installation and parent-death recovery remain separate."}),
    )
}
