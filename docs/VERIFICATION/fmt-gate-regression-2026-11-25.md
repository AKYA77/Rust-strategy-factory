# Formatting-gate regression — mandatory local ladder red on clean `main`

Date: 2026-11-25 (UTC). Baseline revision: `f8e4eafbd56a344ce17a29eebb4c1a215137ea85`
(`git rev-parse HEAD` == `git rev-parse origin/main`, clean worktree, branch `main`).

Scope discipline: this is a verification-gate correctness fix. No financial, provenance,
scientific, security or data-scope rule was changed. No corpus was copied or modified; no
production/service change, install, activation or reboot was performed. No V2 scientific
qualification is claimed.

## 1. Hard guards (before any action)

| Guard | Observed |
|---|---|
| `pgrep -af 'cargo\|rustc'` | no cargo/rustc processes (only this shell) |
| `git status --porcelain` | empty |
| `flock -n /root/workspace/.rsf-heavy.lock true` | `LOCK_ACQUIRED` |
| branch / HEAD | `main` / `f8e4eafbd56a344ce17a29eebb4c1a215137ea85` |
| other editing agents | none (only webui, gateway, codebase-memory and cron daemons) |

## 2. Failing before (exact, reproduced)

Command (env: `CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
PATH=/workspace/.cargo/bin:$PATH RSF_BUILDER_PROFILE=hermes-v2-m1`):

```
$ sh scripts/check.sh
# cargo-locked.sh fmt --all -- --check
Diff in /root/workspace/Rust-strategy-factory/src/research.rs:1474:
-                let value: serde_json::Value = prep_cache::decode(
-                    &hit.data,
-                    &hit.receipt.content_hash,
-                )?;
+                let value: serde_json::Value =
+                    prep_cache::decode(&hit.data, &hit.receipt.content_hash)?;
CHECK_EXIT=1
```

Failing layer: `cargo fmt --all -- --check` (first step of `scripts/check.sh`). No test
binaries ran; there were no failing test *names*. `git blame` attributes both unformatted
lines to commit `2177da0` ("Fix prep cache budget crash and tolerate cache exhaustion in
research Driver", 2026-11-25 15:28 CET). `rustfmt 1.11.0-stable (0ef1bc3860 2026-11-09)`
from the pinned toolchain.

Root cause of CI green on the same SHA: `.github/workflows/ci.yml` `rust-checks` ran only
`cargo test --locked --lib --bins --tests`. It had **no** `cargo fmt --all -- --check` step,
so a formatting regression that breaks the mandatory local push gate still produced a green
hosted run (`45634655873`, `f8e4eaf`, terminal `success`, all jobs). This receipt records
that discrepancy rather than treating green CI as proof the local gate was clean.

## 3. Fix

1. `src/research.rs` — applied the exact rustfmt rewrite at lines 1474–1478 (whitespace /
   line wrapping only; no semantic change to the prep-cache decode path).
2. `.github/workflows/ci.yml` — added `cargo fmt --all -- --check` to `rust-checks` before
   `cargo test`, with a comment recording the regression. Hosted CI now enforces the same
   format gate as `scripts/check.sh`.

Changed paths: `src/research.rs`, `.github/workflows/ci.yml`, this receipt, and the three
state ledgers (`docs/NEXT_ACTION.md`, `docs/PROJECT_STATE.json`,
`docs/IMPLEMENTATION_QUEUE.md`).

## 4. Passing after (exact, reproduced on the corrected tree)

```
$ scripts/cargo-locked.sh fmt --all                  # EXIT=0; only src/research.rs changed
$ scripts/cargo-locked.sh fmt --all -- --check       # EXIT=0
$ sh scripts/check.sh                                # EXIT=0
```

`scripts/check.sh` result on the corrected source tree (with the CI workflow edit present):

- `cargo fmt --all -- --check`: EXIT=0
- `cargo clippy --locked --all-targets -- -D warnings`: EXIT=0
- `cargo test --locked --lib --bins --tests`: EXIT=0
- **744 passed / 0 failed / 0 ignored across 69 test binaries** (lib `276 passed`; `main`
  unit-test binary has 0 tests). Zero `FAILED` lines in the full log.
- Ladder spent ~2 min forcing `cargo clean -p rust-strategy-factory` because the HEAD/dirty
  provenance stamp changed (19640 files / 3.6 GiB of project artifacts only); dependency
  cache stayed warm. Times: dev compile 19.06 s, test compile 1 m 30 s; total wall ~4 m.
- Full raw log: `/root/.hermes/cache/scratch/rsf-check-2026-11-25-1555.log` (scratch, not
  committed); exit code file `...-1555.exit` contains `0`.

Ledger correction: the prior `PROJECT_STATE.json` implemented entry claimed
"746 passed / 0 failed over 70 binaries"; the actual command output above is **744 / 69**.
Counts are corrected in this change rather than carrying forward the unverified figure.

## 5. Scope and non-claims

- The fix is a formatting repair plus a CI gate. It does not change any trading rule,
  cost model, data class, scientific gate, V2 scope (seven FX instruments), or the 22-month
  continuous accounting contract.
- No browser suite was needed: no grammar, enumerator, or `scripts/browser-*.cjs` file changed.
- This is **not** Milestone 7 acceptance. Milestone 7 remains NOT accepted: authenticated
  dashboard endpoints (no saved credentials) and a fresh owner-executed reboot are external
  evidence gates. See `docs/VERIFICATION/milestone7-vps-acceptance.md` §7.
- Percentages are not claimed here; see the final report for the rubric-based status.
- Hosted CI on the pushed commit `e92934ce57a4e84654f5f0a2df4275557c60aa30`: run
**45640484836** reached terminal `completed` / `success` on all 7 jobs
(release-build, browser, rust-checks, data-compute-identity, full-horizon,
check-operations, dependency-audit). `rust-checks` includes the newly added format
step and passed. Recorded only after the run terminated; no `in_progress` job was
called a pass.
