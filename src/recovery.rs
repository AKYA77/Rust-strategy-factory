//! Private bounded registry snapshots and new-path recovery. No active service cutover or deletion.
use crate::{Result, data, research, storage::Registry};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
const MAX_SNAPSHOTS: usize = 16;
const MAX_ROOT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_DATABASE_BYTES: u64 = 512 * 1024 * 1024;
const RESERVE: u64 = 128 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Artifact {
    name: String,
    bytes: u64,
    hash: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    version: String,
    id: String,
    operation: String,
    parent_id: Option<String>,
    created_at_ms: i64,
    compute_revision: String,
    schema_version: u32,
    database: Artifact,
    artifacts: Vec<Artifact>,
    counts: serde_json::Value,
    revoked_leases: usize,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    body: Body,
    hash: String,
}
fn io(e: std::io::Error) -> String {
    e.to_string()
}
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && id.as_bytes()[0].is_ascii_alphanumeric()
}
fn root(database: &Path) -> Result<PathBuf> {
    if database == Path::new(":memory:") {
        return Err("managed recovery requires a filesystem registry".into());
    }
    let db = database.canonicalize().map_err(io)?;
    if !db.is_file() {
        return Err("recovery requires a filesystem registry".into());
    }
    let mut name = db
        .file_name()
        .ok_or("registry filename missing")?
        .to_os_string();
    name.push(".recovery");
    Ok(db.with_file_name(name))
}
#[cfg(unix)]
fn private(path: &Path, directory: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let m = fs::symlink_metadata(path).map_err(io)?;
    if m.file_type().is_symlink()
        || m.is_dir() != directory
        || !directory && !m.is_file()
        || m.permissions().mode() & 0o077 != 0
    {
        return Err("recovery entry must be private and without symlink indirection".into());
    }
    Ok(())
}
#[cfg(not(unix))]
fn private(_path: &Path, _directory: bool) -> Result<()> {
    Err("managed private recovery currently requires Unix permissions".into())
}
fn mkdir(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(io)?;
    private(path, true)
}
fn secure_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(io)?;
    }
    private(path, false)
}
fn sync(path: &Path) -> Result<()> {
    File::open(path).and_then(|f| f.sync_all()).map_err(io)
}
fn lock(root: &Path) -> Result<File> {
    if !root.exists() {
        mkdir(root)?;
        sync(root.parent().ok_or("recovery root parent missing")?)?;
    }
    private(root, true)?;
    let path = root.join(".operations.lock");
    if fs::symlink_metadata(&path).is_ok() {
        private(&path, false)?;
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path).map_err(io)?;
    fs2::FileExt::try_lock_exclusive(&file).map_err(|_| "another recovery operation is active")?;
    Ok(file)
}
fn artifact(path: &Path, name: &str, maximum: u64) -> Result<Artifact> {
    private(path, false)?;
    let bytes = fs::metadata(path).map_err(io)?.len();
    if bytes > maximum {
        return Err("recovery file exceeds byte budget".into());
    }
    let hash = data::file_hash(path)?;
    if fs::metadata(path).map_err(io)?.len() != bytes {
        return Err("recovery file changed length".into());
    }
    Ok(Artifact {
        name: name.into(),
        bytes,
        hash,
    })
}
fn artifacts(directory: &Path, secure: bool) -> Result<Vec<Artifact>> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    if secure {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).map_err(io)?;
        }
    }
    private(directory, true)?;
    let mut rows = Vec::new();
    let mut total = 0u64;
    for e in fs::read_dir(directory).map_err(io)? {
        let e = e.map_err(io)?;
        if rows.len() >= 4096 {
            return Err("recovery artifact count exceeded".into());
        }
        let name = e
            .file_name()
            .into_string()
            .map_err(|_| "non-UTF8 recovery artifact")?;
        if !name
            .strip_suffix(".json")
            .is_some_and(crate::machine_evidence::hex)
        {
            return Err("unexpected recovery artifact name".into());
        }
        if secure {
            secure_file(&e.path())?;
        }
        let entry = artifact(&e.path(), &name, 64 * 1024 * 1024)?;
        total = total
            .checked_add(entry.bytes)
            .ok_or("recovery artifact byte overflow")?;
        if total > 1024 * 1024 * 1024 {
            return Err("recovery artifact byte cap exceeded".into());
        }
        rows.push(entry);
    }
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(rows)
}
fn counts(reg: &Registry) -> Result<serde_json::Value> {
    let c = reg.counts()?;
    Ok(
        serde_json::json!({"registered":c.registered.to_string(),"completed":c.completed.to_string(),"running":c.running.to_string(),"attempts":c.attempts.to_string()}),
    )
}
fn read_manifest(directory: &Path, id: &str) -> Result<Manifest> {
    private(directory, true)?;
    let path = directory.join("manifest.json");
    private(&path, false)?;
    let mut bytes = Vec::new();
    File::open(&path)
        .map_err(io)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() > 1024 * 1024 {
        return Err("recovery manifest exceeds1MiB".into());
    }
    let m: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let artifact_bytes = m.body.artifacts.iter().try_fold(0u64, |n, a| {
        if a.bytes > 64 * 1024 * 1024
            || !a
                .name
                .strip_suffix(".json")
                .is_some_and(crate::machine_evidence::hex)
            || !crate::machine_evidence::hex(&a.hash)
        {
            return Err("invalid recovery artifact manifest");
        }
        n.checked_add(a.bytes)
            .ok_or("recovery artifact total overflow")
    })?;
    if artifact_bytes > 1024 * 1024 * 1024
        || m.body.database.bytes == 0
        || !crate::machine_evidence::hex(&m.body.database.hash)
        || m.body.schema_version == 0
        || m.body.operation == "backup" && m.body.parent_id.is_some()
        || m.body.operation == "restore" && m.body.parent_id.is_none()
    {
        return Err("invalid recovery manifest scope/bounds".into());
    }
    for field in ["registered", "completed", "running", "attempts"] {
        if m.body.counts[field]
            .as_str()
            .and_then(|v| v.parse::<i64>().ok())
            .is_none_or(|n| n < 0)
        {
            return Err("invalid recovery count manifest".into());
        }
    }
    if m.body.version != "managed-registry-recovery-v1"
        || m.body.id != id
        || !valid_id(id)
        || !matches!(m.body.operation.as_str(), "backup" | "restore")
        || m.body.parent_id.as_ref().is_some_and(|s| !valid_id(s))
        || m.body.database.name != "registry.sqlite"
        || m.body.database.bytes > MAX_DATABASE_BYTES
        || m.body.artifacts.len() > 4096
        || chrono::DateTime::from_timestamp_millis(m.body.created_at_ms).is_none()
        || data::hash(&m.body)? != m.hash
    {
        return Err("recovery manifest integrity mismatch".into());
    }
    Ok(m)
}
fn public(m: &Manifest, fresh: bool) -> serde_json::Value {
    serde_json::json!({"id":m.body.id,"operation":m.body.operation,"parent_id":m.body.parent_id,"created_at_ms":m.body.created_at_ms.to_string(),"compute_revision":m.body.compute_revision,"schema_version":m.body.schema_version.to_string(),"database_bytes":m.body.database.bytes.to_string(),"database_hash":m.body.database.hash,"artifact_files":m.body.artifacts.len().to_string(),"artifact_bytes":m.body.artifacts.iter().map(|a|a.bytes).sum::<u64>().to_string(),"counts":m.body.counts,"revoked_leases":m.body.revoked_leases.to_string(),"manifest_hash":m.hash,"status":"complete","fresh_content_verified":fresh,"verified_at_ms":fresh.then(||chrono::Utc::now().timestamp_millis().to_string()),"compatibility":if m.body.schema_version==crate::storage::SCHEMA_VERSION{"current schema; full registry integrity checked when fresh"}else{"older schema; full current integrity deferred to migrated new copy"},"activation_performed":false,"scope":"Registry and referenced receipt artifacts only. Source archives, credentials and deployment configuration are separate. Unclosed process observations remain unknown; no PID ownership or active cutover is granted."})
}
fn tree_size(path: &Path, depth: usize, entries: &mut usize) -> Result<u64> {
    if depth > 3 {
        return Err("unexpected recovery nesting".into());
    }
    let m = fs::symlink_metadata(path).map_err(io)?;
    if m.file_type().is_symlink() {
        return Err("recovery symlink refused".into());
    }
    if m.is_file() {
        return Ok(m.len());
    }
    if !m.is_dir() {
        return Err("unexpected recovery file type".into());
    }
    let mut bytes = 0u64;
    for e in fs::read_dir(path).map_err(io)? {
        *entries += 1;
        if *entries > 16 * (4096 + 8) + 1 {
            return Err("recovery physical inventory cap".into());
        }
        bytes = bytes
            .checked_add(tree_size(&e.map_err(io)?.path(), depth + 1, entries)?)
            .ok_or("recovery physical size overflow")?;
    }
    Ok(bytes)
}
fn admission(root: &Path, estimate: u64) -> Result<()> {
    let mut folders = 0;
    for e in fs::read_dir(root).map_err(io)? {
        let e = e.map_err(io)?;
        if e.file_name() == ".operations.lock" {
            continue;
        }
        folders += 1;
        if folders >= MAX_SNAPSHOTS {
            return Err("recovery inventory full; preserve snapshots".into());
        }
        if !e.file_type().map_err(io)?.is_dir() || !e.file_name().to_str().is_some_and(valid_id) {
            return Err("unexpected recovery root entry".into());
        }
    }
    let used = tree_size(root, 0, &mut 0)?;
    if used
        .checked_add(estimate)
        .is_none_or(|n| n > MAX_ROOT_BYTES)
        || fs2::available_space(root).map_err(io)? < estimate.saturating_add(RESERVE)
    {
        return Err("recovery byte/disk reserve admission refused".into());
    }
    Ok(())
}
fn offline(path: &Path) -> Result<()> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        if fs::symlink_metadata(PathBuf::from(name)).is_ok() {
            return Err("recovery snapshot must be offline without sidecars".into());
        }
    }
    Ok(())
}
fn verify_at(directory: &Path, id: &str) -> Result<Manifest> {
    let m = read_manifest(directory, id)?;
    let db = directory.join("registry.sqlite");
    offline(&db)?;
    if artifact(&db, "registry.sqlite", MAX_DATABASE_BYTES)? != m.body.database
        || artifacts(&directory.join("registry.sqlite.artifacts"), false)? != m.body.artifacts
    {
        return Err("snapshot content differs from immutable manifest".into());
    }
    let connection = Connection::open_with_flags(
        &db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())?;
    let mode: String = connection
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if mode != "delete" {
        return Err("snapshot journal mode is not sealed offline".into());
    }
    let reg = Registry { connection };
    let schema: u32 = reg
        .connection
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if schema != m.body.schema_version || schema > crate::storage::SCHEMA_VERSION {
        return Err("snapshot schema unavailable/newer than supported".into());
    }
    if schema == crate::storage::SCHEMA_VERSION {
        reg.integrity()?;
    } else {
        let result: String = reg
            .connection
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if result != "ok" {
            return Err("older snapshot SQLite integrity failure".into());
        }
    }
    if counts(&reg)? != m.body.counts {
        return Err("snapshot counts differ from manifest".into());
    }
    drop(reg);
    offline(&db)?;
    Ok(m)
}
fn finish(
    directory: &Path,
    id: &str,
    operation: &str,
    parent: Option<&str>,
    revoked: usize,
) -> Result<Manifest> {
    let db = directory.join("registry.sqlite");
    {
        let reg = Registry::open(&db)?;
        reg.integrity()?;
        reg.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);PRAGMA journal_mode=DELETE;")
            .map_err(|e| e.to_string())?;
    }
    secure_file(&db)?;
    offline(&db)?;
    let files = artifacts(&directory.join("registry.sqlite.artifacts"), true)?;
    let reg = Registry {
        connection: Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?,
    };
    let schema = reg
        .connection
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let body = Body {
        version: "managed-registry-recovery-v1".into(),
        id: id.into(),
        operation: operation.into(),
        parent_id: parent.map(str::to_owned),
        created_at_ms: chrono::Utc::now().timestamp_millis(),
        compute_revision: research::compute_revision()?,
        schema_version: schema,
        database: artifact(&db, "registry.sqlite", MAX_DATABASE_BYTES)?,
        artifacts: files,
        counts: counts(&reg)?,
        revoked_leases: revoked,
    };
    drop(reg);
    let m = Manifest {
        hash: data::hash(&body)?,
        body,
    };
    let bytes = serde_json::to_vec(&m).map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("recovery manifest cap exceeded".into());
    }
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let temporary = directory.join(".manifest.pending");
    let mut file = options.open(&temporary).map_err(io)?;
    file.write_all(&bytes).map_err(io)?;
    file.sync_all().map_err(io)?;
    fs::hard_link(&temporary, directory.join("manifest.json")).map_err(io)?;
    drop(file);
    fs::remove_file(temporary).map_err(io)?;
    sync(directory)?;
    sync(directory.parent().ok_or("recovery parent missing")?)?;
    verify_at(directory, id)
}
pub fn list(database: &Path) -> Result<serde_json::Value> {
    let root = root(database)?;
    if !root.exists() {
        return Ok(
            serde_json::json!({"rows":[],"maximum_snapshots":MAX_SNAPSHOTS.to_string(),"maximum_bytes":MAX_ROOT_BYTES.to_string()}),
        );
    }
    private(&root, true)?;
    let mut rows = Vec::new();
    for e in fs::read_dir(&root).map_err(io)? {
        let e = e.map_err(io)?;
        if e.file_name() == ".operations.lock" {
            continue;
        }
        if rows.len() >= MAX_SNAPSHOTS {
            return Err("recovery inventory cap exceeded".into());
        }
        let id = e
            .file_name()
            .into_string()
            .map_err(|_| "invalid recovery name")?;
        if !valid_id(&id) {
            return Err("unexpected recovery root entry".into());
        }
        private(&e.path(), true)?;
        if e.path().join("manifest.json").exists() {
            rows.push(public(&read_manifest(&e.path(), &id)?, false));
        } else {
            rows.push(serde_json::json!({"id":id,"status":"incomplete; retained for inspection","fresh_content_verified":false,"activation_performed":false}));
        }
    }
    rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    Ok(
        serde_json::json!({"rows":rows,"maximum_snapshots":MAX_SNAPSHOTS.to_string(),"maximum_bytes":MAX_ROOT_BYTES.to_string(),"scope":"Manifest inventory only; verify rechecks actual snapshot bytes and registry integrity. No automatic deletion or active service cutover."}),
    )
}
pub fn describe(database: &Path, id: &str) -> Result<Option<serde_json::Value>> {
    if !valid_id(id) {
        return Err("invalid recovery identity".into());
    }
    let root = root(database)?;
    if !root.exists() {
        return Ok(None);
    }
    private(&root, true)?;
    let directory = root.join(id);
    if fs::symlink_metadata(&directory).is_err() {
        return Ok(None);
    }
    private(&directory, true)?;
    if fs::symlink_metadata(directory.join("manifest.json")).is_err() {
        return Ok(None);
    }
    Ok(Some(public(&read_manifest(&directory, id)?, false)))
}
pub fn verify(database: &Path, id: &str) -> Result<serde_json::Value> {
    if !valid_id(id) {
        return Err("invalid recovery identity".into());
    }
    let root = root(database)?;
    let _guard = lock(&root)?;
    Ok(public(&verify_at(&root.join(id), id)?, true))
}
pub fn backup(database: &Path, id: &str) -> Result<serde_json::Value> {
    if !valid_id(id) {
        return Err("invalid recovery identity".into());
    }
    let root = root(database)?;
    let _guard = lock(&root)?;
    if fs::metadata(database).map_err(io)?.len() > MAX_DATABASE_BYTES {
        return Err("primary registry exceeds snapshot database cap".into());
    }
    let reg = Registry::open(database)?;
    let inventory = crate::result_artifact::inventory(&reg)?;
    let mut estimate = fs::metadata(database).map_err(io)?.len();
    if estimate > MAX_DATABASE_BYTES {
        return Err("primary registry exceeds snapshot database cap".into());
    }
    let mut wal = database.as_os_str().to_os_string();
    wal.push("-wal");
    estimate = estimate.saturating_add(
        fs::metadata(PathBuf::from(wal))
            .map(|m| m.len())
            .unwrap_or(0),
    );
    estimate = estimate.saturating_add(
        inventory["physical_bytes"]
            .as_str()
            .ok_or("artifact byte inventory missing")?
            .parse::<u64>()
            .map_err(|_| "artifact byte inventory invalid")?,
    );
    admission(&root, estimate)?;
    let directory = root.join(id);
    mkdir(&directory)?;
    reg.backup(&directory.join("registry.sqlite"))?;
    let manifest = finish(&directory, id, "backup", None, 0)?;
    if tree_size(&root, 0, &mut 0)? > MAX_ROOT_BYTES {
        return Err("recovery root grew beyond admission cap; snapshot retained".into());
    }
    Ok(public(&manifest, true))
}
fn copy(source: &Path, destination: &Path, expected: &Artifact) -> Result<()> {
    private(source, false)?;
    let mut incoming = File::open(source).map_err(io)?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut out = options.open(destination).map_err(io)?;
    let copied = std::io::copy(
        &mut std::io::Read::by_ref(&mut incoming).take(expected.bytes + 1),
        &mut out,
    )
    .map_err(io)?;
    if copied != expected.bytes {
        return Err("restore source changed length".into());
    }
    out.sync_all().map_err(io)?;
    if artifact(destination, &expected.name, expected.bytes)?.hash != expected.hash {
        return Err("restore copied bytes differ from snapshot".into());
    }
    Ok(())
}
pub fn restore(database: &Path, source_id: &str, id: &str) -> Result<serde_json::Value> {
    restore_expected(database, source_id, id, None)
}
pub fn restore_expected(
    database: &Path,
    source_id: &str,
    id: &str,
    expected_manifest: Option<&str>,
) -> Result<serde_json::Value> {
    if !valid_id(source_id) || !valid_id(id) || id == source_id {
        return Err("invalid distinct restore identity".into());
    }
    let root = root(database)?;
    let _guard = lock(&root)?;
    let source = root.join(source_id);
    let manifest = verify_at(&source, source_id)?;
    if expected_manifest.is_some_and(|h| h != manifest.hash) {
        return Err("reviewed snapshot manifest changed".into());
    }
    let estimate =
        manifest.body.database.bytes + manifest.body.artifacts.iter().map(|a| a.bytes).sum::<u64>();
    admission(&root, estimate)?;
    let destination = root.join(id);
    mkdir(&destination)?;
    let db = destination.join("registry.sqlite");
    copy(
        &source.join("registry.sqlite"),
        &db,
        &manifest.body.database,
    )?;
    if !manifest.body.artifacts.is_empty() {
        let files = destination.join("registry.sqlite.artifacts");
        mkdir(&files)?;
        for a in &manifest.body.artifacts {
            copy(
                &source.join("registry.sqlite.artifacts").join(&a.name),
                &files.join(&a.name),
                a,
            )?;
        }
        sync(&files)?;
    }
    let mut reg = Registry::open(&db)?;
    let tx = reg.connection.transaction().map_err(|e| e.to_string())?;
    let mut revoked = 0;
    for table in [
        "experiments",
        "research_runs",
        "corpus_searches",
        "corpus_fleets",
        "corpus_campaign_runs",
        "corpus_campaign_rotations",
    ] {
        let n=tx.execute(&format!("UPDATE {table} SET token=NULL,expires_ms=NULL{} WHERE (token IS NOT NULL OR expires_ms IS NOT NULL){}",if table=="experiments"{",state=CASE WHEN state='running' THEN 'queued' ELSE state END"}else{""},if table=="experiments"{" AND state IN ('queued','running')"}else{""}),[]).map_err(|e|e.to_string())?;
        revoked += n;
    }
    tx.commit().map_err(|e| e.to_string())?;
    reg.integrity()?;
    drop(reg);
    let restored = finish(&destination, id, "restore", Some(source_id), revoked)?;
    verify_at(&source, source_id)?;
    Ok(public(&restored, true))
}
pub fn path(database: &Path, id: &str) -> Result<PathBuf> {
    if !valid_id(id) {
        return Err("invalid recovery identity".into());
    }
    Ok(root(database)?.join(id).join("registry.sqlite"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Claim, ExperimentKey, Outcome, Stage};
    fn fixture(name: &str) -> (PathBuf, PathBuf, Registry) {
        let root =
            std::env::temp_dir().join(format!("managed-recovery-{name}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let db = root.join("state.sqlite");
        let reg = Registry::open(&db).unwrap();
        (root, db, reg)
    }
    fn key(name: &str) -> ExperimentKey {
        ExperimentKey {
            concept_hash: name.into(),
            instrument: "EURUSD".into(),
            dataset_hash: "synthetic-storage-fixture".into(),
            start_ms: 0,
            end_exclusive_ms: 1000,
            signal_version: "s1".into(),
            indicator_version: "i1".into(),
            cost_hash: "cost".into(),
            fill_policy: "fill".into(),
            profile_hash: "synthetic-profile".into(),
            engine_version: "e1".into(),
            protocol_hash: "test-only".into(),
            code_revision: "fixture".into(),
            configuration_hash: "config".into(),
            stage: Stage::TickVerification,
        }
    }
    #[test]
    fn committed_wal_and_file_receipts_restore_with_new_actor_fences_and_unchanged_snapshot() {
        let (directory, db, mut source) = fixture("wal");
        let result_id = source.register(&key("completed")).unwrap();
        let Claim::Acquired(lease) = source.claim(&result_id, 1000, 100000).unwrap() else {
            panic!()
        };
        source.finish(&lease,1001,Outcome::Completed,&serde_json::json!({"synthetic_storage_test_only":true,"padding":"x".repeat(300000)})).unwrap();
        let active = source.register(&key("active")).unwrap();
        let Claim::Acquired(old) = source.claim(&active, 1000, 100000).unwrap() else {
            panic!()
        };
        assert!(db.with_extension("sqlite-wal").exists());
        let original = source.result(&result_id).unwrap().unwrap();
        let snapshot = backup(&db, "one").unwrap();
        assert_eq!(snapshot["artifact_files"], "1");
        assert_eq!(snapshot["counts"]["running"], "1");
        assert_eq!(snapshot["fresh_content_verified"], true);
        let sealed = snapshot["database_hash"].clone();
        let restored = restore(&db, "one", "recovered").unwrap();
        assert_eq!(restored["activation_performed"], false);
        assert_eq!(restored["revoked_leases"], "1");
        assert_eq!(restored["counts"]["running"], "0");
        assert_eq!(verify(&db, "one").unwrap()["database_hash"], sealed);
        let mut target = Registry::open(&path(&db, "recovered").unwrap()).unwrap();
        assert_eq!(
            target.result(&result_id).unwrap().unwrap().result_hash,
            original.result_hash
        );
        assert!(
            target
                .finish(
                    &old,
                    1002,
                    Outcome::Failed,
                    &serde_json::json!({"test":"stale copied actor"})
                )
                .is_err()
        );
        let Claim::Acquired(new) = target.claim(&active, 1002, 100000).unwrap() else {
            panic!()
        };
        target
            .finish(
                &new,
                1003,
                Outcome::Failed,
                &serde_json::json!({"synthetic_storage_test_only":"new actor"}),
            )
            .unwrap();
        target.integrity().unwrap();
        assert_eq!(source.counts().unwrap().running, 1);
        assert_eq!(verify(&db, "one").unwrap()["database_hash"], sealed);
        assert!(
            !list(&db)
                .unwrap()
                .to_string()
                .contains(directory.to_str().unwrap())
        );
        drop(target);
        drop(source);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn invalid_names_private_modes_and_no_replace_preserve_existing_bytes() {
        let (directory, db, reg) = fixture("privacy");
        for id in ["", "../escape", "/tmp/escape", "Upper", "-bad", "x/y"] {
            assert!(backup(&db, id).is_err());
        }
        assert!(!root(&db).unwrap().exists());
        backup(&db, "one").unwrap();
        let file = path(&db, "one").unwrap();
        let before = data::file_hash(&file).unwrap();
        assert!(backup(&db, "one").is_err());
        assert!(restore(&db, "one", "one").is_err());
        assert_eq!(data::file_hash(&file).unwrap(), before);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&file).unwrap().permissions().mode() & 0o777,
                0o600
            );
            let managed = root(&db).unwrap();
            assert_eq!(
                fs::metadata(&managed).unwrap().permissions().mode() & 0o777,
                0o700
            );
            fs::set_permissions(&managed, fs::Permissions::from_mode(0o755)).unwrap();
            assert!(list(&db).is_err());
            assert!(verify(&db, "one").is_err());
            fs::set_permissions(&managed, fs::Permissions::from_mode(0o700)).unwrap();
        }
        reg.integrity().unwrap();
        drop(reg);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn corruption_and_live_sidecars_refuse_before_any_restore_destination() {
        let (directory, db, reg) = fixture("corruption");
        backup(&db, "one").unwrap();
        let file = path(&db, "one").unwrap();
        let original = fs::read(&file).unwrap();
        let mut bytes = original.clone();
        bytes[0] ^= 1;
        fs::write(&file, bytes).unwrap();
        assert!(verify(&db, "one").is_err());
        assert!(restore(&db, "one", "bad-copy").is_err());
        assert!(!root(&db).unwrap().join("bad-copy").exists());
        fs::write(&file, original).unwrap();
        let mut sidecar = file.as_os_str().to_os_string();
        sidecar.push("-wal");
        fs::write(PathBuf::from(&sidecar), []).unwrap();
        assert!(verify(&db, "one").is_err());
        fs::remove_file(PathBuf::from(sidecar)).unwrap();
        let manifest = file.parent().unwrap().join("manifest.json");
        let mut m: Manifest = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        m.body.artifacts.push(Artifact {
            name: format!("{}.json", "a".repeat(64)),
            bytes: u64::MAX,
            hash: "b".repeat(64),
        });
        m.hash = data::hash(&m.body).unwrap();
        fs::write(&manifest, serde_json::to_vec(&m).unwrap()).unwrap();
        assert!(verify(&db, "one").is_err());
        assert!(list(&db).is_err());
        drop(reg);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn incomplete_capacity_byte_pressure_and_concurrent_operation_never_delete_evidence() {
        let (directory, db, reg) = fixture("admission");
        let managed = root(&db).unwrap();
        let guard = lock(&managed).unwrap();
        assert!(backup(&db, "busy").is_err());
        drop(guard);
        let partial = managed.join("partial");
        mkdir(&partial).unwrap();
        let sparse = partial.join("registry.sqlite");
        File::create(&sparse)
            .unwrap()
            .set_len(MAX_ROOT_BYTES)
            .unwrap();
        assert!(backup(&db, "too-large").is_err());
        assert!(sparse.exists());
        fs::remove_file(sparse).unwrap();
        for i in 1..16 {
            mkdir(&managed.join(format!("partial-{i}"))).unwrap();
        }
        assert_eq!(list(&db).unwrap()["rows"].as_array().unwrap().len(), 16);
        assert!(backup(&db, "full").is_err());
        assert!(!managed.join("full").exists());
        assert!(partial.exists());
        drop(reg);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    #[cfg(unix)]
    fn symlink_managed_roots_are_refused_without_touching_the_target() {
        let (directory, db, reg) = fixture("symlink");
        let outside = directory.join("outside");
        mkdir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root(&db).unwrap()).unwrap();
        assert!(backup(&db, "one").is_err());
        assert!(list(&db).is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        drop(reg);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn restored_unclosed_sessions_remain_unknown_without_fabricated_terminal_receipts() {
        let (directory, mut reg, source) =
            crate::supervision_session::tests::fixture("recovery-unclosed");
        let db = directory.join("state.sqlite");
        let owner =
            crate::supervision_session::start(&mut reg, &source, std::process::id(), 1, 0, 1000)
                .unwrap();
        let before = crate::supervision_session::status(&reg, &owner.id).unwrap();
        backup(&db, "one").unwrap();
        restore(&db, "one", "copy").unwrap();
        let copied = Registry {
            connection: Connection::open_with_flags(
                path(&db, "copy").unwrap(),
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap(),
        };
        assert_eq!(
            crate::supervision_session::status(&copied, &owner.id).unwrap(),
            before
        );
        assert_eq!(
            before["observation"],
            "unclosed; PID liveness not established"
        );
        assert!(before["receipt_hash"].is_null());
        copied.integrity().unwrap();
        drop(copied);
        drop(reg);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn older_snapshot_schema_migrates_only_the_new_copy() {
        let (directory, db, reg) = fixture("older-schema");
        backup(&db, "older").unwrap();
        let snapshot = path(&db, "older").unwrap();
        {
            let connection = Connection::open(&snapshot).unwrap();
            connection.execute_batch("DROP TABLE corpus_fleet_runtime;DROP TABLE corpus_fleet_transitions;PRAGMA user_version=17;").unwrap();
        }
        let manifest_path = snapshot.parent().unwrap().join("manifest.json");
        let mut m: Manifest = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        m.body.schema_version = 17;
        m.body.database = artifact(&snapshot, "registry.sqlite", MAX_DATABASE_BYTES).unwrap();
        m.hash = data::hash(&m.body).unwrap();
        fs::write(&manifest_path, serde_json::to_vec(&m).unwrap()).unwrap();
        let before = data::file_hash(&snapshot).unwrap();
        verify(&db, "older").unwrap();
        let restored = restore(&db, "older", "migrated").unwrap();
        assert_eq!(
            restored["schema_version"],
            crate::storage::SCHEMA_VERSION.to_string()
        );
        assert_eq!(data::file_hash(&snapshot).unwrap(), before);
        verify(&db, "older").unwrap();
        drop(reg);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn reviewed_manifest_mismatch_cannot_create_a_restore_destination() {
        let (directory, db, reg) = fixture("reviewed");
        let receipt = backup(&db, "one").unwrap();
        assert!(restore_expected(&db, "one", "stale", Some(&"0".repeat(64))).is_err());
        assert!(!root(&db).unwrap().join("stale").exists());
        assert_eq!(
            describe(&db, "one").unwrap().unwrap()["fresh_content_verified"],
            false
        );
        let copied =
            restore_expected(&db, "one", "accepted", receipt["manifest_hash"].as_str()).unwrap();
        assert_eq!(copied["activation_performed"], false);
        assert_eq!(copied["fresh_content_verified"], true);
        drop(reg);
        fs::remove_dir_all(directory).unwrap();
    }
}
