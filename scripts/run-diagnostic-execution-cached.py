#!/usr/bin/env python3
"""Reproducible cached diagnostic full-corpus execution driver.

Runs `historical-corpus-execute-cached` over every requested instrument for the complete
verified 22-month window, twice against one caller-supplied content-addressed cache:

  * cold pass - the cache is empty, so every month is replayed from source;
  * warm pass - the same cache is reused, so every month must be a cache hit.

The two passes must produce byte-identical execution reports apart from the cache counters,
which proves that resuming from the cache does not change any financial result. The aggregate
receipt records full provenance (source SHA, binary SHA-256, input hashes, command lines,
wall-clock times) and stays explicitly non-eligible: every month record is
`diagnostic_simulation` / `none`.

This is a local performance/resumption aid and an evidence-producing acceptance run. It never
registers research, grants eligibility/approval, or feeds a scientific gate.

Usage:
  python3 scripts/run-diagnostic-execution-cached.py \
      --cache-dir <empty-or-absent-dir> --report-dir <dir> --receipt <receipt.json>
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

DEFAULT_INSTRUMENTS = [
    "EURUSD", "GBPUSD", "USDJPY", "USDCHF", "USDCAD", "AUDUSD", "NZDUSD", "XAUUSD",
]
SCRIPT_VERSION = "diagnostic-execution-cache-driver-v1"
LIMITATIONS = (
    "Diagnostic simulation only, produced by an explicitly non-eligible execution path. "
    "The authoritative sequential engine and frozen financial rules are applied to real "
    "structurally-valid literal-USD history using explicit test-convention profiles, so these "
    "are not broker-approved executions. No research eligibility, calendar/profile/licensing "
    "approval, scientific validity or qualification is granted. Non-USD-quote instruments and "
    "eligibility-bearing data classes are refused by the engine. Money figures and counts are "
    "observations, not performance evidence. The cache is a local resumption aid: a hit is only "
    "possible when corpus identity, month, chunk inventory, strategy, costs, budgets and engine "
    "version are identical."
)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run_json(command: list[str]) -> tuple[dict, float]:
    started = time.monotonic()
    completed = subprocess.run(command, capture_output=True, text=True)
    elapsed = time.monotonic() - started
    if completed.returncode != 0:
        raise RuntimeError(
            f"command failed ({completed.returncode}): {' '.join(command)}\n"
            f"stderr: {completed.stderr.strip()}"
        )
    try:
        payload = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(
            f"command did not print JSON: {' '.join(command)}\n"
            f"stdout: {completed.stdout[:2000]}\nerror: {error}"
        ) from error
    return payload, elapsed


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


def strip_cache_counters(report: dict) -> dict:
    cloned = json.loads(json.dumps(report))
    cloned.pop("cache_hits", None)
    cloned.pop("cache_misses", None)
    return cloned


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, help="release rust-strategy-factory binary")
    parser.add_argument("--repo", default=str(Path(__file__).resolve().parents[1]))
    parser.add_argument("--catalog-root", default="/var/lib/hermes-factory")
    parser.add_argument(
        "--native-output",
        default="/root/workspace/historical-migration-output/full-100k",
    )
    parser.add_argument("--instruments", default=",".join(DEFAULT_INSTRUMENTS))
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
    parser.add_argument("--receipt", required=True)
    parser.add_argument(
        "--allow-nonempty-cache",
        action="store_true",
        help="skip the cold-pass cache-empty precondition (for a deliberate warm-only run)",
    )
    parser.add_argument(
        "--prior-receipt",
        default="docs/historical-migration/DIAGNOSTIC_EXECUTION_RECEIPT.json",
        help="optional earlier receipt to cross-check identities and totals against",
    )
    args = parser.parse_args()

    repo = Path(args.repo).resolve()
    binary = Path(args.binary).resolve()
    catalog_root = Path(args.catalog_root).resolve()
    native_output = Path(args.native_output).resolve()
    strategy_path = repo / args.strategy
    costs_path = repo / args.costs
    cache_dir = Path(args.cache_dir).resolve()
    report_dir = Path(args.report_dir).resolve()
    receipt_path = Path(args.receipt).resolve()
    instruments = [item.strip() for item in args.instruments.split(",") if item.strip()]

    if not binary.is_file():
        raise SystemExit(f"binary not found: {binary}")
    if not catalog_root.is_dir():
        raise SystemExit(f"catalog root not found: {catalog_root}")
    if not native_output.is_dir():
        raise SystemExit(f"native output not found: {native_output}")
    for label, path in (("strategy", strategy_path), ("costs", costs_path)):
        if not path.is_file():
            raise SystemExit(f"{label} config not found: {path}")

    cache_entries = []
    if cache_dir.exists():
        cache_entries = [p for p in cache_dir.iterdir() if p.suffix == ".json"]
    if cache_entries and not args.allow_nonempty_cache:
        raise SystemExit(
            f"cache dir {cache_dir} already holds {len(cache_entries)} entries; "
            "the cold pass requires an empty cache (use --allow-nonempty-cache to override)"
        )

    report_dir.mkdir(parents=True, exist_ok=True)
    cache_dir.mkdir(parents=True, exist_ok=True)

    strategy = json.loads(strategy_path.read_text())
    costs = json.loads(costs_path.read_text())

    binary_sha = sha256_file(binary)
    provenance = {
        "kind": "historical-diagnostic-execution-cache-receipt-v1",
        "driver": SCRIPT_VERSION,
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
            "sha256": binary_sha,
            "size_bytes": binary.stat().st_size,
        },
        "engine_version": "ordered-l1-v6-frozen-atr-stops",
        "inputs": {
            "catalog_root": str(catalog_root),
            "native_output": str(native_output),
            "strategy": {
                "path": str(strategy_path),
                "sha256": sha256_file(strategy_path),
                "value": strategy,
            },
            "costs": {
                "path": str(costs_path),
                "sha256": sha256_file(costs_path),
                "value": costs,
            },
            "budget": {
                "max_candles": args.max_candles,
                "feature_cells": args.feature_cells,
                "max_receipts": args.max_receipts,
            },
            "expected_months": args.expected_months,
            "cache_dir": str(cache_dir),
        },
        "classification": "diagnostic_simulation",
        "eligibility": "none",
        "limitations": LIMITATIONS,
        "instruments": [],
        "totals": {},
        "verification": {},
    }

    prior = None
    if args.prior_receipt and Path(args.prior_receipt).is_file():
        prior = json.loads(Path(args.prior_receipt).read_text())

    executed_months = 0
    refused_months = 0
    trades = 0
    rejections = 0
    pnl = 0
    max_drawdown = 0
    all_cold_equal_warm = True
    all_counters_ok = True
    all_months_ok = True

    for instrument in instruments:
        command = [
            str(binary),
            "historical-corpus-execute-cached",
            str(catalog_root),
            str(native_output),
            instrument,
            str(strategy_path),
            str(costs_path),
            str(args.max_candles),
            str(args.feature_cells),
            str(args.max_receipts),
            str(cache_dir),
        ]
        cold_report_path = report_dir / f"{instrument}.cold.json"
        warm_report_path = report_dir / f"{instrument}.warm.json"
        cold_command = [*command, str(cold_report_path)]
        warm_command = [*command, str(warm_report_path)]

        cold_summary, cold_wall = run_json(cold_command)
        warm_summary, warm_wall = run_json(warm_command)

        cold_report = json.loads(cold_report_path.read_text())
        warm_report = json.loads(warm_report_path.read_text())

        months = len(cold_report["months"])
        refusals = len(cold_report["refusals"])
        # Only successful month executions are memoised. A month the engine refuses (for example
        # a non-USD quote currency) is re-read on every pass, so a warm pass reports one hit per
        # executed month and one miss per refused month.
        counters_ok = (
            cold_summary["cache_hits"] == 0
            and cold_summary["cache_misses"] == months + refusals
            and warm_summary["cache_hits"] == months
            and warm_summary["cache_misses"] == refusals
        )
        identical = strip_cache_counters(cold_report) == strip_cache_counters(warm_report)
        months_ok = months + refusals == args.expected_months
        labelled = all(
            month["classification"] == "diagnostic_simulation"
            and month["eligibility"] == "none"
            for month in cold_report["months"]
        )
        all_counters_ok = all_counters_ok and counters_ok
        all_cold_equal_warm = all_cold_equal_warm and identical
        all_months_ok = all_months_ok and months_ok
        if not (counters_ok and identical and months_ok and labelled):
            raise SystemExit(
                f"{instrument}: verification failed "
                f"(counters_ok={counters_ok} cold_equals_warm={identical} "
                f"months_ok={months_ok} labelled={labelled})"
            )

        totals = cold_report["totals"]
        executed_months += months
        refused_months += refusals
        trades += totals["trades"]
        rejections += totals["rejections"]
        pnl += totals["pnl_micro_usd"]
        max_drawdown = max(max_drawdown, totals["maximum_drawdown_micro_usd"])

        entry = {
            "instrument": instrument,
            "identity": cold_report["identity"],
            "strategy_identity": cold_report["strategy_identity"],
            "classification": cold_report["classification"],
            "eligibility": cold_report["eligibility"],
            "months_executed": months,
            "months_refused": refusals,
            "refusal_reasons": [item["reason"] for item in cold_report["refusals"]],
            "trades": totals["trades"],
            "rejections": totals["rejections"],
            "initial_micro_usd": totals["initial_micro_usd"],
            "pnl_micro_usd": totals["pnl_micro_usd"],
            "maximum_drawdown_micro_usd": totals["maximum_drawdown_micro_usd"],
            "cache_hits_cold": cold_summary["cache_hits"],
            "cache_misses_cold": cold_summary["cache_misses"],
            "cache_hits_warm": warm_summary["cache_hits"],
            "cache_misses_warm": warm_summary["cache_misses"],
            "wall_secs_cold": round(cold_wall, 3),
            "wall_secs_warm": round(warm_wall, 3),
            "reports": {
                "cold": str(cold_report_path),
                "warm": str(warm_report_path),
            },
            "cold_equals_warm": identical,
            "counters_ok": counters_ok,
            "first_month": (
                f"{cold_report['months'][0]['year']}-{cold_report['months'][0]['month']:02d}"
                if cold_report["months"]
                else None
            ),
            "last_month": (
                f"{cold_report['months'][-1]['year']}-{cold_report['months'][-1]['month']:02d}"
                if cold_report["months"]
                else None
            ),
        }
        if prior is not None:
            previous = next(
                (item for item in prior.get("instruments", []) if item["instrument"] == instrument),
                None,
            )
            if previous is not None:
                entry["prior_receipt"] = {
                    "source": str(Path(args.prior_receipt)),
                    "identity_matches": previous.get("identity") == cold_report["identity"],
                    "strategy_identity_matches": previous.get("strategy_identity")
                    == cold_report["strategy_identity"],
                    "executed_months_match": previous.get("months_executed") == months,
                    "trades_match": previous.get("trades") == totals["trades"],
                    "rejections_match": previous.get("rejections") == totals["rejections"],
                    "pnl_match": previous.get("pnl_micro_usd") == totals["pnl_micro_usd"],
                    "maximum_drawdown_match": previous.get("maximum_drawdown_micro_usd")
                    == totals["maximum_drawdown_micro_usd"],
                }
        provenance["instruments"].append(entry)
        print(
            f"{instrument}: executed={months} refused={refusals} trades={totals['trades']} "
            f"cold={cold_wall:.1f}s(hits={cold_summary['cache_hits']}) "
            f"warm={warm_wall:.1f}s(hits={warm_summary['cache_hits']} "
            f"misses={warm_summary['cache_misses']}) identical={identical}",
            flush=True,
        )

    provenance["totals"] = {
        "instruments": len(provenance["instruments"]),
        "instruments_executed": sum(1 for i in provenance["instruments"] if i["months_executed"]),
        "instruments_refused": sum(1 for i in provenance["instruments"] if not i["months_executed"]),
        "months_executed": executed_months,
        "months_refused": refused_months,
        "trades": trades,
        "rejections": rejections,
        "pnl_micro_usd": pnl,
        "maximum_drawdown_micro_usd": max_drawdown,
    }
    provenance["verification"] = {
        "cold_pass_empty_cache_required": not args.allow_nonempty_cache,
        "all_cold_equal_warm": all_cold_equal_warm,
        "all_cache_counters_exact": all_counters_ok,
        "every_instrument_has_expected_months": all_months_ok,
        "every_executed_month_labelled_diagnostic_none": True,
        "cross_checked_against_prior_receipt": prior is not None,
    }
    provenance["receipt_sha256"] = None
    receipt_path.parent.mkdir(parents=True, exist_ok=True)
    receipt_path.write_text(json.dumps(provenance, indent=2, sort_keys=False) + "\n")
    provenance["receipt_sha256"] = sha256_file(receipt_path)
    receipt_path.write_text(json.dumps(provenance, indent=2, sort_keys=False) + "\n")

    print(json.dumps(provenance["totals"], indent=2))
    print(f"receipt: {receipt_path} sha256={provenance['receipt_sha256']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
