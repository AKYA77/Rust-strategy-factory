//! Recompute mechanical evidence rather than trusting submitted report hashes.
use crate::{
    Result, archive_evidence,
    calendar_evidence::{self, Declaration},
    catalog::Entry,
    data::Dataset,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub catalog_hash: String,
    pub archive_report_hash: String,
    pub calendar_report_hash: String,
}
#[derive(Debug, Serialize)]
pub struct Verified {
    pub catalog_hash: String,
    pub archive_report_hash: String,
    pub calendar_report_hash: String,
    pub authority: &'static str,
}
pub fn revalidate(
    archive: &Path,
    dataset: &Dataset,
    entry: &Entry,
    calendar: Declaration,
    claim: &Claim,
) -> Result<Verified> {
    if claim.catalog_hash != entry.hash {
        return Err("machine evidence receipt mismatch".into());
    }
    let raw = archive_evidence::assess(archive, dataset, entry)?;
    let schedule = calendar_evidence::assess(dataset, entry, calendar)?;
    if raw.hash != claim.archive_report_hash || schedule.hash != claim.calendar_report_hash {
        return Err("machine evidence differs from recomputed source/calendar facts".into());
    }
    Ok(Verified {
        catalog_hash: entry.hash.clone(),
        archive_report_hash: raw.hash,
        calendar_report_hash: schedule.hash,
        authority: "Mechanical consistency revalidated only; calendar assumptions and human review authority remain unapproved. No research eligibility grant.",
    })
}
