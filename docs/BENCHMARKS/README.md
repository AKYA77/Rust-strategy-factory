# Synthetic baseline — 2026-11-12

Rust 1.100.0, release thin LTO, one codegen unit. This baseline is SYNTHETIC_TEST_ONLY, warm resident fixtures, 20,000 events per instrument, eight instruments. Candles/features and receipt identities are rebuilt each trial. It is not evidence of genuine-data, compression, full-history, index acceleration or target-VPS performance.

| Workers | Median wall seconds / eight replays | Max minus min seconds | Resolved trades / batch |
|---|---:|---:|---:|
| 1 | 0.087730 | 0.019085 | 6997 |
| 2 | 0.045448 | 0.011943 | 6997 |
| 3 | 0.030509 | 0.024142 | 6997 |
| 4 | 0.038998 | 0.032366 | 6997 |

Five repetitions per worker count. Three workers had the fastest median in this short trial; this does not establish a universal production worker count. Whole benchmark child: 1.108s wall, 1.815s user CPU, 0.082s system CPU, peak RSS 21620 KiB. I/O block counters are process totals, not bytes of market data decoded. Exact CPU model/flags, cgroup limits, per-trial times, dataset hashes, source inputs hash and executable hash are in the JSON artifacts. No new research discoveries or cached simulation counts are claimed by repeating fixtures.

Reproduce using `cargo bench --locked --bench replay`. For isolated child resource accounting: compile with `cargo bench --locked --bench replay --no-run`; pass its reported executable and two new output JSON paths to `python3 scripts/measure.py`. The wrapper uses wait4 per-child statistics to exclude compilation. Check the child exit code before using the result.

Missing acceptance: legally usable representative real quotes, cold/warm I/O comparison, compression encode/decode/random access, first-hit index versus full replay accounting, sustained load and actual four-vCPU/eight-GB VPS validation.

## Order lifecycle refactor checks

The intermediate `orders` and `orders-final` receipts are retained as measured iterations, including their source/executable hashes. Three subsequent alternating pairs compare frozen commit a5f4a13 with the final order-fill refactor and stream-hashing source. Each executable performs the same five repetitions per worker count. The aggregate contains all underlying reports and per-child resource receipts. Every batch resolved 6,997 synthetic trades.

| Workers | Baseline median of medians (s) | Refactor median of medians (s) |
|---|---:|---:|
| 1 | 0.114655 | 0.083782 |
| 2 | 0.079524 | 0.119404 |
| 3 | 0.085774 | 0.077548 |
| 4 | 0.114681 | 0.109591 |

Results vary substantially and differ by worker count; they establish neither a consistent speedup nor a universal worker setting. No optimization is enabled on this evidence. Stable sustained/cold-I/O and real-data comparisons remain mandatory before performance acceptance.

## Engine v6 packed-prefix and sequential baseline

Current receipts: [component timings](v6-prefix-component.json), [component resources](v6-prefix-component-resources.json), [sequential timings](v6-sequential-synthetic.json), [sequential resources](v6-sequential-synthetic-resources.json). Exit codes are zero. Both binaries report the same compiled compute revision; source fingerprint/per-file digests were independently verified against the measured working tree. The checkout was dirty at measurement; the per-file manifest, not the base Git SHA alone, identifies the exact code. Resource wrapper v2 now includes build/toolchain inputs, compiler/CPU/features, cgroup limits and its own hash.

Component only: equal resident masks, 999,983 relevant bits, five alternating repetitions of 25 comparisons. Median per-call scalar validation was 2.688380 ms; packed validation was 0.002947 ms. Allocation/generation is excluded from operation timers. This is bit-mask validation, **not candidate screens, market replays or research throughput**, and establishes no whole-factory speedup.

The separate sequential benchmark executes eight 20,000-event synthetic histories per repetition, rebuilding derived candles/features. Fixed100 stops exercise the actual engine/risk/accounting path; ATR support is present but these timings do not benchmark ATR policies. Every batch resolved 6,997 synthetic trades. Five repetitions per worker count:

| Workers | Median batch seconds | Max-minus-min seconds | Resolved trades/batch |
|---|---:|---:|---:|
| 1 | 0.102943 | 0.043004 | 6997 |
| 2 | 0.057072 | 0.024256 | 6997 |
| 3 | 0.048434 | 0.004141 | 6997 |
| 4 | 0.039235 | 0.004182 | 6997 |

Whole sequential child: 1.329842 s wall, 2.280543 s user CPU, 0.040079 s system CPU, peak RSS 21696 KiB. These are process totals across trials, not per-worker memory. The machine reports Xeon Platinum 8573C, four-vCPU quota and 32 GiB cgroup memory cap. This short warm fixture favored four workers; it neither sets a production default nor validates the target eight-GB VPS. There is no before/after whole-engine A/B claim, real-data/full-history performance, cold I/O or codec measurement. Repeated fixtures are not new research discoveries or qualified strategies.

