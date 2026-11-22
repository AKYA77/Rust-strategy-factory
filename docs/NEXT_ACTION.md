# Next action

Status: SOFTWARE_INCOMPLETE. Use the existing checkout. Read AGENTS.md, PROJECT_STATE.json, IMPLEMENTATION_QUEUE.md and relevant MASTER_BLUEPRINT.md requirements. After every verified milestone update evidence/state/this file, commit, push, check remote/hosted CI, reread this file from disk and continue.

## Latest verified local milestone

Both PR #22 and PR #24 are merged into `main`, along with PR #35 (screening-efficiency, `740b3d1`) and PR #33 (supervised crash/disk-full injection + restore cutover, `5d1de0f`); current HEAD `dcc12f7` is the PR #33 merge commit.

- `hermes/research-22m-window` (PR #22, merged `634734f`): pins the owner-selected research
  window: `historical_corpus::ResearchWindow::OWNER_SELECTED` = 2025-01..2026-10 (exactly 22
  completed months; owner decision 2026-11-19), `EXPECTED_MONTHS` derives from the pin, and
  `register_research_window` resolves exactly those calendar months from the canonical corpus,
  refusing any window with missing months by exact list (2026-11/2026-12 are not
  completed/available; no padding, backfill or silent truncation). `verify` requires full-length
  specifications to cover the pinned boundaries. The full-corpus research CLIs
  (register/study/execute/execute-cached/audit) register through the pin; new
  `historical-corpus-register-research-window` and `historical-corpus-register-window-config` CLIs
  expose the configuration surface (RUNBOOKS/research-window.md, DECISIONS/0056). Real-data tests
  assert the 22-month boundaries for all eight instruments, identity equality with exact-count
  registration (no reinterpretation of completed results), and exact out-of-range refusals. No
  eligibility, approval or qualification change; the corpus remains REAL_STRUCTURALLY_VALID only.

- `hermes/dashboard-activation-panels` (PR #24, merged `d71ae48`): closes the authenticated
  dashboard's remaining operational gaps from queue item 14: an optional `--operations` serve
  configuration exposes the bounded import/coverage activation lifecycle (inventory/detail
  re-verify every published chunk; explicit activation confirms expected archive hash/event count
  and registers a local catalog observation only — declared class is never approval; explicit
  refusal retains self-hashed append-only decision receipts; corrupt history/tampered chunks refuse
  without deleting evidence), bounded coverage reports, request-time registry integrity audit,
  exact peak-RSS/CPU-tick resource observations and exact owned-visit/member CPU-tick cost charts
  with measured/unmeasured separation. All surfaces stay behind existing host/origin/session/CSRF
  guards plus a 60/min operations bucket and one blocking-work slot; the only non-dashboard source
  change is 42 lines of serve --operations CLI wiring in src/main.rs. Pre-push CI-first
  verification (one flock hold: clean -p, fmt --check, clippy --all-targets -D warnings,
  159 lib/bin unit tests incl. 11 new operations tests, explicit bin build + --operations
  provenance gate, four touched Chromium suites) is receipted with binary sha256/source SHA.
  Hosted CI on the exact head `d4a4325` concluded green (PR run `44664583137`, push twin
  `44664562822`; full suite 529 passed / 0 failed across 54 binaries plus 2 doc tests,
  all 13 Chromium suites, release/identity/operations/full-horizon). Observations are not
  enforcement; no eligibility, approval, qualification, acquisition, live trading or public
  exposure is granted. Software remains incomplete.

Current main CI run `44883200980` on merge commit `d71ae48` concluded success (run #305); hosted_ci in PROJECT_STATE.json now records merged-main CI run #333 (`44915269257`) on `dcc12f7`.

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

Scientific validation over the 22-month corpus (branch `hermes/scientific-validation`): the
owner-approved criteria are now evaluated per instrument from diagnostic execution evidence
(`scientific_validation` module + `scientific-validation` CLI; per-month closed-trade outcome
statistics in `historical_corpus`). Refreshed cached diagnostic execution: 8/8 instruments, 176
executed months, 0 refused, 21,104 trades, 1,094,379 rejections, P&L -3,077,229,124 uUSD;
every instrument's cold and warm reports byte-identical apart from cache counters. Every
instrument is REJECTED (diagnostic class plus failing numerical floors) and 0 qualify.
Reset-fold per-month accounts cannot establish continuous full-period accounting, which stays
unknown. Evidence: `docs/historical-migration/SCIENTIFIC_VALIDATION.md` and
`SCIENTIFIC_VALIDATION_RECEIPT.json`; full Rust suite 58 binaries / 619 passed / 0 failed.

The highest-priority blockers are external: real/protected/non-USD execution, approved source
acquisition, broker/calendar/conversion facts and target-VPS acceptance all remain pending (source
access still returns 403 and no legitimate real quote window has been obtained). Continue the
Continue the remaining software queue — full end-to-end acceptance with hardware/resource evidence, writer backpressure, aggregate audit coverage and the requirement-to-evidence ledger — without weakening financial, provenance, scientific or security safeguards. The converted native corpus is `REAL_STRUCTURALLY_VALID` only and grants no calendar completeness, profile approval, source licensing, scientific eligibility or qualification.

Owner decisions final (2026-11-21): broker dependency removed — deterministic broker-independent execution-cost model (2.0 pip spread, 2.0 pip slippage per side, zero commission); external data acquisition disabled — 22-month local corpus (176 partitions, 421M events) is the final dataset; market calendar policy owner-approved; scientific criteria owner-approved; legal reviewed for local corpus use only; target VPS acceptance continues against existing environment.

Continue the remaining software queue — full end-to-end acceptance with hardware/resource evidence, writer backpressure, supervised crash/disk-full injection and aggregate audit coverage — without weakening financial, provenance, scientific or security safeguards. The converted native corpus is `REAL_STRUCTURALLY_VALID` only and grants no calendar completeness, profile approval, source licensing, scientific eligibility or qualification.

Supervised crash/disk-full injection and restore cutover (PR #33, merged `5d1de0f` into main as part of HEAD `dcc12f7`): `tests/crash_injection.rs` and `tests/restore_cutover.rs` plus provenance receipt `docs/VERIFICATION/crash-injection-cutover-ladder-v1.json`. Disk-full on tmpfs rolls back receipt publication and preserves committed WAL data, resuming once space is freed; SIGKILL during checkpoint write preserves the hash-chained checkpoint chain and resumes past the killed attempt. Restore cutover fences copied actors, refuses corrupt/mismatched snapshots before any destination is created, leaves the original registry untouched, and `registry-check` passes on the restored copy. Local ladder under flock: fmt, clippy `-D warnings`, 6 new tests (3 crash-injection, 3 restore-cutover) and release build all green; hosted CI green on PR #33 head 5d1de0f with run #333 verifying the merged main SHA dcc12f7. Synthetic development evidence only — no real/protected/non-USD execution, no eligibility or qualification.

Scheduler/resource controls (queue item 10; PR #27 `hermes/scheduler-resource-controls`, merged `52ce73e`, head `4a20055`; implementation commit `9f4e1db`) are merged into `main` with hosted CI green on the exact merged SHA. Implemented and tested: exact per-thread native-tick CPU allocation windows with observed/unknown separation and no equalisation (`resource::AllocationWindow`, `observe_thread_cpu`, `named_worker_threads`); a bounded deterministic adaptive worker-selection policy with a mandatory dead band, hysteresis, floor >= 1 and caps at min(policy maximum, declared workers, observed parallelism), fail-closed on unknown (`worker_selection`); reproducible cgroup-v2 OS-enforcement evidence (`os_enforcement`, `resource-enforcement`/`resource-allocation` CLIs) reporting enforced/weaker-than-declared/declared-only/not-declared/unknown; and sampled `cpu_guard`/`resource_guard` admission that refuses before fleet/campaign/rotation lease claims and owned-child spawn and pauses the research Driver before experiment registration. `tests/scheduler_resources.rs` covers pre-claim refusal ordering, legacy definition-byte preservation, Driver pause-before-registration, adaptive corpus-batch receipt-hash parity and both CLIs. No hard kernel enforcement is claimed: on this VPS the gateway cgroup has no cpu controller and memory.max=max, so the deploy templates measure declared-only. Bounded foreground fleet waiting, owned-parent child RSS/CPU-tick visits with cooperative-grace escalation and pre-claim disk/memory admission remain as previously landed (schema11-22). Software remains incomplete.


Current streaming boundary: one authoritative AccountState transition serves materialized and synthetic USD corpus replay. Account checkpoints are hash-chained/lease-fenced; saved prefixes and source artifacts are revalidated before advancement. Bounded parallel corpus rounds now serve both supplied batches and a durable generated queue/coordinator. The materialized Driver remains separate. Real/protected/non-USD execution, fast-seek resume and full-history qualification remain refused/unfinished. See DECISIONS/0010-durable-corpus-backlog.md for implemented boundaries and remaining acceptance.


Keep indexed policy experimental. Profile dominant complete-pipeline stages and validate varied/representative workloads before any promotion; engine-only gains or a small noisy median are insufficient. Implement checkpoint-aware shared-index/batch lifecycle, aggregate reference-audit coverage monitoring; metadata lookup scaling now has component evidence. Continue explicit source revision/correction links, catalog approval lifecycle and coordinated multi-component recovery; bounded immutable inventory, continuous export and the import/coverage activation dashboard workflow now exist. Preserve declared class versus independent approval and source availability. Continue remaining lifecycle, and the remaining scientific/operations queue rather than treating the performance experiment as software completion. Preserve exact eligibility/fallback, source scope, immutable evidence and reference/proof audit independence.

Continue the full queue: justified codecs, resumable sources/imports/coverage, dependence-aware prespecified qualification/evidence gates and real diverse boards, remaining real dashboard/operations/security workflows and integrated acceptance. Use IMPLEMENTATION_QUEUE.md's source/test reconciliation; do not confuse implemented code with acceptance.

## Owner decisions (final, 2026-11-21)

* Broker dependency: REMOVED. Deterministic broker-independent execution-cost model: fixed 2.0 pip spread, fixed 2.0 pip slippage per side, zero commission, no broker-specific swap/financing, no broker API/account/credentials/connectivity.
* External data acquisition: DISABLED. 22-month local corpus (2025-01..2026-10, 176 partitions, 421M events) is the final dataset. No Dukascopy/HistData/FXOpen/TrueFX retries.
* Market calendar policy: APPROVED. Trade when instrument market is open; include Sunday evening FX sessions and Friday sessions before close; respect gold-market breaks; handle holidays/DST with UTC-normalized timestamps; never invent candles/ticks/prices during closed sessions.
* Scientific protocol: owner-approved. ≥50.00% verified net win rate per instrument; ≤5 consecutive losses; 3:1 RR target; keep 5 best qualified strategies per instrument; preserve OOS testing, leakage prevention, robustness checks.
* Legal review: local corpus provenance documented; no redistribution; no invented license approval.
* Target VPS acceptance: continue testing against existing 4-vCPU/8-GB environment — production builds, local dataset loading, systemd service behavior, bounded memory/CPU, safe restart/recovery.

## Invariants

Exact executable bid/ask, 2-pip spread floor plus separate 2-pip adverse market/stop slippage, zero commission, 10% intended risk/$20 initial/3R/24h; frozen pending distance/quantity; no fabricated gaps/fills/depth; no protected OOS/final adaptation; synthetic never qualifies. Qualification needs independent approved complete evidence, not a high test count or tiny benchmark. Registry-local protection cannot cover external tools/copied registries. No execution continues after a session ends unless an actual supported mechanism exists.

Zero acquired bytes from external sources; no accounts/emails/purchases. Full historical qualification and target-VPS acceptance remain pending engineering work, not external approvals.

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

