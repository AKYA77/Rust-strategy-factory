# MASTER DIRECTIVE — BUILD OUR OWN ULTRA-FAST RUST STRATEGY RESEARCH FACTORY

**Project:** Rust Strategy Factory  
**Directive date:** November 12, 2026  
**Suggested new GitHub repository:** `AKYA77/rust-strategy-factory`  
**Target development language:** Rust  
**Target deployment:** Ubuntu VPS, 4 vCPUs, 8 GB RAM  
**Scope:** Historical strategy research and paper simulation only

---

# 0. YOUR MISSION

You are Codex, acting as the complete software architect, Rust engineer, performance engineer, quantitative research-system developer, test engineer, dashboard developer, security engineer and release engineer.

Your assignment is to **design, implement, optimize, test, document and publish a fully functioning custom Rust strategy research factory**.

THIS IS AN IMPLEMENTATION DIRECTIVE.

Do not finish by producing a blueprint, a repository skeleton, mock endpoints, incomplete modules or an unverified prototype.

Continue building the complete product through a persistent, dependency-aware engineering loop.

Our objective is to create the smallest, fastest and most accurate purpose-built research factory that our available hardware can support.

It must:

1. Generate enormous numbers of genuinely distinct, valid strategy candidates.
2. Screen candidates extremely quickly using cached one-minute market data and shared indicators.
3. Promote promising or scientifically unresolved candidates to exact historical tick verification.
4. Use correct executable bid/ask prices, spread, slippage and instrument-specific contract conventions.
5. Find strategies meeting our defined historical research criteria.
6. Analyze failures, improve candidates and continuously generate new hypotheses.
7. Validate strategies using independent chronological evidence.
8. Maintain up to five independently qualified strategies for each of eight instruments.
9. Avoid repeating identical completed experiments.
10. Run autonomously, restart safely and use bounded hardware resources.
11. Provide a complete, functional and authenticated web dashboard.
12. Push tested changes and durable progress records to GitHub.
13. Operate without any LLM connection after the software has been built.
14. Preserve scientific integrity regardless of the speed optimizations used.

**The final product should be our own Rust application—not a collection of existing backtesting frameworks connected together.**

Reuse existing open-source algorithms and libraries where beneficial, but own the application architecture, strategy grammar, screening pipeline, execution model, validation system and research scheduler.

Do not create unnecessary infrastructure.

---

# 1. COMPLETELY NEW PROJECT

This is NOT a migration of the previous Hermes Factory.

Do not continue developing the existing repository.

Do not modify:

`AKYA77/hermes-factory`

Do not use its historical architecture or development instructions as the authority for this new project.

Its previous discussions and design documents may be consulted as reference material for feature requirements.

This directive is the new project's primary authority.

Create an independent Git repository.

Preferred repository:

`AKYA77/rust-strategy-factory`

First check whether the repository already exists.

If it exists, inspect ownership and contents before taking any action. Do not overwrite an unrelated project.

Otherwise, create the new repository using the existing authorized GitHub connection. Prefer private visibility for the new research repository.

If GitHub creation or authentication is unavailable, create a clean local repository, document the publication blocker and continue all independent implementation work.

Do not alter the previous project's repository, branch, pull requests, data or services.

The old project must remain unaffected.

---

# 2. ENGINEERING PHILOSOPHY

Follow these principles throughout development.

## 2.1 Build the minimum necessary system

Prefer:

- One Rust application.
- One authoritative execution engine.
- One reusable data pipeline.
- One strategy generator.
- One fast screening engine.
- One experiment registry.
- One research scheduler.
- One validation system.
- One lightweight dashboard.
- One reproducible deployment.

Avoid redundant microservices, unnecessary frontend frameworks, distributed queues, heavyweight databases, multiple competing backtesting engines, excessive dependencies and duplicated calculations.

Do not introduce a feature merely because another platform supports it.

Every feature must improve our research coverage, execution correctness, speed, scientific reliability, usability or operational stability.

## 2.2 Optimize measured bottlenecks

Use this process for every significant optimization:

BASELINE → PROFILE → OPTIMIZE → VERIFY CORRECTNESS → BENCHMARK → KEEP OR REVERT.

Do not assume an optimization improves performance.

Measure actual end-to-end throughput, CPU time, wall time, memory, storage and correctness.

Reject optimizations that make the engine slower on representative workloads or introduce unacceptable correctness problems.

## 2.3 Accuracy must never be silently sacrificed

An approximate M1 candle result must never be described as tick-accurate.

A cached result must not be counted as a new simulation.

A strategy that has not passed protected independent evaluation must not be labeled validated.

A missing market event must never be fabricated.

A hypothetical broker fill is not proof of historical executable liquidity.

A benchmark number must not be invented or extrapolated from an unrelated operation.

## 2.4 All computation-heavy production code should be Rust

Use Rust for:

- Data processing.
- Candle aggregation.
- Indicators.
- Strategy generation.
- Strategy compilation.
- Deduplication.
- M1 screening.
- Tick execution.
- Tick indexing.
- Risk calculations.
- Financial accounting.
- Optimization.
- Diagnostics.
- Statistical validation.
- Scheduling.
- Persistent state management.
- HTTP API.

Small Python or shell scripts may be used for development, CI, external reference fixtures and operational setup when justified.

Python must not be required in the production trading-research hot path.

Prefer one deployable Rust binary with statically served dashboard assets.

---

# 3. HARDWARE AND RESOURCE REQUIREMENTS

Target:

- Ubuntu 24.04-compatible deployment.
- Four available vCPUs.
- Eight GB physical RAM.
- Modest VPS storage and I/O.
- No GPU requirement.
- No paid hosted computation.
- No permanently running LLM.
- No Kubernetes.
- No distributed cluster.

The VPS may simultaneously run other services.

Do not assume all 8 GB RAM is available.

The application must implement configurable resource limits.

Initial target:

- Approximately 5 GB maximum application RSS under representative sustained load, adjustable after measurement.
- Bounded worker count.
- Bounded task queues.
- Bounded feature caches.
- Bounded decompression buffers.
- Bounded database writer queue.
- Bounded disk staging and temporary artifacts.
- Backpressure under CPU, RAM or disk pressure.
- Automatic crash and restart recovery.

Reserve sufficient memory for the operating system, filesystem cache and other processes.

Do not hard-code a worker count of four simply because four vCPUs exist.

Benchmark multiple worker counts and use the fastest stable configuration.

Use CPU affinity or specialized thread placement only when measurements justify it.

---

# 4. SUPPORTED INSTRUMENTS

Support exactly these eight instruments initially:

1. EURUSD
2. GBPUSD
3. USDJPY
4. USDCHF
5. USDCAD
6. AUDUSD
7. NZDUSD
8. XAUUSD

Each instrument must have an independently versioned specification describing:

- Symbol.
- Base currency.
- Quote currency.
- Account currency handling.
- Exact price scale.
- Tick size.
- Pip size.
- Contract size.
- Lot-size conventions.
- Minimum volume.
- Volume increments.
- Maximum permitted volume when applicable.
- Minimum stop distance where applicable.
- Margin and leverage assumptions.
- Trading calendar and sessions.
- Broker/profile provenance.
- Data-source identity.
- Relevant financial conversion rules.

Forex, JPY-quoted pairs and gold must not share inaccurate hard-coded monetary assumptions.

XAUUSD requires independently reviewed gold-specific specifications.

Observed historical quote precision does not establish broker contract size or pip-value conventions.

Where authoritative specifications are unavailable, permit explicitly labeled research profiles and deterministic synthetic test profiles.

Such profiles may support software testing, but must not establish verified broker feasibility or final historical qualification.

Do not invent specifications to make tests pass.

---

# 5. SUPPORTED TIMEFRAMES

