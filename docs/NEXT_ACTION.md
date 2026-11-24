# Next action

Status: SOFTWARE_INCOMPLETE. Use the existing checkout at `/root/workspace/Rust-strategy-factory`.
Read AGENTS.md, PROJECT_STATE.json (superseded eight-instrument fields), IMPLEMENTATION_QUEUE.md,
MASTER_BLUEPRINT.md and the V2 handover mandate. After every verified milestone update
evidence/state/this file, commit, push, inspect remote/hosted CI, reread this file from disk and
continue. Never treat a pending CI run as passed.

## Latest verified local milestone (2026-11-24)

HEAD `cf179c5` on `main` == `origin/main` (pushed). Commits this session: `b8571d8` (V2
seven-instrument scope, P0 fixes, verified gold-only data deletion), `a066dfa`, `3705bc8`,
`cf179c5` (browser integration fixes).

- Hosted CI run **45474835643** on `cf179c5`: **all 7 jobs success** (rust-checks, release-build,
  data-compute-identity, full-horizon, check-operations, browser, dependency-audit).
- `scripts/check.sh` on `RSF_BUILDER_PROFILE=hermes-v2-m1`: **EXIT=0**, 65 test suites,
  **694 passed / 0 failed**, fmt clean, Clippy `-D warnings` clean. Log:
  `/root/.hermes/cache/scratch/m1-check16.log` (outside repo; durable evidence is the commit +
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
  event-aligned observation. Registered corpus mode still refuses non-USD profiles unless a
  conversion file is supplied; continuous event-aligned conversion support is NOT complete.
- Data-validation addendum implemented in `src/data_validation.rs` (timestamp ties vs duplicate
  `(timestamp_ms, sequence)` identities, time-based gap evidence, bounded streaming examples with
  exact counters) and `src/economic_calendar.rs` (instrument-identity filtering, causal
  availability, interval validation).

### Gold-only data deletion (verified)

Receipt: `docs/VERIFICATION/xauusd-removal-2026-11-24.json`. No corpus job was active (the only
long-running process is the dashboard `serve`, which holds no corpus files open).

- Deleted 44 XAUUSD-only partitions across both project-owned roots
  (`historical-migration-output/full-100k/` and `.../full/`), plus
  `XAUUSD-corpus.json`, `audit-XAUUSD.json`, `study-XAUUSD.json`, 22 tick archives and 22 locks.
- **10,436,202,166 bytes freed**; 1,875 files deleted; remaining gold paths under both roots: 0.
- All **308 retained FX partitions** were hashed before mutation and re-hashed after deletion:
  **0 identity mismatches**. Retained counts: 154 partitions per root, 7 instruments.
- Preserved as immutable superseded audit metadata: gold manifests/page-sha256, import reports,
  backfill logs and study/diagnostic-cache JSON. See
  `docs/VERIFICATION/superseded-eight-instrument-receipts.md`.

## Implement next (ordered)

1. **P0.2 full-corpus coverage**: replace the 160-path Jan 2025–Aug 2026 first-chunk config with
   ordered consumption of every eligible chunk for the specified stage; record ordered coverage;
   keep round-robin research selection independent from coverage evidence.
2. **P0.2 proof authorization contract**: reproduce that `improvement::authorize` rejects
   `Stage::CandleScreen` while proof-policy evaluation invokes it; correct the contract without
   bypassing authorization.
3. **P0.3 continuous accounting/currency**: one continuous chronological account state across
   months (feature history + open positions, exact intraperiod drawdown, correct streak logic)
   and event-aligned conversion support for USDJPY/USDCHF/USDCAD in registered corpus mode.
4. **P0.4 real-data scientific path**: implement the approved real-data qualification path in
   `src/science.rs`/`qualification.rs` bound to actual accounting receipts; recompute all counters
   from immutable receipts; audit prior exposure of the 22-month window before claiming any
   holdout is unseen (else mark contaminated).
5. **Milestone 2**: replace the broad enumerator with exactly ten fixed-daily-checkpoint families
   and the 70 instrument-family groups / max-35 leaderboard, with per-family contract tests.
6. **Milestone 3**: durable stage transitions, 40% Stage-1 policy with proven-bound handling,
   bounded population, immutable evaluator, telemetry/lineage.
7. **Milestones 4–7**: shared preparation caches, full validation, measured optimization
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
