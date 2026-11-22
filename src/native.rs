//! Lossless bounded uncompressed SoA interchange baseline, not a selected production codec.
use crate::{
    Result,
    contracts::Profile,
    data::{Dataset, Manifest, Tick},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
};
const MAGIC: &[u8; 8] = b"RSFNAT01";
const MAX_EVENTS: usize = 2_000_000;
const MAX_META: usize = 1024 * 1024;
pub(crate) const MAX_BYTES: u64 = 52 + MAX_META as u64 + MAX_EVENTS as u64 * 32;
const CHUNK: usize = 8192;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    profile: Profile,
    manifest: Manifest,
}
fn io(error: std::io::Error) -> String {
    error.to_string()
}
fn header(count: usize, metadata: usize) -> [u8; 20] {
    let mut h = [0; 20];
    h[..8].copy_from_slice(MAGIC);
    h[8..16].copy_from_slice(&(count as u64).to_le_bytes());
    h[16..20].copy_from_slice(&(metadata as u32).to_le_bytes());
    h
}
/// Atomic no-replace publication. The source and all provenance/category fields remain unchanged.
pub fn write(path: &Path, dataset: &Dataset) -> Result<()> {
    dataset.verify()?;
    if dataset.ticks.len() > MAX_EVENTS {
        return Err("native event budget exceeded".into());
    }
    let metadata = serde_json::to_vec(&Metadata {
        profile: dataset.profile.clone(),
        manifest: dataset.manifest.clone(),
    })
    .map_err(|e| e.to_string())?;
    if metadata.len() > MAX_META {
        return Err("native metadata budget exceeded".into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let suffix = nonce.iter().map(|n| format!("{n:02x}")).collect::<String>();
    let temp = parent.join(format!(".factory-native-{suffix}.tmp"));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(io)?;
    let result = (|| {
        let mut writer = BufWriter::new(file);
        let mut hash = blake3::Hasher::new();
        let h = header(dataset.ticks.len(), metadata.len());
        writer.write_all(&h).map_err(io)?;
        hash.update(&h);
        writer.write_all(&metadata).map_err(io)?;
        hash.update(&metadata);
        let mut buffer = Vec::with_capacity(CHUNK * 8);
        for column in 0..4 {
            for ticks in dataset.ticks.chunks(CHUNK) {
                buffer.clear();
                for tick in ticks {
                    let bytes = match column {
                        0 => tick.timestamp_ms.to_le_bytes(),
                        1 => tick.sequence.to_le_bytes(),
                        2 => tick.bid.to_le_bytes(),
                        _ => tick.ask.to_le_bytes(),
                    };
                    buffer.extend_from_slice(&bytes);
                }
                writer.write_all(&buffer).map_err(io)?;
                hash.update(&buffer);
            }
        }
        writer.write_all(hash.finalize().as_bytes()).map_err(io)?;
        writer.flush().map_err(io)?;
        writer.get_ref().sync_all().map_err(io)?;
        fs::hard_link(&temp, path).map_err(io)?;
        File::open(parent).and_then(|f| f.sync_all()).map_err(io)
    })();
    let cleanup = fs::remove_file(&temp).map_err(io);
    result.and(cleanup)
}
/// Validate lengths before allocation, checksum the exact bytes read, and verify original identities.
pub fn read(path: &Path) -> Result<Dataset> {
    let file = File::open(path).map_err(io)?;
    let length = file.metadata().map_err(io)?.len();
    decode(BufReader::new(file), length)
}
/// Enforce the caller's event cap before allocating rows, and fingerprint the
/// exact consumed bytes on the same descriptor used for checksum validation.
pub fn read_bounded(path: &Path, max_events: usize) -> Result<(Dataset, String)> {
    let file = File::open(path).map_err(io)?;
    let length = file.metadata().map_err(io)?.len();
    struct Fingerprint<R> {
        reader: R,
        hash: blake3::Hasher,
    }
    impl<R: Read> Read for Fingerprint<R> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let n = self.reader.read(buffer)?;
            self.hash.update(&buffer[..n]);
            Ok(n)
        }
    }
    let mut reader = Fingerprint {
        reader: BufReader::new(file),
        hash: blake3::Hasher::new(),
    };
    let dataset = decode_bounded(&mut reader, length, max_events)?;
    Ok((dataset, reader.hash.finalize().to_hex().to_string()))
}
/// Decode the exact verified byte sequence, shared by bounded compressed storage.
pub(crate) fn decode(reader: impl Read, length: u64) -> Result<Dataset> {
    decode_bounded(reader, length, MAX_EVENTS)
}
fn decode_bounded(mut reader: impl Read, length: u64, max_events: usize) -> Result<Dataset> {
    if max_events == 0 || max_events > MAX_EVENTS {
        return Err("invalid native caller event budget".into());
    }
    let mut h = [0u8; 20];
    reader.read_exact(&mut h).map_err(io)?;
    if &h[..8] != MAGIC {
        return Err("unsupported native format magic/version".into());
    }
    let count = u64::from_le_bytes(h[8..16].try_into().unwrap());
    let metadata = u32::from_le_bytes(h[16..20].try_into().unwrap()) as usize;
    if count == 0 || count > max_events as u64 || metadata == 0 || metadata > MAX_META {
        return Err("native header exceeds event/metadata budget".into());
    }
    let expected = 52 + metadata as u64 + count * 32;
    if length != expected {
        return Err("native exact file length mismatch".into());
    }
    let mut hash = blake3::Hasher::new();
    hash.update(&h);
    let mut bytes = vec![0u8; metadata];
    reader.read_exact(&mut bytes).map_err(io)?;
    hash.update(&bytes);
    let metadata: Metadata = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if metadata.manifest.events as u64 != count {
        return Err("native manifest count mismatch".into());
    }
    let mut ticks = Vec::new();
    ticks
        .try_reserve_exact(count as usize)
        .map_err(|e| e.to_string())?;
    ticks.resize(
        count as usize,
        Tick {
            timestamp_ms: 0,
            sequence: 0,
            bid: 0,
            ask: 0,
        },
    );
    let mut buffer = vec![0u8; CHUNK * 8];
    for column in 0..4 {
        for chunk in ticks.chunks_mut(CHUNK) {
            let data = &mut buffer[..chunk.len() * 8];
            reader.read_exact(data).map_err(io)?;
            hash.update(data);
            for (tick, part) in chunk.iter_mut().zip(data.as_chunks::<8>().0) {
                let bits: [u8; 8] = *part;
                match column {
                    0 => tick.timestamp_ms = i64::from_le_bytes(bits),
                    1 => tick.sequence = u64::from_le_bytes(bits),
                    2 => tick.bid = i64::from_le_bytes(bits),
                    _ => tick.ask = i64::from_le_bytes(bits),
                }
            }
        }
    }
    let mut checksum = [0u8; 32];
    reader.read_exact(&mut checksum).map_err(io)?;
    if hash.finalize().as_bytes() != &checksum {
        return Err("native payload checksum mismatch".into());
    }
    if reader.read(&mut [0u8; 1]).map_err(io)? != 0 {
        return Err("native grew during read".into());
    }
    let dataset = Dataset {
        profile: metadata.profile,
        manifest: metadata.manifest,
        ticks,
    };
    dataset.verify()?;
    Ok(dataset)
}

