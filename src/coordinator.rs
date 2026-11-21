//! Shared local coordinator fencing. Experiment/child leases remain independent.
use crate::{Result, storage::Registry};
use rusqlite::{Connection, params};
#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Search,
    Fleet,
    Campaign,
    CampaignRotation,
}
impl Kind {
    fn table(self) -> &'static str {
        match self {
            Self::Search => "corpus_searches",
            Self::Fleet => "corpus_fleets",
            Self::Campaign => "corpus_campaign_runs",
            Self::CampaignRotation => "corpus_campaign_rotations",
        }
    }
}
#[derive(Clone)]
pub(crate) struct Lease {
    pub id: String,
    pub token: String,
    pub attempt: i64,
    kind: Kind,
    epoch: Option<i64>,
}
pub(crate) fn claim(
    registry: &mut Registry,
    kind: Kind,
    id: &str,
    now: i64,
    duration: i64,
) -> Result<Option<Lease>> {
    if !(1..=86_400_000).contains(&duration) {
        return Err("invalid coordinator lease duration".into());
    }
    let expiry = now
        .checked_add(duration)
        .ok_or("coordinator lease overflow")?;
    let table = kind.table();
    let tx = crate::storage::writer_tx(&registry.connection)?;
    let (state, old, until, attempt): (String, Option<String>, Option<i64>, i64) = tx
        .query_row(
            &format!("SELECT state,token,expires_ms,attempt FROM {table} WHERE id=?1"),
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|e| e.to_string())?;
    if state != "running" || old.is_some() && until.is_some_and(|t| t > now) {
        return Ok(None);
    }
    let attempt = attempt
        .checked_add(1)
        .ok_or("coordinator attempt overflow")?;
    let epoch = match kind {
        Kind::Search => Some(crate::search_control::read(&tx, id)?.epoch),
        Kind::Fleet => Some(crate::fleet_control::read(&tx, id)?.epoch),
        Kind::Campaign => Some(crate::corpus_campaign::epoch(&tx, id)?),
        Kind::CampaignRotation => Some(crate::campaign_rotation::epoch(&tx, id)?),
    };
    let token: String = tx
        .query_row("SELECT lower(hex(randomblob(32)))", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    tx.execute(
        &format!("UPDATE {table} SET token=?2,expires_ms=?3,attempt=?4 WHERE id=?1"),
        params![id, token, expiry, attempt],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(Some(Lease {
        id: id.into(),
        token,
        attempt,
        kind,
        epoch,
    }))
}
pub(crate) fn fence(connection: &Connection, lease: &Lease, now: i64) -> Result<()> {
    let table = lease.kind.table();
    let valid:bool=connection.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=?1 AND token=?2 AND attempt=?3 AND expires_ms>?4 AND state='running')"),params![lease.id,lease.token,lease.attempt,now],|r|r.get(0)).map_err(|e|e.to_string())?;
    if !valid
        || lease.epoch.is_some_and(|epoch| match lease.kind {
            Kind::Search => crate::search_control::read(connection, &lease.id)
                .map_or(true, |h| h.epoch != epoch),
            Kind::CampaignRotation => {
                crate::campaign_rotation::epoch(connection, &lease.id) != Ok(epoch)
            }
            Kind::Campaign => crate::corpus_campaign::epoch(connection, &lease.id) != Ok(epoch),
            Kind::Fleet => {
                crate::fleet_control::read(connection, &lease.id).map_or(true, |h| h.epoch != epoch)
            }
        })
    {
        return Err("coordinator expired, controlled or fenced".into());
    }
    Ok(())
}
pub(crate) fn release(registry: &Registry, lease: &Lease) -> Result<()> {
    let table = lease.kind.table();
    // Autocommit write on the hot post-step path: admit it through the same
    // bounded writer gate as transactions so lease release cannot pile up in
    // SQLite's busy handler either.
    let _guard = crate::storage::writer_guard(&registry.connection)?;
    registry.connection.execute(&format!("UPDATE {table} SET token=NULL,expires_ms=NULL WHERE id=?1 AND token=?2 AND attempt=?3"),params![lease.id,lease.token,lease.attempt]).map_err(|e|e.to_string())?;
    Ok(())
}
