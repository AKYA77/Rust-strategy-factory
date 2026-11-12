# Next action

Status: SOFTWARE_INCOMPLETE. Read AGENTS.md, PROJECT_STATE.json, ACCEPTANCE.md and the complete IMPLEMENTATION_QUEUE.md. Update this file after each verified milestone, reread it and continue the next task.

## Latest milestone

71 Rust application tests pass; formatting/Clippy and release compilation passed before the final added training-scope test. Chromium desktop/mobile acceptance still passes. Engine v3 supports chronological evaluation windows: prior-only warmup, flat initial positions, signal/entry start boundaries, exclusive end, unresolved positions rather than post-window fills. Independent windows preserve parent dataset identity and reject altered repeat holdout experiments. Real autonomous data requires explicit training_window.

Dataset identities now bind provenance/class/parser/archive metadata while excluding repeat-import time. Compute identity binds relevant source, manifest/lock/compiler/target/profile/flags and excludes embedded UI assets. Actual CSS-only rebuild changed executable SHA256 but preserved compute identity; original CSS was restored. See BENCHMARKS/2026-11-12-compute-identity.json. Different build profiles deliberately have distinct compute revisions.

## Begin next

Implement immutable frozen scientific protocol records and actual training/OOS/three-month walk-forward/final evaluation orchestration, including protected exposure, prior-only selection proofs, complete evidence identities, uncertainty/multiple-testing/robustness checks and safe qualification transitions. Current numeric diagnostics and window execution are foundations; they cannot issue validated status. Corpus-wide alias/source-revision exposure protection remains unfinished.

Continue HTF/operators and bounded mutation/improvement lineage, candidate-level net candle screening, native lossless storage/source acquisition/import acceptance, diagnostics, worker pools/OS resource control, full dashboard operations, production deployment and final end-to-end acceptance. All nine base families, pending orders, durable single-worker loop and authenticated dashboard core are implemented/tested; none makes the full product complete.

See RUNBOOKS/research-loop.md and RUNBOOKS/dashboard.md. No test process remains running. No qualified strategies, real approved quote samples, broker/DST approval, full-history or VPS acceptance. API/provider custom domains are still draft-only/unapplied, so source terms and hosted CI status remain externally unverified. Do not stop because this checkpoint passed; keep following the complete queue.