Support seven configurable entry-signal timeframes:

- M1
- M5
- M15
- M30
- H1
- H4
- D1

Support higher-timeframe confirmation combinations.

For example:

- M1 entry with M15 confirmation.
- M5 entry with H1 confirmation.
- M15 entry with H4 confirmation.
- H1 entry with H4 and D1 confirmation.
- D1-only strategies.

Build candles from verified historical tick data when available.

Do not require separate downloads for every timeframe.

Candle construction must preserve:

- Bid and ask OHLC.
- Accurate timestamps.
- Correct UTC alignment.
- Trading-session boundaries.
- Daylight-saving-time handling.
- Market closures.
- Original event order.
- Data quality and source versions.
- Genuine historical price gaps.
- Correct finalization and publication time.

Never manufacture empty candles as if actual quotes existed.

Use only closed, available candles for signals.

Never allow a current or future unfinished candle to influence an earlier decision.

A signal can execute only against eligible subsequent market events.

Optimize strategies according to their actual required timeframes.

An H1-only strategy should not unnecessarily recalculate M1 indicators for every minute.

However, the default broad screening representation should retain shared M1-derived data and indexing.

---

# 6. HISTORICAL DATA REQUIREMENTS

## 6.1 Codex development dataset

DO NOT download the complete two-year tick dataset for all eight instruments during development.

It would consume unnecessary time, storage, bandwidth and development resources.

Instead, acquire enough representative real historical data to verify the software.

Target:

- All eight instruments represented.
- At least two distinct calendar periods per instrument, including 2025 and 2026 where available.
- Multiple trading sessions.
- Actual bid/ask tick sequences.
- At least some volatile and relatively quiet trading periods.
- Weekend and trading-session boundaries where relevant.
- Timestamp ties and sparse quote intervals.
- JPY-specific price increments.
- Gold-specific quote precision.
- Genuine gaps or anomalies when available and legally usable.

Prefer several short windows per instrument over one large continuous download.

A reasonable initial target is selected trading days or short weeks in 2025 and 2026, subject to source availability and size.

Keep the complete compressed development sample approximately within 1 GB unless a justified test requires more.

Avoid excessive temporary storage.

Allow the sample budget to be configurable.

Use available legitimate free sources, including HistData where appropriate, after checking actual availability, formats and usage terms.

Do not bypass provider restrictions, fabricate download success or use paid services without authorization.

If an actual source is unavailable, retain the blocker and continue with deterministic synthetic data.

Do not commit large raw datasets to GitHub.

Small redistributable fixtures may be committed only when their licenses permit it.

Preserve sample source URLs, acquisition timestamps, original archive hashes and parser versions.

## 6.2 Development data categories

Every dataset must be classified clearly:

- SYNTHETIC_TEST_ONLY
- REAL_UNREVIEWED
- REAL_STRUCTURALLY_VALID
- REAL_QUALITY_ACCEPTED
- REAL_PROFILE_APPROVED
- RESEARCH_ELIGIBLE
- QUARANTINED

A synthetically generated dataset must never be presented as actual market history.

A successfully parsed dataset is not automatically scientifically accepted.

## 6.3 Full historical data later

The finished application must support complete historical acquisition, import, verification, resume and incremental refresh.

The intended research period is:

January 1, 2025 through the latest verified available date in 2026.

The system must report precise coverage and the actual latest included event.

As of this directive, November 12, 2026, 2026 is still incomplete.

Do not call January 2025–November 2026 a full 24-month period.

If a separate exact 24-month research window is needed, support a configurable 24-completed-month interval.

The full historical backfill will occur later on the VPS.

Codex is responsible for creating and testing the data-management software, not downloading every historical tick now.

The software must support:

- Source discovery.
- Download only missing ranges.
- Resumable acquisition.
- Checkpointed imports.
- Immutable archives.
- File and chunk hashing.
- Dataset manifests.
- Data verification.
- Gap detection.
- Duplicate detection.
- Crossed quote detection.
- Out-of-order event detection.
- Corrections with immutable provenance.
- Quarantine.
- Source revisions.
- Calendar verification.
- Partial-month handling.
- Incremental updates.
- Storage quotas.
- Safe cache invalidation.

Include a command to inspect and import an already existing external historical dataset without downloading it again.

Never overwrite existing valid raw archives.

Full-data acceptance must remain explicitly pending until the VPS actually contains the required verified history.

---

# 7. FINANCIAL RESEARCH CONTRACT

These are mandatory versioned historical simulation settings.

## 7.1 Starting account

Initial hypothetical account balance:

**$20 USD**

Account currency:

**USD**

## 7.2 Risk per trade

Risk budget:

**10% of current simulated equity per trade.**

Recalculate the intended monetary risk budget before each new trade using current marked equity, according to the defined valuation policy.

Position sizing must account for:

- Entry price.
- Stop distance.
- Contract value.
- Instrument price scale.
- Tick/pip value.
- Account currency.
- Currency conversion.
- Minimum lot size.
- Volume step.
- Available simulated equity.
- Margin limitations.
- Execution assumptions.

Never force an infeasible minimum lot just to generate a trade.

If the $20 hypothetical account cannot support an eligible order under the approved instrument profile, skip or reject that order and record the exact reason.

A 10% intended risk budget is not a guarantee that actual realized losses cannot exceed 10%.

Historical gaps, fills and market conditions may produce larger simulated losses.

Do not silently cap calculated losses to the intended risk budget.

## 7.3 Reward/risk

Required initial target:

**1:3 risk/reward.**

For an initial stop distance of 1R, the planned target distance is 3R.

Allow the strategy generator to search different stop distances.

Maintain 3R initial target distance unless a future owner-approved version changes this contract.

Compute stops and targets using exact instrument tick grids and the approved price-reference semantics.

## 7.4 Maximum holding time

Maximum duration:

**24 hours per trade.**

An unresolved trade reaching its deadline must follow a deterministic, explicitly defined historical exit policy.

If no eligible exit quote exists, record that fact.

Never fabricate a fill at a nonexistent price.

## 7.5 Spread

Minimum effective spread:

**2 pips.**

Use the observed historical bid/ask spread when it is wider.

When the recorded spread is narrower, apply the defined pessimistic spread-floor transformation.

Do not double-count the recorded spread.

Do not treat spread as interchangeable with slippage.

The transformation must not improve historical executable prices.

Apply exact outward rounding to the instrument tick grid.

## 7.6 Slippage

Separate adverse slippage:

**2 pips per eligible market or stop-market fill.**

Apply slippage in the unfavorable direction for the actual buy or sell fill.

This includes applicable entries and exits.

Do not automatically apply market-order slippage to passive limit orders.

Preserve correct order-type semantics.

Do not manufacture better fills when an order crosses the market.

All costs and fills must be versioned.

Use the existing user-specified zero-commission hypothetical scenario as a configurable default, while allowing explicit nonzero commissions when scientifically required.

## 7.7 Account objective

Research objective:

**At least $500 hypothetical ending account equity.**

This is a research target, not evidence of achievable returns.

Track the achieved value accurately.

Never adjust risk, costs, fills or initial equity to force the objective.

---

# 8. STRATEGY QUALIFICATION CRITERIA

Each strategy must be assessed independently for each instrument.

Mandatory qualification gates:

### A. Win rate

Minimum verified net win rate:

**50.00%.**

Exactly 50.00% passes the numerical threshold.

Below 50.00% fails.

A winning trade must have positive net realized P&L after applicable costs.

Use:

Win rate = positive-net-P&L closed trades / all resolved closed trades.

Break-even closed trades count in the denominator and not the numerator.

Unresolved or invalid trades must be reported separately and cannot be used to manufacture qualification.

### B. Trade frequency

Require:

**At least 50 resolved trades in calendar year 2025.**

