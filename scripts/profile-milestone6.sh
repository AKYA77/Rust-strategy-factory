#!/usr/bin/env bash
# Milestone 6 baseline/parity profiler.
#
# Runs the SAME binary twice on the SAME synthetic workload with two fresh
# registries, records GNU time raw numbers, and proves exact parity of every
# non-volatile registry projection. No source file is modified; no optimization
# candidate is built, so no speedup is implied.
#
# Usage:
#   BIN=/path/to/rust-strategy-factory \
#   OUT=docs/VERIFICATION RUN_TAG=baseline STEPS=200 \
#   bash scripts/profile-milestone6.sh
set -euo pipefail

BIN=${BIN:?set BIN to an existing rust-strategy-factory binary}
OUT=${OUT:-docs/VERIFICATION}
RUN_TAG=${RUN_TAG:-baseline}
STEPS=${STEPS:-200}
WORK=${WORKDIR:-$(mktemp -d "${TMPDIR:-/tmp}/rsf-m6.XXXXXX")}
mkdir -p "$OUT" "$WORK"

test -x "$BIN" || { echo "BIN is not executable: $BIN" >&2; exit 2; }
command -v /usr/bin/time >/dev/null || { echo "/usr/bin/time (GNU time) required" >&2; exit 2; }
command -v python3 >/dev/null || { echo "python3 required" >&2; exit 2; }

P="$OUT/milestone6-research-${RUN_TAG}"
cat > "$WORK/canonical.py" <<'PY'
import json, sqlite3, sys
# Volatile columns are deliberately excluded: token/expires/at_ms are lease and
# wall-clock bookkeeping, not research results. Everything else is compared.
SPEC = {
    "experiments":       ["id", "key_json", "state", "attempt", "result_json", "result_hash"],
    "events":            ["event_id", "experiment_id", "action", "attempt"],
    "telemetry_events":  ["id", "experiment_id", "scope", "event", "payload", "decision_hash"],
    "research_steps":    ["run_id", "cursor", "experiment_id", "outcome", "reused", "error"],
    "stage_transitions": ["experiment_id", "sequence", "from_stage", "to_stage", "decision_hash"],
    "result_artifacts":  ["experiment_id", "hash", "bytes"],
    "research_runs":     ["id", "configuration", "state", "cursor", "executed",
                          "reused", "rejected", "failed", "latest", "last_error"],
}
def dump(path):
    con = sqlite3.connect(path)
    out = {}
    for table, wanted in SPEC.items():
        have = {r[1] for r in con.execute(f'PRAGMA table_info({table})')}
        cols = [c for c in wanted if c in have]
        order = "event_id" if table == "events" else "rowid"
        rows = con.execute(
            "SELECT " + ",".join(f'"{c}"' for c in cols) + f" FROM {table} ORDER BY {order}"
        ).fetchall()
        out[table] = [list(r) for r in rows]
    con.close()
    return json.dumps(out, sort_keys=True, separators=(",", ":"))
if __name__ == "__main__":
    sys.stdout.write(dump(sys.argv[1]))
PY

# ---- synthetic workload (fixed content hash, no external data) --------------
"$BIN" fixture EURUSD "$WORK/fixture.json" > "$P-fixture.out" 2>&1
"$BIN" pack-native "$WORK/fixture.json" "$WORK/input.rsf" >> "$P-fixture.out" 2>&1
python3 - "$WORK" <<'PY'
import json, sys
d = sys.argv[1]
cfg = {
  "limits": {"seed": 42, "feature_cells": 1000000, "max_total_events": 500000,
             "max_regions": 20, "lease_ms": 300000, "state_budget_bytes": 268435456,
             "improvement_children": 0, "improvement_max_depth": 0,
             "shadow_policy": None,
             "proof_policy": {"version": "fixed-market-proof-screen-v1",
                              "audit_bps": 1000, "seed": 42},
             "indexed_policy": None, "resource_guard": None, "cpu_guard": None},
  "inputs": [{"dataset": d + "/input.rsf"}]}
open(d + "/research.json", "w").write(json.dumps(cfg, indent=2) + "\n")
PY

for pass in A B; do
  db="$WORK/run-$pass.sqlite"
  /usr/bin/time -v -o "$P-$pass.time" \
    "$BIN" research "$WORK/research.json" "$db" "$STEPS" \
    > "$P-$pass.out" 2> "$P-$pass.err"
  echo "$?" > "$P-$pass.exit"
  python3 - "$db" > "$P-$pass.canonical.json" <<PY
import sys; sys.path.insert(0, "$WORK")
from canonical import dump
print(dump(sys.argv[1]))
PY
  "$BIN" recovery-list "$db" > "$P-$pass.recovery.json" 2>&1 || true
done

