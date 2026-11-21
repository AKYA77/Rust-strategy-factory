#!/usr/bin/env bash
# scripts/repro-cargo-isolation.sh — re-runnable cross-worktree cargo isolation repro.
#
# Builds a FOREIGN checkout into the shared CARGO_TARGET_DIR via scripts/cargo-locked.sh,
# then verifies the MAIN checkout through the SAME wrapper and asserts NO artifact reuse:
#   - the main phase is forced to run `cargo clean -p rust-strategy-factory`
#     (the stamp still holds the foreign checkout -> mismatch),
#   - main's artifact sha256 is DISTINCT from the foreign build's,
#   - main reports its OWN signature (counts), not the foreign one.
#
# This proves the shared target can never silently serve one checkout's artifacts
# to another checkout's verification.
#
# Usage:
#   MAIN_CHECKOUT=/root/workspace/rsf-wt-rust scripts/repro-cargo-isolation.sh [FOREIGN_SHA]
#
# Env:
#   MAIN_CHECKOUT     checkout to verify as "main" (default: this script's repo root)
#   FOREIGN_SHA       commit to build as foreign (default: hermes/non-usd-conversion head,
#                     a branch carrying distinct Rust source; override with the first arg)
#   RSF_RECEIPT_DIR   receipt output dir (default /root/workspace/rsf-audit)
#   RSF_BUILDER_PROFILE, RSF_LOCK_WAIT
set -euo pipefail

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd -P)
WRAPPER="$SCRIPT_DIR/cargo-locked.sh"
RECEIPT_DIR=${RSF_RECEIPT_DIR:-/root/workspace/rsf-audit}
export RSF_BUILDER_PROFILE=${RSF_BUILDER_PROFILE:-rsf-qa-repro}
export RSF_LOCK_WAIT=${RSF_LOCK_WAIT:-600}
export RSF_RECEIPT_DIR="$RECEIPT_DIR"
mkdir -p "$RECEIPT_DIR"

MAIN_CHECKOUT=${MAIN_CHECKOUT:-"$ROOT"}
MAIN_CHECKOUT=$(CDPATH= cd -- "$MAIN_CHECKOUT" && pwd -P)
main_sha=$(git -C "$MAIN_CHECKOUT" rev-parse HEAD)

FOREIGN_SHA=${1:-}
if [ -z "$FOREIGN_SHA" ]; then
  FOREIGN_SHA=$(git -C "$MAIN_CHECKOUT" rev-parse --verify hermes/non-usd-conversion 2>/dev/null \
                || git -C "$MAIN_CHECKOUT" rev-parse --verify origin/hermes/cargo-target-isolation 2>/dev/null \
                || true)
fi
[ -n "$FOREIGN_SHA" ] || { echo "repro: no foreign SHA available (pass one as arg)" >&2; exit 1; }
[ "$FOREIGN_SHA" != "$main_sha" ] || { echo "repro: foreign SHA equals main SHA" >&2; exit 1; }

RUN_DIR="$RECEIPT_DIR/cargo-isolation-repro-$(date -u +%Y%m%dT%H%M%SZ)-$$"
mkdir -p "$RUN_DIR"
foreign_wt=$(mktemp -d "${TMPDIR:-/tmp}/rsf-repro-foreign-wt.XXXXXX")
cleanup(){ git worktree remove --force "$foreign_wt" 2>/dev/null || true; git -C "$MAIN_CHECKOUT" worktree prune 2>/dev/null || true; }
trap cleanup EXIT

echo "repro: main      = $MAIN_CHECKOUT @ $main_sha"
echo "repro: foreign   = $FOREIGN_SHA"
echo "repro: run dir   = $RUN_DIR"

git -C "$MAIN_CHECKOUT" worktree add --detach "$foreign_wt" "$FOREIGN_SHA" >/dev/null 2>&1 \
  || { echo "repro: failed to create foreign worktree at $FOREIGN_SHA" >&2; exit 1; }

fail=0