And:

**At least 50 resolved trades in 2026 year-to-date through the approved data cutoff.**

Do not annualize an incomplete 2026 period to fabricate the required count.

Assign yearly counts by a consistently versioned policy, using the completed trade's exit timestamp.

Report both years independently.

### C. Losing streak

Maximum permitted historical consecutive losing trades:

**5.**

Count losses chronologically across the complete evaluated sequence, including the boundary between 2025 and 2026.

Also report the maximum streak for each year.

A loss means a resolved trade with negative net P&L.

A break-even trade must not artificially erase an ongoing loss streak.

Report unresolved positions separately.

### D. Reward/risk

The initial strategy uses 1:3 planned RR with actual achieved net results calculated from real modeled fills.

### E. Validation

The strategy must pass the applicable independent scientific validation requirements.

Passing a training backtest alone is insufficient.

### F. Evidence

Require valid:

- Dataset provenance.
- Instrument profile.
- Cost profile.
- Execution version.
- Complete eligible historical coverage.
- Historical accounting.
- Independent validation evidence.

If any requirement lacks evidence, the strategy must not be labeled qualified.

### G. Further research ambition

Continue researching toward:

**70%+ verified win rate.**

This is an aspirational discovery target.

Never guarantee such performance.

Never weaken spread, slippage, sample sizes or validation requirements to achieve it.

---

# 9. STRATEGY GENERATOR

Implement a native Rust generator operating on typed declarative strategy representations.

Do not execute arbitrary user-generated or LLM-generated code inside the research engine.

Initial mandatory strategy families:

1. Mean reversion.
2. Momentum.
3. Breakout.
4. Trend following.
5. Reversal.
6. Range trading.
7. Volatility.
8. Session-based conditions.
9. Confluence.

Also support a bounded set of useful operators and templates for:

- Moving-average crossovers.
- Moving-average pullbacks.
- RSI threshold and momentum conditions.
- MACD-style momentum conditions.
- Bollinger Band conditions.
- ATR volatility and stop features.
- Donchian breakouts.
- Support and resistance rules.
- Price-channel conditions.
- Volatility compression and expansion.
- Higher-timeframe trend filters.
- Session restrictions.
- Weekday restrictions.
- Long-only and short-only modes.
- Combined directional confirmations.
- Market-regime filters.
- Simple Boolean rule combinations.

Do not implement every conceivable indicator immediately.

Begin with the smallest reusable primitive set.

Use bounded rule complexity.

Prevent internally contradictory expressions and invalid time dependencies.

Strategy parameters may include:

- Instrument research scope.
- Entry timeframe.
- Confirmation timeframes.
- Indicator types.
- Indicator parameters.
- Lookback periods.
- Thresholds.
- Direction.
- Session restrictions.
- Stop distance.
- Entry conditions.
- Exit conditions within the approved research contract.
- Bounded confirmation combinations.
- Strategy-family composition.

Implement:

- Deterministic candidate enumeration.
- Seeded random exploration.
- Parameter mutations.
- Bounded structural mutations.
- Controlled crossover.
- Adaptive search.
- Candidate novelty measurement.
- Diversity controls.
- Search coverage reporting.
- Durable search checkpoints.

Use a compact grammar.

Do not install a heavyweight genetic-programming framework merely to obtain simple mutations.

Introduce more sophisticated algorithms only when experiments demonstrate improved independently validated discovery per CPU-hour.

---

# 10. CANONICAL STRATEGY IDENTITIES

Implement permanent exact deduplication.

Distinguish two identities.

## Strategy concept identity

Canonical hash of effective strategy rules and parameters, independent of instrument-specific test outcomes.

Equivalent rule expressions should normalize to the same representation when semantic equivalence is proven.

Commutative operations should have canonical ordering.

Do not accidentally merge nonequivalent rules.

## Experiment identity

Hash of:

- Canonical strategy concept.
- Instrument.
- Dataset ID and content hash.
- Requested date range.
- Signal version.
- Indicator version.
- Cost and fill policies.
- Instrument profile.
- Execution engine version.
- Screening or validation protocol.
- Relevant code revision and configuration.
- Evaluation stage.

Identical completed experiments must be reused.

Never execute the same fully completed effective experiment twice unnecessarily.

A changed dataset, cost policy, engine version or instrument legitimately creates a different experiment.

Use durable unique constraints, transactionally recorded results and restart-safe leases.

A crash-interrupted experiment may be safely retried.

A cache hit must never increment the count of newly executed simulations.

Use efficient fingerprints such as BLAKE3.

A probabilistic Bloom filter may be used as a lookup accelerator, but must never cause a unique candidate to be silently discarded based only on a possible false positive.

Support compact completed search-space ranges for deterministic enumerations.

Preserve failed, rejected, invalid and inconclusive experiment history.

---

# 11. FAST TWO-STAGE RESEARCH PIPELINE

The production research pipeline must have only two backtesting stages.

Lightweight structural validation and duplicate checking happen before those stages but do not count as backtests.

Flow:

GENERATE  
→ CANONICALIZE  
→ DEDUPLICATE  
→ STRUCTURAL FILTER  
→ M1 CANDLE SCREEN  
→ EXACT TICK VERIFICATION  
→ INDEPENDENT VALIDATION  
→ QUALIFY / REJECT  
→ DIAGNOSE  
→ GENERATE NEW HYPOTHESES.

## Stage 1 — Extremely fast candle screening

Use M1 candle data derived from the actual quote stream.

Retain bid/ask OHLC and sufficient metadata to identify ambiguous trades.

Use:

- Shared indicator arrays.
- Cached signal primitives.
- Dense Boolean bitsets.
- Sparse compressed bitsets where advantageous.
- Batched parameter evaluations.
- Vectorized numerical operations.
- SIMD where proven useful.
- CPU cache-friendly arrays.
- Reusable intermediate expressions.
- Precompiled strategy rules.
- Bounded parallel workers.

The purpose is to reject unpromising candidates very cheaply without unnecessarily discarding genuinely promising strategies.

### The 50% promotion concept

Our desired promotion threshold is approximately 50% estimated win rate.

However, a strict M1 estimate may be wrong because an M1 candle does not reveal the ordering of its high and low.

Therefore implement:

1. An estimated candle result.
2. Conservative outcome bounds where possible.
3. Detection of ambiguous SL/TP ordering.
4. Explicit classification of unresolved entries/exits.
5. Optional promotion of uncertain candidates.
6. Periodic tick audits of a representative sample of candle-rejected candidates.
7. False-negative monitoring.
8. Versioned screening policies.

If a candle's high and low touch both SL and TP, do not guess which happened first.

If the screening process cannot establish a candidate's failure under a proven conservative bound, preserve its uncertainty and allow possible escalation to tick testing.

A strict estimated 50% gate may exist as an explicitly labeled experimental or selectable screening policy, but must never claim zero false negatives without proof.

**Default priority: maximize useful research throughput while controlling and measuring the risk of missing valid strategies.**

The screening engine must distinguish:

- PROVEN_INVALID
- DUPLICATE
- PROVEN_NO_SIGNALS
- PROVEN_SCREEN_FAILURE
- APPROXIMATE_SCREEN_FAILURE
- SCREEN_PROMOTED
- SCREEN_UNCERTAIN
- REQUIRES_TICK_VERIFICATION

Never label candle-estimated performance as historically verified tick performance.

## Stage 2 — Exact tick verification

Promoted candidates must be evaluated against actual ordered historical bid/ask events.

Use one authoritative Rust execution engine.

Required features:

