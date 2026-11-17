# Checkpoint and source-proof integration

Fresh base: main `e16063713bd4c9f7891169bba11cf3df4eb78b3d`.
Explicit owner authorization: combine PR #2 and PR #7 on a separate integration branch, never main. Exact prerequisite heads were fetched and their successful hosted CI verified before integration:

- PR #2: `f3b268e778761d7a9236f2e4974785c0ff3a4804` (run43781157958).
- PR #7: `96f7780249328c70de61fcb6f94b7dfc4918f9c0` (run43785092364).

The integration retains both original commits as merge parents. No conflict resolution or production rewrite was needed. Joint tests add complete nonzero market/stop/limit receipt parity across native/blocked/mixed sources, actual warm prefix/account reuse with fresh byte checks, rehashed same-cursor checkpoint forgery refusal, consumed-source corruption refusal and post-key-admission mutation with a previously retained account checkpoint. Source proof never authenticates stored account JSON.

Existing PRs #1/#3/#4/#5 remain intact and are not implicitly incorporated. PR #5 is a descendant of #2; when later explicitly integrating it, merge its original history so the #2 commit is not duplicated. Hermes's converter/ingestion/VPS work and shared central state ledgers remain untouched. No approval to merge main is inferred.

Local combined verification:458 Rust tests, formatting, Clippy,2 compile-fail checks, all11 Chromium suites, optimized release and artifact-aware install/update/rollback/backup/restore operations pass. The1,051,200-quote synthetic horizon preserves all closed-frame counts and exact complete scalar/resumed/cached receipts. Three new joint tests cover9 nonzero native/blocked/mixed order-mode replays plus6 corruption/forgery scenarios and2 post-admission warm-checkpoint mutation scenarios. No prerequisite implementation was weakened. Draft integration PR must list dependencies #2/#7 and exact combined CI SHA. Independent green prerequisite runs are not combined acceptance.

Next action: create a separate stacked fast-seek branch from this verified integration commit. Add private proof-bound decoded artifact ranges with final whole-source byte verification. Warm account state can start at its exact committed cursor; cold/unmatched checkpoints must recompute the full account prefix. Keep the independent scalar replay and full source/class/budget/forgery checks. Record decoder work and complete receipt parity, plus adversarial mutation of skipped consumed/future artifacts. No persistent authenticated checkpoint or source-IO skipping is implied.
