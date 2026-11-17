# Codex integration-stack compatibility notes

Branch `hermes/real-data-migration` vs the Codex stack tip
`codex/retained-preflight-inventory` @ `74790a44d39f` (PR #13), inspected read-only.
Codex branches were not modified.

## Dependency chain

Codex PRs #8–#13 form a stacked engine/checkpoint/corpus/campaign series. The tip
`codex/retained-preflight-inventory` @ `74790a44d39f` is **11 commits** ahead of `main`:

1. `f3b268e` reuse private worker-produced checkpoint state (trusted checkpoint reuse)
2. `20db094` fence checkpoint publication by the complete current lease attempt
3. `2bd84fc` allow bounded checkpoint histories through the global journal ceiling
4. `63c7e4a` relinquish still-current corpus leases after returned execution errors
5. `96f7780` rehash every current artifact without redecoding verified snapshots
6. `1068517` integrate checkpoint-reuse and source-rehash heads + adversarial parity tests
7. `eaf8bd6` decode checkpoint-aware source ranges with full final-byte verification
8. `97059bf` preserve private checkpoint ownership before parallel work stealing
9. `c1f134c` integrate failed-step cleanup with checkpoint-aware worker replay
10. `10461ef` integrate exact checkpoint authority and long-history safeguards
11. `74790a4` reuse privately verified source inventories across batch preflight rounds

Earlier branches (`checkpoint-range-seek`, `trusted-session-checkpoints`, …) are the
individual PR heads; the tip is the accumulated stack. None are merged to `main`;
individually green PRs do not prove the combined stack.

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