/// Fixed block size for the native indexed reader: 8,192 rows per block.
pub const INDEX_BLOCK_ROWS: usize = 8192;

/// Direct columnar view of one verified block: hot arrays with bounds checks,
/// no per-tick construction and exact consumed-byte provenance bindings.
pub struct BlockColumns<'a> {
    pub block_index: usize,
    pub row_offset: usize,
    pub row_count: usize,
    pub timestamps: &'a [i64],
    pub sequences: &'a [u64],
    pub bids: &'a [i64],
    pub asks: &'a [i64],
}

/// Complete resident hot arrays over one verified native chunk, in file order.
pub struct HotArrays<'a> {
    pub timestamps: &'a [i64],
    pub sequences: &'a [u64],
    pub bids: &'a [i64],
    pub asks: &'a [i64],
}

/// Bounds-checked indexed access over one verified native `.rsf` chunk.
///
/// The whole file is checksum-verified and length-bound at open; every block
/// view is bounds-checked against the resident column arrays and cross-block
/// event order is validated at open. The file hash and dataset identity bind
/// each view to the exact consumed bytes.
pub struct NativeIndexed {
    file_hash: String,
    profile: Profile,
    manifest: Manifest,
    rows: usize,
    block_rows: usize,
    block_count: usize,
    timestamps: Vec<i64>,
    sequences: Vec<u64>,
    bids: Vec<i64>,
    asks: Vec<i64>,
}

