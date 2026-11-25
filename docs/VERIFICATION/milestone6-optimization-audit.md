# Milestone 6 — optimization/cleanup: measured BASELINE, exact parity, dead-code and dependency audit

Date: 2026-11-25 (UTC). Repo: `/root/workspace/Rust-strategy-factory`, branch `main`, exact HEAD
`98cdc7921cc747366c83262003404c8375c01f7c`.

## 1. Scope and honesty statement

**No optimization change was made in this task.** Therefore there is no corrected-reference-vs-candidate
speedup to report, and none is claimed. The "reference" and the "candidate" in the parity comparison are
**the same binary** (identical SHA) run twice on the same workload with two fresh registries. The
deliverable is an honest baseline plus exact-parity evidence, as instructed.

The workload is a **synthetic fixture**, not the full FX corpus. The acceptance text's "same full-corpus
workload" condition is therefore **not satisfied by this receipt**: no full-corpus optimization A/B was
executed here, and no full-corpus speedup figure exists. Every number below is an actually observed
measurement; derived rates are labelled as arithmetic extrapolation from a short run.

No commit, push, `src/` edit, test edit, `scripts/browser*.cjs` edit or `docs/NEXT_ACTION.md` edit was made
by this task. The three pre-existing dirty files (`docs/NEXT_ACTION.md`, `scripts/browser-check.cjs`,
`scripts/browser-long-ledger-check.cjs`) were already modified when the task began and remain untouched.

## 2. Environment, hardware, compiler, provenance

| Item | Observed value |
|---|---|
| Kernel / arch | `Linux vmi3658677 6.8.0-149-generic #149-Ubuntu SMP PREEMPT_DYNAMIC Fri Oct 23 13:21:10 UTC 2026 x86_64` |
| CPU | 6 vCPU, `AMD EPYC Processor (with IBPB)`, QEMU/KVM, 2.0 GHz base, 8 MiB L3, SMT disabled |
| Memory | 11 GiB total, ~8.0 GiB available at measurement time, 0 swap |
| Disk | 96 G total; 77 G used / 20 G available at start (81%), 75 G / 22 G (78%) later |
| rustc | `rustc 1.100.0 (0ef1bc386 2026-11-09)` |
| cargo | `cargo 1.100.0 (3acc79c0c 2026-11-05)` |
| Toolchain | `1.100.0-x86_64-unknown-linux-gnu`, overridden by `rust-toolchain.toml` (`channel = "1.100.0"`, minimal + rustfmt + clippy) |
| Env | `CARGO_HOME=/workspace/.cargo`, `RUSTUP_HOME=/workspace/.rustup`, `RSF_BUILDER_PROFILE=hermes-v2-m1` |
| Exact SHA | `98cdc7921cc747366c83262003404c8375c01f7c` |
| Cargo.toml SHA-256 | `d6f42d8d80578b945c7be4a576029cbb3c8e5284bfcfd65f1c125aad0e3e716b` |
| Cargo.lock SHA-256 | `e5308e42eb38c93a8fa5e65dc58b8d990bc5a23ef13e14069fd109a1b24cea52` |
| GNU time | `/usr/bin/time` (GNU Time, reports wall/user/system/RSS/faults/FS I/O/context switches) |

### Isolation and source provenance (important)

A concurrent verification flow was editing `src/lib.rs`, adding uncommitted `src/prep_cache.rs` /
`tests/prep_cache.rs`, and rebuilding the shared `target/debug/rust-strategy-factory` during this task
(shared binary SHA changed `0913ab49…` → `986fc45e…` → `6962e5fb…` without any command from this task).
Consequently **all measurements used by this receipt were taken from an isolated snapshot**:

```
$ git archive 98cdc79... | tar -x -C /root/.hermes/cache/scratch/m6-head
  521 tracked files, 0 missing, snapshot_tree_sha256=46c69acdb9ff0d9d4af93931fb8f9994e5917dbee1a1b9a1652d7f48707a33f1
  Cargo.toml / Cargo.lock SHA-256 identical to the main checkout (values above)
$ CARGO_TARGET_DIR=/root/.hermes/cache/scratch/m6-target cargo build --locked
```