Build `cargo bench --locked --bench replay --bench mask_prefix --no-run`; measure each reported executable separately with new output paths using scripts/measure.py. Compilation is excluded by wait4 child accounting. Archive existing receipts rather than overwriting them.

## Structural generation component — 2026-11-13

`cargo bench --locked --bench exploration --no-run` builds the standalone generator check. Pass the reported executable to `scripts/measure.py` with new result/resource paths. [Generation timings](structural-generation-v1.json) and [resources](structural-generation-v1-resources.json) retain a zero-exit run and checked per-file source manifest. Five repetitions of100 seeds use one synthetic M1 long parent. Median0.054807ms per generation call includes canonicalization/hash/dedup and output identity checks with warm fixed templates. Each repetition returns695 proposals and the same ordered workload hash;56 canonical concepts occur across seeds. Whole-child peak RSS10496KiB. These are component results with no quotes, screening, replay, useful behavioral diversity, discovery quality or full-history throughput claim. Earlier sequential receipts remain historical source snapshots.

## ZSTD/native component — 2026-11-13

[Codec timings](zstd-native-v1.json) and [resources](zstd-native-v1-resources.json) retain a zero-exit child with verified per-file source digests and binary/compiler/CPU/cgroup evidence. `cargo bench --locked --bench compressed --no-run` builds the executable; measure it through scripts/measure.py with NEW output paths. One20000-event synthetic EURUSD native file is exercised in five warm-file repetitions, checking every decoded dataset and restored byte stream. Native640965 bytes versus compressed76648 bytes. Median validated native read3.316922ms; compressed validate/encode/atomic publish6.771314ms; compressed validated read2.968584ms; exact restore/atomic publish6.661441ms. Whole-child peak RSS10496KiB. Timers include their documented validation/allocation/file-I/O scopes; fixture setup and equality serialization are outside operation timers. This tiny ordered warm component does not establish a universal decode improvement, research replay throughput, Parquet/cold-I/O/full-history performance or selected production storage.

## Same-corpus native/RSFZ/Parquet pipeline — 2026-11-13

[Timings](storage-pipeline-v1.json) and [resources](storage-pipeline-v1-resources.json) retain a zero-exit child with verified per-file source hashes. Eight20000-event synthetic instruments, three candidate complexities, five rotated repetitions and warm/advisory-eviction conditions produce30 batches. Every batch matches full persisted results/experiment identities and resolves17660 trades in24 new experiments;720 total new experiments and720 separate cached-result reads. This exercises current signal-stage screening, sequential replay and SQLite persistence, not a completed candidate net screen or independent qualification.

Total native/RSFZ/Parquet bytes:5127903/688609/618546. Warm median load+new pipeline517.515/530.934/534.987ms; advisory-eviction medians544.244/599.365/546.723ms. File-local fadvise succeeds and observed load I/O includes5144576/700416/626688 bytes, but physical coldness remains unverified; warm observations include zero storage reads. No global cache dropping. Peak whole-child RSS26208KiB. Read RUNBOOKS/parquet-storage.md and exact artifact scopes for encoder/helper/equality-check costs. Parquet is kept as the required standard cold baseline with smaller fixture size; no default change or general replay-speed improvement is claimed. Real-corpus, physical-cold and full-history acceptance remain pending. `scripts/measure.py` now uses workload-neutral limitations; older resource receipts' generic no-decode wording must not override their actual codec child measurements.

## Metadata-only health component

189 Rust tests, formatting/Clippy all targets, debug/release builds, actual Chromium desktop/mobile and local installation/recovery checks pass. An additive schema-v9 partial covering index contains only proof/indexed contradiction actions. SQLite maintains it for older compatible writers; no row format or version changes. A50000-event upgrade fixture verifies unchanged history/version, both parameterized covering plans, absent/present marker semantics, integrity and reopen. Existing same-name indexes are preserved.

`audit-health-v1` is metadata-only, not market/research throughput:50k/200k synthetic ordinary events, absent/present-tail markers,5 rotated scan/index repetitions and60 checks per record. At200k, median scans693.613892/715.381970ms versus indexed0.347232/0.295488ms; all outcomes match. Whole-child RSS10496KiB, exit0, source hashes verified. DDL separately timed; one-time upgrade scanning, production writes, disk-full and full-factory acceptance remain unproven. Indexed factory timing remains inconclusive and defaults unchanged. Hosted indexed-policy run43493131959 reports Success for exact da62884e65b22806c5bfaa128e7d50bc9d28de54; new milestone verification follows publication.

## Local normalized import cost

201 Rust tests, formatting/Clippy all targets, debug/release builds, actual Chromium desktop/mobile and local install/recovery checks pass. Separate local import-step/status APIs bind full bounded source snapshot/hash, parser/profile/source/category/chunk size; one immediate WAL transaction serializes each chunk. Native publication/fsync precedes checkpoint commit. Stable metadata permits exact matching-orphan recovery after simulated checkpoint rollback; conflicts and damaged/missing published chunks refuse without overwriting. Bad rows/order never advance. This does not alter experiment registry schema or financial rules.

