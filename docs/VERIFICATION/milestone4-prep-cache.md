# Milestone 4 — shared preparation caches: contract evidence

Baseline: `main` @ `98cdc79` (`M3: crash-safe stage machine, immutable telemetry, 40% Stage-1 policy`).
Workspace: `/root/workspace/Rust-strategy-factory`. Implementation and tests are in the
primary checkout; no external data, no downloads, no live trading. Nothing here grants
eligibility, approval or qualification. A cache hit proves only that identical bytes were
previously prepared for an identical scope.

## Requirement → evidence map

| Exit requirement | Implementation | Contract test |
|---|---|---|
| Exact scope identity over instrument/dataset/strategy/costs/engine/config/window/stage | `src/prep_cache.rs::Scope::{validate,identity}` (`prep-cache-scope-v1`, blake3 over a fixed-order tuple; protection is a mandatory stage qualifier) | `scope_identity_is_exact_over_every_dimension`, `changing_any_scope_dimension_invalidates_the_cache_entry` |
| Durable on-disk cache, candle/feature/decision | `PrepCache::open`, `Stage::{Candle,Feature,Decision}`, sqlite index + content-addressed `.bin` files, fsync + hard-link publication | `cold_warm_parity_is_exact_for_candle_feature_and_decision`, `artifacts_survive_reopen_with_exact_receipts` |
| Immutable artifacts, byte caps, content hashes | `prep_artifacts` table with `prep_artifact_no_update` / `prep_artifact_no_delete` triggers; `MAX_ARTIFACT_BYTES`, `MAX_STORE_BYTES`, `MAX_ARTIFACTS`, caller `Limits` bounded by hard ceilings; `hash_raw` on every read | `immutable_index_rows_reject_update_and_delete`, `byte_caps_are_enforced_on_artifact_and_store`, `corrupted_mismatched_or_truncated_artifacts_are_refused` |
| Refuse corrupt/mismatched/truncated artifacts | `read_artifact` checks regular-file (no symlink), exact length, 64-hex hash and recomputed blake3; `integrity` re-verifies every indexed row | `corrupted_mismatched_or_truncated_artifacts_are_refused` |
| Deduplication, equivalent decoded data | Content-addressed filenames share identical payloads across scopes; `put` on an existing scope verifies and creates no new row; `encode`/`decode` verify the hash before decoding | `identical_scopes_deduplicate_and_decode_to_equivalent_data` |
| Exact receipt comparison, cold/warm parity | `Receipt` is cold/warm invariant (`Status` returned separately); `get_or_fill` never calls the producer on a hit | `cold_warm_parity_is_exact_for_candle_feature_and_decision`, `artifacts_survive_reopen_with_exact_receipts` |
| Protected cache isolation | Protection class is in the identity, selects a separate artifact directory, and each handle serves exactly one policy; cross-class `get`/`put` is refused | `protected_scopes_are_physically_and_logically_isolated` |
| Recovery/refusal after partial writes | Staged `.partial-<64hex>` files are never read; `inventory` reports them; `recover_partial` removes only valid remnants under a writer transaction; unknown entries are refused | `partial_writes_are_never_served_and_recovery_is_explicit` |

Reused patterns: `storage::writer_tx` / `writer_tx_open` and immutable triggers
(`src/storage.rs`, `src/result_artifact.rs`), `data::hash` / `hash_raw` (`src/data.rs`),
bounded fail-closed validation and exact-string receipts (`src/ordered_coverage.rs`).
No scientific gate, exposure ledger, qualification check or threshold was modified.

## Exact commands and results

Environment: `CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
PATH=/workspace/.cargo/bin:$PATH RSF_BUILDER_PROFILE=hermes-v2-m1`, toolchain `1.100.0`.

```
$ cargo fmt --all
FMT_EXIT=0

$ cargo clippy --locked --all-targets -- -D warnings
    Checking rust-strategy-factory v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.23s
CLIPPY_EXIT=0

$ cargo test --locked --test prep_cache
running 10 tests
test artifacts_survive_reopen_with_exact_receipts ... ok
test corrupted_mismatched_or_truncated_artifacts_are_refused ... ok
test identical_scopes_deduplicate_and_decode_to_equivalent_data ... ok
test cold_warm_parity_is_exact_for_candle_feature_and_decision ... ok
test scope_identity_is_exact_over_every_dimension ... ok
test changing_any_scope_dimension_invalidates_the_cache_entry ... ok
test immutable_index_rows_reject_update_and_delete ... ok
test byte_caps_are_enforced_on_artifact_and_store ... ok
test partial_writes_are_never_served_and_recovery_is_explicit ... ok
test protected_scopes_are_physically_and_logically_isolated ... ok
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
TEST_EXIT=0
```

## Files changed by this task

- `src/prep_cache.rs` (new, 749 lines) — scope identity, limits, receipts, durable store,
  byte accounting, dedup, corruption refusal, partial-write recovery.
- `tests/prep_cache.rs` (new, 534 lines) — 10 integration tests with exact assertions.
- `docs/VERIFICATION/milestone4-prep-cache.md` (new) — this record.
- `src/lib.rs` — exactly one added line: `pub mod prep_cache;` (rustfmt places it in
  declaration order). No other file was edited by this task and nothing was committed.

In-flight untracked `milestone6-*`/`milestone7-*` files and edits to `docs/NEXT_ACTION.md`
and `scripts/browser*.cjs` belong to concurrent sibling agents and were left untouched.

## Observed behavior worth recording

- All ten standard scopes in the invalidation test produced eleven distinct scope IDs
  while deduplicating to two physical artifacts (one standard, one protected): identity
  is per scope, storage is per content hash.
- A read-only reopened handle serves a warm hit without invoking the producer, and the
  receipt compares exactly equal to the receipt from the original cold fill.
- The window dimension is checked both ways: moving either `start_ms` or
  `end_exclusive_ms` by 1 ms changes the identity and forces a cold fill.

## Honest limits

- `inventory` counts summed file lengths, not allocated filesystem blocks, and does not
  verify artifact contents; `integrity` performs full content verification for one
  protection class per handle. These are declared metadata observations, not an OS quota.
- An interrupted write can leave a finalized-but-unindexed content file (dedup reuses it
  on the next `put`) or a valid `.partial-*` remnant (removed by `recover_partial`). The
  cache never guesses which scope such bytes belonged to and never serves them.
- The cache is not wired into the research pipeline by this task: it is a durable,
  contract-tested primitive for later callers. No scientific speedup or parity with an
  unwarmed pipeline is claimed here.
- Full-suite (`check.sh`) and cross-milestone verification remain the parent's step.
