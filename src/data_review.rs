//! Receipt-bound human evidence journal, never an independent qualification grant.
use crate::{Result, data};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Area {
    Source,
    Profile,
    Calendar,
    Quality,
    Coverage,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Pending,
    Rejected,
    AcceptedAssertion,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub area: Area,
    pub decision: Decision,
    pub explanation: String,
    pub evidence_text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub catalog_identity: String,
    pub catalog_hash: String,
    pub reviewer: String,
    pub assessments: [Assessment; 5],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub version: String,
    pub request: Request,
    pub evidence_hashes: Vec<String>,
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    Incomplete,
    Rejected,
    AssertionsRecorded,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub record: Record,
    pub hash: String,
    pub state: State,
    pub limitations: String,
}
const LIMIT: &str = "Reviewer identity and decisions are supplied assertions, not authenticated independent authority. Evidence is retained and hash-bound to one observed receipt. No data class, source/profile/calendar approval, historical completeness or research eligibility is granted; synthetic data cannot qualify.";
fn hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        if !hex(&self.catalog_identity)
            || !hex(&self.catalog_hash)
            || self.reviewer.trim().is_empty()
            || self.reviewer.len() > 256
        {
            return Err("review scope/reviewer invalid".into());
        }
        let mut seen = Vec::new();
        for a in &self.assessments {
            if seen.contains(&a.area)
                || a.explanation.len() > 4096
                || a.evidence_text.len() > 65536
                || a.decision != Decision::Pending
                    && (a.explanation.trim().is_empty() || a.evidence_text.trim().is_empty())
            {
                return Err("review requires five distinct bounded evidence areas; decided assertions require retained evidence and explanation".into());
            }
            seen.push(a.area);
        }
        Ok(())
    }
}
impl Report {
    pub fn create(request: Request) -> Result<Self> {
        request.validate()?;
        let evidence_hashes = request
            .assessments
            .iter()
            .map(|a| {
                blake3::hash(a.evidence_text.as_bytes())
                    .to_hex()
                    .to_string()
            })
            .collect();
        let record = Record {
            version: "receipt-bound-review-v1".into(),
            request,
            evidence_hashes,
        };
        let hash = data::hash(&record)?;
        Self::verified(record, hash)
    }
    pub fn verified(record: Record, hash: String) -> Result<Self> {
        record.request.validate()?;
        let hashes = record
            .request
            .assessments
            .iter()
            .map(|a| {
                blake3::hash(a.evidence_text.as_bytes())
                    .to_hex()
                    .to_string()
            })
            .collect::<Vec<_>>();
        if record.version != "receipt-bound-review-v1"
            || data::hash(&record)? != hash
            || hashes != record.evidence_hashes
        {
            return Err("review evidence integrity mismatch".into());
        }
        let a = &record.request.assessments;
        let state = if a.iter().any(|a| a.decision == Decision::Rejected) {
            State::Rejected
        } else if a.iter().any(|a| a.decision == Decision::Pending) {
            State::Incomplete
        } else {
            State::AssertionsRecorded
        };
        Ok(Self {
            record,
            hash,
            state,
            limitations: LIMIT.into(),
        })
    }
}
