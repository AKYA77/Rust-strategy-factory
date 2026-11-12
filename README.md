# Rust Strategy Factory

Historical strategy research and paper simulation only. **SOFTWARE_INCOMPLETE.** No qualified strategies, approved broker profiles, real-data acceptance or full research readiness are claimed.

The current Rust library/CLI implements exact integer contracts, explicit synthetic profiles for eight symbols, normalized bid/ask CSV import with content hashes and quarantine checks, closed UTC candles on seven timeframes, causal SMA/EMA/RSI primitives, shared mean features and dense masks, canonical Boolean rule identities, an initial deterministic candidate grid, conservative signal-only screening and sequential market-entry tick replay, a SQLite experiment registry with fenced leases and verified backup/restore, frozen holdout reservations, and numerical qualification/chronological fold utilities. An experimental fixed-barrier index is tested but disabled in replay. It is an implementation checkpoint, not the complete factory.

## Development

Install the toolchain in `rust-toolchain.toml` using rustup. On the configured cloud machine:

```sh
export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
export PATH="$CARGO_HOME/bin:$PATH"
cd /workspace/Rust-strategy-factory
cargo build --locked
sh scripts/check.sh
cargo run --locked -- --help
```

No external runtime service or secret is needed for the current CLI. `cargo run --locked -- profiles` prints explicitly synthetic software-test profiles. To use one, save a single profile object into a local JSON file. These conventions, particularly volume steps, leverage and gold pip size, are arbitrary test inputs and cannot establish broker feasibility.

CSV input has exactly `timestamp_ms,sequence,bid,ask` headers. Prices are exact decimal strings, timestamps UTC Unix milliseconds, and sequences globally strictly increasing in source event order. Timestamps may tie. Import never sorts or repairs events. Parse/precision errors fail; structurally invalid event streams are saved quarantined and return failure. Source files remain untouched. Import budgets are 64 MiB and one million events; output paths must be new.

```sh
cargo run --locked -- import sample.csv profile.json source-description dataset.json synthetic
cargo run --locked -- inspect dataset.json
cargo run --locked -- candles dataset.json M1
cargo run --locked -- generate
cargo run --locked -- evaluate dataset.json strategy.json result.json
cargo run --locked -- experiment dataset.json strategy.json registry.sqlite
cargo run --locked -- registry-check registry.sqlite
cargo run --locked -- backup registry.sqlite backup.sqlite
```

Example strategy:

```json
{"timeframe":"M1","side":"Long","stop_ticks":30,"rule":{"AboveMean":{"window":2}}}
```

`evaluate` currently supports USD-quoted profiles; non-USD conversions must be supplied explicitly per event through the Rust API. Results remain hypothetical and retain source/profile/cost/strategy identities and full trade receipts. The initial account is $20, intended risk 10%, planned target 3R, minimum spread two pips, separate adverse market/stop slippage two pips, maximum holding period 24 hours. Limits at the target receive the requested price without market slippage. Deadline expiration cancels protective orders before processing the first event at or after the deadline. No quote means no invented exit. Gaps can exceed intended risk.

Spread policy v1 anchors recorded bid and widens ask upward; neither executable side improves. Candles use a continuous UTC research calendar, publish only when the next actual event proves closure, and discard the unfinished final bucket. Signals execute strictly after their publication event, including sequence ordering for timestamp ties. This calendar is not an approved broker/DST calendar. L1 fills assume full execution; depth and queue feasibility are unknown. Only one position is open at a time. Commissions are configurable through the Rust API; orders exceeding the risk budget are rejected.

See [acceptance evidence](docs/ACCEPTANCE.md), [unfinished work](docs/IMPLEMENTATION_QUEUE.md), and the authoritative [master directive](docs/MASTER_BLUEPRINT.md).

`experiment` runs conservative screening and sequential verification with a durable identity that includes the executable BLAKE3 hash. Completed results are reused without a new simulation. A five-minute fenced lease protects completion; failed/rejected/inconclusive evidence is retained. There is no autonomous scheduler yet. Backup is online and checks integrity; test restores by opening the new backup with `registry-check` before using it. Holdout reservations exist through the Rust API and block overlapping registered research; they are scoped to an immutable dataset hash and do not yet protect against exposure through other corpus revisions or external tools. No qualification endpoint is provided.

Benchmark: `cargo bench --locked --bench replay` runs the explicit synthetic baseline. For process resources, first compile with `cargo bench --locked --bench replay --no-run`, then pass the reported executable path and two new output paths to `python3 scripts/measure.py`. Python is development-only. See `docs/BENCHMARKS/README.md` for measurements and limits.

The `orders` library implements immutable pending market/stop-market/limit entry orders, cancellations, quote-free expiry and restart snapshots. These APIs use the declared L1 full-fill assumption. Strategy-generated pending orders, reserved risk/margin and position integration remain unfinished; the CLI strategy evaluator now also accepts an optional `entry` plan, as documented below.

Pending-entry example field on a strategy: `"entry":{"StopMarket":{"offset_ticks":30,"ttl_ms":60000,"cancel_on_signal_false":true}}`; use `Limit` with the same parameters for a pullback entry. Omitting `entry` preserves market-entry concept identity. Engine v2 sizes against post-entry marked equity, commissions and initial margin, and fixes pending quantity at placement. It rejects infeasible fills before execution rather than resizing them. See `docs/DECISIONS/0001-pending-entry-reservations.md` for event precedence and explicit research assumptions.

`generate` now emits valid distinct strategies from all nine required families. RSI is Wilder-smoothed and compared at nearest basis-point precision; Donchian/channel references exclude the current candle. Session masks use UTC publication time and explicit weekday bits, not broker/DST sessions. Shared caches evict unused features within their cell budget. Higher-timeframe confirmations, additional operators, adaptive mutations and full scientific evaluation remain unfinished.

A standalone training research loop is now available: `research config/research.example.json state/research.sqlite 10` runs ten safe checkpoints; omit the step budget for continuous operation. Create a small explicit synthetic input with `fixture EURUSD state/fixture.json` after making `state/`. Use the printed run ID with `research-status` and `research-control ... running|paused|stopped`. See [research loop operations](docs/RUNBOOKS/research-loop.md) for tested setup, budgets, conversions, restart and limitations. There is still no complete independent qualification or dashboard.

An authenticated loopback dashboard now reads real persisted results and controls existing runs. Initialize a new Argon2 password file with `auth-init` (password from stdin), then use `serve <registry.sqlite> <auth.json> 3000`. See [dashboard operations](docs/RUNBOOKS/dashboard.md). `npm run test:browser` verifies actual Chromium desktop/mobile workflows; Node is development-only. The dashboard and the full scientific research product remain incomplete, with pending operations explicitly listed in the runbook and queue.

Chronological replay/evaluation windows are now supported by the Rust API and `limits.training_window` (`start_ms`, `end_exclusive_ms`) in research configuration. Real autonomous inputs require explicit training scope. Warmup cannot open earlier positions; no post-window quote resolves a position. The `version` command prints engine/indicator/signal and compiled compute identities. UI-only assets do not invalidate completed computations; changed provenance or relevant compute/build inputs do. Full independent scientific orchestration remains pending.
