# Verification throughput (quality-neutral)

Card t_72dd165f, PR #21. Purpose: cut verification wall time for every lane without changing
any test, gate, financial contract or source semantic. Adopted only with measured
before/after evidence and identical receipts; anything not provably quality-neutral or not
provably faster is SKIPped below with the reason.

## What changed

### Local profiles (Cargo.toml, automatic for every checkout on this branch)

| Setting | Value | Effect |
|---|---|---|
| `[profile.dev] debug` | `line-tables-only` | smaller artifacts, faster links; backtraces keep crate line info |
| `[profile.dev] split-debuginfo` | `unpacked` | debug info out of the linked binary |
| `[profile.dev.package."*"]` | `opt-level=2`, `debug=false` | dependencies optimized once and cached; inherited by the test profile |
| `[profile.dev.build-override]` | `opt-level=3` | workspace build script/proc-macros faster at execution time. Dep build scripts (incl. bundled SQLite and zstd C compilation via cc) take opt-level=2 from the package wildcard, which takes precedence for non-workspace packages — still optimized vs the previous -O0 |
| `[profile.test]` | `opt-level=1` | test code (lib/bin/integration harnesses) optimized; the CPU-bound real-data suites are the dominant local cost |

Rust has no fast-math; exact integer accounting is unchanged at every opt-level. Release
profile is untouched (`lto=thin`, `codegen-units=1`). Verified from actual rustc invocations:
deps (serde/tokio/rusqlite/libsqlite3_sys) at `-C opt-level=2`, workspace build script at
`-C opt-level=3`, crate debuginfo `line-tables-only` (receipt `opt_level_evidence*`).

### Hosted CI (.github/workflows/ci.yml)

- **Three parallel required jobs** (admin lever 2026-11-20): `rust-checks`
  (fmt/clippy/check.sh/doc-tests), `browser` (node/playwright + all Chromium suites),
  `release-acceptance` (debug+release builds, compute identity, operations, full-horizon).
  Every previous stage/assertion preserved; wall time = max(jobs) instead of sum. All three
  are required for merge: branch-protection required status checks + QA verifies the
  complete rollup on the exact SHA.
- `Swatinem/rust-cache@v2` per job (job-scoped keys, no parallel save races): restores the
  cargo registry and target artifacts keyed on lockfile/rustc; dependency recompilation
  disappears on warm runs.
- `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`:
  CI artifacts are cached whole; no debug info in CI (panic locations and symbol names remain).
- **mold: evaluated, SKIPped** — see the link A/B below. The rustc-1.100 default linker
  (rust-lld 23.1.5) already matches mold 2.30 on this workload; no rustflags or extra apt
  dependency are added.

## Compute identity

`build.rs` binds blake3 over 75 enumerated source files (path+bytes, including Cargo.toml)
plus `TARGET`, `PROFILE`, `CARGO_CFG_TARGET_FEATURE`, `CARGO_ENCODED_RUSTFLAGS` and
`rustc -vV`. Consequences:

- The Cargo.toml profile edit changes `compute_revision` — an **honest build-metadata
  revision change**, not a semantic change:
  `compute-blake3:3b7b85d9…` (ee5d08a) → `compute-blake3:47d1658d…` (this branch).
  The reversible 30-probe identity check is self-relative and passes 30/30 with all
  original bytes restored (`docs/VERIFICATION/throughput-compute-identity-v1.json`);
  no test hardcodes a revision constant.
- No linker rustflags are added (mold SKIPped), so `CARGO_ENCODED_RUSTFLAGS` is unchanged
  from main. CI and local identities match (same Cargo.toml, same flags).
- Historical receipts pinned to older revisions remain valid for their source SHAs.

## Measured evidence

