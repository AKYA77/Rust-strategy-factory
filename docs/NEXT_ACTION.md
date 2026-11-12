# Next action

Status: SOFTWARE_INCOMPLETE. Read AGENTS.md, PROJECT_STATE.json, ACCEPTANCE.md and the complete IMPLEMENTATION_QUEUE.md. Update this file after each verified milestone, reread it and continue the next task.

## Latest milestone

93 Rust application tests pass, with formatting/Clippy, release build and Chromium desktop/mobile acceptance. SQLite schema6 now atomically reserves immutable OOS intervals before evaluation. Other protocols and dataset revisions cannot register overlapping work. Exact recovery remains allowed; range-set changes fail. Multi-range failures roll back completely. An actual two-thread/two-connection race proves that reservation and outside exposure cannot both succeed. An index supports same-instrument exposure lookup. Legacy global reservations stay conservative.

Frozen protocol execution, immutable reports/reuse, instrument-scoped and cross-dataset final protection, chronological windows, conditional Hoeffding/Bonferroni diagnostics and initial executable-stop feasibility remain tested. Reports always return INSUFFICIENT_EVIDENCE, uncertainty assumptions remain unverified, real final execution stays blocked before reservation. No qualified strategies. Earlier invalid 30-tick fixture goldens are historical/superseded (see ACCEPTANCE.md).

Frozen protocol v3 now binds deterministic cost stress and stop-neighborhood cases before OOS execution. Every case runs actual fold replay, has effective cost/concept/window receipts, and counts toward the trial budget. Only OOS inputs are used; final windows are excluded. Outcomes remain DESCRIPTIVE_ONLY. Old reports without later optional fields retain hash readability. See DECISIONS/0003-frozen-robustness-probes.md.

Protocol v4 now binds exact OOS exit-year/UTC entry-session breakdowns and per-fold realized cash drawdown. Net expectancy retains exact numerator/count; missing replay and unresolved positions stay explicit. Diagnostics reject receipt order errors and preserve state on overflow. Realized cash drawdown excludes intratrade marks, so it cannot stand in for full-account drawdown. Two independent arithmetic/boundary tests pass.

Protocol v5 now binds fixed causal M1 regime labels. Trend and range compare the latest strictly prior closed candle with 20 preceding candles; gaps reset history and unknown warmup/gap/stale context remains counted. One bounded publication-summary classifier is shared across frozen candidates. Prefix/entry-quote invariance, ties, gap reset/recovery and expanded/downward cases pass. OOS regime counts reconcile with actual receipts. See DECISIONS/0004-causal-regime-labels.md.

Selected-source review now records exact Git revisions/file hashes/licenses for Tickforge, Wickra and LZ4 Flex. Tickforge is unsuitable as the authoritative engine under our later-event/exact-account contract; Wickra is a possible numerical oracle requiring compatibility checks; LZ4 Flex remains an unmeasured codec candidate. No code copied/dependency added. See SOURCE_REUSE_MATRIX.md and SOURCE_REVIEW receipts. Network status was rechecked: custom provider/API domains remain unapplied.

## Begin next

Implement broader parameter neighborhoods and dependence-aware inference, with sufficient-sample and prespecified robustness acceptance gates. Keep assumptions explicit and work reproducible. Complete corpus-wide external exposure proofs, coverage/approval prerequisites and safe qualification transitions before unlocking real final evaluation. Current protocol execution and pooled reset-fold statistics cannot issue validated status.

Continue HTF/operators and bounded mutation/improvement lineage, candidate-level net candle screening, native lossless storage/source acquisition/import acceptance, diagnostics, worker pools/OS resource control, full dashboard operations, production deployment and final end-to-end acceptance. Follow the complete IMPLEMENTATION_QUEUE.md rather than stopping at this milestone.

See RUNBOOKS/scientific-protocol.md, RUNBOOKS/research-loop.md and RUNBOOKS/dashboard.md. No real approved quote samples, broker/DST approval, full-history or VPS acceptance. API/provider custom domains remain draft-only/unapplied, so source terms and hosted CI status remain externally unverified. External tools and independent registry copies remain outside local exposure protection.
