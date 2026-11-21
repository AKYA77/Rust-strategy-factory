#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
# Route every build-touching cargo call through the locked isolation wrapper so
# the shared CARGO_TARGET_DIR can never be reused across checkouts (see
# docs/RUNBOOKS/cargo-target-isolation.md). fmt takes the lock but is not
# build-like (no forced clean); clippy/test enforce the flock + stamp/clean
# protocol. Ad-hoc cargo: PATH="scripts/shim:$PATH" cargo <cmd> or call
# scripts/cargo-locked.sh <cmd> directly.
export RSF_BUILDER_PROFILE="${RSF_BUILDER_PROFILE:-check.sh}"
scripts/cargo-locked.sh fmt --all -- --check
scripts/cargo-locked.sh clippy --locked --all-targets -- -D warnings
scripts/cargo-locked.sh test --locked --lib --bins --tests
