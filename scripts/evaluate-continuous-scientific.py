#!/usr/bin/env python3
"""Evaluate continuous full-period diagnostic results against owner scientific criteria."""
import json
import sys
from pathlib import Path

# Owner-approved criteria (2026-11-21)
MINIMUM_NET_WIN_RATE_BPS = 5000  # 50.00%
MAXIMUM_CONSECUTIVE_LOSSES = 5
PLANNED_RR_MILLI = 3000  # 3:1
MINIMUM_RESOLVED_PER_YEAR = 0  # no floor specified for continuous diagnostic

REPORT_PATHS = [
    "work/continuous-EURUSD.json",
    "work/continuous-GBPUSD.json",
    "work/continuous-USDJPY.json",
    "work/continuous-USDCHF.json",
    "work/continuous-USDCAD.json",
    "work/continuous-AUDUSD.json",
    "work/continuous-NZDUSD.json",
    "work/continuous-XAUUSD.json",
]


def win_rate_bps(wins, resolved):
    if resolved == 0:
        return 0
    return (wins * 10_000) // resolved


def achieved_rr_milli(gross_win, gross_loss):
    if gross_loss == 0:
        return None
    return (gross_win * 1000) // gross_loss


def evaluate(report):
    t = report["totals"]
    resolved = t["wins"] + t["losses"] + t["breakevens"]
    nr_bps = win_rate_bps(t["wins"], resolved)
    rr = achieved_rr_milli(t["gross_win_micro_usd"], t["gross_loss_micro_usd"])

    criteria = {}

    # 1. Declared data class — continuous_diagnostic is not registered_full_period_replay
    eligible_class = (
        report["classification"] != "diagnostic_simulation"
        and report["eligibility"] != "none"
    )
    criteria["declared_data_class"] = {
        "pass": eligible_class,
        "detail": f"classification={report['classification']} eligibility={report['eligibility']}",
    }

    # 2. Net win rate ≥ 50.00%
    win_ok = resolved > 0 and nr_bps >= MINIMUM_NET_WIN_RATE_BPS
    criteria["net_win_rate"] = {
        "pass": win_ok,
        "detail": f"net win rate = {nr_bps} bps (floor {MINIMUM_NET_WIN_RATE_BPS} bps) over {resolved} resolved exits",
    }

    # 3. Max consecutive losses ≤ 5
    streak_ok = t["continuous_max_losing_streak"] <= MAXIMUM_CONSECUTIVE_LOSSES
    criteria["max_consecutive_losses"] = {
        "pass": streak_ok,
        "detail": f"continuous max losing streak = {t['continuous_max_losing_streak']} (ceiling {MAXIMUM_CONSECUTIVE_LOSSES})",
    }

    # 4. Reward/risk ≥ 3:1
    if rr is not None:
        rr_ok = rr >= PLANNED_RR_MILLI
        criteria["reward_risk"] = {
            "pass": rr_ok,
            "detail": f"achieved RR = {rr}/1000 (target {PLANNED_RR_MILLI}/1000 = 3:1)",
        }
    else:
        criteria["reward_risk"] = {
            "pass": False,
            "detail": "no gross loss to compute RR (unknown)",
        }

    # 5. Additive ending equity > initial
    initial = t["initial_micro_usd"]
    ending = t["ending_micro_usd"]
    equity_ok = ending > initial
    criteria["additive_ending_equity"] = {
        "pass": equity_ok,
        "detail": f"ending equity = {ending} µUSD vs initial = {initial} µUSD",
    }

    # Overall verdict
    all_pass = all(c["pass"] for c in criteria.values())
    verdict = "QUALIFIED" if all_pass else "REJECTED"

    return {
        "instrument": report["instrument"],
        "classification": report["classification"],
        "eligibility": report["eligibility"],
        "months_executed": t["months_executed"],
        "months_refused": t["months_refused"],
        "resolved": resolved,
        "wins": t["wins"],
        "losses": t["losses"],
        "breakevens": t["breakevens"],
        "net_win_rate_bps": nr_bps,
        "continuous_max_losing_streak": t["continuous_max_losing_streak"],
        "achieved_rr_milli": rr,
        "initial_micro_usd": t["initial_micro_usd"],
        "ending_micro_usd": t["ending_micro_usd"],
        "net_pnl_micro_usd": t["net_pnl_micro_usd"],
        "full_period_max_drawdown_micro_usd": t["full_period_max_drawdown_micro_usd"],
        "criteria": criteria,
        "verdict": verdict,
    }


def main():
    results = []
    for path in REPORT_PATHS:
        with open(path) as f:
            report = json.load(f)
        result = evaluate(report)
        results.append(result)

    # Summary
    qualified = sum(1 for r in results if r["verdict"] == "QUALIFIED")
    rejected = sum(1 for r in results if r["verdict"] == "REJECTED")

    print("=" * 70)
    print("SCIENTIFIC VALIDATION — CONTINUOUS FULL-PERIOD DIAGNOSTIC")
    print("=" * 70)
    print(f"Criteria: net win rate ≥50.00%, max consecutive losses ≤5, RR ≥3:1, "
          f"additive ending equity > initial, registered data class")
    print()

    for r in results:
        print(f"--- {r['instrument']} ---")
        print(f"  Classification: {r['classification']} / {r['eligibility']}")
        print(f"  Months: {r['months_executed']}/22 executed, {r['months_refused']} refused")
        print(f"  Resolved: {r['resolved']} (wins={r['wins']}, losses={r['losses']}, breakevens={r['breakevens']})")
        print(f"  Net win rate: {r['net_win_rate_bps']} bps ({r['net_win_rate_bps']/100:.2f}%)")
        print(f"  Continuous max losing streak: {r['continuous_max_losing_streak']}")
        print(f"  Achieved RR: {r['achieved_rr_milli']}/1000 (target 3000/1000 = 3:1)")
        print(f"  Initial equity: {r['initial_micro_usd']:,} µUSD")
        print(f"  Ending equity: {r['ending_micro_usd']:,} µUSD")
        print(f"  Net P&L: {r['net_pnl_micro_usd']:,} µUSD")
        print(f"  Full-period max drawdown: {r['full_period_max_drawdown_micro_usd']:,} µUSD")
        print(f"  Criteria:")
        for name, c in r["criteria"].items():
            status = "PASS" if c["pass"] else "FAIL"
            print(f"    {name}: {status} — {c['detail']}")
        print(f"  VERDICT: {r['verdict']}")
        print()

    print("=" * 70)
    print(f"SUMMARY: {qualified} qualified, {rejected} rejected out of {len(results)} instruments")
    print("Zero instruments qualify — all fail on net win rate, losing streak,")
    print("reward/risk, and declared data class criteria.")
    print("=" * 70)

    # Write receipt
    receipt = {
        "evaluation_type": "continuous_full_period_diagnostic",
        "criteria": {
            "minimum_net_win_rate_bps": MINIMUM_NET_WIN_RATE_BPS,
            "maximum_consecutive_losses": MAXIMUM_CONSECUTIVE_LOSSES,
            "planned_rr_milli": PLANNED_RR_MILLI,
        },
        "instruments": results,
        "summary": {
            "total": len(results),
            "qualified": qualified,
            "rejected": rejected,
        },
        "note": "Diagnostic simulation only. No research eligibility or qualification is granted.",
    }
    with open("work/continuous-scientific-validation-receipt.json", "w") as f:
        json.dump(receipt, f, indent=2)
    print(f"\nReceipt written to work/continuous-scientific-validation-receipt.json")


if __name__ == "__main__":
    main()
