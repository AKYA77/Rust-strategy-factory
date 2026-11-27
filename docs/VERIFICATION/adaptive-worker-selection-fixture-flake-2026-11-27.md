# Adaptive worker-selection integration fixture flake (2026-11-27 continuity run)

Status: RED baseline reproduced on clean `main`, GREEN after a test-only fixture
fix. No production source, financial, scientific, provenance, data-scope or
security rule changed. Classification remains `REAL_STRUCTURALLY_VALID` with
eligibility `none`; no diagnostic result was relabelled and no gate was widened.

## Baseline (RED)

- Tree: clean `main` HEAD `eacf7f663ee2cb8f03150a0c73a1781cfa235528`, equal to
  `origin/main`; no other cargo/rustc worker; heavy lock acquired.
- Command (UTC timestamps from the captured log):
  `export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup PATH=/workspace/.cargo/bin:$PATH RSF_BUILDER_PROFILE=hermes-v2-m1; sh scripts/check.sh`
- `STARTED 2026-11-27T15:12:30Z`, `EXIT=101`, `ENDED 2026-11-27T15:22:17Z`.
- Failing test (exactly one named failure):
  `tests/scheduler_resources.rs::adaptive_batch_measures_cpu_and_adapts_workers_without_changing_receipts`
  panicked at `tests/scheduler_resources.rs:350`: `never downscaled: [4, 3, 3, 2]`
  (`test result: FAILED. 5 passed; 1 failed`), followed by
  `error: test failed, to rerun pass --test scheduler_resources`.
- Before the failing binary, 56 test binaries had `test result: ok` with 653
  passing tests; fmt and clippy layers had already passed. The failure is a
  load-dependent fixture timing issue, not a compile or formatting defect.
- The same test passed 5/5 in isolation on the unmodified tree
  (`bash scripts/cargo-locked.sh test --locked --test scheduler_resources
  adaptive_batch_measures_cpu_and_adapts_workers_without_changing_receipts --
  --exact`), and the whole `scheduler_resources` binary passed 5/5 consecutive
  runs in isolation. It failed only inside the full ladder.

## Root cause

`batch_config` produced only 8 jobs and the CLI was invoked with 10 max rounds.
The adaptive policy starts at 4 workers and steps down once per qualifying host
CPU observation (hysteresis 1). When one sampled window is idle/unknown and
holds the down-streak, the sequence becomes `4,3,3,2`; with 8 jobs the batch
reaches `stop_reason=complete` after 4 rounds, so the tested floor
(`minimum_workers=1`) is never observed in `workers_used`. The production
selection logic (`src/worker_selection.rs::select`) is correct and is already
pinned by the pure unit test
`sustained_pressure_steps_down_to_floor_and_never_below`; the integration
fixture was too short to outlast host-sampling noise. No production change is
warranted.

## Fix (GREEN)

Changed path: `tests/scheduler_resources.rs` only.

- Fixture: 24 jobs (`(80..320).step_by(10)`) instead of 8; `expected.len() == 24`.
- Round budget: 40 max rounds for both the fixed-1-worker control and the
  adaptive run (the control needs one round per job).
- Kept the original contract assertions (`used[0] == 4`, `used.contains(&1)`,
  monotone non-increase, identical completed receipt hashes, measured host
  evidence per round) and added a stronger assertion
  `used.last() == Some(&1)` so the fixture must still be running when selection
  reaches the floor and must stay there.
- Canonical `cargo fmt --all` applied; the patch tool's rustfmt/clippy lint
  output was spurious (different edition) and is not a verdict.

Verification on the final tree:

- Focused regression test: 5/5 consecutive `EXIT=0` runs (1 passed each;
  ~1.45–1.75 s per run).
- Full ladder on the exact modified tree: `EXIT=0`,
  **765 passed / 0 failed / 0 ignored across 72 test binaries** (lib 278),
  zero named failures, fmt and `cargo clippy --locked --all-targets --
  -D warnings` clean; `STARTED 2026-11-27T15:26:46Z`,
  `ENDED 2026-11-27T15:35:24Z`. The target test is recorded as
  `adaptive_batch_measures_cpu_and_adapts_workers_without_changing_receipts ... ok`.

## Hosted CI and owner gates

- Hosted run **45988023105** on the exact baseline HEAD `eacf7f6` (event push,
  branch main) reached terminal `completed`/`success` on all 7 jobs:
  release-build, full-horizon, browser, rust-checks, dependency-audit,
  data-compute-identity, check-operations. Only terminal conclusions are
  recorded; no in-progress run was called a pass.
- Read-only owner-gate re-check: `browser_vault list` returned no saved logins;
  `who -b` = 2026-11-22 17:47 with `uptime` 4 days 22:25, so no fresh reboot;
  deployed user-scope units remain research `activating`/`auto-restart`
  `NRestarts=830` and dashboard `inactive`/`dead` `NRestarts=32266`. No service,
  config or production cutover was changed.
- V2 software milestones 1–6 remain evidenced. Milestone 7 is still NOT
  accepted: its two remaining items (authenticated dashboard exercise with
  owner-provided credentials, and an owner manual reboot persistence proof)
  are external-evidence gates, not repository code. Rubric from
  `docs/historical-migration/PROGRESS.md`: software ~85%, verified end-to-end
  ~80% -- unchanged by this test-fixture fix.
