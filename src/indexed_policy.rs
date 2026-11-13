//! Training-only experimental index configuration and independent reference audits.
use crate::{
    Result, data,
    storage::{Lease, Registry},
    tick_engine::Replay,
};
use serde::{Deserialize, Serialize};
pub const BLOCKED: &str = "indexed replay disabled by recorded reference disagreement";
pub const DISAGREEMENT: &str = "indexed replay disagreed with independent sequential reference";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub version: String,
    pub audit_bps: u16,
    pub seed: u64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            version: "training-shared-index-v1".into(),
            audit_bps: 1000,
            seed: 42,
        }
    }
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        if self.version != "training-shared-index-v1" || !(1..=10000).contains(&self.audit_bps) {
            return Err(
                "invalid indexed training policy; nonzero reference audits required".into(),
            );
        }
        Ok(())
    }
    pub fn audit(&self, id: &str) -> Result<bool> {
        self.validate()?;
        let bytes = serde_json::to_vec(&(self, id)).map_err(|e| e.to_string())?;
        let digest = blake3::hash(&bytes);
        let n = u64::from_le_bytes(
            digest.as_bytes()[..8]
                .try_into()
                .map_err(|_| "audit hash width")?,
        ) % 10000;
        Ok(n < u64::from(self.audit_bps))
    }
}
pub(crate) fn guard(connection: &rusqlite::Connection) -> Result<()> {
    let blocked: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE action='indexed-audit-contradiction-v1')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if blocked {
        return Err(BLOCKED.into());
    }
    Ok(())
}
pub(crate) fn record_failure(registry: &Registry, lease: &Lease) -> Result<()> {
    registry.connection.execute("INSERT INTO events(experiment_id,at_ms,action,attempt) VALUES (?1,?2,'indexed-audit-contradiction-v1',?3)",rusqlite::params![lease.experiment_id,chrono::Utc::now().timestamp_millis(),lease.attempt]).map_err(|e|e.to_string())?;
    Ok(())
}
pub fn compare(indexed: &Replay, reference: &Replay) -> Result<(String, String)> {
    let actual = data::hash(indexed)?;
    let expected = data::hash(reference)?;
    if actual != expected {
        return Err(DISAGREEMENT.into());
    }
    Ok((actual, expected))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexed_sampling_is_reproducible_and_requires_reference_coverage() {
        let policy = Policy::default();
        let mut selected = 0;
        for i in 0..1000 {
            let id = i.to_string();
            let chosen = policy.audit(&id).unwrap();
            assert_eq!(chosen, policy.audit(&id).unwrap());
            selected += usize::from(chosen);
        }
        assert!((70..130).contains(&selected));
        assert!(
            Policy {
                audit_bps: 0,
                ..policy.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            Policy {
                audit_bps: 10001,
                ..policy.clone()
            }
            .validate()
            .is_err()
        );
        let all = Policy {
            audit_bps: 10000,
            ..policy
        };
        for i in 0..1000 {
            assert!(all.audit(&i.to_string()).unwrap());
        }
    }
}