Receipt: `docs/VERIFICATION/verification-throughput-v1.json` (per-binary counts, binary
sha256, source SHA, stage wall times, opt-level evidence, linker `.comment` proof, CI run
JSON). Baseline: QA acceptance-sweep clean re-run on `ee5d08a` under exclusive flock,
`CARGO_BUILD_JOBS=4`, no concurrent cargo (`/root/workspace/rsf-audit/ACCEPTANCE_SWEEP_2026-11-19.md`).

### Local suite (same host, same exclusive-flock protocol)

| Measurement | Baseline ee5d08a (opt0) | This branch (opt1/opt2) | Speedup |
|---|---|---|---|
| Full suite, 54 binaries (sum of per-binary times) | 3986.68 s | 440.25 s (final config) | 9.06× |
| `scripts/check.sh` wall (fmt+clippy+suite) | 4018 s | ~456 s (suite 440.74 s + fmt/clippy ~15 s) | ~8.8× |
| historical_pipeline_real (3 tests) | 2873.76 s | 281.04 s | 10.2× |
| historical_corpus_real (3 tests) | 906.96 s | 99.45 s | 9.1× |
| historical_migration_real (1 test) | 53.82 s | 7.55 s | 7.1× |
| lib unit tests (148) | 7.66 s | 3.46 s (pre-push targeted) | 2.2× |
| corpus suite (95, incl. fleet timing tests) | 90.55 s | 30.19 s (pre-push targeted) | 3.0× |
| Full test-binary build (54, deps cached) | not separately measured | 78.24–81.57 s (two runs) | — |
| Release build | 130 s | 124.35 s | ~1× (profile unchanged) |
| Identity check (30 probes) | 289 s | 290.48 s | ~1× (91 incremental builds) |
| Browser (12 Chromium suites) | 240 s | 156.61 s | 1.5× |

**Parity: exact main signature 54 binaries / 518 passed / 0 failed / 0 ignored / lib 148**,
verified twice on this branch: once on default-linker binaries (412.18 s) and once on
mold-linked binaries during the mold evaluation (440.74 s). Per-binary rows and sha256 in
the receipt. Pre-push targeted gate: 293/293 (lib 148 + corpus 95 + core 16 + storage 27 +
batch 7) with provenance at the pushed SHA.

### Link-only A/B — mold evaluated, SKIPped

Method: remove the 54 test executables, rebuild from cached artifacts (`cargo test
--no-run`), time each side; prove the actual linker via `readelf -p .comment`. The default
linker on this toolchain is **rust-lld 23.1.5** (rustc 1.100 default), not GNU ld.

| Pass | Default (rust-lld) | mold 2.30 (`mold -run`) | Note |
|---|---|---|---|
| First pass | 77.54 s | 6.29 s | INVALID for comparison: a concurrent non-flock lane build had invalidated the incremental cache, so the default side paid a full cold lib recompile (~70 s); the mold side ran warm |
| Clean re-run (concurrency-guarded, no other cargo/rustc on the box) | **6.00 s** | **6.55 s** | parity — each side: one warm incremental lib-harness compile + 54 relinks; `.comment` proves LLD 23.1.5 vs mold 2.30.0 |

**Decision: SKIP mold.** No measured gain over the default rust-lld on this workload
(small test binaries, line-tables-only debug info, rust-lld 23.1.5 already very fast).
The first-pass 12.3× figure was contamination-inflated and is retained in the receipt only
as a documented measurement error. `mold -run` remains a valid identity-neutral mechanism
(no rustflags) if a future workload shows a genuine link bottleneck; re-evaluate then with
a clean A/B. The full parity suite was also run on mold-linked binaries (440.74 s,
54/518/148) — linker choice does not affect execution semantics, as expected.

### Hosted CI (exact SHAs, concluded runs only)