# ================= FOREIGN phase =================
export RSF_RECEIPT_LABEL=foreign
export RSF_CHECKOUT_DIR="$foreign_wt"
echo "repro: building FOREIGN checkout into shared target..."
set +e
"$WRAPPER" test --locked --lib --bins --tests >"$RUN_DIR/foreign-cargo-test.log" 2>&1
foreign_rc=$?
set -e
if [ "$foreign_rc" -ne 0 ]; then
  echo "repro: FOREIGN build/test failed rc=$foreign_rc (log: $RUN_DIR/foreign-cargo-test.log)" >&2
  fail=1
fi
cp "${CARGO_TARGET_DIR:-$ROOT/target}/.rsf-build-stamp.json" "$RUN_DIR/stamp-after-foreign.json" 2>/dev/null || true
cp "$RECEIPT_DIR/foreign.json" "$RUN_DIR/foreign.json" 2>/dev/null || true

# ================= MAIN phase =================
unset RSF_CHECKOUT_DIR
export RSF_RECEIPT_LABEL=main
echo "repro: verifying MAIN checkout THROUGH the wrapper (expect forced clean -p)..."
set +e
"$WRAPPER" test --locked --lib --bins --tests >"$RUN_DIR/main-cargo-test.log" 2>&1
main_rc=$?
set -e
if [ "$main_rc" -ne 0 ]; then
  echo "repro: MAIN build/test failed rc=$main_rc (log: $RUN_DIR/main-cargo-test.log)" >&2
  fail=1
fi
cp "${CARGO_TARGET_DIR:-$ROOT/target}/.rsf-build-stamp.json" "$RUN_DIR/stamp-after-main.json" 2>/dev/null || true
cp "$RECEIPT_DIR/main.json" "$RUN_DIR/main.json" 2>/dev/null || true

# ================= VERDICT =================
VERDICT_JSON="$RUN_DIR/verdict.json"
VERDICT_MD="$RUN_DIR/verdict.md"
CHECKS_TSV="$RUN_DIR/checks.tsv"

# Provenance log lives in the SHARED target dir (CARGO_TARGET_DIR), not under the
# worktree (the worktree has no target/ of its own).
PROV_LOG="${CARGO_TARGET_DIR:-$ROOT/target}/.rsf-provenance.jsonl"
main_forced_clean="?"; main_clean_reason="?"
if [ -f "$PROV_LOG" ]; then
  read -r main_forced_clean main_clean_reason < <(env -u PYTHONPATH python3 - "$PROV_LOG" <<'PY'
import json,sys
recs=[json.loads(l) for l in open(sys.argv[1]) if l.strip()]
r=recs[-1] if recs else {}
print(r.get("forced_clean","?"), r.get("clean_reason","?"))
PY
)
fi

foreign_json="$RUN_DIR/foreign.json"; main_json="$RUN_DIR/main.json"
[ -f "$foreign_json" ] || foreign_json=""
[ -f "$main_json" ] || main_json=""

set +e
env -u PYTHONPATH python3 - "$VERDICT_JSON" "$VERDICT_MD" "$CHECKS_TSV" "$FOREIGN_SHA" "$main_sha" \
  "$foreign_json" "$main_json" "$PROV_LOG" \
  "$main_forced_clean" "$main_clean_reason" "$foreign_rc" "$main_rc" "$fail" <<'PY'
import json, sys, datetime, os
(out, verdict_md, checks_tsv, fsha, msha, fj, mj, prov,
 fc, rc_clean, frc, mrc, fail) = sys.argv[1:]
def load(p):
    return json.load(open(p)) if p and os.path.isfile(p) else None
fd = load(fj); md = load(mj)
def tot(d):
    t = (d or {}).get("totals") or {}
    return {"bins": t.get("bins"), "passed": t.get("passed"), "failed": t.get("failed"), "libs": t.get("libs")}
ft = tot(fd); mt = tot(md)
def lib_sha(d):
    for a in (d or {}).get("artifacts") or []:
        p = a.get("path") or ""
        if a.get("kind") == "lib" or p.endswith(".rlib"):
            return a.get("sha256")
    return None
