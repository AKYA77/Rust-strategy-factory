# Cargo target-dir isolation runbook

## Hazard

All rsf profiles (commander, rust, qa, research, dashboard) share a single
`CARGO_TARGET_DIR` (/root/workspace/Rust-strategy-factory/target on this VPS;
~7.8 GB, disk too tight for per-lane copies). Cargo's metadata hash for
rust-strategy-factory's lib/bin/test artifacts is checkout- and path-independent,
so:

- a `cargo test`/`build`/`clippy` in one worktree overwrites that checkout's
  artifacts in the shared target;
- a later ladder in a *different* checkout then silently executes the FOREIGN
  branch's binaries while reporting green.

Verified by rsf-qa acceptance sweep t_e0f73db5 (2026-11-19): main's
`scripts/check.sh` ran the dashboard branch's 529 passed / 159 lib instead of
main's 518 / 148, and a data-compute-identity round-trip failed in the same
contamination window. Hosted CI is unaffected (clean environment).

## Binding commander decision

KEEP the single shared `CARGO_TARGET_DIR`. Do NOT duplicate the dependency cache
per lane (~39 GB for 5 lanes vs ~23 GB free; cold caches cost ~20 min/lane).
Instead make cross-checkout artifact reuse IMPOSSIBLE by machine-enforcing, for
ALL cargo invocations:

1. the team-wide heavy flock (`flock -w 3600 /root/workspace/.rsf-heavy.lock`);
2. a provenance stamp (`target/.rsf-build-stamp.json`: checkout path + git HEAD
   SHA + dirty-tree content hash + builder profile) compared on every build-like
   call; on ANY mismatch/absence, `cargo clean -p rust-strategy-factory` runs
   INSIDE the lock (project artifacts only — the dependency cache stays warm);
3. a provenance log (`target/.rsf-provenance.jsonl`) + optional verification
   receipt (per-binary test counts + artifact sha256 + source SHA).

## Enforced protocol

`scripts/cargo-locked.sh` is the single chokepoint. Every cargo invocation that
touches the target must route through it (directly or via `scripts/shim/cargo`).

Per invocation:
1. `exec 9>"$LOCK"; flock -w 3600 9` — one heavy job at a time, team-wide.
2. Resolve the REAL cargo (rustup proxy; shim dir stripped from PATH to prevent
   recursion; never a bare toolchain binary, which can drift off the pinned
   toolchain and raise E0514).
3. If the command is build-like (`build|test|clippy|check|run|bench|doc|rustc|install`),
   compare the stamp to the current checkout/source:
   - match → reuse (within-lane incremental caching preserved);
   - mismatch/absence/unreadable → `cargo clean -p rust-strategy-factory`
     INSIDE the lock, then build.
4. Run `$REAL_CARGO "$@"`, teeing output to a temp file OUTSIDE the target so a
   full `cargo clean` (which deletes the target dir itself) cannot destroy the
   log (defect D1 — see below). cargo's own exit code is always reported.
5. Recreate target/log dirs, record the stamp (success) or delete it (failure /
   full clean), append a provenance log line, and — if `RSF_RECEIPT_DIR` is set
   — write a verification receipt via `scripts/rsf_cargo_receipt.py`.

`cargo fmt` and `cargo doc` take the lock but are not treated as build-like (no
forced clean); `fmt` does not touch the target. `cargo clean -p $PKG` keeps the
target dir and the stamp (same-checkout recompile is correct); a full `cargo
clean` removes the stamp so the next build-like call forces `clean -p`.

### Defect D1 (fixed)

A full `cargo clean` deletes the target directory itself, so post-run
bookkeeping (stamp/provenance/log) used to crash with FileNotFoundError and the
wrapper reported rc=1 for a successful cargo command, losing the provenance
record. Fix: tee through a temp file outside the target, recreate the dirs
before any bookkeeping, and always report cargo's own exit code. Regression
covered by TEST F in scripts/test-cargo-locked.sh.

## Files

- `scripts/cargo-locked.sh` — wrapper + stamp + forced clean + provenance + receipt.
- `scripts/rsf_cargo_receipt.py` — per-binary test counts + artifact sha256 + source SHA receipt.
- `scripts/test-cargo-locked.sh` — light mechanics suite (throwaway target/lock, no compilation).
- `scripts/shim/cargo` — thin shim; `PATH="scripts/shim:$PATH" cargo <cmd>`.
- `scripts/check.sh` — push gate, every cargo call routed through the wrapper.
- `docs/RUNBOOKS/cargo-target-isolation.md` — this document.

