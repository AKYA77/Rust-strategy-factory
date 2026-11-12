# Current architecture

One Cargo package, safe Rust. `contracts` defines profile units, USD millionths, micro-lots, explicit currency conversion and conservative arithmetic. `data` hashes provenance and validates original event order without repairs. `candles` publishes closed UTC candles only after a real boundary event. `indicators` and `strategies` share causal primitives, canonical rules and bounded features. `tick_engine` is authoritative sequential replay; `main` provides file-based CLI workflows.

No persistent research service exists yet. The planned next layer is immutable experiment identities, SQLite WAL leases/results, scientific split reservations and restart-safe scheduling. Axum and embedded assets follow the durable backend. Only supported execution semantics may enter an accelerated index. Full contract and calendar provenance must gate qualification.

All fixture profiles are SYNTHETIC_TEST_ONLY. Reading a JSON field that says research-eligible is not independent approval; no qualification API exists. Dataset integrity hashes establish content identity, not trusted provider authenticity. Production approval records and signatures/access control remain unfinished.
