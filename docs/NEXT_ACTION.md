# Next action

Status: SOFTWARE_INCOMPLETE. Use the existing checkout. Read AGENTS.md, PROJECT_STATE.json, IMPLEMENTATION_QUEUE.md and relevant MASTER_BLUEPRINT.md requirements. After every verified milestone update evidence/state/this file, commit, push, check remote/hosted CI, reread this file from disk and continue.

## Latest verified local milestone

Current HEAD `0ba942e` on `main` == `origin/main` == `hermes/integration-codex-stack` (all identical and pushed). Commit `0ba942e` "feat: continuous full-period execution + scientific validation + completion metrics" merges PRs #44, #45, #47 and #48, all closed as superseded with history preserved. No open PRs remain; every PR head commit is an ancestor of `origin/main`; no force-push, no branch deletion.

- Hosted CI run 45031961750 (push, main, head_sha 0ba942e) = success, completed 2026-11-22T14:16:56Z. Twin stack-branch run 45031941165 = success too. CI evidence is on the exact current SHA, not an ancestor. PROJECT_STATE.json hosted_ci records run 45031961750 conclusion=success on sha 0ba942e; tests_passed stays 619 (58 binaries, 0 failed, CI-verified suite).

- PR #44 (scientific validation over the 22-month diagnostic corpus): owner-criteria checks (>=50.00% net win rate, <=5 consecutive losses, 3:1 planned RR, >=50 resolved exits in 2025 and in 2026, $500 equity objective). 8/8 instruments, 176 executed months, 0 refused, 21,104 trades, 1,094,379 rejections, P&L -3,077,229,124 uUSD. Every instrument REJECTED (diagnostic class plus failing numerical floors); 0 qualify. Evidence: docs/historical-migration/SCIENTIFIC_VALIDATION.md and SCIENTIFIC_VALIDATION_RECEIPT.json.

- PR #45 (sparse signal-mask storage + opt-in controlled recursive acceptance): merged and closed.

- PR #47 (item-10 ledger reconciliation: scheduler/resource controls via PR #27): merged and closed.

- PR #48 (richer regimes, broader seeded exploration, behavioral diversity): merged and closed.

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