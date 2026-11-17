# Gap assessment — whole project (branch `hermes/real-data-migration`)

Factual state as of `2681c70`. Sources: `PROJECT_STATE.json`, `ACCEPTANCE.md`,
`NEXT_ACTION.md`, `MASTER_BLUEPRINT.md`, `IMPLEMENTATION_QUEUE.md`, and the real VPS corpus.
Main-branch ledgers predate the real-data work; their "zero real data" claims are historical.

## 1. Implemented and tested (evidence in repo/CI)

- Integer contracts, profiles, candles M1–D1, causal indicators/families, conservative
  screening, sequential market/pending order replay, SQLite registry with fenced leases,
  holdout reservations, frozen protocols, qualification-boundary checks, authenticated
  Axum dashboard, Argon2id sessions/CSRF, backup/restore, normalized import, dataset catalog,
  evidence journal, review authority, bounded native corpus, lossless native/RSFZ/Parquet
  storage, resource/disk admission, operations scripts.
- Historical data: 176 canonical partitions → 4,299 native chunks (421,078,937 events),
  source-bound conversion, restart safety, full-corpus bounded audit, multi-month corpus,
  catalog v2 with review gates. 450+ Rust tests; fmt/Clippy clean; CI green; 11 Chromium
  suites and local operations acceptance.

## 2. Implemented but not independently verified

- Indexed replay engine (experimental, opt-in; measured inconclusive, no default promotion).
- Multi-month historical corpus/catalog (verified by the implementer + one adversarial
  reviewer; a second specification reviewer was lost to provider rate limiting).
- Historical-data dashboard views (catalog dashboard exists; corpus/historical views partial).

## 3. Developed on an unmerged PR (not integrated)

- Codex PRs #8–#13: trusted checkpoint reuse, source-byte verification, checkpoint-aware
  range decoding, parallel worker affinity, failed-step lease cleanup, attempt fencing, long
  checkpoint histories, retained preflight inventory. 13 open PRs, none merged to main.

## 4. Partially implemented

- Real historical data → research pipeline: monthly native corpora load and run through
  candles/signals/replay, but no committed real-data end-to-end acceptance test and no
  multi-month research study artifact.
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

- Disk at 95% (≈5 GiB free). Full-history multi-instrument research workloads and further
  large allocations are unsafe until disposable intermediates are reclaimed (owner approval
  required; nothing deleted).

## 8. Blocked by owner merge authorization

- Merging PRs #6–#13 into main; target-VPS deployment/cutover; public exposure.

## Shortest dependency-aware route to real completion

1. Land the historical-data bridge (this branch): a committed real-data acceptance test
   proving corpus → candles → signals → replay on genuine VPS data, plus a multi-month study
   artifact. No gate bypass; structural/diagnostic only.
2. Surface historical instruments/months/coverage/gates in the existing dashboard.
3. Prepare a separately tested `hermes/integration-*` branch over Codex's stack; add shared
   integration tests without touching Codex branches.
4. Complete VPS operational acceptance (install/restart/reboot/recovery) on the release
   binary.
5. External approvals remain the hard blocker for >95% *verified* end-to-end; engineering can
   reach the software ceiling but not scientific qualification.

## Honest ceiling

Software implementation can approach completion independently; verified end-to-end readiness
is capped by (a) unmerged Codex stack, (b) external approvals and (c) live VPS acceptance.
No percentage is claimed above what evidence supports.
