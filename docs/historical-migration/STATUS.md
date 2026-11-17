# Historical migration status — branch `hermes/real-data-migration`

Checkpoint for the next Hermes conversation. This file is branch-specific and is not a
shared progress file.

## Recovered state

- Repository: `/root/workspace/Rust-strategy-factory`, branch `hermes/real-data-migration`.
- Started from `main` at `e16063713bd4c9f7891169bba11cf3df4eb78b3d`.
- The previous conversation's only surviving local change was the `sha2 = "=0.11.1"` pin in
  `Cargo.toml`/`Cargo.lock`. No converter code had been written.
- Remote `main` was still at `e16063713bd4`; Codex branch `codex/long-campaign-ledger` at
  `12698e4` was left untouched.

## Implemented

- `src/historical_migration.rs`: bounded legacy-Parquet → native `.rsf` chunk converter.
  - Exact ten-column `data/v2` schema check; canonical 24-field manifest parsing.
  - Parquet SHA-256 and reproduced `quote_stream_sha256` verification.
  - Exact nanosecond→millisecond and fixed-point scale conversion; strict sequence assignment.
  - Atomic no-replace chunk publication, deterministic receipts, restart from committed chunks.
  - Refusal of crossed/nonpositive/quality-flagged/mixed-source/non-monotonic/non-millisecond
    rows, wrong manifest identity, corrupted chunks and partial output.
- `src/lib.rs`: module registration.
- `src/main.rs`: `historical-convert` and `historical-verify` CLI commands.
- `configs/historical-migration/profiles/*.json`: eight explicit migration profiles
  (source scale 1,000,000; tick = observed quantum; not-approved provenance).
- `docs/RUNBOOKS/historical-migration.md`: runbook with rules, output layout and results.
- Unit tests: `cargo test --lib historical_migration` → 7 passed.

## Verified results (real VPS data)

| Sample | Events | Chunks | Quote stream SHA-256 | Output hash |
| --- | --- | --- | --- | --- |
| NZDUSD 2025-12 | 876,772 | 2 | `73063fe49795367da85c2344d03cc7ff96a1cab9892321c5c390f985e965100b` | `3473fee1de619f65d0384689321f944a1400efe743ba92c68e06aedad5dc34e8` |
| USDCAD 2026-07 | 1,226,059 | 3 | `e7605ea2d554b13ddda2bdb1a02be07df2fce3b41dd0f21f2d6fa31f7b5aded4` | `3043d9ace63d8801ffff94b11635cc0f9df1cfa94d14f22e8ed9982302b29b2a` |

Independent Python readback parsed the native chunks and compared every tick against the
legacy Parquet: exact match for both samples; strictly increasing sequences; non-decreasing
timestamps; positive, non-crossed quotes. USDCAD retained exactly 1,226,059 rows and
preserved the 353-quote quarantine evidence.

Canonical corpus independently established: 176 partitions, 421,078,937 rows, all monotonic,
no crossed quotes, no quality flags. Seven noncanonical records excluded.

## Resource measurements

Release build (`cargo build --release --locked`), `--chunk-events 500000`:

| Sample | Rows | Wall | User | System | Peak RSS | Output |
| --- | --- | --- | --- | --- | --- | --- |
| NZDUSD 2025-12 | 876,772 | 2.32 s | 1.68 s | 0.60 s | 80,052 KiB | 27 MiB |
| USDCAD 2026-07 | 1,226,059 | 2.85 s | 2.13 s | 0.69 s | 89,752 KiB | 38 MiB |

Both release receipts reproduce the debug-build `output_hash` values exactly, confirming the
conversion is deterministic across build profiles. Debug-build USDCAD took ~30 s for the same
work, so release timings are the realistic planning numbers. Peak RSS is dominated by the
bounded chunk buffer plus Parquet row-group decode, not by the month size.

## Full corpus milestone

A full canonical batch completed on the VPS (4 workers, `--chunk-events 500000`, release):

- 176/176 partitions converted, 0 failed; 421,078,937 events; 928 chunks; 13 GiB output.
- Elapsed ~5.5 minutes wall; ~1,181 s aggregate CPU; peak worker RSS ~356 MiB.
- Re-run verified all 176 as already complete with identical `output_hash` values.
- `historical-verify` with the legacy source passed for sampled partitions (USDCAD 2026-07,
  XAUUSD 2026-09, AUDUSD 2025-01, EURUSD 2026-10).
