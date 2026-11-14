//! Sampled filesystem admission; no reserved space, deadline or OS quota guarantee.
use crate::Result;
use serde::{Deserialize, Serialize, Serializer};
use std::path::Path;
fn decimal<S: Serializer>(n: &u64, s: S) -> std::result::Result<S::Ok, S::Error> {
    s.serialize_str(&n.to_string())
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    #[serde(serialize_with = "decimal")]
    pub available_bytes: u64,
    #[serde(serialize_with = "decimal")]
    pub total_bytes: u64,
    pub scope: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Guard {
    pub minimum_available_bytes: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct Decision {
    pub allowed: bool,
    pub reasons: Vec<String>,
    pub observed: Snapshot,
}
pub fn observe(database: &Path) -> Result<Snapshot> {
    if database == Path::new(":memory:") {
        return Err("disk observation requires a filesystem registry".into());
    }
    let target = if database.try_exists().map_err(|e| e.to_string())? {
        if !database.is_file() {
            return Err("disk target must be a registry file".into());
        }
        database
            .canonicalize()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("disk target parent missing")?
            .to_path_buf()
    } else {
        let parent = database
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = parent
            .canonicalize()
            .map_err(|e| format!("disk observation parent unavailable: {e}"))?;
        if !parent.is_dir() {
            return Err("disk target parent is not a directory".into());
        }
        parent
    };
    let observed=Snapshot{available_bytes:fs2::available_space(&target).map_err(|e|format!("disk availability unavailable: {e}"))?,total_bytes:fs2::total_space(&target).map_err(|e|format!("disk capacity unavailable: {e}"))?,scope:"Registry filesystem bytes available to the current user, sampled before a bounded round. No space reservation, inode guarantee, write-success promise, filesystem-stall deadline or hard OS enforcement; other filesystems are not covered.".into()};
    validate(&observed)?;
    Ok(observed)
}
fn validate(observed: &Snapshot) -> Result<()> {
    if observed.total_bytes == 0 || observed.available_bytes > observed.total_bytes {
        return Err("unsupported/inconsistent disk observation".into());
    }
    Ok(())
}
impl Guard {
    pub fn validate(&self) -> Result<()> {
        if self.minimum_available_bytes == 0 {
            return Err(
                "disk reserve must be positive; omit the optional guard to disable it".into(),
            );
        }
        Ok(())
    }
    pub fn assess(&self, observed: Snapshot) -> Result<Decision> {
        self.validate()?;
        validate(&observed)?;
        let allowed = observed.available_bytes >= self.minimum_available_bytes;
        Ok(Decision {
            allowed,
            reasons: if allowed {
                vec![]
            } else {
                vec!["registry-filesystem-below-disk-reserve".into()]
            },
            observed,
        })
    }
    pub fn check(&self, database: &Path) -> Result<Decision> {
        self.assess(observe(database)?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_disk_reserve_boundary_and_unsupported_observations() {
        let guard = Guard {
            minimum_available_bytes: 100,
        };
        let observation = |n| Snapshot {
            available_bytes: n,
            total_bytes: u64::MAX,
            scope: "synthetic boundary fixture".into(),
        };
        assert!(guard.assess(observation(100)).unwrap().allowed);
        assert!(!guard.assess(observation(99)).unwrap().allowed);
        let exact = serde_json::to_value(observation(u64::MAX)).unwrap();
        assert_eq!(exact["available_bytes"], u64::MAX.to_string());
        assert!(
            guard
                .assess(Snapshot {
                    available_bytes: 1,
                    total_bytes: 0,
                    scope: String::new()
                })
                .is_err()
        );
        assert!(
            guard
                .assess(Snapshot {
                    available_bytes: 2,
                    total_bytes: 1,
                    scope: String::new()
                })
                .is_err()
        );
        assert!(
            Guard {
                minimum_available_bytes: 0
            }
            .validate()
            .is_err()
        );
        assert!(observe(Path::new(":memory:")).is_err());
    }
}
