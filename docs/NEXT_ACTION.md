# Next action

Status: SOFTWARE_INCOMPLETE. Use the existing checkout at `/root/workspace/Rust-strategy-factory`.
Read AGENTS.md, PROJECT_STATE.json (superseded eight-instrument fields), IMPLEMENTATION_QUEUE.md,
MASTER_BLUEPRINT.md and the V2 handover mandate. After every verified milestone update
evidence/state/this file, commit, push, inspect remote/hosted CI, reread this file from disk and
continue. Never treat a pending CI run as passed.

## Cached-receipt convention reconciliation (2026-11-26 continuity run)

Baseline clean `main` `d3f354eb842a822e94de652caa54305f16a2e6db` == `origin/main` (0/0),
no other worker, heavy lock acquired. Four already-pushed commits beyond the ledgers' prior
HEAD `3b789b2` were verified: `34aae4b` (ledger CI record for run 45734055499), `be6553e`
(PROJECT_STATE scope correction + 16 cached-report `limitations` strings), `8a8e41c`
(test rename `..._direction_based` -> `..._bid_based` in `tests/core.rs`), and `d3f354e`
(6 non-USD cached reports' `conversion_policy` label `..._long_bid_short_ask` ->
`..._bid_only`). Changed-line audit over `3b789b2..HEAD`: 138 policy labels, 16 limitations
strings, 1 test rename, 4 ledger CI lines, 2 blank lines; no numeric field, `receipt_hash`,
count, P&L, classification, eligibility, gate or financial/scientific assertion changed.

Relabelling is truthful: the cached execution uses `"side": "Long"`
(`m1-long-above-mean-v1.json`), so Long fills take `bid` under both the superseded
direction-based rule and the active bid-only rule; no Short fill existed, hence numbers and
receipt hashes are unchanged. `SCIENTIFIC_VALIDATION_RECEIPT.json` keeps the old string as the
historical pre-relabel validation receipt.

- Baseline `scripts/check.sh` (`RSF_BUILDER_PROFILE=hermes-v2-m1`): **EXIT=0**, **760 passed / 0
  failed across 71 binaries** (lib 276), zero named failures, fmt and clippy clean.
- RED/GREEN: new `tests/cached_receipt_convention.rs` (3 tests) pins labels to the active code
  constants and the long-only strategy. With the `3b789b2` versions of the six non-USD cached
  reports, `cached_diagnostic_receipt_labels_match_active_convention` FAILED (`EXIT=101`,
  `long_bid_short_ask` vs `bid_only`); after byte-for-byte HEAD restore it passed 3/3 (`EXIT=0`).
- Final-tree `scripts/check.sh`: **EXIT=0**, **763 passed / 0 failed across 72 binaries** (lib
  276), including the new 3-test binary; fmt and clippy clean.
- Hosted CI terminal conclusions: **45736386703** (`34aae4b`), **45740888354** (`be6553e`),
  **45741014575** (`8a8e41c`), **45741284289** (`d3f354e`) -- each `completed`/`success` on all
  7 jobs. No in-progress run called a pass.
- Evidence: `docs/VERIFICATION/cached-receipt-convention-reconciliation-2026-11-26.md`. No
  financial, scientific, provenance, data-scope or security rule changed; class stays
  REAL_STRUCTURALLY_VALID, eligibility none. V2 milestones 1–6 remain evidenced; M7 is still
  NOT accepted (two owner-only external gates). No further software milestone is unachieved.

## Bid-only conversion convention reconciliation (2026-11-26 continuity run)

Baseline clean `main` `38fcc736863e4bb59b888134d958b963d27b3138` == `origin/main` (0/0),
no other worker, heavy lock acquired. Two already-pushed commits were unreconciled in the
ledgers and are now verified:

- `617f21575129130127c37c4b7d917ad1fd41326a` (code fix): `tick_engine::diagnostic_conversion`
  is bid-only for both sides (Side ignored), matching `src/main.rs::research_inputs()` which
  derives conversions at load time before a side is known; policy string renamed to
  `diagnostic_reciprocal_closing_rate_v1_bid_only`. This corrects the Short-side divergence
  between the research loader (bid) and diagnostic replay (previously ask). Exact integer
  arithmetic, strict `as_of_ms` alignment and all `Conversion::validate` safeguards preserved;
  reciprocal convention remains an unverified research assumption, not a broker rule.
