//! Opt-in exact no-entry screening, distinct from approximate candle shadow policy.
use crate::{Result, data, screen::MarketStopProof, storage::ExperimentKey};
use serde::{Deserialize, Serialize};
pub const BLOCKED: &str = "proof screening disabled by recorded audit contradiction";
pub(crate) fn guard(connection: &rusqlite::Connection) -> Result<()> {
    let blocked: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE action='proof-audit-contradiction-v1')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if blocked {
        return Err(BLOCKED.into());
    }
    Ok(())
}
pub(crate) fn record_contradiction(
    registry: &crate::storage::Registry,
    lease: &crate::storage::Lease,
) -> Result<()> {
    registry.connection.execute("INSERT INTO events(experiment_id,at_ms,action,attempt) VALUES (?1,?2,'proof-audit-contradiction-v1',?3)",rusqlite::params![lease.experiment_id,chrono::Utc::now().timestamp_millis(),lease.attempt]).map_err(|e|e.to_string())?;
    Ok(())
}
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
            version: "fixed-market-proof-screen-v1".into(),
            audit_bps: 1000,
            seed: 42,
        }
    }
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        if self.version != "fixed-market-proof-screen-v1" || !(1..=10000).contains(&self.audit_bps)
        {
            return Err("invalid proof screening policy; nonzero audits required".into());
        }
        Ok(())
    }
    pub fn audit(&self, id: &str) -> Result<bool> {
        self.validate()?;
        let bytes = serde_json::to_vec(&(self, id)).map_err(|e| e.to_string())?;
        let digest = blake3::hash(&bytes);
        let bucket = u64::from_le_bytes(
            digest.as_bytes()[..8]
                .try_into()
                .map_err(|_| "audit hash width")?,
        ) % 10000;
        Ok(bucket < u64::from(self.audit_bps))
    }
}
/// Recompute the proof from its exact immutable training specification. A proof
/// means no entry is possible, not that a simulated zero-return trade occurred.
pub fn verified_rejection(
    result: &serde_json::Value,
    key: &ExperimentKey,
) -> Result<Option<MarketStopProof>> {
    if result["proof_rejection"] != true {
        return Ok(None);
    }
    crate::improvement::authorize(key)?;
    let policy: Policy =
        serde_json::from_value(result["proof_policy"].clone()).map_err(|e| e.to_string())?;
    policy.validate()?;
    let selected = policy.audit(&key.identity()?)?;
    if result["proof_audited"].as_bool() != Some(selected)
        || result["tick_replay"].is_null() == selected
    {
        return Err("proof audit selection/receipt mismatch".into());
    }
    let profile: crate::contracts::Profile =
        serde_json::from_value(result["instrument_profile"].clone()).map_err(|e| e.to_string())?;
    let strategy: crate::strategies::Strategy =
        serde_json::from_value(result["strategy"].clone()).map_err(|e| e.to_string())?;
    let costs: crate::tick_engine::Costs =
        serde_json::from_value(result["cost_profile"].clone()).map_err(|e| e.to_string())?;
    let manifest: data::Manifest =
        serde_json::from_value(result["data_manifest"].clone()).map_err(|e| e.to_string())?;
    let proof: MarketStopProof =
        serde_json::from_value(result["market_stop_proof"].clone()).map_err(|e| e.to_string())?;
    if data::hash(&profile)? != key.profile_hash
        || strategy.identity()? != key.concept_hash
        || data::hash(&costs)? != key.cost_hash
        || profile.symbol != key.instrument
        || manifest.profile_hash != key.profile_hash
        || manifest.content_hash != key.dataset_hash
    {
        return Err("proof rejection specification/context mismatch".into());
    }
    crate::screen::verify_market_stop_proof(&proof, &profile, &costs, &strategy)?;
    if !result["tick_replay"].is_null() {
        let replay: crate::tick_engine::Replay =
            serde_json::from_value(result["tick_replay"].clone()).map_err(|e| e.to_string())?;
        if replay.strategy_id != key.concept_hash
            || replay.dataset_id != key.dataset_hash
            || replay.profile_id != key.profile_hash
            || replay.cost_id != key.cost_hash
            || replay.version != key.engine_version
            || !replay.trades.is_empty()
            || replay.unresolved.is_some()
            || replay.pending_entry.is_some()
        {
            return Err("proof rejection contradicted by audit replay".into());
        }
    }
    Ok(Some(proof))
}
