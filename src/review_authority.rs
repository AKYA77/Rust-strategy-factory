//! Small local trust root. Credentials and signing key never enter catalog exports.
use crate::{Result, contracts::SYMBOLS, data, data_review::Area, machine_evidence::hex};
use argon2::{Argon2, PasswordVerifier, password_hash::PasswordHash};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reviewer {
    pub name: String,
    pub password_hash: String,
    pub grants: Vec<Grant>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub symbol: String,
    pub source: String,
    pub submitter: String,
    pub areas: Vec<Area>,
    pub expires_ms: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Secret {
    version: String,
    key: String,
    reviewers: Vec<Reviewer>,
}
pub struct Authority {
    secret: Secret,
    key: [u8; 32],
    id: String,
}
/// Only successful credential verification can construct this capability.
pub struct Principal {
    name: String,
    authority: String,
}
impl Principal {
    pub fn name(&self) -> &str {
        &self.name
    }
}
fn valid_reviewers(reviewers: &[Reviewer]) -> Result<()> {
    if reviewers.is_empty() || reviewers.len() > 16 {
        return Err("authority requires1..16 reviewers".into());
    }
    let mut names = std::collections::HashSet::new();
    for r in reviewers {
        if r.name.trim().is_empty()
            || r.name.len() > 128
            || !names.insert(&r.name)
            || r.grants.is_empty()
            || r.grants.len() > 64
            || r.password_hash.len() > 256
        {
            return Err("invalid reviewer identity/grants".into());
        }
        let h =
            PasswordHash::new(&r.password_hash).map_err(|_| "invalid reviewer password hash")?;
        let number = |name| h.params.get_decimal(name).unwrap_or(0);
        if h.algorithm.as_str() != "argon2id"
            || h.version != Some(19)
            || h.salt.is_none()
            || h.hash.is_none()
            || number("m") < 19456
            || number("m") > 65536
            || number("t") < 2
            || number("t") > 4
            || number("p") != 1
        {
            return Err("reviewer requires bounded Argon2id credentials".into());
        }
        for g in &r.grants {
            if !SYMBOLS.contains(&g.symbol.as_str())
                || g.source.trim().is_empty()
                || g.source.len() > 4096
                || g.submitter.trim().is_empty()
                || g.submitter.len() > 128
                || g.submitter == r.name
                || g.areas.is_empty()
                || g.areas.len() > 5
                || chrono::DateTime::from_timestamp_millis(g.expires_ms).is_none()
            {
                return Err("reviewer must be independent of submitter with bounded exact source/symbol scopes".into());
            }
            let mut seen = Vec::new();
            for area in &g.areas {
                if seen.contains(area) {
                    return Err("duplicate review permission".into());
                }
                seen.push(*area);
            }
        }
    }
    Ok(())
}
impl Authority {
    /// Operator-only provisioning; no remote grant creation endpoint exists.
    pub fn initialize(path: &Path, reviewers: Vec<Reviewer>) -> Result<()> {
        valid_reviewers(&reviewers)?;
        let mut key = [0; 32];
        getrandom::fill(&mut key).map_err(|_| "OS randomness unavailable")?;
        let secret = Secret {
            version: "independent-data-authority-v1".into(),
            key: key.iter().map(|b| format!("{b:02x}")).collect(),
            reviewers,
        };
        let body = serde_json::to_vec(&secret).map_err(|e| e.to_string())?;
        if body.len() > 128 * 1024 {
            return Err("authority file exceeds128KiB".into());
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path).map_err(|e| e.to_string())?;
        file.write_all(&body)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())
    }
    pub fn load(path: &Path) -> Result<Self> {
        let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.len() > 128 * 1024 {
            return Err("authority requires bounded regular file".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                return Err("authority file must be owner-only".into());
            }
        }
        let body = fs::read(path).map_err(|e| e.to_string())?;
        if body.len() > 128 * 1024 {
            return Err("authority file exceeds budget".into());
        }
        let secret: Secret = serde_json::from_slice(&body).map_err(|_| "invalid authority file")?;
        valid_reviewers(&secret.reviewers)?;
        if secret.version != "independent-data-authority-v1" || !hex(&secret.key) {
            return Err("invalid authority key/version".into());
        }
        let mut key = [0; 32];
        for (i, b) in key.iter_mut().enumerate() {
            *b = u8::from_str_radix(&secret.key[i * 2..i * 2 + 2], 16)
                .map_err(|_| "invalid authority key")?;
        }
        let id = data::hash(&secret)?;
        Ok(Self { secret, key, id })
    }
    pub fn identity(&self) -> &str {
        &self.id
    }
    pub fn authenticate(&self, name: &str, password: &str) -> Result<Principal> {
        if password.len() < 16 || password.len() > 1024 {
            return Err("review credentials refused".into());
        }
        let r = self
            .secret
            .reviewers
            .iter()
            .find(|r| r.name == name)
            .ok_or("review credentials refused")?;
        let h = PasswordHash::new(&r.password_hash).map_err(|_| "review credentials refused")?;
        Argon2::default()
            .verify_password(password.as_bytes(), &h)
            .map_err(|_| "review credentials refused")?;
        Ok(Principal {
            name: name.into(),
            authority: self.id.clone(),
        })
    }
    pub(crate) fn grant(
        &self,
        principal: &Principal,
        symbol: &str,
        source: &str,
        area: Area,
        now: i64,
    ) -> Result<&Grant> {
        if principal.authority != self.id {
            return Err("review credential authority changed".into());
        }
        self.permission(&principal.name, symbol, source, area, now)
    }
    pub(crate) fn permission(
        &self,
        name: &str,
        symbol: &str,
        source: &str,
        area: Area,
        at: i64,
    ) -> Result<&Grant> {
        self.secret
            .reviewers
            .iter()
            .find(|r| r.name == name)
            .and_then(|r| {
                r.grants.iter().find(|g| {
                    g.symbol == symbol
                        && g.source == source
                        && g.areas.contains(&area)
                        && at < g.expires_ms
                })
            })
            .ok_or_else(|| "review permission absent or expired".into())
    }
    pub(crate) fn sign<T: Serialize>(&self, record: &T) -> Result<String> {
        Ok(blake3::keyed_hash(
            &self.key,
            &serde_json::to_vec(record).map_err(|e| e.to_string())?,
        )
        .to_hex()
        .to_string())
    }
    pub(crate) fn authentic<T: Serialize>(&self, record: &T, tag: &str) -> Result<bool> {
        Ok(constant_time_eq::constant_time_eq(
            self.sign(record)?.as_bytes(),
            tag.as_bytes(),
        ))
    }
    pub fn shares_password_hash(&self, hash: &str) -> bool {
        self.secret
            .reviewers
            .iter()
            .any(|r| r.password_hash == hash)
    }
}