## Rejected alternatives

(a) Per-worktree full `CARGO_TARGET_DIR`. Disk-prohibitive: ~7.8 GB/target × 5
lanes ≈ 39 GB vs ~23 GB free; cold caches cost ~20 min/lane. Rejected by the
commander; the shared target stays.

(b) Path-dependent build hashes via `RUSTFLAGS`/`-C metadata` or a rustc wrapper.
Risks duplicating the dependency cache and fragile cargo-1.100 internals surgery.
Rejected for now; revisable only as a LATER optimization against the same
acceptance repro (cross-worktree no-reuse).

Do not weaken any test or gate to accommodate this protocol.

## Invocation conventions

- Through the wrapper (recommended, works everywhere):
  `scripts/cargo-locked.sh test --locked --lib --bins --tests`
- Through the shim (ad-hoc `cargo` feel):
  `PATH="$PWD/scripts/shim:$PATH" cargo test --locked --lib --bins --tests`
- `scripts/check.sh` is fully wired; run `sh scripts/check.sh`.

`RSF_CHECKOUT_DIR` lets the wrapper drive a *foreign* checkout through this same
wrapper (used by the cross-worktree repro). `RSF_BUILDER_PROFILE` labels who
built (default `$USER`).

## Cross-worktree repro (acceptance 1; re-runnable by rsf-qa)

Goal: prove a foreign checkout's artifacts are never reused by this checkout.

1. `git fetch origin main`
2. Create a throwaway detached worktree on a distinct commit:
   `git worktree add --detach /tmp/rsf-foreign-repro <foreign-sha>`
3. Under the team lock, build the foreign checkout into the shared target and
   record its signature:
   ```
   export RSF_CHECKOUT_DIR=/tmp/rsf-foreign-repro
   export RSF_BUILDER_PROFILE=rsf-qa-repro
   export RSF_RECEIPT_DIR=/root/workspace/rsf-audit
   export RSF_RECEIPT_LABEL=foreign
   scripts/cargo-locked.sh test --locked --lib --bins --tests
   cp /root/workspace/rsf-audit/foreign.json /root/workspace/rsf-audit/foreign-signature.json
   ```
   Record: source SHA + per-binary counts + binary sha256 (from `foreign.json`).
4. Remove the throwaway worktree: `git worktree remove --force /tmp/rsf-foreign-repro`.
5. Run this checkout's verification through the wrapper:
   ```
   unset RSF_CHECKOUT_DIR
   scripts/cargo-locked.sh test --locked --lib --bins --tests
   ```
   (receipt written to /root/workspace/rsf-audit/run.json by the wrapper since
   RSF_RECEIPT_DIR is still set)
6. Confirm:
   - this run's provenance shows `forced_clean: true` / a `mismatch:` reason
     (the stamp still held the foreign checkout → forced rebuild);
   - this checkout's test artifact sha256 is DISTINCT from the foreign build's;
   - this checkout reports its own signature (counts + lib count), not the
     foreign one.

No artifact reuse is demonstrated by the forced rebuild + distinct sha256. The
receipts under /root/workspace/rsf-audit/ are the machine evidence.

## Mechanics suite

`scripts/test-cargo-locked.sh` — 28 assertions, throwaway target + lock, no
heavy builds, re-runnable by any lane. Includes the D1 regression (TEST F) and
the no-stamp / matching / foreign / stale-stamp forced-clean cases (A–E, G).
Run: `scripts/test-cargo-locked.sh` (exit 0 = all pass).

## Receipt format

`/root/workspace/rsf-audit/<label>.json`:
```
{
  "at": "...",
  "checkout_path": "...",
  "head_sha": "...",
  "dirty_hash": "...",
  "subcommand": "test",
  "builder": "...",
  "log": "...",
  "totals": {"artifacts": N, "bins": B, "libs": L, "passed": P, "failed": 0},
  "artifacts": [
    {"path": "...", "kind": "bin", "tests_run": n, "passed": p, "failed": 0, "sha256": "..."},
    ...
  ]
}
```
Record source SHA + per-binary counts + binary sha256 for every verification run.

## Scope guard

Do not edit docs/PROJECT_STATE.json, docs/NEXT_ACTION.md, docs/ACCEPTANCE.md,
docs/historical-migration/PROGRESS.md, docs/historical-migration/STATUS.md, or
docs/IMPLEMENTATION_QUEUE.md — owned by the batched reconciliation card
t_c2b3f167. This diff is limited to scripts/, the new runbook, and build env.
