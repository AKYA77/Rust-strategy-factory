# Codex integration-stack compatibility notes

Branch `hermes/real-data-migration` vs the Codex stack tip
`codex/retained-preflight-inventory` @ `74790a44d39f` (PR #13), inspected read-only.
Codex branches were not modified.

## Dependency chain

Codex PRs #8–#13 form a stacked engine/checkpoint/corpus/campaign series (trusted checkpoint
reuse, source-byte verification, checkpoint-aware range decoding, parallel worker affinity,
failed-step lease cleanup, attempt fencing, checkpoint history limits, retained preflight
inventory). None are merged to `main`; individual green PRs do not prove the combined stack.

## File overlap with this branch

`git diff --name-only main..codex/retained-preflight-inventory` (34 files) vs
`main..hermes/real-data-migration`:

- **Shared:** `tests/corpus.rs` only (this branch hardened three timing-sensitive tests;
  Codex's stack also edits corpus tests). Merge-conflict risk is limited to that file.
- **This branch's `src/` files are disjoint:** `historical_migration.rs`,
  `historical_corpus.rs`, `historical_catalog.rs`, `data_review.rs`, plus additive CLI arms
  in `main.rs`. No Codex engine/corpus/campaign/dashboard file is modified.

## Compatibility risks

1. **`corpus` API coupling.** `historical_corpus`/`historical_catalog` and the real-data
   pipeline depend on `corpus::{describe, inspect, load, source_facts, candle_context,
   signals}`. If the Codex stack changes those signatures, this branch must be re-based.
2. **`tests/corpus.rs` conflict.** Both sides edit campaign/fleet timing tests.
3. **Compute identity.** The compiled compute identity enumerates specific source modules.
   The additive historical modules are not in that enumeration, so they should not change it;
   this must be confirmed on the combined branch (reversible rebuild check).
4. **Scientific scope is unchanged.** This branch does not touch replay/eligibility gates;
   `corpus-replay` still refuses real/protected data (synthetic literal-USD only).

## Recommended integration approach

Create `hermes/integration-codex-stack` from the Codex tip, merge this branch, run the full
Rust/browser/operations acceptance plus the real-data tests, and fix only integration-specific
incompatibilities. Do not rewrite Codex commits; preserve ancestry; keep the integration PR
explicitly dependent on its upstream PRs. Disk admission for the combined build must be
confirmed first (see `STORAGE_REPORT.md`).
