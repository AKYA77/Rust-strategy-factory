//! Bounded observations of the parent invocation. Stored PIDs never grant process ownership.
use crate::{Result, data, owned_process::Limits, storage::Registry};
use rusqlite::{Connection, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
#[derive(Clone)]
pub struct Owner {
    pub id: String,
    token: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    version: String,
    id: String,
    search_id: String,
    parent_pid: u32,
    child_pid: Option<u32>,
    started_at_ms: i64,
    finished_at_ms: Option<i64>,
    compute_revision: String,
    config_hash: String,
    policy_input_hash: String,
    maximum_rounds: usize,
    maximum_wait_ms: u64,
    limits: Limits,
    summary: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resources: Option<crate::resource::ProcessObservation>,
}
fn hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn time(n: i64) -> bool {
    chrono::DateTime::from_timestamp_millis(n).is_some()
}
pub(crate) fn initialize(c: &mut Connection, migrate: bool) -> Result<()> {
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS corpus_owned_invocations(id TEXT PRIMARY KEY,search_id TEXT NOT NULL REFERENCES corpus_searches(id),token TEXT,body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=8192),hash TEXT NOT NULL CHECK(length(hash)=64));CREATE TRIGGER IF NOT EXISTS owned_invocation_no_delete BEFORE DELETE ON corpus_owned_invocations BEGIN SELECT RAISE(ABORT,'owned invocation retained');END;CREATE TRIGGER IF NOT EXISTS owned_invocation_identity BEFORE UPDATE OF id,search_id ON corpus_owned_invocations BEGIN SELECT RAISE(ABORT,'owned identity immutable');END;CREATE TRIGGER IF NOT EXISTS owned_invocation_terminal BEFORE UPDATE ON corpus_owned_invocations WHEN json_extract(OLD.body,'$.finished_at_ms') IS NOT NULL BEGIN SELECT RAISE(ABORT,'owned terminal immutable');END;").map_err(|e|e.to_string())?;
    if migrate {
        tx.execute_batch(
            "UPDATE corpus_searches SET token=NULL,expires_ms=NULL;PRAGMA user_version=16;",
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}
fn read(c: &Connection, id: &str) -> Result<(Body, Option<String>, String)> {
    let(body,source,token,hash):(String,String,Option<String>,String)=c.query_row("SELECT CASE WHEN length(CAST(body AS BLOB))<=8192 THEN body END,search_id,token,hash FROM corpus_owned_invocations WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())?;
    let b: Body = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    b.limits.validate()?;
    if b.version != "owned-invocation-v1"
        || b.id != id
        || !hex(id)
        || b.search_id != source
        || !hex(&source)
        || b.parent_pid == 0
        || b.child_pid == Some(0)
        || !time(b.started_at_ms)
        || b.finished_at_ms.is_some_and(|n| !time(n))
        || !(1..=10000).contains(&b.maximum_rounds)
        || b.maximum_wait_ms > 3_600_000
        || !b
            .compute_revision
            .strip_prefix("compute-blake3:")
            .is_some_and(hex)
        || !hex(&b.config_hash)
        || !hex(&b.policy_input_hash)
        || b.finished_at_ms.is_some() != b.summary.is_some()
        || b.finished_at_ms.is_some() == token.is_some()
        || token.as_deref().is_some_and(|v| !hex(v))
        || data::hash(&b)? != hash
    {
        return Err("owned invocation integrity mismatch".into());
    }
    if let Some(resources) = &b.resources {
        resources.validate()?;
        if b.child_pid.is_none() {
            return Err("unattached process resources refused".into());
        }
    }
    if let Some(s) = &b.summary {
        validate_summary(&b, s)?;
    }
    Ok((b, token, hash))
}
fn write(c: &Connection, b: &Body, token: &str, finished: bool) -> Result<()> {
    let body = serde_json::to_string(b).map_err(|e| e.to_string())?;
    if body.len() > 8192 {
        return Err("owned summary exceeds metadata bound".into());
    }
    let n=c.execute("UPDATE corpus_owned_invocations SET body=?3,hash=?4,token=CASE WHEN ?5 THEN NULL ELSE token END WHERE id=?1 AND token=?2",params![b.id,token,body,data::hash(b)?,finished]).map_err(|e|e.to_string())?;
    if n != 1 {
        return Err("owned invocation owner fenced".into());
    }
    Ok(())
}
pub fn start(
    reg: &mut Registry,
    source: &str,
    config_hash: &str,
    policy_input_hash: &str,
    rounds: usize,
    wait: u64,
    limits: &Limits,
) -> Result<Owner> {
    limits.validate()?;
    if !hex(config_hash)
        || !hex(policy_input_hash)
        || !(1..=10000).contains(&rounds)
        || wait > 3_600_000
    {
        return Err("invalid owned metadata admission".into());
    }
    let tx = reg
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    crate::search_control::read(&tx, source)?;
    let n: usize = tx
        .query_row("SELECT count(*) FROM corpus_owned_invocations", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    if n >= 128 {
        return Err("owned invocation inventory full; preserve evidence".into());
    }
    let (id, token): (String, String) = tx
        .query_row(
            "SELECT lower(hex(randomblob(32))),lower(hex(randomblob(32)))",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let b = Body {
        version: "owned-invocation-v1".into(),
        id: id.clone(),
        search_id: source.into(),
        parent_pid: std::process::id(),
        child_pid: None,
        started_at_ms: chrono::Utc::now().timestamp_millis(),
        finished_at_ms: None,
        compute_revision: crate::research::compute_revision()?,
        config_hash: config_hash.into(),
        policy_input_hash: policy_input_hash.into(),
        maximum_rounds: rounds,
        maximum_wait_ms: wait,
        limits: limits.clone(),
        summary: None,
        resources: None,
    };
    tx.execute(
        "INSERT INTO corpus_owned_invocations(id,search_id,token,body,hash) VALUES(?1,?2,?3,?4,?5)",
        params![
            id,
            source,
            token,
            serde_json::to_string(&b).map_err(|e| e.to_string())?,
            data::hash(&b)?
        ],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(Owner { id, token })
}
pub fn attach(reg: &mut Registry, owner: &Owner, pid: u32) -> Result<()> {
    let tx = reg
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let (mut b, token, _) = read(&tx, &owner.id)?;
    if token.as_deref() != Some(&owner.token) || b.child_pid.is_some() || pid == 0 {
        return Err("owned child attachment fenced".into());
    }
    b.child_pid = Some(pid);
    write(&tx, &b, &owner.token, false)?;
    tx.commit().map_err(|e| e.to_string())
}
pub fn observe_resources(
    reg: &mut Registry,
    owner: &Owner,
    observation: &crate::resource::ProcessObservation,
) -> Result<()> {
    observation.validate()?;
    let tx = reg
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let (mut b, token, _) = read(&tx, &owner.id)?;
    if token.as_deref() != Some(&owner.token)
        || b.child_pid.is_none()
        || b.resources.as_ref().is_some_and(|old| {
            observation.samples < old.samples
                || observation.peak_rss_bytes < old.peak_rss_bytes
                || old.start_ticks.is_some() && observation.start_ticks != old.start_ticks
                || observation.last_cpu_ticks < old.last_cpu_ticks
        })
    {
        return Err("owned resource observation fence refused".into());
    }
    b.resources = Some(observation.clone());
    write(&tx, &b, &owner.token, false)?;
    tx.commit().map_err(|e| e.to_string())
}
fn validate_summary(b: &Body, s: &serde_json::Value) -> Result<()> {
    if s["invocation_id"] != b.id
        || s["search_id"] != b.search_id
        || s["pid"].as_str().and_then(|v| v.parse::<u32>().ok()) != b.child_pid
        || s["reaped"] != true
        || s["version"] != "owned-corpus-process-v1"
        || !s["report_hash"].as_str().is_some_and(hex)
        || s.get("child_report").is_some()
        || s.get("session_status").is_some()
        || !matches!(
            s["stop_reason"].as_str(),
            Some(
                "signal"
                    | "wall-budget"
                    | "output-limit-or-read-error"
                    | "child-rss-pressure"
                    | "child-cpu-budget"
                    | "child-resource-observation-unavailable"
                    | "parent-resource-journal-unavailable"
                    | "child-exited"
                    | "child-failed"
            )
        )
    {
        return Err("owned terminal summary mismatch".into());
    }
    if let Some(pin) = s.get("input_pin").filter(|v| !v.is_null()) {
        let pin: crate::owned_process::InputPin =
            serde_json::from_value(pin.clone()).map_err(|e| e.to_string())?;
        pin.validate_binding(&b.search_id, &b.config_hash, &b.policy_input_hash)?;
    }
    Ok(())
}
pub fn finish(
    reg: &mut Registry,
    owner: &Owner,
    report: &serde_json::Value,
) -> Result<serde_json::Value> {
    let tx = reg
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let (mut b, token, _) = read(&tx, &owner.id)?;
    if token.as_deref() != Some(&owner.token) {
        return Err("owned terminal owner fenced".into());
    }
    let mut summary = report.clone();
    let object = summary.as_object_mut().ok_or("owned report missing")?;
    object.remove("child_report");
    object.remove("session_status");
    object.remove("limitations");
    for field in [
        "stop_error",
        "stdout_read_error",
        "stderr_read_error",
        "session_status_error",
        "resource_journal_error",
    ] {
        if let Some(value) = object.get_mut(field)
            && let Some(s) = value.as_str()
        {
            *value = s.chars().take(256).collect::<String>().into();
        }
    }
    summary["report_hash"] = data::hash(report)?.into();
    summary["child_report_hash"] = report
        .get("child_report")
        .filter(|v| !v.is_null())
        .map(data::hash)
        .transpose()?
        .into();
    summary["session_receipt_hash"] = report["session_status"]["receipt_hash"].clone();
    validate_summary(&b, &summary)?;
    b.finished_at_ms = Some(chrono::Utc::now().timestamp_millis());
    b.summary = Some(summary);
    write(&tx, &b, &owner.token, true)?;
    tx.commit().map_err(|e| e.to_string())?;
    status(reg, &owner.id)
}
pub fn status(reg: &Registry, id: &str) -> Result<serde_json::Value> {
    status_on(&reg.connection, id)
}
pub(crate) fn status_on(c: &Connection, id: &str) -> Result<serde_json::Value> {
    let (b, _, hash) = read(c, id)?;
    let mut v = serde_json::to_value(&b).map_err(|e| e.to_string())?;
    for field in [
        "parent_pid",
        "started_at_ms",
        "maximum_rounds",
        "maximum_wait_ms",
    ] {
        v[field] = v[field].to_string().into();
    }
    v["child_pid"] = b.child_pid.map(|n| n.to_string()).into();
    v["finished_at_ms"] = b.finished_at_ms.map(|n| n.to_string()).into();
    v["observation"] = if b.finished_at_ms.is_some() {
        "finished; child reaped by recording parent"
    } else {
        "unclosed; parent and child liveness not established"
    }
    .into();
    v["resources"] = b.resources.as_ref().map(|r| r.exact()).transpose()?.into();
    for field in [
        "maximum_wall_ms",
        "shutdown_grace_ms",
        "maximum_output_bytes",
        "maximum_child_rss_bytes",
        "maximum_child_cpu_ticks",
    ] {
        if !v["limits"][field].is_null() {
            v["limits"][field] = v["limits"][field].to_string().into();
        }
    }
    v["resources_scope"]="Sampled owned-child RSS bytes and raw native kernel user+system CPU ticks; no elapsed-time conversion, unsampled peak or kernel enforcement is implied.".into();
    v["observation_hash"] = hash.into();
    Ok(v)
}
pub fn list(reg: &Registry) -> Result<Vec<serde_json::Value>> {
    let ids = {
        let mut q = reg
            .connection
            .prepare("SELECT id FROM corpus_owned_invocations ORDER BY rowid DESC LIMIT 129")
            .map_err(|e| e.to_string())?;
        q.query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    if ids.len() > 128 {
        return Err("owned inventory quota exceeded".into());
    }
    ids.into_iter().map(|id| status(reg, &id)).collect()
}
pub(crate) fn integrity(reg: &Registry) -> Result<()> {
    list(reg)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn limits() -> Limits {
        Limits {
            maximum_wall_ms: 1000,
            shutdown_grace_ms: 100,
            maximum_child_rss_bytes: None,
            maximum_child_cpu_ticks: None,
            maximum_output_bytes: 1024,
        }
    }
    fn start_one(reg: &mut Registry, source: &str) -> Owner {
        start(
            reg,
            source,
            &"a".repeat(64),
            &"b".repeat(64),
            10,
            0,
            &limits(),
        )
        .unwrap()
    }
    fn report(id: &str, source: &str) -> serde_json::Value {
        serde_json::json!({"invocation_id":id,"version":"owned-corpus-process-v1","search_id":source,"pid":"123","reaped":true,"stop_reason":"child-exited","child_report":{"synthetic_test_only":true},"session_status":null})
    }
    #[test]
    fn input_pin_summary_bindings_refuse_forgery_and_preserve_legacy_reports() {
        let (root, mut registry, source) =
            crate::supervision_session::tests::fixture("owned-input-summary");
        let owner = start_one(&mut registry, &source);
        attach(&mut registry, &owner, 123).unwrap();
        let mut report = report(&owner.id, &source);
        let pin = serde_json::json!({"version":"owned-input-pin-v1","operator_hash":"a".repeat(64),"effective_operator_hash":"a".repeat(64),"policy_hash":"b".repeat(64),"source_id":source,"start_index":null});
        for (field, value) in [
            ("version", "future-version".into()),
            ("operator_hash", "bad".into()),
            ("effective_operator_hash", "c".repeat(64)),
            ("policy_hash", "c".repeat(64)),
            ("source_id", "c".repeat(64)),
        ] {
            let mut wrong = pin.clone();
            wrong[field] = value.into();
            report["input_pin"] = wrong;
            assert!(finish(&mut registry, &owner, &report).is_err());
            assert!(status(&registry, &owner.id).unwrap()["summary"].is_null());
        }
        report["input_pin"] = pin.clone();
        let done = finish(&mut registry, &owner, &report).unwrap();
        assert_eq!(done["summary"]["input_pin"], pin);
        registry.backup(&root.join("pin-copy.sqlite")).unwrap();
        let copied = Registry::open(&root.join("pin-copy.sqlite")).unwrap();
        assert_eq!(
            status(&copied, &owner.id).unwrap()["summary"]["input_pin"],
            pin
        );
        drop(copied);
        let (mut body, _, _) = read(&registry.connection, &owner.id).unwrap();
        body.summary.as_mut().unwrap()["input_pin"]["policy_hash"] = "c".repeat(64).into();
        registry
            .connection
            .execute_batch("DROP TRIGGER owned_invocation_terminal;")
            .unwrap();
        registry
            .connection
            .execute(
                "UPDATE corpus_owned_invocations SET body=?2,hash=?3 WHERE id=?1",
                params![
                    owner.id,
                    serde_json::to_string(&body).unwrap(),
                    data::hash(&body).unwrap()
                ],
            )
            .unwrap();
        assert!(status(&registry, &owner.id).is_err());
        drop(registry);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn owner_attachment_terminal_and_backup_fences_preserve_bounded_observations() {
        let (root, mut reg, source) = crate::supervision_session::tests::fixture("owned-fences");
        let owner = start_one(&mut reg, &source);
        let mut forged = owner.clone();
        forged.token = "0".repeat(64);
        assert!(attach(&mut reg, &forged, 123).is_err());
        assert!(attach(&mut reg, &owner, 0).is_err());
        let pending = status(&reg, &owner.id).unwrap();
        assert!(pending["child_pid"].is_null());
        assert!(
            pending["observation"]
                .as_str()
                .unwrap()
                .starts_with("unclosed")
        );
        assert!(pending.get("token").is_none());
        attach(&mut reg, &owner, 123).unwrap();
        assert!(attach(&mut reg, &owner, 124).is_err());
        let report = report(&owner.id, &source);
        let done = finish(&mut reg, &owner, &report).unwrap();
        assert_eq!(done["summary"]["report_hash"], data::hash(&report).unwrap());
        assert!(done["summary"].get("child_report").is_none());
        assert!(finish(&mut reg, &owner, &report).is_err());
        assert!(
            reg.connection
                .execute(
                    "UPDATE corpus_owned_invocations SET hash=?2 WHERE id=?1",
                    params![owner.id, "0".repeat(64)]
                )
                .is_err()
        );
        let backup = root.join("owned-backup.sqlite");
        reg.backup(&backup).unwrap();
        let restored = Registry::open(&backup).unwrap();
        assert_eq!(status(&restored, &owner.id).unwrap(), done);
        restored.integrity().unwrap();
        drop(restored);
        drop(reg);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn terminal_write_failure_rolls_back_then_corruption_is_refused() {
        let (root, mut reg, source) = crate::supervision_session::tests::fixture("owned-rollback");
        let owner = start_one(&mut reg, &source);
        attach(&mut reg, &owner, 123).unwrap();
        reg.connection.execute_batch("CREATE TRIGGER fail_owned BEFORE UPDATE ON corpus_owned_invocations WHEN NEW.token IS NULL BEGIN SELECT RAISE(ABORT,'injected metadata failure');END;").unwrap();
        let report = report(&owner.id, &source);
        assert!(finish(&mut reg, &owner, &report).is_err());
        assert!(status(&reg, &owner.id).unwrap()["finished_at_ms"].is_null());
        reg.connection
            .execute_batch("DROP TRIGGER fail_owned;")
            .unwrap();
        finish(&mut reg, &owner, &report).unwrap();
        reg.connection
            .execute_batch("DROP TRIGGER owned_invocation_terminal;")
            .unwrap();
        reg.connection
            .execute(
                "UPDATE corpus_owned_invocations SET hash=?2 WHERE id=?1",
                params![owner.id, "0".repeat(64)],
            )
            .unwrap();
        assert!(status(&reg, &owner.id).is_err());
        assert!(reg.integrity().is_err());
        drop(reg);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn inventory_cap_refuses_start_but_preserves_existing_completion() {
        let (root, mut reg, source) = crate::supervision_session::tests::fixture("owned-cap");
        let first = start_one(&mut reg, &source);
        for _ in 1..128 {
            start_one(&mut reg, &source);
        }
        assert!(
            start(
                &mut reg,
                &source,
                &"a".repeat(64),
                &"b".repeat(64),
                10,
                0,
                &limits()
            )
            .is_err()
        );
        attach(&mut reg, &first, 123).unwrap();
        finish(&mut reg, &first, &report(&first.id, &source)).unwrap();
        assert_eq!(list(&reg).unwrap().len(), 128);
        reg.integrity().unwrap();
        drop(reg);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    #[test]
    fn legacy_limit_digests_and_monotonic_owned_resource_backup_remain_verifiable() {
        #[derive(Serialize)]
        struct LegacyLimits {
            maximum_wall_ms: u64,
            shutdown_grace_ms: u64,
            maximum_output_bytes: usize,
        }
        let limits = Limits {
            maximum_wall_ms: 1000,
            shutdown_grace_ms: 100,
            maximum_output_bytes: 1024,
            maximum_child_rss_bytes: None,
            maximum_child_cpu_ticks: None,
        };
        assert_eq!(
            data::hash(&limits).unwrap(),
            data::hash(&LegacyLimits {
                maximum_wall_ms: 1000,
                shutdown_grace_ms: 100,
                maximum_output_bytes: 1024
            })
            .unwrap()
        );
        let (root, mut reg, source) =
            crate::supervision_session::tests::fixture("owned-resource-fences");
        let owner = start(
            &mut reg,
            &source,
            &"a".repeat(64),
            &"b".repeat(64),
            10,
            0,
            &limits,
        )
        .unwrap();
        let observation = crate::resource::ProcessObservation {
            samples: 2,
            peak_rss_bytes: u64::MAX,
            last_cpu_ticks: Some(100),
            start_ticks: Some(42),
            observed_at_ms: 1000,
            observation_error: None,
        };
        assert!(observe_resources(&mut reg, &owner, &observation).is_err());
        attach(&mut reg, &owner, 123).unwrap();
        observe_resources(&mut reg, &owner, &observation).unwrap();
        let mut stale = observation.clone();
        stale.samples = 1;
        assert!(observe_resources(&mut reg, &owner, &stale).is_err());
        stale = observation.clone();
        stale.last_cpu_ticks = Some(99);
        assert!(observe_resources(&mut reg, &owner, &stale).is_err());
        stale = observation.clone();
        stale.start_ticks = Some(43);
        assert!(observe_resources(&mut reg, &owner, &stale).is_err());
        let expected = status(&reg, &owner.id).unwrap();
        assert_eq!(
            expected["resources"]["peak_rss_bytes"],
            u64::MAX.to_string()
        );
        let backup = root.join("resource-backup.sqlite");
        reg.backup(&backup).unwrap();
        let restored = Registry::open(&backup).unwrap();
        assert_eq!(status(&restored, &owner.id).unwrap(), expected);
        restored.integrity().unwrap();
        drop(restored);
        drop(reg);
        std::fs::remove_dir_all(root).unwrap();
    }
}
