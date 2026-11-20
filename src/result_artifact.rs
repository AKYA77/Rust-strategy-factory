//! Content-addressed receipt files. Permanent evidence is never garbage-collected.
use crate::Result;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub const INLINE_BYTES: usize = 256 * 1024;
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_FILES: usize = 4096;
const MAX_STORE_BYTES: u64 = 1024 * 1024 * 1024;
pub(crate) fn initialize(c: &Connection) -> Result<()> {
    let tx = crate::storage::writer_tx_open(c).map_err(|e| e.to_string())?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS result_artifacts(experiment_id TEXT PRIMARY KEY REFERENCES experiments(id),hash TEXT NOT NULL CHECK(length(hash)=64),bytes INTEGER NOT NULL CHECK(bytes>262144 AND bytes<=67108864));
    CREATE TRIGGER IF NOT EXISTS result_artifact_no_update BEFORE UPDATE ON result_artifacts BEGIN SELECT RAISE(ABORT,'result artifact immutable');END;
    CREATE TRIGGER IF NOT EXISTS result_artifact_no_delete BEFORE DELETE ON result_artifacts BEGIN SELECT RAISE(ABORT,'result artifact immutable');END;").map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}
struct Limited {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other(
                "receipt serialization budget exceeded",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(crate) fn serialize(value: &Value, limit: usize) -> Result<Vec<u8>> {
    let mut writer = Limited {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, value).map_err(|e| e.to_string())?;
    Ok(writer.bytes)
}
fn small(value: &Value) -> Value {
    if serialize(value, 16 * 1024).is_ok() {
        value.clone()
    } else {
        Value::Null
    }
}
pub(crate) fn marker(value: &Value, hash: &str, bytes: usize) -> Value {
    json!({"kind":"file-backed-result-v1","content_hash":hash,"bytes":bytes,"data_manifest":small(&value["data_manifest"]),"strategy":small(&value["strategy"]),"tick_replay":{"realized_micro_usd":small(&value["tick_replay"]["realized_micro_usd"])},"_receipt_summary":{"closed_trades":value["tick_replay"]["trades"].as_array().map(Vec::len)}})
}
fn root(c: &Connection) -> Result<PathBuf> {
    let path = c
        .path()
        .filter(|s| !s.is_empty())
        .ok_or("large receipts require a file-backed registry")?;
    let database = Path::new(path).canonicalize().map_err(|e| e.to_string())?;
    let mut name = database
        .file_name()
        .ok_or("registry filename missing")?
        .to_os_string();
    name.push(".artifacts");
    Ok(database.with_file_name(name))
}
fn directory(path: &Path, create: bool) -> Result<bool> {
    if create {
        match fs::create_dir(path) {
            Ok(()) => {
                File::open(path.parent().ok_or("artifact parent missing")?)
                    .and_then(|f| f.sync_all())
                    .map_err(|e| e.to_string())?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => Ok(true),
        Ok(_) => Err("artifact directory must be a regular directory, not a symlink".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}
fn file_bytes(path: &Path, expected: usize) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() != expected as u64
        || expected > MAX_BYTES
    {
        return Err("artifact file type/size mismatch".into());
    }
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(expected as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() != expected {
        return Err("artifact read size mismatch".into());
    }
    Ok(bytes)
}
fn scan(path: &Path) -> Result<(usize, u64, usize)> {
    if !directory(path, false)? {
        return Ok((0, 0, 0));
    }
    let (mut files, mut bytes, mut partial) = (0usize, 0u64, 0usize);
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name = name.to_str().ok_or("invalid artifact filename")?;
        let is_partial = name
            .strip_prefix(".partial-")
            .is_some_and(crate::machine_evidence::hex);
        if !is_partial
            && !name
                .strip_suffix(".json")
                .is_some_and(crate::machine_evidence::hex)
        {
            return Err("unrecognized artifact-store entry; preserve and inspect".into());
        }
        let m = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        if !m.is_file() || m.file_type().is_symlink() || m.len() > MAX_BYTES as u64 {
            return Err("invalid artifact-store entry type/size".into());
        }
        files += 1;
        bytes = bytes
            .checked_add(m.len())
            .ok_or("artifact byte count overflow")?;
        partial += usize::from(is_partial);
        if files > MAX_FILES || bytes > MAX_STORE_BYTES {
            return Err(
                "artifact store quota exceeded; evidence is never deleted automatically".into(),
            );
        }
    }
    Ok((files, bytes, partial))
}
/// Caller serializes writers with the registry's immediate transaction.
pub(crate) fn publish(c: &Connection, hash: &str, bytes: &[u8]) -> Result<()> {
    if !crate::machine_evidence::hex(hash)
        || bytes.len() > MAX_BYTES
        || blake3::hash(bytes).to_hex().as_str() != hash
    {
        return Err("invalid artifact publication".into());
    }
    let root = root(c)?;
    directory(&root, true)?;
    let destination = root.join(format!("{hash}.json"));
    if destination.try_exists().map_err(|e| e.to_string())? {
        if file_bytes(&destination, bytes.len())? != bytes {
            return Err("existing artifact content mismatch".into());
        }
        return Ok(());
    }
    let (files, total, _) = scan(&root)?;
    if files.saturating_add(2) > MAX_FILES || total + 2 * bytes.len() as u64 > MAX_STORE_BYTES {
        return Err("artifact store quota prevents publication; retain evidence".into());
    }
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let nonce = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let staging = root.join(format!(".partial-{nonce}"));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        match fs::hard_link(&staging, &destination) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if file_bytes(&destination, bytes.len())? != bytes {
                    return Err("concurrent artifact mismatch".into());
                }
            }
            Err(e) => return Err(e.to_string()),
        }
        File::open(&root)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    // An abrupt crash may retain a partial file. Inventory counts it; nothing sweeps evidence.
    let removed = fs::remove_file(&staging);
    if result.is_ok() {
        removed.map_err(|e| e.to_string())?;
        File::open(&root)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
    }
    result
}
fn reference(c: &Connection, id: &str) -> Result<Option<(String, usize)>> {
    c.query_row(
        "SELECT hash,bytes FROM result_artifacts WHERE experiment_id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(|e| e.to_string())
}
pub(crate) fn resolve(c: &Connection, id: &str, stored: Value, expected: &str) -> Result<Value> {
    let Some((hash, len)) = reference(c, id)? else {
        return Ok(stored);
    };
    if hash != expected
        || !crate::machine_evidence::hex(&hash)
        || !(INLINE_BYTES + 1..=MAX_BYTES).contains(&len)
    {
        return Err("artifact reference integrity mismatch".into());
    }
    let path = root(c)?;
    if !directory(&path, false)? {
        return Err("artifact directory missing".into());
    }
    let bytes = file_bytes(&path.join(format!("{hash}.json")), len)?;
    if blake3::hash(&bytes).to_hex().as_str() != hash {
        return Err("artifact content hash mismatch".into());
    }
    let result: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if marker(&result, &hash, len) != stored {
        return Err("artifact summary/reference mismatch".into());
    }
    Ok(result)
}
pub(crate) fn copy_referenced(source: &Connection, destination: &mut Connection) -> Result<()> {
    let tx = crate::storage::writer_tx(destination)?;
    {
        let mut query = tx
            .prepare("SELECT experiment_id,hash,bytes FROM result_artifacts ORDER BY bytes DESC,experiment_id")
            .map_err(|e| e.to_string())?;
        let rows = query
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, usize>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (id, hash, len) = row.map_err(|e| e.to_string())?;
            if !crate::machine_evidence::hex(&hash)
                || !(INLINE_BYTES + 1..=MAX_BYTES).contains(&len)
            {
                return Err("invalid backup artifact reference".into());
            }
            let source_root = root(source)?;
            if !directory(&source_root, false)? {
                return Err("backup source artifact directory missing".into());
            }
            let bytes = file_bytes(&source_root.join(format!("{hash}.json")), len)?;
            publish(&tx, &hash, &bytes).map_err(|e| format!("backup artifact {id}: {e}"))?;
        }
    }
    tx.commit().map_err(|e| e.to_string())
}
pub(crate) fn integrity(c: &Connection) -> Result<()> {
    let invalid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM result_artifacts a LEFT JOIN experiments e ON e.id=a.experiment_id WHERE e.id IS NULL OR e.state IN ('queued','running') OR e.result_hash IS NOT a.hash)",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if invalid {
        return Err("artifact index/experiment mismatch".into());
    }
    if c.path().is_some_and(|s| !s.is_empty()) {
        let _ = scan(&root(c)?)?;
    }
    Ok(())
}
pub fn inventory(registry: &crate::storage::Registry) -> Result<Value> {
    let c = &registry.connection;
    let (files, bytes, partial) = scan(&root(c)?)?;
    let (refs, referenced): (i64, i64) = c
        .query_row(
            "SELECT count(*),coalesce(sum(bytes),0) FROM result_artifacts",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    Ok(
        json!({"referenced_receipts":refs.to_string(),"referenced_bytes":referenced.to_string(),"physical_files":files.to_string(),"physical_bytes":bytes.to_string(),"partial_files":partial.to_string(),"maximum_files":MAX_FILES.to_string(),"maximum_store_bytes":MAX_STORE_BYTES.to_string(),"maximum_receipt_bytes":MAX_BYTES.to_string(),"automatic_deletion":false,"limitations":"Metadata inventory only, not full content verification. Byte counts sum file lengths, not allocated filesystem blocks; publication reserves both staging and final link entries. Unreferenced or partial evidence consumes physical quotas and is never removed automatically; registry-check verifies referenced receipt contents. Not an OS disk quota."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serialization_budget_counts_encoded_bytes_and_bounded_summary_fields() {
        assert_eq!(serialize(&json!("é"), 4).unwrap().len(), 4);
        assert!(serialize(&json!("é"), 3).is_err());
        let value = json!({"strategy":"s".repeat(20000),"tick_replay":{"realized_micro_usd":123,"trades":[{},{}]}});
        let summary = marker(&value, &"a".repeat(64), 300000);
        assert!(summary["strategy"].is_null());
        assert_eq!(summary["tick_replay"]["realized_micro_usd"], 123);
        assert_eq!(summary["_receipt_summary"]["closed_trades"], 2);
    }
}
