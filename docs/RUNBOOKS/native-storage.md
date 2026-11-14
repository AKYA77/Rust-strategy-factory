# Native uncompressed SoA baseline

This is a bounded lossless interchange/storage foundation, not the selected production hot format or the required Parquet/ZSTD cold baseline. Research execution currently reconstructs the existing in-memory event representation. Compression, memory mapping, direct SoA execution, full-history streaming and representative real-data measurements remain unfinished.

```sh
mkdir -p state
./target/release/rust-strategy-factory fixture EURUSD state/fixture.json
./target/release/rust-strategy-factory pack-native state/fixture.json state/fixture.rsf
./target/release/rust-strategy-factory inspect state/fixture.rsf
./target/release/rust-strategy-factory unpack-native state/fixture.rsf state/restored.json
```

All output paths must be new. `inspect`, `candles`, `evaluate`, `experiment`, and research/protocol input files accept native datasets with the `.rsf` extension. Other dataset extensions continue to select JSON. `unpack-native` explicitly decodes a native input regardless of extension. Research configuration still uses the `dataset` path and the same event/byte/training/conversion budgets. For example, replace `../state/fixture.json` with `../state/fixture.rsf`. Conversions remain separate explicit event-aligned JSON; no FX rates are inferred.

The bytes encode `RSFNAT01`, a little-endian u64 count, a u32 metadata length, exact typed profile/manifest JSON, and four complete little-endian columns in this order: timestamp i64, sequence u64, bid i64, ask i64. A trailing BLAKE3 covers all preceding bytes. The exact file length is `52 + metadata_length + 32 * count`. There is no lossy conversion, price rounding, sorting, delta convention or fabricated quality flag. Format version, lengths, checksum and dataset identities are checked before use. Sequence values retain the entire u64 domain.

Limits are two million events, one MiB encoded metadata and a 64 KiB column staging buffer. Header/file-length checks precede event allocation. Decode validates the exact stream checksum and original manifest/profile/event invariants; checksums do not replace structural validation, source approval or authenticity. Trailing/truncated input and changed headers are rejected. Invalid/quarantined datasets cannot be published through this writer. All existing source/archive/parser/category metadata remains unchanged; native encoding never promotes data readiness.

Publication writes a random new temporary inode in the destination directory, flushes/syncs it, hard-links without replacement, syncs the directory and removes the temporary name. Existing artifacts remain untouched on an overwrite attempt. Preserve immutable original source archives separately: this wrapper is not a replacement for source/license/acquisition evidence. Disk-full/crash-injection and production filesystem acceptance remain pending.

Four native tests cover all eight profiles, tied event order, exact provenance, chunk boundaries, high unsigned sequences, hostile lengths, checksum corruption, truncation/trailing bytes, independent structural validation and no-overwrite cleanup. The real CLI test packs/unpacks and replays a nonzero trade through native input, then reuses the JSON experiment identity. Chromium acceptance runs the actual standalone research loop using native input before inspecting its persisted results.

A bounded lossless ZSTD wrapper is now available as `.rsfz`; see compressed-native.md for framing, exact native restoration, source/licenses, window/output/file caps, CLI equivalence and component measurements. The native format itself is unchanged. This wrapper does not supply full-history streaming, Parquet comparison or a selected production codec.

## Ordered multi-chunk corpus inventory

```sh
rust-strategy-factory corpus-create data/corpus.json data/chunk-000000.rsf data/chunk-000001.rsf
rust-strategy-factory corpus-inspect data/corpus.json
```

Creation describes and verifies existing native chunks; it never concatenates or overwrites them. The new specification is published through the existing synced no-replace JSON writer after a complete ordered inspection. Paths initially use canonical local locations. Moving unchanged artifacts and updating only their paths preserves corpus identity; each ordered chunk's exact consumed-byte BLAKE3 and dataset/provenance identity remain mandatory. Original archives/catalog receipts stay separate. A path list is not evidence of source completeness or approval.

The versioned specification contains `version: "ordered-native-corpus-v1"`, `max_chunk_events`, and ordered `chunks` with `path`, `dataset_identity`, `file_hash`. The CLI uses65536 events/chunk; the API permits1..100000, at most10000 chunks and a16MiB loaded specification. Relative paths in an edited specification resolve against its directory. Native count limits are checked before allocating rows. The bounded reader fingerprints the exact descriptor stream being decoded rather than reopening the path for a later hash. Existing native/RSFZ/Parquet loading defaults are unchanged; compressed corpus blocks remain future work.

Inspection retains one quote chunk plus seven incremental candle states and compact metadata/hashes. It compares the exact profile, declared class and source across chunks, validates each immutable artifact, and refuses backward timestamps, reused/reset sequence IDs, revisions or corrupt bytes. Gaps/ties/repeated quotes are observed across boundaries without inserting/deleting events. Global ordered-event and each timeframe's candle hashes equal the ordinary canonical JSON-array hash. The shared candle publication transition emits only when a later real quote crosses a boundary; chunk ends never finalize or discard a forming candle. No final unfinished bucket is emitted. All seven timeframes work across chunk boundaries.

Output is bounded counts/fingerprints with exact decimal sequence strings, not an in-memory full-history candle/tick array. Per-chunk metadata/validation allocations and the specification add memory beyond the quoted resident-event bound. There is no catalog/classification promotion, missing-range inference, provider acquisition/resume, scientific approval or strategy/account replay streaming. The existing experiment/research loaders retain their materialized input limits. Future streaming features/replay must consume this ordered path without resetting account, causal warmup or publication state.
