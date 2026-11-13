//! Strict bounded Parquet/ZSTD cold baseline: exact required integer columns, no Arrow runtime.
use crate::{
    Result,
    contracts::Profile,
    data::{Dataset, Manifest, Tick},
};
use parquet::{
    basic::{Compression, Encoding, ZstdLevel},
    column::reader::ColumnReader,
    data_type::Int64Type,
    file::{
        metadata::KeyValue,
        properties::{EnabledStatistics, WriterProperties, WriterVersion},
        reader::FileReader,
        serialized_reader::SerializedFileReader,
        writer::SerializedFileWriter,
    },
    schema::parser::parse_message_type,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::Arc,
};
#[path = "parquet_guard.rs"]
mod guard;
const SCHEMA: &str = "message rsf_ticks { REQUIRED INT64 timestamp_ms (TIMESTAMP(MILLIS,true)); REQUIRED INT64 sequence (UINT_64); REQUIRED INT64 bid; REQUIRED INT64 ask; }";
const META_KEY: &str = "rsf.dataset.v1";
const MAX_FILE: u64 = 128 * 1024 * 1024;
const MAX_FOOTER: usize = 2 * 1024 * 1024;
const ROWS: usize = 8192;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    profile: Profile,
    manifest: Manifest,
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
struct Limited<'a> {
    file: &'a mut File,
    written: u64,
}
impl Write for Limited<'_> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if self.written + b.len() as u64 > MAX_FILE {
            return Err(std::io::Error::other("Parquet output file budget exceeded"));
        }
        let n = self.file.write(b)?;
        self.written += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
