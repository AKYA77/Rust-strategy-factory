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

## Closed higher-timeframe rules and complex graph verification

105 Rust tests pass with formatting/Clippy, release build and actual Chromium acceptance. HigherTimeframe conditions use strict enclosing hierarchy, actually published higher candles, causal primary-event joins and next-event trade execution. The authoritative engine recomputes the higher-aware mask. Five focused tests verify exact joins/nonzero trades, tampering rejection, prefix invariance, context/budget/hierarchy refusal, nested warmup, autonomous regions and valid complex graphs. An additional frozen-protocol test produces nonzero higher-aware OOS trades, and browser setup submits a higher-aware plan. Signal version v2-htf is persisted in identities.

Every fifth expanded region adds a direction-matched next-frame mean condition where a higher frame exists. The independent mask-verifier cache allowance was raised from 34 to 128 cell units per primary candle to accommodate valid bounded graphs; a 30-node/44-mean-feature regression verifies actual nonzero replay without weakening mask equality or accounting. Compiler budgets remain enforced. This is not hard RSS enforcement, complete shared-cache optimization, approved sessions/calendars, or real-data qualification.

## MACD and population Bollinger crosses

109 Rust tests pass with formatting/Clippy, release build and actual Chromium acceptance. Four focused tests check independent rational MACD and rounded population-band fixtures, prefix and very-large-integer translation invariance, invalid/overflow failure, autonomous variants and actual tick accounting. Each cross independently produces entry100450/exit100750, quantity16660 and net $4.998 on the declared synthetic L1/cost/profile fixture. MACD histogram signs and bands use documented integer-unit quantization; Bollinger variance uses checked exact centered sums before sqrt. Indicator version v3 and neighborhood version v2 are bound in evidence identities; canonical probes include signal_window. Browser protocol setup now includes MACD plus higher confirmation. No external implementation copied, dependency added, real performance or qualification claimed.

## Causal indicator evaluation boundaries

111 Rust tests pass with formatting/Clippy, release build and Chromium acceptance. Engine v5/signal v3 compile only the relevant exclusive-end primary prefix and recursively truncate higher contexts to actual enclosing publication. A valid 8e18 future quote series causes full-history Bollinger arithmetic overflow but cannot alter the earlier completed $4.998 trade, signal count or higher-context mask. A second test rejects malformed serialized backing/padding without index panics. Replay still validates parent integrity, exact full mask length and every usable bit. Benchmark output now reports the compiled engine constant; archived performance receipts remain historical, without a new measurement claim. ATR policy is explicitly planned in decision0008 and remains unimplemented.

## Frozen outward ATR initial stops

119 Rust tests pass with formatting/Clippy, release build and actual Chromium acceptance. Six ATR tests verify independent quantized Wilder/grid fixtures, exact nonzero market accounting and fixed-stop equivalence, pending distance/quantity freeze despite later ATR10->525, warmup/budget/domain refusal, legacy/strict JSON, autonomous regions and policy probes. Market and pending fixtures each independently net $2.8815 with quantity19210; pending receipts retain distance50 despite later prepared distance2625. Valid extreme future prices cannot change the earlier ATR window. Additional protocol and risk tests execute nonzero OOS ATR trades, include every policy probe in trial budgets and reject off-grid risk requests.

Engine v6/indicator v4 and protocol v7 bind the new policy. Browser setup submits ATR with MACD/higher confirmation. Strategy/rule/policy parsing rejects unknown fields. No fixed-stop assertion was removed, no distance clamped or pending quantity resized, and no real strategy qualifies. Decision0008 now describes implemented/tested behavior rather than the preceding plan. Current performance benchmarking, independent inference, approved coverage and production acceptance remain unfinished.

## Packed causal mask prefix and current synthetic baseline

121 Rust tests pass with formatting/Clippy, release build and actual Chromium acceptance. Two prefix tests cover 2400 seeded independent Boolean-oracle queries and exact full-word/partial/empty/malformed boundaries. The engine now checks packed words without weakening representation or relevant-bit checks. Both benchmark children exited zero; source/per-file and compiled compute identities reconcile. See BENCHMARKS/README.md and v6 component/sequential/raw resource receipts. Every sequential synthetic batch resolved 6997 trades. Component median 2.688380 ms scalar versus 0.002947 ms packed measures masks only; no end-to-end A/B, real/full-history speed or universal worker setting is claimed. Default production scheduling remains one worker.