The snapshot received a synthetic git commit (`1a98ae868e3ee3334571a0d2989753b739c75554`) solely so
`scripts/check.sh`'s `cargo-locked.sh` wrapper (which requires a git checkout) could run. Source bytes are
identical to exact HEAD; the synthetic commit SHA is **not** the source SHA. The repo's own
`target/debug/` was never deleted and was not used for evidence after isolation was detected.

The first shared-target build (`milestone6-build-cold.log`, `milestone6-build-cold.time`) is recorded but
**VOID for binary identity**: the wrapper forced `cargo clean -p` (stamp `head_sha` mismatch) and the
binary was subsequently replaced by the concurrent flow. Its raw numbers: EXIT=0, wall `1:26.96`,
user `31.99 s`, system `5.05 s`, RSS `1,300,052 kB`, FS inputs `2,808`, FS outputs `1,038,608`; it also
showed one `unused_mut` warning in the concurrent, uncommitted `src/prep_cache.rs:286`, which does not
exist in exact HEAD.

## 3. Build — cold and warm (isolated target, exact HEAD)

| Command | Exit | Wall | User CPU | Sys CPU | Max RSS | FS in/out |
|---|---:|---:|---:|---:|---:|---:|
| `cargo build --locked` (cold, dependencies compiled) | 0 | `1:31.30` | `259.28 s` | `23.29 s` | `1,331,704 kB` | `131,928` / `1,924,368` |
| `cargo build --locked` (warm, no source change) | 0 | `0:00.17` | `0.06 s` | `0.07 s` | `41,472 kB` | `2,896` / `40` |

Cold log ends `Finished 'dev' profile ... in 1m 31s`; warm log: `Finished ... in 0.13s`. Cold binary SHA
at that moment `d4440ef394942617ff69217f3279a4e1e3847d03ea7eaf400740a6869152b2c7`; the binary was later
rebuilt by `scripts/check.sh` in the same isolated target to
`39ece2dd091d679c4dd55ef873677fe55703908e5c6b2f0ad01ec09496d90e9d` (the SHA actually used for the
research workload). Zero compiler warnings on the clean HEAD build.

## 4. Dead-code audit — clippy `-D warnings`

```
$ cargo clippy --locked --all-targets -- -D warnings
EXIT=0
wall 0:23.84; user 55.37 s; system 12.52 s; max RSS 1,062,180 kB
warning/error lines: 0
Finished `dev` profile [unoptimized + debuginfo] target(s) in 23.74s
```
`milestone6-clippy.log` contains the complete, unedited output. At exact HEAD, clippy with `-D warnings`
is clean: no dead-code, unused-item or any other warning was promoted to an error. `grep -c
'allow(dead_code)' src` returned `3` (pre-existing, documented suppressions in source; not modified).

## 5. Full local check ladder — `scripts/check.sh`

First attempt in the non-git archive returned `EXIT=128` with no output: the wrapper's `dirty_hash` uses
`git ls-files` under `set -o pipefail`, which fails outside a git checkout. This is an environment issue,
not a source failure. After the synthetic git init described in §2, the real ladder ran:

```
$ sh scripts/check.sh          # fmt --check; clippy -D warnings; test --lib --bins --tests
START_UTC=2026-11-25T01:12:34Z  END_UTC=2026-11-25T01:20:58Z
EXIT=0
wall 8:24.40; user 885.70 s; system 197.31 s; max RSS 1,581,776 kB
FS inputs 17,709,112; FS outputs 8,587,320
68 "test result" blocks: 723 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; all blocks "ok"
```
Raw log: `milestone6-check-sh.log` (81,211 bytes). No test or source file was edited to obtain this.

## 6. Dependency review

Raw output: `milestone6-dependency-review.txt`.

