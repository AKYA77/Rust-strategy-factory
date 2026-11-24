# Next action

Status: SOFTWARE_INCOMPLETE. Use the existing checkout. Read AGENTS.md, PROJECT_STATE.json, IMPLEMENTATION_QUEUE.md and relevant MASTER_BLUEPRINT.md requirements. After every verified milestone update evidence/state/this file, commit, push, check remote/hosted CI, reread this file from disk and continue.

## Latest verified local milestone

Current HEAD `ba66956` on `main` == `origin/main` (pushed). Merge commit `ba66956` "Merge pull request #52 from AKYA77/wt/registered-full-period" merges PR #52 (registered full-period real-data executor: execute_continuous + historical-corpus-execute-continuous CLI; classification continuous_diagnostic, eligibility none). PR #52 head 0cb5d5a66cccba8308ff2f0262d469d0f17b5eb4 is an ancestor of origin/main (containment proven via git merge-base --is-ancestor); no force-push, no branch deletion; PR #52 closed as merged with history preserved. One open PR remains: #51 (continuous diagnostic accounting hygiene: breakeven streak, dead term, unused constant).

- Hosted CI run 45339910079 (push, main, head_sha ba66956) = success on all 7 jobs: release-build, data-compute-identity, check-operations, full-horizon, rust-checks, browser, dependency-audit. CI evidence is on the exact current SHA, not an ancestor. PROJECT_STATE.json hosted_ci records run 45339910079 conclusion=success on sha ba66956; tests_passed stays 619 (CI-verified suite).

- PR #52 (registered full-period real-data executor over the 22-month corpus): execute_continuous replays each registered month through execute_month, chains each month's ending marked equity into the next month's starting equity, and reports compounded full-period equity, cross-month continuous losing streak and full-period peak-to-trough max drawdown. All 8 instruments execute (176/176 months, 0 refused); diagnostic only, never registers or qualifies. Evidence: tests/registered_full_period.rs.

- Prior milestone (historical, superseded): HEAD 0ba942e merged PRs #44, #45, #47, #48 via commit 0ba942e; hosted CI run 45031961750 success; those PRs closed as superseded with history preserved.

## Implement next

Next steps are external-evidence gated, not implementation:

(a) Owner manual reboot for reboot-persistence evidence. `systemctl reboot` is on the agent's unconditional blocklist — the owner must reboot the VPS manually to complete target-VPS acceptance. Reboot persistence is verified by enabled units + active user linger (Linger=yes); no disruptive reboot executed by the agent. Receipt: docs/VERIFICATION/vps-live-acceptance-2026-11-21.json.

(b) External-evidence gates: real-data acquisition disabled by owner — 22-month local corpus (2025-01..2026-10, 176 partitions, 421,078,937 events) is the final dataset, no Dukascopy/HistData/FXOpen/TrueFX retries; broker/session/profile/calendar conversion facts absent (broker dependency removed — deterministic broker-independent execution-cost model; market calendar policy approved — all valid available data incl. Sunday sessions; scientific criteria owner-approved; legal local-only — REAL_STRUCTURALLY_VALID is not eligibility). 0 instruments research-eligible — a numerical result on diagnostic evidence, not an external gate.

Continue the remaining software queue — full end-to-end acceptance with hardware/resource evidence, resumable sources/imports/coverage acceptance — without weakening financial, provenance, scientific or security safeguards. The converted native corpus is REAL_STRUCTURALLY_VALID only and grants no calendar completeness, profile approval, source licensing, scientific eligibility or qualification.

## Owner decisions (final, 2026-11-21)

* Broker dependency: REMOVED. Deterministic broker-independent execution-cost model: fixed 2.0 pip spread, fixed 2.0 pip slippage per side, zero commission, no broker-specific swap/financing, no broker API/account/credentials/connectivity. Never add commission to the $0-commission research scenario.
* External data acquisition: DISABLED. 22-month local corpus (2025-01..2026-10, 176 partitions, 421M events) is the final dataset. No Dukascopy/HistData/FXOpen/TrueFX retries.
* Market calendar policy: APPROVED. Trade when instrument market is open; include Sunday evening FX sessions and Friday sessions before close; respect gold-market breaks; handle holidays/DST with UTC-normalized timestamps; never invent candles/ticks/prices during closed sessions.
* Scientific protocol: owner-approved. >=50.00% verified net win rate per instrument; <=5 consecutive losses; 3:1 RR target; keep 5 best qualified strategies per instrument; preserve OOS testing, leakage prevention, robustness checks. Synthetic never qualifies.
* Legal review: local corpus provenance documented; no redistribution; no invented license approval.
* Target VPS acceptance: continue testing against existing 4-vCPU/8-GB environment — production builds, local dataset loading, systemd service behavior, bounded memory/CPU, safe restart/recovery. Reboot persistence half requires owner manual reboot (agent blocklist).

## Invariants

Exact executable bid/ask, 2-pip spread floor plus separate 2-pip adverse market/stop slippage, zero commission, 10% intended risk/$20 initial/3R/24h; frozen pending distance/quantity; no fabricated gaps/fills/depth; no protected OOS/final adaptation; synthetic never qualifies. Qualification needs independent approved complete evidence, not a high test count or tiny benchmark.