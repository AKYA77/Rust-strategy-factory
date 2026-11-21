# Hardware / resource evidence — all 15 benchmarks (2026-11-21)

## Scope and boundary

All 15 executables produced by `benches/*.rs` were compiled in release mode and each was run in a fresh child process. Per-child CPU ticks, peak RSS and block I/O were captured with `scripts/measure.py` (`os.wait4` / `getrusage`), which excludes compilation. The child stdout is the benchmark result JSON.

Every workload here is **synthetic**. The registered engine gate is `SyntheticTestOnly`; no benchmark result is research evidence, real-market performance, broker-specific execution, scientific qualification or VPS acceptance. The existing 22-month converted corpus is `REAL_STRUCTURALLY_VALID` only (see `docs/historical-migration/REAL_DATA_STUDY_RECEIPT.json`); `ReplayMode::Diagnostic` output is labelled `diagnostic_simulation` and is not registered as an experiment. No real-data benchmark timing is claimed.

CPU/CPU-second/RSS/I-O figures are **whole-process** totals for the whole benchmark child, not per trial. Legacy receipts from earlier source revisions are retained for provenance; their numbers are not directly comparable to this run because the compiled source/input fingerprint differs.

## Environment

- CPU model: `AMD EPYC Processor (with IBPB)`; affinity CPUs: `6`; cgroup cpu.max: `unavailable`; cgroup memory.max: `unavailable`; `MemAvailable:    8738160 kB`.
- Compiler: `rustc 1.100.0 (0ef1bc386 2026-11-09)` (probe recorded verbatim in every receipt).
- Base revision at measurement: `4d25ceb7c2543ec4ca8842972b623b20eb1a422c` (`source_dirty_at_measurement: true` — result files were being written; per-file manifest is authoritative).
- Source input fingerprint (identical in all 15 receipts): `b892e4686a241e6198409ffdabc87b45336117119c091ee6aa441bb997c9f977`; measurement script sha256: `355f5dbe5a8a52c195b0314bf5f327916e8f080d3d4e8b4463dfb0555a732541`.
- Compiled compute revision (identical in all 15 result files): `compute-blake3:11e0114e6f10efe2bde619c8147a7007e2a5517c83a47b3fd83ab30cbc985f60`.

## Per-benchmark hardware / resource evidence

