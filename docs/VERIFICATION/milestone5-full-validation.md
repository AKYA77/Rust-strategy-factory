# Milestone 5 — full validation: contract evidence

Date: 2026-11-25
Baseline verified: `main` @ `18d6e9b` ("Milestone 2: ten fixed-daily-checkpoint families replace broad enumerator").

Verification was performed in an isolated `git worktree` at that exact commit because
the primary checkout (`/root/workspace/Rust-strategy-factory`) carried concurrent,
uncommitted Milestone 3/4 edits from sibling agents during this session. The
contract tests themselves live in the primary checkout at
`tests/milestone5_full_validation.rs`.

No external data, no downloads, no live execution. All fixtures are local synthetic
tick streams. Nothing here grants eligibility, approval or qualification.

## Requirement → evidence map

| # | Requirement | Implementation (existing, tested) | New contract test |
|---|-------------|-----------------------------------|-------------------|
| 1 | `REAL_STRUCTURALLY_VALID` admitted to the frozen-protocol pipeline; synthetic never qualifies | `src/science.rs::execute` admits `SyntheticTestOnly` and `RealStructurallyValid`; `summary()` always returns `qualification_granted=false` | `req1_real_structurally_valid_is_admitted_and_never_granted_qualification`, `req1_classes_below_the_structural_bar_are_refused_before_reservation` |
| 2 | Prior-only training/feature warmup, 24h embargo, boundary-leakage prevention, contaminated ≠ unseen | `src/validation.rs::{walk_forward,Fold::training_observation}`, `MAX_HOLD_MS`; `src/science.rs` exposure check refuses an already-exposed range | `req2_walk_forward_folds_are_prior_only_with_a_24h_purge_embargo`, `req2_boundary_leakage_is_rejected_by_the_training_observation_contract`, `req2_prior_exposure_is_treated_as_contaminated_never_unseen` |
| 3 | Continuous chronological accounting with exact exit-year counts; no monthly-reset summing | `historical_corpus::{execute_continuous,execute_registered_full_period}` (single account chained across months); `src/validation.rs::assess` reconciles exit-year buckets to the pool | `req3_continuous_accounting_reconciles_exact_exit_year_counts` |
| 4 | All ten active families evaluated; annual resolved-trade floor preserved; skips excluded | `src/strategies.rs::family_checkpoints` (exactly ten); `src/science.rs` evaluates every frozen candidate; `validation::assess` enforces `>=50` resolved exits per calendar year | `req4_all_ten_active_families_are_evaluated_in_one_frozen_protocol`, `req5_scientific_gates_pin_the_owner_criteria_exactly` |
| 5 | Synthetic excluded; `>=50%` net win rate; `>=50` resolved exits/year; max loss streak 5; zero unresolved; frozen OOS/holdout only | `src/validation.rs::assess` (numeric gates) and `src/qualification.rs::assess` (data-class gate rejects synthetic/quarantined; `qualification_granted` always false) | `req5_scientific_gates_pin_the_owner_criteria_exactly`, `req5_synthetic_evidence_is_rejected_at_the_gate_regardless_of_numbers` |

## Exact commands and results

Environment: `CARGO_HOME=/root/.cargo RUSTUP_HOME=/root/.rustup PATH=/root/.cargo/bin:$PATH`,
toolchain `1.100.0` (`rust-toolchain.toml`).

### Build / format / new contract suite

```
$ cargo fmt --all
$ cargo test --locked --test milestone5_full_validation
running 9 tests
test req1_classes_below_the_structural_bar_are_refused_before_reservation ... ok
test req1_real_structurally_valid_is_admitted_and_never_granted_qualification ... ok
test req2_boundary_leakage_is_rejected_by_the_training_observation_contract ... ok
test req2_prior_exposure_is_treated_as_contaminated_never_unseen ... ok
test req2_walk_forward_folds_are_prior_only_with_a_24h_purge_embargo ... ok
test req3_continuous_accounting_reconciles_exact_exit_year_counts ... ok
test req4_all_ten_active_families_are_evaluated_in_one_frozen_protocol ... ok
test req5_scientific_gates_pin_the_owner_criteria_exactly ... ok
test req5_synthetic_evidence_is_rejected_at_the_gate_regardless_of_numbers ... ok
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
EXIT=0
```

### Full Milestone 5-relevant regression set

```
$ cargo test --locked --test science --test registered_full_period \
    --test scientific_validation --test validation --test families \
    --test milestone5_full_validation
tests/families.rs            ... 10 passed; 0 failed
tests/milestone5_full_validation.rs ... 9 passed; 0 failed
tests/registered_full_period.rs     ... 6 passed; 0 failed
tests/science.rs             ... 17 passed; 0 failed
tests/scientific_validation.rs      ... 7 passed; 0 failed
tests/validation.rs          ... 5 passed; 0 failed
TOTAL: 54 passed; 0 failed; EXIT=0
```

## Observed behavior worth recording

- Running a `REAL_STRUCTURALLY_VALID` dataset and a `SYNTHETIC_TEST_ONLY` dataset with
  **identical tick content** through the *same* registry correctly fails with
  `independent range already exposed outside this frozen protocol`. This is the
  exposure ledger refusing to call an already-touched range "unseen"; the contract
  test pins it (`req2_prior_exposure_is_treated_as_contaminated_never_unseen`).
- A structurally valid bundle whose other evidence gates are satisfied reaches
  `QUALIFICATION_EVIDENCE_COMPLETE` but `qualification_granted` remains `false`;
  only an authenticated independent decision can qualify. The same bundle labelled
  `SYNTHETIC_TEST_ONLY` is `REJECTED` at the `data-class` gate regardless of numbers.

## Pending / honest limits

- The Milestone 3/4 work observed in the primary checkout (durable stage transitions,
  telemetry lineage, 40% Stage-1 policy, shared preparation caches) is owned by other
  agents and was not committed by this task. It must land and pass `scripts/check.sh`
  before any cross-milestone claim.
- No XAUUSD anywhere; seven-instrument FX scope retained by `contracts::SYMBOLS`.
- Live VPS acceptance and owner manual reboot evidence remain external blockers.
