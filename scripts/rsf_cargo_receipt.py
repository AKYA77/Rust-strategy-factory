#!/usr/bin/env python3
"""rsf_cargo_receipt.py — build a verification receipt from a cargo test/build log.

Reads a cargo-locked tee log, attributes per-binary test counts to each test
artifact (via the "Running <artifact>" headers cargo prints), sha256-hashes every
artifact, and writes a JSON receipt capturing checkout path + source SHA + per
binary/artifact counts + artifact sha256.

Non-fatal on parse gaps: unhashable or uncounted artifacts are recorded with
null fields so a receipt is always produced for a successful run.

Usage:
  rsf_cargo_receipt.py <log> <out.json> <checkout_path> <head_sha> <dirty_hash> <subcommand>
"""
import sys
import json
import os
import re
import hashlib
import datetime

log_path, out_path, checkout_path, head_sha, dirty_hash, sub = sys.argv[1:7]
builder = os.environ.get("RSF_BUILDER_PROFILE") or os.environ.get("USER", "unknown")

with open(log_path, "r", errors="replace") as fh:
    text = fh.read()

artifacts = []
current = None
total_passed = 0
total_failed = 0

run_re = re.compile(r"running (\d+) tests")
result_re = re.compile(r"test result:\s*ok\.?\s*(\d+)\s*passed;\s*(\d+)\s*failed")
running_re = re.compile(r"^\s*Running\s+(.+?)\s*$", re.M)

for line in text.splitlines():
    m = running_re.match(line)
    if m:
        raw = m.group(1).strip()
        pm = re.search(r"\(([^)]+)\)", raw)
        path = pm.group(1) if pm else raw
        if path.startswith("unittests "):
            path = raw.split("(", 1)[1].rstrip(")") if "(" in raw else path
        kind = "lib" if path.endswith(".rlib") or "librust" in path else "bin"
        current = {
            "path": path,
            "kind": kind,
            "tests_run": None,
            "passed": None,
            "failed": None,
            "sha256": None,
        }
        artifacts.append(current)
        continue
    if current is not None and current["tests_run"] is None:
        m = run_re.search(line)
        if m:
            current["tests_run"] = int(m.group(1))
            continue
    if current is not None and current["passed"] is None:
        m = result_re.search(line)
        if m:
            current["passed"] = int(m.group(1))
            current["failed"] = int(m.group(2))
            total_passed += current["passed"]
            total_failed += current["failed"]
            current = None
            continue

td = os.environ.get("CARGO_TARGET_DIR")
for a in artifacts:
    candidates = [a["path"]]
    if td and not os.path.isabs(a["path"]):
        candidates.append(os.path.join(td, a["path"]))
    for c in candidates:
        if os.path.isfile(c):
            try:
                h = hashlib.sha256()
                with open(c, "rb") as fh:
                    while True:
                        chunk = fh.read(1 << 16)
                        if not chunk:
                            break
                        h.update(chunk)
                a["sha256"] = h.hexdigest()
            except OSError:
                a["sha256"] = None
            break

bins = [a for a in artifacts if a["kind"] == "bin"]
libs = [a for a in artifacts if a["kind"] == "lib"]
receipt = {
    "at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "checkout_path": checkout_path,
    "head_sha": head_sha,
    "dirty_hash": dirty_hash,
    "subcommand": sub,
    "builder": builder,
    "log": log_path,
    "totals": {
        "artifacts": len(artifacts),
        "bins": len(bins),
        "libs": len(libs),
        "passed": total_passed,
        "failed": total_failed,
    },
    "artifacts": artifacts,
}
os.makedirs(os.path.dirname(os.path.abspath(out_path)), exist_ok=True)
with open(out_path, "w") as fh:
    json.dump(receipt, fh, indent=2)
    fh.write("\n")
print("receipt: %s (artifacts=%d bins=%d libs=%d passed=%d failed=%d)"
      % (out_path, len(artifacts), len(bins), len(libs), total_passed, total_failed))
