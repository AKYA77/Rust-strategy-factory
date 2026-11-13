# Acceptance evidence

Status: **SOFTWARE_INCOMPLETE**. This is a foundational checkpoint, not final acceptance.

2026-11-12, cloud Linux x86_64, Rust/Cargo 1.100.0:

- `scripts/check.sh`: formatting, Clippy all targets with warnings denied, and 35 integration tests passed (14 core, one CLI workflow, seven order lifecycle, seven persistence/holdout, one index differential and five validation-boundary tests). No tests were ignored or disabled.
- `cargo build --locked --release`: passed.
- Existing GitHub read access: `git ls-remote origin HEAD` passed for AKYA77/Rust-strategy-factory.
- Toolchain smoke: temporary library compiled, one generated unit test passed, formatting and Clippy passed. This is environment evidence, not application acceptance.
- CLI test actually imports decimal CSV, verifies hashes, rejects overwrites, creates causal candles/signals, executes one nonzero synthetic trade and verifies independently calculated $1.7982 net P&L.
- Core tests exercise all profile constructors, JPY/gold arithmetic, infeasible volume, adverse fractional-money rounding, future/stale conversions, tied timestamps, quarantine/tampering, all seven UTC bucket sizes, gap preservation, causal indicator prefixes, masks, canonical equivalence, feature budget, bid/ask spread floor, limit/stop distinction, short execution, losses beyond intended risk, deadline exits, unresolved positions and candle tampering.

No real historical quotes acquired. No approved broker/session profiles. A synthetic-only benchmark is saved in docs/BENCHMARKS; it is not representative real-data acceptance. The static first-hit prototype is disabled in replay. Full indexed accounting, persistent scheduler, complete protected validation/qualification, dashboard, deployment and full historical acceptance are unfinished. Dependency license inventory exists; external source/codec research and security audit remain pending. No claim of billion-per-second throughput.

GitHub publication/CI evidence is recorded in commit history and the final session report; a workflow file alone does not establish a successful hosted CI run.

## Second checkpoint evidence

- Full CLI integration includes two identical experiment invocations: the first executes one synthetic trade, the second reuses its committed result; registered/completed/attempt counts remain one. Online backup is opened and integrity checked.
- SQLite schema v2 migrations, transactionally unique identities, competing claims, lease expiry fencing, idempotent immutable completion, result tamper detection, restart recovery and WAL-aware backup/restore passed.
- Frozen holdout tests prevent overlapping training/screening registration, retrospective reservation after training exposure, unselected children, protocol retuning and repeated altered final experiments. Protection persists across restart, within the same dataset hash. These are safeguards, not a completed independent validation system.
- Numerical tests distinguish exactly 50% versus below 50%, 50 exits per year versus fewer, break-even losses/streaks, year-boundary streaks, unresolved results, calendar-quarter folds, prior-only labels and embargo. Numeric gates cannot issue VALIDATED or qualified status.
- Fixed-barrier index differential test ran 2,000 seeded queries across four block sizes on effective bid/ask quotes with timestamp ties and deadlines. This does not prove full replay/accounting equivalence; optimization remains disabled.
- Benchmark executed 20 eight-instrument synthetic batches (five repetitions for each of 1/2/3/4 workers), 20,000 events per instrument, 6,997 resolved trades per batch. See exact hashes/timings/resource receipts. No real-data/compression/index-speed claim.
- GitHub API/hosted CI status inspection returned a proxy CONNECT 403 for api.github.com. Existing GH_TOKEN binding was detected by name only; no value was read. The domain addition is saved to the environment draft; no replacement token requested. Hosted CI status remains unverified.

All current tests pass. Initial lint and a miscounted test-fixture streak were corrected, then rerun; no assertion was disabled. The first benchmark wrapper incorrectly included earlier build resources through cumulative child usage; it was replaced with per-child wait4 accounting and the benchmark rerun. Only the corrected measurements are retained.

Source discovery is also blocked under the current network policy. HistData and Dukascopy information pages were not retrieved (403); no licensing or availability conclusion is claimed. Provider domains were saved as draft additions for later source/terms review. See DATA_MANIFESTS.md for precise probes and pending actions.

## Order lifecycle checkpoint