## Browser diagnostic integer precision

The unchanged 121 Rust tests, formatting/Clippy and both builds pass. Chromium now imports/executes an actual synthetic extreme-gap short loss beyond signed64/safe-JS range through the CLI, reads its persisted numeric assessment, guards trade/chart/generic JSON display and downloads exactly the original HTTP receipt. The regression failed on the old generic JSON renderer, then passed after the fix. Currency formatting now uses BigInt quotient/remainder, including exact positive/negative Number.MAX_SAFE_INTEGER boundary checks. Initial precision acceptance covered table/chart/raw export; generic diagnostic JSON now has explicit coverage too.

The actual debug and release executable hashes changed while every compute/version field stayed identical. See VERIFICATION/ui-integer-display-identity.json for hashes and red/green evidence. UI changes did not alter financial math or invalidate compute identity; no new simulation throughput, real performance or qualification claim follows.

## Immutable training diagnostics and proposals

126 Rust tests, formatting/Clippy, debug/release builds and actual Chromium desktop/mobile acceptance pass. Four new integration tests exercise actual replay receipts: an independently checked -$5.9582 short loss, proven no signals, unresolved positions, computation failure, explicit training-window authorization and rejection of scientific/OOS and reserved independent-final evidence. Concurrent connections publish exactly one report; retries/reopen and verified online backup preserve identities. Valid JSON evidence tampering fails integrity. The CLI workflow diagnoses a real persisted trade and reads its exact report without increasing experiment attempts. A further probe test preserves costs at their allowed ceiling without requiring an impossible cost stress.

Schema7 records bounded immutable reports and UNEVALUATED strategy-only child proposals. Parent experiment/result hash, build, seed, canonical concept and operator/edge identities are retained. Failure and unresolved outcomes yield no proposals. This does not execute children, demonstrate an improvement, complete adaptive scheduling or qualify any strategy. Recorded benchmark receipts remain historical source snapshots after these compute-source changes. See RUNBOOKS/training-improvement.md.

## Comparable bounded training reviews

128 Rust tests, formatting/Clippy, debug/release builds and Chromium desktop/mobile acceptance pass. Two additional integration tests execute the real parent baseline and proposed children under one build, check exact -$5.9582 baseline accounting and net differences, retain proposal/experiment/result edges, and recover after baseline/one-child commits without duplicate attempts. Changed features, quotes, provenance and conversions are refused before registration; scoped windows stay exact. Verified backup/reopen preserves review evidence and valid JSON tampering fails registry integrity. The CLI integration executes/reads/reuses reviews from native input with the original exact feature budget.

Schema8 reviews use normal registered-range checks and fenced evaluation, capped child/event/feature/state budgets and immutable publication. Failed/unresolved evidence has no comparable net; no-signals has zero net. Results remain DESCRIPTIVE_TRAINING_ONLY, with no qualification/improvement acceptance. Automatic queue integration, richer diagnoses, independent scientific acceptance and external prerequisites remain unfinished.

## Durable opt-in improvement scheduling

131 Rust tests, formatting/Clippy, debug/release builds and actual Chromium desktop/mobile acceptance pass. Three queue tests verify bounded/current-build configuration, atomic pending work, pause/reopen and recovery of already committed diagnosis/review evidence without new attempts, stale-coordinator refusal and state-pressure pause retaining the pending job. Schema9 stores permanent job/parent/diagnosis/review links; registry integrity checks cross-links and foreign keys. Browser acceptance actually processes ten opt-in jobs, inspects CLI history and verifies base simulations and child receipt links remain separate in the API/UI.

Default scheduling remains disabled; enabled runs cap at four children and one queued review, without recursive descendants. Current improvement checkpoints process a bounded review batch; finer per-child checkpoints remain pending. No synthetic qualification, broader improvement acceptance, hard resource quota or worker supervision is claimed.