python3 - "$WORK" "$OUT" "$P" "$STEPS" "$BIN" <<'PY'
import hashlib, json, os, re, sqlite3, sys
work, out, prefix, steps, binary = sys.argv[1:6]
def sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""): h.update(chunk)
    return h.hexdigest()
def time_fields(path):
    raw = open(path, errors="replace").read()
    d = {"__raw": raw}
    for line in raw.splitlines():
        if "：" in line or ":" not in line: continue
        k, _, v = line.strip().rpartition(":")
        d[k.strip()] = v.strip()
    return d
def parse_time(d):
    def f(key):
        try: return float(d.get(key, "").split()[0])
        except (ValueError, IndexError): return None
    m = re.search(r"Elapsed \(wall clock\) time \(h:mm:ss or m:ss\): (\d+):(\d+\.\d+)", d.get("__raw", ""))
    if m:
        wall = int(m.group(1)) * 60 + float(m.group(2))
    else:
        wall = None
    return {"user_cpu_s": f("User time (seconds)"),
            "system_cpu_s": f("System time (seconds)"),
            "wall_s": wall,
            "max_rss_kb": f("Maximum resident set size (kbytes)"),
            "minor_faults": f("Minor (reclaiming a frame) page faults"),
            "major_faults": f("Major (requiring I/O) page faults"),
            "fs_inputs": f("File system inputs"),
            "fs_outputs": f("File system outputs"),
            "voluntary_ctx": f("Voluntary context switches"),
            "involuntary_ctx": f("Involuntary context switches")}
rec = {
  "binary_sha256": sha(binary),
  "fixture_sha256": sha(work + "/fixture.json"),
  "dataset_sha256": sha(work + "/input.rsf"),
  "config_sha256": sha(work + "/research.json"),
  "config": json.load(open(work + "/research.json")),
  "steps_requested": int(steps),
  "passes": {},
}
for p in ("A", "B"):
    txt = open(prefix + "-" + p + ".out", errors="replace").read()
    dec = json.JSONDecoder(); idx = 0; objs = []
    while idx < len(txt):
        while idx < len(txt) and txt[idx].isspace(): idx += 1
        if idx >= len(txt): break
        obj, idx = dec.raw_decode(txt, idx); objs.append(obj)
    started, final = objs[0], objs[-1]
    db = work + "/run-" + p + ".sqlite"
    con = sqlite3.connect(db)
    cnt = dict(con.execute("SELECT action, COUNT(*) FROM events GROUP BY action"))
    contra = dict(con.execute("SELECT action, COUNT(*) FROM events WHERE action LIKE '%contradiction%' GROUP BY action"))
    proof = [json.loads(r[0]) for r in con.execute("SELECT result_json FROM experiments")]
    con.close()
    sizes = {n: os.path.getsize(db + n) for n in ("", "-wal", "-shm") if os.path.exists(db + n)}
    tm = parse_time(time_fields(prefix + "-" + p + ".time"))
    denom = final["executed"] + final["reused"] + final["rejected"]
    rec["passes"][p] = {
      "exit_code": int(open(prefix + "-" + p + ".exit").read().strip()),
      "run_id": started["run_id"],
      "final_status": final,
      "time": tm,
      "unique_completed": final["executed"],
      "cache_reads": final["reused"],
      "cache_hit_rate": (final["reused"] / denom) if denom else None,
      "unique_completed_per_hour": (final["executed"] / tm["wall_s"] * 3600) if tm["wall_s"] else None,
      "event_counts": cnt,
      "screening_contradictions": contra,
      "proof_rejection_true": sum(1 for r in proof if r.get("proof_rejection") is True),
      "proof_audited_true": sum(1 for r in proof if r.get("proof_audited") is True),
      "recovery_counts": open(prefix + "-" + p + ".recovery.json", errors="replace").read().strip(),
      "registry_bytes": sizes,
      "canonical_sha256": sha(prefix + "-" + p + ".canonical.json"),
      "canonical_bytes": os.path.getsize(prefix + "-" + p + ".canonical.json"),
    }
rec["exact_parity"] = (rec["passes"]["A"]["canonical_sha256"] ==
                       rec["passes"]["B"]["canonical_sha256"] and
                       rec["passes"]["A"]["run_id"] == rec["passes"]["B"]["run_id"])
with open(prefix + "-summary.json", "w") as fh:
    json.dump(rec, fh, indent=2, sort_keys=True); fh.write("\n")
print(json.dumps({"exact_parity": rec["exact_parity"],
                  "A": rec["passes"]["A"]["time"],
                  "B": rec["passes"]["B"]["time"]}, indent=2))
PY
echo "workdir=$WORK"
echo "artifacts=$P-*.{time,out,err,exit,canonical.json,summary.json}"
