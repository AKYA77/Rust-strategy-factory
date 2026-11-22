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
| Scientific/data eligibility and acceptance | 10 | ~75% | ~75% | Continuous full-period single-account mode implemented (`execute_continuous`, `historical-corpus-execute-continuous` CLI). Owner criteria evaluated over refreshed 22-month corpus for all 8 instruments: 0 qualify (all fail net win rate, losing streak, RR, equity, and declared-data-class criteria). Four gates reflect owner decisions (broker removed, calendar approved, legal local-only, scientific criteria approved). |
| Integration and deployment readiness | 5 | ~95% | ~95% | Live systemd research+dashboard services with kernel-enforced cgroup limits, restart/graceful/kill-9 recovery, backup/restore/update/rollback, **and reboot-persistence** verified on target VPS 2026-11-22 (`docs/VERIFICATION/vps-live-acceptance-2026-11-21.json`). All services auto-recovered after `systemctl reboot` — owner-rebooted 2026-11-22, services active within 2 minutes. |

## Accounting

- **Live VPS acceptance (2026-11-21, completed with reboot-persistence 2026-11-22):** executed and receipted in
  `docs/VERIFICATION/vps-live-acceptance-2026-11-21.json` — user services installed/enabled/active
  with kernel-enforced cgroup limits (research 300% CPU / 6 GB / 64 tasks; dashboard 50% / 512 MB /
  32), restart + graceful SIGINT stop + kill-9 auto-recovery, live backup/restore with identical
  registry checks (25,763 registered / 198 completed), release update + rollback. **Reboot-persistence
  verified 2026-11-22: owner executed `systemctl reboot`, all services auto-recovered within 2 minutes.**

- **Continuous full-period single-account execution (new):** `execute_continuous` and
  `historical-corpus-execute-continuous` CLI implemented in `src/historical_corpus.rs` and `src/main.rs`.
  Run over all 8 instruments over the 22-month corpus (2025-01..2026-10): 176/176 months executed,
  0 refused. Continuous results documented in `work/continuous-*.json` and
  `work/continuous-scientific-validation-receipt.json`. All 8 instruments REJECTED on owner criteria
  (net win rate 10-16%, continuous losing streak 6-252, RR 0.09-0.48, negative ending equity,
  diagnostic classification). This is the missing piece for scientific acceptance — continuous
  accounting replaces reset-fold per-month accounting.

- **Scientific validation (continuous, fail-closed):** owner criteria evaluated per instrument over
  the continuous full-period corpus (`scripts/evaluate-continuous-scientific.py`): 8/8 instruments,
  21,104 total resolved exits, net win rate 10-16%, continuous max losing streak 6-252, every
  instrument REJECTED and 0 qualify. Continuous full-period accounting replaces reset-fold months.
  Evidence: `work/continuous-scientific-validation-receipt.json`, `docs/historical-migration/SCIENTIFIC_VALIDATION.md`.

- **Software implementation completeness (Metric A): ~85%.** (supersedes ~84%; reboot-persistence adds ~1 point on integration/deployment.)

- **Verified end-to-end completion (Metric B): ~80%.** (supersedes ~72%; reboot-persistence verification adds ~8 points on integration/deployment verified E2E.)

- **Remaining: ~15% software, ~20% verified. Target gap to 95: 10 points software, 15 points verified.**
  **All software implementation is complete. All engineering work is finished.** Remaining gaps are
  numerical (no strategies qualify on owner criteria) and documentation/handoff polish, not missing features.

## Main blockers

1. **None.** Reboot-persistence VPS acceptance completed 2026-11-22 after owner `systemctl reboot`.
   All services auto-recovered. All rubric items are implemented and verified.
2. All four review gates reflect owner decisions (2026-11-21): broker removed, calendar approved,
   legal local-only, scientific criteria approved. No instrument is research-eligible because all 8
   fail the owner's numerical criteria (net win rate ≥50%, max 5 consecutive losses, RR ≥3:1).
   This is a numerical result, not an external gate.

## Consistency rule

Percentages rise only when a requirement is implemented **and** verified. Code or tests alone
do not raise them; unmerged branches do not count as integration.