- Long and short positions.
- Market orders.
- Stop orders.
- Stop-market orders.
- Limit orders.
- Entry and exit processing.
- Correct executable bid/ask prices.
- Exact price/tick grids.
- Spread-floor policy.
- Separate slippage policy.
- Deterministic event sequencing.
- Equal-timestamp ordering.
- Correct entry timing.
- SL/TP first-hit ordering.
- Order expiration.
- Order cancellations.
- 24-hour maximum holding period.
- Gaps and stale-quote handling.
- Partial fills when the available data and explicit model support them.
- Correct position state.
- Exact quantity and volume handling.
- Currency conversion.
- Margin and feasibility checks.
- Simulated equity accounting.
- Trade-level P&L.
- Drawdown.
- MFE and MAE.
- Maximum consecutive losses.
- Execution receipts and provenance.

Do not manufacture order-book depth from L1 quotes.

If historical liquidity or queue position is unknown, mark the fill model as an assumption.

Do not claim exact historical real-world fills from bid/ask quotes alone.

A versioned simplified order model is acceptable when appropriate, but must not silently misrepresent unsupported real-market behavior.

Prefer exact integer price representations and safe integer or decimal monetary arithmetic.

Test against independently computed reference examples.

---

# 12. TICK FIRST-HIT ACCELERATION

Implement a prototype exact tick first-hit index.

Its goal is to skip large ranges of historical quote events when it can mathematically establish that no relevant order or protective barrier was reached.

Research:

- Hierarchical range min/max summaries.
- Block metadata.
- Segment-tree approaches.
- Bid-side and ask-side crossing indexes.
- First-crossing search.
- Time-window bounds.
- Event-ID-based ordering.
- Session/gap flags.
- Sparse event indexing.
- Memory-mapped index access.
- Precomputed fixed-barrier acceleration.

Index queries must operate on the actual executable-side quote stream and the correct effective spread model.

Preserve:

- First eligible event.
- Original event sequence.
- Timestamp.
- Entry/exit price.
- Stop/target ordering.
- Fill eligibility.
- MFE/MAE.
- Deadlines and gaps.
- P&L.
- Complete trade accounting.

Implement two internal execution algorithms:

1. Indexed execution for proven supported cases.
2. Sequential quote replay as the authoritative fallback.

These are modes within ONE execution engine.

Run differential tests comparing indexed and sequential results.

Compare all fills, prices, timestamps, order states, position transitions, P&L, and resulting metrics.

On mismatch, fail the equivalence test and disable the indexed optimization for the affected execution class.

Never silently substitute an approximation.

If the index provides insufficient benefit, remove or disable it.

Do not maintain an expensive index simply because the technique is interesting.

---

# 13. COMPRESSION AND DATA ENGINEERING

Research and benchmark the following projects and methods:

- `apache/arrow-rs`
- `pcodec/pcodec`
- `quickwit-oss/bitpacking`
- `fast-pack/FastPFor-rs`
- `PSeitz/lz4_flex`
- `minio/minlz-rs`
- `RoaringBitmap/roaring-rs`
- StreamVByte-style integer encoding.
- Delta and delta-of-delta encoding.
- ZigZag integer encoding.
- Bitpacking.
- Block-indexed compression.
- Memory mapping.
- Zero-copy numeric representations.

Do not install every library automatically.

Build a small benchmark harness and compare them on representative genuine quote data.

## Cold storage

Use a reproducible, lossless historical storage format.

Parquet with ZSTD is the initial baseline.

Preserve immutable source archives and cryptographic hashes.

## Hot storage

Use compact, CPU-friendly native arrays.

Prefer Structure of Arrays, for example:

- Timestamp or timestamp delta.
- Fixed-point bid price.
- Exact spread or ask price.
- Event sequence.
- Source/quality flags.

Reconstruct prices losslessly.

Potential hot formats:

- Uncompressed native integer arrays.
- Delta + LZ4.
- Delta + SIMD bitpacking.
- FastPFor.
- Pco.
- MinLZ.
- A measured hybrid.

The winning format is whichever offers the fastest relevant end-to-end workloads under our memory/storage constraints.

Maximum compression ratio alone is not sufficient.

## Benchmark requirements

Measure:

- Original data size.
- Compressed size.
- Compression ratio.
- Encoding throughput.
- Sequential decoding throughput.
- Random-access latency.
- CPU cost.
- Peak RAM.
- Block/index overhead.
- Full M1 screening throughput.
- Full exact tick verification throughput.
- Storage reads and writes.

Test several block sizes, initially including 64 KiB, 256 KiB and 1 MiB.

Use real representative historical data, including different instruments and price characteristics.

Require exact event round-trip verification.

Do not lose timestamp ties, price precision, original sequence or source metadata.

Every hot cache must have a content-addressed identity and a rebuild path.

---

# 14. SHARED INDICATOR AND BITSET ENGINE

This is one of the highest-priority performance subsystems.

Implement reusable primitive computations.

Examples:

- Rolling sums.
- Rolling means.
- Rolling variance.
- EMA.
- RSI.
- ATR.
- High/low envelopes.
- Slope.
- Price differences.
- Volatility measures.
- Session masks.
- Momentum masks.
- Threshold comparisons.

Prefer incremental O(1)-update algorithms where valid.

Do not recompute an entire rolling window unnecessarily.

Study the open-source Wickra Rust project for incremental and streaming indicator approaches.

For every reusable computation, establish deterministic cache identities based on:

- Source data.
- Timeframe.
- Price semantics.
- Indicator parameters.
- Indicator algorithm version.
- Candle availability policy.
- Relevant numerical precision rules.

Cache signal masks and intermediate expressions.

Use dense bitsets for high-density conditions and sparse compressed sets when appropriate.

Benchmark both.

Use bitwise AND, OR and NOT operations for compatible Boolean expressions.

Use population-count operations for efficient signal counting.

Do not allocate a separate full historical indicator array for every generated strategy.

Factor common computations into a shared expression graph.

Keep bounded cache eviction.

Measure reuse efficiency.

---

# 15. PERFORMANCE AND THROUGHPUT

Our aspirational target is:

**At least one billion generated or evaluated strategy combinations per second wherever realistically achievable.**

This is a research ambition, not a verified or guaranteed full-backtest target.

A complete two-year backtest is much more expensive than generating a parameter combination or evaluating one vectorized Boolean expression.

Never confuse these measurements.

Expose separate counters for:

1. Raw parameter combinations considered.
2. Canonically unique strategy concepts generated.
3. Duplicates detected.
4. Invalid candidates rejected.
5. Shared signal operations evaluated.
6. Full M1-screened strategies.
7. M1-rejected strategies.
8. M1-promoted strategies.
9. Ambiguous candidates requiring escalation.
10. Completed indexed tick tests.
11. Completed sequential tick tests.
12. Full period-complete historical evaluations.
13. Reused cached experiment results.
14. Completed independent validations.
15. Independently qualified strategies.
16. Unique validated strategy concepts across all eight instruments.

Never inflate throughput by repeatedly counting cache hits.

Never use only zero-trade strategies in performance benchmarks.

## Performance methods to investigate

- SIMD integer processing.
- Dense/sparse bitsets.
- Shared expression graphs.
- Batch evaluation.
- Precomputed indicator arrays.
- Incremental indicators.
- Fixed-point representations.
- SoA memory layout.
- Reusable buffers.
- Allocation reduction.
- Branch simplification.
- Static dispatch in hot loops.
- Selective specialization.
- Memory-mapped arrays.
- Efficient compression codecs.
- Tick first-hit indexes.
- Data locality optimization.
- Work-stealing parallelism.
- CPU-aware task scheduling.
- Adaptive batch sizing.
- Profile-guided optimization.
- Link-time optimization.
- Safe architecture-specific CPU optimizations.

Do not introduce unsafe Rust purely for speculative speed.

Any unsafe optimization requires focused correctness tests, clear safety invariants and evidence that it materially improves throughput.

