//! Experimental bounded block SoA storage. Pinned roots verify ranges; full scans verify the manifest.
use crate::{
    Result,
    contracts::Profile,
    data::{self, Dataset, Manifest, Tick},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};
const MAGIC: &[u8; 8] = b"RSFBLK01";
const HEADER: usize = 28;
const MAX_META: usize = 1024 * 1024;
const MAX_EVENTS: usize = 2_000_000;
const MAX_BLOCKS: usize = 4096;
const ENTRY: usize = 64;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    profile: Profile,
    manifest: Manifest,
}
#[derive(Clone)]
struct Index {
    first_ms: i64,
    last_ms: i64,
    first_sequence: u64,
    last_sequence: u64,
    hash: [u8; 32],
}
impl Index {
    fn encode(&self) -> [u8; ENTRY] {
        let mut b = [0; ENTRY];
        b[..8].copy_from_slice(&self.first_ms.to_le_bytes());
        b[8..16].copy_from_slice(&self.last_ms.to_le_bytes());
        b[16..24].copy_from_slice(&self.first_sequence.to_le_bytes());
        b[24..32].copy_from_slice(&self.last_sequence.to_le_bytes());
        b[32..].copy_from_slice(&self.hash);
        b
    }
    fn decode(b: &[u8; ENTRY]) -> Self {
        Self {
            first_ms: i64::from_le_bytes(b[..8].try_into().unwrap()),
            last_ms: i64::from_le_bytes(b[8..16].try_into().unwrap()),
            first_sequence: u64::from_le_bytes(b[16..24].try_into().unwrap()),
            last_sequence: u64::from_le_bytes(b[24..32].try_into().unwrap()),
            hash: b[32..].try_into().unwrap(),
        }
    }
}
fn encode(ticks: &[Tick]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ticks.len() * 32);
    for col in 0..4 {
        for t in ticks {
            out.extend_from_slice(&match col {
                0 => t.timestamp_ms.to_le_bytes(),
                1 => t.sequence.to_le_bytes(),
                2 => t.bid.to_le_bytes(),
                _ => t.ask.to_le_bytes(),
            });
        }
    }
    out
}
fn err(e: std::io::Error) -> String {
    e.to_string()
}
fn bounds(rows: usize, block_rows: usize, blocks: usize) -> Result<()> {
    if rows == 0
        || rows > MAX_EVENTS
        || !(1..=8192).contains(&block_rows)
        || blocks == 0
        || blocks > MAX_BLOCKS
        || blocks != rows.div_ceil(block_rows)
    {
        return Err("blocked native row/block budget mismatch".into());
    }
    Ok(())
}
/// Atomic no-replace publication; the original canonical manifest and category are retained.
pub fn write(path: &Path, dataset: &Dataset, block_rows: usize) -> Result<()> {
    dataset.verify()?;
    let blocks = dataset.ticks.len().div_ceil(block_rows.max(1));
    bounds(dataset.ticks.len(), block_rows, blocks)?;
    let metadata = serde_json::to_vec(&Metadata {
        profile: dataset.profile.clone(),
        manifest: dataset.manifest.clone(),
    })
    .map_err(|e| e.to_string())?;
    if metadata.is_empty() || metadata.len() > MAX_META {
        return Err("blocked native metadata cap exceeded".into());
    }
    let mut header = [0; HEADER];
    header[..8].copy_from_slice(MAGIC);
    header[8..16].copy_from_slice(&(dataset.ticks.len() as u64).to_le_bytes());
    header[16..20].copy_from_slice(&(metadata.len() as u32).to_le_bytes());
    header[20..24].copy_from_slice(&(block_rows as u32).to_le_bytes());
    header[24..].copy_from_slice(&(blocks as u32).to_le_bytes());
    let mut prefix = Vec::with_capacity(HEADER + metadata.len() + blocks * ENTRY);
    prefix.extend_from_slice(&header);
    prefix.extend_from_slice(&metadata);
    for ticks in dataset.ticks.chunks(block_rows) {
        let first = ticks.first().ok_or("empty block")?;
        let last = ticks.last().ok_or("empty block")?;
        prefix.extend_from_slice(
            &Index {
                first_ms: first.timestamp_ms,
                last_ms: last.timestamp_ms,
                first_sequence: first.sequence,
                last_sequence: last.sequence,
                hash: *blake3::hash(&encode(ticks)).as_bytes(),
            }
            .encode(),
        );
    }
    let root = blake3::hash(&prefix);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(
        ".factory-blocks-{}.tmp",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(err)?;
    let result = (|| {
        file.write_all(&prefix).map_err(err)?;
        file.write_all(root.as_bytes()).map_err(err)?;
        for ticks in dataset.ticks.chunks(block_rows) {
            file.write_all(&encode(ticks)).map_err(err)?;
        }
        file.sync_all().map_err(err)?;
        fs::hard_link(&temporary, path).map_err(err)?;
        File::open(parent).and_then(|f| f.sync_all()).map_err(err)
    })();
    drop(file);
    let cleanup = fs::remove_file(&temporary).map_err(err);
    result.and(cleanup)
}
pub struct Reader {
    file: File,
    profile: Profile,
    manifest: Manifest,
    root: String,
    block_rows: usize,
    index: Vec<Index>,
    offset: u64,
    length: u64,
    prefix: Vec<u8>,
    pinned: bool,
}
#[derive(Serialize)]
pub struct Report {
    pub version: &'static str,
    pub dataset_identity: String,
    pub file_hash: String,
    pub root: String,
    pub events: String,
    pub blocks: String,
    pub maximum_resident_events: String,
    pub declared_class: crate::data::DataClass,
    pub limitations: &'static str,
}
impl Reader {
    pub fn profile(&self) -> &Profile {
        &self.profile
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn root(&self) -> &str {
        &self.root
    }
    pub fn block_rows(&self) -> usize {
        self.block_rows
    }
    pub fn block_count(&self) -> usize {
        self.index.len()
    }
    /// A supplied root must come from the caller's frozen/verified artifact context, not this file itself.
    pub fn open(path: &Path, maximum_events: usize, expected_root: Option<&str>) -> Result<Self> {
        if maximum_events == 0 || maximum_events > MAX_EVENTS {
            return Err("invalid blocked reader event cap".into());
        }
        let mut file = File::open(path).map_err(err)?;
        if !file.metadata().map_err(err)?.is_file() {
            return Err("blocked input must be regular".into());
        }
        let length = file.metadata().map_err(err)?.len();
        let mut header = [0; HEADER];
        file.read_exact(&mut header).map_err(err)?;
        if &header[..8] != MAGIC {
            return Err("unsupported blocked native magic/version".into());
        }
        let rows = u64::from_le_bytes(header[8..16].try_into().unwrap());
        let meta = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
        let block_rows = u32::from_le_bytes(header[20..24].try_into().unwrap()) as usize;
        let blocks = u32::from_le_bytes(header[24..].try_into().unwrap()) as usize;
        if rows > maximum_events as u64 || meta == 0 || meta > MAX_META {
            return Err("blocked native metadata/event admission cap".into());
        }
        bounds(rows as usize, block_rows, blocks)?;
        let offset = (HEADER + meta + blocks * ENTRY + 32) as u64;
        let expected = offset
            .checked_add(rows.checked_mul(32).ok_or("blocked byte count overflow")?)
            .ok_or("blocked length overflow")?;
        if length != expected {
            return Err("blocked native exact length mismatch".into());
        }
        let mut prefix = Vec::with_capacity(HEADER + meta + blocks * ENTRY + 32);
        prefix.extend_from_slice(&header);
        prefix.resize(HEADER + meta + blocks * ENTRY, 0);
        file.read_exact(&mut prefix[HEADER..]).map_err(err)?;
        let mut root = [0; 32];
        file.read_exact(&mut root).map_err(err)?;
        if blake3::hash(&prefix).as_bytes() != &root {
            return Err("blocked index checksum mismatch".into());
        }
        let root_text = blake3::Hash::from(root).to_hex().to_string();
        if expected_root.is_some_and(|s| s != root_text) {
            return Err("blocked index root differs from pinned artifact".into());
        }
        let metadata: Metadata =
            serde_json::from_slice(&prefix[HEADER..HEADER + meta]).map_err(|e| e.to_string())?;
        metadata.profile.validate()?;
        if metadata.manifest.events as u64 != rows
            || metadata.manifest.class == crate::data::DataClass::Quarantined
            || metadata.manifest.profile_hash != data::hash(&metadata.profile)?
            || metadata.manifest.symbol != metadata.profile.symbol
        {
            return Err("blocked manifest/profile mismatch".into());
        }
        let mut index: Vec<Index> = Vec::with_capacity(blocks);
        for bytes in prefix[HEADER + meta..].as_chunks::<ENTRY>().0 {
            let entry = Index::decode(bytes);
            if entry.first_ms > entry.last_ms
                || entry.first_sequence > entry.last_sequence
                || index.last().is_some_and(|old| {
                    old.last_ms > entry.first_ms || old.last_sequence >= entry.first_sequence
                })
            {
                return Err("blocked index event ordering invalid".into());
            }
            index.push(entry);
        }
        if index[0].first_ms != metadata.manifest.first_ms
            || index.last().ok_or("empty index")?.last_ms != metadata.manifest.last_ms
        {
            return Err("blocked index manifest endpoints differ".into());
        }
        prefix.extend_from_slice(&root);
        Ok(Self {
            file,
            profile: metadata.profile,
            manifest: metadata.manifest,
            root: root_text,
            block_rows,
            index,
            offset,
            length,
            prefix,
            pinned: expected_root.is_some(),
        })
    }
    fn block(&mut self, index: usize) -> Result<(Vec<Tick>, Vec<u8>)> {
        let entry = self
            .index
            .get(index)
            .ok_or("block index outside artifact")?;
        let start = index * self.block_rows;
        let count = self.block_rows.min(self.manifest.events - start);
        let mut bytes = vec![0; count * 32];
        self.file
            .seek(SeekFrom::Start(self.offset + start as u64 * 32))
            .map_err(err)?;
        self.file.read_exact(&mut bytes).map_err(err)?;
        if blake3::hash(&bytes).as_bytes() != &entry.hash {
            return Err("blocked quote payload checksum mismatch".into());
        }
        let mut ticks = Vec::with_capacity(count);
        for row in 0..count {
            let word = |col: usize| -> [u8; 8] {
                bytes[(col * count + row) * 8..(col * count + row + 1) * 8]
                    .try_into()
                    .unwrap()
            };
            let tick = Tick {
                timestamp_ms: i64::from_le_bytes(word(0)),
                sequence: u64::from_le_bytes(word(1)),
                bid: i64::from_le_bytes(word(2)),
                ask: i64::from_le_bytes(word(3)),
            };
            tick.validate(&self.profile)?;
            if chrono::DateTime::from_timestamp_millis(tick.timestamp_ms).is_none()
                || ticks.last().is_some_and(|old: &Tick| {
                    tick.timestamp_ms < old.timestamp_ms || tick.sequence <= old.sequence
                })
            {
                return Err("blocked quote calendar/order invalid".into());
            }
            ticks.push(tick);
        }
        let first = ticks.first().ok_or("empty quote block")?;
        let last = ticks.last().ok_or("empty quote block")?;
        if (
            first.timestamp_ms,
            first.sequence,
            last.timestamp_ms,
            last.sequence,
        ) != (
            entry.first_ms,
            entry.first_sequence,
            entry.last_ms,
            entry.last_sequence,
        ) {
            return Err("blocked payload differs from index boundaries".into());
        }
        Ok((ticks, bytes))
    }
    /// Callbacks see checked blocks; a complete successful return is required before publishing anything.
    pub fn visit(
        &mut self,
        mut visit: impl FnMut(usize, &Profile, &Manifest, &[Tick]) -> Result<()>,
    ) -> Result<Report> {
        let mut canonical = crate::corpus::SequenceHash::new();
        let mut hash = blake3::Hasher::new();
        hash.update(&self.prefix);
        let mut peak = 0;
        for index in 0..self.index.len() {
            let (ticks, bytes) = self.block(index)?;
            peak = peak.max(ticks.len());
            hash.update(&bytes);
            for tick in &ticks {
                canonical.push(tick)?;
            }
            visit(index, &self.profile, &self.manifest, &ticks)?;
        }
        if canonical.finish() != self.manifest.content_hash {
            return Err("blocked canonical content hash mismatch".into());
        }
        if self.file.metadata().map_err(err)?.len() != self.length {
            return Err("blocked artifact changed length during read".into());
        }
        Ok(Report {
            version: "blocked-native-v1",
            dataset_identity: data::dataset_identity(&self.manifest)?,
            file_hash: hash.finalize().to_hex().to_string(),
            root: self.root.clone(),
            events: self.manifest.events.to_string(),
            blocks: self.index.len().to_string(),
            maximum_resident_events: peak.to_string(),
            declared_class: self.manifest.class,
            limitations: "Experimental block layout; verified structural/provenance identity does not grant source approval or strategy qualification.",
        })
    }
    /// Exact row window from an independently pinned index. Every touched complete block is checked.
    pub fn range(&mut self, start: usize, count: usize) -> Result<Vec<Tick>> {
        if !self.pinned
            || count == 0
            || count > 65536
            || start
                .checked_add(count)
                .is_none_or(|end| end > self.manifest.events)
        {
            return Err("range requires pinned root and bounded in-artifact rows".into());
        }
        let end = start + count;
        let mut rows = Vec::with_capacity(count);
        for index in start / self.block_rows..=(end - 1) / self.block_rows {
            let (ticks, _) = self.block(index)?;
            let base = index * self.block_rows;
            rows.extend_from_slice(
                &ticks[start.saturating_sub(base)..(end - base).min(ticks.len())],
            );
        }
        if self.file.metadata().map_err(err)?.len() != self.length {
            return Err("blocked artifact changed length during range".into());
        }
        Ok(rows)
    }
}
pub fn inspect(path: &Path, maximum_events: usize) -> Result<Report> {
    Reader::open(path, maximum_events, None)?.visit(|_, _, _, _| Ok(()))
}
pub fn read(path: &Path, maximum_events: usize) -> Result<Dataset> {
    Ok(read_bounded(path, maximum_events)?.0)
}
pub fn read_bounded(path: &Path, maximum_events: usize) -> Result<(Dataset, Report)> {
    let mut reader = Reader::open(path, maximum_events, None)?;
    let mut ticks = Vec::new();
    let report = reader.visit(|_, _, _, rows| {
        ticks.extend_from_slice(rows);
        Ok(())
    })?;
    Ok((
        Dataset {
            profile: reader.profile,
            manifest: reader.manifest,
            ticks,
        },
        report,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("blocked-{name}-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let path = root.join("sample.rsfb");
        write(&path, &data::synthetic_fixture("EURUSD").unwrap(), 7).unwrap();
        (root, path)
    }
    #[test]
    fn malformed_headers_lengths_caps_and_atomic_no_replace_are_refused() {
        let (root, path) = fixture("headers");
        let original = fs::read(&path).unwrap();
        assert!(Reader::open(&path, 0, None).is_err());
        assert!(Reader::open(&path, 1999, None).is_err());
        for (index, value) in [(0, 0u8), (8, 255), (16, 255), (20, 0), (24, 255)] {
            let mut bytes = original.clone();
            if index == 20 {
                bytes[20..24].fill(0);
            } else {
                bytes[index] = value;
            }
            fs::write(&path, bytes).unwrap();
            assert!(inspect(&path, MAX_EVENTS).is_err());
        }
        for (start, width) in [(8, 8), (16, 4), (24, 4)] {
            let mut bytes = original.clone();
            bytes[start..start + width].fill(255);
            fs::write(&path, bytes).unwrap();
            assert!(Reader::open(&path, MAX_EVENTS, None).is_err());
        }
        for size in [0, 27, original.len() - 1, original.len() + 1] {
            let mut bytes = original.clone();
            bytes.resize(size, 0);
            fs::write(&path, bytes).unwrap();
            assert!(inspect(&path, MAX_EVENTS).is_err());
        }
        fs::write(&path, &original).unwrap();
        let d = data::synthetic_fixture("EURUSD").unwrap();
        assert!(write(&path, &d, 7).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(write(&root.join("invalid.rsfb"), &d, 0).is_err());
        assert!(write(&root.join("invalid.rsfb"), &d, 8193).is_err());
        assert!(!root.join("invalid.rsfb").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unpinned_outside_and_mutated_block_ranges_refuse_without_partial_rows() {
        let (root, path) = fixture("range");
        let mut reader = Reader::open(&path, MAX_EVENTS, None).unwrap();
        assert!(reader.range(0, 1).is_err());
        let root_hash = reader.root.clone();
        assert!(Reader::open(&path, MAX_EVENTS, Some(&"0".repeat(64))).is_err());
        let mut reader = Reader::open(&path, MAX_EVENTS, Some(&root_hash)).unwrap();
        for (start, count) in [(0, 0), (usize::MAX, 1), (1999, 2), (0, 65537)] {
            assert!(reader.range(start, count).is_err());
        }
        let mut bytes = fs::read(&path).unwrap();
        bytes[reader.offset as usize] ^= 1;
        fs::write(&path, bytes).unwrap();
        assert!(reader.range(0, 1).is_err());
        assert!(inspect(&path, MAX_EVENTS).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn resigned_payload_cannot_bypass_quote_or_complete_manifest_verification() {
        let (root, path) = fixture("resigned");
        let reader = Reader::open(&path, MAX_EVENTS, None).unwrap();
        let offset = reader.offset as usize;
        let index_start =
            HEADER + u32::from_le_bytes(reader.prefix[16..20].try_into().unwrap()) as usize;
        let prefix_end = offset - 32;
        let original = fs::read(&path).unwrap();
        let old_root = reader.root.clone();
        drop(reader);
        for zero_bid in [true, false] {
            let mut bytes = original.clone();
            let pos = offset + 7 * 16;
            let old = i64::from_le_bytes(bytes[pos..pos + 8].try_into().unwrap());
            bytes[pos..pos + 8].copy_from_slice(&if zero_bid { 0 } else { old + 1 }.to_le_bytes());
            let block = blake3::hash(&bytes[offset..offset + 7 * 32]);
            bytes[index_start + 32..index_start + 64].copy_from_slice(block.as_bytes());
            let root_hash = blake3::hash(&bytes[..prefix_end]);
            bytes[prefix_end..offset].copy_from_slice(root_hash.as_bytes());
            fs::write(&path, &bytes).unwrap();
            assert!(Reader::open(&path, MAX_EVENTS, Some(&old_root)).is_err());
            assert!(inspect(&path, MAX_EVENTS).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn visitor_requires_complete_success_and_rechecks_every_touched_block() {
        let (root, path) = fixture("visitor");
        let mut reader = Reader::open(&path, MAX_EVENTS, None).unwrap();
        let mut seen = 0;
        assert!(
            reader
                .visit(|_, _, _, ticks| {
                    seen += ticks.len();
                    Err("injected consumer refusal".into())
                })
                .is_err()
        );
        assert_eq!(seen, 7);
        let report = reader.visit(|_, _, _, _| Ok(())).unwrap();
        assert_eq!(report.events, "2000");
        assert_eq!(report.maximum_resident_events, "7");
        assert_eq!(report.file_hash, data::file_hash(&path).unwrap());
        let mut pinned = Reader::open(&path, MAX_EVENTS, Some(&report.root)).unwrap();
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&[0])
            .unwrap();
        assert!(pinned.range(0, 1).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