pub fn write(path: &Path, dataset: &Dataset) -> Result<()> {
    dataset.verify()?;
    if dataset.ticks.len() > 2_000_000 {
        return Err("Parquet event budget exceeded".into());
    }
    let metadata = serde_json::to_string(&Metadata {
        profile: dataset.profile.clone(),
        manifest: dataset.manifest.clone(),
    })
    .map_err(err)?;
    if metadata.len() > 1024 * 1024 {
        return Err("Parquet metadata budget exceeded".into());
    }
    let schema = Arc::new(parse_message_type(SCHEMA).map_err(err)?);
    let props = Arc::new(
        WriterProperties::builder()
            .set_writer_version(WriterVersion::PARQUET_1_0)
            .set_dictionary_enabled(false)
            .set_encoding(Encoding::PLAIN)
            .set_compression(Compression::ZSTD(ZstdLevel::try_new(3).map_err(err)?))
            .set_statistics_enabled(EnabledStatistics::None)
            .set_data_page_size_limit(65536)
            .set_data_page_row_count_limit(ROWS)
            .set_write_batch_size(ROWS)
            .set_max_row_group_row_count(Some(ROWS))
            .set_key_value_metadata(Some(vec![KeyValue::new(META_KEY.into(), metadata)]))
            .build(),
    );
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(err)?;
    let temporary = parent.join(format!(
        ".factory-parquet-{}.tmp",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(err)?;
    let result = (|| {
        {
            let mut writer = SerializedFileWriter::new(
                Limited {
                    file: &mut file,
                    written: 0,
                },
                schema,
                props,
            )
            .map_err(err)?;
            for block in dataset.ticks.chunks(ROWS) {
                let mut group = writer.next_row_group().map_err(err)?;
                for column in 0..4 {
                    let values = block
                        .iter()
                        .map(|t| match column {
                            0 => t.timestamp_ms,
                            1 => t.sequence as i64,
                            2 => t.bid,
                            _ => t.ask,
                        })
                        .collect::<Vec<_>>();
                    let mut column = group
                        .next_column()
                        .map_err(err)?
                        .ok_or("Parquet output column missing")?;
                    if column
                        .typed::<Int64Type>()
                        .write_batch(&values, None, None)
                        .map_err(err)?
                        != block.len()
                    {
                        return Err("Parquet incomplete column write".into());
                    }
                    column.close().map_err(err)?;
                }
                group.close().map_err(err)?;
            }
            writer.close().map_err(err)?;
        }
        file.seek(SeekFrom::End(-8)).map_err(err)?;
        let mut trailer = [0u8; 8];
        file.read_exact(&mut trailer).map_err(err)?;
        if u32::from_le_bytes(trailer[..4].try_into().unwrap()) as usize > MAX_FOOTER {
            return Err("Parquet output footer budget exceeded".into());
        }
        file.sync_all().map_err(err)?;
        fs::hard_link(&temporary, path).map_err(err)?;
        File::open(parent).and_then(|f| f.sync_all()).map_err(err)
    })();
    drop(file);
    let cleanup = fs::remove_file(temporary).map_err(err);
    result.and(cleanup)
}
pub fn read(path: &Path) -> Result<Dataset> {
    let file = File::open(path).map_err(err)?;
    if file.metadata().map_err(err)?.len() > MAX_FILE {
        return Err("Parquet file budget exceeded".into());
    }
    let mut captured = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut captured)
        .map_err(err)?;
    if captured.len() as u64 > MAX_FILE
        || captured.len() < 12
        || &captured[..4] != b"PAR1"
        || &captured[captured.len() - 4..] != b"PAR1"
    {
        return Err("Parquet file magic/size mismatch".into());
    }
    let footer = u32::from_le_bytes(
        captured[captured.len() - 8..captured.len() - 4]
            .try_into()
            .unwrap(),
    ) as usize;
    if footer == 0 || footer > MAX_FOOTER || footer > captured.len() - 12 {
        return Err("Parquet footer budget/length mismatch".into());
    }
    let footer_start = captured.len() - 8 - footer;
    guard::Compact::new(&captured[footer_start..captured.len() - 8]).footer()?;
    let captured = bytes::Bytes::from(captured);
    let reader = SerializedFileReader::new(captured.clone()).map_err(err)?;
    let meta = reader.metadata();
    let fm = meta.file_metadata();
    if fm.schema_descr().root_schema() != &parse_message_type(SCHEMA).map_err(err)? {
        return Err("Parquet exact tick schema mismatch".into());
    }
    if !(1..=2_000_000).contains(&fm.num_rows())
        || meta.num_row_groups() == 0
        || meta.num_row_groups() > 4096
    {
        return Err("Parquet row/group budget exceeded".into());
    }
    let kv = fm
        .key_value_metadata()
        .ok_or("Parquet dataset metadata absent")?;
    if kv.len() != 1 || kv[0].key != META_KEY {
        return Err("Parquet dataset metadata key mismatch".into());
    }
    let metadata = kv[0]
        .value
        .as_deref()
        .ok_or("Parquet dataset metadata value absent")?;
    if metadata.len() > 1024 * 1024 {
        return Err("Parquet dataset metadata budget exceeded".into());
    }
    let metadata: Metadata = serde_json::from_str(metadata).map_err(err)?;
    if metadata.manifest.events as i64 != fm.num_rows() {
        return Err("Parquet manifest row count mismatch".into());
    }
    let mut total = 0usize;
    for group in meta.row_groups() {
        if !(1..=ROWS as i64).contains(&group.num_rows()) || group.num_columns() != 4 {
            return Err("Parquet row group shape mismatch".into());
        }
        total = total
            .checked_add(group.num_rows() as usize)
            .ok_or("Parquet rows overflow")?;
        for column in group.columns() {
            if !matches!(column.compression(), Compression::ZSTD(_))
                || column.dictionary_page_offset().is_some()
                || column.file_path().is_some()
                || column.num_values() != group.num_rows()
            {
                return Err("Parquet column codec/count/external-path mismatch".into());
            }
            let (start, length) = column.byte_range();
            let end = start
                .checked_add(length)
                .filter(|n| *n <= footer_start as u64)
                .ok_or("Parquet column range outside captured data")?
                as usize;
            if start < 4 || length == 0 {
                return Err("Parquet column range invalid".into());
            }
            let mut cursor = start as usize;
            let mut values = 0usize;
            while cursor < end {
                let (header, encoded, count) =
                    guard::Compact::new(&captured[cursor..end.min(cursor + 65536)]).page()?;
                cursor = cursor
                    .checked_add(header)
                    .and_then(|n| n.checked_add(encoded))
                    .filter(|n| *n <= end)
                    .ok_or("Parquet page exceeds column range")?;
                values = values
                    .checked_add(count)
                    .filter(|n| *n <= group.num_rows() as usize)
                    .ok_or("Parquet page values exceed row group")?;
            }
            if values != group.num_rows() as usize {
                return Err("Parquet page value total mismatch".into());
            }
        }
    }
    if total as i64 != fm.num_rows() {
        return Err("Parquet row group total mismatch".into());
    }
    let mut ticks = Vec::new();
    ticks.try_reserve_exact(total).map_err(err)?;
    for index in 0..meta.num_row_groups() {
        let group = reader.get_row_group(index).map_err(err)?;
        let rows = group.metadata().num_rows() as usize;
        let mut columns = Vec::with_capacity(4);
        for column in 0..4 {
            let ColumnReader::Int64ColumnReader(mut column) =
                group.get_column_reader(column).map_err(err)?
            else {
                return Err("Parquet integer column required".into());
            };
            let mut values = Vec::with_capacity(rows);
            if column
                .read_records(rows, None, None, &mut values)
                .map_err(err)?
                != (rows, rows, rows)
                || values.len() != rows
            {
                return Err("Parquet decoded row count mismatch".into());
            }
            columns.push(values);
        }
        for i in 0..rows {
            ticks.push(Tick {
                timestamp_ms: columns[0][i],
                sequence: columns[1][i] as u64,
                bid: columns[2][i],
                ask: columns[3][i],
            });
        }
    }
    let dataset = Dataset {
        profile: metadata.profile,
        manifest: metadata.manifest,
        ticks,
    };
    dataset.verify()?;
    Ok(dataset)
}
