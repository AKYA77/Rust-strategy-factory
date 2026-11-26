# Bid-only conversion convention reconciliation (2026-11-26 continuity run)

Status: VERIFIED (code fix + regression evidence + ledger reconciliation). No financial,
scientific, provenance, data-scope or security rule was weakened. The corpus class remains
`REAL_STRUCTURALLY_VALID` with eligibility `none`. This is diagnostic simulation only: no
calendar completeness, broker/profile approval, source licensing, research eligibility or
qualification is granted.

## Baseline and scope

Baseline `main` HEAD `38fcc736863e4bb59b888134d958b963d27b3138` (equal to `origin/main`,
0/0 ahead-behind, clean worktree, no other cargo/rustc worker, heavy lock acquired). Two
already-pushed commits were unreconciled in the evidence ledgers:

- `617f21575129130127c37c4b7d917ad1fd41326a` — "Reconcile conversion convention: bid-only
  everywhere, correct memory docs" (7 files: `src/tick_engine.rs`, `src/historical_corpus.rs`,
  `tests/verify_conversion.rs`, `tests/historical_pipeline_real.rs`,
  `docs/CONVERSION_CONVENTION.md`, `docs/PROJECT_STATE.json`,
  `docs/historical-migration/SCIENTIFIC_VALIDATION.md`).
- `38fcc736863e4bb59b888134d958b963d27b3138` — "Document conversion memory consumption and
  coverage gap" (docs only: `docs/CONVERSION_CONVENTION.md`; measured per-partition conversion
  memory, the distinct 2 GiB input-file budget vs 8 GB post-load RAM budget, ~6.58 GB of
  conversion vectors for the full 22-month/7-instrument corpus, a 20-month dev vs 22-month
  corpus coverage table with 74 missing inputs, and limitations #7/#8).

## What the code fix changed

At `c3ee6c0` the diagnostic replay path used a direction-based reciprocal convention
(long -> bid, short -> ask) while the research loader in `src/main.rs::research_inputs()`
derived one conversion per tick from that tick's bid at load time, before any strategy side
is known. Batch evaluation likewise shares one conversion vector across strategies with
potentially different sides. That mismatch made the two paths disagree for Short positions.

`617f215` makes `tick_engine::diagnostic_conversion` bid-only for both sides (the `Side`
argument is ignored) and renames the bound policy string from
`diagnostic_reciprocal_closing_rate_v1_long_bid_short_ask` to
`diagnostic_reciprocal_closing_rate_v1_bid_only`. The diagnostic replay now hashes the same
bid-based conversion sequence the research loader produces. The reciprocal rate remains an
explicit, **unverified** research assumption derived from the instrument's own verified quote
(`usd_num/quote_den = price_scale/bid_price`, `as_of_ms = tick.timestamp_ms`) — not an
approved broker rule. Because the policy string is bound to the execution report and corpus
identity receipt, changing the convention invalidates numerical comparison with reports
produced under the prior string. No class, gate, eligibility or financial rule was changed.

## Red / green regression demonstration

Temporary RED change: restore the pre-`617f215` direction-based body of
`diagnostic_conversion` (Long -> `tick.bid`, Short -> `tick.ask`), with the current tests left
untouched. File restored byte-for-byte afterwards (sha256 `c4bbbd49…` before and after);
`git status --porcelain` empty.

| Phase | Command | Result | Named outcome |
|---|---|---|---|
| RED | `cargo-locked.sh test --locked --lib worker_preparation_tests::diagnostic_conversion_is_bid_based_reciprocal` | `EXIT=101`, 0 passed / 1 failed | `tick_engine::worker_preparation_tests::diagnostic_conversion_is_bid_based_reciprocal ... FAILED` at `src/tick_engine.rs:2143`, left `(10, 1501)` (ask) vs right `(1, 150)` (bid) |
| GREEN | same command after restore | `EXIT=0`, 1 passed / 0 failed | same test `ok` |
| RED | `cargo-locked.sh test --locked --test verify_conversion` | `EXIT=101`, 13 passed / 1 failed | `all_three_non_usd_instruments_derive_conversions_from_dataset ... FAILED` at `tests/verify_conversion.rs:316`: "USDJPY loader vs tick engine Short", loader `Conversion { usd_num: 2, quote_den: 217, … }` vs engine `Conversion { usd_num: 25, quote_den: 2713, … }` |
| GREEN | same command after restore | `EXIT=0`, 14 passed / 0 failed | all 14 tests `ok` |

The RED failure is the exact bug the fix removes: the engine's Short-side rate diverged from
the loader's bid-only rate. `tests/verify_conversion.rs` (14 tests) now pins loader == engine
for Long and Long == Short for USDJPY/USDCHF/USDCAD, plus the exact `i128` cross-product;
`conversion_vector_memory_is_linear_in_tick_count` documents the 24-byte-per-conversion
linear allocation that underlies the 38fcc73 memory note.

## Full verification ladder

`scripts/check.sh` with `RSF_BUILDER_PROFILE=hermes-v2-m1` at baseline HEAD: **EXIT=0**,
**760 passed / 0 failed across 71 test binaries** (lib unit tests 276), zero named failures,
`cargo fmt --all -- --check` and `cargo clippy --locked --all-targets -- -D warnings` clean.
This corrects the stale `762 passed / 72 binaries` ledger figure set by `617f215`: a fresh
full run reproducibly yields 760/71 (69 integration binaries + lib unittests + bin unit
tests), no test binary skipped.

## Hosted CI (terminal conclusions only)

- `617f215` — run `45701751661`, terminal `completed` / `success` on all 7 jobs
  (full-horizon, check-operations, release-build, rust-checks, browser,
  data-compute-identity, dependency-audit).
- `38fcc73` — run `45716427171`, terminal `completed` / `success` on all 7 jobs
  (same job names).
- The docs-only commit `80dbddd` had run `45697619785` conclude terminal `failure`; it is
  retained as historical evidence and is superseded by the two successful runs above. No
  in-progress run is reported as a pass.

## Limitations

- The bid-only reciprocal rate is a documented research assumption, not a broker/venue rule;
  no live/broker use and no external data acquisition.
- Bounded code + regression + documentation reconciliation only: not a full 22-month non-USD
  conversion study and not scientific qualification. The 38fcc73 memory/coverage figures are
  measured observations and a documented 20-month/22-month input-coverage gap (74 missing
  inputs), not evidence that the pipeline is complete.
- V2 software milestones 1–6 remain evidenced; Milestone 7 is still NOT accepted — the two
  remaining items (no saved dashboard credentials for authenticated endpoint exercise, and a
  fresh owner manual reboot for reboot-persistence evidence) are owner-only external gates
  that repository code cannot close.