- `scripts/check.sh`: 35 integration tests passed, formatting passed, Clippy all targets with warnings denied passed. No ignored/disabled tests.
- Market, stop-market and limit entry order lifecycle implements immutable submitted contracts, subsequent-event eligibility, adverse market/stop slippage, executable-side triggers, non-slipped observed-quote limit eligibility, cancellation, quote-free expiry, conflicting event rejection and serializable restart snapshots. Seven focused tests passed.
- Sequential replay shares the market-fill price calculation. Pending stop/limit entries are available through the library but are not yet emitted by the strategy grammar or integrated with reserved risk/margin/position accounting. No partial-liquidity model is claimed.
- Normalized CSV archive hash now comes from the exact byte stream read by the parser, rather than reopening the input path. The CLI test independently verifies the raw hash.
- Post-refactor benchmark receipts and three alternating pairs against frozen a5f4a13 are retained. Every batch still resolved 6,997 synthetic trades. Timing varies substantially; no consistent speed improvement or universal worker optimum is established. This is not a production performance acceptance result.

## Pending strategy entry integration

47 application tests, formatting and Clippy pass. Eight integration tests exercise strategy stop/limit entry fills on both sides, fixed quantity and receipts, pending/unresolved outcomes, expiry, cancellation causality, pre-fill margin rejection, and backward-compatible concept identities. Four risk tests independently check marked initial margin, largest feasible grid volume, commissions, minimum-volume feasibility and gold/JPY conventions. Market fixture accounting now correctly uses 19,210 micro-lots and $1.7289 target net P&L, replacing the earlier cash-only margin calculation. Pending false-signal cancellation is processed after existing quote eligibility, preventing hindsight cancellation.

The versioned policy is documented in DECISIONS/0001-pending-entry-reservations.md. Synthetic replay benchmark rerun with source/binary/resource hashes and nonzero trades; final receipts use the v2 engine identifier. No broker approval, maintenance margin model, real data, complete scientific qualification or autonomous research readiness is claimed.

## Nine-family signal milestone

53 application tests pass with formatting, Clippy all targets and release compilation. Six family tests cover all nine families, distinct canonical identities, nonzero signals and actual closed tick trades, future-prefix invariance, repeated cache reuse, deterministic cache eviction, brute-force prior extrema equivalence and overnight/weekday UTC publication masks. Family labels are metadata outside effective concept identity. RSI comparisons use nearest basis-point values from Wilder smoothing; channel references exclude the current bar. These are synthetic fixture results, not independent validation or broker session approval.

## Durable loop and conditional screen audit

64 tests, formatting, Clippy and release build pass. Six loop tests cover pause/resume/stop, restart cursors, competing coordinators and stale fencing, recovery after result commit before cursor advance, novel bounded regions, configuration limits, state-pressure pause and restored cursors. An independent Rust-process test exercises live controls and graceful safe-checkpoint exit; its initial startup JSON framing failure was corrected and rerun, with a child cleanup guard.

The documented standalone smoke command completed ten synthetic steps: three newly completed tick simulations, seven zero-signal rejections, zero failures. It released its lease and left no process running. Counters do not claim full-history tests or qualification. Four screen tests include 4,000 seeded conditional bound queries against sequential events, collisions, entry/deadline ambiguity and unknown effective-ask minimum correlation. Every family replay is also checked against conditional M1 receipt bounds. Candidate-level net outcome screening remains incomplete; no unjustified performance rejection is enabled.

## Authenticated dashboard core

65 Rust tests, formatting, Clippy and release compilation pass. Chromium browser acceptance ran against the real Axum HTTP service and a freshly generated persistent research fixture. It passed authentication, owner-only/no-overwrite auth initialization, body limits, HttpOnly/SameSite sessions, CSRF and origin/host rejection, login rate limit, metrics-write denial, durable run pause, instrument filtering, actual trade/equity views, exact receipt download, numeric training diagnostics, desktop/mobile/no page overflow, restart persistence and logout. Screenshots were inspected. Browser tests stop their processes and remove temporary secrets/state. No public preview or production deployment occurred.

An adversarial actual replay with uncapped losses beyond i64 range now round-trips its monetary values through public JSON exactly (serde_json arbitrary_precision enabled). The browser never silently rounds unsafe-range integer display/export values. Both profile and cost specifications accompany current persisted results. Numeric diagnostics retain REJECTED/INSUFFICIENT_EVIDENCE semantics; they cannot issue validation.

The UI implements a functional authenticated core, not all requested operations. Full scientific validation/ranking, imports/approval/refresh, backup/restore UI, worker management, pagination and richer analytics remain pending. Local HTTP cookies are scoped to loopback mode; public HTTPS deployment needs additional implementation. No full security/supply-chain audit claim is made. Hosted CI was configured to run browser acceptance but remote status remains blocked/unverified.

