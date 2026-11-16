# Branch progress record — `hermes/real-data-migration`

Provisional rubric. Percentages are estimates, not audited measurements. Unmerged branches
are not counted as integrated into `main`.

## Rubric (weights sum to 100)

| Criterion | Weight | Implementation | Verified end-to-end | Note |
|---|---|---|---|---|
| Core Rust implementation | 20 | ~85% | ~70% | Engine, corpus, campaign/fleet, checkpoint modules exist and are tested; Codex stack #8–#13 unmerged. |
| Data ingestion and provenance | 15 | ~85% | ~75% | Legacy Parquet → native conversion with source binding, receipt identity, restart safety. |
| Multi-instrument historical integration | 15 | ~80% | ~60% | 8/8 instruments converted (176 partitions, 421,078,937 events) and registered; downstream research integration pending. |
| Engine correctness and verification | 20 | ~80% | ~65% | CI green, adversarial/spec reviews, regression tests; full unmerged stack not jointly accepted. |
| Operational usability and recovery | 10 | ~65% | ~50% | Migration/corpus/catalog/audit CLIs; dashboard integration and recovery runbooks incomplete. |
| Performance | 5 | ~75% | ~65% | Measured bounded conversion, windowed decode, receipt-only catalog scan. |
| Scientific/data eligibility and acceptance | 10 | ~25% | ~5% | Four gates implemented, all pending; 0 instruments eligible. |
| Integration and deployment readiness | 5 | ~45% | ~30% | PR #6 open and green; not merged; VPS-only data path. |

## Accounting

- **Overall project completion: ~72%** (implementation-weighted; range 70–74%).
- **Verified end-to-end readiness: ~55–60%** (requires real-data acceptance, merged stack and
  operational runbooks; tracked separately).
- **Progress added this session: ~2–3 percentage points.** The multi-month corpus,
  historical catalog with review gates, bounded streaming audit, review-binding workflow and
  fresh-context reviews were added and verified on real VPS data. The gain is real but modest
  because the large remaining items (scientific eligibility, merged-stack integration,
  operational runbooks) did not move.
- **Remaining: ~28%.**

## Main blockers

1. All four review gates (calendar/profile/legal/scientific) remain pending; no instrument is
   research-eligible. External owner determinations are required and must not be invented.
2. Codex PRs #8–#13 are unmerged; the full stack is not jointly accepted.
3. Downstream research-system integration and operational/recovery runbooks are incomplete.
4. VPS disk pressure (93% used) limits further full-corpus workloads.

## Consistency rule

Percentages rise only when a requirement is implemented **and** verified. Code or tests alone
do not raise them; unmerged branches do not count as integrated.
