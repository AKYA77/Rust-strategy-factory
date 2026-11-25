# Evidence receipt — V2 Stage-1 CandleScreen CLI regression (2026-11-25)

## Verdict

A correctness regression in the unpushed bot integration line (`4b52ea4`) was
reproduced, corrected at commit `767f777`, and verified with the full local
ladder. Milestone 7 external acceptance is **unchanged and still NOT accepted**;
this receipt covers only the Stage-1 CLI regression and the integrated bot line.

## Scope and provenance

- Checkout: `/root/workspace/Rust-strategy-factory`, branch `main`.
- Before: `4b52ea4` (9 commits ahead of `origin/main` `8a363fc`: bot families 3–4
  work and `4b52ea4` integration of families, improvement queueing, CandleScreen
  entry, prep-cache integration and research-evaluate consistency).
- After: `767f777` (code/test fix on top of `4b52ea4`).
- Data: synthetic EURUSD fixtures only. No real corpus file was opened or
  modified; no XAUUSD path was touched; no data acquired; no gate weakened.

## Reproduction (failing before)

Command (env: `CARGO_HOME=/workspace/.cargo`, `RUSTUP_HOME=/workspace/.rustup`,
`PATH=/workspace/.cargo/bin:$PATH`, `RSF_BUILDER_PROFILE=hermes-v2-m1`):

```
sh scripts/check.sh   # at 4b52ea4
```

Result: `EXIT=101`, one failure:

```
test independent_rust_process_runs_pauses_resumes_and_stops_at_durable_checkpoints ... FAILED
thread '...' panicked at tests/research_cli.rs:139:10:
called `Result::unwrap()` on an `Err` value: QueryReturnedNoRows
error: test failed, to rerun pass `--test research_cli`
```

Ground truth from the abandoned test registry (`factory-research-cli-1149488/
state.sqlite`): 3 experiments, all `state='rejected'`; each `result_json`
carries `shadow_policy.threshold_bps=7000`, `shadow_policy.seed=19`,
`tick_replay: null` and `candidate_screen_audit: null`. The old assertion
`WHERE state='completed'` encoded the superseded TickVerification entry; a
no-signal Stage-1 screen has no authoritative tick replay and is not `completed`.

## Fix

- `tests/research_cli.rs`: assert the V2 stage contract instead of the stale
  outcome — terminal `rejected` experiment, null `tick_replay`, unchanged
  shadow-policy publication, and zero `stage_transitions` rows so a candidate
  without a passing Stage-1 audit cannot be mislabeled as advanced.
- `src/research.rs`: correct the stale `Driver::step` comment (candidates enter
  at `CandleScreen`; promotion is gate-bound, never index-selected).

No financial, scientific, screening, stage or registration gate was changed.

## Verification (passing after, exact tree `767f777`)

```
cargo fmt --all
cargo test --locked --test research_cli -- --nocapture
  => test result: ok. 2 passed; 0 failed (EXIT=0)
sh scripts/check.sh
  => EXIT=0
  => 744 passed / 0 failed across 67 test binaries; lib unittests 276
```

Full-ladder logs: `/root/.hermes/cache/scratch/rsf-cron-check.log` (before) and
`/root/.hermes/cache/scratch/rsf-cron-check-final.log` (after). Both runs were
made with no cargo/rustc process active and with the heavy lock free
(`flock -n /root/workspace/.rsf-heavy.lock true` -> acquired). A zero-byte
`sh` file created by a malformed `flock` command line was removed before commit;
the committed tree contains no such artifact.

## Hosted CI

- Prior terminal baseline: `origin/main` `8a363fc`, run `45537391935`,
  `Rust checks` conclusion `success` (inspected before this change).
- Exact pushed HEAD (the docs reconciliation commit on top of `767f777`): the CI
  run ID and terminal conclusion are recorded by the milestone continuity job
  after push; never reported as passed while in progress.

## Remaining

Milestone 7 remains NOT accepted (stale installed research config, older
deployed dashboard release, unexercised authenticated dashboard endpoints,
owner manual reboot) — all owner-gated. No qualification claim is made: 0
qualified strategies; diagnostic/synthetic results are not research evidence.
