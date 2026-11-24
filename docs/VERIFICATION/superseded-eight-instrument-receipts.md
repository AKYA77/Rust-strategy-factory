# Superseded eight-instrument receipts

Status: SUPERSEDED 2026-11-24 by the V2 handover mandate (seven FX instruments, no XAUUSD).

These files are preserved byte-for-byte as immutable historical audit evidence. They describe
the pre-V2 eight-instrument corpus (which included XAUUSD) and are **not** current acceptance
receipts:

- `docs/PROJECT_STATE.json` — eight-symbol campaign state; use the V2 notes below instead.
- `docs/REAL_DATA_MATRIX.json` — eight-instrument real-data matrix.
- `docs/historical-migration/REAL_DATA_STUDY_RECEIPT.json`
- `docs/historical-migration/DIAGNOSTIC_EXECUTION_RECEIPT.json`
- `docs/historical-migration/DIAGNOSTIC_EXECUTION_CACHED_RECEIPT.json`
- `docs/historical-migration/SCIENTIFIC_VALIDATION_RECEIPT.json`
- `docs/historical-migration/SCIENTIFIC_VALIDATION.md`
- `docs/historical-migration/DATA_READINESS.md`
- `docs/historical-migration/studies/XAUUSD.json`
- `docs/historical-migration/diagnostic-cache-v1/XAUUSD.cold.json`
- `docs/historical-migration/diagnostic-cache-v1/XAUUSD.warm.json`
- `docs/BENCHMARKS/*.json` entries whose scope was eight instruments.

## Current V2 evidence

- Scope and deletion: `docs/VERIFICATION/xauusd-removal-2026-11-24.json`.
- Active instrument grammar: `src/contracts.rs::SYMBOLS` (7 symbols).
- Policy decision record: `docs/DECISIONS/0058-v2-seven-instrument-scope.md`.
- Resume point: `docs/NEXT_ACTION.md`.

## Preserved gold-only audit metadata (not deleted)

`/var/lib/hermes-factory/archives/XAUUSD-*-ticks.manifest.json`,
`/var/lib/hermes-factory/archives/XAUUSD-*-ticks.page-sha256`,
`/var/lib/hermes-factory/catalog/XAUUSD-*-import-report.json`,
`/var/lib/hermes-factory/backfill/logs/XAUUSD-*.log`, plus the study/diagnostic-cache JSON above.

No source, licensing, calendar-completeness, broker/session/profile or scientific approval is
granted by this supersession. The retained local corpus remains REAL_STRUCTURALLY_VALID only.