| Command | Exit | Result |
|---|---:|---|
| `cargo tree --locked --edges normal,build,dev --prefix none` | 0 | 226 package lines, 106 unique crate names |
| `cargo tree --locked -d --prefix none` | 0 | 63 lines, 10 duplicate-version groups: `block-buffer`, `cpufeatures`, `crypto-common`, `digest`, `getrandom` (3 entries), `hashbrown`, `r-efi`, `syn`, `zstd`, `zstd-safe` |
| `cargo audit --ignore RUSTSEC-2026-0391 --ignore RUSTSEC-2026-0459 --ignore RUSTSEC-2026-0543` (identical to CI) | 0 | 1,567 advisories loaded, 145 crates scanned, **1 allowed warning**: `RUSTSEC-2026-0595`, `parquet 60.1.0`, unsound (Bloom filters written as plaintext when writing encrypted Parquet; fixed in >=60.2.0). `warning: 1 allowed warning found` — not a vulnerability error |
| `cargo deny check` | 0 | `advisories ok, bans ok, licenses ok, sources ok`; 10 duplicate warnings (same groups as above) |
| `cargo audit --version` / `cargo deny --version` | 0 / 0 | `cargo-audit-audit 0.23.0` / `cargo-deny 0.22.0` |

The pre-existing dependency-audit CI job status is reported in §7. The dependency review found no new
blocking advisory; the duplicated transitive crates are warnings already configured as such in
`deny.toml`.

## 7. Hosted CI status — honest, on the exact SHA

`gh run list --branch main` and `gh run view <id> --json jobs` (raw in `milestone6-ci-status.txt`):

* First query of run `45521997695` (SHA `98cdc79…`) showed `status=in_progress` — not a result.
  Re-queried after conclusion: **`status=completed`, `conclusion=failure`**.
  Jobs: `rust-checks success`, `dependency-audit success`, `check-operations success`,
  `data-compute-identity success`, `full-horizon success`, `release-build success`,
  **`browser failure`**. The overall run is red solely because of `browser`; failure excerpt:
  `AssertionError ... assert(proofReceipt.result.screen.signals>0)`.
* Prior main run `45519520343` (SHA `57c13fc…`): `completed`, `failure`; same shape — `browser
  failure`, other six jobs `success` including `dependency-audit`.

Therefore: **hosted CI is NOT green at exact HEAD**, and "all check/CI suites green" is not claimed.
The local ladder (fmt/clippy/tests) and the local dependency audit are green; the browser job is red on
the exact SHA. The browser fixtures are also dirty in this shared checkout, so a local browser rerun
would not test HEAD state; it was not run here.

## 8. Baseline research workload — measured, two identical passes

Reproducer: `scripts/profile-milestone6.sh` (new file), invoked as

```
BIN=/root/.hermes/cache/scratch/m6-target/debug/rust-strategy-factory \
OUT=/root/workspace/Rust-strategy-factory/docs/VERIFICATION \
RUN_TAG=baseline STEPS=200 bash scripts/profile-milestone6.sh
```

Workload: CLI-generated `fixture EURUSD` (2,000 synthetic ticks, `SYNTHETIC_TEST_ONLY`) packed with
`pack-native` to `.rsf`; one input, `seed=42`, `max_regions=20`, `feature_cells=1,000,000`,
`state_budget_bytes=268,435,456`, `proof_policy={fixed-market-proof-screen-v1, audit_bps=1000, seed=42}`,
`improvement_children=0`. No training window (synthetic-only input). Fresh registry per pass.

* fixture SHA-256 `8b69c11d220a39f270d56441e0b1f2aadc7d9167c152cf1b8cb4c19983e15bb1`
* dataset SHA-256 `5e7fab15ec798db54421dab4135e70421a2d0295ec7107017271440086499e01`
* config SHA-256 `ef52474ffddabe172db6b8a6aa17bf60ec400a662350821a1bac6aebd5444090`
* binary SHA-256 `39ece2dd091d679c4dd55ef873677fe55703908e5c6b2f0ad01ec09496d90e9d`

