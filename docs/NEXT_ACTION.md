# Next action

Status: SOFTWARE_INCOMPLETE. Use the existing checkout. Read AGENTS.md, PROJECT_STATE.json, IMPLEMENTATION_QUEUE.md and relevant MASTER_BLUEPRINT.md requirements. After every verified milestone update evidence/state/this file, commit, push, check remote/hosted CI, reread this file from disk and continue.

## Latest verified local milestone

Verification throughput landed through PR #21 (`hermes/verification-throughput`):
quality-neutral dev/test optimization profiles (deps opt2 cached once, test code opt1,
line-tables-only debug, workspace build script opt3) and a three-job parallel hosted CI
(rust-checks / browser / release-acceptance) with per-job rust-cache, CARGO_INCREMENTAL=0
and CI debug=0. Measured: full suite 3986.68s -> 440.25s (9.06x) with exact 54/518/148
parity verified twice; real-data suites 9.1x/7.1x/10.2x faster; hosted CI 27.75min
single-job -> 15.37min cold parallel. mold was evaluated and SKIPped: a clean
concurrency-guarded link-only A/B shows the rustc-1.100 default rust-lld already matches
mold (6.00s vs 6.55s for 54 executables); the first-pass 12.3x figure was
contamination-inflated and is documented as a measurement error. Receipts:
`docs/VERIFICATION/verification-throughput-v1.json`; runbook
`docs/RUNBOOKS/verification-throughput.md` records all SKIP decisions
(mold/nextest/sccache/Cranelift/consolidation) and the CI-first pre-push gate. The
Cargo.toml edit is an honest compute-identity build-metadata change (3b7b85d9... ->
47d1658d...). No gate, test or financial contract changed; no percentage claim from
throughput alone.

`main` integrates PR #19 (`hermes/integration-codex-stack` @ `cd5ec86`, configurable
completed-month research window) and PR #20 (`hermes/historical-execution-cache` @ `2603073`,
resumable self-labelling diagnostic execution). PR #20 merged through GitHub as `f7e1cd1`; PR
#19's unique commit was integrated by a normal merge of `origin/hermes/integration-codex-stack`
into `main`. Only the two state docs conflicted, and both were resolved by keeping BOTH
features' entries; `src/historical_corpus.rs` and `src/main.rs` auto-merged with the window
registration and the execution cache both present. Both PR heads are ancestors of `main`. The
merged tree passes `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -- -D
warnings`, `cargo test --locked --lib --bins --tests` (518 Rust tests passed, 0 failed) and
`scripts/check.sh`. Hosted CI on the exact merge SHA `65daf72` concluded green (run
`44577035194`, success) and is recorded in `docs/PROJECT_STATE.json`. The research window is
configurable and refuses a
24-month window with the exact shortfall (2026-11 and 2026-12 are not completed/available);
diagnostic execution resumes identical months from a content-addressed, self-hashed cache and
every month record stays `diagnostic_simulation`/`none`. Neither feature grants eligibility,
approval or qualification.

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

## Cached diagnostic full-corpus execution (card t_9623c913) — COMPLETE

- Deliverable: `scripts/run-diagnostic-execution-cached.py` + configs
  `configs/historical-migration/strategies/m1-long-above-mean-v1.json` and
  `configs/historical-migration/costs/bid-anchored-floor-v1-default.json`;
  per-instrument reports in `docs/historical-migration/diagnostic-cache-v1/`;
  aggregate `docs/historical-migration/DIAGNOSTIC_EXECUTION_CACHED_RECEIPT.json`
  (sha256 `02be66677cd4b719f3e6e5bf34e4d31560dc0e41b92f473cc08e326964ae0ca0`).
- What was verified:
  - 8 instruments x 22 months (2025-01..2026-10): 110 executed months, 66 refused months
    (USDJPY/USDCHF/USDCAD refused on the literal-USD gate; not a supported class).
  - Cold pass: 0 cache hits, 110+66 misses; warm pass: 110 hits, 66 misses; all 110 executed
    months served from cache on the second pass.
  - Every instrument cold == warm byte-identical apart from cache counters (no financial drift
    from resume).
  - Totals match the earlier non-cached full-corpus receipt exactly: 11,584 trades,
    694,984 rejections, P&L -1,759,377,176 µUSD, worst month drawdown 47,783,390 µUSD.
  - Full corpus: 26.4 min cold -> 10.0 min warm (3.2x with 6 cores), with per-instrument wall
    times recorded in the receipt; the warm pass serves all 110 executed months from the
    content-addressed cache.
- Not research evidence: every month is `diagnostic_simulation`/`none`; no eligibility,
  approval or qualification.