- `38fcc736863e4bb59b888134d958b963d27b3138` (docs only): measured conversion memory per
  partition (2 GiB input-file budget vs 8 GB post-load RAM budget; ~6.58 GB of conversion
  vectors for the full corpus), 20-month dev vs 22-month corpus coverage gap (74 missing
  inputs), limitations #7/#8, 14 validation-safeguard tests.

- `scripts/check.sh` (`RSF_BUILDER_PROFILE=hermes-v2-m1`): **EXIT=0**, **760 passed / 0 failed
  across 71 test binaries** (lib 276), zero named failures, fmt and clippy clean. This
  corrects the stale `762/72` figure set by `617f215`; a fresh full run yields 760/71.
- Red/green: restoring the pre-`617f215` direction-based `diagnostic_conversion` body made
  `tick_engine::worker_preparation_tests::diagnostic_conversion_is_bid_based_reciprocal` FAIL
  (`EXIT=101`, `(10,1501)` vs `(1,150)`) and `tests/verify_conversion.rs::
  all_three_non_usd_instruments_derive_conversions_from_dataset` FAIL (`EXIT=101`,
  "USDJPY loader vs tick engine Short"); after byte-for-byte restore both pass (`EXIT=0`,
  1/1 and 14/14). Tree clean.
- Hosted CI: run **45701751661** on `617f215` and **45716427171** on `38fcc73` both reached
  terminal `completed`/`success` on all 7 jobs. Docs-only `80dbddd` run 45697619785 had a
  terminal `failure`, retained as history and superseded. No in-progress run called a pass.
- Evidence: `docs/VERIFICATION/bid-only-convention-reconciliation-2026-11-26.md`. No
  financial, scientific, provenance, data-scope or security rule changed; class remains
  REAL_STRUCTURALLY_VALID, eligibility none. V2 software milestones 1–6 remain evidenced;
  M7 is still NOT accepted (two owner-only external gates: saved dashboard credentials and
  a fresh owner manual reboot). No further software milestone is unachieved.

  Post-push: the reconciliation commit `3b789b28ed6b946e21ffdddb60a8f278da7438e7` produced
  hosted CI run **45734055499**, which reached terminal `completed`/`success` on all 7 jobs
  (release-build, browser, rust-checks, data-compute-identity, full-horizon, check-operations,
  dependency-audit); remote SHA `3b789b2` verified equal to local HEAD.

## Conversion implementation reconciliation at HEAD (2026-11-25 continuity run)

Baseline clean `main` `c3ee6c0` (== `origin/main`, 0/0 ahead-behind, no other worker, heavy lock
acquired). Commit `c3ee6c0` ("Reconcile conversion implementation for USDJPY/USDCHF/USDCAD") was
already on `main` but not yet reflected in the ledgers; this run verified it and reconciled the
evidence.

- `scripts/check.sh` (`RSF_BUILDER_PROFILE=hermes-v2-m1`): **EXIT=0**, **759 passed / 0 failed across
  71 binaries** (lib 276), zero named failures; fmt and clippy clean.
- Focused: `verify_conversion` 13/13, `real_conversion_evidence` 2/2.
- Red/green on the behavior change: restoring parent `15a635e`'s assertion
  `replay_corpus(...).is_err()` in `tests/corpus.rs` made
  `corpus_account_replay_matches_all_materialized_receipt_fields` **FAIL** (`EXIT=101`); the HEAD
  assertion `.is_ok()` passes (`EXIT=0`). File restored byte-for-byte; tree clean.
- Changed paths: `src/main.rs`, `src/tick_engine.rs`, `tests/corpus.rs`, `tests/verify_conversion.rs`
  (+2 tests). Non-USD quotes now derive exact event-aligned reciprocal conversions (bid Long /
  ask Short) instead of being refused; identity sentinel still refused for non-USD; class remains
  REAL_STRUCTURALLY_VALID, eligibility none; no scientific/financial/security rule changed.
  NOTE (superseded): at `c3ee6c0` the diagnostic replay used bid Long / ask Short; `617f215`
  (2026-11-26, above) unified both sides to bid-only so the research loader and replay agree.
- Evidence: `docs/VERIFICATION/conversion-implementation-reconciliation-2026-11-25.md`. Hosted CI
  run **45676627807** on exact SHA `c3ee6c0` reached terminal `completed`/`success` on all 7 jobs.
  The docs-reconciliation commits `b2d0eb1` (evidence file) and `6221875` (ledgers) both reached
  terminal `completed`/`success` on all 7 jobs (runs **45694761489**, **45694794740**). V2 milestones 1–6
  remain evidenced; M7 remains NOT accepted (two owner-only external gates).