## Chronological windows and compute/provenance identities

71 Rust tests now pass. Four window integration tests verify warmup without pre-window positions, exclusive end boundaries, invariance to appended future quotes, pending-order boundaries and parent-dataset protected holdout execution/cache/retuning rejection. A real-data autonomous scope test prevents implicit whole-history exploration. A provenance identity test distinguishes changed source metadata while reusing a reimport of the same archive. Formatting/Clippy, release build and Chromium acceptance passed on these implementation changes; the final added scope test was run separately.

Compute identity now binds relevant source/compiler/target/profile/flags and manifest/lock inputs rather than the whole executable. An actual reversible CSS-only change rebuilt a different executable with identical compute identity, then restored/rebuilt the original asset. This avoids rerunning numerical experiments for embedded UI changes. Evidence is retained in BENCHMARKS/2026-11-12-compute-identity.json. This does not complete frozen protocol orchestration, corpus-wide exposure protection or scientific qualification.

## Frozen protocol execution and initial-stop correction

79 application tests pass, with formatting/Clippy and release build verified for this milestone. Chromium desktop/mobile acceptance also passes against a real Rust server and SQLite, including actual CLI-created frozen protocol evidence and immutable reuse. Six scientific tests exercise nonzero synthetic OOS trades, prior-only calendar folds, retuning/retrospective exposure rejection, report integrity, cross-instrument equal-content independence, and refusal to spend real protected-final data while independent prerequisites remain incomplete. Schema v4/v5 persists frozen definitions/reports and instrument-scoped reservations; legacy global reservations stay conservative.

Engine v4 corrects an initial protective-stop feasibility defect: a stop already crossed by the executable quote could previously open and later be counted as a target. Such entries now fail before opening. Stops also satisfy the profile minimum distance after entry friction. Valid long/short fixture goldens now use 50-tick stops; long quantity 19,210 micro-lots and target net P&L $2.8815. Earlier 30-tick/$1.7982/$1.7289 fixture examples above are historical and superseded; they must not be used as current correctness evidence. Legacy engine receipts remain historical, and current numeric diagnostics refuse their reuse as fresh verification.

Protocol results always remain INSUFFICIENT_EVIDENCE. Fold accounts reset independently; their descriptive pooled P&L is not compounded full-period equity. Corpus-wide alias/source-revision exposure, independent uncertainty/multiplicity, cost/neighborhood/regime validation, approved coverage and qualification remain unfinished. Real protocol execution is blocked before reservation; synthetic fixtures alone exercise this incomplete workflow. No strategies qualify.

## Conditional uncertainty diagnostics

82 Rust tests pass, with formatting/Clippy, release build and Chromium protocol/dashboard checks. Frozen protocol v2 includes a deterministic one-sided Hoeffding/Bonferroni net-win diagnostic at family alpha 0.05 using the declared trial budget. Three focused tests check an independently calculated bound, monotonicity/multiplicity, empty data, breakevens and invalid input; the nonzero OOS integration verifies persisted counts against actual receipts. Assumptions are explicitly unverified: trade dependence and complete search accounting remain unresolved. Bounds are descriptive conditional diagnostics and never enable qualification. Existing reports remain immutable/readable without retroactive evidence.

## Registry-wide alias protection

84 Rust tests pass with formatting/Clippy, release build and actual Chromium acceptance. Instrument holdout reservations inspect all registered same-symbol histories rather than only one content hash. Registration rejects overlapping protected ranges through other dataset hashes; changed quote histories cannot conceal previous OOS exposure. Two new regressions cover altered content, retrospective independence, restart, separate symbols and exact adjacent exclusive boundaries. Legacy unscoped reservations conservatively cover all symbols. This is registry-local protection; external tools, independent registry copies and concurrent OOS-range exposure are not established as protected. Real final execution remains blocked.

## Atomic OOS reservations

87 Rust tests pass, with formatting/Clippy, release build and Chromium acceptance. Schema6 introduces transactional immutable OOS interval reservations and a same-instrument experiment index. Three storage tests cover cross-connection exclusion, restart and immutable range-set recovery, complete rollback when a later interval is exposed, and a real two-thread race: outside registration and reservation cannot both succeed. Frozen execution reserves OOS intervals before evaluations; its own expanding training may reuse earlier OOS time under the same frozen protocol. Other protocols/archives cannot expose these ranges. Final reservations cannot overlap OOS reservations. No unlock API exists. This closes local registration races; external research and independent registries remain untracked, so real qualification stays blocked.