## Individual improvement experiment checkpoints

133 Rust tests, formatting/Clippy, debug/release builds and actual Chromium desktop/mobile acceptance pass. Two added tests reopen the registry after every missing baseline/child commit and verify exactly one new attempt per advancement; publication performs none and explicit/incremental final report identities match. The worker is paused/restarted after its first child, retains the pending diagnosis and finishes only the remaining work without changing base counters. Browser acceptance drains ten jobs through separate process checkpoints and retains correct history/API/UI counters.

Automatic review work now yields between individual experiments, preserving coordinator renewal and pause/stop checks. A single replay still runs to its existing safe completion boundary; no preemption, hard resource enforcement or supervision is claimed. Queue/report schemas and historical scientific evidence remain unchanged.

## Bounded factual training evidence

136 Rust tests, formatting/Clippy, debug/release builds and actual Chromium desktop/mobile acceptance pass. The real loss diagnosis reconciles exact -$5.9582 net, $14.0418 realized cash, one losing exit and the correct year/entry bin. Two new summary tests independently check cross-year exit attribution, entry bins, strict streak reset at breakeven, exact -$10 total/615-micro commission totals, recorded versus recomputed account values, empty estimates, i128 overflow, chronological/exclusive-window refusal and reason budgets. An actual v1 report was generated before extension and read unchanged afterward; its committed small synthetic receipt fixture tests original hash readability with the optional field absent.

Diagnostics remain training-only/INSUFFICIENT_EVIDENCE. Profile/calendar approval, independent sample count, coverage, inference and improvement acceptance are not established by descriptive breakdowns. Historical reports are preserved, and no OOS/final evidence feeds adaptation.

## Declared-friction training stop hypotheses

139 Rust tests, formatting/Clippy, debug/release builds and actual Chromium desktop/mobile acceptance pass. Three new integration tests verify an actual protective-distance rejection, priority/cap/canonical proposal and comparable baseline/child replay: short entry100030, stop100071, target/exit99907, volume19220, exact net $2.364060. A passive limit probe excludes entry slippage and preserves its entry/ATR policy; no-op/over-budget floors yield no probe. Both sides still reject an independently wider observed spread. Existing failure/unresolved/scientific authorization, immutable backup/restart and legacy-v1 readability tests continue passing.

The v3 hypothesis uses profile minimum ticks plus declared spread-floor and applicable entry slippage; actual gaps/spread/risk/margin may still refuse entry. It does not repair or resize an existing order, lower costs, prove improvement, or qualify a strategy. Compute-source changes make archived benchmark receipts historical; no new speed claim is made.

## Independent all-instrument sizing oracle

140 Rust tests, formatting and Clippy pass. One new exhaustive oracle covers192 combinations: eight synthetic profiles, two directions, four equities and three commission levels. It independently enumerates100 possible volumes and computes rounded total risk, fees, post-entry marked equity and initial margin using explicit JPY/CHF/CAD conversion rationals. Exact largest-feasible volume and every recorded reservation field match the production search; minimum-volume infeasibility is also exercised. This is synthetic contract/arithmetic evidence, not approved broker/conversion sourcing. Production code is unchanged; debug/release and browser checks passed at the immediately preceding stop-hypothesis milestone.

## Bounded seeded training structure and mutation classes

143 Rust tests, formatting/Clippy all targets, debug/release builds and actual Chromium desktop/mobile acceptance pass. Two new generation tests check exact reproducibility for equivalent reordered parents, changed seeds, bounded unique canonical concepts, primary execution/cost preservation, term removal, next-frame confirmation, template crossover, incompatible direction/timeframe, contradiction, daily ceiling and oversized flattening refusal. One integration test starts with proven no signals, removes a term and independently verifies the actual -$5.9582 losing trade. Two-child budgets retain scalar and structural classes; existing bounded jobs/recovery/scientific authorization/legacy fixtures still pass. Training diagnosis version4 separates new immutable reports from prior versions.

