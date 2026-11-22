#!/usr/bin/env python3
"""Scientific validation driver over the verified 22-month corpus.

Runs the cached diagnostic full-corpus execution (cold + warm) through the unchanged
authoritative sequential engine, then evaluates the owner-approved criteria per instrument with
the `scientific-validation` CLI and records a provenance-bound receipt.

Everything this driver produces is diagnostic: classification `diagnostic_simulation`,
eligibility `none`, `qualification_granted: false`. It never registers an experiment, feeds a
scientific gate, grants eligibility, or qualifies a strategy. The diagnostic execution resets the
account each month, so the criteria summary is explicitly reset-fold and cannot establish the
continuous single-account full-period accounting the owner criteria require.

Usage:
  python3 scripts/run-scientific-validation.py \
      --binary target/release/rust-strategy-factory \
      --cache-dir work/scientific-validation-cache-v1 \
      --report-dir docs/historical-migration/diagnostic-cache-v1 \
      --diagnostic-receipt docs/historical-migration/DIAGNOSTIC_EXECUTION_CACHED_RECEIPT.json \
      --validation-receipt docs/historical-migration/SCIENTIFIC_VALIDATION_RECEIPT.json
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

DEFAULT_INSTRUMENTS = [
    "EURUSD", "GBPUSD", "USDJPY", "USDCHF", "USDCAD", "AUDUSD", "NZDUSD", "XAUUSD",
]
DRIVER_VERSION = "scientific-validation-driver-v1"
LIMITATIONS = (
    "Diagnostic simulation only. The authoritative sequential engine and frozen financial rules "
    "are applied to real structurally-valid history, and each month starts a fresh account, so "
    "the criteria summary is reset-fold and descriptive. No research eligibility, "
    "calendar/profile/licensing approval, scientific validity or qualification is granted, and "
    "no strategy is qualified. Synthetic or diagnostic evidence can never qualify."
)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git_provenance(repo: Path) -> dict:
    def git(*args: str) -> str:
        return subprocess.run(
            ["git", "-C", str(repo), *args], capture_output=True, text=True, check=True
        ).stdout.strip()

    return {
        "head": git("rev-parse", "HEAD"),
        "head_short": git("rev-parse", "--short", "HEAD"),
        "branch": git("rev-parse", "--abbrev-ref", "HEAD"),
        "dirty_paths": git("status", "--porcelain").splitlines(),
    }


def run_json(command: list[str]) -> dict:
    started = time.monotonic()
    completed = subprocess.run(command, capture_output=True, text=True)
    elapsed = time.monotonic() - started
    if completed.returncode != 0:
        raise RuntimeError(
            f"command failed ({completed.returncode}): {' '.join(command)}\n"
            f"stderr: {completed.stderr.strip()}"
        )
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(
            f"command did not print JSON: {' '.join(command)}\n"
            f"stdout: {completed.stdout[:2000]}\nerror: {error}"
        ) from error


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, help="release rust-strategy-factory binary")
    parser.add_argument("--repo", default=str(Path(__file__).resolve().parents[1]))
    parser.add_argument("--catalog-root", default="/var/lib/hermes-factory")
    parser.add_argument(
        "--native-output",
        default="/root/workspace/historical-migration-output/full-100k",
    )
    parser.add_argument(
        "--instruments", default=",".join(DEFAULT_INSTRUMENTS),
    )
    parser.add_argument(
        "--strategy",
        default="configs/historical-migration/strategies/m1-long-above-mean-v1.json",
    )
    parser.add_argument(
        "--costs",
        default="configs/historical-migration/costs/bid-anchored-floor-v1-default.json",
    )
    parser.add_argument("--max-candles", type=int, default=1_000_000)
    parser.add_argument("--feature-cells", type=int, default=1_000_000)
    parser.add_argument("--max-receipts", type=int, default=1_000_000)
    parser.add_argument("--expected-months", type=int, default=22)
    parser.add_argument("--cache-dir", required=True)
    parser.add_argument("--report-dir", required=True)
    parser.add_argument("--diagnostic-receipt", required=True)
    parser.add_argument("--validation-receipt", required=True)
    parser.add_argument(
        "--prior-receipt",
        default="docs/historical-migration/DIAGNOSTIC_EXECUTION_RECEIPT.json",
        help="earlier cached receipt to cross-check identities and totals against",
    )
    args = parser.parse_args()

    repo = Path(args.repo).resolve()
    binary = Path(args.binary).resolve()
    catalog_root = Path(args.catalog_root).resolve()
    native_output = Path(args.native_output).resolve()
    cache_dir = Path(args.cache_dir).resolve()
    report_dir = Path(args.report_dir).resolve()
    diagnostic_receipt = Path(args.diagnostic_receipt).resolve()
    validation_receipt = Path(args.validation_receipt).resolve()
    prior_receipt = Path(args.prior_receipt).resolve()
    instruments = [item.strip() for item in args.instruments.split(",") if item.strip()]

    if not binary.is_file():
        raise SystemExit(f"binary not found: {binary}")
    if not catalog_root.is_dir():
        raise SystemExit(f"catalog root not found: {catalog_root}")
    if not native_output.is_dir():
        raise SystemExit(f"native output not found: {native_output}")
    strategy_path = repo / args.strategy
    costs_path = repo / args.costs
    for label, path in (("strategy", strategy_path), ("costs", costs_path)):
        if not path.is_file():
            raise SystemExit(f"{label} config not found: {path}")

    # 1. Refresh the diagnostic full-corpus execution (cold + warm) with the current engine.
    subprocess.run(
        [
            sys.executable,
            str(repo / "scripts" / "run-diagnostic-execution-cached.py"),
            "--binary",
            str(binary),
            "--repo",
            str(repo),
            "--catalog-root",
            str(catalog_root),
            "--native-output",
            str(native_output),
            "--instruments",
            ",".join(instruments),
            "--strategy",
            args.strategy,
            "--costs",
            args.costs,
            "--max-candles",
            str(args.max_candles),
            "--feature-cells",
            str(args.feature_cells),
            "--max-receipts",
            str(args.max_receipts),
            "--expected-months",
            str(args.expected_months),
            "--cache-dir",
            str(cache_dir),
            "--report-dir",
            str(report_dir),
            "--receipt",
            str(diagnostic_receipt),
            "--prior-receipt",
            str(prior_receipt),
        ],
        check=True,
    )

    # 2. Evaluate the owner criteria over the fresh cold reports.
    reports = [report_dir / f"{instrument}.cold.json" for instrument in instruments]
    missing = [str(path) for path in reports if not path.is_file()]
    if missing:
        raise SystemExit(f"diagnostic reports missing after execution: {missing}")
    validation = run_json([str(binary), "scientific-validation", *map(str, reports)])

    receipt = {
        "kind": "scientific-validation-receipt-v1",
        "driver": DRIVER_VERSION,
        "generated_at_utc": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "host": {
            "node": platform.node(),
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "cpus": os.cpu_count(),
        },
        "source": git_provenance(repo),
        "binary": {
            "path": str(binary),
            "sha256": sha256_file(binary),
            "size_bytes": binary.stat().st_size,
        },
        "inputs": {
            "catalog_root": str(catalog_root),
            "native_output": str(native_output),
            "strategy": {
                "path": str(strategy_path),
                "sha256": sha256_file(strategy_path),
            },
            "costs": {
                "path": str(costs_path),
                "sha256": sha256_file(costs_path),
            },
            "budget": {
                "max_candles": args.max_candles,
                "feature_cells": args.feature_cells,
                "max_receipts": args.max_receipts,
            },
            "expected_months": args.expected_months,
            "cache_dir": str(cache_dir),
            "report_dir": str(report_dir),
            "diagnostic_receipt": {
                "path": str(diagnostic_receipt),
                "sha256": sha256_file(diagnostic_receipt),
            },
        },
        "classification": "diagnostic_simulation",
        "eligibility": "none",
        "qualification_granted": False,
        "limitations": LIMITATIONS,
        "validation": validation,
    }
    receipt["receipt_sha256"] = None
    validation_receipt.parent.mkdir(parents=True, exist_ok=True)
    validation_receipt.write_text(json.dumps(receipt, indent=2, sort_keys=False) + "\n")
    receipt["receipt_sha256"] = sha256_file(validation_receipt)
    validation_receipt.write_text(json.dumps(receipt, indent=2, sort_keys=False) + "\n")

    print(json.dumps(validation["totals"], indent=2))
    for item in validation["instruments"]:
        print(
            f"{item['instrument']:8} resolved={item['resolved']:5} "
            f"win_rate={item['net_win_rate_bps']}bps streak={item['max_losing_streak']} "
            f"status={item['criteria']['status']}"
        )
    print(f"validation receipt: {validation_receipt} sha256={receipt['receipt_sha256']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