- Output: `/root/workspace/historical-migration-output/full` with `batch-report.json`.

### Corpus-admissible re-run (100,000-event chunks)

After aligning the chunk cap, the full corpus was re-converted into
`/root/workspace/historical-migration-output/full-100k`:

- 176/176 partitions converted, 0 failed; 421,078,937 events; 4,299 chunks; max chunk
  100,000 events; 13 GiB.
- Sampled `historical-verify <dir> <legacy.parquet>` (full tick equivalence) passed for
  NZDUSD 2025-12, USDCAD 2026-07 and XAUUSD 2026-09.
- `corpus-create-bounded` + `corpus-inspect` on NZDUSD 2025-12 produced a loadable
  `ordered-native-corpus-v1` specification (9 chunks), confirming genuine native corpus
  admission.
- The earlier 500,000-event output under `full/` is superseded and inadmissible; it is retained
  until an independent rollback copy is verified.

## Integrity-fix milestone (Codex review response)

Codex's `codex/review-pr6-data-integrity` review found two reproduced P1 defects; both are
fixed with permanent regressions:

- **Requested-scope binding** — completed-output reuse now re-verifies the requested source
  bytes, trusted manifest identity, profile and receipt version. A self-consistent receipt is
  no longer accepted for a different source/profile.
- **Source-prefix equivalence** — resumed conversions compare every adopted chunk tick-by-tick
  with the legacy source prefix; changed-but-consistently-rehashed prices are refused.
  `historical-verify <dir> <source>` performs the same full equivalence check.
- **Bounded decoded memory** — row groups are preflighted against a 65,536-row cap and a
  256 MiB decoded string-byte budget before allocation.
- **Corpus admission** — chunk size aligned to the native `ordered-native-corpus-v1` cap of
  100,000; each month remains a distinct single-source corpus and the loader refuses to merge
  distinct monthly sources.
- **Test stability** — the two campaign/fleet rotation tests now use the wait-capable loop
  drivers so scheduler-idle retry backoffs are waited out instead of being treated as a
  terminal stop. Assertions are unchanged.

New permanent tests: 16 in `historical_migration` (including Codex's two reproducers, which
failed before the fix and pass after), plus corpus-admission, timestamp-tie and
distinct-month tests. Real-corpus integration passes on the VPS with 100,000-event chunks.

### Independent adversarial review outcome

A fresh-context adversarial reviewer ran 13 probes against the fixed code and found **no
bypass** of the two required attack classes (requested-scope binding; source-prefix
equivalence), and confirmed the row/string bounds and corpus-admission behaviour. It raised
four secondary findings; three were acted on in `6fbfd0a`:

1. String budget was enforced only after full-group decode → columns are now read in
   8,192-row windows with the budget enforced after each window (peak ≤ budget + one window).
2. `refuses_oversized_row_group` was not a true regression test (fake manifest failed for the
   wrong reason) → it now builds a correct manifest with a cap-bypassing test-only scan and
   asserts the `decoded-row budget` error.
3. Reuse bound only a subset of manifest identity → it now also binds fidelity,
   `source_precision`, provenance and `import_evidence`.
4. Low/defense-in-depth (accepted, documented): reusing a completed receipt does not re-read
   chunk contents against the source; doing so requires forging `manifest.json`, the trust
   anchor. `historical-verify <dir> <source>` remains the explicit full-equivalence check.

The module now has 17 tests; `cargo clippy --locked --all-targets -- -D warnings` is clean and
the real-corpus integration test passes with the windowed reader.

### Specification review outcome

A second fresh-context reviewer checked the stated contract invariants and found the corpus
rules unweakened and deterministic restart intact, plus further gaps (commit `eb98c68`):

- Source-prefix equivalence now checks price exactness (`% factor`) and quality flags, and
  `verify_output` requires `receipt.events == source rows`.
- Adopted chunks are re-checked against the 100,000-event corpus cap in both `rebuild_state`
  and `verify_output`; `verify_output` also checks each chunk's `archive_hash`.
- The manifest instrument is bound to `profile.symbol` on both fresh conversion and reuse.
- The receipt stores an explicit `source_description`, so no-manifest conversions are now
  verifiable (previously `verify_output` reconstructed a mismatched description).