| Metric (raw GNU time / CLI) | Pass A | Pass B |
|---|---:|---:|
| Exit code | 0 | 0 |
| Run ID (deterministic, config+identity+revision hash) | `5d290897…6a67` | `5d290897…6a67` |
| Cursor / steps executed | 200 / **199** | 200 / **199** |
| Cache reads (`reused`, not counted as completed) | 0 | 0 |
| Rejected / failed | 1 / 0 | 1 / 0 |
| Cache hit rate (`reused / (executed+reused+rejected)`) | 0.0 | 0.0 |
| Unique completed experiments/hour (arithmetic extrapolation) | 387,243.24/h | 439,509.20/h |
| Wall | `0:01.85` (1.85 s) | `0:01.63` (1.63 s) |
| User CPU / system CPU | `0.81 / 0.16 s` | `0.73 / 0.16 s` |
| Max RSS | `15,104 kB` | `14,848 kB` |
| Minor / major page faults | `1,475 / 0` | `1,469 / 0` |
| File system inputs / outputs (GNU time units) | `0 / 49,680` | `0 / 49,680` |
| Voluntary / involuntary context switches | `3,253 / 0` | `3,262 / 1` |
| Registry bytes (`.sqlite`; `-wal`/`-shm` absent after clean close) | `1,826,816` | `1,826,816` |
| Events `claimed / completed / rejected` | `200 / 199 / 1` | `200 / 199 / 1` |
| Screening contradictions (`proof-audit-contradiction-v1`, `indexed-audit-contradiction-v1`) | 0 | 0 |
| `proof_rejection=true` experiments | 1 | 1 |
| `failed=0`, `last_error=null`, `recovery-list` rows | 0 / null / `[]` | 0 / null / `[]` |
| Canonical registry projection SHA-256 / bytes | `8374878e…8e87db` / `1,368,202` | `8374878e…8e87db` / `1,368,202` |

**Exact parity: TRUE.** The canonical projection covers every non-volatile column of `experiments`,
`events`, `telemetry_events`, `research_steps`, `stage_transitions`, `result_artifacts` and
`research_runs` (volatile lease/wall-clock columns `token`, `expires_ms`, `at_ms` excluded); both passes
produced byte-identical canonical JSON and the same final `latest` experiment ID
`fecd5d591ebc71cc2f4b978a0535442099f1c434835d82ce0ce37b8c7a7a8f62`. The raw SQLite files are not
byte-identical (SQLite page/header metadata), so parity is asserted on the deterministic projection,
not on incidental file bytes.

Conditions: two **cold** runs (fresh registry each time), same binary, same config bytes, same input
bytes. There is no warm/cache-hit comparison pass: `reused=0` in both passes, so the cache-hit rate is
genuinely 0% and cannot be reported as a speedup. No real corpus, no cache prewarm, no optimization
candidate. The per-hour figures are `199 / 1.85 s * 3600` and `199 / 1.63 s * 3600`; a ~1.8 s synthetic
run is **not** a stable performance benchmark and the A/B difference (0.22 s) is not a measured effect.

## 9. Files written (all new; no existing file modified by this task)

Under `docs/VERIFICATION/`: `milestone6-preflight.txt`, `milestone6-snapshot-identity.txt`,
`milestone6-build-cold.log/.time` (VOID, contaminated), `milestone6-isolated-build-cold.log/.time`,
`milestone6-isolated-build-warm.log/.time`, `milestone6-clippy.log/.time`,
`milestone6-check-sh.log/.time`, `milestone6-dependency-review.txt`, `milestone6-ci-status.txt`,
`milestone6-research-baseline-{A,B}.{time,out,err,exit,canonical.json,recovery.json}`,
`milestone6-research-baseline-fixture.out`, `milestone6-research-baseline-summary.json`,
and this file. New script: `scripts/profile-milestone6.sh`.

Ephemeral experiment data (registries, fixture, isolated target) lives outside the repo under
`/root/.hermes/cache/scratch/m6-*`; those paths are recorded in the raw artifacts above. Nothing was
committed or pushed.

## 10. Honest limits / unresolved blockers

* No full-corpus workload and no optimization candidate → this receipt cannot satisfy a measured
  speedup claim; it supplies baseline and exact-parity evidence only.
* Hosted CI overall `failure` at exact HEAD (`browser` job); not green. `dependency-audit` is green.
* Real-data suites inside `check.sh` were executed locally in the isolated snapshot (723 passed total);
  the full-horizon / data-compute-identity / check-operations / release-build jobs are evidenced only by
  their concluded hosted-CI `success` on this SHA, not by separate local reruns in this task.
* Shared-checkout concurrency (other agent editing `src/`, adding milestone-7 files and rebuilding the
  shared binary) means the repo's `target/debug` SHA is outside this task's control; all evidence above
  is pinned to the isolated snapshot and to the SHAs stated per section.
