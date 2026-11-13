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
