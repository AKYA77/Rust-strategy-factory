#!/usr/bin/env bash
# cargo-locked.sh — the single chokepoint for EVERY cargo invocation that builds
# against the shared CARGO_TARGET_DIR on this VPS.
#
# Hazard it removes (see docs/RUNBOOKS/cargo-target-isolation.md):
#   All rsf lanes share one CARGO_TARGET_DIR. The project's lib/bin/test artifacts
#   live at target/debug/build/rust-strategy-factory/<hash>/out/ and cargo's
#   metadata <hash> is checkout-independent, so one worktree's build overwrites
#   another's and a later ladder can silently run FOREIGN binaries while green.
#
# Machine-enforced protocol:
#   1. Acquire the team-wide heavy flock (one heavy job at a time).
#   2. For build-like commands, compare a provenance stamp (checkout path + git
#      HEAD SHA + dirty-tree content hash) against the shared target's last
#      builder. On ANY mismatch/absence run `cargo clean -p rust-strategy-factory`
#      INSIDE the lock (project artifacts only; the dependency cache stays warm).
#   3. Run the real cargo command via the rustup proxy (shim dir stripped from PATH).
#   4. On success record the stamp + a provenance log line; on failure invalidate
#      the stamp so the next invocation is forced to rebuild. Never reuse uncertain
#      artifacts. Bookkeeping survives a full `cargo clean` (which deletes the
#      target dir itself): dirs are recreated and the log preserved via a temp
#      file, so cargo's own exit code is always reported faithfully (defect D1).
#
# Mechanics regression suite (light, throwaway target/lock, no heavy builds):
#   scripts/test-cargo-locked.sh
#
# Usage (identical args to cargo):
#   scripts/cargo-locked.sh test --locked --lib --bins --tests
#   scripts/cargo-locked.sh clippy --locked --all-targets -- -D warnings
#
# Env knobs:
#   RSF_CHECKOUT_DIR   override the checkout to operate on (default: this script's
#                      own repo root). Used by the cross-worktree repro to drive a
#                      foreign/main checkout through this same wrapper.
#   RSF_HEAVY_LOCK     override the team-wide lock path (default the VPS lock; if
#                      its dir is absent, e.g. hosted CI, fall back to a per-target
#                      lock so CI keeps working).
#   RSF_LOCK_WAIT      seconds to wait for the heavy lock (default 3600). Raise it
#                      when queuing behind a long ladder (e.g. 10800).
#   RSF_RECEIPT_DIR    if set, write a per-binary test-count + binary-sha256 +
#                      source-SHA receipt for test/build runs into this directory.
#   RSF_RECEIPT_LABEL  receipt file stem (default "run").
#   RSF_BUILDER_PROFILE provenance label for who is building (default $USER).
set -euo pipefail

# --- locate the checkout to operate on ----------------------------------------
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
if [ -n "${RSF_CHECKOUT_DIR:-}" ]; then
  ROOT=$(CDPATH= cd -- "$RSF_CHECKOUT_DIR" && pwd -P)
else
  ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd -P)
fi
cd "$ROOT"

# --- cargo / rustup environment -----------------------------------------------
# Respect an existing cargo/rustup environment (VPS lanes preset it; hosted CI has
# its own). Only fall back to the VPS locations when they actually exist, so we do
# NOT point rustup at a nonexistent /workspace/.rustup on a CI runner.
if [ -z "${CARGO_HOME:-}" ] && [ -d /workspace/.cargo ]; then
  export CARGO_HOME=/workspace/.cargo
fi
if [ -z "${RUSTUP_HOME:-}" ] && [ -d /workspace/.rustup ]; then
  export RUSTUP_HOME=/workspace/.rustup
fi
if [ -n "${CARGO_HOME:-}" ]; then
  export PATH="$CARGO_HOME/bin:$PATH"
fi
# Respect CARGO_BUILD_JOBS if the caller set it (VPS lanes use 4); do not override
# it here so hosted CI keeps its own parallelism.

# --- shared target dir (respect the preset; CI falls back to ./target) --------
if [ -n "${CARGO_TARGET_DIR:-}" ]; then
  TARGET_DIR=$CARGO_TARGET_DIR
else
  TARGET_DIR="$ROOT/target"
fi
export CARGO_TARGET_DIR="$TARGET_DIR"
mkdir -p "$TARGET_DIR"