Nine tests cover whole-source ordered quote equality/full-u64 ties, actual separate CLI process resume/status, changed revision/profile/category/source/chunk refusal, crossed/malformed/cross-boundary rows, rollback/orphan conflict preservation, missing artifacts, checkpoint corruption, unrelated database preservation, competing writer, empty/source/chunk bounds, CRLF/quotes/blank tails/no newline and real-parser classification ceiling. Every fixture is synthetic; invoking the real-parser branch in a disposable test does not establish genuine data.

Source-pinned import-job-v1 measures5 rotated pairs of one20000-event/708920-byte synthetic source, not research throughput. All10 combined ordered quote hashes match; parser/layout artifact identities intentionally differ. Median monolithic import+native publication10.828293ms;4 checkpoint steps43.179817ms (slower, retained recovery overhead); completed source/artifact revalidation4.902842ms separately. Native artifacts641018 bytes/one vs644092/four, excluding ledger/temp quota. Whole-child RSS10496KiB, exit0/source hashes verified. No per-path RSS, physical-cold/source acquisition/full-history/factory speed claim. See RUNBOOKS/normalized-import.md. Continuous completed-job export, source catalog/calendar/quality approval and actual SIGKILL/disk-full/power-loss acceptance remain open. Hosted prior regression run43516704107 reports Success for exact5e1f5448eb4ee224e43ffd7d6f01d42797126fa9; new import milestone verification follows publication.

Review SQL lookup: review-query-v1.json is a warm in-memory Python SQLite3.53.1 component comparison using5000 explicitly invalid-business-receipt metadata fixtures,5 matching IDs,10 rotated scan/index pairs and12 queries each. Timed SQL setup/fetch only; no Rust receipt validation or financial processing. Medians12.677626ms/0.074934ms, whole-process RSS21056KiB. Reproduce with scripts/benchmark-review-query.py and a new output path. Source manifests are retained. The concurrent-build exploratory run is separately labeled; neither establishes full application speed/RSS or cold-I/O acceptance.

## Ordered native corpus / incremental candles — 2026-11-14

Final [timings](corpus-v3.json) and [resources](corpus-v3-resources.json) retain a zero-exit child with all source digests independently matched to the measured tree.33 immutable native chunks contain2,162,688 synthetic EURUSD events (69,238,841 bytes), exceeding the prior single-artifact event cap. Five warm scans verify the independent explicit ordered-event oracle and identical seven-timeframe candle fingerprints. At most65,536 quotes are retained per scan chunk; no full corpus concatenation or candle array is retained. Median complete inventory/candle scan1374.234167ms, max-minus-min98.842013ms. Whole child9.601743s wall,9.219656s user,0.295967s system, peak RSS14472KiB; includes bounded fixture generation, independent event oracle, component comparisons and all scans. Input block counters are zero (warm cached files); this is not physical-cold evidence, per-scan RSS, compression, market replay, screens, backtests or research throughput. It is one synthetic instrument and does not establish full-history/VPS acceptance.

The frozen pre-stream batch-candle component and final shared-transition implementation process the same65,536-event input25 times in each of five alternating pairs, including dataset validation. Medians148.045529/150.294600ms per25 calls: the final component retains about1.5% median overhead, not a speedup. This bounded architectural cost is accepted to share one checked candle publication transition with streaming input; actual financial/candle equality tests pass. No engine/policy default is promoted on this evidence. Broader representative end-to-end measurement remains open.

Retained negative iterations: [v1 timings](corpus-v1.json)/[resources](corpus-v1-resources.json) initially measured144.629098/155.743408ms (about7.7% median batch overhead); [v2 timings](corpus-v2-inline.json)/[resources](corpus-v2-inline-resources.json) reduced it to144.159699/148.744034ms using inlining. Final v3 removes a repeated optional-initialization branch from the batch path while preserving the shared transition. Source-pinned reverse patches [v1](corpus-v1-from-v3.patch)/[v2](corpus-v2-inline-from-v3.patch) reconstruct the exact earlier candle source from v3; their reconstructed SHA256s were checked against the original resource manifests. All other measured source files match the published74a3cfd corpus snapshot. A later build-input enumeration change is separate identity hardening, not a new performance measurement. Apply patches only in a disposable measurement copy. Do not overwrite existing timing evidence.

Reproduce with `cargo bench --locked --bench corpus --no-run`, then pass its reported executable to `scripts/measure.py` with two NEW output paths. Fixture creation retains one extra sample chunk during setup/component work; the whole-child RSS includes this and is not claimed to isolate scan-only memory. Native chunk traversal/candles are integrated into corpus-create/corpus-inspect; strategy features, account replay, compressed blocks and durable streaming checkpoints remain separate unfinished work.
