# Runbook: cross-worktree cargo artifact isolation (shared `CARGO_TARGET_DIR`)

Status: **enforced** by `scripts/cargo-locked.sh` (card `t_f368fccb`, commander infra
decision `t_2eb2e0d6`). This runbook is the authoritative description of the hazard and
the machine-enforced protocol. Hosted CI is unaffected (clean per-checkout environment).

## 1. The hazard

All rsf lanes (the main checkout `/root/workspace/Rust-strategy-factory` and every
`rsf-wt-*` worktree) share **one** `CARGO_TARGET_DIR`:

```
CARGO_TARGET_DIR=/root/workspace/Rust-strategy-factory/target
```

The project's lib / bin / test artifacts are written under

```
target/debug/build/rust-strategy-factory/<hash>/out/<name>-<hash>
```

(each `<hash>` dir holds its own `fingerprint/` and `out/`). Cargo's metadata `<hash>`
for this package is **checkout-independent** — it does not encode the workspace root
path — so the *same* hash dir is used by every checkout. Consequences:

- A `cargo test`/`build` in worktree **A** overwrites the artifacts in that hash dir.
- A later ladder in worktree **B** can see a matching fingerprint (mtime/content
  coincidence across checkouts) and **reuse A's foreign binaries** while reporting
  green ("Finished in 0.20s").

**Observed incident (rsf-qa acceptance sweep `t_e0f73db5`, 2026-11-19, main `ee5d08a`):**
main's `scripts/check.sh` ran the **dashboard** branch's artifacts — **529 passed / 159
lib** — instead of main's **518 passed / 54 binaries / 148 lib**; a data-compute-identity
round-trip also failed in the same contamination window. Evidence:
`/root/workspace/rsf-audit/ACCEPTANCE_SWEEP_2026-11-19.md` (Findings F1/F2) and
`/root/.hermes/profiles/rsf-qa/cache/scratch/rsf-sweep-2026-11-19/`. Detection is by
per-binary test counts vs the expected signature + binary `sha256` + source SHA.

## 2. The decision (binding)

**Keep the single shared target.** Do **not** duplicate the dependency cache per lane.
Make cross-checkout artifact reuse *impossible* by machine-enforcing a locked,
provenance-stamped forced-rebuild protocol for **every** cargo invocation.

### Rejected alternatives

- **(a) Per-worktree full `CARGO_TARGET_DIR`.** Disk-prohibitive: the volume is ~77%
  full (~23 GiB free) and one target is ~7.8 GiB; five lanes ≈ 39 GiB will not fit.
  Cold caches also cost ~20 min/lane. Rejected.
- **(b) Path-dependent build hashes** via `RUSTFLAGS`/`-C metadata` or a `rustc`
  wrapper. Risks duplicating the dependency cache and/or fragile surgery on
  cargo-1.100 internals. May be revisited **only as a later optimization** if the
  forced-rebuild cost proves prohibitive, and only against the same acceptance repro
  (§5). Rejected for now.

No test or gate is weakened to accommodate the protocol.

## 3. The enforced protocol

`scripts/cargo-locked.sh` is the **single chokepoint**. For every invocation it:

1. **Acquires the team-wide heavy lock** `flock -w 3600 /root/workspace/.rsf-heavy.lock`
   (one heavy job team-wide; `RSF_LOCK_WAIT` raises the wait when queuing behind a long
   ladder). Off the VPS (e.g. hosted CI, where that path's directory
   does not exist) it falls back to a per-target lock so CI keeps working.
2. **Provenance stamp.** For build-like commands (`build|test|clippy|check|run|bench|
   doc|rustc|install`) it reads `target/.rsf-build-stamp.json`, which records the last
   builder's `checkout_path` + git `head_sha` + `dirty_hash` (a content hash of
   `git diff HEAD` plus build-relevant untracked files). Because `dirty_hash` binds
   tracked edits, `Cargo.toml` (including `[profile.*]` opt/debug settings),
   `Cargo.lock`, `build.rs` and source changes — committed (`head_sha`) or in the
   working tree (`dirty_hash`) — all invalidate the stamp; the identity is
   content-based, not keyed on profile name. If the stamp is absent or any
   field differs from the current checkout/source, it runs
   **`cargo clean -p rust-strategy-factory` inside the lock** — project artifacts only;
   the dependency cache stays warm. This preserves within-lane incremental caching
   while guaranteeing no cross-lane reuse. The dirty hash is conservative: any
   uncertainty forces a rebuild, never a reuse.
3. **Runs the real cargo** command, resolved via `rustup which cargo` (the toolchain
   binary — never a PATH shim, so there is no recursion).
4. **Records provenance.** On success it writes the stamp and appends a line to
   `target/.rsf-provenance.jsonl` (checkout, SHA, dirty hash, command, exit code,
   whether a clean was forced, log path). On failure it **invalidates** the stamp so the
   next invocation rebuilds. `cargo clean` also invalidates the stamp. Non-build
   commands (`fmt`, `metadata`, …) still take the lock but do not touch the stamp.

