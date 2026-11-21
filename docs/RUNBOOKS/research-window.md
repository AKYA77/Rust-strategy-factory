# Research window configuration

The active historical research period is the owner-selected pin
`historical_corpus::ResearchWindow::OWNER_SELECTED` = **2025-01..2026-10** (exactly 22 completed
months; owner decision 2026-11-19, see DECISIONS/0056-owner-selected-research-window.md). It is
an explicit calendar period resolved from the canonical corpus, not a sliding "latest N months"
default. Windows extending beyond available completed months are refused with the exact missing
months; nothing is downloaded, backfilled, padded or silently truncated to satisfy a window.

## CLI surfaces

All commands take the legacy catalog root (`/var/lib/hermes-factory`) and the converted native
output root (`/root/workspace/historical-migration-output/full-100k`).

```sh
# The owner-selected pin (no month-count argument: the pin IS the configuration).
target/release/rust-strategy-factory historical-corpus-register-research-window \
  <catalog-root> <output-root> <instrument> <spec.json>

# An explicit alternative window from a JSON config file.
target/release/rust-strategy-factory historical-corpus-register-window-config \
  <catalog-root> <output-root> <instrument> <window.json> <spec.json>

# Configurable latest-N completed months (PR #19 mechanism; refuses N > available).
target/release/rust-strategy-factory historical-corpus-register-window \
  <catalog-root> <output-root> <instrument> <months> <spec.json>

# Exact-count registration (the plan must hold exactly <expected> partitions).
target/release/rust-strategy-factory historical-corpus-register \
  <catalog-root> <output-root> <instrument> <spec.json> <expected>
```

Window JSON shape (unknown fields refused):

```json
{ "start_year": 2025, "start_month": 1, "end_year": 2026, "end_month": 10 }
```

The registration output reports `identity`, `instrument`, `months`, `first`, `last`, `window`
and `window_source` (`owner-selected-2026-11-19` for the pin, `explicit-config` for a supplied
window JSON).

The full-corpus research CLIs — `historical-corpus-register` (without the count argument),
`historical-corpus-study`, `historical-corpus-execute`, `historical-corpus-execute-cached` and
`historical-corpus-audit` — all register through the owner-selected pin.

## Refusal semantics

- A window month that is not a canonical completed partition is refused with the exact list, e.g.
  `research window 2025-01..2026-12 requires 24 completed months for EURUSD; only 22 available;
  missing: 2026-11, 2026-12`.
- Invalid calendar fields (month 0/13, year outside 2000..=2200) and reversed windows are refused
  before any selection.
- `register_window` with N greater than the available completed months refuses with the exact
  shortfall (`only 22 completed monthly partitions are available ... a 24-month window cannot be
  formed`); N = 0 is refused.
- Canonical partitions outside a requested window are simply not selected; they remain in the
  corpus and in catalog status. The pin never slides when later months become available.
- `historical-corpus-verify` requires a full-length specification to cover exactly the
  owner-selected window boundaries, then re-verifies every monthly receipt and chunk hash.

## Boundaries

The pin is a research-period configuration only. It grants no calendar completeness,
broker/profile approval, source licensing, research eligibility or qualification; the corpus
remains `REAL_STRUCTURALLY_VALID` with all four review gates pending. Diagnostic execution over
the window stays labelled `diagnostic_simulation`/`none` and is never research evidence.