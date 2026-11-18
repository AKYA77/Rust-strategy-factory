# Gap assessment — whole project (merged state, `main` `47e7ccb`)

Factual state as of `main` `47e7ccb` (integration of all 13 PRs). Sources: `PROJECT_STATE.json`,
`ACCEPTANCE.md`, `NEXT_ACTION.md`, `MASTER_BLUEPRINT.md`, `IMPLEMENTATION_QUEUE.md`, and the real
VPS corpus. Earlier main-branch ledgers predate the real-data work; their "zero real data" claims
are historical.

## 1. Implemented and tested (evidence in repo/CI)

- Integer contracts, profiles, candles M1–D1, causal indicators/families, conservative
  screening, sequential market/pending order replay, SQLite registry with fenced leases,
  holdout reservations, frozen protocols, qualification-boundary checks, authenticated
  Axum dashboard, Argon2id sessions/CSRF, backup/restore, normalized import, dataset catalog,
  evidence journal, review authority, bounded native corpus, lossless native/RSFZ/Parquet
  storage, resource/disk admission, operations scripts.
- Checkpoint/performance stack (trusted checkpoint reuse, source-byte verification,
  checkpoint-aware range decoding, parallel worker affinity, failed-step lease cleanup, attempt
  fencing, long checkpoint histories, retained preflight inventory), campaign-owned ledger
  scaling, and durable campaign rotation.
- Historical data: 176 canonical partitions → 4,299 native chunks (421,078,937 events),
  source-bound conversion, restart safety, full-corpus bounded audit, multi-month corpus,
  catalog v2 with review gates. 511 Rust tests passed, 0 failed; fmt/Clippy clean; hosted CI
  green on `main`; 11 Chromium suites and local operations acceptance.

## 2. Implemented but not independently verified

- Indexed replay engine (experimental, opt-in; measured inconclusive, no default promotion).
- Multi-month historical corpus/catalog (verified by the implementer + one adversarial
  reviewer; a second specification reviewer was lost to provider rate limiting).

## 3. Integrated (closed as superseded)

- All 13 open PRs (#1–#13) were merged into `main` through #14 (`3d2d6f2`) with no conflicts.
  Every PR head commit was verified as an ancestor of `main` before its PR was closed; no branch
  was force-pushed or deleted.

## 4. Partially implemented

- Real historical data → research pipeline: monthly native corpora load and run through
  candles/signals, and `tick_engine::replay_corpus_diagnostic` now executes the authoritative
  sequential engine over the real structurally-valid corpus as explicitly non-eligible
  `diagnostic_simulation` (`tests/historical_pipeline_real.rs` plus
  `DIAGNOSTIC_EXECUTION_RECEIPT.json`). Approved real/protected *research* execution and FX
  conversion semantics remain refused.
- Full-history qualification, fast-seek resume, complete indexed accounting.
- Autonomous campaign replenishment and durable multi-worker supervision.
- Dashboard historical instruments/months/coverage/gate views.

## 5. Missing

- Automatic retry/process supervision; block/direct-array codecs; full indexed accounting;
  live systemd/reboot acceptance; public HTTPS deployment.

## 6. Blocked by external approval (must not be invented)

- Broker instrument/profile specifications, trading calendar/session and DST, provider
  licence/redistribution (source `HistData.com`), scientific protocol approval. Source
  acquisition endpoints return HTTP 403; no genuine quotes acquired outside the VPS corpus.

## 7. Blocked by insufficient resources

- Disk at ~92–95% (≈5 GiB free). Full-history multi-instrument research workloads and further
  large allocations are unsafe until disposable intermediates are reclaimed (owner approval
  required; nothing deleted).

## 8. Resolved

- Owner merge authorization: PR #14 merged the integration branch into `main`; #15–#17 landed
  the state reconciliation. No merge authorization is currently pending.

## Shortest dependency-aware route to real completion

1. ~~Land the historical-data bridge~~ — done: committed real-data acceptance test plus a
   multi-month study artifact; structural/diagnostic only, no gate bypass.
2. ~~Integrate the Codex stack and close superseded PRs~~ — done via #14.
3. Surface historical instruments/months/coverage/gates in the existing dashboard.
4. Complete VPS operational acceptance (install/restart/reboot/recovery) on the release binary.
5. External approvals remain the hard blocker for >95% *verified* end-to-end; engineering can
   reach the software ceiling but not scientific qualification.

## Honest ceiling

Software implementation can approach completion independently; verified end-to-end readiness
is capped by (a) external approvals and (b) live VPS acceptance. No percentage is claimed above
what evidence supports.
