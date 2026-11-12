#!/usr/bin/env python3
"""Development-only Linux resource wrapper. Run after compiling, in a fresh process."""
import hashlib
import json
from pathlib import Path
import os
import subprocess
import sys
import time

if len(sys.argv) != 4:
    sys.exit("usage: python3 scripts/measure.py <benchmark-executable> <new-result.json> <new-resources.json>")
executable, output, resources = sys.argv[1:]
root = Path(__file__).resolve().parent.parent
code = hashlib.sha256()
for path in sorted([root / "Cargo.toml", root / "Cargo.lock", *root.glob("src/**/*.rs"), *root.glob("benches/**/*.rs")]):
    code.update(str(path.relative_to(root)).encode() + b"\0" + path.read_bytes())
start = time.monotonic()
with open(output, "x") as target:
    process = subprocess.Popen([str(Path(executable).resolve())], stdout=target)
    _, status, usage = os.wait4(process.pid, 0)
    process.returncode = os.waitstatus_to_exitcode(status)
elapsed = time.monotonic() - start
report = {
    "exit_code": process.returncode,
    "source_inputs_sha256": code.hexdigest(),
    "executable_sha256": hashlib.sha256(Path(executable).read_bytes()).hexdigest(),
    "wall_seconds_whole_benchmark": elapsed,
    "user_cpu_seconds": usage.ru_utime,
    "system_cpu_seconds": usage.ru_stime,
    "peak_rss_kib_linux": usage.ru_maxrss,
    "filesystem_input_blocks": usage.ru_inblock,
    "filesystem_output_blocks": usage.ru_oublock,
    "limitations": "Synthetic warm resident fixture baseline. CPU/RSS/I/O cover the whole benchmark process, not individual worker trials. No decode/compression measured. Source hash covers Cargo manifests, src and benches."
}
with open(resources, "x") as target:
    json.dump(report, target, indent=2)
    target.write("\n")
sys.exit(process.returncode)
