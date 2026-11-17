# Next action

Status: SOFTWARE_INCOMPLETE. Use the existing checkout. Read AGENTS.md, PROJECT_STATE.json, IMPLEMENTATION_QUEUE.md and relevant MASTER_BLUEPRINT.md requirements. After every verified milestone update evidence/state/this file, commit, push, check remote/hosted CI, reread this file from disk and continue.

## Latest verified local milestone

The integration branch `hermes/integration-codex-stack` combined the Codex stack tip
(`codex/retained-preflight-inventory` @ `74790a4`, PRs #8–#13), the independent
`codex/long-campaign-ledger` (PR #1) and the Hermes historical Parquet→native migration (PR #6),
assembled on top of `main` @ `e160637`. All four lines merged cleanly with no conflicts and every
PR head commit is an ancestor of the combined branch. The combined branch passed `cargo build`,
`cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, the full
`scripts/check.sh` (511 Rust tests passed, 0 failed, including the real-data pipeline/study
tests), the release build, `check-data-compute-identity.py`, `check-operations.py` and
`check-full-horizon.py`; hosted CI was green on the pushed SHA. PR #14 then merged the branch
into `main` as `3d2d6f2`, and the 13 superseded PRs were closed with their history preserved.
Three load-induced test flakes were diagnosed and fixed without weakening any assertion: the
fleet/campaign subprocess tests now serialize on a process-local lock, the
`fleet_loop_ready_retry` fixture backoff (6000 ms) is larger than the fixture-to-child gap, and
the parent supervisor's own bookkeeping writes retry transient SQLite lock contention instead of
aborting a healthy child. Software remains incomplete.

## Implement next

The highest-priority blockers are external: real/protected/non-USD execution, approved source
acquisition, broker/calendar/conversion facts and target-VPS acceptance all remain pending (source
access still returns 403 and no legitimate real quote window has been obtained). Continue the
remaining software queue — full end-to-end acceptance with hardware/resource evidence, writer
backpressure, supervised crash/disk-full injection and aggregate audit coverage — without weakening
financial, provenance, scientific or security safeguards. The converted native corpus is
`REAL_STRUCTURALLY_VALID` only and grants no calendar completeness, profile approval, source
licensing, scientific eligibility or qualification.


Current streaming boundary: one authoritative AccountState transition serves materialized and synthetic USD corpus replay. Account checkpoints are hash-chained/lease-fenced; saved prefixes and source artifacts are revalidated before advancement. Bounded parallel corpus rounds now serve both supplied batches and a durable generated queue/coordinator. The materialized Driver remains separate. Real/protected/non-USD execution, fast-seek resume and full-history qualification remain refused/unfinished. See DECISIONS/0010-durable-corpus-backlog.md for implemented boundaries and remaining acceptance.


Keep indexed policy experimental. Profile dominant complete-pipeline stages and validate varied/representative workloads before any promotion; engine-only gains or a small noisy median are insufficient. Implement checkpoint-aware shared-index/batch lifecycle, aggregate reference-audit coverage monitoring; metadata lookup scaling now has component evidence. Continue explicit source revision/correction links, catalog approval/coverage lifecycle, import/backup activation dashboard workflows and coordinated multi-component recovery; bounded immutable inventory and continuous export now exist. Preserve declared class versus independent approval and source availability. Continue remaining lifecycle, and the remaining scientific/operations queue rather than treating the performance experiment as software completion. Preserve exact eligibility/fallback, source scope, immutable evidence and reference/proof audit independence.

Continue the full queue: indexed blocks/direct arrays/streaming and justified codecs, resumable sources/imports/coverage, dependence-aware prespecified qualification/evidence gates and real diverse boards, bounded multiworkers/supervision/resources, remaining real dashboard/operations/security workflows and integrated acceptance. Use IMPLEMENTATION_QUEUE.md's source/test reconciliation; do not confuse implemented code with acceptance.

## External acceptance

Fresh Dukascopy/HistData info requests returned403; no real quotes/terms obtained. Proposed source domains could not be saved because the original setup draft is not editable; changes are not applied. Preserve existing settings when transferring the additive proposal. Do not retry without a meaningful access change. Small legally usable2025/2026 samples, approved broker/calendar/conversion facts and representative/physical-cold measurements remain pending; full backfill and target-VPS authorization/operation are separate later steps. Do not touch Hermes or change public repository visibility.

## Invariants

Exact executable bid/ask,2-pip spread floor plus separate2-pip adverse market/stop slippage,10% intended risk/$20 initial/3R/24h; frozen pending distance/quantity; no fabricated gaps/fills/depth; no protected OOS/final adaptation; synthetic never qualifies. Qualification needs independent approved complete evidence, not a high test count or tiny benchmark. Registry-local protection cannot cover external tools/copied registries. No execution continues after a session ends unless an actual supported mechanism exists.

Latest continuation: persisted machine bindings and scoped independent authority are implemented; no genuine quotes or broker approvals exist. Alternative source findings and exact blockers are in REAL_DATA_MATRIX.json. Zero acquired bytes; no accounts/emails/purchases. Full historical qualification and target-VPS acceptance remain pending.