Every invocation's output is tee'd to `target/.rsf-logs/<ts>-<subcmd>.<pid>.log`.

### Verification receipts

Set `RSF_RECEIPT_DIR=<dir>` (and optionally `RSF_RECEIPT_LABEL=<stem>`) to have a
successful `test`/`build` run also write `<dir>/<label>.json` via
`scripts/rsf_cargo_receipt.py`: **per-binary test counts, each test binary's `sha256`,
and the source SHA / checkout / dirty-tree hash**. This is the provenance required for
any verification run (per-binary counts + binary sha256 + source SHA).

## 4. How to run cargo (no bypass)

- **Push gate / CI:** `sh scripts/check.sh` — already wired; every cargo call inside it
  goes through the wrapper.
- **Ad-hoc cargo:** call the wrapper directly with the exact cargo args:
  ```sh
  scripts/cargo-locked.sh test --locked --lib --bins --tests
  scripts/cargo-locked.sh clippy --locked --all-targets -- -D warnings
  scripts/cargo-locked.sh build --locked --release
  ```
- **Make bare `cargo` go through the wrapper:** source the env activator (bash), which
  puts `scripts/shim` first on `PATH`. The shim resolves the *current checkout's*
  wrapper via `git rev-parse --show-toplevel`:
  ```sh
  . scripts/cargo-env.sh
  cargo test --locked --lib --bins --tests   # -> scripts/cargo-locked.sh
  ```
  `scripts/shim/cargo` refuses to run if the checkout lacks the wrapper. The only way
  to bypass the protocol is to invoke the rustup/toolchain cargo binary directly; that
  is a protocol violation and must not be done against the shared target.

Resource rules on the VPS: `CARGO_HOME=/workspace/.cargo`,
`RUSTUP_HOME=/workspace/.rustup`, `PATH="$CARGO_HOME/bin:$PATH"`, `cargo -j 4`
(set `CARGO_BUILD_JOBS=4`), python as `env -u PYTHONPATH`, one heavy job team-wide,
release binary for heavy real-data stages, never `pkill -f` (kill by explicit PID).

## 5. The cross-worktree repro (independently re-runnable)

`scripts/repro-cargo-isolation.sh` proves the protocol against the shared target:

1. **Phase 1 (foreign):** `git worktree add --detach` a throwaway worktree on a distinct
   commit (`FOREIGN_REF`, default `8b01418` — an ancestor of main with identical
   `Cargo.lock` and minimal real-data tests, so the foreign phase is fast). Build + run
   its full test suite **through the wrapper** into the shared target; record per-binary
   counts + binary sha256 + source SHA (`foreign.json`).
2. **Phase 2 (main):** run a main-source verification **through the wrapper** — the
   wired `scripts/check.sh` (fmt + clippy + full `cargo test`). The wrapper detects the
   foreign stamp, **forces `cargo clean -p`**, rebuilds, and reports main's signature
   (`main.json`).
3. **Phase 3:** assert the forced clean happened, the stamps record the right checkouts,
   main's signature matches (`EXPECT_BINARIES/EXPECT_PASSED/EXPECT_LIB`, default
   **54 / 518 / 148** at `ee5d08a`), and main's binary sha256 is **distinct** from the
   foreign build's. Writes `verdict.json` / `verdict.md`.

Run it (holds the heavy lock via the wrapper for each phase):

```sh
scripts/repro-cargo-isolation.sh
# pre-merge, point MAIN_CHECKOUT at the branch carrying the wrapper:
MAIN_CHECKOUT=/root/workspace/rsf-wt-rust scripts/repro-cargo-isolation.sh
# reproduce a specific incident with a different foreign commit:
FOREIGN_REF=<dashboard-head-sha> scripts/repro-cargo-isolation.sh
```

Receipts land under `/root/workspace/rsf-audit/cargo-isolation-repro-<ts>/`. The foreign
worktree is removed on exit. rsf-qa re-runs this in review before approving.

## 6. Hosted CI behavior

CI has a clean per-checkout target (`CARGO_TARGET_DIR` unset → `./target`), python3, and
rustup. The wrapper falls back to a per-target lock, and the stamp matches within the
single CI checkout, so no extra rebuilds are introduced; the one initial
`cargo clean -p` on a fresh target is harmless. Bare cargo steps in `ci.yml`
(`cargo test --doc`, `cargo build`) are safe in CI's isolated environment.

## 7. Interim protocol (superseded by this wrapper)

Until this change landed, the standing rule was: every cargo under
`flock /root/workspace/.rsf-heavy.lock`; `cargo clean -p rust-strategy-factory` inside
the lock whenever another checkout may have built; receipts with per-binary counts +
binary sha256 + source SHA. The wrapper now **machine-enforces** exactly that, so the
manual discipline is no longer the only line of defense.