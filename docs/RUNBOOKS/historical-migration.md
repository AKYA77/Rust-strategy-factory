# Historical Parquet → native migration

Conversion of the retired factory's corrected Parquet corpus into this repository's native
`.rsf` chunk format. This is a format conversion only. It does not acquire data, approve a
calendar or profile, repair source defects, or grant research eligibility.

## Source corpus

The legacy canonical partitions live under `/var/lib/hermes-factory/catalog/<dataset_hash>/`
with `quotes.parquet`, `source.bin` and a `manifest.json`. The legacy Parquet schema is the
ten-column `data/v2` shape:

```
optional binary instrument_id (String)
optional int64 event_ts_utc_ns
optional int64 bid_int
optional int64 ask_int
optional int64 price_scale
optional binary source_id (String)
optional binary ingest_batch_id (String)
optional int64 original_record_order
optional binary source_precision (String)
optional binary quality_flags (String)
```

Canonical selection is explicit and reproducible:

- accept only manifests with `quality_status = STRUCTURAL_PASS_CALENDAR_REVIEW_REQUIRED` and
  the exact 24-field `data/v2` manifest shape;
- group by `(instrument, year, month)` obtained by joining `source_sha256` to the archive
  manifest under `/var/lib/hermes-factory/archives/`;
- for a duplicated group, select the lexicographically smallest `dataset_hash`.

This yields 176 partitions (8 instruments × 22 months, 2025-01 through 2026-10) and
421,078,937 rows. The seven excluded records are four `LEAF_STRUCTURAL_CHECKS_ONLY_*`
partitions, two unclassified partitions, and one byte-identical duplicate of AUDUSD 2026-10.
The broader 183-manifest inventory totals 449,323,635 rows and is not the accepted canonical
total.

## Conversion rules

- Row groups are streamed in bounded batches; a month is never materialized as a whole.
- `event_ts_utc_ns` must be non-negative and exactly divisible by 1,000,000. The quotient is
  the native `timestamp_ms`. Sub-millisecond precision is refused, never truncated.
- `price_scale` must be an exact positive multiple of the profile's `price_scale`. Both
  `bid_int` and `ask_int` must be exactly divisible by the resulting factor. The native
  `bid`/`ask` are the exact quotients; no floating point is used.
- `sequence` is the zero-based global retained-row index. It is strictly increasing and
  preserves published event order, including timestamp ties.
- Timestamps must be non-decreasing. Crossed, nonpositive, quality-flagged, mixed-source or
  mixed-scale rows are refused.
- `quote_stream_sha256` is recomputed as SHA-256 over the sorted-key metadata JSON
  (`batch`, `instrument`, `precision`, `scale`, `source`) followed by each row's four
  big-endian signed 64-bit fields (`event_ts_utc_ns`, `bid_int`, `ask_int`,
  `original_record_order`). It must equal the trusted manifest value.
- The legacy Parquet file SHA-256 must equal `parquet_sha256`, and the file is re-hashed after
  conversion to detect mutation.
- Previously removed or quarantined rows are not recreated. The import receipt's
  `import_resume` correction/quarantine evidence is preserved in the migration receipt.

## Output

```
<output-dir>/
  chunk-000000.rsf   # native SoA, <= --chunk-events rows
  chunk-000001.rsf
  manifest.json      # migration receipt, published last
```

Each chunk carries a normal native `Manifest` with
`parser_version = historical-parquet-migration-chunk-v1`, a deterministic `imported_at`
derived from the source identity, and `class = REAL_STRUCTURALLY_VALID`. The receipt records
the source identity, chunk hashes/bytes/counts, exact totals (events, first/last
millisecond, timestamp ties, identical quote events, gaps over one hour), the reproduced
`quote_stream_sha256`, preserved limitations and a deterministic `output_hash`.

Chunks are written with the existing atomic no-replace native publisher. A completed
directory is never rewritten: re-running verifies and returns the existing receipt. If a run
is interrupted, the committed chunks are verified and the conversion resumes from their exact
boundary; an uncommitted or conflicting chunk is refused, never overwritten.

## CLI

```sh
cargo run --locked -- historical-convert \
  <legacy.parquet> <output-dir> <profile.json> <legacy-manifest.json> <import-receipt.json> \
  [chunk-events]

cargo run --locked -- historical-verify <output-dir> [legacy.parquet]
```

