# Bounded shared candidate batches

`batch-experiments <one-input-config.json> <same-timeframe-strategies.json> <registry.sqlite>` evaluates1..64 canonical strategies over one input/frame/window. The input configuration uses the existing research schema, default cost contract, event/feature/state limits, optional training window and shadow/proof policies. Real data requires explicit training scope. `improvement_children` must be0: this command does not schedule autonomous descendants or create a research coordinator.

Create a new strategies JSON array, for example:

```json
[
  {"timeframe":"M1","side":"Long","stop_ticks":50,"rule":{"AboveMean":{"window":5}}},
  {"timeframe":"M1","side":"Long","stop_ticks":100,"rule":{"AboveMean":{"window":5}}}
]
```

After generating the synthetic fixture as in README, run:

```sh
cargo run --locked -- batch-experiments config/research.example.json state/strategies.json state/research.sqlite
```

The library APIs are `research::evaluate_batch` (executions) and `evaluate_batch_report` (executions plus shared primary/visible counts and feature-build/cache-hit counters). Statistics describe this batch compilation only, not all nested compilation inside authoritative replay. They remain outside immutable experiment results, so cache history cannot alter evidence hashes.

An immutable borrowed input is validated before creating private preparation. Its exact conversion/data/cost/feature/policy configuration hash, closed primary candles and bounded feature cache are reused. Rust ownership prevents caller mutation during the batch; private preparation cannot be injected through the public API. Authoritative replay still independently validates every accepted signal/quote/accounting path. Higher-context, pending, ATR, proof and exclusive-window behavior use the same result kernel as scalar evaluation. No cost/risk/fill/scientific assertion is removed. M1 diagnostic reconstruction is still per candidate; this is not a complete streaming/hot-array optimization.

Every experiment keeps its existing exact key, fenced lease and immutable receipt. One lease runs at a time. Each completed prefix is durable; a busy member or state-pressure error returns without rolling back previous experiments. Retry reuses the prefix and any independently completed worker receipt. State budgets are checked before each candidate. Preflight compilation errors return before registration, including for an all-cached request; this differs from asking the scalar API to reuse a previously failed compilation. This manual command has no research pause/resume coordinator or graceful batch-specific signal checkpoint contract; the existing research loop retains its own controls. Do not assume background work after the command exits.

## Validation and measurements

Five new tests compare exact IDs/outcomes/full JSON across all8 synthetic instruments, fees, causal higher context, MACD, exclusive windows, ATR, pending entries and proof policy. They cover invalid/corrupted preflight input, budgets, mixed frames, busy-worker committed-prefix recovery and the actual CLI with separate cached reuse. The full suite is178 tests. No synthetic result qualifies.

The final `shared-batch-v2-prepared` comparison uses8 inputs x12 unique same-M1 candidates x12000 quotes, identical costs and feature budget. Five rotated scalar/shared pairs each execute96 new attempts in a fresh registry and96 separately timed registry cache reads. All96 candidates close nonzero actual trades; all10 complete execution fingerprints agree,34829 trades/batch. These are96 case identities repeatedly measured, not960 independent scientific trials. No cache reuse substitutes for the960 measured new attempts. The960 raw registry cache reads are not complete cached batch-API latency measurements.

Median complete new evaluation is1279.078645ms scalar versus1025.118184ms shared, about20% lower on this synthetic corpus. Median raw cache-read106.696238/102.836829ms is separate. Shared feature builds total48 versus192 independent component builds; full shared counters record216 cache hits. The isolated component (including mask hash/checks) totals4.849483/1.588861ms and is not the pipeline result. Whole-child RSS142332KiB includes all modes, setup, checks and caches; it is not a per-path memory comparison. Both phase equality and source manifest hashes were verified. Real/full-history/VPS performance and automatic scheduler promotion remain pending.

## Retained negative baseline

The first successful `shared-batch-v1` implementation still repeated immutable validation/configuration hashing per candidate. It was slower:1351.508579ms scalar versus1552.053417ms shared despite fewer feature builds. It was not promoted. Its source-pinned negative receipt remains alongside the improved comparison. `shared-batch-v1-source.patch` reverses the final preparation change to exactly reconstruct the old `src/research.rs` SHA25606eed82dbfe2a11b431ca60121be24cc6b3c5d3b2a9dfb0f43cd58397b4704f1; every other measured source file is identical. For reproduction in an isolated copy of this milestone, apply that patch and verify every old resource-receipt source digest before rebuilding. `git apply --check` passed; the live checkout was not reverted.

The initial6000-quote benchmark correctly failed its nonzero-trade assertion because the shorter sample left positions unresolved. The final12000-quote sample spans24h deadlines and passes the unchanged assertion. This is workload correction, not a financial-model change. Broader feature/corpus/timeframe tests, automatic checkpoint-aware batch scheduling, full accounting indexed replay and representative acceptance remain separate work.