## Benchmark standards

Report:

- Exact CPU model.
- CPU feature set.
- Number of cores available.
- Cgroup CPU quota.
- Available RAM.
- Rust compiler version.
- Build configuration.
- Dataset hash.
- Candidate count and complexity.
- Number of actual trades.
- CPU time.
- Wall time.
- Memory.
- Disk I/O.
- Warm/cold cache conditions.
- Independent benchmark repetitions.

Prefer repeated measurements with median and dispersion.

Do not extrapolate a tiny synthetic benchmark into claims about full two-year historical tests.

The principal optimization objective is:

**Maximize independently useful, correctly evaluated candidate research per CPU-hour and per GB of memory.**

---

# 16. LIBRARIES AND EXISTING SOURCE CODE

Inspect the following open-source projects for reusable techniques.

Backtesting and execution:

- `KE-NETIZEN-OOPS/tickforge`
- `openquanter/openquanter`
- `citrusquant/citrusquant`
- `alphabench/raptorbt`
- `barter-rs/barter-rs`
- `jensnesten/rust_bt`
- `nkaz001/hftbacktest`
- `DaruFinance/quant-research-framework-rs`

Indicators:

- `wickra-lib/wickra`
- Other small, maintained Rust indicator crates.

Compression:

- Pco.
- bitpacking.
- FastPFor.
- lz4_flex.
- Roaring bitmaps.
- Other well-supported lossless Rust codecs.

Research automation:

- `karpathy/autoresearch`, for the experiment-loop concept.

Also examine useful open-source approaches from vectorized backtesting, compiled strategy languages, parameter optimization and scientific validation systems.

Avoid exhaustive low-value repository browsing.

Prioritize candidates by direct relevance, license compatibility, source quality, dependency weight, testing quality and measured performance.

Produce a source-reuse matrix containing:

- Project.
- License.
- Relevant component.
- Reason for considering it.
- Approximate complexity.
- Dependency impact.
- Whether code is actually open source.
- Integration or adaptation plan.
- Whether it was adopted or rejected.
- Actual measurements when performed.

Never copy proprietary code.

Do not treat a publicly downloadable binary as open-source implementation code.

Preserve all required licenses and attribution.

---

# 17. STATISTICAL VALIDATION

The enormous search space creates severe selection bias and overfitting risk.

Implement strong scientific safeguards.

Mandatory:

- Chronological training periods.
- Clearly separated out-of-sample data.
- Three-month walk-forward testing.
- Prior-only parameter fitting.
- Frozen candidate definitions.
- Purge/embargo where required.
- Independent evaluation.
- Protected final holdout.
- No future information leakage.
- No hidden repeated final-holdout optimization.
- Annual 2025 reporting.
- 2026 year-to-date reporting.
- Pooled reporting.
- Cost sensitivity.
- Parameter-neighborhood stability.
- Market-regime robustness.
- Session-specific analysis.
- Independent net expectancy.
- Drawdown analysis.
- Multiple-testing safeguards.
- Strategy diversity.
- Insufficient-sample warnings.
- Source/version comparability checks.

Do not repeatedly inspect the final holdout and then adapt strategies based on its results.

Preserve the distinction between training diagnosis and independent evaluation.

Possible status values:

- GENERATED
- SCREENED
- TICK_TESTED
- REJECTED
- INSUFFICIENT_EVIDENCE
- VALIDATION_FAILED
- VALIDATED
- DATA_BLOCKED
- PROFILE_BLOCKED

Use one coherent versioned state machine.

A strategy must never be promoted because its training performance looks unusually good.

All qualification claims must reference their independent validation evidence.

---

# 18. STRATEGY IMPROVEMENT SYSTEM

Implement a deterministic failure-analysis and improvement loop.

Analyze:

- Immediate stop-outs.
- Favorable movement before a loss.
- Nearly reached targets.
- Stop distance sensitivity.
- Volatility conditions.
- False breakouts.
- Entry timing.
- Confirmation effectiveness.
- Session behavior.
- Long versus short behavior.
- Year-by-year performance.
- Consecutive-loss patterns.
- Trades lost because of spread/slippage.
- Infeasible position sizes.
- Excessively sparse strategies.
- Excessively dense strategies.
- Unstable parameters.
- Inconsistent results across regimes.
- Training success followed by OOS failure.

Produce typed improvement hypotheses.

For example, diagnosis may propose testing a different confirmation or stop distance.

Every child strategy must have:

- Its own canonical identity.
- Parent references.
- Exact mutation description.
- Research hypothesis.
- Training evidence.
- New experiment identity.
- Independent confirmation status.

Preserve unsuccessful child attempts.

Do not erase the original parent.

Do not allow failure analysis on a protected final holdout to feed the generator.

Limit recursive improvement depth and CPU budgets.

---

# 19. CONTINUOUS AUTONOMOUS RESEARCH

The application must run an actual persistent research loop.

It must not depend on Codex or an LLM continuing to respond.

The loop:

1. Inspect available approved research data.
2. Find uncovered instrument/timeframe/family regions.
3. Allocate research budgets fairly.
4. Generate new candidates.
5. Normalize and deduplicate.
6. Run fast M1 screening.
7. Promote eligible candidates.
8. Execute exact tick verification.
9. Save immutable results.
10. Analyze failures.
11. Generate bounded improvements.
12. Submit eligible frozen candidates for independent validation.
13. Update qualified leaderboards.
14. Continue researching.

Use durable queues and checkpoints.

Implement:

- Pause.
- Resume.
- Stop after current safe checkpoint.
- Restart.
- Worker supervision.
- Lease expiration.
- Task retry.
- Duplicate execution prevention.
- CPU fairness.
- Candidate backlog replenishment.
- Search-region rotation.
- Disk and memory controls.
- Crash recovery.
- Clear error reporting.

If all currently registered regions are exhausted, generate additional eligible research regions instead of permanently idling.

Avoid endlessly regenerating invalid or duplicate candidates.

The loop must be fully deterministic when given the same fixed seed, inputs and execution versions.

An optional LLM hypothesis interface may be designed later, but must not be required for core operation.

---

# 20. TOP FIVE STRATEGIES

Maintain eight separate leaderboards.

One for each instrument.

Maximum five independently qualified strategies per leaderboard.

Maximum 40 positions across the eight instruments.

If only two candidates qualify, display two.

If none qualify, show an empty board with accurate reasons.

Never insert disqualified candidates simply to fill slots.

Rank using a versioned methodology that considers:

- Independent validation status.
- Verified net win rate.
- Complete trade count.
- Net expectancy.
- Actual simulated equity and return.
- Maximum drawdown.
- Loss streak.
- Year-by-year stability.
- Parameter stability.
- Market-regime robustness.
- Cost sensitivity.
- Strategy diversity.
- Evidence completeness.

Never rank by training win rate alone.

Allow filtering by instrument, timeframe, family, date range and validation status.

Retain an archive of previous leaders and rejected candidates.

---

# 21. LIGHTWEIGHT DASHBOARD

Build a complete, real, authenticated dashboard.

Use the smallest maintainable frontend architecture.

Preferred implementation:

- Rust `axum` HTTP API.
- Server-rendered HTML where practical.
- Minimal CSS.
- Minimal JavaScript.
- Embedded static assets.
- No mandatory separate Node.js frontend service in production.

Use a larger frontend framework only if actual product requirements justify it.

The dashboard must function on desktop and mobile.

Required sections:

## A. Overview

- Current factory status.
- Software verification status.
- Historical data readiness.
- Research loop state.
- Queue depth.
- Active worker count.
- CPU/RAM/storage usage.
- Latest successful experiment.
- Errors and blockers.

## B. Research performance