| Benchmark | Result class / workload | Wall s | User CPU s | System CPU s | Peak RSS KiB | In blocks | Out blocks | Binary sha256 |
|---|---|---:|---:|---:|---:|---:|---:|---|
| `audit_health` | METADATA_ONLY_NOT_RESEARCH_THROUGHPUT | 14.906 | 8.720 | 5.871 | 16512 | 0 | 109008 | `8c48377378cfcf580d8454fd08f6096d406520bcde385c5c995c18fc15e679bd` |
| `batch` | SYNTHETIC_TEST_ONLY 8 instruments x12 unique same-frame candidates x12000 quotes; same feature budget/default costs; five rotated new registry batches | 27.907 | 17.309 | 3.151 | 144272 | 0 | 592096 | `00cc06e6d7a8517ac0fce0be8ac55fe519e1d4023a03f3ce9f73df53aeffddf5` |
| `blocked_pipeline` | SYNTHETIC_FRESH_PROCESS_STREAMING_CHECKPOINT_PIPELINE | 105.364 | 86.379 | 6.142 | 19596 | 57240 | 884984 | `9b643c59cba87438025400beac76ba0f78fb7395c30b92b55df6fb3d03ab4cb6` |
| `candidate_screen` | SYNTHETIC_TEST_ONLY 8 instruments x 2 sides x 3 stop distances x 12000 quotes, five identical repetitions | 1.757 | 1.744 | 0.012 | 16512 | 0 | 912 | `7c05fbe40c02cb2119690f0aa877ab63d6c233edbe7990d480c29406fc65a839` |
| `compressed` | SYNTHETIC_CODEC_COMPONENT_NOT_RESEARCH_THROUGHPUT | 0.202 | 0.093 | 0.032 | 16512 | 0 | 8304 | `3c7700fd8a1b8b708a21a785404e4995e594b41a722c242a1bd0bbe5528174e8` |
| `corpus` | SYNTHETIC_TEST_ONLY ordered corpus/candle inventory and batch-candle component | 12.653 | 12.112 | 0.313 | 16512 | 0 | 135456 | `7f2700936245492352e7d5a1308246ce83ef79acf9ee6817308c22eb18f5054d` |
| `exploration` | COMPONENT_GENERATION_NOT_RESEARCH_THROUGHPUT | 0.046 | 0.043 | 0.001 | 16512 | 0 | 8 | `da12cefa86b02f129edcd7d6f1861be2fcd5d333dc4f88b0369d3171218f0231` |
| `import_job` | LOCAL_SYNTHETIC_IMPORT_NOT_RESEARCH_THROUGHPUT | 1.021 | 0.371 | 0.220 | 16512 | 0 | 16720 | `e5fce93eeaf62c0c6322abdbc4be7ebb8ddbf2e16609a9c70cdeed5835c9d58c` |
| `indexed_factory` | CURRENT_FACTORY_PIPELINE_WITH_SEPARATE_CACHE_READS | 31.863 | 24.455 | 1.993 | 119168 | 0 | 207136 | `c4ec8a616cb294b5077151ee6cc77d3a436dde5af2f77c2d0b275eea5760c3e6` |
| `indexed_replay` | FULL_ACCOUNTING_ENGINE_COMPARISON_NOT_FACTORY_PIPELINE | 13.131 | 13.079 | 0.050 | 44024 | 0 | 16 | `a84c01e81192fcddf743d431947b95eea9987770ec81cf92f72aae12b7cdf78a` |
| `mask_prefix` | COMPONENT_MICROBENCHMARK_NOT_RESEARCH_THROUGHPUT | 0.204 | 0.197 | 0.006 | 16512 | 0 | 8 | `c9c1d3fd523ad1585dd5e444c61c242d60ecc59f7aded407bde81b15ab61635f` |
| `proof_screen` | SYNTHETIC_PROVEN_NO_ENTRY_CURRENT_EVALUATION_COMPARISON | 3.827 | 1.978 | 0.588 | 27768 | 0 | 78848 | `d03d17b5c53ec49d21c9d6329a6d1527ebe5e60c38ba2a4a28924c0d08591724` |
| `replay` | SYNTHETIC_TEST_ONLY | 1.501 | 2.623 | 0.107 | 22336 | 0 | 8 | `181488c9d9b9e2d215f91b5f9ac181953eb2153844bd64d2f83d27f247ee1f02` |
| `sparse_mask` | COMPONENT_MICROBENCHMARK_NOT_RESEARCH_THROUGHPUT | 0.349 | 0.341 | 0.007 | 16512 | 0 | 24 | `7ec692bf735739c9e9d80b67c2266149cbb122338f3735c0721f6159e53b68b8` |
| `storage_pipeline` | SYNTHETIC_SAME_CORPUS_STORAGE_AND_CURRENT_RESEARCH_PIPELINE | 44.156 | 30.582 | 4.357 | 25472 | 63200 | 604736 | `ab4a54a75a7112bf20fdcc5c634c0a0409abc4906f5c89b4b11d48171345d309` |

Each `<benchmark>-result.json` carries the workload, measurements, `compute_revision`, scope/workload fields and explicit `limitations`. Each `<benchmark>-resources.json` carries `recorded_at_utc`, the full per-file source manifest, CPU features, cgroup/host memory, source-input fingerprint, executable sha256, wall/user/system CPU, peak RSS, block I/O and the measurement wrapper hash.

## Baseline receipts retained

Earlier receipts under `docs/BENCHMARKS/` (`*-v1-resources.json`, `2026-11-12-*-resources.json`, `v6-*-resources.json`, `candidate-screen-v2-sparse-resources.json`, `zstd-native-v1-resources.json`, etc.) are kept with their original source/executable fingerprints. They back the historical baselines described in `README.md` and `docs/PROJECT_STATE.json`; none was regenerated or silently replaced by this run.

## Reproduction

```sh
export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
export PATH="$CARGO_HOME/bin:$PATH"
flock -w 3600 /root/workspace/.rsf-heavy.lock sh -c \
  "cd /root/workspace/Rust-strategy-factory && cargo bench --locked --release --bench <name> --no-run"
python3 scripts/measure.py <reported-bench-executable> docs/BENCHMARKS/<name>-result.json docs/BENCHMARKS/<name>-resources.json
```

`measure.py` opens both output paths with `x` (no overwrite) and exits with the child status; check the resource receipt `exit_code` before using a result.
