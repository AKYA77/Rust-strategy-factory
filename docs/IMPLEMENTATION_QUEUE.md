# Implementation queue

Status: SOFTWARE_INCOMPLETE. This ledger does not replace the full directive.

Continuity run 2026-11-27 (CI-record continuity for 04bb753): baseline clean `main`
`04bb7535fc549963d46b02339fd2c26a6abdf59c` == `origin/main` (0/0), no other
cargo/rustc worker, heavy lock acquired. HEAD is the real code fix "Fix load-dependent
adaptive worker-selection fixture flake"; its hosted push CI run **46026413867**
(head_sha exactly `04bb7535fc...`) reached terminal `completed`/`success` on all 7 jobs
(release-build, browser, rust-checks, data-compute-identity, full-horizon,
check-operations, dependency-audit), created 15:36:31Z, concluded 15:51:22Z; no
in-progress run called a pass. This was the sole unrecorded verified increment — the
prior ledger stopped at parent `eacf7f6` (run 45988023105). Baseline `scripts/check.sh`
(`RSF_BUILDER_PROFILE=hermes-v2-m1`): **EXIT=0, 765 passed / 0 failed / 0 ignored over
72 binaries** (lib 278), zero named failures, fmt/clippy clean; 17:55:02Z–18:07:11Z;
head-provenance rebuild first (9013 files, 3.0 GiB), no source changed. The flake fix's
own RED→GREEN regression evidence is unchanged in
`docs/VERIFICATION/adaptive-worker-selection-fixture-flake-2026-11-27.md`; docs-only
CI record here, no new test applies and no percentage changed. Read-only owner-gate
re-check: `browser_vault list` empty; no reboot since 2026-11-22 (`who -b`, uptime
5d1h20m); deployed user-scope units research `activating`/`auto-restart`
`NRestarts=863`, dashboard `inactive`/`dead` `NRestarts=32266`. V2 milestones 1–6
evidenced; M7 NOT accepted. Rubric: software ~85%, verified E2E ~80%, unchanged.

Continuity run 2026-11-27 (adaptive worker-selection fixture flake fix): baseline clean `main`
`eacf7f663ee2cb8f03150a0c73a1781cfa235528` == `origin/main`, no other cargo/rustc worker,
heavy lock acquired. Baseline `scripts/check.sh` (`RSF_BUILDER_PROFILE=hermes-v2-m1`) was
**EXIT=101** (15:12:30Z–15:22:17Z): only
`tests/scheduler_resources.rs::adaptive_batch_measures_cpu_and_adapts_workers_without_changing_receipts`
failed (`never downscaled: [4, 3, 3, 2]` at line 350; binary 5 passed / 1 failed; 56 earlier
binaries 653 passed), because the 8-job/10-round fixture completed before measured worker
selection reached the floor on a noisy host window. Production selection logic unchanged
(its floor is pinned by `sustained_pressure_steps_down_to_floor_and_never_below`). Test-only
fix in `tests/scheduler_resources.rs`: 24 jobs, 40 max rounds, `expected.len() == 24`, extra
`used.last() == Some(&1)` assertion, original contract/receipt assertions retained. Focused
target 5/5 isolated runs EXIT=0 before and after; final-tree `scripts/check.sh`
**EXIT=0, 765 passed / 0 failed / 0 ignored over 72 binaries** (lib 278), zero named failures,
fmt/clippy clean (15:26:46Z–15:35:24Z). Hosted run **45988023105** on exact HEAD `eacf7f6`
terminal `completed`/`success` on all 7 jobs. `browser_vault list` empty, no reboot since
2026-11-22, research `activating` NRestarts=830 / dashboard `inactive` NRestarts=32266.
No code, financial, scientific, provenance, data-scope or security rule changed; class
REAL_STRUCTURALLY_VALID, eligibility none. V2 milestones 1–6 evidenced; M7 NOT accepted;
rubric software ~85%, verified E2E ~80%, unchanged. Evidence:
`docs/VERIFICATION/adaptive-worker-selection-fixture-flake-2026-11-27.md`.

Continuity run 2026-11-27 (CI-record continuity for 8802a83): baseline clean `main`
`8802a834b41e6627259b9554ca8fa3064e4b7d18` == `origin/main` (0/0, `git ls-remote`
identical), no other cargo/rustc worker, heavy lock acquired. The only unrecorded
verified increment was the terminal hosted CI result for the CI-record commit
itself: run **45969419004** (push, `main`, head_sha exactly `8802a834b41e6627259b9554ca8fa3064e4b7d18`)
`completed`/`success` on all 7 jobs (rust-checks, browser, data-compute-identity,
full-horizon, release-build, dependency-audit, check-operations); no in-progress
run called a pass. Baseline `scripts/check.sh` (`RSF_BUILDER_PROFILE=hermes-v2-m1`):
**EXIT=0, 765 passed / 0 failed / 0 ignored over 72 binaries** (lib 278), zero
named failures, fmt/clippy clean; started 2026-11-27T10:57:52Z, ended 11:07:13Z;
no source changed. Docs-only CI-record continuity: no code, financial, scientific,
provenance, data-scope or security rule changed; the existing
`tests/cached_receipt_convention.rs` (3 tests) and `src/economic_calendar.rs`
gap-contract guards are unchanged, so no new behavioral regression test applies.
Read-only owner-gate re-check: `browser_vault list` empty; no reboot since
2026-11-22 (`who -b`, `uptime` 4d18h); deployed user-scope units remain research
`activating`/`auto-restart` `NRestarts=781` and dashboard `inactive`/`dead`
`NRestarts=32266`; installed config 140 inputs / 0 conversion fields. V2
milestones 1–6 evidenced; M7 NOT accepted. Rubric: software ~85%, verified E2E
~80%, unchanged.

Continuity run 2026-11-27 (CI-record continuity for 3f4dd6a): baseline clean `main`
`3f4dd6af02ca82b46a2fa0c23021c5a378cab053` == `origin/main` (0/0, `git ls-remote`
identical), no other cargo/rustc worker, heavy lock acquired. The only unrecorded
verified increment was the terminal hosted CI result for the CI-record commit
itself: run **45953612456** (push, `main`, head_sha exactly `3f4dd6a`) `completed`/
`success` on all 7 jobs (rust-checks, browser, data-compute-identity, full-horizon,
release-build, dependency-audit, check-operations); no in-progress run called a
pass. Baseline `scripts/check.sh` (`RSF_BUILDER_PROFILE=hermes-v2-m1`): **EXIT=0,
765 passed / 0 failed / 0 ignored over 72 binaries** (lib 278), zero named
failures, fmt/clippy clean; the head-provenance check forced a project-only rebuild
first (9009 files, 3.0 GiB), no source changed. Docs-only CI-record continuity: no
code, financial, scientific, provenance, data-scope or security rule changed; the
existing `tests/cached_receipt_convention.rs` (3 tests) and
`src/economic_calendar.rs` gap-contract guards are unchanged, so no new behavioral
regression test applies. Read-only owner-gate re-check: `browser_vault list` empty;
no reboot since 2026-11-22 (`who -b`, `uptime` 4d15h); deployed **user-scope**
units remain research `activating`/`auto-restart` `NRestarts=755` and dashboard
`inactive`/`dead` `NRestarts=32266`; installed config 140 inputs / 0 conversion
fields. V2 milestones 1–6 evidenced; M7 NOT accepted. Rubric: software ~85%,
verified E2E ~80%, unchanged.

Continuity run 2026-11-27 (CI-record continuity for de43cd9): baseline clean `main`
`de43cd986e8aec924505806a6f18b8b6b75332ab` == `origin/main` (0/0, `git ls-remote`
identical), no other cargo/rustc worker, heavy lock acquired. The only unrecorded
verified increment was the terminal hosted CI result for the CI-record commit
itself: run **45940321277** (push, `main`, head_sha exactly `de43cd9`) `completed`/
`success` on all 7 jobs (rust-checks, browser, data-compute-identity, full-horizon,
release-build, dependency-audit, check-operations); no in-progress run called a
pass. Baseline `scripts/check.sh` (`RSF_BUILDER_PROFILE=hermes-v2-m1`): **EXIT=0,
765 passed / 0 failed / 0 ignored over 72 binaries** (lib 278), zero named
failures, fmt/clippy clean; started 2026-11-27T06:33:03Z, ended 06:41:37Z; the
head-provenance check forced a project-only rebuild first, no source changed.
Docs-only CI-record continuity: no code, financial, scientific, provenance,
data-scope or security rule changed; the existing `tests/cached_receipt_convention.rs`
(3 tests) and `src/economic_calendar.rs` gap-contract guards are unchanged, so no
new behavioral regression test applies. Read-only owner-gate re-check:
`browser_vault list` empty; no reboot since 2026-11-22 (`who -b`, `uptime` 4d13h);
the deployed **user-scope** units remain research `activating`/`auto-restart`
`NRestarts=731` and dashboard `inactive`/`dead` `NRestarts=32266` (plain `systemctl`
had hit stale system-scope duplicates). V2 milestones 1–6 evidenced; M7 NOT
accepted. Rubric: software ~85%, verified E2E ~80%, unchanged.

Continuity run 2026-11-27 (CI-record for 8c9a6fd + deployed-service evidence correction):
baseline clean `main` `8c9a6fd6aec0fd318da4bf071679c284c6b430a5` == `origin/main`
(0/0, `git ls-remote` identical), no other cargo/rustc worker, heavy lock acquired.
Run **45926014164** on head_sha exactly `8c9a6fd` reached terminal `completed`/
`success` on all 7 jobs (release-build, browser, rust-checks, data-compute-identity,
full-horizon, check-operations, dependency-audit); no in-progress run called a pass.
Baseline `scripts/check.sh` (`RSF_BUILDER_PROFILE=hermes-v2-m1`): **EXIT=0, 765 passed
/ 0 failed over 72 binaries** (lib 278), 0 ignored, zero named failures, fmt/clippy
clean; log ended 2026-11-27T04:28:48Z; head-provenance check forced a project-only
rebuild (9009 files, 3.0 GiB), no source changed. New read-only finding: actual
deployed units are `rust-strategy-factory-{research,dashboard}.service` -- research
`activating`/`auto-restart` `NRestarts=704` (exit 1, USDJPY missing an explicit
approved event-aligned conversion file), dashboard `inactive`/`dead` `NRestarts=32266`;
installed config 140 inputs / 0 conversion fields. The previous entry's `rsf-*`
probe and its "active, NRestarts=0" claim are corrected by
`docs/VERIFICATION/live-service-state-2026-11-27.md`. No service/config/cutover
change: conversion authority and production changes are owner-only; no gate weakened.
`browser_vault list` empty; `Linger=yes` is not reboot proof; the recorded 2026-11-22
owner reboot conflicts with the M7 receipt's open reboot item and is not re-decided
here. Docs-only evidence correction: no code/financial/scientific/provenance/
data-scope/security rule changed; existing regression guards unchanged, no new test
applies, no percentage gain. V2 milestones 1–6 evidenced; M7 NOT accepted. Rubric:
software ~85%, verified E2E ~80%, unchanged.

Continuity run 2026-11-27 (CI-record continuity for 513395d): baseline clean `main`
`513395db6f86db5d6d025cccac33c88cbd2ba2a3` == `origin/main` (0/0, `git ls-remote`
identical), no other cargo/rustc worker, heavy lock acquired. The only unrecorded
verified increment was the terminal hosted CI result for the CI-record commit
itself: run **45913367601** (push, `main`, head_sha exactly `513395d`) `completed`/
`success` on all 7 jobs (release-build, browser, rust-checks, data-compute-identity,
full-horizon, check-operations, dependency-audit). Baseline `scripts/check.sh`
(`RSF_BUILDER_PROFILE=hermes-v2-m1`): **EXIT=0, 765 passed / 0 failed over 72
binaries** (lib 278), 0 ignored, zero named failures, fmt/clippy clean; log ended
2026-11-27T02:01:51Z; the head-provenance check forced a project-only rebuild first,
no source changed. Docs-only CI-record continuity: no code, financial, scientific,
provenance, data-scope or security rule changed; the existing
`tests/cached_receipt_convention.rs` (3 tests) and `src/economic_calendar.rs`
gap-contract guards are unchanged, so no new behavioral regression test applies.
V2 software milestones 1–6 evidenced; M7 NOT accepted — `browser_vault list` still
returned no saved logins and a fresh reboot remains owner-only (units
active/running, NRestarts=0, Linger=yes, but no reboot proof). No software milestone
unachieved; further CI-record commits add no project value while those gates remain
owner-blocked.

