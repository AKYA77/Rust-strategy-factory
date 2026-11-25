# Conversion implementation reconciliation — USDJPY/USDCHF/USDCAD (2026-11-25)

Status: VERIFIED (code + regression evidence). No financial, scientific, provenance, data-scope or
security rule was weakened. No calendar completeness, broker/profile approval, source licensing,
research eligibility or qualification is granted. The corpus class remains REAL_STRUCTURALLY_VALID
with eligibility none.

Implemented at commit `c3ee6c040171a9ffaee53eedf438267f431faf1b` ("Reconcile conversion
implementation for USDJPY/USDCHF/USDCAD"), already on `main` and equal to `origin/main` when this
continuity run began. Hosted CI run `45676627807` on that exact SHA reached terminal
`completed` / `success` on all 7 jobs (release-build, browser, rust-checks,
data-compute-identity, full-horizon, check-operations, dependency-audit). Only that terminal
conclusion is recorded here.

## What changed

- `src/main.rs` `research_inputs()`: a non-USD-quote dataset is no longer refused solely for
  lacking a conversion file. For USD-base non-USD-quote profiles (USDJPY, USDCHF, USDCAD) one
  exact reciprocal conversion is derived per tick from that tick's own bid price with
  `Conversion::usd_account_from_usd_base_price` (event time = `tick.timestamp_ms`). USD-quoted
  profiles keep `Conversion::usd()`.
- `src/tick_engine.rs` `replay_corpus_internal`: `ReplayMode::Registered` now uses
  `Conversion::usd()` for USD quotes and `diagnostic_conversion(...)` for USD-base non-USD quotes
  (bid for Long, ask for Short). The declared-class guard is unchanged; the quote guard now
  refuses only instruments whose base is also non-USD. `ReplayMode::Diagnostic` is unchanged.
- `tests/corpus.rs`: the USDJPY registered-replay regression was inverted from "must be refused"
  to "must succeed with the reciprocal convention", for both the library call and the CLI
  `corpus-replay` command.
- `tests/verify_conversion.rs`: expanded to 13 tests, adding dataset-level derivation tests that
  mirror `research_inputs()` for all three instruments.

## Invariants preserved (explicit)

Exact integer arithmetic via reduced `price_scale / price` and `i128` cross-product checks; strict
`as_of_ms == tick.timestamp_ms`; every `Conversion::validate` safeguard (identity sentinel refused
for non-USD quotes, future/stale refusal, bounded staleness only when configured, price/tick
validity); no `1/1` identity conversion for a non-USD quote; provenance and declared class
unchanged. The reciprocal rate remains an **explicit, unverified diagnostic research convention
derived from the instrument's own verified quote** — not an approved broker conversion rule.
`ReplayMode::Registered` here names the engine execution mode; it does not promote the rate to an
approved conversion or grant scientific qualification.

## Red / green regression demonstration

| Phase | Change | Command result | Named outcome |
|---|---|---|---|
| RED (parent `15a635e` inverse assertion restored temporarily in `tests/corpus.rs`) | `assert!(replay_corpus(...).is_err())` | `EXIT=101` | `corpus_account_replay_matches_all_materialized_receipt_fields ... FAILED` (`0 passed; 1 failed; 95 filtered out`); panic at `tests/corpus.rs:508` |
| GREEN (HEAD assertion restored) | `assert!(replay_result.is_ok(), ...)` | `EXIT=0` | same test `ok` (`1 passed; 0 failed; 95 filtered out`) |

The temporary edit was byte-for-byte restored; `git status --porcelain` was empty afterwards. No
tracked file was left modified.

Direct focused runs at HEAD:

- `scripts/cargo-locked.sh test --locked --test verify_conversion --test real_conversion_evidence`
  → `13 passed / 0 failed` and `2 passed / 0 failed`.
- Full ladder `scripts/check.sh` with `RSF_BUILDER_PROFILE=hermes-v2-m1`: **EXIT=0**,
  **759 passed / 0 failed across 71 test binaries (lib unit tests 276)**, zero named failures,
  fmt and clippy `-D warnings` clean.

## Limitations

- The reciprocal convention is a documented research assumption, not a broker or venue rule;
  no live/broker use and no data acquisition.
- Bounded implementation + regression evidence only: not a full 22-month non-USD conversion
  study and not scientific qualification.
- M7 external gates (saved dashboard credentials, owner manual reboot evidence) are unchanged
  and remain owner-gated.