- Import evidence is shape-validated (64-hex content hash, object/null correction records).
- Removed the dead `Config.legacy_manifest_path` field.

The module now has 20 tests. Known accepted residual: reuse of a completed receipt does not
re-read chunk contents against the source (requires forging the receipt trust anchor);
`historical-verify <dir> <source>` is the explicit full-equivalence check.

Measured memory bound for the windowed decode: a malicious 65,536-row group with 5,000-byte
`source_id` strings (358 KB compressed Parquet) is refused with
`legacy column 5 exceeds the decoded string byte budget` at **~300 MiB peak RSS**
(budget 256 MiB + one 8,192-row window). The same class of input peaked at ~1.24 GiB before
the windowed fix.

## Multi-month corpus and catalog milestone

- `src/historical_corpus.rs`: explicit aggregate specification over the 22 independently
  verified monthly native corpora. It preserves per-month source identities and provenance,
  refuses missing/overlapping/ambiguous/non-contiguous months, excludes the seven noncanonical
  partitions via the canonical plan, and traverses months in order with explicit month
  boundaries and per-month sequence spaces. No chunk data is copied. Deterministic identity;
  CLI `historical-corpus-register` / `historical-corpus-verify`.
- `src/historical_catalog.rs`: structural status per instrument and month (verified, missing,
  noncanonical, load-failure), source identities, coverage, timestamp ties, gaps and legacy
  correction/quarantine counts. Gates are calendar/profile/legal/scientific, all pending;
  eligibility requires structural validity plus all gates accepted. CLI
  `historical-catalog-scan` / `historical-catalog-verify`. Added
  `historical_migration::verify_receipt_only` so catalog scans do not re-read every chunk.
- Real VPS results: all eight instruments register 22/22 months; total 421,078,937 rows;
  catalog identity `c7ca19266439680d28f2a81ba01f1eb8407955dde26839b22f451a05959b1339`;
  8/8 structurally valid, 0 eligible (all gates pending). Legacy corrections and quarantine
  are surfaced (e.g. USDCAD 2026-07: 353 quarantined quotes; XAUUSD: 103,621 reordered rows).
- End-to-end real test `tests/historical_corpus_real.rs`: registers EURUSD, verifies receipt
  equivalence, scans the catalog, checks cross-month ordering, and assembles causal M1 candles
  while asserting per-month sequence restart and per-month source identity.

## Bounded audit and v2 review gates

- `historical_corpus::audit` / CLI `historical-corpus-audit` streams every chunk of one
  instrument with bounded memory and verifies source attribution, per-month sequence
  continuity from zero, cross-month timestamp ordering, month and total row counts, and
  causal M1 candle generation. Full-corpus result: 8 instruments, **421,078,937 rows**,
  **4,299 chunks**, **5,413,197 M1 candles**, all exit 0; ~12 min total wall, chunk-by-chunk.
- Catalog **v2** gives each gate explicit `required_evidence`, an operator-visible
  `explanation`, and — on acceptance — a `dataset_identity` and review `receipt` binding.
  `bind_review` refuses a report whose catalog identity does not match; `bind_assertions`
  records explicit calendar/profile/legal/scientific decisions with evidence. A pending or
  rejected gate blocks eligibility; 0 instruments are eligible.
- `docs/historical-migration/DATA_READINESS.md` records verified facts (coverage, source
  metadata `HistData.com`/millisecond/scale 1e6, Saturday-absent Sunday-open session
  observation, corrections and quarantine) and the unknowns that require owner/external
  determination (exact session/holiday calendar, licence, broker specification, scientific
  approval). `docs/historical-migration/PROGRESS.md` records the completion rubric.
- Fresh-context review fixes: `bind_review` now verifies the review report (`Report::verified`:
  record hash, version, evidence hashes) before trusting any decision, and `validate_months`
  rejects out-of-range calendar months that could alias into the next year. Both have
  permanent regression tests. The `tests/corpus` campaign-owned supervision test was hardened
  against scheduler-idle timing with an unchanged `exhausted` assertion.

## Full source-bound verification (all eight instruments)

`historical-corpus-verify-bound` was run for every instrument: each month's native chunks
are compared tick-for-tick against the canonical legacy Parquet prefix, not just their own
receipt. All 8/8 passed (`bound=0`): EURUSD 137 s, GBPUSD 135 s, USDJPY 190 s, USDCHF 100 s,
USDCAD 124 s, AUDUSD 101 s, NZDUSD 79 s, XAUUSD 461 s (~22 min total, 421,078,937 events).
Combined with the bounded audit and the real-data structural pipeline, the converted corpus
is verified for source equivalence, ordering, row counts, candles and causal signals.

