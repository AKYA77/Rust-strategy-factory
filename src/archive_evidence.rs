//! Verify consumed normalized CSV bytes and exact quotes, without asserting authenticity.
use crate::{
    Result,
    catalog::{Entry, Facts},
    data::{self, DataClass, Dataset},
};
use serde::Serialize;
use std::path::Path;
#[derive(Debug, Serialize)]
pub struct Evidence {
    pub version: &'static str,
    pub catalog_hash: String,
    pub archive_hash: String,
    pub content_hash: String,
    pub profile_hash: String,
    pub events: usize,
    pub archive_parser: String,
    pub observed_parser: String,
    pub declared_class: DataClass,
    pub limitations: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub evidence: Evidence,
    pub hash: String,
}
pub fn assess(archive: &Path, dataset: &Dataset, entry: &Entry) -> Result<Report> {
    if data::hash(&entry.receipt)? != entry.hash || entry.receipt.facts != Facts::observe(dataset)?
    {
        return Err("archive evidence catalog/dataset mismatch".into());
    }
    let parsed = data::import_csv(
        archive,
        dataset.profile.clone(),
        &dataset.manifest.source,
        dataset.manifest.class == DataClass::SyntheticTestOnly,
        64 * 1024 * 1024,
        2_000_000,
    )?;
    parsed.verify()?;
    if parsed.manifest.archive_hash != dataset.manifest.archive_hash
        || parsed.manifest.content_hash != dataset.manifest.content_hash
        || parsed.ticks != dataset.ticks
    {
        return Err("consumed normalized archive bytes or quote stream mismatch".into());
    }
    let evidence = Evidence {
        version: "normalized-archive-consistency-v1",
        catalog_hash: entry.hash.clone(),
        archive_hash: parsed.manifest.archive_hash,
        content_hash: parsed.manifest.content_hash,
        profile_hash: parsed.manifest.profile_hash,
        events: parsed.ticks.len(),
        archive_parser: parsed.manifest.parser_version,
        observed_parser: dataset.manifest.parser_version.clone(),
        declared_class: dataset.manifest.class,
        limitations: "Bounded normalized CSV consistency only. Hashes describe the exact consumed byte stream, not future file availability, provider authenticity, licensing, calendar/profile/quality approval, full history or eligibility. Classification is preserved, not promoted. No raw archive is copied or changed; partial chunks cannot stand for a complete archive.",
    };
    let hash = data::hash(&evidence)?;
    Ok(Report { evidence, hash })
}