- Unique candidates generated per second.
- Duplicate rate.
- M1 screens per second.
- Tick tests per second.
- Completed full-period tests.
- Indexed versus sequential usage.
- Cache reuse.
- Actual research throughput.
- CPU time.
- Memory use.
- Compression and decoding statistics.
- Per-stage failure and promotion counts.

## C. Instrument leaderboards

Eight independent top-five boards.

Display:

- Rank.
- Strategy ID.
- Instrument.
- Timeframe.
- Family.
- Win rate.
- Annual trade counts.
- Max losing streak.
- Risk/reward.
- Net expectancy.
- Simulated ending equity.
- Drawdown.
- Validation status.
- Data cutoff.

## D. Strategy detail

- Full declarative specification.
- Signal conditions.
- Confirmations.
- Parameters.
- Stop distance.
- Entry and exit behavior.
- Trade list.
- Trade timestamps.
- Costs.
- MFE/MAE.
- Equity chart.
- Drawdown chart.
- Yearly statistics.
- Failure diagnostics.
- Parent-child lineage.
- Exact experiment hashes.
- Evidence exports.

## E. Data management

- Available instruments.
- Available months.
- Real versus synthetic datasets.
- Historical coverage.
- Source verification.
- Gaps.
- Quarantines.
- Import and refresh status.
- Compression statistics.
- Disk usage.
- Provenance reports.

## F. Validation laboratory

- Training periods.
- OOS periods.
- Walk-forward folds.
- Protected holdout status.
- Annual performance.
- Parameter stability.
- Cost sensitivity.
- Regime robustness.
- Statistical uncertainty.
- Qualification reasons.
- Rejected candidate evidence.

## G. Research controls

- Start.
- Pause.
- Resume.
- Worker configuration.
- Research budgets.
- Instrument inclusion/exclusion.
- Family inclusion/exclusion.
- Target profile.
- Read-only inspection of protected scientific settings.
- Task cancellations.
- Retry failed jobs.
- Controlled export.
- Backup and restore interfaces.

## H. Operations

- Health checks.
- Software version.
- Git commit.
- Active configuration hash.
- Process uptime.
- Job errors.
- Restart history.
- Backups.
- Restore verification.
- Disk pressure.
- Database status.

All panels must use actual backend/database state.

Do not display fabricated live data.

Do not label synthetic fixture results as genuine historical performance.

Implement full browser tests covering authentication, workflows, exports, desktop and mobile layouts.

Tests should exercise the actual Rust HTTP service and persistent backend, not just mocked frontend functions.

---

# 22. DATABASE AND PERSISTENCE

Prefer SQLite in WAL mode for metadata, queues, configurations and experiment records.

Use immutable files for large artifacts.

Avoid writing every generated candidate into SQLite before it becomes a meaningful unique experiment.

Use efficient batching where safe.

Implement:

- Versioned schema migrations.
- Exact uniqueness constraints.
- Idempotent task completion.
- Immutable result identities.
- Atomic artifact publication.
- Crash-safe state transitions.
- Consistent cache indexes.
- Database integrity checks.
- Backups.
- Verified restores.
- Transactional recovery.
- Data and experiment provenance.
- Bounded retention for disposable caches.

Never delete genuine scientific evidence just to reclaim cache space.

Preserve permanent rejected and failed experiment information in a compact form.

Do not allow dashboard clients to submit fabricated official performance metrics.

---

# 23. SECURITY

Implement:

- Local authenticated dashboard.
- Secure password handling.
- No plaintext passwords in the repository.
- Session security.
- CSRF protection for state-changing browser requests.
- API authorization.
- Reasonable request-size and rate limits.
- Restricted file paths.
- Input validation.
- No arbitrary shell execution through API inputs.
- No arbitrary generated-code execution.
- No secret logging.
- Safe dependency pinning.
- Supply-chain checks.
- Least-privilege runtime user.

Bind the API to localhost by default.

Do not automatically expose it publicly.

Document how to place it behind an authenticated HTTPS reverse proxy when explicitly authorized.

Never print GitHub tokens or other credentials.

No real-money broker execution or autonomous order placement.

---

# 24. TESTING AND CORRECTNESS

Create automated test suites from the beginning.

Mandatory categories:

## Data tests

- Exact timestamp parsing.
- Bid/ask preservation.
- Source event ordering.
- Duplicate handling.
- Equal timestamps.
- Crossed quotes.
- Source gaps.
- Trading sessions.
- Gold precision.
- JPY precision.
- Data compression round-trips.
- File corruption detection.
- Cache invalidation.
- Partial import recovery.
- Resumable acquisition.

## Signal tests

- Correct indicator arithmetic.
- Correct rolling warmup.
- No future data usage.
- Correct M1–D1 candle generation.
- Closed higher-timeframe confirmations.
- Sparse data.
- Session boundaries.
- Strategy canonicalization.
- Equivalent expression hashing.
- Different expression hashing.
- Feature cache reuse.

## Screening tests

- Exact and uncertain SL/TP outcomes.
- Ambiguous high/low ordering.
- Conservative promotion.
- Proven safe rejection cases.
- False-negative audit workflow.
- M1 screening version identities.
- Actual nonzero signal/trade cohorts.

## Execution tests

- Long entries.
- Short entries.
- Bid/ask executable sides.
- 2-pip spread floor.
- Wider historical spread retention.
- 2-pip adverse slippage.
- Limit-order exceptions.
- Stop-market fills.
- SL/TP collisions.
- Trade deadlines.
- Missing exit quotes.
- Gaps.
- Stale prices.
- Partial fills under explicit test models.
- Order cancellation.
- Position sizing.
- Minimum-lot rejection.
- $20 initial equity.
- 10% intended risk budgeting.
- 3R target.
- Gold contract handling.
- JPY and cross-currency conversion.
- Equity/P&L reconciliation.
- Drawdown.
- MFE/MAE.

## Tick-index tests

- Differential indexed versus sequential execution.
- Randomized eligible barrier queries.
- Timestamp ties.
- Direction changes.
- Outward price rounding.
- Spread and slippage interactions.
- Deadline boundaries.
- Unsupported-state fallback.
- Deterministic repeated runs.

## Qualification tests

- Exactly 50% win rate.
- Just below 50%.
- Break-even trades.
- Incomplete trades.
- Exactly 50 trades in 2025.
- Exactly 50 trades in 2026 YTD.
- Below 50 trades in either period.
- Exactly five consecutive losses.
- Six consecutive losses.
- Losing streak across year boundary.
- Infeasible position sizes.
- Account balance calculations.
- Annual and pooled reporting.
- Incompatible data and engine versions.

## Scientific tests

- Walk-forward boundaries.
- Prior-only parameter fitting.
- OOS contamination prevention.
- Holdout reservations.
- Holdout leakage prevention.
- Multiple-testing safeguards.
- Parameter-neighborhood analysis.
- Regime reporting.
- Validation rejection.
- Insufficient sample status.
- Correct qualified promotion.

## Operational tests

- Interrupted task recovery.
- Worker crash.
- Restart during database writes.
- Duplicate completion protection.
- Interrupted cache publication.
- Database migration.
- Backup.
- Restore.
- Full API workflows.
- Authentication.
- Dashboard controls.
- Browser exports.
- Mobile layout.
- Low-memory behavior.
- Disk-full behavior.
- Clean shutdown.
- Reboot recovery.

Use property-based testing where valuable.

Use deterministic golden fixtures with independently calculated reference results.

Maintain explicit negative tests.

Do not weaken a scientific assertion merely to make a test suite green.

---

# 25. END-TO-END ACCEPTANCE

Build a fully automated integration path:

