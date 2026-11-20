# Source this file (in bash) to activate cross-worktree cargo isolation for the
# current shell:
#
#     . scripts/cargo-env.sh
#
# It exports the pinned cargo/rustup environment and puts scripts/shim first on
# PATH so that EVERY bare `cargo ...` routes through the current checkout's
# scripts/cargo-locked.sh (team-wide flock + provenance-stamp forced rebuild).
# scripts/check.sh and scripts/repro-cargo-isolation.sh call the wrapper directly,
# so they are enforced regardless of whether this file has been sourced.
#
# This is a VPS-lane convenience; hosted CI does not source it (CI uses a clean
# per-checkout target and is unaffected).
_RSF_ENV_ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
export CARGO_HOME="${CARGO_HOME:-/workspace/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-/workspace/.rustup}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/root/workspace/Rust-strategy-factory/target}"
export PATH="$_RSF_ENV_ROOT/scripts/shim:$CARGO_HOME/bin:$PATH"
export RSF_BUILDER_PROFILE="${RSF_BUILDER_PROFILE:-${USER:-unknown}}"
unset _RSF_ENV_ROOT