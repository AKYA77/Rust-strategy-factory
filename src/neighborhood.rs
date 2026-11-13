//! Bounded deterministic parameter probes; no performance input or protected-data adaptation.
use crate::{Result, data, strategies::Rule};
use serde_json::Value;
use std::collections::BTreeSet;
pub const VERSION: &str = "canonical-first-six-scalar-rule-neighbors-v2";
fn paths(value: &Value, prefix: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let next = format!("{prefix}/{key}");
                if matches!(
                    key.as_str(),
                    "window"
                        | "lookback"
                        | "fast"
                        | "slow"
                        | "signal_window"
                        | "basis_points"
                        | "multiple_bps"
                ) && value.as_u64().is_some()
                {
                    out.push(next);
                } else {
                    paths(value, &next, out);
                }
            }
        }
        Value::Array(values) => {
            for (i, value) in values.iter().enumerate() {
                paths(value, &format!("{prefix}/{i}"), out);
            }
        }
        _ => {}
    }
}
/// Canonical traversal; one scalar changes by max(1,floor(value/10)). Invalid or
/// equivalent concepts are omitted. At most six distinct valid rule neighbors.
pub fn rules(rule: &Rule) -> Result<Vec<Rule>> {
    let canonical = rule.canonical()?;
    let value = serde_json::to_value(&canonical).map_err(|e| e.to_string())?;
    let mut slots = Vec::new();
    paths(&value, "", &mut slots);
    let mut seen = BTreeSet::from([data::hash(&canonical)?]);
    let mut neighbors = Vec::new();
    for path in slots {
        let n = value
            .pointer(&path)
            .and_then(Value::as_u64)
            .ok_or("invalid canonical scalar path")?;
        let step = (n / 10).max(1);
        for next in [
            n.saturating_sub(step),
            n.checked_add(step).ok_or("neighbor scalar overflow")?,
        ] {
            let mut candidate = value.clone();
            *candidate
                .pointer_mut(&path)
                .ok_or("missing canonical scalar")? = Value::from(next);
            let Ok(candidate) = serde_json::from_value::<Rule>(candidate) else {
                continue;
            };
            let Ok(candidate) = candidate.canonical() else {
                continue;
            };
            if seen.insert(data::hash(&candidate)?) {
                neighbors.push(candidate);
                if neighbors.len() == 6 {
                    return Ok(neighbors);
                }
            }
        }
    }
    Ok(neighbors)
}
