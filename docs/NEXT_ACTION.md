# Next action

Status: SOFTWARE_INCOMPLETE. Use the existing checkout. Read AGENTS.md, PROJECT_STATE.json, IMPLEMENTATION_QUEUE.md and relevant MASTER_BLUEPRINT.md requirements. After every verified milestone update evidence/state/this file, commit, push, check remote/hosted CI, reread this file from disk and continue.

## Latest verified local milestone

242 Rust tests/fmt/Clippy, release build, both actual browser suites and local recovery pass for the shared authoritative account transition and synthetic USD corpus replay. Whole receipts match materialized market/stop/limit references with nonzero trades; serialized account state survives batch-boundary round trips. No real/protected or non-USD corpus execution is enabled. Hosted prior run43602831136 is Success for6a34245; new account milestone CI follows publication. Real quotes/approved profiles/qualified strategies remain zero.

## Implement next

Shared visit_chunks now provides the verified traversal boundary used by corpus inspection; its stateful candle/refusal test passes. Bounded candle_context/corpus-candles now provide actual candle arrays for feature preparation without quote concatenation. Corpus signals now compile primary and higher-timeframe rules using verified single-pass multi-frame contexts and the existing causal feature engine. Windowed corpus compilation now preserves warmup and excludes end-boundary publication before feature arithmetic. Next specific task: persist and validate source-bound account checkpoints with registry fencing, restart recovery and atomic result publication. The single authoritative AccountState event transition now drives ordinary replay and bounded synthetic USD corpus replay; complete receipt and serialized state parity tests pass. Real/protected corpus execution and non-USD conversions remain refused until provenance/exposure/conversion integration is complete. Benchmark before claiming production performance. Continue the full queue after this recovery milestone.


Continuation source findings: `tick_engine::prepare_replay` currently requires a materialized Dataset and per-event conversion slice; `replay_internal` holds balance/marked peak/drawdown, position, pending order, candle cursor and prior-event timing in one loop. Preserve one authoritative event transition when extracting checkpoint state. `strategies::Features::with_context` and Higher rules still rebuild candles from Dataset context; streaming must preserve prior-only higher-frame publication and warmup. Test complete receipts, pending risk reservations, gaps/deadlines, conversions, pause/restart and corrupted source/checkpoint refusal before enabling this path. The compute identity now explicitly binds data/evidence/corpus source modules.


Keep indexed policy experimental. Profile dominant complete-pipeline stages and validate varied/representative workloads before any promotion; engine-only gains or a small noisy median are insufficient. Implement checkpoint-aware shared-index/batch lifecycle, aggregate reference-audit coverage monitoring; metadata lookup scaling now has component evidence. Continue explicit source revision/correction links, catalog approval/coverage lifecycle, import/backup activation dashboard workflows and coordinated multi-component recovery; bounded immutable inventory and continuous export now exist. Preserve declared class versus independent approval and source availability. Continue remaining lifecycle, and the remaining scientific/operations queue rather than treating the performance experiment as software completion. Preserve exact eligibility/fallback, source scope, immutable evidence and reference/proof audit independence.

Continue the full queue: indexed blocks/direct arrays/streaming and justified codecs, resumable sources/imports/coverage, dependence-aware prespecified qualification/evidence gates and real diverse boards, bounded multiworkers/supervision/resources, remaining real dashboard/operations/security workflows and integrated acceptance. Use IMPLEMENTATION_QUEUE.md's source/test reconciliation; do not confuse implemented code with acceptance.

## External acceptance

Fresh Dukascopy/HistData info requests returned403; no real quotes/terms obtained. Proposed source domains could not be saved because the original setup draft is not editable; changes are not applied. Preserve existing settings when transferring the additive proposal. Do not retry without a meaningful access change. Small legally usable2025/2026 samples, approved broker/calendar/conversion facts and representative/physical-cold measurements remain pending; full backfill and target-VPS authorization/operation are separate later steps. Do not touch Hermes or change public repository visibility.

## Invariants

Exact executable bid/ask,2-pip spread floor plus separate2-pip adverse market/stop slippage,10% intended risk/$20 initial/3R/24h; frozen pending distance/quantity; no fabricated gaps/fills/depth; no protected OOS/final adaptation; synthetic never qualifies. Qualification needs independent approved complete evidence, not a high test count or tiny benchmark. Registry-local protection cannot cover external tools/copied registries. No execution continues after a session ends unless an actual supported mechanism exists.

Latest continuation: persisted machine bindings and scoped independent authority are implemented; no genuine quotes or broker approvals exist. Alternative source findings and exact blockers are in REAL_DATA_MATRIX.json. Zero acquired bytes; no accounts/emails/purchases. Full historical qualification and target-VPS acceptance remain pending.
