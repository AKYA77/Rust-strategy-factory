#!/usr/bin/env bash
# Independently re-runnable cross-worktree contamination repro for the locked cargo
# wrapper (card t_f368fccb). Proves, against the SHARED CARGO_TARGET_DIR:
#
#   Phase 1 (foreign): a throwaway detached worktree on a DISTINCT commit builds its
#                      artifacts into the shared target through the wrapper; its
#                      per-binary counts + binary sha256 + source SHA are recorded.
#   Phase 2 (main):    a main-source verification THROUGH the wrapper (the wired
#                      scripts/check.sh = fmt + clippy + full cargo test) detects the
#                      foreign provenance stamp, forces `cargo clean -p
#                      rust-strategy-factory` inside the lock, rebuilds, and reports
#                      main's signature with binary sha256 DISTINCT from phase 1.
#
# The forced clean + distinct hashes + restored main signature are the evidence that
# cross-checkout artifact reuse is impossible. rsf-qa re-runs this in review.
#
# Usage:
#   scripts/repro-cargo-isolation.sh
# Env overrides:
#   FOREIGN_REF      distinct commit for the foreign worktree. Default 8b01418 (an
#                    ancestor of main, identical Cargo.lock, minimal real-data tests
#                    -> fast foreign phase). Any distinct reachable commit works; set
#                    it to a dashboard head to reproduce that specific incident.
#   MAIN_CHECKOUT    checkout carrying main's source AND this wrapper. Default the
#                    main checkout. Pre-merge, point it at the branch carrying the
#                    change (e.g. /root/workspace/rsf-wt-rust).
#   RSF_AUDIT_DIR    receipt root (default /root/workspace/rsf-audit).
#   EXPECT_BINARIES / EXPECT_PASSED / EXPECT_LIB  main signature (default 54/518/148
#                    at ee5d08a). Override if main's signature has legitimately changed.
set -euo pipefail

HERE=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
WRAPPER="$HERE/cargo-locked.sh"
FOREIGN_REF=${FOREIGN_REF:-8b01418}
MAIN_CHECKOUT=${MAIN_CHECKOUT:-/root/workspace/Rust-strategy-factory}
AUDIT=${RSF_AUDIT_DIR:-/root/workspace/rsf-audit}
EXPECT_BINARIES=${EXPECT_BINARIES:-54}
EXPECT_PASSED=${EXPECT_PASSED:-518}
EXPECT_LIB=${EXPECT_LIB:-148}

export CARGO_HOME="${CARGO_HOME:-/workspace/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-/workspace/.rustup}"
export PATH="$CARGO_HOME/bin:$PATH"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
export RSF_LOCK_WAIT="${RSF_LOCK_WAIT:-10800}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/root/workspace/Rust-strategy-factory/target}"
export RSF_BUILDER_PROFILE="${RSF_BUILDER_PROFILE:-rsf-rust-repro}"
STAMP="$CARGO_TARGET_DIR/.rsf-build-stamp.json"

RUN="$AUDIT/cargo-isolation-repro-$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$RUN"
echo "[repro] run dir:        $RUN"
echo "[repro] foreign ref:    $FOREIGN_REF"
echo "[repro] main checkout:  $MAIN_CHECKOUT"
echo "[repro] shared target:  $CARGO_TARGET_DIR"
echo "[repro] expect main:    $EXPECT_BINARIES binaries / $EXPECT_PASSED passed / lib $EXPECT_LIB"

FOREIGN_WT="$AUDIT/repro-foreign-wt-$(date -u +%s)-$$"
cleanup() { git -C "$MAIN_CHECKOUT" worktree remove --force "$FOREIGN_WT" >/dev/null 2>&1 || true; \
            git -C "$MAIN_CHECKOUT" worktree prune >/dev/null 2>&1 || true; }
trap cleanup EXIT

# --- Phase 1: foreign detached worktree, full test through the wrapper ---------
echo "[repro] phase 1: building FOREIGN artifacts ($FOREIGN_REF) into the shared target"
git -C "$MAIN_CHECKOUT" worktree add --detach "$FOREIGN_WT" "$FOREIGN_REF"
FOREIGN_SHA=$(git -C "$FOREIGN_WT" rev-parse HEAD)
echo "[repro] foreign worktree: $FOREIGN_WT @ $FOREIGN_SHA"
RSF_CHECKOUT_DIR="$FOREIGN_WT" RSF_RECEIPT_DIR="$RUN" RSF_RECEIPT_LABEL=foreign \
  "$WRAPPER" test --locked --lib --bins --tests -j 4 2>&1 | tee "$RUN/foreign-cargo-test.log"
cp "$STAMP" "$RUN/stamp-after-foreign.json"
echo "[repro] phase 1 done; foreign stamp captured"

