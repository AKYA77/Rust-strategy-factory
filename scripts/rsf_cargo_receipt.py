#!/usr/bin/env python3
"""Parse a cargo test/build log into a provenance receipt.

Records per-binary test counts, each test binary's sha256, and the source SHA /
checkout / dirty-tree hash, so any verification run can be checked against the
expected signature (e.g. main = 54 binaries / 518 passed / lib 148) and against a
foreign build's binary hashes. Layout-agnostic: binary paths are taken from the
cargo "Running ... (<path>)" lines, whatever the target-dir layout.

Usage:
  rsf_cargo_receipt.py <log> <out.json> <checkout_path> <head_sha> <dirty_hash> <subcommand>
"""
import hashlib
import json
import os
import re
import sys
import datetime

RUNNING = re.compile(r"^\s*Running\s+(.+?)\s+\((.+)\)\s*$")
RESULT = re.compile(
    r"^test result:\s*(ok|FAILED)\.\s*(\d+) passed;\s*(\d+) failed;\s*(\d+) ignored;"
    r"\s*(\d+) measured;\s*(\d+) filtered out(?:;\s*finished in ([\d.]+)s)?"
)


def sha256(path):
    try:
        h = hashlib.sha256()
        with open(path, "rb") as fh:
            for chunk in iter(lambda: fh.read(1 << 20), b""):
                h.update(chunk)
        return h.hexdigest()
    except OSError:
        return None


def main():
    log, out, checkout, head_sha, dirty_hash, sub = sys.argv[1:7]
    binaries = []
    cur = None
    with open(log, "r", errors="replace") as fh:
        for line in fh:
            m = RUNNING.match(line)
            if m:
                cur = {"name": m.group(1).strip(), "path": m.group(2).strip()}
                continue
            r = RESULT.match(line.strip())
            if r and cur is not None:
                cur.update(
                    {
                        "status": r.group(1),
                        "passed": int(r.group(2)),
                        "failed": int(r.group(3)),
                        "ignored": int(r.group(4)),
                        "filtered_out": int(r.group(6)),
                        "finished_s": float(r.group(7)) if r.group(7) else None,
                        "sha256": sha256(cur["path"]),
                    }
                )
                binaries.append(cur)
                cur = None

    total_passed = sum(b["passed"] for b in binaries)
    total_failed = sum(b["failed"] for b in binaries)
    total_ignored = sum(b["ignored"] for b in binaries)
    lib_passed = next(
        (b["passed"] for b in binaries if b["name"] == "unittests src/lib.rs"), None
    )
    receipt = {
        "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "subcommand": sub,
        "checkout_path": checkout,
        "source_sha": head_sha,
        "dirty_tree_hash": dirty_hash,
        "binary_count": len(binaries),
        "total_passed": total_passed,
        "total_failed": total_failed,
        "total_ignored": total_ignored,
        "lib_passed": lib_passed,
        "binaries": binaries,
    }
    os.makedirs(os.path.dirname(out) or ".", exist_ok=True)
    with open(out, "w") as fh:
        json.dump(receipt, fh, indent=2)
        fh.write("\n")
    print(
        f"receipt: {len(binaries)} binaries, {total_passed} passed, "
        f"{total_failed} failed, lib={lib_passed} -> {out}"
    )


if __name__ == "__main__":
    main()