Controlled crossover uses only fixed template rules matching primary timeframe and direction; donor execution settings are ignored and no donor is represented as independently confirmed. Complexity/time hierarchy is never relaxed; invalid children are omitted. Up to8 structural candidates are seed-ranked and deduplicated, then alternated with scalar/stop probes within the existing total16/automatic4 caps and no-descendant queue. This is canonical mutation-class coverage, not proven behavioral diversity, recursive acceptance or scientific improvement.

The standalone generation benchmark child exited0, with identical ordered workload hashes across five100-seed repetitions. Median0.054807ms/parent call includes generation/canonicalization/hash/output validation;695 candidates across100 calls yield56 canonical concepts. Peak whole-child RSS10496KiB. Source/per-file digests were checked against the measured working tree; complete CPU/cgroup/binary/resource receipts are retained. This is not tick/research throughput, an A/B speedup or independently useful diversity. Hosted CI API inspection again received proxy CONNECT403 for api.github.com; local checks and Git remote SHA verification are available.

## Canonical Boolean output validity

145 Rust tests, formatting/Clippy, debug/release builds and actual Chromium desktop/mobile acceptance pass. A directed regression failed on the preceding implementation because valid nested nodes could flatten into more than eight direct terms, producing a supposedly canonical rule rejected by the next normalization. The canonicalizer now applies the existing arity limit after flattening/deduplication. Both All/Any oversized cases refuse, equivalent overlapping eight-term sets still collapse correctly, and1000 seeded nested-rule attempts verify idempotence and JSON round-trip for every accepted result. The structural generator's complexity refusal still passes. No financial assertion, accepted complexity limit or scientific gate was relaxed.

## Persisted experiment history pages

146 Rust tests, formatting/Clippy, debug/release builds and expanded actual Chromium desktop/mobile acceptance pass. The authenticated history API returns at most100 rows (default25), a decimal-string continuation cursor, and server-side instrument filtering. It uses bound parameters and a strict descending rowid boundary without offsets/count scans. Existing `/api/experiments` retains its newest100 array shape. One isolated SQL projection test exercises rowid9223372036854775807, exact string continuation, symbol filtering, empty boundaries and unsafe summary-money null guards; it does not represent a valid research registry.

Browser acceptance creates110 distinct proven-no-signal receipts through the actual CLI and traverses17-row pages without duplicates. A new insert between requests stays outside the older cursor range; all110 originals appear in exact reverse insertion order. Older GBPUSD receipts beyond the global100-row cutoff remain discoverable. Invalid sizes/cursors/symbols/unknown query fields return400; unauthenticated history returns401. Both mobile/desktop controls traverse older/newer/newest pages, reset on instrument changes and preserve layout. A deliberately delayed old EURUSD response cannot overwrite a newer GBPUSD selection. Provenance cards are labeled as current-page evidence, not a full dataset inventory. Trade/protocol/run pagination and the remaining management/operations UI are still unfinished.

## Local release and recovery operations

The unchanged146 Rust tests, formatting and Clippy pass; debug/release and expanded browser acceptance passed at the preceding history milestone. `python3 scripts/check-operations.py` passes against actual release/debug binaries, using disposable prefixes including spaces. It verifies install/reinstall, update/rollback/activation, manifest/binary checksum tamper refusal with current release retained, preservation of local config/registry and unrelated activation files, native verified backups, staged new-path restores, exact persisted counts/diagnosis integrity, preservation of original backup bytes, overwrite/sidecar/live-backup refusal, invalid registry handling, staging cleanup and root-prefix refusal. Backup/restore publish only a verified new file and never replace a running registry.

Two populated non-root user-unit templates pass `systemd-analyze --user verify`. The initial direct verification lacked a runtime directory and failed; the acceptance supplies a temporary owner-only XDG_RUNTIME_DIR and populated installed paths, then verifies successfully without starting a manager/service. Templates declare CPU/memory/task limits and loopback dashboard operations. Live enforcement, reboot survival, target-VPS execution, production monitoring/retention, destructive cutover and schema downgrade compatibility remain unverified. CI now runs release-build and operations acceptance, but hosted status remains blocked/unverified. No actual service or user configuration was installed outside the temporary acceptance directories.
