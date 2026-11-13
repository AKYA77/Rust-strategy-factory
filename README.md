# Rust Strategy Factory

Historical strategy research and paper simulation in Rust. **SOFTWARE_INCOMPLETE.** No qualified strategies, approved broker profiles, real-data acceptance or full historical research readiness are claimed.

The current implementation includes exact integer accounting, eight explicit synthetic instrument profiles, bounded normalized bid/ask import, closed UTC candles on seven timeframes, causal features including MACD/Bollinger crosses, typed closed higher-timeframe confirmations and nine strategy families, market/stop/limit entry replay with fixed reservations, typed outward ATR initial stops and marked-margin checks, a durable single-worker research loop, immutable SQLite experiment evidence, chronological evaluation windows, frozen protocol execution, and an authenticated loopback dashboard.

## Development

Toolchain and dependencies are pinned. In this cloud environment:

```sh
export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
export PATH="$CARGO_HOME/bin:$PATH"
cd /workspace/Rust-strategy-factory
cargo build --locked
sh scripts/check.sh
cargo run --locked -- --help
npm --cache /workspace/.npm-cache ci --ignore-scripts
npm run test:browser
```

Node and Chromium are development dependencies only. The production dashboard is embedded in the Rust executable. No external runtime service or secret is needed for CLI research.

## Research and evidence

Native `.rsf` storage supports lossless packing/unpacking and the same dataset identities; see [native storage](docs/RUNBOOKS/native-storage.md). This is an uncompressed interchange baseline; full-history streaming and codec selection remain unfinished.

Training-only deterministic diagnostics persist bounded unevaluated child proposals; see [training improvement](docs/RUNBOOKS/training-improvement.md). Scientific/OOS/final receipts cannot authorize adaptation. Explicit bounded reviews replay comparable parent/children and retain immutable evaluation edges; opt-in bounded one-worker jobs retain scheduling/recovery state. Individual child checkpoints preserve pause/restart recovery; full improvement acceptance remains unfinished.

Start with [research loop operations](docs/RUNBOOKS/research-loop.md), [frozen protocol execution](docs/RUNBOOKS/scientific-protocol.md), and [dashboard operations](docs/RUNBOOKS/dashboard.md). `fixture` creates explicitly synthetic software-test data; `profiles` prints arbitrary test contracts. Neither establishes broker feasibility. Real autonomous research requires an explicit training interval.

```sh
mkdir -p state
cargo run --locked -- fixture EURUSD state/fixture.json
cargo run --locked -- research config/research.example.json state/research.sqlite 10
cargo run --locked -- registry-check state/research.sqlite
cargo run --locked -- backup state/research.sqlite state/backup.sqlite
cargo run --locked -- version
```

Completed experiments are reused by exact provenance, strategy, profile, costs, window, protocol and compiled compute identity. Relevant source/compiler/target/build settings affect that identity; embedded UI assets do not. Fenced leases, durable cursors, immutable result hashes and verified online backups preserve evidence. The loop supports pause/resume/stop; worker pools and production supervision remain unfinished.

Frozen protocols execute selection training, prior-only expanding training, complete three-month OOS folds and a protected final interval. Reports remain `INSUFFICIENT_EVIDENCE`: orchestration alone does not establish independent qualification. Corpus-wide alias exposure, uncertainty/multiple testing, robustness and final qualification are unfinished. Synthetic results can never qualify. The eight top-five boards are honestly empty.

## Financial and data assumptions

Prices, volume and money use exact integers. Initial equity is $20, intended risk 10%, planned target 3R and maximum holding period 24 hours. The minimum spread is two pips; adverse market/stop slippage is separately two pips. Passive target limits use their requested price. Gaps can exceed the risk budget; absent quotes never create exits. One pending order or position is allowed. L1 fills assume full execution; depth and queue feasibility are unknown. Initial stops must remain at least the profile minimum distance beyond the executable mark after entry friction. Infeasible entries are rejected before a trade opens.

CSV headers are exactly `timestamp_ms,sequence,bid,ask`. Prices are decimal strings, timestamps UTC milliseconds and sequence numbers strictly increase in original event order. Timestamp ties are retained; import never sorts or repairs events. Parse failures reject the import; structurally invalid streams are quarantined. Output paths must be new. Non-USD conversion requires explicit event-aligned prior-only rates. UTC continuous research candles publish only after a subsequent real event proves closure; unfinished final candles and fabricated gap candles are excluded. Broker/DST calendars remain unapproved.

See the [master directive](docs/MASTER_BLUEPRINT.md), [acceptance history](docs/ACCEPTANCE.md), [project state](docs/PROJECT_STATE.json), [complete unfinished queue](docs/IMPLEMENTATION_QUEUE.md), and [next action](docs/NEXT_ACTION.md). Synthetic benchmark receipts in `docs/BENCHMARKS` describe their specific source versions and workloads; they do not establish real-data throughput. The experimental first-hit index remains disabled in replay.

Optional ATR stop_policy uses stop_ticks as a minimum floor and freezes the distance at pending placement. Unknown strategy/rule fields are rejected. See [ATR policy and exact fixtures](docs/DECISIONS/0008-atr-initial-stops.md).