# --- team-wide heavy lock -----------------------------------------------------
LOCK=${RSF_HEAVY_LOCK:-/root/workspace/.rsf-heavy.lock}
LOCK_DIR=$(dirname -- "$LOCK")
if [ ! -d "$LOCK_DIR" ] || [ ! -w "$LOCK_DIR" ]; then
  LOCK="$TARGET_DIR/.rsf-heavy.lock"
fi
mkdir -p "$(dirname -- "$LOCK")"
touch "$LOCK"

STAMP="$TARGET_DIR/.rsf-build-stamp.json"
PROV_LOG="$TARGET_DIR/.rsf-provenance.jsonl"
LOG_DIR="$TARGET_DIR/.rsf-logs"
mkdir -p "$LOG_DIR"
PKG=rust-strategy-factory

# --- resolve the REAL cargo: the rustup-managed PROXY (never our shim) ---------
# Use the rustup proxy, NOT a bare toolchain binary from `rustup which cargo`: the
# proxy exports RUSTUP_TOOLCHAIN so build scripts / proc-macros use the pinned
# toolchain. A direct toolchain binary can let subprocesses fall back to the
# runner's DEFAULT toolchain (e.g. a different stable rustc on CI) -> E0514
# incompatible-crate errors. Strip our own shim dir from PATH to avoid recursion.
_clean_path=""
IFS=':' read -ra _path_parts <<< "$PATH"
for _p in "${_path_parts[@]}"; do
  case "$_p" in
    */scripts/shim) ;; # skip the cargo shim dir
    *) _clean_path="${_clean_path:+$_clean_path:}$_p" ;;
  esac
done
REAL_CARGO=$(PATH="$_clean_path" command -v cargo 2>/dev/null || true)
if [ -z "$REAL_CARGO" ] && [ -n "${CARGO_HOME:-}" ] && [ -x "$CARGO_HOME/bin/cargo" ]; then
  REAL_CARGO="$CARGO_HOME/bin/cargo"
fi
if [ -z "$REAL_CARGO" ] || [ ! -x "$REAL_CARGO" ]; then
  echo "cargo-locked: cannot resolve the rustup cargo proxy" >&2
  exit 127
fi

SUB=${1:-}
is_build_like() {
  case "$1" in
    build|test|clippy|check|run|bench|doc|rustc|install) return 0 ;;
    *) return 1 ;;
  esac
}

# --- provenance identity of the CURRENT checkout/source -----------------------
HEAD_SHA=$(git rev-parse HEAD 2>/dev/null || echo unknown)
CHECKOUT_PATH=$(pwd -P)
# Dirty-tree content hash: tracked edits (git diff HEAD) plus build-relevant
# untracked files. Conservative by design — any uncertainty forces a rebuild,
# never a reuse. Clean tree => stable hash => within-lane incremental caching.
dirty_hash() {
  {
    git diff HEAD 2>/dev/null || true
    git ls-files --others --exclude-standard -z 2>/dev/null \
      | while IFS= read -r -d '' f; do
          case "$f" in
            *.rs|*.toml|*.lock|build.rs)
              printf '%s\n' "$f"
              sha256sum -- "$f" 2>/dev/null || true
              ;;
          esac
        done
  } | sha256sum | awk '{print $1}'
}
DIRTY_HASH=$(dirty_hash)

# --- stamp comparison (python stdlib only) ------------------------------------
stamp_status() {
  if [ ! -f "$STAMP" ]; then printf 'no-stamp'; return 0; fi
  env -u PYTHONPATH python3 - "$STAMP" "$CHECKOUT_PATH" "$HEAD_SHA" "$DIRTY_HASH" <<'PY'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception:
    print("unreadable-stamp"); sys.exit(0)
cur = {"checkout_path": sys.argv[2], "head_sha": sys.argv[3], "dirty_hash": sys.argv[4]}
for k in ("checkout_path", "head_sha", "dirty_hash"):
    if d.get(k) != cur[k]:
        print("mismatch:" + k); sys.exit(0)
print("match")
PY
}

write_stamp() {
  env -u PYTHONPATH python3 - "$STAMP" "$CHECKOUT_PATH" "$HEAD_SHA" "$DIRTY_HASH" "$CMD_STR" <<'PY'
import json, sys, os, datetime
d = {
    "checkout_path": sys.argv[2],
    "head_sha": sys.argv[3],
    "dirty_hash": sys.argv[4],
    "command": sys.argv[5],
    "builder": os.environ.get("RSF_BUILDER_PROFILE", os.environ.get("USER", "unknown")),
    "written_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
}
os.makedirs(os.path.dirname(os.path.abspath(sys.argv[1])), exist_ok=True)
with open(sys.argv[1], "w") as fh:
    json.dump(d, fh, indent=2)
    fh.write("\n")
PY
}

