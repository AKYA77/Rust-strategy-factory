# Format-gate repair and calendar-wiring reconciliation (2026-11-26 continuity run)

## Baseline and guards

Clean `main` at `d1f55eaf2d320b36d021bc1453620bed573f5f05` == `origin/main` (0 ahead /
0 behind, `git ls-remote` identical), branch `main`, no other `cargo`/`rustc` worker,
non-blocking heavy lock acquired. No worktree, no reset, no history rewrite, no
market-data download, no XAUUSD path touched.

Two already-pushed commits were unreconciled in `docs/NEXT_ACTION.md`,
`docs/PROJECT_STATE.json` and `docs/IMPLEMENTATION_QUEUE.md` (ledgers ended at
`9442d85`):

1. `dbc75983846d6967119c200b69c3af1e0cdcbe76` — docs-only CI-record continuity for
   `9442d85`. Hosted run **45814075396** (push, `main`, head_sha exactly `dbc7598`)
   reached terminal `completed` / `success` on all 7 jobs (full-horizon, browser,
   data-compute-identity, dependency-audit, check-operations, rust-checks,
   release-build). No in-progress run was called a pass.
2. `d1f55eaf2d320b36d021bc1453620bed573f5f05` — "Implement economic calendar from
   corpus gaps and wire through pipeline": added
   `EconomicCalendar::from_corpus_gaps()`, `research::Input::economic_calendar`,
   `Features::with_economic_calendar()`, gap-calendar construction in `main.rs` and
   `tick_engine.rs`, and `economic_calendar: None` in offline/bench/test initializers.

## Defect discovered (RED)

`d1f55ea` claimed "cargo fmt clean" in its commit message, but hosted CI run
**45822773618** on that exact SHA concluded `completed` / `failure`: only the
`rust-checks` job (job id 139919015606) failed, at step 5
`cargo fmt --all -- --check`; the other six jobs (release-build, browser,
data-compute-identity, full-horizon, check-operations, dependency-audit) were
`success`. Tests in that job were `skipped`, so no test result was called a pass.

Local reproduction with the pinned toolchain (`rustfmt 1.11.0-stable`, toolchain
1.100.0): `cargo fmt --all -- --check` **EXIT=1** with four indentation diffs:

- `benches/batch.rs:99`
- `benches/indexed_factory.rs:73`
- `benches/proof_screen.rs:65`
- `tests/windows.rs:311`

Raw RED output retained at `docs/VERIFICATION/fmt-gate-repair-2026-11-26-before.log`.

## Scientific review of the calendar wiring

`Features::news_blackout_mask()` is defined and the calendar is attached to
`Features`, but the mask is **not called** by `signal()`, `signal_sparse()`,
`evaluate_prepared()`, `evaluate_batch_report()` or the replay path. The wiring is
therefore inert with respect to signals, fills and financial receipts: no
strategy outcome, classification or receipt hash changed. The calendar is derived
only from local corpus gaps — no external acquisition, no released/forecast values
fabricated, and every event carries `source_id`, revision and causal `published_ms`.
The module doc was tightened to state that gap-derived events are diagnostic
observations, **not** approved market-calendar facts, calendar-completeness claims
or closed-session evidence — consistent with the owner's WITHDRAWN market-calendar
policy. No financial, scientific, provenance, data-scope or security rule was
weakened. Class remains `REAL_STRUCTURALLY_VALID`, eligibility none.

## Fix and regression guards (GREEN)

- Applied the canonical `cargo fmt --all` rewrite (whitespace only, 4 lines) to the
  four files above.
- Added two unit regressions in `src/economic_calendar.rs` for the previously
  untested `from_corpus_gaps()` path:
  - `corpus_gap_events_are_causal_provenance_tagged_and_impact_bounded`: gap events
    carry instrument/`EventType::Other`, provenance (`corpus-gap-analysis-v1`,
    `rev-1`), bounded impact (sub-day Medium, multi-day High), causal
    `published_ms == post-gap tick` (not available one ms earlier), and no
    fabricated actual/forecast/previous values.
  - `corpus_gap_thresholds_are_validated_and_normal_cadence_emits_nothing`:
    non-positive thresholds are refused; normal cadence and empty input emit none.
- RED/GREEN: `cargo fmt --all -- --check` EXIT=1 before, EXIT=0 after; focused
  `cargo test --locked --lib economic_calendar` 8 passed / 0 failed (EXIT=0).
- Full ladder `scripts/check.sh` (`RSF_BUILDER_PROFILE=hermes-v2-m1`, heavy lock):
  **EXIT=0**, **765 passed / 0 failed across 72 test binaries** (lib 278), zero
  named failures; `cargo fmt --all -- --check` and
  `cargo clippy --locked --all-targets -- -D warnings` clean. The +2 over the
  763-test baseline are the two new calendar regressions.

## CI status

Hosted CI for the repair commit is recorded in the ledger continuity entry in
`docs/NEXT_ACTION.md`: run **45837439949** on exact SHA
`16afb16c5b55ea48fe8b0129820399ec19af29c4` reached terminal `completed` /
`success` on all 7 jobs, including the previously failing `rust-checks`; no
in-progress run was claimed as passed. The failed run 45822773618 is retained as
evidence of the defect and is superseded by the repair.

V2 software milestones 1–6 remain evidenced. Milestone 7 remains NOT accepted: the
two owner-only external gates (saved dashboard credentials via `browser_vault` and
a fresh owner manual reboot) are unchanged by this repository increment.
