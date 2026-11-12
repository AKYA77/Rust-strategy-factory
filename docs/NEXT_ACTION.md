# Next action

Status: SOFTWARE_INCOMPLETE. Read AGENTS.md, PROJECT_STATE.json, ACCEPTANCE.md and the complete IMPLEMENTATION_QUEUE.md. Continue implementation after every milestone; this file is a checkpoint, not completion.

## Verified milestone: pending strategy entry integration

47 application tests pass, plus formatting and Clippy. Typed pending stop-market/limit entry plans now execute through replay with placement-fixed quantity, exclusive reservations, pre-fill current risk/marked-margin checks, cancellation, expiry and receipts. Same-event quote processing precedes newly decided cancellations. Initial sizing now includes entry mark losses and commissions. Engine version is ordered-l1-v2-marked-margin. Legacy market concept hashes stay stable. See DECISIONS/0001-pending-entry-reservations.md.

Synthetic eight-instrument benchmark rerun: 6,997 resolved trades per batch, five repetitions for each of 1/2/3/4 workers. Artifacts are BENCHMARKS/2026-11-12-reservation-final-*.json. No real-data or target-VPS speed claim.

## Begin next

Implement all nine family templates and the shared causal primitives they require (RSI, prior Donchian/channel edges, mean crosses/trends, volatility expansion, UTC session/weekday masks). Preserve effective-concept deduplication, bounded complexity/cache budgets and legacy identities. Test prefix causality and genuinely nonzero cohorts. Then implement conservative candle outcomes/uncertainty, a bounded durable scheduler, complete independent validation and the authenticated dashboard. Reinspect the full queue rather than stopping at this next milestone.

External requirements remain: representative real quotes, reviewed broker/calendar profiles, full historical data and target VPS validation. Running network policy still excludes drafted provider/API domains, so hosted CI and source terms remain unverified. No qualified strategies, autonomous scheduler or web service exists yet. No process continues research after this session unless a real scheduler is implemented and started.