append_provenance() {
  # $1=exit_code $2=cleaned(0/1) $3=reason $4=logfile
  env -u PYTHONPATH python3 - "$PROV_LOG" "$CHECKOUT_PATH" "$HEAD_SHA" "$DIRTY_HASH" \
      "$CMD_STR" "$1" "$2" "$3" "$4" <<'PY'
import json, sys, os, datetime
rec = {
    "at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "checkout_path": sys.argv[2],
    "head_sha": sys.argv[3],
    "dirty_hash": sys.argv[4],
    "command": sys.argv[5],
    "exit_code": int(sys.argv[6]),
    "forced_clean": sys.argv[7] == "1",
    "clean_reason": sys.argv[8],
    "log": sys.argv[9],
    "builder": os.environ.get("RSF_BUILDER_PROFILE", os.environ.get("USER", "unknown")),
}
os.makedirs(os.path.dirname(os.path.abspath(sys.argv[1])), exist_ok=True)
with open(sys.argv[1], "a") as fh:
    fh.write(json.dumps(rec) + "\n")
PY
}

CMD_STR="$*"
TS=$(date -u +%Y%m%dT%H%M%SZ)
LOG="$LOG_DIR/$TS-${SUB:-cmd}.$$.log"

# --- acquire the team-wide heavy lock for the whole invocation ----------------
exec 9>"$LOCK"
LOCK_WAIT=${RSF_LOCK_WAIT:-3600}
if ! flock -w "$LOCK_WAIT" 9; then
  echo "cargo-locked: timed out after ${LOCK_WAIT}s waiting for $LOCK" >&2
  exit 124
fi

CLEANED=0
REASON="not-build-like"
if is_build_like "$SUB"; then
  STATUS=$(stamp_status)
  if [ "$STATUS" != "match" ]; then
    REASON=$STATUS
    echo "cargo-locked: provenance $STATUS -> forcing 'cargo clean -p $PKG' (project artifacts only)" >&2
    "$REAL_CARGO" clean -p "$PKG" >&2
    CLEANED=1
  else
    REASON="stamp-match"
  fi
fi

# --- run the requested cargo command, tee to a log, keep cargo's exit code ----
# Tee through a temp file OUTSIDE the target dir: a full `cargo clean` (no -p)
# deletes the target dir itself, which would destroy a log file open inside it.
# The temp file lets the log be restored after the dirs are recreated (D1).
TMP_LOG=$(mktemp "${TMPDIR:-/tmp}/rsf-cargo-log.XXXXXX")
set +e
"$REAL_CARGO" "$@" 2>&1 | tee "$TMP_LOG"
rc=${PIPESTATUS[0]}
set -e

# --- record provenance / stamp ------------------------------------------------
# A full `cargo clean` removes the target dir itself; recreate it and the log
# dir BEFORE any bookkeeping so stamp/provenance/log recording always succeeds
# and the wrapper reports cargo's own exit code, not a bookkeeping crash (D1).
mkdir -p "$TARGET_DIR" "$LOG_DIR"
mv -f "$TMP_LOG" "$LOG"
if [ "$SUB" = "clean" ]; then
  # artifacts were removed; never let a stale stamp claim they are valid
  rm -f "$STAMP"
elif is_build_like "$SUB"; then
  if [ "$rc" -eq 0 ]; then
    write_stamp
  else
    rm -f "$STAMP"   # failed build: force a rebuild next time
  fi
fi
append_provenance "$rc" "$CLEANED" "$REASON" "$LOG"

# --- optional verification receipt (per-binary counts + sha256 + source SHA) --
if [ "$rc" -eq 0 ] && [ -n "${RSF_RECEIPT_DIR:-}" ]; then
  case "$SUB" in
    test|build)
      mkdir -p "$RSF_RECEIPT_DIR"
      LABEL=${RSF_RECEIPT_LABEL:-run}
      env -u PYTHONPATH python3 "$SCRIPT_DIR/rsf_cargo_receipt.py" \
        "$LOG" "$RSF_RECEIPT_DIR/$LABEL.json" "$CHECKOUT_PATH" "$HEAD_SHA" "$DIRTY_HASH" "$SUB" \
        || echo "cargo-locked: receipt generation failed (non-fatal)" >&2
      ;;
  esac
fi

exit "$rc"