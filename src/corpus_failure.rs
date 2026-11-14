//! Typed provenance; only actual SQLite busy/locked admission errors are retry candidates.
use crate::{Result, data};
use rusqlite::{Connection, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    RegistryAdmissionBusy,
    Unclassified,
    WorkerInterrupted,
    LegacyUnknown,
}
impl Kind {
    pub fn retry_candidate(self) -> bool {
        self == Self::RegistryAdmissionBusy
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::RegistryAdmissionBusy => "registry_admission_busy",
            Self::Unclassified => "unclassified",
            Self::WorkerInterrupted => "worker_interrupted",
            Self::LegacyUnknown => "legacy_unknown",
        }
    }
    pub(crate) fn parse(s: &str) -> Result<Self> {
        match s {
            "registry_admission_busy" => Ok(Self::RegistryAdmissionBusy),
            "unclassified" => Ok(Self::Unclassified),
            "worker_interrupted" => Ok(Self::WorkerInterrupted),
            "legacy_unknown" => Ok(Self::LegacyUnknown),
            _ => Err("unknown corpus failure kind".into()),
        }
    }
}
#[derive(Debug, Clone)]
pub struct AdmissionFailure {
    pub kind: Kind,
    pub message: String,
}
impl std::fmt::Display for AdmissionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl From<String> for AdmissionFailure {
    fn from(message: String) -> Self {
        Self {
            kind: Kind::Unclassified,
            message,
        }
    }
}
impl From<&str> for AdmissionFailure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
impl From<rusqlite::Error> for AdmissionFailure {
    fn from(error: rusqlite::Error) -> Self {
        let busy = matches!(&error,rusqlite::Error::SqliteFailure(code,_) if matches!(code.code,rusqlite::ErrorCode::DatabaseBusy|rusqlite::ErrorCode::DatabaseLocked));
        Self {
            kind: if busy {
                Kind::RegistryAdmissionBusy
            } else {
                Kind::Unclassified
            },
            message: error.to_string(),
        }
    }
}
pub(crate) fn fingerprint(
    id: &str,
    slot: usize,
    job_hash: &str,
    kind: Kind,
    error: &str,
) -> Result<String> {
    data::hash(&("corpus-failure-v1", id, slot, job_hash, kind, error))
}
pub(crate) fn initialize(c: &mut Connection, migrate: bool) -> Result<()> {
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let columns:usize=tx.query_row("SELECT count(*) FROM pragma_table_info('corpus_search_jobs') WHERE name IN ('error_kind','failure_hash')",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if migrate {
        if columns == 0 {
            tx.execute_batch("ALTER TABLE corpus_search_jobs ADD COLUMN error_kind TEXT;ALTER TABLE corpus_search_jobs ADD COLUMN failure_hash TEXT;").map_err(|e|e.to_string())?;
        } else if columns != 2 {
            return Err("partial corpus failure migration".into());
        }
        let rows = {
            let mut q=tx.prepare("SELECT search_id,slot,hash,error FROM corpus_search_jobs WHERE state='error' LIMIT 4097").map_err(|e|e.to_string())?;
            q.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, usize>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
        };
        if rows.len() > 4096 {
            return Err("corpus failure migration budget exceeded".into());
        }
        for (id, slot, hash, error) in rows {
            if error.chars().count() > 512 {
                return Err("legacy failure exceeds metadata cap".into());
            }
            tx.execute("UPDATE corpus_search_jobs SET error_kind=?3,failure_hash=?4 WHERE search_id=?1 AND slot=?2",params![id,slot,Kind::LegacyUnknown.name(),fingerprint(&id,slot,&hash,Kind::LegacyUnknown,&error)?]).map_err(|e|e.to_string())?;
        }
        tx.execute_batch(
            "UPDATE corpus_searches SET token=NULL,expires_ms=NULL;PRAGMA user_version=12;",
        )
        .map_err(|e| e.to_string())?;
    } else if columns != 2 {
        return Err("current corpus failure columns missing; refusing repair".into());
    }
    tx.commit().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_sqlite_codes_not_human_text_can_mark_busy_admission() {
        let text: AdmissionFailure = "database is locked; SQLITE_BUSY".into();
        assert_eq!(text.kind, Kind::Unclassified);
        assert!(!text.kind.retry_candidate());
        let busy = AdmissionFailure::from(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("arbitrary message".into()),
        ));
        assert_eq!(busy.kind, Kind::RegistryAdmissionBusy);
        assert!(busy.kind.retry_candidate());
        let other = AdmissionFailure::from(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
            Some("database is locked".into()),
        ));
        assert_eq!(other.kind, Kind::Unclassified);
        assert!(!Kind::WorkerInterrupted.retry_candidate());
        assert!(!Kind::LegacyUnknown.retry_candidate());
    }
}