Continuity run 2026-11-27 (CI-record continuity for d14cd41): baseline clean `main`
`d14cd4129703185026f01e63dd351f63e404af13` == `origin/main` (0/0, `git ls-remote`
identical), no other cargo/rustc worker, heavy lock acquired. The only unrecorded
verified increment was the terminal hosted CI result for the CI-record commit
itself: run **45899547434** (push, `main`, head_sha exactly `d14cd41`) `completed`/
`success` on all 7 jobs (release-build, browser, rust-checks, data-compute-identity,
full-horizon, check-operations, dependency-audit). Baseline `scripts/check.sh`
(`RSF_BUILDER_PROFILE=hermes-v2-m1`): **EXIT=0, 765 passed / 0 failed over 72
binaries** (lib 278), 0 ignored, zero named failures, fmt/clippy clean; the
head-provenance check forced a project-only rebuild before the ladder (no source
changed). Docs-only CI-record continuity: no code, financial, scientific,
provenance, data-scope or security rule changed; the existing
`tests/cached_receipt_convention.rs` (3 tests) and `src/economic_calendar.rs`
gap-contract guards are unchanged, so no new behavioral regression test applies.
V2 software milestones 1–6 evidenced; M7 NOT accepted — `browser_vault list`
still returned no saved logins and a fresh reboot remains owner-only. No software
milestone unachieved; further CI-record commits add no project value while those
gates remain owner-blocked.

Continuity run 2026-11-26 (CI-record continuity for e62c1a2): baseline clean `main`
`e62c1a2d2245248bddb890537fc8c7ad7936bdfb` == `origin/main` (0/0, `git ls-remote`
identical), no other cargo/rustc worker, heavy lock acquired. The only unrecorded
verified increment was the terminal hosted CI result for the CI-record commit
itself: run **45883499704** (push, `main`, head_sha exactly `e62c1a2`) `completed`/
`success` on all 7 jobs (release-build, browser, rust-checks, data-compute-identity,
full-horizon, check-operations, dependency-audit). Baseline `scripts/check.sh`
(`RSF_BUILDER_PROFILE=hermes-v2-m1`): **EXIT=0, 765 passed / 0 failed over 72
binaries** (lib 278), 0 ignored, zero named failures, fmt/clippy clean; the
head-provenance check forced a project-only rebuild first with no source change.
Docs-only CI-record continuity: no code, financial, scientific, provenance,
data-scope or security rule changed; the existing `tests/cached_receipt_convention.rs`
(3 tests) and `src/economic_calendar.rs` gap-contract guards are unchanged, so no
new behavioral regression test applies. V2 milestones 1–6 evidenced; M7 NOT
accepted — `browser_vault list` still returned no saved logins and a fresh reboot
remains owner-only. No software milestone unachieved; further CI-record commits
add no project value while those gates remain owner-blocked.

Continuity run 2026-11-26 (CI-record continuity for ca76653): baseline clean `main`
`ca76653c7a5a7c590beb7f1b69b0bce19df88742` == `origin/main` (0/0, `git ls-remote`
identical), no other cargo/rustc worker, heavy lock acquired. The only unrecorded
verified increment was the terminal hosted CI result for the CI-record commit
itself: run **45863237225** (push, `main`, head_sha exactly `ca76653`) `completed`/
`success` on all 7 jobs (release-build, browser, rust-checks, data-compute-identity,
full-horizon, check-operations, dependency-audit). Baseline `scripts/check.sh`
(`RSF_BUILDER_PROFILE=hermes-v2-m1`): **EXIT=0, 765 passed / 0 failed over 72
binaries** (lib 278), zero named failures, fmt/clippy clean; the head-provenance
check forced a project-only rebuild first, with no source change. Docs-only
CI-record continuity: no code, financial, scientific, provenance, data-scope or
security rule changed; the existing `tests/cached_receipt_convention.rs` (3 tests)
and `src/economic_calendar.rs` gap-contract guards are unchanged, so no new
behavioral regression test applies. V2 milestones 1–6 evidenced; M7 NOT accepted
(two owner-only external gates). No software milestone unachieved; further
CI-record commits add no project value while those gates remain owner-blocked.

Continuity run 2026-11-26 (CI-record continuity for 981466a): baseline clean `main`
`981466ab675673820e759d015e389160162d53f2` == `origin/main` (0/0, `git ls-remote`
identical), no other cargo/rustc worker, heavy lock acquired. The only unrecorded
verified increment was the terminal hosted CI result for the CI-record commit itself:
run **45839693702** (push, `main`, head_sha exactly `981466a`) `completed`/`success`
on all 7 jobs (release-build, browser, rust-checks, data-compute-identity,
full-horizon, check-operations, dependency-audit). Baseline `scripts/check.sh`
(`RSF_BUILDER_PROFILE=hermes-v2-m1`): first ladder hit a transient lock flake in
`tests/restore_cutover.rs::restore_cutover_fences_copied_actors_and_revives_on_restored_copy`
(`EXIT=101`, `"another recovery operation is active"`), passed 3/3 in isolation, and a
second full ladder was clean **EXIT=0, 765 passed / 0 failed over 72 binaries**
(lib 278), fmt/clippy clean. Docs-only CI-record continuity: no code, financial,
scientific, provenance, data-scope or security rule changed; the existing
`tests/cached_receipt_convention.rs` (3 tests) and `src/economic_calendar.rs` (2
gap-contract) guards are unchanged, so no new behavioral regression test applies.
V2 software milestones 1–6 evidenced; M7 NOT accepted (two owner-only external gates).
No software milestone is unachieved; further CI-record commits add no project value
while those gates remain owner-blocked.

Continuity run 2026-11-26 (format-gate repair + calendar-wiring reconciliation):
baseline clean `main` `d1f55eaf2d320b36d021bc1453620bed573f5f05` == `origin/main`
(0/0, `git ls-remote` identical), no other cargo/rustc worker, heavy lock acquired.
Reconciled `dbc7598` (docs-only CI record; hosted run **45814075396**, terminal
`completed`/`success` on all 7 jobs) and `d1f55ea` (economic-calendar wiring). The
latter broke the mandatory format gate despite claiming fmt-clean: hosted run
**45822773618** concluded `completed`/`failure` with only `rust-checks` failing at
`cargo fmt --all -- --check` (job 139919015606, step 5); the other six jobs
succeeded and its tests were skipped. Local pinned-toolchain reproduction EXIT=1
with four indentation diffs in `benches/batch.rs:99`, `benches/indexed_factory.rs:73`,
`benches/proof_screen.rs:65`, `tests/windows.rs:311`.

Fix: canonical `cargo fmt --all` (whitespace only) + two new unit regressions in
`src/economic_calendar.rs` for the previously untested `from_corpus_gaps()` path
(causal `published_ms`, provenance tags, impact bounds, threshold validation).
RED fmt EXIT=1 → GREEN EXIT=0; focused calendar tests 8/8; final-tree
`scripts/check.sh` (`RSF_BUILDER_PROFILE=hermes-v2-m1`) **EXIT=0**, **765 passed /
0 failed over 72 binaries** (lib 278), zero named failures, fmt/clippy clean.
Calendar wiring reviewed as inert w.r.t. signals/fills/receipts (`news_blackout_mask`
is never called); gap events are diagnostic corpus observations, not approved
calendar facts or closed-session evidence, consistent with the WITHDRAWN policy.
No financial, scientific, provenance, data-scope or security rule changed; class
`REAL_STRUCTURALLY_VALID`, eligibility none. Hosted CI **45837439949** on the exact
repair SHA `16afb16c5b55ea48fe8b0129820399ec19af29c4` reached terminal
`completed`/`success` on all 7 jobs (including the previously failing rust-checks);
only terminal conclusions recorded. Evidence:
`docs/VERIFICATION/fmt-gate-repair-2026-11-26.md`. V2 milestones 1–6 evidenced;
M7 NOT accepted (two owner-only external gates). No software milestone unachieved.

Continuity run 2026-11-26 (CI-record continuity for 9442d85): baseline clean `main`
`9442d85742be85b99798027f102950450e0fa9ee` == `origin/main` (0/0, `git ls-remote` identical), no
other cargo/rustc worker, heavy lock acquired. `scripts/check.sh`
(`RSF_BUILDER_PROFILE=hermes-v2-m1`): EXIT=0, **763 passed / 0 failed over 72 binaries** (lib 276),
zero named failures, fmt/Clippy clean. The docs-only CI-record continuity commit `9442d85` had a
terminal hosted CI result not yet in the ledgers: run **45793196846** (push, `main`, head_sha
exactly `9442d85`) `completed`/`success` on all 7 jobs (release-build, browser, rust-checks,
data-compute-identity, full-horizon, check-operations, dependency-audit). Docs-only continuity
record: no code, financial, scientific, provenance, data-scope or security rule changed; the
`tests/cached_receipt_convention.rs` (3 tests) guard is unchanged. V2 software milestones 1–6
evidenced; M7 NOT accepted (two owner-only external gates). No software milestone is unachieved;
further CI-record commits add no project value while those gates remain owner-blocked.

Continuity run 2026-11-26 (cached-receipt CI-record continuity): baseline clean `main`
`ef3c33a3cc0414e31d2621b1139e18d64a534fed` == `origin/main` (0/0, `git ls-remote` identical), no
other cargo/rustc worker, heavy lock acquired. `scripts/check.sh`
(`RSF_BUILDER_PROFILE=hermes-v2-m1`): EXIT=0, **763 passed / 0 failed over 72 binaries** (lib 276),
zero named failures, fmt/Clippy clean. The docs-only CI-record reconciliation commit `ef3c33a` had
a terminal hosted CI result not yet in the ledgers: run **45771533851** (push, `main`, head_sha
exactly `ef3c33a`) `completed`/`success` on all 7 jobs (release-build, browser, rust-checks,
data-compute-identity, full-horizon, check-operations, dependency-audit). Docs-only: no code,
financial, scientific, provenance, data-scope or security rule changed; the existing
`tests/cached_receipt_convention.rs` (3 tests) regression guard is unchanged. V2 software
milestones 1–6 evidenced; M7 NOT accepted (two owner-only external gates). No software milestone
is unachieved.

Continuity run 2026-11-26 (cached-receipt reconciliation CI record): baseline clean `main`
`fe15d1d17aeb3d78c118f20009a5dfe110462da8` == `origin/main` (0/0), no other cargo/rustc worker,
heavy lock acquired. Baseline `scripts/check.sh` (`RSF_BUILDER_PROFILE=hermes-v2-m1`) EXIT=0,
**763 passed / 0 failed over 72 binaries** (lib 276), zero named failures (fmt, clippy `-D warnings`
and all tests passed). The reconciliation commit `fe15d1d` had a terminal hosted CI result that
was not yet in the ledgers: run **45754331414** (push, `main`, head_sha exactly `fe15d1d`)
`completed`/`success` on all 7 jobs (release-build, browser, rust-checks, data-compute-identity,
full-horizon, check-operations, dependency-audit). Docs-only CI-record reconciliation: no code,
financial, scientific, provenance, data-scope or security rule changed; the existing regression
guard `tests/cached_receipt_convention.rs` (3 tests) is unchanged. V2 software milestones 1–6
evidenced; M7 NOT accepted (owner-only external gates). No software milestone is unachieved.