## Verified gate repair (2026-11-25, continuity run)

Baseline `main` `f8e4eafbd56a344ce17a29eebb4c1a215137ea85` was clean and equal to
`origin/main`, but the mandatory `scripts/check.sh` failed at its first layer:
`cargo fmt --all -- --check` EXIT=1 on `src/research.rs:1474` (introduced by `2177da0`).
Hosted `rust-checks` had no format step, so CI run 45634655873 on the same SHA was green
despite the broken local gate. Fixed by applying the exact rustfmt rewrite (whitespace only)
and adding `cargo fmt --all -- --check` to `.github/workflows/ci.yml` `rust-checks`.

Verified after: `cargo fmt --all -- --check` EXIT=0, clippy `-D warnings` EXIT=0, full
`scripts/check.sh` EXIT=0 with **744 passed / 0 failed across 69 binaries** (lib 276), zero
failing test names. Evidence: `docs/VERIFICATION/fmt-gate-regression-2026-11-25.md`.
Verified increment commit: `e92934ce57a4e84654f5f0a2df4275557c60aa30`. Hosted CI run
**45640484836** on that exact SHA reached terminal `completed` / `success` on all 7 jobs
(release-build, browser, rust-checks, data-compute-identity, full-horizon,
check-operations, dependency-audit); `rust-checks` includes the new format gate. No
`in_progress` run was called a pass. No financial/scientific/data
rule changed. V2 milestones 1–6 remain evidenced; Milestone 7 remains NOT accepted.

## Real-corpus non-USD conversion evidence (2026-11-25 continuity run)

Baseline clean `main` `18bba89` (== `origin/main`): `scripts/check.sh` EXIT=0,
**755 passed / 0 failed over 70 binaries** (lib 276), no named failures. Increment:
`tests/real_conversion_evidence.rs` (2 tests) verifies USDJPY/USDCHF/USDCAD against actual
retained `.rsf` ticks and the real manifest profile — exact integer cross-product, strict
`as_of_ms == tick.timestamp_ms`, all `Conversion::validate` safeguards, no identity rate for
non-USD quotes, and provenance hashes (source/dataset/quote-stream/output/chunk + deterministic
consumed-byte blake3). Red/green: with the *wrong* synthetic-vs-corpus expectation the negative
control FAILED (`250/37661` vs `1/150644`, a silent 1000x divergence); with the true `assert_ne!`
invariant both tests passed (EXIT=0, 2/2). Evidence:
`docs/VERIFICATION/real-corpus-conversion-2026-11-25.md`; `docs/CONVERSION_CONVENTION.md` now
states that synthetic fixtures are not the real corpus scale. Increment commit
`83213c15df87356ffd897dabce04b9438458ecb8`; hosted CI run **45668913065** on that exact SHA
reached terminal `completed` / `success` on all 7 jobs (browser, release-build, full-horizon,
dependency-audit, data-compute-identity, rust-checks, check-operations). No financial, scientific, provenance or security rule changed; class remains
REAL_STRUCTURALLY_VALID with eligibility none. M7 external gates unchanged.

Post-increment full ladder on the corrected tree: `scripts/check.sh` **EXIT=0**, **757 passed /
0 failed over 71 binaries** (lib 276), including the new target 2/2; fmt and clippy clean.

## Latest verified local milestone (2026-11-25)

Cursor 2026-11-25: `main` HEAD `767f777` (Stage-1 CandleScreen CLI regression fix,
integrating nine previously unpushed bot commits; parent `4b52ea4`). Before:
`scripts/check.sh` EXIT=101 at `4b52ea4`, failing
`tests/research_cli.rs::independent_rust_process_runs_pauses_resumes_and_stops_at_durable_checkpoints`
(`QueryReturnedNoRows` on a stale `state='completed'` query). After: full ladder
EXIT=0, **744 passed / 0 failed across 67 test binaries** (lib 276). Hosted CI
run **45591831853** on the exact pushed SHA `80b7665` reached a terminal
**success on all 7 jobs**. Evidence:
`docs/VERIFICATION/stage1-cli-regression-2026-11-25.md`. The inherited bot line
adds `src/daily_checkpoint.rs`, family renames, CandleScreen entry gating, prep-cache
integration in `Driver::step`, and `improvement_children=1`/`max_depth=1` in both
real-data configs; these are integrated and test-verified, not scientifically
qualified. M7 remains NOT accepted.