| Run | SHA | Config | Wall |
|---|---|---|---|
| 44581603412 (baseline) | ee5d08a | single job, no cache | 27.75 min |
| 44656886446 (PR #21, cold cache) | 40f5711 | 3 parallel jobs + rust-cache (mold present during evaluation) | 15.37 min (rust-checks 4.22, browser 5.83, release-acceptance 15.37) |

Cold-cache parallel split alone cuts wall time ~45%. Warm-cache runs (after the first save)
remove dependency recompilation; the warm-run measurement on the evidence commit is recorded
on the card and in the batched reconciliation. `in_progress` is never a result.

## SKIPped (proof-gated decisions)

- **mold linker**: clean link-only A/B shows parity with the rustc-1.100 default rust-lld
  (6.00 s vs 6.55 s for 54 executables). No gain → not adopted; no apt dependency or
  interception complexity added. Details above.
- **cargo-nextest**: process-per-test execution defeats the process-local
  `FLEET_TIMING_LOCK` (`tests/corpus.rs:3031`, `static Mutex<()>`) that serializes the
  fleet/campaign wall-clock subprocess tests — the exact contention class diagnosed and
  fixed earlier. Under nextest each test is a separate process, the guard no longer
  serializes across tests, and the load-induced flakes return. Restoring serialization
  would require cross-process locks — an invasive test redesign, not quality-neutral.
  SKIP unless a future redesign proves identical semantics and results.
- **sccache**: rust-cache already restores compiled dependencies keyed on lockfile/rustc.
  sccache's marginal value is limited to cache-miss windows, adds RUSTC_WRAPPER machinery,
  and does not accelerate the bundled-SQLite/zstd C compilation (cc). Deferred; revisit with
  CI cache-hit telemetry if miss windows prove frequent.
- **Cranelift backend**: nightly-only; toolchain pinned 1.100.0 stable. SKIP.
- **Test-binary consolidation**: too invasive for the gain. SKIP.

## Usage and coordination

- Local: profiles apply automatically; no extra tooling needed.
- CI-first verification (owner mission §4 offload): pre-push = fmt + clippy -D warnings +
  targeted tests + provenance receipt under the flock; hosted CI runs the full clean ladder;
  QA reviews diff + CI SUCCESS on the exact SHA; full local check.sh at acceptance milestones.
- Shared-target protocol remains in force: every cargo invocation under
  `flock /root/workspace/.rsf-heavy.lock`; `cargo clean -p rust-strategy-factory` inside the
  lock when another checkout may have built; receipts carry per-binary sha256 + source SHA.
  The locked wrapper (t_f368fccb) stamp (checkout path + HEAD + dirty hash) invalidates
  naturally on profile changes because HEAD/Cargo.toml change.
- One-time transition cost: the first build after this merge recompiles dependencies at
  opt-level=2 and build scripts at opt-level=2/3 (cached afterwards).
- cargo 1.100 target layout: artifacts live under `target/debug/build/<pkg>/<hash>/out/`;
  receipts parse paths from cargo's own output, so tooling is layout-independent. Note:
  `cargo -v` prints `Executable` lines in backtick/absolute-path format while non-verbose
  mode prints `name (relpath)` — parsers must handle both.
- Measurement hazard learned: a concurrent non-flock build can invalidate the incremental
  cache and inflate a "relink" timing into a full recompile. Link A/Bs must be
  concurrency-guarded (no other cargo/rustc process) and report per-side compile counts.

## Limitations

- Single-host before/after; baseline is the QA sweep re-run under identical protocol, not a
  same-session re-run. Wall times on a shared VPS; the heavy lock excludes other cargo jobs
  but not all host activity (the two full-suite runs differ 412 vs 441 s, ~7% noise).
- Link-only A/B removes the 54 test executables and rebuilds from cached artifacts; each
  side logs one incremental lib-harness compile. Full compile cost is excluded.
- Hosted CI skips real-data suites (no corpus on runners); real-data before/after is
  local-only evidence.
- No completion-percentage claim: throughput infrastructure does not move the PROGRESS.md
  rubric criteria. No financial, provenance, scientific or security gate touched; diagnostic
  execution remains `diagnostic_simulation` / eligibility `none`.