Continuity run 2026-11-26 (cached-receipt convention reconciliation): baseline clean `main`
`d3f354eb842a822e94de652caa54305f16a2e6db` == `origin/main` (0/0), no other worker, heavy
lock acquired. Verified four already-pushed commits beyond the ledgers' prior head `3b789b2`:
`34aae4b` (ledger CI record), `be6553e` (PROJECT_STATE scope correction + 16 limitations
strings), `8a8e41c` (test rename), `d3f354e` (6 non-USD cached reports' conversion_policy
label switched to `diagnostic_reciprocal_closing_rate_v1_bid_only`). Changed-line audit over
`3b789b2..HEAD`: only 138 policy labels, 16 limitations strings, 1 test rename, 4 ledger CI
lines, 2 blank lines -- no numeric field, receipt_hash, count, P&L, classification, gate or
financial/scientific assertion changed. Relabel is truthful: the cached execution strategy is
`"side": "Long"`, so Long fills use `bid` under both the superseded direction-based rule and
the active bid-only rule; numbers and hashes stay valid. New
`tests/cached_receipt_convention.rs` (3 tests) pins this: RED with `3b789b2` cached files FAILED
(EXIT=101, `long_bid_short_ask` vs `bid_only`), GREEN after byte-for-byte HEAD restore 3/3
(EXIT=0). `scripts/check.sh` EXIT=0 760 passed/0 failed over 71 binaries baseline, and EXIT=0
**763 passed / 0 failed over 72 binaries** (lib 276) on the final tree. Hosted CI terminal
success on all 7 jobs for 45736386703, 45740888354, 45741014575, 45741284289. Evidence:
`docs/VERIFICATION/cached-receipt-convention-reconciliation-2026-11-26.md`. No financial,
scientific, provenance, data-scope or security rule changed; class stays REAL_STRUCTURALLY_VALID,
eligibility none; V2 milestones 1–6 evidenced, M7 NOT accepted (owner-only external gates).
No further software milestone is unachieved.

