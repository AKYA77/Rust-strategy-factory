#!/usr/bin/env bash
# Light mechanics suite for scripts/cargo-locked.sh — NO heavy builds: every
# command is `version`, `clean`, or a fast-failing package resolution, run
# against a THROWAWAY target + lock (never the shared CARGO_TARGET_DIR).
# Re-runnable by any lane/profile from any checkout; rsf-qa re-runs it in
# review. It needs no team heavy lock because it never touches the shared
# target and never compiles.
#
# Includes the defect-D1 regression (TEST F): a full `cargo clean` deletes the
# target dir itself; the wrapper must recreate the dirs, preserve the tee'd
# log, record provenance (command="clean", exit_code=0) and report cargo's
# rc=0 — not a bookkeeping crash. Isolation must stay intact afterwards (G).
#
# Usage: scripts/test-cargo-locked.sh    (exit 0 = all assertions pass)
set -uo pipefail

HERE=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
ROOT=$(CDPATH= cd -- "$HERE/.." && pwd -P)
cd "$ROOT"
W="$HERE/cargo-locked.sh"

T=$(mktemp -d "${TMPDIR:-/tmp}/rsf-cargo-mech.XXXXXX")
export RSF_HEAVY_LOCK="$T/lock"
export CARGO_TARGET_DIR="$T/target"
export RSF_LOCK_WAIT=10
# Respect an existing cargo/rustup environment; fall back to the VPS locations
# only when present (same rule as the wrapper, so this also runs on CI).
if [ -z "${CARGO_HOME:-}" ] && [ -d /workspace/.cargo ]; then export CARGO_HOME=/workspace/.cargo; fi
if [ -z "${RUSTUP_HOME:-}" ] && [ -d /workspace/.rustup ]; then export RUSTUP_HOME=/workspace/.rustup; fi
if [ -n "${CARGO_HOME:-}" ]; then export PATH="$CARGO_HOME/bin:$PATH"; fi
mkdir -p "$CARGO_TARGET_DIR"

STAMP="$CARGO_TARGET_DIR/.rsf-build-stamp.json"
PROV="$CARGO_TARGET_DIR/.rsf-provenance.jsonl"
CHECKOUT=$(pwd -P); HEAD=$(git rev-parse HEAD)
# Same dirty-tree content hash the wrapper computes (tracked edits + build-
# relevant untracked files), so stamp-match tests reflect the live tree.
DIRTY=$( { git diff HEAD 2>/dev/null||true; git ls-files --others --exclude-standard -z 2>/dev/null | while IFS= read -r -d '' f; do case "$f" in *.rs|*.toml|*.lock|build.rs) printf '%s\n' "$f"; sha256sum -- "$f" 2>/dev/null||true;; esac; done; } | sha256sum | awk '{print $1}')
pass=0; fail=0
ck(){ if [ "$1" = "$2" ]; then echo "  PASS: $3"; pass=$((pass+1)); else echo "  FAIL: $3 (got '$1' want '$2')"; fail=$((fail+1)); fi; }
lastprov(){ tail -1 "$PROV" | env -u PYTHONPATH python3 -c 'import sys,json;d=json.load(sys.stdin);print(d[sys.argv[1]])' "$1"; }
cleanup(){ if [ "$fail" -eq 0 ]; then rm -rf "$T"; else echo "logs kept at: $T"; fi; }
trap cleanup EXIT

echo "TEST A: non-build (version) -> lock+provenance, no stamp, no clean"
timeout 90 "$W" version >/dev/null 2>&1; rc=$?
ck "$rc" 0 "version exit 0"
ck "$(lastprov forced_clean)" False "version not forced-clean"
ck "$([ -f "$STAMP" ] && echo yes || echo no)" no "version writes no stamp"

echo "TEST B: build-like, NO stamp -> forced clean, fail invalidates"
timeout 90 "$W" check --offline -p no-such-pkg >"$T/b.log" 2>&1; rc=$?
ck "$([ "$rc" -ne 0 ] && echo nonzero || echo zero)" nonzero "bad package exits nonzero"
ck "$(lastprov forced_clean)" True "no-stamp forces clean"
ck "$(grep -c "forcing 'cargo clean -p rust-strategy-factory'" "$T/b.log")" 1 "clean message present"
ck "$([ -f "$STAMP" ] && echo yes || echo no)" no "failed build writes no stamp"