impl NativeIndexed {
    /// Open and fully verify `path`, decoding its four column sections into
    /// resident hot arrays. Refuses budgets, magic, length, checksum,
    /// manifest and order violations before any array is published.
    pub fn open(path: &Path, max_events: usize) -> Result<Self> {
        if max_events == 0 || max_events > MAX_EVENTS {
            return Err("invalid native indexed event budget".into());
        }
        let file = File::open(path).map_err(io)?;
        let length = file.metadata().map_err(io)?.len();
        if length == 0 || length > MAX_BYTES {
            return Err("native indexed artifact byte budget exceeded".into());
        }
        let mut bytes = Vec::new();
        file.take(length.checked_add(1).ok_or("artifact length overflow")?)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() as u64 != length {
            return Err("native indexed artifact grew during read".into());
        }
        if bytes.len() < 20 || &bytes[..8] != MAGIC {
            return Err("unsupported native format magic/version".into());
        }
        let count = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
        let meta = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
        if count == 0 || count > max_events || meta == 0 || meta > MAX_META {
            return Err("native indexed header exceeds event/metadata budget".into());
        }
        let expected = 52 + meta + count.checked_mul(32).ok_or("native event count overflow")?;
        if length as usize != expected {
            return Err("native indexed exact file length mismatch".into());
        }
        let end = bytes.len() - 32;
        let mut hash = blake3::Hasher::new();
        hash.update(&bytes[..end]);
        let mut checksum = [0u8; 32];
        checksum.copy_from_slice(&bytes[end..]);
        if hash.finalize().as_bytes() != &checksum {
            return Err("native indexed payload checksum mismatch".into());
        }
        let metadata: Metadata =
            serde_json::from_slice(&bytes[20..20 + meta]).map_err(|e| e.to_string())?;
        if metadata.manifest.events as usize != count {
            return Err("native indexed manifest count mismatch".into());
        }
        let profile = metadata.profile.clone();
        profile.validate()?;
        if metadata.manifest.class == crate::data::DataClass::Quarantined
            || metadata.manifest.profile_hash != crate::data::hash(&profile)?
            || metadata.manifest.symbol != profile.symbol
        {
            return Err("native indexed manifest/profile mismatch".into());
        }
        let payload = 20 + meta;
        let mut timestamps = Vec::with_capacity(count);
        let mut sequences = Vec::with_capacity(count);
        let mut bids = Vec::with_capacity(count);
        let mut asks = Vec::with_capacity(count);
        for row in 0..count {
            let ts = payload + row * 8;
            let sq = payload + count * 8 + row * 8;
            let bd = payload + 2 * count * 8 + row * 8;
            let ak = payload + 3 * count * 8 + row * 8;
            timestamps.push(i64::from_le_bytes(bytes[ts..ts + 8].try_into().unwrap()));
            sequences.push(u64::from_le_bytes(bytes[sq..sq + 8].try_into().unwrap()));
            bids.push(i64::from_le_bytes(bytes[bd..bd + 8].try_into().unwrap()));
            asks.push(i64::from_le_bytes(bytes[ak..ak + 8].try_into().unwrap()));
        }
        let ticks: Vec<Tick> = (0..count)
            .map(|i| Tick {
                timestamp_ms: timestamps[i],
                sequence: sequences[i],
                bid: bids[i],
                ask: asks[i],
            })
            .collect();
        let dataset = Dataset {
            profile: profile.clone(),
            manifest: metadata.manifest.clone(),
            ticks,
        };
        dataset.verify()?;
        let file_hash = blake3::hash(&bytes).to_hex().to_string();
        let block_rows = INDEX_BLOCK_ROWS;
        let block_count = count.div_ceil(block_rows);
        Ok(Self {
            file_hash,
            profile,
            manifest: metadata.manifest,
            rows: count,
            block_rows,
            block_count,
            timestamps,
            sequences,
            bids,
            asks,
        })
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn block_rows(&self) -> usize {
        self.block_rows
    }

    pub fn block_count(&self) -> usize {
        self.block_count
    }

    pub fn file_hash(&self) -> &str {
        &self.file_hash
    }

    pub fn dataset_identity(&self) -> Result<String> {
        crate::data::dataset_identity(&self.manifest)
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Bounds-checked direct columnar view of block `index`; cross-block event
    /// order was validated at open.
    pub fn block(&self, index: usize) -> Result<BlockColumns<'_>> {
        if index >= self.block_count {
            return Err("block index outside artifact".into());
        }
        let row_offset = index * self.block_rows;
        let row_count = self.block_rows.min(self.rows - row_offset);
        Ok(BlockColumns {
            block_index: index,
            row_offset,
            row_count,
            timestamps: &self.timestamps[row_offset..row_offset + row_count],
            sequences: &self.sequences[row_offset..row_offset + row_count],
            bids: &self.bids[row_offset..row_offset + row_count],
            asks: &self.asks[row_offset..row_offset + row_count],
        })
    }

    /// Complete resident hot arrays over the verified chunk, in file order.
    pub fn hot_arrays(&self) -> HotArrays<'_> {
        HotArrays {
            timestamps: &self.timestamps,
            sequences: &self.sequences,
            bids: &self.bids,
            asks: &self.asks,
        }
    }