## Frozen OOS cost and stop probes

89 Rust tests pass, with formatting/Clippy, release build and Chromium acceptance. Protocol v3 freezes actual cost stress and bounded stop-neighborhood cases, including them in declared trial-budget requirements. Each probe executes sequential OOS fold replay with its own effective strategy/cost identities; integration checks verify nonzero synthetic trades, exact OOS windows/stages, no protected-final input and unchanged attempt count after reuse. Two focused tests verify deterministic exact changes, boundaries and infeasible stress rejection. Historical reports without optional later evidence fields remain hash-readable, with an explicit regression. Probes are DESCRIPTIVE_ONLY; regimes, broader parameters, dependence-aware inference and prespecified acceptance gates are unfinished. No qualification claim.

## OOS annual/session and realized-cash diagnostics

91 Rust tests pass with formatting/Clippy, release build and Chromium acceptance. Protocol v4 binds the breakdown method version; actual OOS receipts populate exit-year and fixed six-hour UTC entry bins plus separate realized-cash fold drawdowns. The exact net expectancy numerator and observation count are retained. Two tests independently check a cross-year/entry-session boundary, $37 realized drawdown from a $25 peak to -$12 cash, uncapped negative cash, reset-fold initialization, breakevens, chronological refusal and atomic overflow failure. Integration totals reconcile with persisted pooled OOS statistics. Missing replay/unresolved positions are explicit. These are descriptive UTC bins and realized cash drawdown, not approved broker/DST sessions or intratrade marked-account drawdown. Qualification remains blocked.

## Fixed causal regime bins

93 Rust tests pass with formatting/Clippy, release build and Chromium acceptance. Frozen protocol v5 includes a fixed prior-21-closed-M1 trend/range recipe. One bounded streaming publication-summary classifier is shared across candidates. Strict entry-event exclusion and unknown warmup/gap/stale labels prevent hindsight or gap repair. Two tests verify prefix/entry-quote invariance, ties, stale context, gap reset/recovery and expanded/downward labels. Actual OOS regime counts reconcile with pooled receipts. These bins remain descriptive, with comprehensive regimes, sufficient independent samples, prespecified acceptance and approved calendars/coverage unfinished. No qualification claim.

## Initial external source review

Selected manifests/licenses/implementation files from Tickforge, Wickra and LZ4 Flex were actually retrieved at exact Git revisions. File hashes and sizes are retained in SOURCE_REVIEW/2026-11-12-initial.json; SOURCE_REUSE_MATRIX.md records concrete semantic/dependency observations and deferred decisions. No source implementation copied, runtime dependency added, upstream benchmark adopted or real-data performance claim made. The remaining requested frameworks/codecs are explicitly unreviewed. The unchanged application still passes all 93 Rust tests and required formatting/Clippy; its last release/browser acceptance passed at the causal-regime milestone.

## Native lossless storage foundation

97 Rust tests pass with formatting/Clippy, release build and actual Chromium acceptance. Four native tests cover all eight profiles/provenance, tied original order, complete u64 sequence range, chunk boundaries, no-overwrite publication/cleanup, malformed lengths, checksum/header/body corruption, truncation/trailing bytes and structural validation independent of checksums. The CLI test packs/unpacks exact JSON bytes, executes the independently calculated nonzero $2.8815 fixture trade via native input and reuses a JSON-created experiment without another attempt. Chromium research now loads native input before its normal real-service persistence/UI checks.

This uncompressed SoA wrapper is a bounded foundation, not selected production storage or Parquet/ZSTD. It reconstructs the existing event representation; no compression, zero-copy/mmap, full-history throughput or real-data performance claim. Source/category metadata never promotes on encoding. Original archives, licensing and approval remain separate requirements. Production crash/disk-full acceptance remains pending.

## Bounded frozen rule-parameter probes

99 Rust tests pass with formatting/Clippy, release build and actual Chromium acceptance. Protocol v6 freezes up to six canonical single-scalar rule perturbations in addition to cost/stop cases. Two focused tests verify exact changes, deterministic equivalent ordering, uniqueness, bounds and fixed-session exclusion. Actual OOS tests verify nonzero rule-probe trades, effective identities, exact OOS windows/stages and cached restart reuse. Budgets cover every case. This deterministic bounded prefix is not exhaustive stability or adaptive improvement lineage; all outcomes remain descriptive and real final execution remains blocked.
