//! Durable, content-addressed preparation caches (candle/feature/decision).
//!
//! A cache entry is keyed by an exact scope identity over instrument, dataset,
//! strategy, costs, engine, configuration, window, stage and protection class.
//! Artifacts are immutable files addressed by their raw-byte content hash; the
//! SQLite index carries the scope identity and exact byte accounting. Nothing
//! here touches scientific gates, eligibility or qualification: a cache hit only
//! proves that identical bytes were previously prepared for an identical scope.
//!
//! Crashes are handled by refusal, never by guessing: a staged `.partial-*` file
//! is never read as an artifact, an incomplete/oversized/symlinked artifact is
//! refused, and valid staging remnants are removed only by `recover_partial`.

use crate::{
    Result,
    contracts::SYMBOLS,
    data::{hash, hash_raw},
    machine_evidence::hex,
    storage,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

/// Hard per-artifact ceiling (independent of the caller's configured limit).
pub const MAX_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;
/// Hard physical store ceiling, counted as summed file lengths.
pub const MAX_STORE_BYTES: u64 = 512 * 1024 * 1024;
/// Hard artifact-count ceiling.
pub const MAX_ARTIFACTS: usize = 4096;
/// Maximum admissible scope window (~100 years, in milliseconds).
pub const MAX_WINDOW_MS: i64 = 100 * 366 * 24 * 60 * 60 * 1000;
pub const RECEIPT_VERSION: &str = "prep-cache-receipt-v1";
const SCOPE_VERSION: &str = "prep-cache-scope-v1";

/// Which preparation stage produced an artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Candle,
    Feature,
    Decision,
}
impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Self::Candle => "candle",
            Self::Feature => "feature",
            Self::Decision => "decision",
        }
    }
}

/// Protection class. This is part of the scope identity and also selects a
/// physically separate artifact directory, so protected OOS preparation can
/// never be served from the shared (unprotected) cache and vice versa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protection {
    Standard,
    Protected,
}
impl Protection {
    pub fn name(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Protected => "protected",
        }
    }
    fn directory(self) -> &'static str {
        self.name()
    }
}

/// Exact identity inputs of one preparation scope. No field is optional and no
/// field is ignored: any difference produces a different cache key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub instrument: String,
    pub dataset_hash: String,
    pub strategy_hash: String,
    pub costs_hash: String,
    pub engine_version: String,
    pub config_hash: String,
    pub start_ms: i64,
    pub end_exclusive_ms: i64,
    pub stage: Stage,
    pub protection: Protection,
}
impl Scope {
    pub fn validate(&self) -> Result<()> {
        if !SYMBOLS.contains(&self.instrument.as_str()) {
            return Err("cache scope instrument outside the seven-instrument scope".into());
        }
        if ![
            &self.dataset_hash,
            &self.strategy_hash,
            &self.costs_hash,
            &self.config_hash,
        ]
        .iter()
        .all(|value| hex(value))
        {
            return Err("cache scope requires 64-hex content identities".into());
        }
        if self.engine_version.is_empty()
            || self.engine_version.len() > 64
            || self.engine_version.trim() != self.engine_version
        {
            return Err("cache scope engine version is incomplete".into());
        }
        let span = self
            .end_exclusive_ms
            .checked_sub(self.start_ms)
            .ok_or("cache scope window overflow")?;
        if self.start_ms < 0 || span <= 0 || span > MAX_WINDOW_MS {
            return Err("cache scope window is outside declared bounds".into());
        }
        Ok(())
    }
    /// Exact, order-stable scope identity. The protection class is a mandatory
    /// qualifier of the stage dimension, never an optional attribute.
    pub fn identity(&self) -> Result<String> {
        self.validate()?;
        hash(&(
            SCOPE_VERSION,
            &self.instrument,
            &self.dataset_hash,
            &self.strategy_hash,
            &self.costs_hash,
            &self.engine_version,
            &self.config_hash,
            self.start_ms,
            self.end_exclusive_ms,
            self.stage.name(),
            self.protection.name(),
        ))
    }
}

/// Byte/count budget selected by the caller. Values must stay within the hard
/// ceilings above; a smaller budget is always legal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_artifact_bytes: usize,
    pub max_store_bytes: u64,
    pub max_artifacts: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_artifact_bytes: MAX_ARTIFACT_BYTES,
            max_store_bytes: MAX_STORE_BYTES,
            max_artifacts: MAX_ARTIFACTS,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        if self.max_artifact_bytes == 0
            || self.max_artifact_bytes > MAX_ARTIFACT_BYTES
            || self.max_store_bytes == 0
            || self.max_store_bytes > MAX_STORE_BYTES
            || self.max_artifacts == 0
            || self.max_artifacts > MAX_ARTIFACTS
            || (self.max_artifact_bytes as u64) > self.max_store_bytes
        {
            return Err("cache limits outside declared bounds".into());
        }
        Ok(())
    }
}

