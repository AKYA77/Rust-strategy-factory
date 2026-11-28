# Restore-cutover parallel flock flake — RED/GREEN evidence (2026-11-28)

## Scope
Test-harness-only fix. No product behaviour, financial rule, scientific gate,
provenance rule, data scope or security control changed. Class remains
REAL_STRUCTURALLY_VALID with eligibility none; no research qualification.

## Trigger: hosted CI failure on exact HEAD 96d817e
- Commit: `96d817e3133879cd2df33b1fb8e0a3953929ae29` (docs-only CI record for
  `cd8ee81`), clean `main`, no other cargo/rustc worker, heavy lock held.
- Run **46091460854** (`push`, `main`, head_sha exactly `96d817e...`): terminal
  `completed` / `failure`, created 2026-11-28T01:07:05Z, concluded 01:25:19Z.
- 6/7 jobs `success` (full-horizon, dependency-audit, check-operations,
  release-build, data-compute-identity, browser).
- `rust-checks` job 140813099183: steps `cargo fmt --check` success,
  `cargo test --locked --lib --bins --tests` **failure** (exit 101).
- Failing test: `tests/restore_cutover.rs::restore_cutover_fences_copied_actors_and_revives_on_restored_copy`,
  binary 2 passed / 1 failed, panic at `tests/restore_cutover.rs:69:46`:
  `called Result::unwrap() on an Err value: "another recovery operation is active"`.
- No in-progress run was called a pass.

## Local baseline on the same exact HEAD (before fix)
`bash scripts/check.sh` with `RSF_BUILDER_PROFILE=hermes-v2-m1` and
`CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup PATH=/workspace/.cargo/bin:$PATH`:
**EXIT=0, 765 passed / 0 failed / 0 ignored across 72 test binaries** (lib 278),
zero named failures, fmt and `clippy -D warnings` clean; 03:09:42Z–03:19:07Z.
The head-provenance check forced a project-only rebuild first (9013 files, 3.0 GiB),
no source changed. The green local run and the red hosted run are both terminal
facts: the defect is load-dependent, not a different tree.

## Reproduction and root cause (RED)
Focused target run from the checked-out binary, 3 tests in parallel:
- 4 failures / 60 runs (plain binary).
- 8 failures / 120 runs with an `LD_PRELOAD` `flock` interceptor logging errno.
- 0 failures / 40 runs with `--test-threads=1` (serial) — proves load dependence.
- Interceptor evidence: `flock(fd, LOCK_EX|LOCK_NB)` returned `errno=11`
  (`EWOULDBLOCK`) on the *same* `<db>.recovery/.operations.lock` path, same PID,
  immediately after a successful acquire on the same fd number. The failing
  panic appears at line 62 or 69 depending on which sequential lock call lands
  in the window.
- Cause: `cli_restore_cutover_registry_check_passes_on_restored_copy` forks
  `Command::new(binary)` children while the two library tests hold the exclusive
  `.operations.lock` flock. A forked child inherits the lock's open file
  description until `exec` closes its CLOEXEC descriptors; a concurrent library
  call then sees a spurious `EWOULDBLOCK`, which `recovery::lock` correctly
  reports as `another recovery operation is active`. The product fail-fast
  conflict contract and the dashboard 409 CONFLICT semantics are NOT the bug and
  were not changed.

## Fix (GREEN)
`tests/restore_cutover.rs` only: process-local `static Mutex<()>` acquired at the
start of all three tests in that binary, with poison-tolerant recovery. This
serializes the fork window without changing product code.
- `cargo test --locked --test restore_cutover`: **3 passed / 0 failed**.
- Same 150-run and 80-run repeated parallel loops with the same interceptor:
  **0 failures / 150** and **0 failures / 80**, with **0 `errno=11` flock events**.
- Full verification ladder on the fixed tree: `bash scripts/check.sh`
  (**EXIT=0, 765 passed / 0 failed / 0 ignored across 72 test binaries**, lib 278,
  zero named failures, fmt and `clippy -D warnings` clean; 03:27:46Z–03:37:00Z).
  Test count is unchanged because the fix serializes existing tests rather than
  adding a new one; the regression evidence is the RED→GREEN repetition count
  above, matching the adaptive-fixture precedent in this repository.
- Hosted CI on the pushed fix commit and the subsequent CI-record commit are
  recorded in `docs/NEXT_ACTION.md` and `docs/IMPLEMENTATION_QUEUE.md`.
