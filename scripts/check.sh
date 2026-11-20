#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
# Every cargo call routes through the locked, provenance-stamped wrapper so the
# shared CARGO_TARGET_DIR can never silently reuse another checkout's artifacts.
# See docs/RUNBOOKS/cargo-target-isolation.md.
CARGO="./scripts/cargo-locked.sh"
"$CARGO" fmt --all -- --check
"$CARGO" clippy --locked --all-targets -- -D warnings
"$CARGO" test --locked --lib --bins --tests