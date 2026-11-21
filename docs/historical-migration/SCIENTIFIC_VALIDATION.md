# Scientific validation over the 22-month corpus — diagnostic evidence

Status: **diagnostic simulation only.** Classification `diagnostic_simulation`, eligibility
`none`, `qualification_granted: false`. **Zero instruments qualify.** No strategy is qualified,
no eligibility is granted, and no scientific gate was fed. This document records an automated,
fail-closed numerical validation run and its evidence.

## What was run

The owner-approved numerical criteria were evaluated per instrument over the refreshed
diagnostic full-corpus execution of the verified 22-month window (2025-01..2026-10):

1. `historical_corpus` diagnostic execution (`ReplayMode::Diagnostic`, the registered gate
   unchanged) over all eight instruments, month by month, through the unchanged authoritative
   sequential engine and frozen financial rules.
2. Per-month closed-trade outcome statistics (wins / losses / breakevens, losing streak, gross
   win / gross loss, net) derived from the same trade list the engine already produced — a
   diagnostic-only addition that changes no financial rule.
3. `scientific_validation::summarize_all` (`scientific-validation` CLI) over the eight cold
   reports, applying the owner floors/ceilings with explicit `pass` / `fail` / `unknown` states.

The engine freezes the 1:3 planned reward/risk; the achieved gross win/loss ratio is reported
descriptively and is never substituted for the planned contract.

## Result

All eight instruments execute all 22 months (176 executed, 0 refused). Aggregate: 21,104 closed
trades, 1,094,379 rejections, gross win 1,984,052,830 uUSD, gross loss 5,061,356,071 uUSD,
closed-trade net -3,077,303,241 uUSD, additive ending equity -2,917,229,124 uUSD, worst
per-month drawdown 47,783,390 uUSD.

| Instrument | Months | Resolved | Wins | Losses | Breakeven | Net win rate | Max streak | Achieved RR | Additive ending (uUSD) | Verdict |
|---|---|---|---|---|---|---|---|---|---|---|
| EURUSD | 22 | 3060 | 480 | 2580 | 0 | 15.68% | 26 | 0.478 | -419523130 | REJECTED |
| GBPUSD | 22 | 3341 | 481 | 2860 | 0 | 14.39% | 33 | 0.423 | -419714330 | REJECTED |
| USDJPY | 22 | 4089 | 614 | 3475 | 0 | 15.01% | 38 | 0.386 | -419374632 | REJECTED |
| USDCHF | 22 | 2345 | 277 | 2068 | 0 | 11.81% | 51 | 0.328 | -419614860 | REJECTED |
| USDCAD | 22 | 3086 | 413 | 2672 | 1 | 13.38% | 34 | 0.384 | -418862456 | REJECTED |
| AUDUSD | 22 | 2681 | 411 | 2269 | 1 | 15.33% | 37 | 0.389 | -419409720 | REJECTED |
| NZDUSD | 22 | 2492 | 339 | 2152 | 1 | 13.60% | 37 | 0.323 | -419559400 | REJECTED |
| XAUUSD | 22 | 10 | 1 | 9 | 0 | 10.00% | 6 | 0.089 | 18829404 | REJECTED |

Per instrument the numerical criteria fail on net win rate (floor 50.00%), consecutive losses
(ceiling 5) and the additive equity objective; the declared class is diagnostic, so the verdict
is `REJECTED` for all eight. `continuous_full_period_accounting` and `independent_evidence`
remain `unknown` for every instrument.

## Why this cannot qualify anything

- **Diagnostic class.** The corpus is `RealStructurallyValid`; the diagnostic bridge exists so
  it can be exercised end-to-end. Its output is labelled `diagnostic_simulation` / `none` and is
  never research evidence.
- **Reset-fold, not continuous.** Each diagnostic month starts a fresh `INITIAL_EQUITY`
  account. The owner criteria require a continuous single-account full-period replay; a
  per-month reset cannot establish that, so the streak and equity figures above are descriptive
  only. This is a real gap, not a pass.
- **No independent evidence.** Calendar/profile/legal/scientific approvals and corpus-wide
  coverage remain pending. Numerical criteria alone never qualify.
- **Conversion is a research convention.** USD-quoted instruments use the identity conversion;
  USD-base non-USD-quote instruments use the explicit direction-based reciprocal diagnostic
  closing rate, which is an unverified research assumption, not an approved broker rule.

## Change versus the previously committed receipt

The committed `DIAGNOSTIC_EXECUTION_CACHED_RECEIPT.json` predated the reciprocal USD-account
conversion and recorded 5 executed / 3 refused instruments (110 executed months, 66 refused,
11,584 trades). The refreshed run records 8 executed / 0 refused (176 / 0, 21,104 trades) with
every instrument's cold and warm reports byte-identical apart from cache counters. The earlier
receipt was stale evidence, not a contradiction: the engine change widened the diagnostic scope
without changing the registered gate.

## Evidence

- `docs/historical-migration/SCIENTIFIC_VALIDATION_RECEIPT.json` — provenance-bound validation
  receipt, sha256 `d16ff988a75ebcfdf6fabaf455d3232cdcbd05ad7c587d8f75f4aa193734e9b8`.
- `docs/historical-migration/DIAGNOSTIC_EXECUTION_CACHED_RECEIPT.json` — refreshed cached
  diagnostic execution receipt, sha256
  `e5da021ac77d0cb599258a4c36823b117bcdcba9c12a4906cef123ee95ae71fe`.
- `docs/historical-migration/diagnostic-cache-v1/*.cold.json` / `*.warm.json` — per-instrument
  month reports, now carrying `outcomes`.
- `scripts/run-scientific-validation.py` — driver (diagnostic execution + criteria evaluation).
- Source `f9c2639903811ba15f5cf9895c5920029b0bd1a0` (branch `hermes/scientific-validation`, working
  tree carrying this card's changes), binary sha256
  `a7f3411f5719fc18c61a4ef5a64e3c8ead33714daed2356800f592f05071403c`.

## Checks that back the evaluator

- `tests/scientific_validation.rs` — synthetic fixtures only: win-rate / streak / RR / per-year
  arithmetic, fail-closed `REJECTED` vs `INSUFFICIENT_EVIDENCE`, `qualification_granted` always
  false, totals/months reconciliation refusal, out-of-range year refusal.
- `src/historical_corpus.rs` unit tests — outcome-statistic counting, streak semantics
  (a net winner resets, a breakeven does not, matching `validation::assess`), overflow refusal,
  additive aggregation.
- `tests/historical_pipeline_real.rs` — real-corpus assertions that per-month outcome counts
  reconcile with resolved trades (skips cleanly when the corpus is absent).

## Next actions (implementable, in priority order)

1. A diagnostic **continuous full-period single-account** mode so win rate, losing streak and
   equity can be observed on one compounded 22-month account rather than reset-fold months. This
   is the missing piece before the owner criteria can even be described on the real corpus.
2. Independent calendar/profile/legal/scientific evidence and corpus-wide coverage — external
   owner determinations, not software.
3. Real registered execution remains blocked by the input-class gate; widening it requires
   owner authorization and approved broker/calendar facts.