`chunk-events` defaults to 100,000 and is capped at 100,000, matching the native
`ordered-native-corpus-v1` admission cap. The input file is capped at 512 MiB and the chunk
count at 8,192. Profiles live in
`configs/historical-migration/profiles/`; they use the legacy fixed-point scale 1,000,000,
`tick` equal to the observed source quantum, and an explicit not-approved provenance string.

## Verified development samples

Both samples were converted and independently read back: every native tick equals the legacy
Parquet row exactly (timestamp, sequence, bid, ask), sequences are strictly increasing and
timestamps non-decreasing.

| Sample | Events | Chunks | Quote stream SHA-256 | Output hash |
| --- | --- | --- | --- | --- |
| NZDUSD 2025-12 | 876,772 | 2 | `73063fe49795367da85c2344d03cc7ff96a1cab9892321c5c390f985e965100b` | `3473fee1de619f65d0384689321f944a1400efe743ba92c68e06aedad5dc34e8` |
| USDCAD 2026-07 | 1,226,059 | 3 | `e7605ea2d554b13ddda2bdb1a02be07df2fce3b41dd0f21f2d6fa31f7b5aded4` | `3043d9ace63d8801ffff94b11635cc0f9df1cfa94d14f22e8ed9982302b29b2a` |

USDCAD preserves the legacy quarantine evidence: 1,226,412 rows in, 1,226,059 retained, 353
crossed quotes removed, `corrected_content_sha256 =
e95a94ae9192019ac61c49c4a8276bc656ac0ab621ca57dbcc19dc770b057477`.

These results demonstrate real-data import correctness only. They do not establish calendar
completeness, profile approval, source licensing or research eligibility.

## Full canonical corpus

`historical-plan` selects the canonical partitions and `historical-batch` converts them with a
bounded worker pool. A full run on the VPS (4 workers, `--chunk-events 500000`, release build)
produced:

- 176/176 partitions converted, 0 failed; 421,078,937 events; 928 native chunks; 13 GiB.
- Per instrument: 22 partitions each for AUDUSD, EURUSD, GBPUSD, NZDUSD, USDCAD, USDCHF,
  USDJPY and XAUUSD.
- Elapsed ~5.5 minutes wall; ~1,181 s aggregate CPU; peak worker RSS ~356 MiB.
- Re-running the batch verified all 176 partitions as already complete with identical
  `output_hash` values, confirming deterministic restart.
- Independent `historical-verify <dir> <legacy.parquet>` checks (which recompute the logical
  quote stream from the legacy file) passed for sampled partitions including USDCAD 2026-07,
  XAUUSD 2026-09, AUDUSD 2025-01 and EURUSD 2026-10.

Every partition's `quote_stream_sha256` was recomputed during conversion and matched its
trusted manifest. Conversion remains a format operation: it does not establish calendar
completeness, profile approval, source licensing or research eligibility.

## Integrity guarantees

- **Requested-scope binding.** Reusing a completed directory re-verifies the exact requested
  source bytes (SHA-256), the trusted manifest identity (including fidelity, source precision
  and provenance), the import evidence, the profile and the receipt version before returning
  it. A self-consistent existing receipt is never accepted for a different requested source or
  profile.
- **Source-prefix equivalence.** On resume, every adopted chunk is compared tick-by-tick with
  the legacy source rows it claims to represent (timestamp, sequence, bid, ask). A chunk whose
  prices were changed and consistently rehashed is refused. `historical-verify <dir> <source>`
  performs the same full equivalence check.
- **Bounded decoded memory.** Each untrusted Parquet row group is preflighted against an
  explicit decoded-row cap (65,536). Columns are decoded in bounded 8,192-row windows and the
  decoded string-byte budget (256 MiB) is enforced after every window, so peak decoded string
  allocation is bounded by the budget plus one window rather than by the untrusted group size.
  A compressed-file ceiling alone is not treated as a memory bound.
- **Corpus admission.** Chunks are capped at 100,000 events so each monthly partition is
  admissible to `ordered-native-corpus-v1`. Each month remains a distinct single-source
  corpus: per-partition source descriptions differ, so the corpus loader refuses to merge
  months into one unverified source. An aggregate multi-month corpus requires a separate
  factory extension and is not implied by conversion.

## Regression coverage

The permanent tests cover exact conversion, determinism across output directories,
restart-from-committed-chunks, corrupted-chunk refusal, crossed/non-millisecond source
refusal, wrong manifest identity, requested-scope reuse refusal (different source bytes and
different manifest identity), changed-source resume refusal, oversized row-group refusal,
native corpus admission, timestamp ties across chunk boundaries, and the requirement that
distinct months stay distinct single-source corpora.
