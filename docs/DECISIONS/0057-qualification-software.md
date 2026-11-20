# 0057 — Independent qualification software: immutable decisions, evidence gates, diverse boards

Status: IMPLEMENTED (software). Zero strategies qualified; real activation still requires
approved actual evidence. Date: 2026-11-20. Branch: `hermes/qualification-software`.

## Requirement

MASTER_BLUEPRINT §8 (qualification criteria A–G), §20 (eight top-five leaderboards) and
IMPLEMENTATION_QUEUE item 12: "Qualification boundary tests and evidence gates; eight
independent diverse top-five boards. Synthetic data must never qualify." Before this change
the repo had numerical gates (`validation.rs`), frozen OOS evidence criteria
(`evidence_gates.rs`) and independent *data* review (`data_approval.rs`), but no software
could record an independent qualification decision or rank qualified strategies.

## Design decisions (owner-mission aligned)

1. **Standalone qualification store.** `qualification::Store` is a separate SQLite WAL
   database (schema v1), like the catalog ledger. It binds registry experiments, frozen
   scientific protocol reports and catalog approvals by exact identity hash; it does not
   modify the experiment registry schema. Two append-only tables with no-update/no-delete
   triggers: `qualification_decisions` and `qualification_boards`.
2. **Reuse of the local trust root.** Qualification decisions use the existing
   operator-provisioned `review_authority::Authority` (Argon2id credentials, keyed blake3
   authenticators, scoped expiring grants, submitter independence). A new additive
   `data_review::Area::ScientificQualification` scopes qualification grants; the five
   data-review areas and their eligibility loop are unchanged. Reviewer must differ from
   the bundle submitter, and the grant must cover the exact instrument/source/submitter.
3. **Ten fail-closed evidence gates** (`qualification::assess`, method
   `qualification-evidence-gates-v1`): identity binding; data class (synthetic/quarantined
   hard-rejected); independent five-area data approvals with live machine revalidation;
   non-research instrument profile; cost profile exact hash + ≥2-pip spread floor and
   separate ≥2-pip slippage; current frozen engine/indicator/signal versions + compute
   revision; complete coverage of exactly the 22 owner-selected months (2025-01..2026-10);
   full-period registered accounting (never diagnostic, never reset-fold sums) with the
   exact §8 numerical floors (≥50.00% net win rate, ≥50 resolved exits in each of 2025 and
   2026 YTD, ≤5 consecutive losses, ≥$500 ending equity, zero unresolved); frozen
   scientific evidence gates incl. conditional final-horizon and all robustness probes;
   planned 1:3 RR bound to the frozen engine contract. The assessment itself always has
   `qualification_granted = false`; only an authenticated independent decision qualifies.
4. **Policy floors cannot be weakened.** `qualification::Policy` (v1) validates against the
   blueprint/owner minima: win rate ≥5000 bps, ≥50/year, streak ≤5, equity ≥$500, the exact
   owner-selected 22-month window, five positions per instrument, diversity cap 1..=5.
   Stricter policies are allowed; weaker ones are refused.
5. **Immutable decisions.** Qualify/Reject/Revoke records carry the frozen policy, the
   complete evidence bundle, the recomputed assessment, authority/reviewer/grant identities
   and exact hashes. Per-(instrument, concept) expected-head fencing, monotonic clocks,
   bounded quotas (10000 decisions, 1000/scope, 256 KiB/body, 128 MiB journal). Qualify
   revalidates live at decision time: registry experiment identity/result hash and window
   coverage, frozen report hash + candidate gate equality, and a fresh
   `Catalog::data_eligibility` recomputation. Supplied hashes are never trusted.
6. **Diverse boards (§20).** Eight per-instrument boards, at most five independently
   qualified strategies each (40 positions total). Ranking method `diverse-ranking-v1` is
   deterministic exact-integer scoring over verified evidence only (training statistics are
   structurally absent): verified net win rate (30), complete trade count (10), net
   expectancy (15), simulated return (10), max drawdown (10), loss streak (5), year-by-year
   stability (10), frozen robustness probe ratio (5), optional-evidence completeness (5).
   Ties break by win rate, sample, then concept id. Diversity method `structural-diversity-v1`
   groups strategies by timeframe/side/sorted leaf-primitive set, so parameter tweaks of one
   rule shape share a class; at most two per class per board (frozen policy cap). Excluded
   qualified candidates are retained with exact reasons (diversity cap, board full, policy
   mismatch, unauthenticated). Empty boards show accurate reasons; disqualified candidates
   are never inserted to fill slots.
7. **Leader history and archive.** Every decision atomically publishes an immutable
   hash-chained board snapshot for its instrument (previous-leader archive). Rejected and
   revoked candidates remain in the decision journal. `history` filters by instrument,
   concept, decision kind, diversity class, timeframe and date range with keyset pagination;
   `leaders` pages retained snapshots. Authority rotation de-authenticates retained
   decisions without deleting them; live data-approval drift refuses new qualifications and
   surfaces through revalidation, never silent board mutation.
8. **Backup.** Verified no-overwrite SQLite snapshot publication (temp + integrity check +
   no-replace hard link), matching the catalog/result-artifact publication pattern.

## Boundaries preserved

- Synthetic data never qualifies; diagnostic simulation never qualifies; REAL_STRUCTURALLY_VALID
  alone grants nothing. Numerical passes remain evidence only.
- `science::execute` still blocks real protected-final execution; this change does not widen
  the registered engine gate or conversion semantics. Real protocol reports remain impossible
  to produce until those prerequisites land; the qualification layer is ready to consume them.
- No broker/calendar/legal/scientific approval is invented. Positive test branches use
  disposable fixtures that simulate the approval control flow only (same convention as the
  data-review acceptance tests) and assert no market evidence.
- Dashboard wiring (authenticated leaderboard views/exports) is intentionally left to the
  dashboard lane (`src/dashboard.rs` + `web/` ownership); the library/CLI surface here is the
  contract it will consume.

## Verification

See ACCEPTANCE.md (qualification software entry) and the provenance receipt
`docs/VERIFICATION/qualification-local-v1.json`. Hosted CI runs the full ladder on the PR SHA.