fsha256 = lib_sha(fd); msha256 = lib_sha(md)
def byname(d):
    out = {}
    for a in (d or {}).get("artifacts") or []:
        p = a.get("path") or ""
        if p:
            out[os.path.basename(p)] = a.get("sha256")
    return out
fbn = byname(fd); mbn = byname(md)
shared = sorted(set(fbn) & set(mbn))
distinct = [b for b in shared if fbn.get(b) and mbn.get(b) and fbn.get(b) != mbn.get(b)]
sha_distinct_all = len(shared) > 0 and len(distinct) == len(shared)

frc_zero = (str(frc) == "0"); mrc_zero = (str(mrc) == "0")
fc_true = (fc == "True")
fail_int = int(str(fail)) if str(fail).isdigit() else 1
checks = [
  ("main phase forced clean -p (foreign stamp detected)", fc_true, "forced_clean=%s reason=%s" % (fc, rc_clean)),
  ("foreign source SHA recorded", bool(fsha), fsha),
  ("main source SHA recorded", bool(msha), msha),
  ("foreign build green (rc=0)", frc_zero, "rc=%s" % frc),
  ("main build green (rc=0)", mrc_zero, "rc=%s" % mrc),
  ("main binary_count recorded", mt["bins"] is not None, str(mt["bins"])),
  ("main total_passed recorded", mt["passed"] is not None, str(mt["passed"])),
  ("main total_failed recorded", mt["failed"] is not None, str(mt["failed"])),
  ("all shared-name binaries have distinct sha256", sha_distinct_all, "%d/%d distinct" % (len(distinct), len(shared))),
]
verdict = "PASS" if (fail_int == 0 and all(c[1] for c in checks) and frc_zero and mrc_zero) else "FAIL"
rec = {
  "at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
  "verdict": verdict,
  "PASS": verdict == "PASS",
  "foreign_ref_sha": fsha,
  "main_sha": msha,
  "foreign": {"binary_count": ft["bins"], "total_passed": ft["passed"], "total_failed": ft["failed"], "lib_passed": ft["libs"], "lib_sha256": fsha256},
  "main": {"binary_count": mt["bins"], "total_passed": mt["passed"], "total_failed": mt["failed"], "lib_passed": mt["libs"], "lib_sha256": msha256},
  "foreign_binaries": fbn,
  "main_binaries": mbn,
  "checks": [{"check": n, "ok": ok, "detail": d} for n, ok, d in checks],
}
json.dump(rec, open(out,"w"), indent=2); open(out,"a").write("\n")
with open(checks_tsv, "w") as fh:
    for n, ok, d in checks:
        fh.write("%s\t%s\t%s\n" % (n, "true" if ok else "false", d))
lines = []
lines.append("# Cross-worktree cargo isolation repro verdict")
lines.append("")
lines.append("- foreign SHA: `%s`  (%s binaries / %s passed / %s failed / lib %s)" % (fsha, ft["bins"], ft["passed"], ft["failed"], ft["libs"]))
lines.append("- main SHA:    `%s`  (%s binaries / %s passed / %s failed / lib %s)" % (msha, mt["bins"], mt["passed"], mt["failed"], mt["libs"]))
lines.append("- foreign lib sha256: `%s`" % fsha256)
lines.append("- main lib sha256:    `%s`" % msha256)
lines.append("")
lines.append("| check | ok | detail |")
lines.append("|---|---|---|")
for n, ok, d in checks:
    lines.append("| %s | %s | %s |" % (n, "PASS" if ok else "FAIL", d))
lines.append("")
lines.append("**VERDICT: %s**" % verdict)
open(verdict_md, "w").write("\n".join(lines) + "\n")
print("=== VERDICT: %s ===" % verdict)
sys.exit(0 if verdict == "PASS" else 1)
PY
rc=$?
set -e
echo "receipts + logs in: $RUN_DIR"
exit $rc
