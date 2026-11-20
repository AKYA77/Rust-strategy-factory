# Branch progress record — `hermes/real-data-migration`

Provisional rubric. Percentages are estimates, not audited measurements. Unmerged branches
are not counted as integrated into `main`.

## Rubric (weights sum to 100)

| Criterion | Weight | Implementation | Verified end-to-end | Note |
|---|---|---|---|---|
| Core Rust implementation | 20 | ~87% | ~72% | Engine, corpus, campaign/fleet, checkpoint modules exist and are tested; Codex stack #8–#13 unmerged. |
| Data ingestion and provenance | 15 | ~90% | ~80% | Legacy Parquet → native conversion with source binding, receipt identity, restart safety; corpus/catalog/audit CLIs. |
| Multi-instrument historical integration | 15 | ~89% | ~71% | 8/8 instruments converted and registered; real corpus → source facts → candles → causal signals verified on genuine data; owner-selected 22-month window explicitly pinned and verified (boundaries/refusals/identity preservation, all 8 instruments). Replay/execution remains synthetic literal-USD scoped. |
| Engine correctness and verification | 20 | ~82% | ~68% | CI green, adversarial review, regression tests; full unmerged stack not jointly accepted. |
| Operational usability and recovery | 10 | ~70% | ~55% | Migration/corpus/catalog/audit CLIs and real-data tests; dashboard historical views and live VPS recovery incomplete. |
| Performance | 5 | ~78% | ~68% | Measured bounded conversion, windowed decode, receipt-only catalog scan, 12-min full-corpus audit. |
| Scientific/data eligibility and acceptance | 10 | ~28% | ~8% | Four gates implemented, all pending; 0 instruments eligible; declarations and machine evidence only. |
| Integration and deployment readiness | 5 | ~50% | ~35% | PR #6 open and green; not merged; VPS-only data path; no systemd/reboot acceptance. |

## Accounting

- **Software implementation completeness (Metric A): ~76%.**
- **Verified end-to-end completion (Metric B): ~63%.**
- **Progress added this milestone: +0.15 weighted points (each metric).** The owner-selected
  22-month research window (2025-01..2026-10) is now an explicit, tested calendar pin rather than
  a count-coincidence default: exact boundaries for all eight instruments, exact out-of-range
  refusals, and identity preservation versus exact-count registration are verified on the real
  corpus (+1 implementation / +1 verified point on the 15%-weighted historical-integration row;
  unchanged at whole-percent granularity). The gain is bounded because this closes a
  configuration requirement, not a new execution capability: real/FX execution, external
  approvals and full-corpus diagnostic execution over the pin (next card) did not move. Prior
  milestone (+3 points): genuine VPS-data structural pipeline for all eight instruments,
  deterministic per-instrument study reports, and `historical-corpus-verify-bound` source-byte
  equivalence for all 421,078,937 events.
- **Remaining: ~38% software, ~38% verified. Target gap to 95%: 19 points software, 33 points
  verified.**
- Baseline used: external provisional estimates (software 74%, verified 60%). The rubric and
  weights are unchanged.

## Main blockers

1. All four review gates (calendar/profile/legal/scientific) remain pending; no instrument is
   research-eligible. External owner determinations are required and must not be invented.
2. Codex PRs #8–#13 are unmerged; the full stack is not jointly accepted.
3. Real/FX execution and full-history qualification are unfinished; corpus replay is
   deliberately scoped to synthetic literal-USD inputs.
4. Live systemd/reboot VPS acceptance and operational runbooks are incomplete.
5. VPS disk pressure (95% used, ~5 GiB free) limits further full-corpus allocations.

## Consistency rule

Percentages rise only when a requirement is implemented **and** verified. Code or tests alone
do not raise them; unmerged branches do not count as integrated.
