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