/// Identity-bearing, cold/warm invariant receipt. `status` is deliberately not
/// part of this type: a cold fill and a warm hit must compare exactly equal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub version: String,
    pub scope_id: String,
    pub stage: String,
    pub protection: String,
    pub instrument: String,
    pub dataset_hash: String,
    pub strategy_hash: String,
    pub costs_hash: String,
    pub engine_version: String,
    pub config_hash: String,
    pub start_ms: i64,
    pub end_exclusive_ms: i64,
    pub content_hash: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Cold,
    Warm,
}
impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Warm => "warm",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedArtifact {
    pub data: Vec<u8>,
    pub receipt: Receipt,
}

/// Canonical byte encoding for a decoded preparation payload.
pub fn encode(value: &Value) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|e| e.to_string())
}
/// Decode only after verifying the content hash, so equivalent decoded data is
/// never produced from a corrupted or substituted artifact.
pub fn decode(bytes: &[u8], content_hash: &str) -> Result<Value> {
    if hash_raw(bytes)? != content_hash {
        return Err("cache decoded content hash mismatch".into());
    }
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())
}
fn ensure_dir(path: &Path, create: bool) -> Result<()> {
    if create {
        match fs::create_dir(path) {
            Ok(()) => sync_dir(path.parent().ok_or("cache directory parent missing")?)?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    match fs::symlink_metadata(path).map_err(|e| e.to_string())? {
        meta if meta.is_dir() && !meta.file_type().is_symlink() => Ok(()),
        _ => Err("cache directory must be a regular directory, not a symlink".into()),
    }
}

fn staging_path(root: &Path) -> Result<PathBuf> {
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let name = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
    Ok(root.join(format!(".partial-{name}")))
}

/// A durable cache handle. Each handle serves exactly one protection class; a
/// standard handle cannot read or write protected scopes, and vice versa.
#[derive(Debug)]
pub struct PrepCache {
    connection: Connection,
    root: PathBuf,
    policy: Protection,
    limits: Limits,
}
impl PrepCache {
    /// Shared cache for unprotected scopes.
    pub fn open(root: &Path) -> Result<Self> {
        Self::open_with(root, Protection::Standard, Limits::default())
    }
    /// Isolated cache for protected (OOS/holdout) scopes.
    pub fn open_protected(root: &Path) -> Result<Self> {
        Self::open_with(root, Protection::Protected, Limits::default())
    }
    pub fn open_with(root: &Path, policy: Protection, limits: Limits) -> Result<Self> {
        limits.validate()?;
        if root.as_os_str().is_empty() {
            return Err("cache root path missing".into());
        }
        fs::create_dir_all(root).map_err(|e| e.to_string())?;
        ensure_dir(root, false)?;
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let database = root.join("prep-cache.sqlite");
        let connection = Connection::open(&database).map_err(|e| e.to_string())?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
            )
            .map_err(|e| e.to_string())?;
        initialize(&connection)?;
        let artifacts = root.join("artifacts");
        ensure_dir(&artifacts, true)?;
        ensure_dir(&artifacts.join(Protection::Standard.directory()), true)?;
        ensure_dir(&artifacts.join(Protection::Protected.directory()), true)?;
        Ok(Self {
            connection,
            root,
            policy,
            limits,
        })
    }
    pub fn policy(&self) -> Protection {
        self.policy
    }
    pub fn limits(&self) -> Limits {
        self.limits
    }

    fn authorize(&self, scope: &Scope) -> Result<()> {
        if scope.protection != self.policy {
            return Err("cache protection isolation violation".into());
        }
        Ok(())
    }
    fn make_receipt(
        &self,
        scope: &Scope,
        scope_id: &str,
        content_hash: &str,
        bytes: u64,
    ) -> Receipt {
        Receipt {
            version: RECEIPT_VERSION.to_owned(),
            scope_id: scope_id.to_owned(),
            stage: scope.stage.name().to_owned(),
            protection: scope.protection.name().to_owned(),
            instrument: scope.instrument.clone(),
            dataset_hash: scope.dataset_hash.clone(),
            strategy_hash: scope.strategy_hash.clone(),
            costs_hash: scope.costs_hash.clone(),
            engine_version: scope.engine_version.clone(),
            config_hash: scope.config_hash.clone(),
            start_ms: scope.start_ms,
            end_exclusive_ms: scope.end_exclusive_ms,
            content_hash: content_hash.to_owned(),
            bytes,
        }
    }
    fn artifact_path(&self, content_hash: &str, protection: Protection) -> Result<PathBuf> {
        if !hex(content_hash) {
            return Err("cache artifact hash is not a 64-hex identity".into());
        }
        Ok(self
            .root
            .join("artifacts")
            .join(protection.directory())
            .join(format!("{content_hash}.bin")))
    }
    fn read_artifact(
        &self,
        content_hash: &str,
        expected: usize,
        protection: Protection,
    ) -> Result<Vec<u8>> {
        let path = self.artifact_path(content_hash, protection)?;
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !meta.is_file()
            || meta.file_type().is_symlink()
            || meta.len() != expected as u64
            || expected > self.limits.max_artifact_bytes
        {
            return Err("cache artifact file type/size mismatch".into());
        }
        let mut bytes = Vec::new();
        File::open(&path)
            .map_err(|e| e.to_string())?
            .take(expected as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() != expected {
            return Err("cache artifact truncated or extended".into());
        }
        if hash_raw(&bytes)? != content_hash {
            return Err("cache artifact content hash mismatch".into());
        }
        Ok(bytes)
    }

    /// Read an exact scope. Returns `None` only for a genuine miss; any
    /// corrupted, mismatched, truncated or out-of-budget artifact is an error.
    pub fn get(&self, scope: &Scope) -> Result<Option<CachedArtifact>> {
        self.authorize(scope)?;
        let id = scope.identity()?;
        let row: Option<(String, String, i64, String, String)> = self
            .connection
            .query_row(
                "SELECT scope_json,content_hash,bytes,stage,protection FROM prep_artifacts WHERE scope_id=?1",
                [&id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some((scope_json, content_hash, length, stage, protection)) = row else {
            return Ok(None);
        };
        let stored: Scope = serde_json::from_str(&scope_json).map_err(|e| e.to_string())?;
        if &stored != scope
            || stored.identity()? != id
            || stage != scope.stage.name()
            || protection != scope.protection.name()
        {
            return Err("cache scope identity mismatch".into());
        }
        let length = usize::try_from(length).map_err(|_| "cache artifact length overflow")?;
        if length == 0 || length > self.limits.max_artifact_bytes {
            return Err("cache artifact length outside declared budget".into());
        }
        let data = self.read_artifact(&content_hash, length, scope.protection)?;
        Ok(Some(CachedArtifact {
            receipt: self.make_receipt(scope, &id, &content_hash, length as u64),
            data,
        }))
    }

    /// Store immutable bytes for an exact scope. Repeating an identical put is
    /// deduplicated: it verifies the existing artifact and creates no new row.
    pub fn put(&mut self, scope: &Scope, data: &[u8]) -> Result<Receipt> {
        self.authorize(scope)?;
        let id = scope.identity()?;
        if data.is_empty() || data.len() > self.limits.max_artifact_bytes {
            return Err("cache artifact length outside declared budget".into());
        }
        let content_hash = hash_raw(data)?;
        let tx = storage::writer_tx(&self.connection)?;
        let existing: Option<(String, i64)> = tx
            .query_row(
                "SELECT content_hash,bytes FROM prep_artifacts WHERE scope_id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some((hash, length)) = existing {
            let length = usize::try_from(length).map_err(|_| "cache artifact length overflow")?;
            if hash != content_hash || length != data.len() {
                return Err("cache scope identity collision".into());
            }
            // Deduplication still verifies physical bytes: a warm put refuses a
            // corrupted artifact instead of silently accepting the collision.
            if self.read_artifact(&hash, length, scope.protection)? != data {
                return Err("cache artifact content mismatch".into());
            }
            tx.commit().map_err(|e| e.to_string())?;
            return Ok(self.make_receipt(scope, &id, &hash, length as u64));
        }
        let (count, stored_bytes): (i64, i64) = tx
            .query_row(
                "SELECT count(*),coalesce(sum(bytes),0) FROM prep_artifacts",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        if count as usize >= self.limits.max_artifacts
            || (stored_bytes as u64).saturating_add(data.len() as u64) > self.limits.max_store_bytes
        {
            return Err("cache store budget exceeded".into());
        }
        let physical = self.scan()?;
        if physical.files.saturating_add(1) > self.limits.max_artifacts
            || physical.bytes.saturating_add(data.len() as u64) > self.limits.max_store_bytes
        {
            return Err("cache physical store budget exceeded".into());
        }
        self.publish(&content_hash, data, scope.protection)?;
        let scope_json = serde_json::to_string(scope).map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO prep_artifacts(scope_id,scope_json,stage,protection,content_hash,bytes) VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                id,
                scope_json,
                scope.stage.name(),
                scope.protection.name(),
                content_hash,
                data.len() as i64,
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(self.make_receipt(scope, &id, &content_hash, data.len() as u64))
    }

    fn publish(&self, content_hash: &str, data: &[u8], protection: Protection) -> Result<()> {
        let directory = self.root.join("artifacts").join(protection.directory());
        let destination = directory.join(format!("{content_hash}.bin"));
        if destination.try_exists().map_err(|e| e.to_string())? {
            if self.read_artifact(content_hash, data.len(), protection)? != data {
                return Err("existing cache artifact mismatch".into());
            }
            return Ok(());
        }
        let staging = staging_path(&self.root.join("artifacts"))?;
        let result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staging)
                .map_err(|e| e.to_string())?;
            file.write_all(data)
                .and_then(|()| file.sync_all())
                .map_err(|e| e.to_string())?;
            let staged = fs::read(&staging).map_err(|e| e.to_string())?;
            if hash_raw(&staged)? != content_hash {
                return Err("staged cache artifact hash mismatch".into());
            }
            match fs::hard_link(&staging, &destination) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if self.read_artifact(content_hash, data.len(), protection)? != data {
                        return Err("concurrent cache artifact mismatch".into());
                    }
                }
                Err(e) => return Err(e.to_string()),
            }
            sync_dir(&directory)?;
            Ok(())
        })();
        // On failure the staged file is preserved for inspection; recovery is
        // explicit and never guesses the intent of an interrupted write.
        let cleanup = fs::remove_file(&staging);
        if result.is_ok() {
            cleanup.map_err(|e| e.to_string())?;
            sync_dir(&self.root.join("artifacts"))?;
        }
        result
    }

    /// Fill on a miss, reuse on a hit. The producer is never called on a warm
    /// hit, which is what makes cold/warm parity mechanically observable.
    pub fn get_or_fill<F>(&mut self, scope: &Scope, fill: F) -> Result<(CachedArtifact, Status)>
    where
        F: FnOnce() -> Result<Vec<u8>>,
    {
        if let Some(hit) = self.get(scope)? {
            return Ok((hit, Status::Warm));
        }
        let data = fill()?;
        let receipt = self.put(scope, &data)?;
        Ok((CachedArtifact { data, receipt }, Status::Cold))
    }

    /// Exact receipt for a stored scope without loading its payload.
    pub fn receipt(&self, scope: &Scope) -> Result<Option<Receipt>> {
        Ok(self.get(scope)?.map(|hit| hit.receipt))
    }

    fn scan(&self) -> Result<Scan> {
        let artifacts = self.root.join("artifacts");
        let mut partial = 0usize;
        for entry in fs::read_dir(&artifacts).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            let name = name.to_str().ok_or("invalid cache staging filename")?;
            if name == Protection::Standard.directory() || name == Protection::Protected.directory()
            {
                let meta = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    return Err("invalid cache artifact class directory".into());
                }
                continue;
            }
            let Some(remainder) = name.strip_prefix(".partial-") else {
                return Err("unrecognized cache staging entry; preserve and inspect".into());
            };
            if !hex(remainder) {
                return Err("unrecognized cache staging entry; preserve and inspect".into());
            }
            let meta = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err("invalid cache staging entry type".into());
            }
            partial += 1;
        }
        let (standard_files, standard_bytes) = scan_class(
            &artifacts.join(Protection::Standard.directory()),
            self.limits.max_artifact_bytes,
        )?;
        let (protected_files, protected_bytes) = scan_class(
            &artifacts.join(Protection::Protected.directory()),
            self.limits.max_artifact_bytes,
        )?;
        let files = standard_files.saturating_add(protected_files);
        let bytes = standard_bytes.saturating_add(protected_bytes);
        if files > self.limits.max_artifacts || bytes > self.limits.max_store_bytes {
            return Err("cache physical budget exceeded".into());
        }
        Ok(Scan {
            files,
            bytes,
            partial,
            standard_files,
            protected_files,
        })
    }

    /// Metadata byte accounting. Counting summed file lengths, not allocated
    /// blocks; full content verification is `integrity`.
    pub fn inventory(&self) -> Result<Value> {
        let scan = self.scan()?;
        let (rows, indexed): (i64, i64) = self
            .connection
            .query_row(
                "SELECT count(*),coalesce(sum(bytes),0) FROM prep_artifacts",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        Ok(json!({
            "version": "prep-cache-inventory-v1",
            "policy": self.policy.name(),
            "indexed_artifacts": rows.to_string(),
            "indexed_bytes": indexed.to_string(),
            "physical_files": scan.files.to_string(),
            "physical_bytes": scan.bytes.to_string(),
            "partial_files": scan.partial.to_string(),
            "standard_files": scan.standard_files.to_string(),
            "protected_files": scan.protected_files.to_string(),
            "maximum_artifact_bytes": self.limits.max_artifact_bytes.to_string(),
            "maximum_store_bytes": self.limits.max_store_bytes.to_string(),
            "maximum_artifacts": self.limits.max_artifacts.to_string(),
            "automatic_deletion": false,
            "limitations": "Metadata inventory only: byte counts sum artifact file lengths, not allocated filesystem blocks. Staged .partial files are never read and consume no indexed bytes. Full content verification is registry_check/integrity. Not an OS disk quota."
        }))
    }

    /// Verify every indexed artifact in this handle's protection class: exact
    /// scope identity, declared stage and protection, file size, content hash.
    /// The other class has its own isolated handle and is not read here.
    pub fn integrity(&self) -> Result<()> {
        let mut query = self
            .connection
            .prepare("SELECT scope_id,scope_json,stage,protection,content_hash,bytes FROM prep_artifacts WHERE protection=?1 ORDER BY scope_id")
            .map_err(|e| e.to_string())?;
        let rows = query
            .query_map([self.policy.name()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (id, json, stage, protection, content_hash, length) =
                row.map_err(|e| e.to_string())?;
            let scope: Scope = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            if scope.identity()? != id
                || stage != scope.stage.name()
                || protection != scope.protection.name()
                || scope.protection != self.policy
            {
                return Err("cache indexed scope identity mismatch".into());
            }
            let length = usize::try_from(length).map_err(|_| "cache artifact length overflow")?;
            if length == 0 || length > self.limits.max_artifact_bytes {
                return Err("cache indexed length outside declared budget".into());
            }
            self.read_artifact(&content_hash, length, scope.protection)?;
        }
        self.scan()?;
        Ok(())
    }

    /// Remove only valid `.partial-*` staging remnants left by an interrupted
    /// write. Finalized artifacts are immutable and never removed. An invalid
    /// store entry is refused rather than cleaned by guesswork.
    pub fn recover_partial(&mut self) -> Result<usize> {
        let tx = storage::writer_tx(&self.connection)?;
        let scan = self.scan()?;
        let mut removed = 0usize;
        let artifacts = self.root.join("artifacts");
        if scan.partial > 0 {
            for entry in fs::read_dir(&artifacts).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let name = entry.file_name();
                let name = name.to_str().ok_or("invalid cache staging filename")?;
                if name.strip_prefix(".partial-").is_some_and(hex) {
                    fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
                    removed += 1;
                }
            }
            sync_dir(&artifacts)?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(removed)
    }
}

#[derive(Default)]
struct Scan {
    files: usize,
    bytes: u64,
    partial: usize,
    standard_files: usize,
    protected_files: usize,
}
fn scan_class(dir: &Path, limit: usize) -> Result<(usize, u64)> {
    let mut files = 0usize;
    let mut bytes = 0u64;
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name = name.to_str().ok_or("invalid cache artifact filename")?;
        if !name.strip_suffix(".bin").is_some_and(hex) {
            return Err("unrecognized cache artifact entry; preserve and inspect".into());
        }
        let meta = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > limit as u64 {
            return Err("invalid cache artifact entry type/size".into());
        }
        files += 1;
        bytes = bytes.checked_add(meta.len()).ok_or("cache byte overflow")?;
    }
    Ok((files, bytes))
}

fn initialize(connection: &Connection) -> Result<()> {
    let tx = storage::writer_tx_open(connection).map_err(|e| e.to_string())?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS prep_artifacts(
            scope_id TEXT PRIMARY KEY CHECK(length(scope_id)=64),
            scope_json TEXT NOT NULL,
            stage TEXT NOT NULL CHECK(stage IN ('candle','feature','decision')),
            protection TEXT NOT NULL CHECK(protection IN ('standard','protected')),
            content_hash TEXT NOT NULL CHECK(length(content_hash)=64),
            bytes INTEGER NOT NULL CHECK(bytes>0 AND bytes<=16777216)
        );
        CREATE TRIGGER IF NOT EXISTS prep_artifact_no_update BEFORE UPDATE ON prep_artifacts BEGIN SELECT RAISE(ABORT,'prep artifact immutable'); END;
        CREATE TRIGGER IF NOT EXISTS prep_artifact_no_delete BEFORE DELETE ON prep_artifacts BEGIN SELECT RAISE(ABORT,'prep artifact immutable'); END;",
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}
