# Standalone research loop

The Rust process runs without Codex or an LLM. Current scope is training research with one bounded worker and one in-flight task. It does not issue independently validated qualifications. Candle screening rejects only proven absence of signals. Signaled receipts now include bounded hypothetical USD M1 cash estimates and conservative shadow-policy comparisons with actual ticks; uncertainty/unsupported/partial cases stay explicit. Every signaled candidate still runs ticks. See [candidate screening](candidate-screening.md) for assumptions, limits and remaining policy work.

Build with `cargo build --locked --release`. For a synthetic development exercise, from the repository root:

```sh
mkdir -p state
cargo run --locked -- fixture EURUSD state/fixture.json
cargo run --locked -- research config/research.example.json state/research.sqlite 10
```

The optional final number is a completed-step budget. Omit it to keep researching until stopped or the configured search budget is exhausted. The first output line is compact JSON containing `run_id`; final output is a persisted status. Paths inside the configuration resolve relative to the configuration file. No files are overwritten by `fixture`. These are short artificial 2025/2026 windows, not real coverage or a full 24-month dataset.

Use actual verified normalized datasets for real training research. Non-USD quote inputs require an explicit event-aligned conversion JSON file, referenced as `"conversions":"path.json"` alongside `dataset`. Future/stale/missing rates are rejected; no rate is inferred. Source/profile/complete-history approval and independent scientific evaluation remain mandatory before any qualification.

Control a running coordinator from another terminal, substituting its actual printed ID:

```sh
cargo run --locked -- research-status state/research.sqlite RUN_ID
cargo run --locked -- research-control state/research.sqlite RUN_ID paused
cargo run --locked -- research-control state/research.sqlite RUN_ID running
cargo run --locked -- research-control state/research.sqlite RUN_ID stopped
```

Pause/stop apply after the current safe task. Paused coordinators wait with lease heartbeats. A stopped run exits; set it back to `running` before restarting it. A finite step budget releases the coordinator lease without resetting its desired control state or cursor. Restart with the same binary, inputs and configuration to continue its cursor. A crashed coordinator or experiment can be reclaimed after its lease expires; a stale worker cannot advance the cursor. A result committed just before a crash is reused rather than simulated again. The compiled compute-source/compiler/target/profile/dependency digest participates in experiment/run identity. Embedded UI-only changes preserve it; changed computation or build profile establishes a new execution revision. Keep one build profile/revision for a frozen validation protocol.

Each input gets one candidate in round-robin order. Frozen base templates expand into deterministic new parameter/entry regions. `max_regions` is a deliberate campaign budget; exhaustion stops with the cursor intact rather than endlessly generating invalid duplicates. Counters distinguish newly completed tick simulations, rejected zero-signal screens, cached results and failures. They do not claim full-history evaluation or qualification. Permanent step records retain failures and cursor outcomes.

Limits: at most eight inputs, configured total event budget up to two million, 128 MiB total JSON input files at CLI load, one worker/one active task, at most four million retained feature cells, bounded grammar and result metadata. The state-file budget covers the SQLite file/WAL/shared-memory files and pauses before another task when headroom is low. It is backpressure, not a filesystem hard quota; one task may change file sizes before the next check. No scientific evidence is deleted to recover space. Increase capacity or archive/restore through a reviewed operator procedure; do not remove committed evidence or reset experiment identities.

Large native history, large immutable result artifacts, dynamic worker pools/supervision, OS-enforced CPU/RSS quotas and comprehensive crash/disk-full/VPS acceptance remain unfinished. Current oversized result publication is recorded as failure, never described as a completed backtest. Use `backup` and `registry-check` to create and verify an online backup; only start a restored coordinator when old processes are stopped and stale leases have expired. Other application services are unaffected.

Optional `limits.training_window` is an object with `start_ms` and `end_exclusive_ms`. Real-data autonomous runs require it; synthetic software tests may omit it. Prior candles only warm indicators. Each evaluation starts flat at $20, excludes earlier signals, and consumes no quote at or beyond the exclusive end. Still-open positions/pending orders remain unresolved, with no invented boundary fill. This is the execution foundation for independent protocols, not a completed scientific validation/approval workflow.

Optional `limits.improvement_children` enables durable training improvement jobs (default0, maximum4). Each completed or proven-no-signals base checkpoint atomically schedules one job. The next checkpoint drains it before another base candidate, preserving at most one queued job and bounded depth: children do not schedule descendants. Only the current compiled revision can enable this mode. Explicit training authorization and exact-context review checks still apply. A job records diagnosis/review identities or its failure; `improvement-jobs <registry.sqlite> <run-id>` reads the latest100 records, with complete history retained in backups.

Base counters (`executed`, `rejected`, `reused`, `failed`) continue to describe base-candidate checkpoints. Separate improvement counters describe queued/completed/failed jobs and child receipt links. Receipt links can reference cached/shared experiments and are not new simulation counts; registry experiment/lease counts remain authoritative. A completed job may have no child proposals when its evidence is unresolved. The dashboard labels these scopes explicitly. The CLI step budget counts base or improvement checkpoints, so the base cursor can stay unchanged while a job completes.

A pending job survives pause/stop/restart and coordinator fencing. Evidence committed before its job checkpoint is reused. State pressure pauses with its queued job intact; other job errors remain permanent failed records. Improvement checkpoints commit at most one missing experiment. Subsequent checkpoints reuse earlier receipts; a final checkpoint publishes the complete review. Pause/stop and coordinator renewal occur between children. A single running experiment must still reach its safe completion boundary. No OOS/final adaptation, qualification, OS hard quota or unattended process supervisor is supplied by this feature.

On Unix, SIGINT and SIGTERM now request a graceful process exit at the existing experiment checkpoint boundary. Listeners are installed before the coordinator ready line. After the current step, the coordinator releases its lease and prints final status; desired running/paused state is preserved, so the same binary/config can restart immediately. A paused process also exits promptly. These signals do not preempt a replay or change its financial outcomes. SIGKILL/OOM/hard service timeouts and crashes still rely on committed receipts/fencing/lease expiry. Process acceptance tests cover both signals in both running/paused state, no leftover claimed experiment and immediate same-run restart.
