# Deployed VPS service state — read-only observation (2026-11-27 continuity run)

Scope: read-only inspection of the already-deployed user services on this VPS. No
service was started, stopped, restarted, reconfigured or redeployed; no repository
source, financial rule, scientific gate, provenance rule or data scope was changed.
This is an evidence correction, not Milestone 7 acceptance.

## Observed state (commands and exact outputs)

Baseline: clean `main` HEAD `8c9a6fd6aec0fd318da4bf071679c284c6b430a5` ==
`origin/main`, no other cargo/rustc worker, heavy lock acquired.

| Command | Observed |
|---|---|
| `systemctl --user list-units --type=service --all --no-pager \| grep -i 'rust-strategy\|rsf'` | `rust-strategy-factory-dashboard.service loaded inactive dead`; `rust-strategy-factory-research.service loaded activating auto-restart` |
| `systemctl --user show rust-strategy-factory-research.service -p ActiveState -p SubState -p NRestarts -p UnitFileState` | `NRestarts=704 ActiveState=activating SubState=auto-restart UnitFileState=enabled` |
| `systemctl --user show rust-strategy-factory-dashboard.service -p ActiveState -p SubState -p NRestarts -p UnitFileState` | `NRestarts=32266 ActiveState=inactive SubState=dead UnitFileState=enabled` |
| `journalctl --user -u rust-strategy-factory-research.service -n 40` | Repeated `error: non-USD quote instrument USDJPY requires an explicit approved event-aligned conversion file`; `Main process exited, code=exited, status=1/FAILURE`; `RestartSec=310`; last sampled restart counter 704 at 2026-11-27 05:17:32 CET |
| `journalctl --user -u rust-strategy-factory-dashboard.service -n 40` | Long restart loop on `Address already in use (os error 98)`; last log line 2026-11-24 16:51:19 CET `{"listening_port":3000,"loopback_only":true}` (restart counter 32266); unit currently dead. Whether a listener is still held outside the unit was not probed. |
| `python3` read of `~/.local/share/rust-strategy-factory/config/research.json` | 140 inputs = 20 months x 7 instruments; includes USDJPY/USDCHF/USDCAD; 0 inputs carry a conversion field or path |
| `browser_vault` `list` | `{"items": []}` — no saved logins; no authenticated dashboard endpoint exercised |
| `loginctl show-user root -p Linger` | `Linger=yes` (unit enablement/linger is not a reboot proof) |
| Unit file read | `ExecStart=.../current/rust-strategy-factory research .../config/research.json .../state/research.sqlite`; `Restart=on-failure`; `RestartSec=310`; `current -> releases/4d8ecc88ad78960f2d2e6dacee9803a14b0a1773-head` |

## Correction of a false ledger claim

Commit `8c9a6fd` recorded "both RSF units `active (running)` with `NRestarts=0`".
That probe used unit names `rsf-research.service` / `rsf-dashboard.service`, which are
not installed; the deployed units are `rust-strategy-factory-research.service` and
`rust-strategy-factory-dashboard.service`. Their actual state is above. The
"zero crash-restarts" claim is therefore not supported by the deployed services and
is corrected here; local `scripts/check.sh` and hosted CI green do not imply a healthy
deployed service.

## Root cause and owner-only boundary

The research process exits immediately because the installed 7-instrument, 20-month
config supplies non-USD-quote instruments (USDJPY among them) with no explicit
event-aligned conversion file. The registered replay gate deliberately refuses that
combination. Remedies that would weaken the gate — an identity (1/1) USD rate for a
non-USD quote, an inferred rate in the registered path, or relabelling a diagnostic
conversion as approved — are prohibited and were NOT applied. The local corpus and
existing exact conversion functions can produce diagnostic evidence (as in
`tests/real_conversion_evidence.rs`), but "approved" conversion authority and any
deployed config change / service cutover require explicit owner action.

## Evidence conflict recorded, not resolved

`docs/VERIFICATION/milestone7-vps-acceptance.md` §7 item 4 still lists a fresh owner
reboot as open, while the skill, `docs/historical-migration/PROGRESS.md` and
`docs/PROJECT_STATE.json` record an owner-executed reboot on 2026-11-22 with all
services auto-recovered. This pass did not reboot, did not re-run the M7 evidence
ladder, and does not close or reopen that gate; it only records the conflict.

## Verdict / non-claims

- No code fix, no service fix, no config edit, no production cutover.
- No new qualification/eligibility: class remains `REAL_STRUCTURALLY_VALID`,
  eligibility none; 0 qualified strategies.
- No CI job still `in_progress` was called a pass.
- V2 software milestones 1-6 remain evidenced; Milestone 7 remains NOT accepted.
