# Cached-receipt conversion-convention reconciliation (2026-11-26 continuity run)

Status: VERIFIED — evidence-label consistency only. No financial, scientific, provenance,
data-scope or security rule changed.

## Baseline and guards

- Clean `main` HEAD `d3f354eb842a822e94de652caa54305f16a2e6db`, equal to `origin/main`
  (`git rev-list --left-right --count origin/main...HEAD` = `0 0`).
- `pgrep -af 'cargo|rustc'` showed only this job's shell; `git status --porcelain` was empty;
  `flock -n /root/workspace/.rsf-heavy.lock true` acquired the heavy lock. No worktree used.
- A first ladder attempt wrapped `check.sh` in an outer `flock`; because `check.sh` internally
  calls `cargo-locked.sh`, which acquires the same lock, this self-deadlocked. The process tree
  was killed by explicit PID, the lock was confirmed free, and `check.sh` was re-run directly
  (it self-locks). No source file was edited while a ladder was in flight.

## Already-pushed commits verified (`3b789b2..HEAD`)

| SHA | Subject | Scope |
| --- | --- | --- |
| `34aae4b859c99767272fde63046ddb97e74ad6cd` | Record terminal hosted CI for bid-only reconciliation commit | ledgers: +4 lines recording run 45734055499 |
| `be6553e4173933cb7c08c0ebd7e673ab933b5dfa` | Fix doc/code contradictions after bid-only conversion reconciliation | `PROJECT_STATE.json` scope correction + 16 cached-report `limitations` strings |
| `8a8e41c9ed914e292b7e736f56804a99b4293c1f` | Rename direction_based test to bid_based convention | `tests/core.rs`: `usd_account_reciprocal_conversion_is_exact_and_direction_based` -> `..._bid_based` |
| `d3f354eb842a822e94de652caa54305f16a2e6db` | Update diagnostic cache conversion_policy to bid-only convention | 6 non-USD cached reports: `conversion_policy` label only |

Full changed-line audit of `git diff 3b789b2..HEAD`: 138 `conversion_policy` label lines, 16
`limitations` text lines, 1 test-function rename, 4 CI-record ledger lines, 2 blank lines. No
numeric field, `receipt_hash`, trade/rejection count, P&L, classification, eligibility, gate or
financial/scientific assertion changed.

## Why relabelling preserves the recorded numbers

- The cached reports were generated at `869d224`/`f0e5c05` (2026-11-21) under `413df32`, whose
  `diagnostic_conversion` selected `Side::Long => tick.bid`, `Side::Short => tick.ask`.
- The cached execution config `configs/historical-migration/strategies/m1-long-above-mean-v1.json`
  is `"side": "Long"`. With no Short fills, both the old direction-based rule and the active
  bid-only rule (post-`617f215`) apply the identical bid rate to every recorded fill.
- Hence the per-month numbers and `receipt_hash` values stay valid under the new label;
  `d3f354e` changed no numeric content. The label now matches the active code constant
  `CONVERSION_POLICY_RECIPROCAL = diagnostic_reciprocal_closing_rate_v1_bid_only`.
- `SCIENTIFIC_VALIDATION_RECEIPT.json` retains the old `long_bid_short_ask` label by design: it
  is the historical validation receipt produced before the relabel, and the ledger already
  states that the policy-string change invalidates numerical comparison across the two strings.

## Verification

- Baseline `scripts/check.sh` (`RSF_BUILDER_PROFILE=hermes-v2-m1`): EXIT=0, 760 passed / 0 failed
  across 71 binaries (lib 276), zero named failures, fmt and clippy clean.
- RED: checked out `3b789b2` versions of the six non-USD cached reports and ran
  `tests/cached_receipt_convention.rs`:
  `cached_diagnostic_receipt_labels_match_active_convention` FAILED (EXIT=101;
  `USDJPY.cold.json` left `diagnostic_reciprocal_closing_rate_v1_long_bid_short_ask`, right
  `diagnostic_reciprocal_closing_rate_v1_bid_only`); 2 passed / 1 failed.
- GREEN: restored the six files byte-for-byte from HEAD; 3 passed / 0 failed (EXIT=0). The
  working tree then contained only the new untracked regression test.
- Final-tree `scripts/check.sh`: EXIT=0, 763 passed / 0 failed across 72 binaries (lib 276),
  including the new 3-test binary; zero named failures; fmt and clippy clean.
- Hosted CI terminal conclusions: 45736386703 (`34aae4b`), 45740888354 (`be6553e`),
  45741014575 (`8a8e41c`), 45741284289 (`d3f354e`) — each `completed`/`success` on all 7 jobs
  (release-build, browser, rust-checks, data-compute-identity, full-horizon, check-operations,
  dependency-audit). No in-progress run was called a pass.

## Classification

Diagnostic evidence only. All cached reports remain `diagnostic_simulation` / `none`; the corpus
class stays REAL_STRUCTURALLY_VALID with research eligibility none. No scientific qualification,
broker/profile/calendar/licensing approval, or security/data-scope rule changed. V2 software
milestones 1–6 remain evidenced; milestone 7 is still NOT accepted because of its two owner-only
external gates (saved dashboard credentials and an owner manual reboot). No further software
milestone is unachieved.