IMPORT SAMPLE DATA  
→ VERIFY  
→ CREATE CANDLES  
→ GENERATE STRATEGIES  
→ DEDUPLICATE  
→ SCREEN M1  
→ VERIFY TICKS  
→ DIAGNOSE  
→ IMPROVE  
→ INDEPENDENTLY VALIDATE  
→ RANK  
→ DISPLAY IN DASHBOARD  
→ RESTART  
→ BACKUP  
→ RESTORE.

Run this on:

1. Synthetic deterministic fixtures.
2. Available representative real data from all eight instruments.
3. Cross-year synthetic fixtures exercising full calendar logic.
4. Adversarial source and execution fixtures.

The dashboard must display the actual persisted results produced by these tests.

The full application must work as an integrated product.

Passing separate unit tests is not enough.

---

# 26. HONEST RELEASE AND DATA STATES

Codex does not need the entire two-year tick dataset during development.

Therefore distinguish these states:

- SOFTWARE_INCOMPLETE
- SOFTWARE_IMPLEMENTED
- SOFTWARE_VERIFIED_ON_FIXTURES
- SOFTWARE_VERIFIED_ON_REPRESENTATIVE_REAL_DATA
- FULL_HISTORICAL_DATA_PENDING
- FULL_HISTORICAL_DATA_VERIFIED
- VPS_PERFORMANCE_PENDING
- VPS_OPERATION_VERIFIED
- FULL_RESEARCH_ACCEPTANCE_READY

A complete software implementation can be handed off while full historical data remains pending.

Do not confuse missing full historical data with missing software implementation.

However, do not declare full research readiness until genuine complete historical coverage, approved profiles, independent validation and VPS acceptance have passed.

Provide precise documentation explaining what is verified and what remains external.

Never fabricate qualifying strategies.

---

# 27. DEPLOYMENT

Build a reproducible Ubuntu deployment.

Provide:

- Rust toolchain requirements.
- Cargo.lock.
- Reproducible release builds.
- Configuration examples.
- Installation script.
- Data initialization commands.
- Database migration commands.
- Systemd service.
- Health checks.
- Start/stop/restart commands.
- Update script.
- Backup script.
- Restore script.
- Rollback procedure.
- Resource-limit configuration.
- Log rotation.
- Disk usage monitoring.
- Operator documentation.

The production research service must survive restart and reboot.

It must not require a connected Codex session.

Do not overwrite other system services.

Do not run with unnecessary root privileges.

Actual deployment to the target VPS must remain a separate authorized action unless the environment and permissions explicitly support it.

---

# 28. GITHUB AND PROJECT STRUCTURE

Create a small maintainable repository.

Suggested structure:

rust-strategy-factory/

- `Cargo.toml`
- `Cargo.lock`
- `README.md`
- `AGENTS.md`
- `CHANGELOG.md`
- `src/main.rs`
- `src/contracts.rs`
- `src/instruments.rs`
- `src/data.rs`
- `src/candles.rs`
- `src/indicators.rs`
- `src/strategies.rs`
- `src/generator.rs`
- `src/dedup.rs`
- `src/screen.rs`
- `src/tick_engine.rs`
- `src/tick_index.rs`
- `src/risk.rs`
- `src/accounting.rs`
- `src/diagnostics.rs`
- `src/validation.rs`
- `src/ranking.rs`
- `src/scheduler.rs`
- `src/storage.rs`
- `src/api.rs`
- `src/dashboard.rs`
- `web/`
- `tests/`
- `benches/`
- `migrations/`
- `config/`
- `scripts/`
- `deploy/`
- `docs/`
- `docs/MASTER_BLUEPRINT.md`
- `docs/ARCHITECTURE.md`
- `docs/ACCEPTANCE.md`
- `docs/IMPLEMENTATION_QUEUE.md`
- `docs/NEXT_ACTION.md`
- `docs/DECISIONS/`
- `docs/BENCHMARKS/`
- `docs/RUNBOOKS/`
- `docs/SOURCE_REUSE_MATRIX.md`
- `docs/DATA_MANIFESTS.md`
- `docs/PROJECT_STATE.json`

This structure is illustrative.

Merge modules where it simplifies the application.

Do not create empty files merely to match a planned layout.

Prefer one Cargo package initially.

Split into separate crates only when a measurable engineering benefit appears.

---

# 29. BUILD PHASES

Follow this dependency-aware order.

## Phase 0 — Initialization

- Inspect actual development environment.
- Verify GitHub access.
- Create independent repository.
- Save this directive.
- Create acceptance criteria.
- Create implementation queue.
- Establish CI.
- Establish formatting, linting, testing and benchmark commands.
- Record real hardware and toolchain information.

## Phase 1 — Core data and contracts

- Instrument specifications.
- Exact numeric representation.
- Price and monetary types.
- Historical sample importer.
- Source manifests.
- Data QA.
- M1 candle generation.
- Shared M1–D1 aggregation.
- Initial compression baseline.
- Correctness fixtures.

## Phase 2 — Fast screening

- Shared indicator primitives.
- Strategy grammar.
- Candidate canonicalization.
- Bitsets.
- Batch evaluation.
- Conservative candle screening.
- Representative performance benchmarks.

## Phase 3 — Sequential tick execution

- Exact ordered quotes.
- Spread.
- Slippage.
- Orders.
- Stops/targets.
- Sizing.
- Equity and P&L.
- Risk rules.
- Gold/JPY handling.
- Reference tests.

## Phase 4 — Acceleration

- Codec benchmarks.
- Native hot-data representation.
- Cache optimization.
- Tick first-hit index.
- Exact index/replay differential testing.
- Real performance comparisons.

## Phase 5 — Strategy generation and research

- Nine strategy families.
- Extended useful templates.
- Parameter generation.
- Bounded mutations.
- Adaptive search.
- Durable deduplication.
- Persistent scheduler.
- CPU fairness.
- Automatic region expansion.

## Phase 6 — Diagnostics and science

- Complete trade diagnostics.
- Automatic bounded improvements.
- Parent-child history.
- Walk-forward validation.
- OOS evaluation.
- Holdout safeguards.
- Multiple-testing controls.
- Qualification rules.
- Eight independent top-five boards.

## Phase 7 — Dashboard and integration

- Real authenticated API.
- Functional dashboard.
- Connected controls.
- Charts.
- Data operations.
- Strategy details.
- Validation laboratory.
- Error handling.
- Browser acceptance.

## Phase 8 — Deployment and reliability

- Installer.
- Systemd.
- Resource limits.
- Backups.
- Restore.
- Updates.
- Rollback.
- Restart recovery.
- Security checks.
- Stable packaging.

## Phase 9 — Comprehensive software acceptance

- Verify every mandatory software requirement.
- Run all reproducible tests.
- Run representative real-data integration.
- Run meaningful performance benchmarks.
- Run stability and recovery tests.
- Run desktop and mobile browser acceptance.
- Validate evidence artifacts.
- Document remaining full-data/VPS-only requirements.
- Publish verified code and handoff instructions.

Do not stop at the end of an individual phase.

Reinspect the complete unfinished queue and immediately select the next implementable task.

---

# 30. CONTINUOUS CODEX DEVELOPMENT LOOP

This instruction is critical.

You must behave as an autonomous implementation agent, not a single-task assistant that stops after making one file or completing one milestone.

During every available execution session, repeat:

1. INSPECT the current exact repository state.
2. READ the master blueprint and unfinished acceptance requirements.
3. IDENTIFY the highest-priority implementable task.
4. CHECK whether the needed functionality already exists.
5. PLAN the smallest correct change.
6. IMPLEMENT working code.
7. TEST the affected subsystem.
8. FIX defects.
9. RUN relevant integration tests.
10. BENCHMARK performance-sensitive modifications.
11. REVERT or repair unacceptable regressions.
12. UPDATE durable state and documentation.
13. COMMIT the verified work.
14. PUSH to GitHub.
15. VERIFY the remote commit and available CI results.
16. REINSPECT the complete backlog.
17. BEGIN the next task.