HEAD `b17ce34` on `main` == `origin/main` (pushed). Milestone chain:
`b8571d8` (V2 seven-instrument scope, P0 fixes, verified gold-only data deletion),
`a5030f8`/`a9601e8` (P0.2–P0.4), `e1b1356` (state), `18d6e9b` (M2 ten families),
`57c13fc` (M5 contract suite + evidence), `98cdc79` (M3 stage machine + telemetry),
`2ced8fe` (M4 prep caches + M6 measured baseline + M7 evidence + browser repair),
`b17ce34` (M7 evidence re-pin to `2ced8fe` + state-ledger reconciliation).

- Full `scripts/check.sh` ladder on `RSF_BUILDER_PROFILE=hermes-v2-m1` at `2ced8fe`:
  **EXIT=0**, 733 passed / 0 failed across 69 test binaries (8m40s), including the M5
  contract suite, `tests/stage_machine.rs` (4/4) and `tests/prep_cache.rs` (10/10).
  `cargo clippy --locked --all-targets -- -D warnings`: EXIT=0.
- Hosted CI run **45528848345** on the exact push SHA `2ced8fe`: **all 7 jobs success**
  (release-build, browser, rust-checks, data-compute-identity, full-horizon,
  check-operations, dependency-audit). Earlier `98cdc79`/`57c13fc`/`18d6e9b` push runs
  failed only the browser job (runs 45521997695/45519520343/45504435877); `2ced8fe`
  repairs `scripts/browser-check.cjs` and `scripts/browser-long-ledger-check.cjs`.
- Hosted CI run **45535143330** on the exact push SHA `b17ce34` (this docs/evidence
  reconciliation commit): **all 7 jobs success** (same seven job names). Terminal
  conclusion recorded only after the run completed.
- Milestone 3: append-only immutable stage lineage with holdout-gated protected
  stage, bounded telemetry replay, exactly-40% inclusive Stage-1 policy with
  proven-bound rejection. CLI `machine advance <registry> <experiment-key.json>`.
- Milestone 7 evidence was re-pinned from the superseded `98cdc79` to the exact HEAD
  `2ced8fe` (see `docs/VERIFICATION/milestone7b-*`): release build EXIT=0
  (binary sha256 `2a731ea1…`) and full-history ordered coverage EXIT=0 over 154
  partitions / 2,819 chunks / 274,098,687 events in 64 s. **Milestone 7 is still NOT
  accepted** — see the corrected status below.
- `npm run test:browser`: all 13 scripts pass locally. Earlier browser failures traced to the V2
  removal of XAUUSD moving the protocol fixture onto GBPUSD, which collided with the GBPUSD crash
  fixture's protected fold windows; the synthetic crash fixture now sits in Feb 2026 (outside every
  protected GBPUSD interval while still reaching the 2026 YTD assessment cutoff).
- Scope: `Profile::SYMBOLS` = EURUSD GBPUSD AUDUSD NZDUSD USDJPY USDCAD USDCHF. XAUUSD submissions
  are rejected (`gold_submission_rejected`); gold calendar checks removed; the 2026-11-21 market
  calendar approval is WITHDRAWN (profile session weeks/hours only, addendum checks retained).
- ORB causal replay fixed (`tests/orb_causality.rs` positive causality/order regressions);
  `tests/families.rs` uses a dedicated completed-range ORB fixture and may no longer skip ORB.
- Removed the false `candidate_index % 3` stage rotation. Every training candidate enters
  explicitly at TickVerification. Durable crash-safe stage transitions are NOT implemented yet.
- Conversion identity sentinel now quote-conditioned (USD-quote only); non-USD quotes require an
  event-aligned observation. Registered corpus mode now accepts non-USD profiles without a
  conversion file (P0.3 complete — conversion is required only when quotes are non-USD).
- Data-validation addendum implemented in `src/data_validation.rs` (timestamp ties vs duplicate
  `(timestamp_ms, sequence)` identities, time-based gap evidence, bounded streaming examples with
  exact counters) and `src/economic_calendar.rs` (instrument-identity filtering, causal
  availability, interval validation).

## P0 completions (verified)

