# Changelog

## 0.1.0 — implementation in progress

Initial exact contracts, normalized importer, closed UTC candles, causal indicators, canonical rules/shared masks, sequential market-entry replay and CLI. Fourteen application tests and release compilation verified on synthetic fixtures. Full factory implementation remains incomplete.

Second checkpoint: SQLite WAL experiment deduplication, fenced leases, immutable completion, online backup/restore, frozen holdout reservations, numeric qualification boundaries and chronological folds. Experimental fixed-barrier index remains disabled in replay. Twenty-eight tests and a synthetic eight-instrument resource/worker benchmark passed; complete factory remains unfinished.

Order checkpoint: pending entry lifecycle, expiry/cancel/restart snapshots and shared fill calculation; stream-exact importer hashes. Thirty-five tests pass. Generated pending-order/risk integration remains unfinished. Paired benchmark timings are retained without a speedup claim.

Pending-entry integration: typed strategy intents, exclusive fixed-quantity reservations, pre-fill risk/margin rejection, causally ordered cancellation, receipts and cost-aware marked initial margin. Engine v2; 47 tests pass. Synthetic benchmark rerun.

Nine-family milestone: causal RSI/mean crosses and trends, prior channels, range expansion, UTC sessions and bounded cache eviction. Fifty-three tests and release build pass; every family has nonzero synthetic tick trades.

Durable loop milestone: schema-v3 cursors/coordinator fencing, controls, fair inputs, bounded new regions, shared CLI evaluation, state-budget backpressure and conditional candle-bound audits. Sixty-four tests and standalone process smoke pass.

Dashboard core: authenticated localhost Axum/embedded frontend, Argon2id initialization, bounded sessions/CSRF/origin/host/rate/body controls, persistent run controls and real trade/provenance/export views. Sixty-five Rust tests and actual Chromium desktop/mobile/restart acceptance pass. Lossless i128 JSON receipts verified.

Chronological window milestone: causal warmup/flat starts/exclusive ends, protected parent-dataset evaluation, real-data training-scope guard, provenance-aware identities and compute fingerprints unaffected by embedded UI changes. Seventy-one tests pass; complete scientific workflow remains unfinished.