Repeat until all implementable software requirements are genuinely complete and verified.

Do not stop just because:

- One milestone passed.
- One test suite passed.
- A pull request was created.
- A dashboard renders.
- One engine function works.
- One instrument works.
- A benchmark is impressive.
- The repository compiles.
- A feature has been described in documentation.
- A partial report was written.

A milestone is a checkpoint, not completion.

If actual session, inference, tool, compute or permission limits prevent further work, preserve a clean durable checkpoint and exact next action.

Do not claim that Codex will continue executing after the session ends unless a real supported execution mechanism exists.

Never bypass resource limits or safety restrictions.

---

# 31. MINIMIZE TOKENS, CALLS AND REPETITIVE WORK

Complete the entire project using the minimum practical amount of reasoning tokens, tool calls and repetitive manual effort.

However, never reduce necessary correctness checks or silently skip acceptance tests just to save tokens.

Apply these techniques:

## A. Reuse available instructions and skills

Inspect applicable existing coding, Rust, testing, benchmarking, security and repository-development skills in the execution environment.

Use relevant skills when available.

Do not reinstall an existing tool merely because a new session started.

When a repeated development workflow appears, create a concise repository-local reusable instruction or script.

For example:

- Rust verification workflow.
- Benchmark workflow.
- Schema migration workflow.
- GitHub publication workflow.
- Dataset integrity workflow.
- Dashboard browser-test workflow.
- Acceptance reconciliation workflow.

Prefer executable scripts for deterministic repeated tasks.

Do not create redundant skills.

Do not edit global agent configuration unless necessary and authorized.

## B. Maintain concise durable context

Use:

- `AGENTS.md` for stable engineering rules.
- `docs/MASTER_BLUEPRINT.md` for this directive.
- `docs/IMPLEMENTATION_QUEUE.md` for unfinished work.
- `docs/NEXT_ACTION.md` for immediate continuation.
- `docs/PROJECT_STATE.json` for machine-readable progress.
- `docs/ACCEPTANCE.md` for actual evidence.

At the beginning of a new session, read these files and only the source needed for the current task.

Do not repeatedly reread the entire repository when a targeted inspection suffices.

## C. Batch independent operations

Combine related source inspections and tests when useful.

Use parallel tool calls for genuinely independent work when the environment supports it.

Avoid spawning competing builds or tests against the same output directory.

Do not run redundant full test suites after every tiny edit.

Use focused tests first, broader regression checks when relevant, and complete verification at major integration boundaries.

## D. Avoid useless work

Do not:

- Rewrite completed code unnecessarily.
- Recreate identical documentation.
- Repeat expensive benchmarks without reason.
- Retest unchanged experiments.
- Download previously verified data again.
- Create parallel implementations of the same feature.
- Generate excessive verbose logs.
- Perform repeated searches for already established facts.
- Add dependencies without justification.
- Optimize paths not shown to be bottlenecks.

Preserve all meaningful evidence despite concise reporting.

## E. Learn from failures

When a bug or recurring workflow failure appears:

1. Determine the root cause.
2. Fix it.
3. Add a regression test.
4. Record the minimal useful lesson.
5. Reuse the lesson in future similar tasks.

Do not repeatedly encounter the same preventable problem.

---

# 32. PUBLICATION REQUIREMENTS

Use GitHub as the durable source of development truth.

Every verified significant milestone must be committed and pushed.

After pushing:

- Confirm the exact remote SHA.
- Verify available CI results.
- Record tests actually executed.
- Record benchmarks actually executed.
- Record changed requirements.
- Record known failures.
- Record next unfinished work.

Do not claim a push succeeded without confirmation.

Do not force-push over unrelated work.

Do not merge protected branches or create a release tag without the necessary checks and authorization.

Do not push raw tick archives, credentials, large caches, private state or unlicensed source material.

Use appropriate `.gitignore` rules from the beginning.

If publication is blocked, continue local development and retain the precise blocker.

---

# 33. MANDATORY QUALITY GATES

The project is not software-complete until:

- All eight instrument contracts exist.
- All seven timeframes work.
- Representative actual data can be imported and verified.
- Data compression is lossless.
- Indicators are causal and reusable.
- Strategy generation produces valid distinct candidates.
- Canonical deduplication works.
- M1 screening works and reports uncertainty.
- Tick execution correctly implements the financial research contract.
- Spread and slippage are applied correctly.
- Gold and JPY requirements are handled honestly.
- Indexed execution matches sequential replay for supported cases.
- Benchmarks are reproducible.
- Research persists across interruptions.
- Diagnostics and strategy improvement work.
- Scientific validation and holdout protections work.
- Qualification correctly enforces the research rules.
- All eight leaderboards function.
- The dashboard uses real persisted state.
- Browser acceptance passes.
- Restart and crash recovery pass.
- Backup and restore work.
- Installation and update procedures exist and are tested where possible.
- Security checks pass.
- Relevant software acceptance tests pass.
- GitHub contains verified code and documentation.
- No essential implementation remains a mock.

Requirements involving complete historical data and actual target-VPS operations must remain explicitly pending until those external conditions are satisfied.

Do not call externally blocked work completed.

Do not treat a high unit-test count as proof of product readiness.

---

# 34. REQUIRED FINAL DELIVERABLES

Deliver:

1. Complete Rust source code.
2. Independent GitHub repository.
3. Pinned Cargo dependencies.
4. Working strategy generator.
5. Working fast M1 screening system.
6. Working exact tick execution engine.
7. Verified acceleration indexes where justified.
8. Lossless data pipeline.
9. Functional historical importer.
10. Resumable future full-data acquisition system.
11. Exact experiment registry and deduplication.
12. Persistent research scheduler.
13. Complete risk and accounting engine.
14. Failure diagnostics and improvement system.
15. Scientific validation.
16. Eight independent qualification boards.
17. Fully connected authenticated dashboard.
18. Reproducible tests.
19. Real performance benchmarks.
20. Source reuse/license audit.
21. Installation and update scripts.
22. Systemd service definition.
23. Backup and restore.
24. Security checks.
25. Operator guide.
26. Full-data VPS handoff.
27. Accurate implementation and readiness report.
28. Verified GitHub publication.

The release must be reproducible from its declared source revision.

The full-data handoff must explain how to install the software on the VPS, import existing history, obtain only missing historical ranges, approve sources/profiles, run full verification, configure research and start the durable scheduler.

---

# 35. START EXECUTING NOW

Do not reply with a blueprint.

Do not ask the owner to restate requirements already given.

Do not wait for confirmation on ordinary reversible implementation choices.

Begin by:

1. Checking the working environment and repository state.
2. Creating the new independent Git repository.
3. Saving this master directive.
4. Establishing the acceptance ledger and implementation queue.
5. Creating the minimal compiling Rust application.
6. Creating the continuous integration workflow.
7. Implementing exact instrument and numerical contracts.
8. Implementing representative historical data ingestion and deterministic fixtures.
9. Building the fastest correct shared M1 screening foundation.
10. Beginning the exact Rust tick-execution implementation.

Continue through the remaining phases.

After every meaningful verified milestone, commit, push, verify and proceed to the next unfinished requirement.

**The desired result is not a prototype. It is a lightweight, exceptionally fast, scientifically reliable, fully working Rust research factory, engineered specifically around our eight instruments and our trading-research rules.**

Never sacrifice scientific correctness to produce impressive benchmark numbers.

Never invent historical market results.

Never mistake generated combinations for completed backtests.

Never treat unfinished software as complete.

Keep the application small, keep the data exact, keep the research repeatable, and continue until the software implementation is genuinely finished within the available execution environment.

**BEGIN IMPLEMENTATION.**