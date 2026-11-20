# Owner-selected 22-month research window

Owner decision (2026-11-19, final for the current research configuration): the active historical
research period is exactly the 22 completed months 2025-01-01 through 2026-10-31 — the full
structurally verified canonical corpus (8 instruments × 22 months = 176 partitions,
421,078,937 events). The window is an explicit owner-selected calendar pin, not a default and not
a sliding "latest N months" selection. No download or backfill is performed to satisfy the older
24-completed-month blueprint wording; 2026-11 and 2026-12 are not completed/available.

## Software semantics

- `historical_corpus::ResearchWindow` is an inclusive calendar window
  (`start_year/start_month/end_year/end_month`, serde JSON, unknown fields refused).
  `ResearchWindow::OWNER_SELECTED` is the pinned active period; `EXPECTED_MONTHS` is derived
  from it (22), so the count and the calendar boundaries cannot drift apart.
- `historical_corpus::register_research_window` resolves exactly the window's months from the
  canonical plan. Every required month must exist as a completed verified partition; missing
  months are refused with the exact list (for example `missing: 2026-11, 2026-12`). Canonical
  partitions outside the window are not selected, so later completed months never slide the pin.
  No padding, silent truncation or fabrication.
- `historical_corpus::register_window` (latest N contiguous completed months, PR #19) is retained
  as the configurable completed-month mechanism; it refuses N greater than the available
  completed months with the exact shortfall.
- `historical_corpus::verify` requires a full-length specification to cover exactly the
  owner-selected window (boundaries, not merely the count), in addition to the existing
  identity/receipt/chunk checks.
- The full-corpus research CLIs (`historical-corpus-register`, `historical-corpus-study`,
  `historical-corpus-execute`, `historical-corpus-execute-cached`, `historical-corpus-audit`)
  register through the pin. New CLIs: `historical-corpus-register-research-window` (the pin) and
  `historical-corpus-register-window-config` (an explicit window JSON). See
  RUNBOOKS/research-window.md.

## What does not change

- The corpus specification format, aggregate identity formula and every existing receipt are
  unchanged: on the current corpus the pin binds exactly the same 22 monthly sources as the
  exact-count registration (asserted by identity equality in the real-data tests). No completed
  result is reinterpreted.
- `REAL_STRUCTURALLY_VALID` remains the maximum automatic class. The pin grants no calendar
  completeness, broker/profile approval, source licensing, research eligibility or qualification;
  all four review gates stay pending and 0 instruments are research-eligible.
- Diagnostic execution stays labelled `diagnostic_simulation`/`none` and can never feed registered
  research or scientific gates. Non-USD-quote instruments remain refused until approved conversion
  semantics exist.
- Financial contracts are untouched ($20 initial equity, 10% intended risk, 3:1 planned RR, 24 h
  maximum hold, minimum 2-pip effective spread or observed-if-wider, separate adverse 2-pip
  slippage, exact bid/ask; $0-commission hypothetical research scenario).

## Verification

Unit tests bind the pin's arithmetic, calendar validation, JSON roundtrip and the `verify`
boundary gate. Real-data tests assert the exact 2025-01..2026-10 boundaries for all eight
instruments (one-month diagnostic sample per instrument inside `cargo test`), identity equality
with the exact-count registration, and exact refusals for windows demanding 2024-12, 2026-11 or
2026-12. The full ladder (fmt, clippy `-D warnings`, full test suite, `scripts/check.sh`) and
hosted CI on the published SHA are recorded in the branch checkpoint and the kanban handoff.