**P0.2 — Ordered full-history coverage + CandleScreen auth:**
- `src/ordered_coverage.rs` (new): `research-ordered-coverage` command expands each configured
  first-chunk pointer to the full instrument-month via the local migration manifest, verifies every
  receipt chunk in order including cross-chunk sequence continuity, rejects XAUUSD and duplicate
  months, writes an ordered-coverage receipt. Fail-closed: never downloads, never mutates the
  corpus, never asserts provider completeness.
- `src/lib.rs`, `src/main.rs`: register ordered_coverage module and `research-ordered-coverage`
  CLI command.
- `tests/ordered_coverage.rs` (new): MonthSpec scope/date validation, duplicate and gold rejection,
  stage/month-limit bounds.
- `src/improvement.rs`, `tests/improvement.rs`: `authorize` accepts `Stage::CandleScreen` under the
  same protocol gate as `TickVerification`; `authorize_screening` delegates to `authorize`;
  proof-gating fully intact.
- Receipt: `docs/VERIFICATION/ordered-coverage-2026-11-24.json` — 140 instrument-months,
  2,551 chunks, 247,989,793 events, Jan 2025–Aug 2026, all sequence-continuous.

**P0.3 — Continuous cross-month accounting:** `src/historical_corpus.rs` — continuous account
state across months (feature history + open positions, exact intraperiod drawdown, correct streak
logic); registered corpus mode accepts non-USD profiles without a conversion file (conversion
required only when quotes are non-USD). Regression test: USDJPY across two months without a
conversion file.

**P0.4 — Real-data scientific qualification path:** `src/science.rs` `execute()` admits
`DataClass::RealStructurallyValid` alongside `SyntheticTestOnly`; every other class
(`RealUnreviewed`, `RealQualityAccepted`, `RealProfileApproved`, `ResearchEligible`, `Quarantined`)
is still refused. Regression test: REAL_STRUCTURALLY_VALID input accepted into the qualification
pipeline; synthetic refusal preserved where required.

## Implement next (ordered)

1. **Milestone 7 remaining items are external-evidence gates only** (see
   `docs/VERIFICATION/milestone7-vps-acceptance.md` §7): the exact-HEAD release is deployed
   and the prep-cache crash is fixed (items 1–2 RESOLVED, service `active (running)` with zero
   crash-restarts). Still open: (3) authenticated dashboard endpoints were not exercised —
   `browser_vault` has no saved logins and new credentials may not be created by an agent;
   (4) a fresh reboot persistence proof requires an explicit owner manual reboot (agent
   blocklist). Neither can be closed by repository code.
2. After any owner authorization for those gates, re-run the M7 evidence ladder and update
   `docs/VERIFICATION/milestone7*` and this file. No software milestone remains unachieved.

## Completed: Milestones 4 and 6 (2026-11-25)

**M4 shared preparation caches:** `src/prep_cache.rs` + `tests/prep_cache.rs` (10/10) with
exact scope identity over instrument/dataset/strategy/costs/engine/config/window/stage,
content-addressed durable artifacts, byte caps, immutable index triggers, hash verification,
deduplication, cold/warm parity, protected isolation and partial-write recovery. Evidence:
`docs/VERIFICATION/milestone4-prep-cache.md`. No scientific gate modified.

**M6 optimization and cleanup:** measured baseline with exact two-run parity
(`milestone6-research-baseline-summary.json`, identical canonical sha256 across passes A/B),
cold/warm isolated build timings, clippy `-D warnings` clean, dependency/license/advisory
review, and honest hosted-CI status. Evidence: `docs/VERIFICATION/milestone6-*`.

**M7 VPS acceptance — NOT accepted.** The first evidence pass ran at `98cdc79`, a revision
whose hosted CI browser job failed (run 45521997695). `docs/VERIFICATION/milestone7-vps-acceptance.md`
itself records four open items: deployed dashboard runs an older release; the research unit is
crash-looping on 20 stale installed XAUUSD config entries; authenticated dashboard endpoints were
not exercised (no saved credentials); reboot persistence needs an owner manual reboot.

**M7 re-pin to HEAD `2ced8fe` (this run):** exact-HEAD release build EXIT=0 (binary sha256
`2a731ea1d8c88bded1ecf7b18e649e9644de53d1270f2c6155d039af13c21caf`) and full-history ordered
coverage EXIT=0 over 154 partitions / 2,819 chunks / 274,098,687 events in 64 s, independently
re-checked (154 rows = 22 months × 7 instruments, 0 missing chunk files, per-month sequence/event
consistency). Evidence: `docs/VERIFICATION/milestone7b-build.log`,
`milestone7b-ordered-coverage-run.log`, `milestone7b-ordered-coverage-full.json`
(sha256 `0aecece4…`). This supersedes the `98cdc79` coverage receipt only; it does not close the
four open items above.

