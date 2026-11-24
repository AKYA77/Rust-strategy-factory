# Next action

Status: SOFTWARE_INCOMPLETE. Use the existing checkout at `/root/workspace/Rust-strategy-factory`.
Read AGENTS.md, PROJECT_STATE.json (superseded eight-instrument fields), IMPLEMENTATION_QUEUE.md,
MASTER_BLUEPRINT.md and the V2 handover mandate. After every verified milestone update
evidence/state/this file, commit, push, inspect remote/hosted CI, reread this file from disk and
continue. Never treat a pending CI run as passed.

## Latest verified local milestone (2026-11-24)

HEAD `a5030f8` on `main` == `origin/main` (pushed). Commits this session:
`b8571d8` (V2 seven-instrument scope, P0 fixes, verified gold-only data deletion), `a066dfa`,
`3705bc8`, `cf179c5` (browser integration fixes), `702360b` (docs milestone 1),
`f0ff8db` (P0.2 CandleScreen proof-authorization contract), `9824a4f` (P0.4 real-data
scientific qualification path), `4c050a2` (P0.3 continuous cross-month accounting),
`a9601e8` (P0.2 ordered full-history coverage command + receipt), `a5030f8` (rustfmt).

- Hosted CI run **45486926751** on `a5030f8`: **success** (Rust checks workflow).
- Hosted CI run **45486211553** on `a9601e8`: **success** (Rust checks workflow, all jobs).
- `scripts/check.sh` on `RSF_BUILDER_PROFILE=hermes-v2-m1`: **EXIT=0**, all suites pass.
  Log: `/root/.hermes/cache/scratch/m1-check19.log` (outside repo; durable evidence is the commit +
  hosted CI above).
- `npm run test:browser`: all 13 scripts pass locally. Earlier browser failures traced to the V2
  removal of XAUUSD moving the protocol fixture onto GBPUSD, which collided with the GBPUSD crash
  fixture's protected fold windows; the synthetic crash fixture now sits in Feb 2026 (outside every
  protected GBPUSD interval while still reaching the 2026 YTD assessment cutoff).
- Scope: `Profile::SYMBOLS` = EURUSD GBPUSD AUDUSD NZDUSD USDJPY USDCAD USDCHF. XAUUSD submissions
  are rejected (`gold_submission_rejected`); gold calendar checks removed; the 2026-11-21 market
  calendar approval is WITHDRAWN (profile session weeks/hours only, addendum checks retained).
- ORB causal replay fixed (`tests/orb_causality.rs` positive causality/order regressions);
  `tests/families.rs` uses a dedicated completed-range ORB fixture and may no longer skip ORB.
- Removed the false `candidate_index % 3` stage rotation. Every training candidate enters
  explicitly at TickVerification. Durable crash-safe stage transitions are NOT implemented yet.
- Conversion identity sentinel now quote-conditioned (USD-quote only); non-USD quotes require an
  event-aligned observation. Registered corpus mode now accepts non-USD profiles without a
  conversion file (P0.3 complete — conversion is required only when quotes are non-USD).
- Data-validation addendum implemented in `src/data_validation.rs` (timestamp ties vs duplicate
  `(timestamp_ms, sequence)` identities, time-based gap evidence, bounded streaming examples with
  exact counters) and `src/economic_calendar.rs` (instrument-identity filtering, causal
  availability, interval validation).

## P0 completions (verified)

**P0.2 — Ordered full-history coverage + CandleScreen auth:**
- `src/ordered_coverage.rs` (new): `research-ordered-coverage` command expands each configured
  first-chunk pointer to the full instrument-month via the local migration manifest, verifies every
  receipt chunk in order including cross-chunk sequence continuity, rejects XAUUSD and duplicate
  months, writes an ordered-coverage receipt. Fail-closed: never downloads, never mutates the
  corpus, never asserts provider completeness.
- `src/lib.rs`, `src/main.rs`: register ordered_coverage module and `research-ordered-coverage`
  CLI command.
- `tests/ordered_coverage.rs` (new): MonthSpec scope/date validation, duplicate and gold rejection,
  stage/month-limit bounds.
- `src/improvement.rs`, `tests/improvement.rs`: `authorize` accepts `Stage::CandleScreen` under the
  same protocol gate as `TickVerification`; `authorize_screening` delegates to `authorize`;
  proof-gating fully intact.
- Receipt: `docs/VERIFICATION/ordered-coverage-2026-11-24.json` — 140 instrument-months,
  2,551 chunks, 247,989,793 events, Jan 2025–Aug 2026, all sequence-continuous.

**P0.3 — Continuous cross-month accounting:** `src/historical_corpus.rs` — continuous account
state across months (feature history + open positions, exact intraperiod drawdown, correct streak
logic); registered corpus mode accepts non-USD profiles without a conversion file (conversion
required only when quotes are non-USD). Regression test: USDJPY across two months without a
conversion file.

**P0.4 — Real-data scientific qualification path:** `src/science.rs` `execute()` admits
`DataClass::RealStructurallyValid` alongside `SyntheticTestOnly`; every other class
(`RealUnreviewed`, `RealQualityAccepted`, `RealProfileApproved`, `ResearchEligible`, `Quarantined`)
is still refused. Regression test: REAL_STRUCTURALLY_VALID input accepted into the qualification
pipeline; synthetic refusal preserved where required.

## Implement next (ordered)

1. **Milestone 2**: replace the broad enumerator with exactly ten fixed-daily-checkpoint families
   and the 70 instrument-family groups / max-35 leaderboard, with per-family contract tests.
2. **Milestone 3**: durable stage transitions, 40% Stage-1 policy with proven-bound handling,
   bounded population, immutable evaluator, telemetry/lineage.
3. **Milestones 4–7**: shared preparation caches, full validation, measured optimization
   (correctness first, reference-equivalence proofs), VPS acceptance evidence.

## Owner decisions (V2, 2026-11-24)

* Active instruments: exactly the seven FX symbols above; XAUUSD forbidden and its project-owned
  data deleted.
* Market calendar policy approved 2026-11-21: **WITHDRAWN**. Session logic uses profile
  week/hour masks; no calendar completeness or closed-session claims.
* Broker dependency: REMOVED (deterministic broker-independent cost model: 2-pip spread floor,
  separate 2-pip adverse slippage, zero commission).
* External data acquisition: DISABLED. The local 22-month corpus (2025-01..2026-10, 154 FX
  partitions/root) is final; no downloads.
* Scientific protocol: >=50.00% net win rate, >=50 resolved exits per calendar year, max loss
  streak 5, no unresolved positions, full-window evidence; synthetic never qualifies. 70%+ is an
  aspiration, not a gate. The 12+6+4 split remains conditional on an exposure audit.
* Target VPS acceptance: continue against the existing 4-vCPU/8-GB environment. Reboot
  persistence requires an owner manual reboot (agent blocklist).

## Invariants

Exact executable bid/ask, 2-pip spread floor plus separate 2-pip adverse slippage, zero commission,
$20 initial / 10% intended risk / planned 3:1 RR / 24h max holding; frozen pending
distance/quantity; no fabricated gaps/fills/depth; no protected OOS or final adaptation; synthetic
never qualifies; one scheduled evaluation per eligible session-day (zero or one entry, deterministic
skip, no forced retry). Qualification requires independent approved complete evidence, not a high
test count or a benchmark. Structural validity is not licensing, calendar, profile or scientific
approval.
