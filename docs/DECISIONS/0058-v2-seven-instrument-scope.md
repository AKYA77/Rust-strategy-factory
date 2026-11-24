# 0058 — V2 mandate: seven active FX instruments, gold withdrawn, false stage rotation removed

Status: IMPLEMENTED (scope/contracts/replay); data removal and full V2 acceptance are tracked
separately. Date: 2026-11-24. Branch: `hermes/v2-seven-instrument-scope`.

## Requirement

The 2026-11-24 handover PDF supersedes the 2026-11-21 owner decision set where they conflict.
It defines the active grammar as exactly `EURUSD GBPUSD AUDUSD NZDUSD USDJPY USDCAD USDCHF`
and makes XAUUSD a forbidden instrument, withdraws the 2026-11-21 market-calendar approval,
removes all false/prior-stage telemetry and restores correct causal ORB simulation.

## Design decisions

1. **Active profile set.** `Profile::SYMBOLS` is the single seven-symbol grammar; XAUUSD is
   absent. `Profile::synthetic("XAUUSD")` and every research/config submission naming gold
   fail closed (`gold_submission_rejected`), matching the prior literal-USD gate rather than a
   generic error so gold submissions are identifiable in receipts.
2. **Gold source data is out of scope for reading or execution.** `HistoricalCorpus::expected_instruments`
   returns the seven FX symbols; the corpus/search/calendar/dashboard/browser paths operate on
   those only. Project-owned XAUUSD-only source/native/candle/index/cache/export files are
   deleted once no corpus job is active; immutable hash/manifest/study/audit receipts remain,
   marked superseded rather than rewritten.
3. **Calendar policy (owner-approved 2026-11-21) is WITHDRAWN.** Session logic no longer reads
   the `economic_calendar::approved()` gate. Execution applies the profile session week/hour
   mask directly; the remaining addendum checks (UTC-normalized open/close alignment,
   holiday/DST-aware next-open) stay fail-closed. The profile matrix and every new fixed-window
   family must carry documented UTC ranges.
4. **Prior-stage/false stage telemetry removed.** `research::{record_prior_stage_advance, advance_stage}`
   and their persistent `current_stage` plumbing were deleted. Exhaustion without an un-promoted
   candidate is terminal; exactly one deterministic child per submitted experiment survives.
   Resume verifies required children by exact ID before use. Durable, crash-safe stage transition
   records and progress/state telemetry are NOT yet implemented and are tracked as remaining work.
5. **ORB order-execution order bug fixed.** `tick_engine::Replay` now creates orders strictly in
   each signal candle's UTC publishing order (only completed candles are published), and order
   processing has a strict all-close phase then a strict all-entry phase. Regression tests
   `tests/orb_causality.rs` bound causality (first signal's stop cannot precede its entry),
   simultaneous outcome precedence (stop -> adverse slippage -> target), multiple same-tick
   signals resolved in signal order with fair-queue queueing, and fill reuse of the same tick
   (which may not post a position at its own entry price).
6. **Conversion identity sentinel is quote-conditioned.** `Conversion::validate` accepts the
   identity sentinel only for USD-quoted profiles. A non-USD quote must carry an event-aligned
   observation within the configured age bound; `diagnostic_conversion` returns the reciprocal
   convention with the actual tick time instead of the sentinel. This closed the P0 finding that
   the USD identity path bypassed event-alignment for non-USD quotes.

## Evidence (verified)

- `scripts/check.sh` on the seven-instrument tree: fmt clean, Clippy `-D warnings`, unit tests
  260 passed / 0 failed, integration suites green including the real-corpus executor (7/7
  instruments execute, 4 identity / 3 reciprocal).
- `tests/orb_causality.rs` (new) passes; `tests/families.rs` now contains a dedicated causal ORB
  fixture (completed range, explicit window high/low and breakout) that asserts at least one
  signal and a closed trade, replacing the old silent skip of `Family::Orb`.
- Repo-wide XAUUSD references are confined to immutable documentation/audit receipts.

## Explicitly NOT granted

No production readiness. XAUUSD removal does not grant source licensing, calendar completeness,
broker/session/profile approval or scientific eligibility. The local corpus remains
REAL_STRUCTURALLY_VALID only; the active campaign continues to run on the owner-selected
20-month window with Sep/Oct 2026 as the untouched OOS holdout. No strategy has yet reached the
scientific qualification trade threshold.
