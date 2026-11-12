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