echo "TEST C: MATCHING stamp -> NO clean"
env -u PYTHONPATH python3 - "$STAMP" "$CHECKOUT" "$HEAD" "$DIRTY" <<'PY'
import json,sys
json.dump({"checkout_path":sys.argv[2],"head_sha":sys.argv[3],"dirty_hash":sys.argv[4]},open(sys.argv[1],"w"))
PY
timeout 90 "$W" check --offline -p no-such-pkg >"$T/c.log" 2>&1; rc=$?
ck "$(lastprov forced_clean)" False "matching stamp -> no clean"
ck "$(grep -c "forcing 'cargo clean" "$T/c.log")" 0 "no clean message on match"
ck "$(lastprov clean_reason)" stamp-match "clean_reason=stamp-match"

echo "TEST D: FOREIGN stamp (different checkout) -> forced clean"
env -u PYTHONPATH python3 - "$STAMP" "$HEAD" "$DIRTY" <<'PY'
import json,sys
json.dump({"checkout_path":"/some/other/checkout","head_sha":sys.argv[2],"dirty_hash":sys.argv[3]},open(sys.argv[1],"w"))
PY
timeout 90 "$W" check --offline -p no-such-pkg >"$T/d.log" 2>&1; rc=$?
ck "$(lastprov forced_clean)" True "foreign checkout forces clean"
ck "$(lastprov clean_reason)" mismatch:checkout_path "reason=mismatch:checkout_path"

echo "TEST E: stale HEAD stamp -> forced clean"
env -u PYTHONPATH python3 - "$STAMP" "$CHECKOUT" "$DIRTY" <<'PY'
import json,sys
json.dump({"checkout_path":sys.argv[2],"head_sha":"deadbeef","dirty_hash":sys.argv[3]},open(sys.argv[1],"w"))
PY
timeout 90 "$W" check --offline -p no-such-pkg >"$T/e.log" 2>&1; rc=$?
ck "$(lastprov clean_reason)" mismatch:head_sha "reason=mismatch:head_sha"

echo "TEST F (D1 regression): FULL clean -> rc=0, dirs recreated, stamp absent, provenance+log recorded"
# Seed the target like a real lane: an artifact marker + a foreign stamp.
echo marker > "$CARGO_TARGET_DIR/marker"
env -u PYTHONPATH python3 - "$STAMP" <<'PY'
import json,sys
json.dump({"checkout_path":"/foreign","head_sha":"deadbeef","dirty_hash":"x"},open(sys.argv[1],"w"))
PY
timeout 90 "$W" clean >"$T/f.log" 2>&1; rc=$?
ck "$rc" 0 "full clean exits 0 (cargo's rc, not a bookkeeping crash)"
ck "$([ -d "$CARGO_TARGET_DIR" ] && echo yes || echo no)" yes "target dir recreated after full clean"
ck "$([ -f "$STAMP" ] && echo yes || echo no)" no "full clean leaves no stamp"
ck "$(lastprov command)" clean "provenance records command=clean"
ck "$(lastprov exit_code)" 0 "provenance records exit_code=0"
FLOG=$(lastprov log)
ck "$([ -f "$FLOG" ] && echo yes || echo no)" yes "tee'd log preserved at recorded path"
ck "$([ -s "$FLOG" ] && echo yes || echo no)" yes "preserved log is non-empty"
ck "$(grep -c 'Traceback' "$T/f.log" || true)" 0 "no bookkeeping traceback"

echo "TEST F2 (control): clean -p -> rc=0, target dir survives"
timeout 90 "$W" clean -p rust-strategy-factory >"$T/f2.log" 2>&1; rc=$?
ck "$rc" 0 "clean -p exits 0"
ck "$([ -d "$CARGO_TARGET_DIR" ] && echo yes || echo no)" yes "clean -p keeps target dir"
ck "$(lastprov command)" "clean -p rust-strategy-factory" "provenance records clean -p"

echo "TEST F3: failing clean -> cargo's nonzero rc preserved, provenance recorded"
timeout 90 "$W" clean -p no-such-pkg >"$T/f3.log" 2>&1; rc=$?
ck "$([ "$rc" -ne 0 ] && echo nonzero || echo zero)" nonzero "bad clean package exits nonzero"
ck "$(lastprov exit_code)" "$rc" "provenance exit_code == cargo rc"

echo "TEST G: after full clean, next build-like -> no-stamp forced clean (isolation intact)"
timeout 90 "$W" check --offline -p no-such-pkg >"$T/g.log" 2>&1; rc=$?
ck "$(lastprov forced_clean)" True "post-clean invocation forces clean"
ck "$(lastprov clean_reason)" no-stamp "reason=no-stamp"

echo "RESULT: pass=$pass fail=$fail"
[ "$fail" -eq 0 ]
