# Rust Strategy Factory

Historical strategy research and paper simulation only. **SOFTWARE_INCOMPLETE.** No qualified strategies, approved broker profiles, real-data acceptance or full research readiness are claimed.

The current Rust library/CLI implements exact integer contracts, explicit synthetic profiles for eight symbols, normalized bid/ask CSV import with content hashes and quarantine checks, closed UTC candles on seven timeframes, causal SMA/EMA/RSI primitives, shared mean features and dense masks, canonical Boolean rule identities, an initial deterministic candidate grid, conservative signal-only screening and sequential market-entry tick replay. It is an implementation checkpoint, not the complete factory.

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
```

Example strategy:

```json
{"timeframe":"M1","side":"Long","stop_ticks":30,"rule":{"AboveMean":{"window":2}}}
```

`evaluate` currently supports USD-quoted profiles; non-USD conversions must be supplied explicitly per event through the Rust API. Results remain hypothetical and retain source/profile/cost/strategy identities and full trade receipts. The initial account is $20, intended risk 10%, planned target 3R, minimum spread two pips, separate adverse market/stop slippage two pips, maximum holding period 24 hours. Limits at the target receive the requested price without market slippage. Deadline expiration cancels protective orders before processing the first event at or after the deadline. No quote means no invented exit. Gaps can exceed intended risk.

Spread policy v1 anchors recorded bid and widens ask upward; neither executable side improves. Candles use a continuous UTC research calendar, publish only when the next actual event proves closure, and discard the unfinished final bucket. Signals execute strictly after their publication event, including sequence ordering for timestamp ties. This calendar is not an approved broker/DST calendar. L1 fills assume full execution; depth and queue feasibility are unknown. Only one position is open at a time. Commissions are configurable through the Rust API; orders exceeding the risk budget are rejected.

See [acceptance evidence](docs/ACCEPTANCE.md), [unfinished work](docs/IMPLEMENTATION_QUEUE.md), and the authoritative [master directive](docs/MASTER_BLUEPRINT.md).
