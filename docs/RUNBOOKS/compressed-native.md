# Bounded ZSTD/native storage baseline

This adds a lossless compressed wrapper, not a selected production codec or full-history streaming engine. Parquet/ZSTD comparison, mmap/direct hot arrays, cold I/O, representative real-data throughput and sustained full-history acceptance remain pending. Encoding never changes dataset category, provenance, profile, event order or scientific status.

From the repository root after the pinned locked build:

```sh
cargo run --locked -- pack-native state/input.json state/input.rsf
cargo run --locked -- compress-native state/input.rsf state/input.rsfz
cargo run --locked -- inspect state/input.rsfz
cargo run --locked -- decompress-native state/input.rsfz state/restored.rsf
```

Every destination must be new. Decompression reproduces exactly the original native bytes. `.rsfz` input is supported by normal inspect/evaluate/experiment/research/scientific-protocol input loading; `pack-native` can also convert it back to native. The original native `.rsf` format remains readable and unchanged. CLI acceptance verifies that JSON/native/compressed inputs reuse one exact experiment within the same build rather than adding attempts. Browser research and its frozen protocol now exercise compressed input.

The wrapper magic is `RSFZST01`, followed by little-endian u64 uncompressed/native length, u64 compressed payload length, one ZSTD frame, and a32-byte BLAKE3 over header plus compressed payload. The native payload keeps its existing metadata/column checksum and structural validation. Writes use level3, window_log23 and the frame checksum. Readers enforce one frame with no unconsumed trailing input, a maximum decoder window_log26, exact decoded length, the complete native checksum and dataset/profile/order/structure checks. Checksums detect corruption; they do not approve provenance or establish adversarial authenticity.

Limits are explicit: native metadata1MiB, at most2,000,000 events and at most65,048,628 raw/native bytes; compressed payload at most128MiB. Lengths are checked before declared output reservation. Reads also cap bytes actually captured if a file grows after metadata inspection. Decoding takes at most declared length+1 bytes and refuses excess output. Compression validates the same captured source bytes that it compresses, without reopening a mutable source. Atomic same-directory hard-link publication never replaces an existing destination and syncs file/parent; ordinary failures clean only their own temporary file.

This bounded baseline holds native/compressed byte buffers and reconstructs owned event structs. The library window and file/output caps are not an OS RSS quota. It is not mmap, zero-copy or streaming across full history. Use target resource enforcement and suitable bounded workloads; do not infer billions-of-events support from a small compressed file.

`zstd0.13.3` is pinned without default features. Cargo.lock resolves `zstd-safe7.4.1` and `zstd-sys2.1.3+zstd.1.5.7`; normal Cargo artifact/TLS checks remain enabled and the downloaded archive SHA256 values were separately checked against lock entries. The actual selected source/license review is retained in `SOURCE_REVIEW/2026-11-13-zstd.json`. Runtime binding/C sources cross a third-party unsafe/C boundary; the application uses safe Rust interfaces without its own unsafe blocks. This selected review is not a complete supply-chain/security audit.

The MIT/BSD notices for this added stack are in `THIRD_PARTY_NOTICES/zstd.txt`. New local installs copy that bundle and bind its SHA256 in manifest format2; verification refuses notice tampering. Historical format1 releases remain readable. The installer needs the notice file distributed with the script. This bundle does not complete the pre-existing dependency/license audit.

Three focused tests cover all eight synthetic profiles, tied timestamps/full-u64 sequences, byte-exact round trips, no replacement/cleanup, outer/inner checksum failures, absurd lengths, bounded excess output, valid-wrapper/truncated ZSTD, a trailing empty frame, and structurally invalid data even with recomputed checksums. No financial/scientific assertion is relaxed.

Reproduce the component measurement with `cargo bench --locked --bench compressed --no-run`, then pass its reported executable and two NEW output paths to `python3 scripts/measure.py`. The benchmark uses one20,000-event synthetic EURUSD fixture and five warm-file repetitions. It separately records native validated read, compressed validate/encode/publish, compressed validated read and exact native restore/publish. It checks every decoded dataset and restored byte stream. Read the result/resource/source receipts and their workload limits before interpreting times; they are not research replay throughput or a codec selection decision.
