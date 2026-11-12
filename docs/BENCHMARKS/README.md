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
