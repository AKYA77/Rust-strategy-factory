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
