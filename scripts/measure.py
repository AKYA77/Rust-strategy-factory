#!/usr/bin/env python3
"""Development-only Linux resource wrapper. Run after compiling, in a fresh process."""
import hashlib
from datetime import datetime, timezone
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
source_manifest = []
for path in sorted([root / "build.rs", root / "rust-toolchain.toml", root / "Cargo.toml", root / "Cargo.lock", *root.glob("src/**/*.rs"), *root.glob("benches/**/*.rs")]):
    contents = path.read_bytes()
    relative = str(path.relative_to(root))
    code.update(relative.encode() + b"\0" + contents)
    source_manifest.append({"path": relative, "sha256": hashlib.sha256(contents).hexdigest()})
def read(path):
    try:
        return Path(path).read_text().strip()
    except OSError:
        return "unavailable"
cpu_lines = read("/proc/cpuinfo").splitlines()
compiler = subprocess.check_output(["rustc", "-Vv"], cwd=root, text=True).strip()
revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=root))
start = time.monotonic()
with open(output, "x") as target:
    process = subprocess.Popen([str(Path(executable).resolve())], stdout=target)
    _, status, usage = os.wait4(process.pid, 0)
    process.returncode = os.waitstatus_to_exitcode(status)
elapsed = time.monotonic() - start
report = {
    "exit_code": process.returncode,
    "recorded_at_utc": datetime.now(timezone.utc).isoformat(),
    "source_fingerprint_version": 2,
    "source_manifest": source_manifest,
    "git_base_revision": revision,
    "source_dirty_at_measurement": dirty,
    "measurement_script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    "rustc_vv_probe": compiler,
    "cpu_model": next((line.split(":", 1)[1].strip() for line in cpu_lines if line.startswith("model name")), "unavailable"),
    "cpu_features": next((line.split(":", 1)[1].strip() for line in cpu_lines if line.startswith("flags") or line.startswith("Features")), "unavailable"),
    "affinity_cpu_count": len(os.sched_getaffinity(0)),
    "cgroup_cpu_max": read("/sys/fs/cgroup/cpu.max"),
    "cgroup_memory_max": read("/sys/fs/cgroup/memory.max"),
    "host_mem_available": next((line for line in read("/proc/meminfo").splitlines() if line.startswith("MemAvailable:")), "unavailable"),
    "source_inputs_sha256": code.hexdigest(),
    "executable_sha256": hashlib.sha256(Path(executable).read_bytes()).hexdigest(),
    "wall_seconds_whole_benchmark": elapsed,
    "user_cpu_seconds": usage.ru_utime,
    "system_cpu_seconds": usage.ru_stime,
    "peak_rss_kib_linux": usage.ru_maxrss,
    "filesystem_input_blocks": usage.ru_inblock,
    "filesystem_output_blocks": usage.ru_oublock,
    "limitations": "Read the benchmark result class: component masks and synthetic replays are different workloads. CPU/RSS/I/O cover this entire child process, excluding compilation, not individual trials. The child report defines which storage/decoding/replay operations were measured; this wrapper supplies no real/full-history acceptance. Source fingerprint v2 covers build.rs, toolchain, Cargo manifests, src and benches; per-file digests are authoritative when the checkout is dirty. The rustc probe describes this measurement environment; compiled compute identity is in the result."
}
with open(resources, "x") as target:
    json.dump(report, target, indent=2)
    target.write("\n")
sys.exit(process.returncode)
