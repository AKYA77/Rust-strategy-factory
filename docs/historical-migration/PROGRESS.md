# Branch progress record — `main`

Provisional rubric. Percentages are estimates, not audited measurements. Unmerged branches
are not counted as integrated into `main`.

## Rubric (weights sum to 100)

| Criterion | Weight | Implementation | Verified end-to-end | Note |
|---|---|---|---|---|
| Core Rust implementation | 20 | ~87% | ~72% | Engine, corpus, campaign/fleet, checkpoint modules exist and are tested; Codex stack #8–#13 unmerged. |
| Data ingestion and provenance | 15 | ~90% | ~80% | Legacy Parquet → native conversion with source binding, receipt identity, restart safety; corpus/catalog/audit CLIs. |
| Multi-instrument historical integration | 15 | ~91% | ~74% | 8/8 instruments converted and registered; real corpus → source facts → candles → causal signals verified on genuine data; owner-selected 22-month window explicitly pinned and verified (boundaries/refusals/identity preservation, all 8 instruments). Cached diagnostic full-corpus execution verified end-to-end (PR #28). Replay/execution remains synthetic literal-USD scoped. |
| Engine correctness and verification | 20 | ~82% | ~68% | CI green, adversarial review, regression tests; full unmerged stack not jointly accepted. |
| Operational usability and recovery | 10 | ~75% | ~60% | Migration/corpus/catalog/audit CLIs and real-data tests; dashboard historical views and live VPS recovery incomplete. `--operations` serve configuration with import/coverage activation lifecycle implemented and tested (PR #24). |
| Performance | 5 | ~78% | ~68% | Measured bounded conversion, windowed decode, receipt-only catalog scan, 12-min full-corpus audit. |
| Scientific/data eligibility and acceptance | 10 | ~28% | ~8% | Four gates implemented, all pending; 0 instruments eligible; declarations and machine evidence only. |
| Integration and deployment readiness | 5 | ~50% | ~35% | PR #6 open and green; not merged; VPS-only data path; no systemd/reboot acceptance. |

## Accounting

- **Software implementation completeness (Metric A): ~82%.** (supersedes the ~81% estimate
  recorded after PR #22/24 integration; PR #28's cached diagnostic full-corpus execution adds
  the multi-instrument historical-integration verification row; PR #24's `--operations`
  dashboard panels move operational usability toward 75% implementation / 60% verified.)
- **Verified end-to-end completion (Metric B): ~69%.** (supersedes ~68%; cached diagnostic
  full-corpus execution over the 22-month pin is now verified end-to-end for all eight
  instruments.)
- **Cached diagnostic full-corpus execution (card t_9623c913 / PR #28):** verified end-to-end
  for all eight instruments over the owner-selected 22-month pin (2025-01..2026-10) through
  the unchanged authoritative sequential engine: 110 executed months, 66 refused months,
  11,584 trades, 694,984 rejections, P&L -1,759,377,176 uUSD, worst-month drawdown 47,783,390
  uUSD; cache ratio cold 0 hits / 110+66 misses then warm 110 hits / 66 misses; every
  instrument's cold and warm reports byte-identical apart from cache counters. Evidence:
  `scripts/run-diagnostic-execution-cached.py`, configs
  `configs/historical-migration/{strategies,costs}/*.json`,
  `docs/historical-migration/diagnostic-cache-v1/`, aggregate receipt
  `docs/historical-migration/DIAGNOSTIC_EXECUTION_CACHED_RECEIPT.json` (sha256
  02be66677cd4b719f3e6e5bf34e4d31560dc0e41b92f473cc08e326964ae0ca0).
- **Progress added this milestone (PR #22 + PR #24 + PR #28): ~+1.0 weighted point each metric.**
  The owner-selected 22-month research window (2025-01..2026-10) is now an explicit, tested
  calendar pin rather than a count-coincidence default: exact boundaries for all eight
  instruments, exact out-of-range refusals, and identity preservation versus exact-count
  registration are verified on the real corpus (+1 implementation / +1 verified point on the
  15%-weighted historical-integration row; unchanged at whole-percent granularity). The
  `--operations` dashboard activation panels add bounded import/coverage activation, registry
  integrity audit, and CPU-tick resource charts (+0.5 implementation / +0.5 verified on the
  10%-weighted operational row). The cached diagnostic full-corpus execution adds end-to-end
  verification of the 22-month research window over the real corpus (+0.5 verified on the
  historical-integration row). The gain is bounded because this closes configuration and
  operational requirements, not new execution capability: real/FX execution, external approvals
  and full-corpus diagnostic execution over the pin did not move.
- **Remaining: ~33% software, ~26% verified. Target gap to 95: 13 points software, 26 points
  verified.**
- Baseline used: external provisional estimates (software 74%, verified 60%). The rubric and
  weights are unchanged.

## Main blockers

1. All four review gates (calendar/profile/legal/scientific) remain pending; no instrument is
   research-eligible. External owner determinations are required and must not be invented.
2. Open PRs not yet integrated: #23 (cargo-target-isolation, t_f368fccb) and #27
   (scheduler-resource-controls, t_794f2a65). Both are CI-green; merge after QA review.
3. Real/FX execution and full-history qualification are unfinished; corpus replay is
   deliberately scoped to synthetic literal-USD inputs.
4. Live systemd/reboot VPS acceptance and operational runbooks are incomplete.
5. VPS disk pressure (95% used, ~5 GiB free) limits further full-corpus allocations.

## Consistency rule

Percentages rise only when a requirement is implemented **and** verified. Code or tests alone
do not raise them; unmerged branches do not count as integration.