## Adversarial review of the corpus/catalog (this session)

A fresh-context adversarial review (empirical, throwaway harness against the real corpus)
found and we fixed:

- **High:** catalog scan marked a foreign/self-consistent receipt as `Verified`; it now binds
  each receipt to the canonical partition identity (dataset/source/parquet path/rows/
  instrument) and reports a mismatch as a load failure. The catalog path also validates plan
  month ranges, duplicates and contiguity.
- **High:** `register`/`verify` did not bind monthly output to the canonical source bytes.
  `verify` now binds month instrument, parquet hash and parquet path and refuses a chunk
  inventory that differs from the receipt; `verify_bound` (CLI `historical-corpus-verify-bound`)
  performs full source-byte equivalence per month.
- **Medium/low:** eligibility also requires no rejected review; `bind_assertions` binds the
  catalog identity; `bind_review` verifies the catalog and the report `catalog_hash`; gate
  transitions clear stale binding metadata; receipt reads are size-capped; correction counts
  saturate; source attribution is exact field binding; traversal and catalog verification were
  tightened. Regression tests cover the foreign-receipt, truncated-inventory, forged-report,
  wrong-identity, wrong-catalog-hash and rejected-review cases.

Residual accepted: `register`/`verify`/`audit` trust the per-month receipt as the trust
anchor; `verify_bound` and `historical-verify <dir> <source>` are the explicit source-byte
equivalence checks. `audit` does not re-read the Parquet.

## Integration branch `hermes/integration-codex-stack`

- Created from the Codex stack tip `codex/retained-preflight-inventory` @ `74790a4` and merged
  with `hermes/real-data-migration` (merge commit `cbcfb62`, clean auto-merge, no conflicts).
- Combined verification: `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets
  -D warnings`, full `scripts/check.sh` (exit 0, all Rust tests including the real-data
  pipeline/study), release build, `check-data-compute-identity.py`,
  `check-operations.py`, `check-full-horizon.py` — all exit 0.
- File overlap between the two lines was only `tests/corpus.rs`; the Hermes `src/` modules are
  additive and disjoint from the Codex engine/corpus/campaign/dashboard files.
- This is the cumulative integration candidate for Codex PRs #8–#13 plus the Hermes
  historical-data work. Not merged to `main`; hosted CI must pass on the exact pushed SHA.

## Remaining work

1. ~~Run `scripts/check.sh`~~ — fmt and Clippy pass; lib tests pass; full suite has two
   load-dependent `tests/corpus` timing flakes that pass in isolation.
2. ~~Commit, push, open a pull request~~ — PR #6, CI green for `86c172c`.
3. ~~Add the real-data integration test~~ — `tests/historical_migration_real.rs` skips cleanly
   without the legacy catalog.
4. ~~Full-corpus preparation~~ — 176/176 converted and verified; batch report published.
5. ~~Fix the two confirmed converter integrity defects~~ — regressions added, both pass.
6. ~~Bounded decoded memory + native corpus admission alignment~~ — implemented and tested.
7. ~~Investigate the two campaign-rotation test failures~~ — wait-capable drivers; assertions
   unchanged.
8. ~~Re-run the full 176-partition migration with the 100,000-event chunk cap~~ — done in
   `full-100k`; sampled source-bound verification passed (NZDUSD, USDCAD, USDJPY ×2,
   XAUUSD ×2).
9. ~~Multi-month corpus model + catalog/review status~~ — `historical_corpus` and
   `historical_catalog`; all eight instruments register 22/22 months, 0 eligible.
10. Continue non-overlapping Rust-factory work from `docs/NEXT_ACTION.md`.
11. Optional: independent scientific/qualification review of the migrated corpus (Codex), and
    explicit calendar/profile/licensing acceptance before any research use.

## Safety notes

- Do not modify `/var/lib/hermes-factory` source archives, canonical partitions or receipts.
- Do not commit raw historical data, credentials or private research state.
- Conversion does not change scientific qualification status; `REAL_STRUCTURALLY_VALID` is
  the maximum automatic class and is not research eligibility.