# --- Phase 2: main-source verification through the wrapper (wired check.sh) ----
echo "[repro] phase 2: main-source verification THROUGH the wrapper (check.sh)"
MAIN_SHA=$(git -C "$MAIN_CHECKOUT" rev-parse HEAD)
(
  cd "$MAIN_CHECKOUT"
  unset RSF_CHECKOUT_DIR
  export RSF_RECEIPT_DIR="$RUN" RSF_RECEIPT_LABEL=main
  sh scripts/check.sh
) 2>&1 | tee "$RUN/main-check-sh.log"
cp "$STAMP" "$RUN/stamp-after-main.json"
echo "[repro] phase 2 done; main stamp captured"

# --- Phase 3: assertions + verdict --------------------------------------------
env -u PYTHONPATH python3 - "$RUN" "$FOREIGN_WT" "$FOREIGN_SHA" "$MAIN_CHECKOUT" "$MAIN_SHA" \
    "$EXPECT_BINARIES" "$EXPECT_PASSED" "$EXPECT_LIB" <<'PY'
import json, sys, os
run, fwt, fsha, mcheckout, msha, eb, ep, el = sys.argv[1:9]
eb, ep, el = int(eb), int(ep), int(el)
foreign = json.load(open(os.path.join(run, "foreign.json")))
main = json.load(open(os.path.join(run, "main.json")))
sf = json.load(open(os.path.join(run, "stamp-after-foreign.json")))
sm = json.load(open(os.path.join(run, "stamp-after-main.json")))
main_log = open(os.path.join(run, "main-check-sh.log"), errors="replace").read()

def lib_sha(rec):
    for b in rec["binaries"]:
        if b["name"] == "unittests src/lib.rs":
            return b.get("sha256")
    return None

checks = []
def ck(name, ok, detail=""):
    checks.append({"check": name, "ok": bool(ok), "detail": detail})

forced_clean = ("forcing 'cargo clean -p rust-strategy-factory'" in main_log)
ck("main phase forced clean -p (foreign stamp detected)", forced_clean)
ck("foreign stamp records foreign checkout", sf.get("checkout_path") == fwt,
   f"{sf.get('checkout_path')} vs {fwt}")
ck("main stamp records main checkout", sm.get("checkout_path") == mcheckout,
   f"{sm.get('checkout_path')} vs {mcheckout}")
ck("foreign source SHA recorded", foreign.get("source_sha") == fsha, fsha)
ck("main source SHA recorded", main.get("source_sha") == msha, msha)
ck(f"main binary_count == {eb}", main["binary_count"] == eb, str(main["binary_count"]))
ck(f"main total_passed == {ep}", main["total_passed"] == ep, str(main["total_passed"]))
ck(f"main lib_passed == {el}", main["lib_passed"] == el, str(main["lib_passed"]))
ck("main total_failed == 0", main["total_failed"] == 0, str(main["total_failed"]))
fl, ml = lib_sha(foreign), lib_sha(main)
ck("main lib sha256 DISTINCT from foreign lib sha256", bool(fl and ml and fl != ml),
   f"foreign={fl} main={ml}")
# every binary present in both must differ (source differs across the two commits)
fmap = {b["name"]: b.get("sha256") for b in foreign["binaries"]}
shared = [b for b in main["binaries"] if b["name"] in fmap]
diff = [b["name"] for b in shared if fmap[b["name"]] and b.get("sha256") and fmap[b["name"]] != b["sha256"]]
ck("all shared-name binaries have distinct sha256", len(diff) == len(shared),
   f"{len(diff)}/{len(shared)} distinct")

verdict = {
    "foreign_ref_sha": fsha, "main_sha": msha,
    "foreign": {k: foreign[k] for k in ("binary_count","total_passed","total_failed","lib_passed")},
    "main": {k: main[k] for k in ("binary_count","total_passed","total_failed","lib_passed")},
    "foreign_lib_sha256": fl, "main_lib_sha256": ml,
    "checks": checks,
    "PASS": all(c["ok"] for c in checks),
}
json.dump(verdict, open(os.path.join(run, "verdict.json"), "w"), indent=2)
lines = ["# Cross-worktree cargo isolation repro verdict", "",
         f"- foreign SHA: `{fsha}`  ({foreign['binary_count']} binaries / {foreign['total_passed']} passed / lib {foreign['lib_passed']})",
         f"- main SHA:    `{msha}`  ({main['binary_count']} binaries / {main['total_passed']} passed / lib {main['lib_passed']})",
         f"- foreign lib sha256: `{fl}`", f"- main lib sha256:    `{ml}`", "",
         "| check | ok | detail |", "|---|---|---|"]
for c in checks:
    lines.append(f"| {c['check']} | {'PASS' if c['ok'] else 'FAIL'} | {c['detail']} |")
lines += ["", f"**VERDICT: {'PASS' if verdict['PASS'] else 'FAIL'}**"]
open(os.path.join(run, "verdict.md"), "w").write("\n".join(lines) + "\n")
print("\n".join(lines))
sys.exit(0 if verdict["PASS"] else 1)
PY

echo "[repro] receipts + verdict in $RUN"