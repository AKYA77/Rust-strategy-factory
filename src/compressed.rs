//! Bounded lossless ZSTD/native baseline. No full-history streaming/codec-selection claim.
use crate::{Result, data::Dataset, native};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
const MAGIC: &[u8; 8] = b"RSFZST01";
const MAX_COMPRESSED: u64 = 128 * 1024 * 1024;
const LEVEL: i32 = 3;
fn io(e: std::io::Error) -> String {
    e.to_string()
}
fn publish(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let name = nonce.iter().map(|n| format!("{n:02x}")).collect::<String>();
    let temporary = parent.join(format!(".factory-zstd-{name}.tmp"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(io)?;
    let result = (|| {
        file.write_all(bytes).map_err(io)?;
        file.sync_all().map_err(io)?;
        fs::hard_link(&temporary, path).map_err(io)?;
        File::open(parent).and_then(|f| f.sync_all()).map_err(io)
    })();
    drop(file);
    let cleanup = fs::remove_file(temporary).map_err(io);
    result.and(cleanup)
}
fn bounded_bytes(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    let file = File::open(path).map_err(io)?;
    if file.metadata().map_err(io)?.len() > maximum {
        return Err("codec file budget exceeded".into());
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes).map_err(io)?;
    if bytes.len() as u64 > maximum {
        return Err("codec file grew beyond budget".into());
    }
    Ok(bytes)
}
/// Compress the same native bytes that were validated; never reopen a mutable source.
pub fn compress(source: &Path, destination: &Path) -> Result<()> {
    let raw = bounded_bytes(source, native::MAX_BYTES)?;
    native::decode(raw.as_slice(), raw.len() as u64)?;
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), LEVEL).map_err(io)?;
    encoder.include_checksum(true).map_err(io)?;
    encoder.window_log(23).map_err(io)?;
    encoder.write_all(&raw).map_err(io)?;
    let payload = encoder.finish().map_err(io)?;
    if payload.len() as u64 > MAX_COMPRESSED {
        return Err("compressed payload budget exceeded".into());
    }
    let mut bytes = Vec::with_capacity(56 + payload.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&payload);
    bytes.extend_from_slice(blake3::hash(&bytes).as_bytes());
    publish(destination, &bytes)
}
fn decoded(path: &Path) -> Result<(Vec<u8>, Dataset)> {
    let bytes = bounded_bytes(path, MAX_COMPRESSED + 56)?;
    if bytes.len() < 56 || &bytes[..8] != MAGIC {
        return Err("unsupported compressed native format".into());
    }
    let raw_len = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
    let compressed_len = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
    if !(53..=native::MAX_BYTES).contains(&raw_len)
        || compressed_len == 0
        || compressed_len > MAX_COMPRESSED
        || bytes.len() as u64 != 56 + compressed_len
    {
        return Err("compressed native length/budget mismatch".into());
    }
    let end = 24 + compressed_len as usize;
    if blake3::hash(&bytes[..end]).as_bytes() != &bytes[end..] {
        return Err("compressed native checksum mismatch".into());
    }
    let mut decoder = zstd::stream::read::Decoder::with_buffer(&bytes[24..end])
        .map_err(io)?
        .single_frame();
    decoder.window_log_max(26).map_err(io)?;
    let mut raw = Vec::new();
    raw.try_reserve_exact(raw_len as usize)
        .map_err(|e| e.to_string())?;
    decoder
        .by_ref()
        .take(raw_len + 1)
        .read_to_end(&mut raw)
        .map_err(io)?;
    if raw.len() as u64 != raw_len || !decoder.finish().is_empty() {
        return Err("compressed native decoded length/trailing frame mismatch".into());
    }
    let dataset = native::decode(raw.as_slice(), raw_len)?;
    Ok((raw, dataset))
}
pub fn read(path: &Path) -> Result<Dataset> {
    Ok(decoded(path)?.1)
}
/// Restore byte-identical native contents after all wrapper/native/structural checks.
pub fn decompress(source: &Path, destination: &Path) -> Result<()> {
    let (raw, _) = decoded(source)?;
    publish(destination, &raw)
}