Continuity run 2026-11-26 (bid-only conversion convention reconciliation): baseline clean
`main` `38fcc736863e4bb59b888134d958b963d27b3138` == `origin/main`, no other worker, heavy
lock acquired. Verified two already-pushed, previously unreconciled commits:
`617f21575129130127c37c4b7d917ad1fd41326a` (code fix: diagnostic replay `diagnostic_conversion`
now bid-only for both sides, matching the research loader that derives conversions before a
side is known; policy string `diagnostic_reciprocal_closing_rate_v1_bid_only`) and
`38fcc736863e4bb59b888134d958b963d27b3138` (docs: per-partition conversion memory, 2 GiB
input-file vs 8 GB post-load RAM budget, 20-month/22-month coverage gap of 74 missing inputs,
limitations #7/#8). `scripts/check.sh` (`RSF_BUILDER_PROFILE=hermes-v2-m1`): **EXIT=0**,
**760 passed / 0 failed over 71 binaries** (lib 276), zero named failures, fmt and clippy
clean — correcting the stale `762/72` ledger figure. Red/green: with the old direction-based
`diagnostic_conversion` body restored, the lib unit test
`worker_preparation_tests::diagnostic_conversion_is_bid_based_reciprocal` FAILED (`EXIT=101`,
`(10,1501)` vs `(1,150)`) and `tests/verify_conversion.rs::
all_three_non_usd_instruments_derive_conversions_from_dataset` FAILED (`EXIT=101`,
"USDJPY loader vs tick engine Short"); after byte-for-byte restore both passed (`EXIT=0`, 1/1
and 14/14), tree clean. Hosted CI: run **45701751661** (`617f215`) and **45716427171**
(`38fcc73`) both terminal `completed`/`success` on all 7 jobs; the docs-only `80dbddd` run
45697619785 was terminal `failure` (retained, superseded). Evidence:
`docs/VERIFICATION/bid-only-convention-reconciliation-2026-11-26.md`. No financial,
scientific, provenance, data-scope or security rule changed; class stays
REAL_STRUCTURALLY_VALID, eligibility none. V2 milestones 1–6 evidenced; M7 NOT accepted
(two owner-only external gates). No further software milestone is unachieved.

Post-push terminal CI: reconciliation commit `3b789b28ed6b946e21ffdddb60a8f278da7438e7`
hosted run **45734055499** concluded `completed`/`success` on all 7 jobs; remote SHA verified
equal to local HEAD. Recorded only after the run reached a terminal conclusion.

Continuity run 2026-11-25 (conversion implementation reconciliation): baseline clean `main`
`c3ee6c0` (== `origin/main`, 0/0, no other worker, heavy lock acquired). The already-pushed
implementation commit `c3ee6c0` (src/main.rs, src/tick_engine.rs, tests/corpus.rs,
tests/verify_conversion.rs +2 tests) derives exact event-aligned reciprocal conversions for
USDJPY/USDCHF/USDCAD instead of refusing non-USD quotes; identity sentinel stays refused, class
REAL_STRUCTURALLY_VALID, eligibility none. Verified: `scripts/check.sh` EXIT=0, **759 passed / 0
failed over 71 binaries** (lib 276), zero named failures; focused verify_conversion 13/13 and
real_conversion_evidence 2/2. Red/green: restoring parent `15a635e`'s `.is_err()` assertion made
`tests/corpus.rs::corpus_account_replay_matches_all_materialized_receipt_fields` FAIL (EXIT=101);
HEAD's `.is_ok()` passes (EXIT=0), file restored byte-for-byte and tree clean. Evidence:
`docs/VERIFICATION/conversion-implementation-reconciliation-2026-11-25.md`. Hosted CI run
**45676627807** on exact SHA `c3ee6c0` reached terminal success on all 7 jobs; the two
docs-reconciliation commits `b2d0eb1` and `6221875` also concluded terminal success on all 7 jobs
(runs **45694761489**, **45694794740**). No financial, scientific, provenance, data-scope or
security rule changed; M7 external gates unchanged.

Continuity run 2026-11-25 (real-corpus conversion evidence): baseline clean `main` `18bba89`
had `scripts/check.sh` EXIT=0, 755 passed / 0 failed over 70 binaries (lib 276). Added
`tests/real_conversion_evidence.rs` (2 tests) and
`docs/VERIFICATION/real-corpus-conversion-2026-11-25.md`, and corrected
`docs/CONVERSION_CONVENTION.md` to distinguish synthetic fixtures from the real corpus profile.
Verified USDJPY/USDCHF/USDCAD on actual 2025-03 `.rsf` chunks: 100,000 events each, 64 samples
each, exact cross-product rates, strict event alignment, all validation safeguards, no identity
conversion, bound provenance hashes. Red control failed with a 1000x synthetic-scale rate that
still passed validation; green invariant passed 2/2. Post-increment full `scripts/check.sh`
EXIT=0, 757 passed / 0 failed over 71 binaries (lib 276), new target 2/2. Increment commit
`83213c15df87356ffd897dabce04b9438458ecb8`; hosted CI run **45668913065** on that exact SHA
concluded terminal `success` on all 7 jobs. No financial, scientific, data-scope or security rule
changed; M7 external gates still open. No further software milestone is unachieved.

Continuity run 2026-11-25 (formatting gate): baseline clean `main` `f8e4eaf` had
`scripts/check.sh` EXIT=1 at `cargo fmt --all -- --check` on `src/research.rs:1474`
(introduced by `2177da0`); hosted `rust-checks` ran only `cargo test`, so CI run 45634655873
on that SHA was green despite the broken local gate. Fixed with the exact rustfmt rewrite
(whitespace only) and a `cargo fmt --all -- --check` step in `.github/workflows/ci.yml`.
Post-fix ladder: fmt/clippy EXIT=0, `scripts/check.sh` EXIT=0, **744 passed / 0 failed over
69 binaries** (lib 276), zero named test failures — correcting the earlier unverified
746/70 figure. Evidence: `docs/VERIFICATION/fmt-gate-regression-2026-11-25.md`.
Increment commit `e92934ce57a4e84654f5f0a2df4275557c60aa30`; hosted CI run **45640484836**
on that exact SHA concluded terminal `success` on all 7 jobs including the new format gate.
No V2 scientific rule or seven-instrument scope changed. V2 milestones 1–6 evidenced; Milestone 7 still NOT
accepted (only external-evidence gates 3–4 remain: saved dashboard credentials and owner
manual reboot). No further software milestone is unachieved.

Integration state: all open PRs have been merged into `main` — the Codex stack #8–#13, the independent campaign-owned ledger #1, the Hermes historical Parquet→native migration #6, and earlier checkpoint PRs #2–#5/#7 via #14 (merge commit `3d2d6f2`); PR #53 (main CI restoration, commit 858a289); PR #52 (registered full-period real-data executor, merge commit ba66956); and PR #54 (20-month training window + OOS holdout config; Conversion::usd sentinel contract restored, merge commit bda7fca). Every PR head commit is an ancestor of `origin/main`; all PRs were closed as merged with history preserved. local `main` HEAD is `767f777` (Stage-1 CLI regression fix over nine integrated bot commits; parent `4b52ea4`), pushed to `origin/main` in this run. Hosted CI push runs **45528848345**
on `2ced8fe` and **45535143330** on `b17ce34` both concluded **success on all 7 jobs** (release-build, browser, rust-checks,
data-compute-identity, full-horizon, check-operations, dependency-audit); the earlier
`98cdc79`/`57c13fc`/`18d6e9b` push runs failed only the browser job and are superseded by the
`2ced8fe` browser-script repair. Local `scripts/check.sh` at `2ced8fe`: EXIT=0, 733 passed /
0 failed over 69 test binaries. V2 mandate milestones 2–6 are evidenced complete; **milestone 7
is NOT accepted** — the full-history ordered coverage was re-pinned to the exact HEAD
(`docs/VERIFICATION/milestone7b-*`), but the deployed dashboard release, the installed research
config (still 20 stale XAUUSD entries; research unit crash-looping), authenticated dashboard
endpoints and reboot persistence remain open and owner-gated. The converted legacy corpus is
REAL_STRUCTURALLY_VALID only.

Cursor 2026-11-25: `767f777` corrected the Stage-1 CandleScreen CLI regression
(failing before: `QueryReturnedNoRows` in `tests/research_cli.rs`; passing after:
full `scripts/check.sh` EXIT=0, 744 passed / 0 failed over 67 binaries; hosted CI
run 45591831853 concluded success on all 7 jobs at `80b7665`. Evidence:
`docs/VERIFICATION/stage1-cli-regression-2026-11-25.md`. M7 status unchanged (NOT accepted).

1. Harden foundational arithmetic/import/replay with adversarial/property tests; extend normalized importer coverage and immutable atomic artifacts.
2. Acquire legally usable short real quote windows for all eight symbols in 2025 and 2026; retain source terms/URLs/archive hashes. No actual source has been acquired yet.
3. Source reviewed instrument contracts, sessions/DST calendars and timestamped FX conversion semantics; keep gold-specific conventions blocked pending evidence.
4. Bounded lossless native uncompressed SoA artifacts, exact hashes and CLI/research loading now work; execution reconstructs current events. Bounded single-frame ZSTD/native wrappers now preserve exact native bytes and input identities, pass malformed/output/window/trailing/structure checks and actual CLI/research/protocol reuse, and retain selected source/license plus synthetic component receipts. A strict bounded Parquet/ZSTD baseline now has exact logical-schema/resource preflight, CLI/research/protocol identity reuse and all-eight same-corpus current-pipeline comparisons with no claimed speedup. Remaining: approved-real-corpus/physical-cold selection, broader justified codec comparison and complete codec/source audit. Direct hot arrays (`src/corpus::direct_arrays`), indexed blocks (`src/native::{NativeIndexed, BlockColumns}`), the mmap guard (`src/corpus::mmap_guard`) and the `corpus-context` CLI are implemented and merged via PR #49 (`hermes/indexed-blocks-storage`; tests/corpus_indexed.rs: all-eight-instruments x seven-frames oracle tests, nonzero full receipt parity vs scalar replay, exact stream checksums).
5. All nine family templates, prior channels, RSI/mean crosses, volatility and UTC session operators now work with bounded shared caches. Closed higher-timeframe confirmation now compiles/executes with causal joins and authoritative mask checks. MACD/Bollinger crosses now compile/executes with independent fixtures and bounded expanded regions. Typed outward ATR initial stops and frozen policy probes now execute with risk/quantity protection. Bounded training-only seeded structural mutations and compatible template crossover now work with canonical/mutation-class coverage; seeded exploration now covers every strictly higher timeframe at two fixed mean windows with three template donors under a 16-case bound, and the fixed descriptive regime bins gained an orthogonal prior-window displacement dimension (trend/range plus momentum) that OOS breakdowns record alongside entry regimes. Remaining: sparse bitsets now have density-adaptive backing in the shared feature cache with controlled recursive acceptance scheduling (opt-in depth cap; OOS/protected holdouts never feed adaptation); behavioral diversity evidence is descriptive only (`behavior::diversity`, `historical_corpus::diversity`, 22-month EURUSD receipt); discovery-quality conclusion from canonical uniqueness remains open.
6. Conditional position barrier bounds, collision/entry/deadline ambiguity and persisted tick receipt audits now work. Implemented: candidate-level net outcome estimates/bounds (`candidate_screen::NetBounds`: pessimistic/optimistic per-candidate net bounds from the estimate's explicit uncertainty set), rejected-candidate sampling (deterministic `audit_sample_bps` tick audit of shadow rejections) and measured false-negative monitoring (`shadow_false_negative` plus aggregate `AuditCoverage` per batch via `BatchReport.audit_coverage` and the `batch-experiments` CLI). All three are diagnostic: not acceptance, not qualification evidence, never execution skipping. The default screen still rejects only proven no-signals.
7. Pending market/stop-market/limit lifecycle, cancellation, expiry and restart snapshots are now implemented/tested in the library. Typed pending entry intents, fixed placement sizing/reservations and pre-fill risk/marked-margin checks are integrated in replay and tested. Complete: richer order audit/diagnostics (Order cancellation/expiry/restart snapshots, pre-fill marked-margin/risk rejection receipts, cost-aware sizing on the exact volume grid) and approved profile/session handling (explicit supported L1 full-fill liquidity model; L1 quotes alone do not establish depth; partial fills only with a justified explicit model — none exists, so only full fills are supported) are implemented and tested in src/orders.rs, src/screen.rs, src/candidate_screen.rs, src/contracts.rs and src/risk.rs, with tests/orders.rs, tests/screen.rs, tests/candidate_screen.rs, tests/core.rs and tests/risk.rs. Diagnostic shadow rejections (NetBounds pessimistic/optimistic bounds, audit_sample_bps rejected-candidate sampling, aggregate AuditCoverage per batch) are diagnostic-only and never gate execution or acceptance. Non-USD quote conversion respects the USD-quote-only engine gate; all eight instruments and USD/non-USD conversions are independently tested.
8. Prototype first-hit index only after sequential reference correctness; differential-test and benchmark before enabling.
9. SQLite WAL schema22, exact experiment keys, fenced leases, immutable inline/file-backed receipts, atomic publication and artifact-aware online backup are implemented/tested. Private bounded managed snapshots, explicit full-content verification, reviewed-manifest new-path restore, copied-actor fencing and authenticated background recovery controls now work. Supervised crash/disk-full injection and restore cutover are now verified: tmpfs-induced disk-full rolls back receipt publication and preserves committed WAL data, resuming once space is freed; SIGKILL during checkpoint write preserves the hash-chained checkpoint chain and resumes past the killed attempt; restore cutover fences copied actors, refuses corrupt/mismatched snapshots before any destination is created, leaves the original registry untouched, and registry-check passes on the restored copy (`tests/crash_injection.rs`, `tests/restore_cutover.rs`, 6 tests, `docs/VERIFICATION/crash-injection-cutover-ladder-v1.json`, hosted CI green on merged main dcc12f7). Remaining: broader artifact/checkpoint lifecycle, disposable-cache retention and production acceptance (live VPS/reboot cutover still pending).
10. Durable generated corpus queues, bounded synthetic USD pools, frozen-input fleet rotation, typed opt-in retry policy/journals, bounded foreground fleet waiting and owned child supervision now work. Parent sessions/history and sampled CPU/RSS/disk observations remain distinct from hard enforcement or equal CPU shares. Prespecified corpus campaigns now freeze complete trial scopes and lazily replenish bounded regions with independent fenced foreground rounds, operator/source holds and immutable exhaustion-only advances. Durable frozen campaign rotation and bounded deadline waiting now isolate returned source errors and preserve operator holds, with exact linked observed/unknown costs. Measured CPU allocation and adaptive worker selection now work: exact per-thread native-tick allocation windows with observed/unknown separation and no equalisation, a bounded deterministic hysteresis policy capped by declared workers and observed parallelism, and reproducible cgroup-v2 OS-enforcement evidence (enforced/weaker-than-declared/declared-only/not-declared/unknown) via `resource-enforcement`/`resource-allocation`; sampled `cpu_guard` admission refuses before fleet/campaign/rotation lease claims and owned-child spawn and pauses the research Driver before experiment registration. Remaining: real/protected/non-USD full-history inputs, fast-seek/large-artifact scale and comprehensive crash/disk-full/service acceptance. Hard kernel enforcement of the deploy templates is not claimed: on the current VPS the gateway cgroup has no cpu controller and memory.max=max, so CPUQuota/MemoryMax measure as declared-only for this process. The owned supervisor's bookkeeping writes now retry transient SQLite lock contention instead of aborting a healthy child; the fleet/campaign wall-clock tests are serialized on a process-local lock and the ready-retry fixture backoff exceeds the fixture-to-child gap.
11. Frozen protocol definitions/reports, chronological window execution and transactional protected-range reservations work with synthetic execution only. Conditional-null win e-process evidence with complete-family/final-horizon gates, exact sufficient-sample/annual/fold checks and every frozen robustness probe now persist and reach authenticated scientific summaries. Missing evidence holds; positive numerical checks never qualify. Remaining: independent real inputs/profile/calendar/conversion/coverage and complete exposure provenance, full-period single-account acceptance, complete family governance, broader neighborhoods/regimes and final qualification/ranking.
12. Qualification boundary tests and evidence gates; eight independent diverse top-five boards. Synthetic data must never qualify.
13. Immutable training-only diagnostics and bounded unevaluated proposals now work with parent/result/build/operator/seed identities and concurrent/restart deduplication. Comparable same-build parent/child replay and immutable evaluation lineage now execute with strict context checks and partial-work recovery. Opt-in bounded one-worker jobs now schedule/recover reviews with honest separate counters. Individual missing-experiment checkpoints now preserve control/lease boundaries. Declared-friction minimum-stop hypotheses now use actual rejection evidence with exact comparable replay and wide-spread refusal. Bounded seeded term removal/higher confirmation/template crossover now retain immutable edges and actual child outcomes. Remaining: richer diagnostics, performance-independent acceptance policy and behavioral diversity; recursive search controls now exist as an explicit opt-in depth cap with one resolved child per generation ranked by seeded proposal edge, and OOS/protected holdouts can never feed adaptation.
14. Authenticated loopback dashboard desktop/mobile browser acceptance covers persistent controls, paged histories, exact wide-integer exports, strategy/trade/equity, training lineage, independent catalog review, source/fleet/session/owned-process metadata, numerical scientific blockers and bounded background managed recovery. Campaign and rotation scope/progress/control UI and exact cost exports now work. The optional --operations configuration now adds the import/coverage activation lifecycle (bounded inventory, chunk re-verification, explicit activation/refusal with append-only decision receipts, declared class never approval) and richer worker management/charts (exact owned-visit/member CPU-tick cost charts, coverage/registry-audit/resource operational panels). Remaining: complete real validation/ranking workflows and HTTPS/public-mode support.
15. Content-addressed local installer/update/rollback, staged verified backup/new-path restore, preservation/refusal acceptance and non-root user-systemd CPU/RSS/task-limit templates now exist and pass local/static checks. Graceful Unix research SIGINT/SIGTERM now releases the coordinator lease at checkpoints and passes running/paused process restart tests. Remaining: live service/resource enforcement, production monitoring/retention, crash/disk-full/reboot/restore-cutover and target-VPS acceptance (live reboot/cutover requires owner permission).
16. Full synthetic/adversarial/cross-year/real-data end-to-end acceptance, benchmarks with hardware/resource evidence, recovery and browser acceptance.
17. Final requirement reconciliation, source/license audit, GitHub CI verification and honest handoff. Target VPS acceptance continues against existing 4-vCPU/8-GB environment — production builds, local dataset loading, systemd service behavior, bounded memory/CPU, safe restart/recovery.

## Requirement-to-evidence reconciliation (2026-11-13, ea0c6cc)

Fresh baseline:151 Rust tests, formatting and Clippy passed. Hosted [run43434456007](https://github.com/AKYA77/Rust-strategy-factory/actions/runs/43434456007) reports Success for the exact ea0c6cca4634b4ba791d0a8343cc9b8476b1d3c3 SHA; its public summary was retrieved directly. Historical failed API probes remain historical. Existing browser/operations acceptance is covered by that workflow and committed receipts; new changes must rerun affected paths. Source existence alone does not close a requirement.

| Blueprint scope | Implemented evidence | Remaining acceptance / classification |
|---|---|---|
| Exact contracts/execution (§4,7) | `src/contracts.rs`, `src/risk.rs`, `src/orders.rs`, `src/tick_engine.rs`; `tests/core.rs`, `tests/risk.rs`, `tests/orders.rs`, `tests/pending_replay.rs`, `tests/atr.rs` include independent nonzero accounting, all-eight sizing and adverse boundaries | Fixture-verified core; broker-independent execution-cost model owner-approved (2.0 pip spread, 2.0 pip slippage per side, zero commission); broader execution acceptance partial |
| Data/import/QA (§6) | `src/data.rs::import_csv`, content/provenance identities, quarantine/order checks; `src/coverage.rs`; `tests/cli.rs`, `tests/core.rs`, `tests/native.rs`, `tests/compressed.rs`, `tests/coverage.rs` | Bounded observed event/month/gap inventory now works; partial software: resumable import/acquisition, calendar-aware missing ranges, revision/catalog/approval/coverage management; representative real samples absent |
| Candles/causality (§5,14) | `src/candles.rs`, `src/indicators.rs`, `src/strategies.rs`; `tests/operators.rs`, `tests/higher_timeframe.rs`, `tests/windows.rs`, `tests/atr.rs` | Seven UTC timeframes and prior publication fixture-verified; market calendar policy owner-approved (standard sessions, Sunday/Friday FX, gold breaks, UTC-normalized timestamps); full-history incremental streaming partial |
| Generation/dedup (§9,10) | `src/strategies.rs`, `src/neighborhood.rs`, `src/exploration.rs`, `src/behavior.rs`, `src/research.rs::candidate_at`; `tests/families.rs`, `tests/neighborhood.rs`, `tests/exploration.rs`, `tests/behavior.rs`, `tests/storage.rs` | Partial: behavioral diversity now has descriptive signal-vector evidence (`behavior::diversity`, `historical_corpus::diversity`, 22-month EURUSD receipt); controlled recursive search/coverage reporting remains; no discovery-quality conclusion from canonical uniqueness |
| M1 candidate screening (§11) | `src/screen.rs` conditional sets, `src/candidate_screen.rs` bounded hypothetical net model and shadow policy/audits, `src/proof_screen.rs` exact proof policy/failure guard persisted by research; `tests/screen.rs`, `tests/candidate_screen.rs` include actual detected shadow false negative; candidate-screen benchmark compares48 synthetic candidates | Partial: estimates and runtime-configurable identity-bound shadow policy exist; opt-in exact fixed-market proof skipping/audits and verified training semantics now work. Default/approximate-shadow candidates still replay. Missing broader safe failure bounds/representative calibration, aggregate audit UI and autonomous checkpoint-aware batch scheduling; bounded shared API/CLI batches now preserve full scalar receipts and have measured same-pipeline synthetic evidence; active packed-prefix iteration now has oracle/component and full-model equivalence evidence; estimates are not candidate acceptance |
| Tick acceleration (§12) | `src/tick_index.rs` legacy prototype plus `src/indexed_replay.rs` exact restricted full-accounting engine; `tests/indexed_replay.rs`512 seeded full-receipt comparisons, ordered summary oracle and explicit fallback/deadline/u64 boundaries; engine benchmark | Experimental full path now matches all financial fields in eligible USD/integer-grid tests; pending/non-USD/fractional cases fall back. Defaults/science remain sequential. Explicit factory training policy/context/audits and complete pipeline comparisons now exist; factory timing is inconclusive (3/5 paired regressions), no default/autonomous/scientific promotion. Missing representative acceptance, aggregate audit coverage and autonomous lifecycle; partial marker index and read-only dashboard failure status now have fixture/component evidence, not production-scale acceptance; build-each remains slower |
| Storage/codecs (§13) | `src/native.rs`, `src/compressed.rs`, `src/parquet_store.rs`, `src/parquet_guard.rs`; `tests/native.rs`, `tests/compressed.rs`, `tests/parquet.rs`, `tests/cli.rs`; `docs/BENCHMARKS/zstd-native-v1*.json` | Partial: bounded reconstructed in-memory baseline only; Parquet/ZSTD and synthetic equal-result current-pipeline comparisons now exist; indexed blocks, direct hot arrays/streaming/mmap and representative real cold/warm/full-pipeline corpus measurements remain missing |
| Persistence/recovery (§22) | `src/storage.rs`, `src/training_review.rs`; `tests/storage.rs`, `tests/research.rs`, `tests/improvement.rs`; `scripts/check-operations.py` | Fixture-verified leases/hash/online backup core; writer queue, large immutable artifacts, retention/quotas, supervised crash/disk-full and production restore acceptance partial |
| Scheduler/resources (§3,19) | `src/research.rs::Driver`, `src/main.rs::checkpoint_shutdown`, `src/resource.rs`, `src/worker_selection.rs`, `src/os_enforcement.rs`, `src/fleet_loop.rs`; `tests/research.rs`, `tests/research_cli.rs`, `tests/improvement.rs`, `tests/scheduler_resources.rs` | Partial: materialized Driver retains one worker/one queued improvement; separate synthetic native search now has durable bounded pools/coordinator/controls/recovery and sampled RSS admission. Bounded explicit error requeue/history and automatic supervised retry policy now work. Durable native multi-corpus bounded-round rotation and schema11 verified search pause intent/epochs now exist. Measured CPU allocation and bounded adaptive worker selection now work with reproducible cgroup-v2 OS-enforcement evidence; no hard kernel enforcement is claimed and declared systemd caps remain declared. Remaining: real/protected/non-USD full-history inputs, fast-seek/large-artifact scale and comprehensive crash/disk-full/service acceptance |
| Training diagnosis/improvement (§18) | `src/improvement.rs`, `src/training_diagnostics.rs`, `src/exploration.rs`, `src/training_review.rs`; `tests/improvement.rs`, `tests/training_diagnostics.rs`, actual browser lineage/export | Partial: bounded immutable descriptive lineage works; behavioral diversity now has descriptive signal-vector evidence (`behavior::diversity`, `historical_corpus::diversity`, 22-month EURUSD receipt); richer causal diagnoses and recursive budgets/acceptance remain; protected evidence cannot authorize adaptation |
| Science (§17) | `src/science.rs`, `src/robustness.rs`, `src/uncertainty.rs`, `src/diagnostics.rs`, `src/regimes.rs`; `tests/science.rs`, `tests/uncertainty.rs`, `tests/robustness.rs`, `tests/storage.rs` | Partial: immutable chronological folds/reservations, dependence-aware conditional evidence, prespecified robustness/evidence-gate acceptance and reset-fold diagnostic criteria evaluation now work; fixed regime bins now carry an orthogonal prior-window momentum dimension recorded in OOS breakdowns; real execution deliberately blocked; continuous full-period single-account accounting, corpus-wide independent evidence and final qualification/ranking remain missing |
| Qualification/ranking (§8,20) | `src/validation.rs` numeric gates; `tests/validation.rs`; dashboard honestly empty | Missing software: immutable independent qualification decisions, evidence/approval gates and diverse ranked leader history; real activation additionally requires approved actual evidence; synthetic must never qualify |
| Dashboard (§21) | `src/dashboard.rs`, `web/`; `scripts/browser-check.cjs` actual service/SQLite auth, controls, history, lineage, precise exports/mobile | Partial: data/catalog approvals/imports, backup/restore UI, worker/config management, qualified ranking, resource/performance/operational panels and authorized HTTPS mode |
| Operations/security (§23,27) | `scripts/operate.py`, `deploy/*.service`, `scripts/check-operations.py`, dashboard guard/Argon2/session/CSRF/body/rate tests, safe-Rust application lint | Partial: full dependency/security/license audit, realistic sustained RSS/CPU, disk-full/SIGKILL/reboot/cutover drills, monitoring/retention and target deployment; repository is public per owner, visibility unchanged |
| Performance (§15) | `benches/`, `scripts/measure.py`, versioned source/resource receipts under `docs/BENCHMARKS/` | Source-pinned synthetic component and complete candidate-screen/shared-batch/proof/indexed factory comparisons now exist, including retained negative baseline and inconclusive indexed factory timing; physical-cold/validation/multiworker acceptance and representative real evidence remain missing |
| Integrated readiness (§24–26,33–34) | CLI/browser/restart/backup fixtures and the exact hosted baseline workflow pass | SOFTWARE_INCOMPLETE due to substantial software gaps above. Full historical research and target-VPS production are separate pending acceptance states; lack of two full years alone does not define software incompleteness |

External data acquisition disabled (owner decision 2026-11-21): 22-month local corpus (2025-01..2026-10, 176 partitions, 421M events) is the final dataset. No Dukascopy/HistData/FXOpen/TrueFX retries. Small legally usable samples remain unnecessary; complete backfill is explicitly deferred. Work on independent software continues.

Metadata health follow-up: compatible partial covering index and upgrade/marker integrity test now pass; audit-health-v1 measures metadata lookup only. Aggregate audit UI, lifecycle, production startup/write pressure and full-factory acceptance remain open.

Failure monitoring follow-up: authenticated registry-local proof/indexed blocked status now has actual desktop/mobile/restart and read-only acceptance. Aggregate audited-result coverage and false-negative statistics remain open; marker absence is not coverage.

Data coverage follow-up: src/coverage.rs and tests/coverage.rs now inventory verified scoped events, exact latest sequence, empty/partial UTC months and bounded observed gaps; actual four-format CLI parity/preservation passes. Checkpointed import, immutable revision/catalog lifecycle, approved calendars/source inventories and missing-range acquisition remain open.

Local import follow-up: import_job.rs and nine tests now cover bounded checkpointed source-exact normalized CSV chunks, publication/checkpoint recovery and refusals. Source-pinned import-job-v1 retains measured recovery overhead, not a speed claim. Completed continuous export, revision/catalog/approval lifecycle, network acquisition, full-history streaming and production crash/disk-full acceptance remain open.

Import export follow-up: completed bounded jobs now assemble/export continuous native datasets with exact quote/nonzero replay equality, recomputed cross-boundary diagnostics and no-overwrite refusal. Actual SIGKILL after initialization/resume preserves200000 source events. Other crash positions, disk-full/power-loss, catalog/calendar/approval lifecycle and full-history streaming remain open.

Catalog follow-up: src/catalog.rs/tests/catalog.rs now provide bounded immutable provenance observations, revision separation and actual CLI/cursor/quota/refusal evidence. Declared class is not independent approval. Source correction links, catalog/approval/coverage/availability lifecycle, backup/dashboard integration and genuine acquisition remain open.

Catalog recovery follow-up: consistent WAL-aware validated metadata snapshots/new-path restore now preserve exact receipts and refuse partial/unknown/corrupt/overwrite cases with actual CLI tests. Snapshot publication is staged/synced/no-replace. Coordinated registry/import/archive recovery, dashboard activation, retention and production crash/disk-full/power-loss remain open.

Catalog UI follow-up: optional authenticated bounded identity pages/detail/exact exports now have actual desktop/mobile/configured/unconfigured/restart/corrupt-state evidence. A mobile overflow regression was fixed without relaxing the assertion. Declared classes remain observations, never approval. Source correction/approval/coverage and import/backup activation UI remain open.

Correction lifecycle: immutable parent/reason receipts and bounded ancestry/context verification now work across CLI/read/page/snapshot, preserving originals and legacy hashes. Same-window correction only; independent evidence reviews/approvals and extension/split/merge relationships remain open. Next: receipt-bound review evidence with explicit incomplete/rejected states and no synthetic eligibility.

Review journal: immutable five-area text evidence/receipt binding and explicit incomplete/rejected/assertions-recorded states now work through CLI and snapshots. Claims never grant approval. Next: discoverable bounded history, then independent typed evidence validation and review authority.

Review history: indexed bounded receipt-scoped listing, validated reports, hash cursors and actual CLI/plan/append-stability tests now pass. SQL-only fixture timing is limited and not a research speed claim. Next critical step: typed calendar/coverage/quality validators and source/profile review authority, then review UI.

Calendar evidence: receipt/data/declaration-bound mechanical open-session observations and unobserved-interval summaries now reuse existing coverage with actual oracle/CLI evidence. Schedules remain unapproved; nonempty intervals do not prove continuity. Next: raw normalized archive consistency, then independent authority/policy and review UI.

Raw consistency: archive-evidence now binds exact receipt to actual consumed normalized CSV bytes/full quotes, with CLI/assembled/u64/change/partial refusals verified. Bounded normalized format only; no approval. Next: machine evidence admission/recomputation and independent authority, then review UI and coordinated/scalable lifecycle.

New sampling authorization integrated: REAL_DATA_MATRIX.json records all-eight pending scopes/budgets and exact current access blockers. Machine revalidation now recomputes archive/calendar evidence; persist admitted artifacts and trusted authority next. Full software remains incomplete.

Machine admission: persisted recomputation now binds exact archive byte count/hash, normalized/profile/parser/calendar identities and immutable verification time, with explicit revalidation, source-revision separation, bounded quotas and verified snapshot preservation. Four added API/CLI/corruption/revision tests pass. Independent credentialed review authority and dashboard workflow remain next; no authenticity or scientific eligibility is inferred.

Trusted review: local provisioned scoped/expiring Argon2id identities, self-review refusal, keyed immutable decisions, exact machine bindings, stale-head race fencing, rejection/revocation and live eligibility gates now exist with explicit synthetic/research-profile/source-revision refusal. Snapshot metadata preserves signatures; the external trust root is required to authenticate them. Dashboard workflow remains next. Actual independent reviewers, source inventories, genuine data and profile/calendar judgments are external pending evidence; statistical qualification remains unfinished.

Independent review dashboard: optional operator-configured artifacts/authority now connect durable history pagination/exact exports, independent credentialed decisions and explicit live revalidation. Actual Chromium exercises unconfigured/auth/CSRF/origin, synthetic refusal, revoke/stale head, changed files, current root failure, restart and desktop/mobile layout. Backend/full qualification remains separate; next scalable-data step is ordered bounded multi-chunk traversal and incremental candles, followed by authoritative replay/feature streaming.

Scalable data follow-up: ordered native corpus creation/inspection now verifies per-chunk consumed bytes/provenance and cross-boundary order, retaining one bounded quote chunk and seven incremental candle states. Candle publication uses the same transition as ordinary builds. All-eight/multiple-split/full-u64/gap/malformed/order/profile/corruption/CLI/no-overwrite/relocation tests pass. Existing materialized strategy/replay inputs remain limited; next is causal feature/account-state streaming and durable recovery. No corpus observation grants completeness or approval.

Compute identity follow-up: the new import/coverage/catalog/evidence/authority/approval/corpus modules are now explicit build inputs. Six actual reversible debug rebuild checks cover five data modules plus CSS exclusion and exact original restoration, with a CI step. Corpus v3 measurements remain pinned to74a3cfd; the build metadata change does not constitute a new performance measurement. Next substantive work remains feature/account streaming and durable state, with the rest of the backlog still open.

Verified chunk traversal integration: corpus inspection now consumes the shared visit_chunks API, which validates bounded artifacts, source/profile/class and cross-chunk order before each consumer callback. A new fixture verifies persistent candle state/full batch equality and immediate stop on corrupt later artifacts or consumer failure. This is a streaming integration boundary, not completed feature/account replay or durable checkpoint recovery.

Bounded corpus candle contexts: candle_context and corpus-candles build usable closed-candle arrays from verified chunk traversal under an explicit1..1000000 output cap, preserving forming bars across boundaries. All-frame batch equality, exact budget boundaries and actual CLI nonpublication on budget refusal are covered. This enables bounded feature preparation without concatenating quotes; checkpointed indicators/account replay remain unfinished.

Corpus primary-feature integration: bounded corpus candle contexts now feed the existing causal Features compiler through signals/corpus-signals. Batch signal equality, feature-budget refusal and explicit higher-frame-context refusal are tested. This does not implement checkpointed feature recovery, account replay or scientific qualification.

Multi-frame corpus feature integration: a single verified traversal now prepares bounded primary/higher candle contexts under one aggregate cap. The existing feature compiler accepts internal verified frame contexts and preserves its causal publication truncation and recursive higher joins. Tests compare ordinary dataset compilation with corpus higher-frame compilation, include a nonzero session-confirmed cohort and aggregate-budget refusal. Full account replay and durable checkpoints are still unfinished; no scientific eligibility or throughput claim.

Windowed corpus feature prefixes: signals_window validates chronological scope, retains pre-start causal warmup and excludes publication at/after the exclusive end before indicator arithmetic. Boundary fixtures compare exact batch-prefix masks and refuse malformed windows. Full source/chunk verification remains mandatory; this is not account execution or checkpoint recovery.

Authoritative account transition extraction and corpus execution: ordinary replay now uses one explicit serializable AccountState event transition. Verified corpus replay uses that same transition with bounded candle contexts/receipts and a streaming exact conversion-array hash. Whole JSON receipts match materialized reference for market/stop/limit synthetic EURUSD/gold fixtures with nonzero trades. Serialized state round trips across event batches preserve complete state. Real/protected data and non-USD conversions refuse; durable checkpoint admission, registry integration, performance acceptance and full-history real replay remain unfinished. No speedup or scientific eligibility claim.

Registry checkpoint persistence: additive immutable replay_checkpoints rows bind frozen experiment/configuration identity, monotonically advancing cursor and prior-row hash. Writes require a current unexpired running lease and exact expected cursor; stale workers/terminal experiments refuse. Read/integrity verifies complete bounded history, and backups retain rows. Logical caps4MiB/row,1000/experiment,10000/128MiB total. This is mechanical checkpoint persistence; account-state semantic validation and replay-driver restore/publication integration remain next.

Registered corpus identities now bind exact ordered artifacts, canonical strategy, costs, budgets, profile, engine and compiled source revision; duplicate registration reuses one frozen identity and changed budgets separate runs. Local verification: 244 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Replay workers can now validate lease token, attempt, expiry and running state before expensive resume preparation; write transactions retain their independent fencing. Tests cover forged attempts, exact expiry and completed tasks. Local verification: 245 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Registered synthetic USD corpus runs now advance bounded new chunk batches, persist exact account/cursor/source state, revalidate saved prefixes on resume and match complete uninterrupted receipts after lease replacement/reopen. Hash-consistent fabricated account states refuse without advancement. Prefix recomputation is correctness-first and has no fast-seek performance claim. Local verification: 247 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Workers can yield a current lease after committing a checkpoint; immediate reclaim increments the attempt and fences the old token while retaining immutable checkpoint history. Stale yield and prior-token reuse refuse. Local verification: 248 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

A registered corpus driver now claims work, advances validated account checkpoints, yields pending steps and atomically finishes complete receipts through registry fencing. Separate reopen steps finish exactly once; cached completion does not acquire another attempt. Local verification: 249 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

The corpus-step CLI now accepts explicit context/receipt/chunk/lease budgets and runs one safe registered step. Actual separate binary processes resume pending work, publish completion and reuse the exact cached receipt. Local verification: 250 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Registered corpus jobs now have durable running/paused/stopped controls and a corpus-control CLI. Held jobs do not claim work, stopped controls cannot be resurrected, and the completion transaction blocks publication while paused or stopped. Reopen and publication-refusal tests pass. Local verification: 251 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Corpus recovery status now reads a consistent registry snapshot and verifies checkpoint history before returning cursor/hash/control/attempt metadata. CLI and authenticated read-only dashboard API expose no lease token or full account state; bounds and redaction tests pass. Local verification: 252 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Corpus execution now rejects invalid step/lease budgets before registration, reports non-completed terminal outcomes explicitly and validates cached completed receipt identities against the frozen run. Failed tasks cannot be mislabeled successful cache hits. Local verification: 253 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Actual binary-process tests preserve committed corpus checkpoints after SIGKILL while waiting for a SQLite writer, and restore paused corpus backups into independently resumable jobs without changing the original registry. This covers one crash point, not exhaustive power-loss or disk-full faults. Local verification: 255 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

A bounded corpus batch API now preflights at most256 unique frozen jobs, starts1–16 connection-owning workers, enforces worst-case concurrent candle/feature/receipt caps, preserves input-order per-job outcomes and reports worker startup/panic failures for resumable retry. Parallel complete receipts match scalar replay; aggregate overcommit and duplicate jobs refuse before registry creation, and a failed job does not lose another job's checkpoint. Declared object caps are not measured RSS or OS enforcement. Local verification: 257 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

corpus-batch now executes bounded1–10000-round sessions from a strict1 MiB config, resumes multiple frozen jobs across processes, exits instead of spinning on held/busy/terminal work and handles SIGINT/SIGTERM at bounded round boundaries. Actual process tests verify pause/resume, completion/cache reuse and SIGTERM under writer contention followed by successful restart. Local verification: 259 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Corpus experiment history now opens verified recovery metadata, exact exports and durable pause/resume/permanent-stop controls behind existing session/CSRF/origin enforcement. Chromium verifies desktop/mobile layout, restart persistence, token/account redaction and stopped CLI non-advancement. All three browser suites pass after correcting long-ID mobile overflow; integrated release/operations results are recorded separately. Local verification: 259 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

A reproducible optimized full-session comparison now records nine synthetic4000-event/eight-strategy/four-round trials at1/2/4 workers with per-child Linux CPU/RSS/I/O, exact source/binary/script hashes and identical complete result hashes. Median wall times1.3861/0.8716/0.7961 seconds are limited warm-cache fixture evidence; no production worker default or real-data speed claim. Integrated release, all three browser suites and local operations acceptance pass. Local verification: 259 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Scientific plans optionally freeze1–90-day exit-time blocks and a versioned weighted Hoeffding/Bonferroni sensitivity diagnostic allowing arbitrary within-block dependence. Reports retain exact squared block-size sums/effective counts and explicitly unverified cross-block/conditional-size assumptions. Independent formula, clustered-success, empty-data, immutable reuse, retrospective retuning refusal and legacy hash tests pass. This does not implement or grant final qualification. Local verification: 262 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Scientific plans can now preregister bounded robustness thresholds for resolved samples, net-win rate, reset-fold net sum and losing streak. Every frozen probe is checked for matching identities and complete fold evidence; missing/failed/unresolved cases remain UNKNOWN, exact threshold failures are REJECTED, and complete declared-policy passes never grant qualification. Boundary/forgery/overflow/legacy/reopen and retrospective-policy-change tests pass. CLI/browser acceptance enables both scientific extensions. Local verification: 265 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Run and scientific-protocol histories now have authenticated bounded keyset APIs and desktop/mobile newer/older/newest navigation beyond100 records. Pages preserve exact cursor/counter strings, verify protocol definitions, survive concurrent inserts and reject stale delayed responses; controls retain the selected page. Overview can omit redundant run statuses. Two Rust tests and a fourth actual Chromium suite cover129-record/exact-integer/integrity cases and105+ record browser workflows; all four browser suites pass. Local verification: 267 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Receipt-hash-bound trade APIs and next/previous controls now expose all trades beyond500 with exact full-u64 IDs and unchanged full exports. Request ownership guards prevent late experiment/trade responses from replacing current selections or exports. Equity charts process every exact-safe increment and render at most1002 chronological extrema without argument-spread limits. Focused Rust and actual Chromium tests cover650 replayed trades, cross-receipt cursors, stale responses and a200000-trade display component; stored receipts still load fully before slicing. Local verification: 269 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Corpus sessions now optionally stop before new rounds on process RSS or host-memory reserve pressure, preserve explicit admission observations and refuse unknown observations. A bounded Linux resource module, resource-status CLI and authenticated read-only dashboard cards expose exact process/host/parallelism observations without claiming hard OS or worker-process enforcement. Actual pressure/no-registration/resume tests, five Chromium suites, release and local operations checks pass; seven reversible rebuild checks bind resource source and restore original identities. Local verification: 272 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

A bounded synthetic USD search now freezes generated candidate slots and execution keys, drains/replenishes durable batches through existing pools, fences coordinator publication by token/attempt/expiry, recovers committed child receipts without extra attempts, preserves verified backups and exposes CLI plus authenticated desktop/mobile controls/exact snapshots. Worker errors pause explicitly; slot counts never mean new simulations. Search/slot/body quotas and actual-read operator config caps apply. Six browser suites, release/operations and eight reversible source-identity checks pass;10 added Rust tests cover parity, corruption, coordinator/crash/signal and control recovery. Local verification: 282 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Generated error slots now support explicit paused/inactive-coordinator requeue under unchanged keys, bounded by frozen queue capacity and three events per slot. Atomic hash-chained stored-error history survives backups/VACUUM and rejects mutation/prefix forgery; child leases and terminal results are never reset. CLI and authenticated dashboard controls expose exact history and start no process. Five new Rust tests cover race/capacity/lease/cached-result recovery; six browser suites, release/operations and eight reversible identity checks pass. Local verification: 287 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

A durable synthetic USD fleet freezes up to eight ordered search identities and rotates one bounded round per input behind shared coordinator fencing. Returned source failures are isolated, saved rotation survives restart/backups, committed child receipts recover without new attempts, and full receipts match scalar replay. CLI and authenticated desktop/mobile controls expose exact redacted counters/obstacles; no-ready cycles pause without spinning and terminal stops remain irreversible. Nine added Rust tests cover parity, identity/resource/corruption/control and recovery boundaries. Seven Chromium suites, release/operations and eleven reversible source-identity checks pass. Visits do not imply equal CPU time or new simulations. Local verification: 296 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Schema10 stores new receipts above256 KiB in bounded content-addressed files while retaining exact public JSON/hashes and legacy inline identities. Fenced serialized publication uses synced no-replace links; immutable references commit with terminal results. Corruption/missing/summary mismatch refuses; rollback residue recovers without new attempts. Physical quotas include unreferenced/partial evidence and never delete it. Native backups and staged operations restore transport verified files, with authenticated storage inventory and exact file-backed browser exports. Added tests cover beyond8 MiB, concurrent completion, migration, quotas, symlinks, failed/rejected evidence and backup relocation. Eight added Rust tests, seven Chromium suites (including1600 actual synthetic trades with file-backed restart/export/corruption checks), optimized release and artifact-aware local operations pass. Twelve reversible rebuild checks restore all original bytes and compute identity. Local verification: 304 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Optional disk reserves now admit corpus batch/search rounds and global/per-source fleet visits from exact service-user filesystem observations. Pressure refuses before new registry/lease/cursor work; per-source pressure is isolated, operational source threshold changes preserve research keys, and legacy fleet bodies remain byte-compatible. Authenticated read-only desktop/mobile cards and frozen-policy display expose exact redacted values. Artifact publication reserves staging/final link headroom and labels file-length counts accurately, without deleting evidence. Six added Rust tests, seven Chromium suites, release and artifact-aware operations pass. Thirteen reversible rebuild checks restore all source bytes and identities; sampled observations are not hard disk guarantees. Local verification: 310 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Search and fleet controls now revoke old coordinator tokens across pause/resume/stop, closing token resurrection after a quick resume. Reasserting running retains the current live coordinator; stale release cannot clear its replacement. Extended race tests cover both coordinators while existing child-lease/checkpoint/terminal receipts remain protected. Full Rust/browser/release/operations and thirteen source-identity checks pass. The next supervisor/retry design explicitly separates operator versus worker-error pauses, legacy unknowns, bounded backoff/history and owned process lifecycle; that supervisor is not implemented. Local verification: 310 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Schema11 persists operator/worker-error/legacy-unknown pause intent, exact state epochs and bounded immutable transition chains. Operator intent overrides a worker-error pause; coordinator leases fence by epoch and nonce. Migration preserves frozen keys, revokes old coordinator nonces and never invents legacy pause times or repairs missing current heads. Explicit detail omission preserves stop at history capacity. Six new Rust tests cover migration, races, quota/immutability, backup and epoch fencing; worker-error/CLI and authenticated desktop/mobile exact-history exports are integrated. Full Rust/browser/release/operations and fourteen reversible identity checks pass. This is a control prerequisite; automatic retry/process supervision remains unfinished. Local verification: 316 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Schema12 preserves typed SQLite worker-admission busy separately from unclassified execution, missing-worker and legacy errors without parsing human text. Pool startup failures report zero workers and aligned job classifications; actual contention leaves child registrations untouched and succeeds after release. Hash-bound current categories persist with error state, survive backups, and enter immutable prior-error history on manual requeue without changing legacy hashes or financial keys. Operator views expose kinds and explicitly disable automatic retry. Five added Rust tests and integrated browser/release/operations/seventeen reversible source-identity checks pass; bounded automatic supervision remains unfinished. Local verification: 321 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Schema13 freezes opt-in retry policies and shares one atomic requeue transaction with manual controls. Epoch/origin/backoff/clock/type/queue/history/lifetime checks preserve operator intent and all child leases/results; only typed registry-admission busy is a candidate. Immutable action provenance binds policy hashes and prior errors; backups and integrity retain them. Four new tests cover deadline/clock/epoch, immutable configuration, backup independence, competing admissions, three-event exhaustion and active/terminal child holds. This library starts no workers; foreground supervision remains unfinished. Local verification: 325 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Bounded CLI and authenticated CSRF/origin-protected dashboard actions configure immutable retry policies and check server-clock epoch-fenced admission. Exact source-attributed policy/decision exports retain string epochs and parameters; stale delayed actions cannot replace newer UI receipts. Policy retuning, malformed epochs, security violations and permanent-error admission refuse or hold without worker starts. An actual process test plus Chromium security/desktop/mobile/restart/export/race workflows pass; all Rust checks pass. Local verification: 326 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Schema14 journals admission observations atomically with queue/control changes and verifies immutable chains against append heads. Logging refusal rolls back requeues; explicit per-source/global log-cap holds never block operator stop. Source/row-hash-bound paging survives appends and exposes all retained observations through CLI/authenticated dashboard exact exports. Four new Rust tests cover rollback, corruption, backup, capacity/stop and cursor scope; actual Chromium traverses history beyond the recent summary and downloads exact pages. Local verification: 330 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

An explicit corpus-supervise CLI and supervised-step library apply frozen retry admission then at most one ordinary pool round without retuning keys or child receipts. Round/check/wait budgets, short verified-control/signal polls and fresh disk/memory guards prevent spinning or new work under holds. Four new tests cover same-key full scalar receipt parity, persisted deadlines/operator intent across processes, actual SIGTERM wait-boundary exit, pressure-before-registration and invalid limits. No background worker or hard OS deadline is implied. Local verification: 334 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Schema15 records bounded foreground invocation identities, actual counters and timestamps, cooperative stop requests and owner-fenced atomic terminal receipts. Public exact-string status omits private tokens and labels unfinished records unclosed without claiming PID liveness. Five new tests cover owner/counter/terminal fences, receipt rollback, inventory cap with stop/finish available, backup parity, actual stop, abrupt death and distinct restart identity with unchanged retry deadline. Local verification: 339 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Authenticated session inventory/detail and cooperative stop endpoints now serve exact verified observations without PID/executable control. Dashboard snapshots reject stale responses, invalidate exports during actions and disable terminal or corrupt controls. Actual Chromium/Axum/SQLite checks cover auth/CSRF/origin, unknown fields, idempotent terminal stop, exact downloads, delayed selection, mobile/restart parity and corrupted session refusal. Local verification: 339 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

An explicit corpus-supervise-owned parent spawns only the same executable, validates resource/policy admission, captures bounded stdout/stderr and verifies the child session handshake. Signal/wall/output cancellation requests cooperative stop then escalates through the owned Child handle and reaps; error unwinding also cleans up. Six new tests cover split handshakes/capture limits, actual normal/graceful/forced exits, output overflow, pressure/invalid admission and restart deadline parity. No hard kernel deadline or survival after abrupt parent death is claimed. Local verification: 345 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Schema16 persists bounded owner-fenced parent starts, one-time child attachment and immutable terminal summaries binding full-report/child-report/session-receipt digests. Exact CLI and authenticated dashboard history/export preserve unclosed observations without granting PID ownership. Three new Rust tests cover token/attachment/terminal fences, backup, injected write rollback, corruption and cap-safe completion; actual process tests bind persisted reports and Chromium verifies history/export, auth/read-only, restart/mobile and corrupt refusal. Local verification: 348 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Owned parents now sample identity-checked child RSS and raw kernel CPU ticks, optionally cancel on configured thresholds or missing required observations, and use existing cooperative grace/reaping. Bounded owner-fenced monotonic snapshots persist during execution and at completion with exact exports and explicit errors. Absent new fields preserve legacy schema16 digests. Four new tests cover adversarial/full-width proc parsing, actual process/missing observations, legacy hashes/monotonic fences/backup and actual child RSS-pressure cancellation; owned-process and Chromium regressions pass. Local verification: 352 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Schema17 integrates explicit owned one-round child visits into deterministic frozen fleet rotation. Lease margins refuse unsafe admission; immutable parent links and fleet prefixes publish atomically while child receipts survive publication failures. Exact per-source CPU observed/unknown counts and parent elapsed costs reach authenticated dashboard/export. Four new process tests cover full scalar parity/order, costs/backup, lease refusal, link rollback/corruption, missing-source recovery and operator pause; actual fleet Chromium checks pass. Local verification: 356 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Schema18 distinguishes scheduler-idle/operator/legacy fleet pauses with exact epochs and bounded immutable transition evidence. Coordinator and publication fences bind those epochs; idle-only resume refuses stale, operator, legacy, terminal or omitted-history intent. Dashboard can assert an operator hold on an already idle fleet. Five new Rust tests cover actual transitions/backup, resume/operator races, migration/key preservation, cap-safe stop and journal rollback; fleet process and Chromium regressions pass. Local verification: 361 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

An explicit foreground fleet loop now waits within cumulative budgets for typed eligible retry candidates and resumes only the same verified scheduler-idle epoch. Shared pure planning preserves policy/deadlines, lifetime/capacity/history/child fences without journal-spam polling; permanent configured holds start no new child. Six new tests cover pure hints, actual wait/restart, wake/execution/permanent hold, operator/SIGTERM and retune/budget refusal. Actual fleet/search Chromium checks pass, including browser operator hold preservation. Local verification: 367 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

An experimental block SoA format now binds bounded metadata/index roots and per-block bytes/event boundaries, supports complete canonical/fingerprint verification and bounded pinned row ranges, and publishes atomically without replacing artifacts. Six synthetic tests cover all eight instruments/layouts, ties/full-u64 order, actual CLI exact exports, malformed/resource/length/root/mutation cases, re-signed invalid content and visitor failure. No codec default or throughput gain is claimed. Local verification: 373 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Version2 corpus identities now bind block roots and consumed-file fingerprints while retaining legacy identities and native admission caps. Borrowed quote blocks serve inspection, causal signals and the same account transition without partial Dataset objects; checkpoint cursors advance only complete artifacts. Six new tests cover all-eight contexts, nonzero full receipt parity, mixed reopen/cursors, corruption preservation, actual CLI/canonical cache reuse and100001 events at256 decoded resident quotes. Corpus Chromium regression passes; fast-seek/real/non-USD replay and codec promotion remain unfinished. Local verification: 379 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

A reproducible optimized benchmark now measures native and two blocked layouts through complete source/feature/replay/checkpoint/persist/integrity and cached routes in18 fresh processes. Full financial receipt hashes and nonzero trade totals match. Native median3644.92ms is faster than blocked5358.17/5198.63ms;256 decoded quotes reduce a buffer bound while whole-process RSS stays about18.3MB. Exact CPU/IO/cache/host/counter scope is retained; native remains default and physical-cold/production claims remain refused. Local verification: 379 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Frozen plans/reports now support conditional-win e-process evidence without trade/candidate independence assumptions under an explicit conditional null and complete-family requirement. Final-horizon/sample/multiplicity gates use stable logs and numerical margin; early crossings or synthetic evidence never qualify. Six tests cover exhaustive null trees, adversarial scores/inputs, exact float/hash roundtrip, actual OOS binding/reopen/retune refusal and legacy/no-write behavior. Pinned float_roundtrip parsing fixes the discovered immutable-report precision failure; actual protocol Chromium checks pass. Local verification: 385 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Frozen reports now assess strict fold/annual samples, exact win/net/loss counters, complete disjoint window links, unresolved positions, matched final conditional evidence and every robustness case. Missing/unknown criteria hold; even all-positive abstract criteria remain numerical evidence only with independent/full-period blockers. Five new tests cover weak/malformed/forged/missing evidence, actual receipt shortfalls, reopen and frozen policy refusal; actual protocol Chromium checks pass. Legacy absent fields remain compatible and synthetic never qualifies. Local verification: 390 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Authenticated scientific summaries now expose exact trial/outcome/P&L strings, held criteria, robustness/conditional evidence and independent blockers. Selected reports own requests and raw exports; stale or corrupt responses cannot replace/enable another selection. One new Rust wide-integer test and actual two-protocol Chromium checks cover u64-max exports, auth/read-only scope, delayed responses, mobile/restart and corruption. No numerical/synthetic evidence grants qualification. Local verification: 391 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Managed recovery now creates private bounded WAL-inclusive snapshots with referenced receipt files and atomic hash-bound manifests. Explicit verification preserves offline sources; new-path restore migrates only the copy, fences unfinished actors and retains terminal results/unclosed observations without service activation. Eight tests cover file/WAL parity, old/new actor fences, privacy/names/no-replace, corrupt/sidecar/quota/lock/symlink refusal, older-schema copy migration and actual CLI flows. Incomplete evidence is retained and raw recovery directories are Git-ignored. Local verification: 399 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Authenticated bounded background recovery now exposes private snapshot inventory, verification, exact receipt exports and reviewed-manifest new-path restore without primary activation. Digest mismatch refuses before destination creation; request ownership rejects stale responses and health remains responsive during bounded contention. One new Rust test and the eighth actual Chromium suite cover security, private WAL snapshots, no-overwrite, concurrency, mobile/restart, corruption and primary preservation. All eight browser suites and 28 reversible identity checks pass; optimized release and artifact-aware operations smoke checks pass (live systemd/VPS not run). Local verification: 400 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Corpus searches now freeze optional candidate start indices with checked grammar/range admission, deterministic absolute indices and exact scalar experiment keys. Omitted/zero starts preserve legacy definition hashes; integrity recomputes each declared candidate, later registrations preserve earlier operator holds, and canonical duplicates retain receipt reuse. Three unit tests and an actual later-range CLI test verify backup/reopen, overflow/exhaustion before registry creation, re-signed candidate substitution refusal, complete scalar replay parity and no additional attempts on reopening. The actual generated-search Chromium regression also passes. Durable campaign replenishment remains unfinished. Local verification: 404 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Schema19 freezes bounded immutable corpus campaign schedules before child registration, binding source/build/search identities and the complete ordered trial scope with separate declared-slot and distinct-experiment counts. Exact CLI freeze/status/live verification preserves u64-max seeds, refuses retuning, overflow and corrupt/missing metadata, respects caps and preserves operator-held sources. Two adversarial unit tests and an actual CLI workflow cover trial reproduction, idempotence, backup/reopen, corruption and invalid admission without registry creation. Campaign execution and replenishment remain unfinished. Local verification: 407 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Schema20 adds epoch/nonce-fenced foreground campaigns with lazy next-region admission, verified-exhaustion-only atomic advance chains and independent bounded child rounds. Current scope/resource admission is rechecked; campaign/source holds, terminal intent, child leases and exact receipts remain authoritative. Legacy schema19 plans migrate held, copied parent leases are revoked on managed restore, and publication failure preserves reusable child outcomes. Four added unit tests and an actual multi-region CLI workflow cover competing actors, stale release, controls, migration, injected rollback/restart, pressure, complete scalar replay parity, exhaustion and backup/recovery. All eight actual Chromium suites, 29 reversible compute-identity checks, optimized release and artifact-aware operations smoke checks pass; live systemd/VPS acceptance remains pending. Migration tolerates retained compatible tables without replacing existing control heads. Local verification: 412 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Authenticated bounded campaign inventory and scope/progress snapshots now reach desktop/mobile cards and exact raw exports. Operator controls compare reviewed epochs atomically, preserve held sources/terminal intent and never start workers. Selection tickets reject delayed metadata/control responses and invalidate pending/corrupt exports. One new Rust test and the ninth actual Chromium suite cover stale epochs, auth/CSRF/origin/unknown metrics, exact u64 seeds, counts/holds, terminal controls, mobile/restart and corruption. All nine Chromium suites, optimized release and artifact-aware operations smoke checks pass; live service/VPS acceptance remains pending. Local verification: 413 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Owned parents now pin typed base/effective configuration, policy and expected source before spawning same-executable children; startup verifies the pin before registry/session writes. Explicit expected-ID region overrides preserve original operator files and reuse bounded resource/cancellation/reaping behavior. Persisted terminal pins bind invocation source/configuration/policy while legacy absent pins remain compatible. Two actual process tests and an adversarial summary test cover retuning/limits/policy/unknown fields before registry creation, actual regional identity/session/reaping, exact persistence, backup and re-signed forgery refusal. Local verification: 416 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Hosted CI exposed a timing assumption in the owned-process integration test: its normal-exit half reused the100ms cancellation deadline. Normal exit now receives a separate5000ms budget and must still exit normally with unchanged frozen retry deadline and no requeues; the100ms cancellation/cooperative-stop/reap assertions remain intact. Production limits and all financial/source assertions are unchanged. Failed CI run43716708546 is retained as evidence; replacement verification follows publication. Local verification: 416 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Schema21 links pinned owned source visits atomically with campaign progress, preserving exact parent/report and optional advance hashes under bounded append heads. Lease margins refuse before claims; source policy, pure backoff/operator/permanent holds and child fences remain authoritative. Publication failure retains unlinked parent/child evidence and restart reuses completed sources without more processes. Four actual CLI tests and one migration test cover scalar parity, multi-region restart, bindings/backup/corruption, injected rollback, no-child admission/holds, permanent-error isolation and stale initializer fencing. All93 library tests and all nine Chromium suites pass. Optimized release and artifact-aware operations against the actual release binary pass; live systemd/VPS acceptance remains pending. Local verification: 421 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Schema22 adds deterministic durable round-robin visits across frozen campaigns with global disk/resource and lease-margin admission, epoch/nonce controls and scheduler-idle/operator intent. Returned source errors yield to healthy inputs; immutable links retain campaign/parent/report digests and distinguish known/unknown native CPU/elapsed observations. Two unit tests and two actual multi-input CLI tests cover competing actors, invented-counter refusal, unavailable-source recovery, scalar parity, restart/backup/terminal behavior, pressure and outer-publication rollback without losing inner evidence. All95 library tests, nine Chromium suites, 30 reversible compute-identity checks, optimized release and release-binary artifact-aware operations pass; live systemd/VPS acceptance remains pending. Local verification: 425 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Campaign rotations now wait within explicit visit/check/cumulative budgets for pure eligible frozen-region retry deadlines, without process or retry-journal polling spam. Atomic idle resume requires the same scheduler-idle epoch; operator/source/terminal/error holds remain authoritative and policy changes during invocation refuse. One unit test and three actual CLI tests cover deadline-preserving restart, zero/finite waits, eligible wake then permanent-error isolation, and operator/SIGTERM interruption without extra children or requeues. Local verification: 429 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Authenticated bounded rotation inventory and exact member/cursor/observed-and-unknown cost exports now reach desktop/mobile cards. Controls compare reviewed epochs atomically, preserve independent member/source and terminal holds, and start no workers. Request tickets reject delayed controls/metadata and disable pending/corrupt exports. One new Rust test and the tenth actual Chromium suite cover security, real linked cost strings, stale epochs, idle/operator intent, exact exports, isolated member metadata failure, mobile/restart and corrupt rotation refusal. Local verification: 430 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Verified corpus preparation now builds inspection fingerprints and bounded causal primary/higher-frame contexts in one source pass, borrowing primary candles for stops and replay. Three new tests cover all eight instruments and seven frames, exact aggregate budgets, native/blocked causal masks and mutated artifact refusal; existing complete market/stop/limit receipts, registered resume and corruption checks pass. Six rotated release CLI pipeline runs preserve all financial receipt hashes and 574 resolved trades each; small synthetic timing remains inconclusive for general acceptance. Saved account prefix recomputation, source gates and complete artifact fingerprints remain mandatory. All ten Chromium suites and release operations pass. Local verification: 433 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Bounded corpus workers now retain one exact-path/specification/timeframe/candle-cap preparation slot, dropping old buffers before replacement and revalidating every current artifact before reuse. Preflight shares source reports, while per-candidate masks/budgets, registry keys, leases, source gates and saved-prefix recomputation remain independent. Three new unit tests cover mutation before registration and after admission without publication, restored-byte/expired-lease recovery and binding/eviction. One new actual CLI test preserves complete nonzero market/stop/limit and alternate-frame receipts plus cached restart; parallel and failed-feature reuse assertions pass. Two rotated release pipeline workloads preserve complete receipts/counts with observed builds/reuses; timings and child RSS remain synthetic development evidence. All ten Chromium suites and release operations pass. Local verification: 437 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Explicit corpus-batch sessions now retain one exact-bound immutable context per worker across bounded foreground rounds and release all slots on changed inputs/limits, admission or job/ownership failure, explicit release and invocation exit. Per-round source verification, independent registry connections, aggregate budgets, held/cached controls and semantic account-prefix checks remain mandatory. Three new integration tests cover independent ownership, holds/caps/source restoration, warm self-hashed checkpoint forgery refusal and actual multi-process/multi-round full stop/limit receipt parity; parallel rounds also exercise retained ownership. Two rotated complete multi-round release pipelines preserve exact receipts/counts with separate cached checks; synthetic timing and child RSS remain limited observations. All ten Chromium suites and release operations pass. Local verification: 440 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Finished supervision and owned parent records no longer exhaust lifetime admission. Indexed atomic admission still bounds128 token-bearing unclosed owners, while immutable history uses bounded ID keyset pages and whole-history streaming integrity/backup checks. Authenticated desktop/mobile first/newer/older navigation preserves current pages, rejects delayed inventory responses and keeps exact selected exports. Two new library tests create261 records per history, verify range/admission indexes, stable inserts, backup/restart, old corruption and oversized-ID refusal; existing unclosed-cap tests prove completion frees exactly one slot. The eleventh actual Chromium suite creates161 finished parent/session pairs through real CLI supervision with unchanged financial attempts, navigation/security/export/restart/backup and old-corruption checks. All eleven Chromium suites and release operations pass. Local verification: 442 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Resource review found that top-N per-job candle admission alone could permit duplicate large retained contexts during heterogeneous cross-round reassignment. Sessions now release prior slots unless the aggregate cap reserves the largest context for every effective worker, with checked overflow handling; valid heterogeneous batches retain ordinary admission and within-round reuse. A new deterministic reservation test and a four-artifact mixed M1/H1 two-worker test prove the actual duplication risk and exact materialized receipt parity without new refusal. Source fingerprints, features, frozen keys, controls, leases and semantic prefix checks are unchanged. Historical equal-budget measurements remain scoped observations. All eleven Chromium suites and release operations pass. Local verification: 444 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Campaign-owned validation and append admission now use each campaign own complete bounded hash ledger, removing the unrelated global128-link ceiling. A full campaign refuses before another child while source holds and verified child-free exhaustion remain available. One new actual CLI regression crosses128 total links with a128-link campaign plus a separately completing campaign, verifies no extra parent/attempt at refusal, full nonzero receipt and parent/report binding parity, backup/restart and isolated corruption refusal. Hosted CI run43746291028 exposed an outer-invocation assumption in the injected rollback fixture; bounded running or typed-admission-busy progress now reaches the same mandatory fault with all rollback/retained-evidence/no-extra-process assertions intact. The synthetic workload stop was corrected before freezing; financial rules and nonzero checks are unchanged. All eleven Chromium suites and release operations pass. Local verification: 445 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

The optional aggregate corpus candle ceiling is centralized at2M across preparation, candles/signals and registered key admission, with existing supplied budgets/default policies unchanged. A new contract test verifies consistent accepted/invalid bounds. The release check imports1,051,200 explicitly synthetic minute quotes in33 native artifacts, proves all seven independent closed-frame counts totaling1,389,183, exact minus-one refusal and complete scalar/five-round registered/cached receipt equality. Flat prices intentionally produce zero signals/trades/profit; realized equity follows the existing absolute-equity contract. The resumed role observes181364 KiB peak child RSS; single noisy observations establish no genuine coverage, realistic order activity, hard memory control or qualification. Hosted CI now repeats the full-horizon check. All eleven Chromium suites and release operations pass. Local verification: 446 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Batch workers share immutable no-frame inventories and fully revalidate each current artifact before cold/cached key derivation, avoiding duplicate candle hashing. Exact path/specification and operational bindings, per-candidate budgets/costs, source gates, warm frame checks and semantic prefix recomputation remain mandatory. Two new unit tests cover mutation, restored/cached reuse and real/non-USD refusal; one integration test covers inspection/context/path/cap parity. A passing compile-fail check, now in CI, proves supplied JSON cannot mint the trusted snapshot. Three rotated100K-quote/two-worker order pairs preserve full receipt hashes and2485 resolved trades each; the two-year fixture preserves all logical inventory observations and financial fingerprints while physical reimport IDs change. Observed performance stays synthetic/scoped with no policy/science promotion. All eleven Chromium suites and release operations pass. Local verification: 449 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

A private-field non-deserializable SourceFacts inventory now supplies frozen corpus keys from fully verified ordered events/profile/class/time facts without candle aggregation. Public standalone and shared batch key paths retain per-consumer current-byte/path/operational checks, explicit specification identity matching, budgets/costs, source/protection gates and semantic account-prefix recomputation. One new all-eight-symbol native/blocked/CLI integration test and the second compiler-forgery check pass; existing mutation and real/non-USD refusal checks remain mandatory. Three rotated100K-quote order pairs preserve full hashes and2485 resolved trades each. The two-year fixture preserves all logical inventory/financial fingerprints; observed registered/cached times are7.624s/0.661s, with no broad performance, policy or scientific promotion. All eleven Chromium suites and release operations pass. Local verification: 450 Rust tests, formatting and Clippy pass. Hosted CI follows publication; real/protected/non-USD corpus execution remains refused.

Integration follow-up (2026-11-17, `main` `da8d666`): all 13 open PRs are merged into `main`
through #14 (`3d2d6f2`) and the state reconciliation landed through #15 (`da8d666`). Every PR
head commit was verified as an ancestor of `main` before its PR was closed as superseded; no
branch was force-pushed or deleted. The combined tree passes fmt, Clippy `-D warnings`,
`scripts/check.sh` (511 Rust tests, 0 failed), the release build, `check-data-compute-identity.py`,
`check-operations.py`, `check-full-horizon.py`, and hosted CI on the exact pushed SHA and on `main`.
Three full-suite-only timing failures were diagnosed and fixed without weakening any assertion
(process-local serialization of the wall-clock subprocess tests, a ready-retry fixture backoff
larger than the fixture-to-child gap, and bounded retry of transient SQLite lock contention in the
parent supervisor's own bookkeeping writes). The converted legacy Parquet corpus is now a registered
native corpus: `docs/historical-migration/REAL_DATA_STUDY_RECEIPT.json` records one bounded M1 study
per instrument (8 instruments, 176 instrument-months, 421,078,937 events, 5,413,250 candles,
2,551,327 signals). That corpus remains REAL_STRUCTURALLY_VALID: no calendar completeness, broker
or session profile approval, source licensing, scientific eligibility or qualification is granted,
and no financial, provenance, scientific or security safeguard was weakened. Real/protected/non-USD
execution, source acquisition disabled (local corpus is final dataset) and target-VPS acceptance continue against existing environment.

Verification throughput follow-up (2026-11-20, PR #21): dev/test opt profiles + three-job
parallel cached CI merged FIRST per admin directive so every later PR benefits. Measured on
the QA-sweep baseline protocol (exclusive flock, -j4): full suite 3986.68s -> 440.25s
(9.06x), real-data suites 906.96/53.82/2873.76s -> 99.45/7.55/281.04s, exact parity 54
binaries/518 passed/0 failed/lib 148 verified twice; hosted CI 27.75min single-job (run
44581603412) -> 15.37min cold parallel (run 44656886446, all three jobs success on 40f5711).
mold evaluated and SKIPped: clean concurrency-guarded link-only A/B shows default rust-lld
23.1.5 matches mold 2.30 (6.00s vs 6.55s for 54 executables); the first-pass 77.54s vs 6.29s
was contamination-inflated (cold lib recompile from a concurrent non-flock lane build) and
is retained as a documented measurement error. CI-first pre-push gate exercised:
fmt/clippy/targeted 293/293 + provenance under flock; hosted CI ran the full clean ladder;
QA review on the exact SHA. Also SKIPped with evidence: nextest (process-local
FLEET_TIMING_LOCK serialization incompatible with process-per-test), sccache (rust-cache
covers dep reuse; revisit with cache-hit telemetry), Cranelift (nightly-only), consolidation
(invasive). Compute identity changes only via bound Cargo.toml bytes (honest build-metadata,
30/30 reversible probes restored). PROGRESS.md rubric percentages unchanged: throughput
infrastructure is not a rubric requirement. Remaining queue unchanged.
