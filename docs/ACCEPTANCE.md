# Acceptance evidence

Status: **SOFTWARE_INCOMPLETE**. This is a foundational checkpoint, not final acceptance.

2026-11-12, cloud Linux x86_64, Rust/Cargo 1.100.0:

- `scripts/check.sh`: formatting, Clippy all targets with warnings denied, and 14 integration tests passed (13 core and one CLI workflow). No tests were ignored or disabled.
- `cargo build --locked --release`: passed.
- Existing GitHub read access: `git ls-remote origin HEAD` passed for AKYA77/Rust-strategy-factory.
- Toolchain smoke: temporary library compiled, one generated unit test passed, formatting and Clippy passed. This is environment evidence, not application acceptance.
- CLI test actually imports decimal CSV, verifies hashes, rejects overwrites, creates causal candles/signals, executes one nonzero synthetic trade and verifies independently calculated $1.7982 net P&L.
- Core tests exercise all profile constructors, JPY/gold arithmetic, infeasible volume, adverse fractional-money rounding, future/stale conversions, tied timestamps, quarantine/tampering, all seven UTC bucket sizes, gap preservation, causal indicator prefixes, masks, canonical equivalence, feature budget, bid/ask spread floor, limit/stop distinction, short execution, losses beyond intended risk, deadline exits, unresolved positions and candle tampering.

No real historical quotes acquired. No approved broker/session profiles. No performance benchmark yet. No indexed mode, persistent scheduler, protected validation, qualification, dashboard, deployment or full historical acceptance yet. Existing source inspection/reuse and license matrix remains pending. No claim of billion-per-second throughput.

GitHub publication/CI evidence is recorded in commit history and the final session report; a workflow file alone does not establish a successful hosted CI run.