**Browser suite repaired for the new grammar:** `scripts/browser-check.cjs` now asserts the
proof contract (`ProvenNoMarketEntry` + `fixed-market-stop-infeasible-v1`) instead of the
removed-enumerator signal count, and `scripts/browser-long-ledger-check.cjs` verifies
`corpus-replay` determinism plus stored-receipt structure. Full suite: 13/13 scripts pass.

## Completed: Milestone 5 (2026-11-25)

`tests/milestone5_full_validation.rs` (555 lines) pins the acceptance contract over the
existing frozen-protocol, continuous-accounting, exposure-protection and scientific-gate
machinery: RealStructurallyValid admitted / below-bar refused / synthetic never granted;
prior-only folds with 24h embargo and boundary-leakage rejection; continuous exit-year
reconciliation; all ten families evaluated; exact numerical gates and synthetic gate
rejection. Evidence: `docs/VERIFICATION/milestone5-full-validation.md`. Covered by the
full check.sh ladder (EXIT=0).

## Completed: Milestone 2 (2026-11-24)

**Milestone 2 — New active grammar:** Exit requirements met:
- Exactly ten family IDs (`Family::ALL` with MeanReversion, Momentum, Breakout, TrendFollowing,
  Reversal, RangeTrading, Volatility, Session, Confluence, Orb) ✓
- Replaced the broad Cartesian-product enumerator (`family_templates()`, 840 candidates) with
  exactly ten fixed-daily-checkpoint families (`family_checkpoints()`, one per family) ✓
- Shared scheduling contract: `candidate_at()` deterministic with seed, all ten families appear
  in region 0 ✓
- 70 instrument-family groups (7 instruments × 10 families) ✓
- Max-35 leaderboard (5 per instrument × 7 instruments) — `BOARD_CAPACITY=5` already correct ✓
- Per-family contract tests: 10 tests in `tests/families.rs` covering exact count, reproducibility,
  scheduling determinism, canonical rules, prefix causality, nonzero trades ✓
- Old broad enumerator archived (removed `family_templates()`, all callers updated) ✓

Files changed: `src/strategies.rs` (`family_checkpoints()` replaces `family_templates()`),
`src/research.rs` (`candidate_at()` and `step()` use `family_checkpoints()`),
`src/exploration.rs` (uses `family_checkpoints()`), `tests/families.rs` (rewritten for 10 checkpoints
+ 4 new contract tests), `tests/operators.rs`, `tests/atr.rs`, `tests/research.rs`,
`tests/higher_timeframe.rs`, `tests/corpus.rs` (candidate lookup made data-driven).

## Owner decisions (V2, 2026-11-24)

* Active instruments: exactly the seven FX symbols above; XAUUSD forbidden and its project-owned
  data deleted.
* Market calendar policy approved 2026-11-21: **WITHDRAWN**. Session logic uses profile
  week/hour masks; no calendar completeness or closed-session claims.
* Broker dependency: REMOVED (deterministic broker-independent cost model: 2-pip spread floor,
  separate 2-pip adverse slippage, zero commission).
* External data acquisition: DISABLED. The local 22-month corpus (2025-01..2026-10, 154 FX
  partitions/root) is final; no downloads.
* Scientific protocol: >=50.00% net win rate, >=50 resolved exits per calendar year, max loss
  streak 5, no unresolved positions, full-window evidence; synthetic never qualifies. 70%+ is an
  aspiration, not a gate. The 12+6+4 split remains conditional on an exposure audit.
* Target VPS acceptance: continue against the existing 4-vCPU/8-GB environment. Reboot
  persistence requires an owner manual reboot (agent blocklist).

## Invariants

Exact executable bid/ask, 2-pip spread floor plus separate 2-pip adverse slippage, zero commission,
$20 initial / 10% intended risk / planned 3:1 RR / 24h max holding; frozen pending
distance/quantity; no fabricated gaps/fills/depth; no protected OOS or final adaptation; synthetic
never qualifies; one scheduled evaluation per eligible session-day (zero or one entry, deterministic
skip, no forced retry). Qualification requires independent approved complete evidence, not a high
test count or a benchmark. Structural validity is not licensing, calendar, profile or scientific
approval.