    /// Exact row window across block boundaries, rebuilt from the column arrays.
    pub fn range(&self, start: usize, count: usize) -> Result<Vec<Tick>> {
        let end = start
            .checked_add(count)
            .ok_or("native indexed row range overflow")?;
        if end > self.rows {
            return Err("native indexed row range outside artifact".into());
        }
        Ok((start..end)
            .map(|i| Tick {
                timestamp_ms: self.timestamps[i],
                sequence: self.sequences[i],
                bid: self.bids[i],
                ask: self.asks[i],
            })
            .collect())
    }
}

#[cfg(test)]
mod indexed_native_tests {
    use super::*;
    use crate::{data, native};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    fn dir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "factory-native-indexed-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        p
    }
    #[test]
    fn indexed_reader_binds_hash_identity_and_bounds_every_block() {
        let root = dir();
        let data = crate::data::synthetic_fixture("EURUSD").unwrap();
        let path = root.join("sample.rsf");
        super::write(&path, &data).unwrap();
        let reader = NativeIndexed::open(&path, MAX_EVENTS).unwrap();
        assert_eq!(reader.rows(), data.ticks.len());
        assert_eq!(
            reader.block_count(),
            data.ticks.len().div_ceil(INDEX_BLOCK_ROWS)
        );
        assert_eq!(reader.file_hash(), crate::data::file_hash(&path).unwrap());
        assert_eq!(
            reader.dataset_identity().unwrap(),
            crate::data::dataset_identity(&data.manifest).unwrap()
        );
        let arrays = reader.hot_arrays();
        assert_eq!(arrays.timestamps.len(), data.ticks.len());
        for (i, t) in data.ticks.iter().enumerate() {
            assert_eq!(arrays.timestamps[i], t.timestamp_ms);
            assert_eq!(arrays.sequences[i], t.sequence);
            assert_eq!(arrays.bids[i], t.bid);
            assert_eq!(arrays.asks[i], t.ask);
        }
        let mut blocks = 0;
        for index in 0..reader.block_count() {
            let block = reader.block(index).unwrap();
            assert_eq!(block.block_index, index);
            let start = index * INDEX_BLOCK_ROWS;
            let count = INDEX_BLOCK_ROWS.min(data.ticks.len() - start);
            assert_eq!(block.row_count, count);
            for (row, t) in data.ticks[start..start + count].iter().enumerate() {
                assert_eq!(block.timestamps[row], t.timestamp_ms);
                assert_eq!(block.sequences[row], t.sequence);
                assert_eq!(block.bids[row], t.bid);
                assert_eq!(block.asks[row], t.ask);
            }
            blocks += 1;
        }
        assert_eq!(blocks, reader.block_count());
        assert!(reader.block(reader.block_count()).is_err());
        // range crosses block boundaries exactly
        let window = reader.range(100, 50).unwrap();
        assert_eq!(window.len(), 50);
        assert_eq!(window, data.ticks[100..150]);
        assert!(reader.range(1990, 20).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn indexed_reader_refuses_budget_magic_length_checksum_and_order_violations() {
        let root = dir();
        let good = root.join("good.rsf");
        native::write(&good, &data::synthetic_fixture("EURUSD").unwrap()).unwrap();
        let bytes = fs::read(&good).unwrap();
        let bad = root.join("bad.rsf");
        for offset in [0, 8, 16, 30, bytes.len() - 33, bytes.len() - 1] {
            let mut changed = bytes.clone();
            changed[offset] ^= 0x40;
            fs::write(&bad, &changed).unwrap();
            assert!(
                NativeIndexed::open(&bad, MAX_EVENTS).is_err(),
                "offset {offset}"
            );
        }
        let mut huge = bytes.clone();
        huge[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
        fs::write(&bad, &huge).unwrap();
        assert!(NativeIndexed::open(&bad, 100).is_err());
        assert!(NativeIndexed::open(&bad, 0).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
