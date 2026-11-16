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

## Remaining work

1. Run `scripts/check.sh` (fmt, Clippy, full test suite) and record the result.
2. Commit the verified milestone on `hermes/real-data-migration`, push, open a pull request
   and verify CI for the exact commit.
3. Add the real-data integration test path (`tests/historical_migration_real.rs`) if not yet
   present; it must skip cleanly when `/var/lib/hermes-factory` is absent.
4. Full-corpus preparation: enumerate the 176 canonical partitions, convert with bounded
   workers, record per-partition receipts, quality/coverage reports and resource usage.
5. Continue non-overlapping Rust-factory work from `docs/NEXT_ACTION.md`.

## Safety notes

- Do not modify `/var/lib/hermes-factory` source archives, canonical partitions or receipts.
- Do not commit raw historical data, credentials or private research state.
- Conversion does not change scientific qualification status; `REAL_STRUCTURALLY_VALID` is
  the maximum automatic class and is not research eligibility.
